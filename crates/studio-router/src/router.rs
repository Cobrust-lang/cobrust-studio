//! Router — strategy + dispatch + retry.
//!
//! Lifted from `cobrust-llm-router` @ `61f2aff` (v0.1.1) per ADR-0005 /
//! ADR-0006. Strips applied:
//!
//! - #1 — `Strategy::Consensus { n }` removed (no multi-provider voting,
//!   no `consensus_pick`, no `ConsensusQuorumLost` error variant).
//! - #3 — `Task` enum + per-task routing tables collapsed into a single
//!   global dispatch flow driven by [`crate::config::RouterSection`].
//! - #6 — `RouterResponse` renamed to [`DispatchResponse`].
//!
//! `Strategy` itself now lives in [`crate::config`] and is re-exported from
//! this module to satisfy ADR-0006's public-API surface contract.

// Upstream copyright: The Cobrust Project. Licensed under Apache-2.0 OR MIT.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use rand::Rng;
use tokio::sync::Mutex as AsyncMutex;
use tokio::time::Instant;

use crate::cache::{Cache, CacheKey};
use crate::config::{ProviderModel, RouterConfig};
use crate::ledger::{Ledger, LedgerEntry, now_rfc3339};
use crate::provider::{CompletionRequest, CompletionResponse, LlmError, LlmProvider};

// Re-export so ADR-0006's `pub use router::Strategy` survives even though
// the type lives in `config`.
pub use crate::config::Strategy;

/// Successful dispatch result. Renamed from upstream `RouterResponse` per
/// strip #6 (ADR-0006); Cobrust-task-tag variants dropped.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DispatchResponse {
    pub response: CompletionResponse,
    pub provider: String,
    pub cache_hit: bool,
}

/// Router-level errors. `LlmError`s from individual provider attempts are
/// rolled into `AllFailed` once the preferred list is exhausted.
#[derive(Debug, thiserror::Error)]
pub enum RouterError {
    #[error("config: {0}")]
    Config(String),
    #[error("no provider configured for dispatch")]
    NoProvider,
    #[error("all providers failed: {0:?}")]
    AllFailed(Vec<(String, LlmError)>),
    #[error("io: {0}")]
    Io(String),
}

impl From<std::io::Error> for RouterError {
    fn from(e: std::io::Error) -> Self {
        RouterError::Io(e.to_string())
    }
}

/// Retry budget: 5 attempts, 30 s elapsed cap, 250 ms base, factor 2, full
/// jitter, honour `Retry-After`. Carried from upstream `adr:0004`.
#[derive(Copy, Clone, Debug)]
pub struct RetryPolicy {
    pub max_attempts: u8,
    pub base_delay_ms: u64,
    pub factor: f64,
    pub max_total_ms: u64,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 5,
            base_delay_ms: 250,
            factor: 2.0,
            max_total_ms: 30_000,
        }
    }
}

impl RetryPolicy {
    /// Compute the next sleep duration for `attempt` (1-indexed). When the
    /// error carries a `Retry-After`, that value overrides the computed delay.
    fn next_delay_ms(&self, attempt: u8, err: &LlmError) -> u64 {
        if let LlmError::RateLimit { retry_after_ms } = err
            && *retry_after_ms > 0
        {
            return *retry_after_ms;
        }
        let exp = attempt.saturating_sub(1);
        let pow = self.factor.powi(i32::from(exp));
        #[allow(
            clippy::cast_precision_loss,
            clippy::cast_possible_truncation,
            clippy::cast_sign_loss
        )]
        let max = (self.base_delay_ms as f64) * pow;
        if max <= 0.0 {
            return 0;
        }
        // Full-jitter: uniform [0, max].
        let mut rng = rand::thread_rng();
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let delay = rng.gen_range(0.0..=max) as u64;
        delay
    }
}

/// In-memory EWMA latency tracker for `Strategy::Latency`. Keys are
/// `provider:model` tags.
#[derive(Default, Debug)]
struct LatencyTracker {
    inner: HashMap<String, f64>,
}

impl LatencyTracker {
    const ALPHA: f64 = 0.2;

    fn observe(&mut self, key: &str, latency_ms: u64) {
        #[allow(clippy::cast_precision_loss)]
        let v = latency_ms as f64;
        let entry = self.inner.entry(key.to_string()).or_insert(v);
        *entry = Self::ALPHA.mul_add(v, (1.0 - Self::ALPHA) * *entry);
    }

    fn get(&self, key: &str) -> Option<f64> {
        self.inner.get(key).copied()
    }
}

/// Router. Holds the provider registry, preferred-model order, cache,
/// ledger, and retry policy. Strip #3 collapsed the per-task routing map to
/// a single global preferred list.
pub struct Router {
    providers: HashMap<String, Arc<dyn LlmProvider>>,
    preferred: Vec<ProviderModel>,
    strategy: Strategy,
    cache: Cache,
    ledger: Ledger,
    retry: RetryPolicy,
    latency: Arc<AsyncMutex<LatencyTracker>>,
}

impl std::fmt::Debug for Router {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Router")
            .field("providers", &self.providers.keys().collect::<Vec<_>>())
            .field("preferred", &self.preferred)
            .field("strategy", &self.strategy)
            .finish_non_exhaustive()
    }
}

/// Builder for the router. Concrete adapters or test doubles are registered
/// via [`RouterBuilder::register_provider`]; the dispatch list is fixed by
/// the parsed [`RouterConfig`].
#[derive(Default)]
pub struct RouterBuilder {
    providers: HashMap<String, Arc<dyn LlmProvider>>,
    retry: Option<RetryPolicy>,
}

impl RouterBuilder {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a concrete provider under the given key. The key must match a
    /// `[providers.<key>]` section in the config.
    #[must_use]
    pub fn register_provider(
        mut self,
        key: impl Into<String>,
        provider: Arc<dyn LlmProvider>,
    ) -> Self {
        self.providers.insert(key.into(), provider);
        self
    }

    /// Override the default retry policy.
    #[must_use]
    pub fn retry_policy(mut self, retry: RetryPolicy) -> Self {
        self.retry = Some(retry);
        self
    }

    /// Build the router from the resolved config + registered providers.
    ///
    /// # Errors
    /// Returns [`RouterError::Config`] if the config fails to validate or
    /// references unregistered providers, or [`RouterError::Io`] if the
    /// cache/ledger paths cannot be opened.
    pub async fn build(self, cfg: &RouterConfig) -> Result<Router, RouterError> {
        cfg.validate().map_err(RouterError::Config)?;
        for name in cfg.providers.keys() {
            if !self.providers.contains_key(name) {
                return Err(RouterError::Config(format!(
                    "provider {name:?} declared in config but not registered with the builder"
                )));
            }
        }
        let mut preferred = Vec::with_capacity(cfg.router.preferred.len());
        for tag in &cfg.router.preferred {
            let pm = ProviderModel::parse(tag).ok_or_else(|| {
                RouterError::Config(format!(
                    "router.preferred: malformed provider:model tag {tag:?}"
                ))
            })?;
            if !self.providers.contains_key(&pm.provider) {
                return Err(RouterError::Config(format!(
                    "router.preferred: provider {:?} referenced by {tag:?} is not registered",
                    pm.provider
                )));
            }
            preferred.push(pm);
        }
        let cache = Cache::new(cfg.router.cache_dir.clone()).await?;
        let ledger = Ledger::open(cfg.router.ledger_path.clone()).await?;
        Ok(Router {
            providers: self.providers,
            preferred,
            strategy: cfg.router.strategy,
            cache,
            ledger,
            retry: self.retry.unwrap_or_default(),
            latency: Arc::new(AsyncMutex::new(LatencyTracker::default())),
        })
    }
}

impl Router {
    /// Convenience: build directly from config; assumes providers are
    /// registered via [`RouterBuilder`].
    #[must_use]
    pub fn builder() -> RouterBuilder {
        RouterBuilder::new()
    }

    /// Optional caller-supplied tag persisted in each ledger entry. Defaults
    /// to `None` when callers do not set one. ADR-0006 strip #4 generalised
    /// the upstream `Task` enum to this single free-form field; pass
    /// `Some("agent-turn")` (or any caller-meaningful label) when the
    /// downstream consumer wants to correlate ledger lines.
    fn task_tag_for_request(_req: &CompletionRequest) -> Option<String> {
        // Future: thread caller-supplied tag through `CompletionRequest`
        // metadata. Today the dispatch contract is task-tag-less, matching
        // ADR-0006 §"M1 dispatch contract".
        None
    }

    /// Dispatch one request. Honours the configured strategy, retries
    /// transient errors per the retry policy, falls through to the next
    /// preferred provider on permanent failure, writes one ledger entry per
    /// attempt, and reads/writes the on-disk cache.
    ///
    /// # Errors
    /// See [`RouterError`] variants.
    pub async fn dispatch(&self, req: CompletionRequest) -> Result<DispatchResponse, RouterError> {
        if self.preferred.is_empty() {
            return Err(RouterError::NoProvider);
        }
        let order = self.order_preferred().await;
        let tag = Self::task_tag_for_request(&req);
        self.dispatch_ordered(tag, &req, &order).await
    }

    /// Dispatch one request with a caller-supplied `task_tag` that is
    /// persisted into each ledger entry written during this dispatch.
    ///
    /// Identical semantics to [`Self::dispatch`] otherwise — same cache,
    /// strategy, retry, and provider-fallthrough behaviour. Wave A5
    /// reconcile added this overload so `studio-server`'s
    /// `POST /api/dispatch` can thread `DispatchContext::task_tag`
    /// (ADR-0006 §F-03) into the ledger without going through the
    /// `CompletionRequest` body (which is keyed by the cache and so
    /// cannot carry mutable metadata).
    ///
    /// # Errors
    /// See [`RouterError`] variants.
    pub async fn dispatch_with_tag(
        &self,
        task_tag: Option<String>,
        req: CompletionRequest,
    ) -> Result<DispatchResponse, RouterError> {
        if self.preferred.is_empty() {
            return Err(RouterError::NoProvider);
        }
        let order = self.order_preferred().await;
        self.dispatch_ordered(task_tag, &req, &order).await
    }

    async fn order_preferred(&self) -> Vec<ProviderModel> {
        match self.strategy {
            Strategy::Latency => {
                // Sort indices into self.preferred by EWMA latency, then
                // clone once per element at return. Prior implementation
                // built a Vec<(f64, ProviderModel)> + sorted + materialized,
                // doubling the per-element allocation. Persona-audit catch
                // (Aleksandr's PR #1 — M5 cycle).
                let tracker = self.latency.lock().await;
                let mut indexed: Vec<(f64, usize)> = self
                    .preferred
                    .iter()
                    .enumerate()
                    .map(|(idx, pm)| {
                        let key = format!("{}:{}", pm.provider, pm.model);
                        let latency = tracker.get(&key).unwrap_or(f64::INFINITY);
                        (latency, idx)
                    })
                    .collect();
                drop(tracker);
                indexed
                    .sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
                indexed
                    .into_iter()
                    .map(|(_, idx)| self.preferred[idx].clone())
                    .collect()
            }
            // Quality and Cost walk the table in submitted order.
            Strategy::Quality | Strategy::Cost => self.preferred.clone(),
        }
    }

    async fn dispatch_ordered(
        &self,
        task_tag: Option<String>,
        req: &CompletionRequest,
        order: &[ProviderModel],
    ) -> Result<DispatchResponse, RouterError> {
        let mut errors: Vec<(String, LlmError)> = Vec::new();
        for pm in order {
            match self.try_provider(task_tag.clone(), req, pm).await {
                Ok(resp) => return Ok(resp),
                Err(err) => {
                    errors.push((pm.provider.clone(), err));
                }
            }
        }
        Err(RouterError::AllFailed(errors))
    }

    fn handle(&self) -> RouterHandle {
        RouterHandle {
            providers: self.providers.clone(),
            cache: self.cache.clone(),
            ledger: self.ledger.clone(),
            retry: self.retry,
            latency: self.latency.clone(),
        }
    }

    async fn try_provider(
        &self,
        task_tag: Option<String>,
        req: &CompletionRequest,
        pm: &ProviderModel,
    ) -> Result<DispatchResponse, LlmError> {
        self.handle().try_provider(task_tag, req, pm).await
    }
}

/// Lightweight cloneable handle (kept for symmetry with the upstream API even
/// though Studio no longer spawns parallel shards — strip #1).
#[derive(Clone)]
struct RouterHandle {
    providers: HashMap<String, Arc<dyn LlmProvider>>,
    cache: Cache,
    ledger: Ledger,
    retry: RetryPolicy,
    latency: Arc<AsyncMutex<LatencyTracker>>,
}

impl RouterHandle {
    #[allow(clippy::too_many_lines)]
    async fn try_provider(
        &self,
        task_tag: Option<String>,
        req: &CompletionRequest,
        pm: &ProviderModel,
    ) -> Result<DispatchResponse, LlmError> {
        let provider = self
            .providers
            .get(&pm.provider)
            .ok_or_else(|| LlmError::Provider {
                code: "unknown_provider".into(),
                message: format!("provider {} not registered", pm.provider),
            })?;

        // Enforce the model from the preferred list; the caller's `req.model`
        // may differ.
        let mut request = req.clone();
        request.model = pm.model.clone();

        let key = CacheKey::compute(&pm.provider, &request);
        // Cache lookup.
        if let Some(resp) = self
            .cache
            .get(&key)
            .await
            .map_err(|e| LlmError::Decode(e.to_string()))?
        {
            self.ledger
                .append(&LedgerEntry::ok(
                    now_rfc3339(),
                    task_tag.clone(),
                    pm.provider.clone(),
                    Some(provider.kind()),
                    pm.model.clone(),
                    key.wire(),
                    true,
                    resp.usage,
                    0,
                    1,
                ))
                .await
                .map_err(|e| LlmError::Decode(e.to_string()))?;
            return Ok(DispatchResponse {
                response: resp,
                provider: pm.provider.clone(),
                cache_hit: true,
            });
        }

        // Live dispatch with retry on transient errors.
        let mut attempt: u8 = 1;
        let total_start = Instant::now();
        loop {
            let call_start = Instant::now();
            let outcome = provider.complete(request.clone()).await;
            let elapsed_ms = u32::try_from(call_start.elapsed().as_millis()).unwrap_or(u32::MAX);
            match outcome {
                Ok(resp) => {
                    self.cache
                        .put(&key, &request, &resp)
                        .await
                        .map_err(|e| LlmError::Decode(e.to_string()))?;
                    {
                        let latency_key = format!("{}:{}", pm.provider, pm.model);
                        let mut tracker = self.latency.lock().await;
                        tracker.observe(&latency_key, u64::from(elapsed_ms));
                    }
                    self.ledger
                        .append(&LedgerEntry::ok(
                            now_rfc3339(),
                            task_tag.clone(),
                            pm.provider.clone(),
                            Some(provider.kind()),
                            pm.model.clone(),
                            key.wire(),
                            false,
                            resp.usage,
                            elapsed_ms,
                            attempt,
                        ))
                        .await
                        .map_err(|e| LlmError::Decode(e.to_string()))?;
                    return Ok(DispatchResponse {
                        response: resp,
                        provider: pm.provider.clone(),
                        cache_hit: false,
                    });
                }
                Err(err) => {
                    let transient = err.is_transient();
                    self.ledger
                        .append(&LedgerEntry::err(
                            now_rfc3339(),
                            task_tag.clone(),
                            pm.provider.clone(),
                            Some(provider.kind()),
                            pm.model.clone(),
                            key.wire(),
                            elapsed_ms,
                            attempt,
                            err.code(),
                            transient,
                        ))
                        .await
                        .map_err(|e| LlmError::Decode(e.to_string()))?;

                    if !transient || attempt >= self.retry.max_attempts {
                        return Err(err);
                    }
                    let total_elapsed_ms =
                        u64::try_from(total_start.elapsed().as_millis()).unwrap_or(u64::MAX);
                    if total_elapsed_ms >= self.retry.max_total_ms {
                        return Err(err);
                    }
                    let delay = self
                        .retry
                        .next_delay_ms(attempt, &err)
                        .min(self.retry.max_total_ms.saturating_sub(total_elapsed_ms));
                    if delay > 0 {
                        tokio::time::sleep(Duration::from_millis(delay)).await;
                    }
                    attempt = attempt.saturating_add(1);
                }
            }
        }
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]
mod tests {
    use super::*;

    #[test]
    fn retry_policy_honours_retry_after() {
        let p = RetryPolicy::default();
        let err = LlmError::RateLimit {
            retry_after_ms: 7777,
        };
        assert_eq!(p.next_delay_ms(1, &err), 7777);
    }

    #[test]
    fn retry_policy_jitter_within_bound() {
        let p = RetryPolicy::default();
        let err = LlmError::Server {
            status: 503,
            body: String::new(),
        };
        for attempt in 1..=4 {
            let exp = i32::from(attempt - 1);
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let upper = (p.base_delay_ms as f64 * p.factor.powi(exp)) as u64;
            for _ in 0..50 {
                let d = p.next_delay_ms(attempt, &err);
                assert!(d <= upper, "delay {d} should be <= {upper}");
            }
        }
    }

    #[test]
    fn latency_tracker_ewma_converges() {
        let mut t = LatencyTracker::default();
        let key = "p:m";
        for _ in 0..50 {
            t.observe(key, 100);
        }
        let v = t.get(key).expect("must exist");
        assert!((v - 100.0).abs() < 0.5, "EWMA should converge: {v}");
    }

    #[test]
    fn strategy_exhaustive_match_has_no_consensus() {
        // If a future change adds `Strategy::Consensus`, this match becomes
        // non-exhaustive and the build fails — strip #1 stays enforced.
        fn label(s: Strategy) -> &'static str {
            match s {
                Strategy::Cost => "cost",
                Strategy::Quality => "quality",
                Strategy::Latency => "latency",
            }
        }
        assert_eq!(label(Strategy::Cost), "cost");
        assert_eq!(label(Strategy::Quality), "quality");
        assert_eq!(label(Strategy::Latency), "latency");
    }
}
