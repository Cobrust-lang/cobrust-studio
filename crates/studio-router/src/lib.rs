//! `studio-router` — LLM provider routing for Cobrust Studio.
//!
//! Forked from `cobrust-llm-router` @ `61f2aff` (v0.1.1) per ADR-0005 and
//! ADR-0006. Six strip operations applied during the lift; readers cross-
//! checking against upstream should consult `docs/agent/adr/0006-*` for the
//! authoritative list. In summary:
//!
//! - **Strip #1** — consensus mode (`Strategy::Consensus { n }`) is gone;
//!   Studio dispatches one provider per call.
//! - **Strip #2** — ADR-0040 honest-gate hooks (translation-pipeline `L2`
//!   verdict typing) are gone (no-op for this pin; documented in
//!   `docs/agent/findings/` for traceability).
//! - **Strip #3** — per-task routing tables (`spec_extract` / `translate` /
//!   `repair` + `RoutingEntry` / `StrategyName` / `DefaultStrategy` / `Task`
//!   enum) collapsed into one global dispatch flow.
//! - **Strip #4** — translation-specific ledger fields (`L0..L3`) generalised
//!   to `LedgerEntry::task_tag: Option<String>`.
//! - **Strip #5** — cache + ledger default paths moved from `.cobrust/` to
//!   `$XDG_DATA_HOME/cobrust-studio/` (or `$HOME/.cache/cobrust-studio/`).
//! - **Strip #6** — `RouterResponse` renamed to [`DispatchResponse`]; Cobrust
//!   task-tag variants dropped.
//!
//! When `cobrust-llm-router` ships to crates.io with a post-strip surface
//! matching this crate's, `studio-router` becomes a thin facade re-exporting
//! from upstream (ADR-0005 §"Consequences"). License attribution carried
//! from upstream (Apache-2.0 OR MIT, The Cobrust Project).
//!
//! See `docs/agent/modules/studio-router.md`.

pub mod anthropic;
pub mod cache;
pub mod config;
pub mod ledger;
pub mod openai;
pub mod provider;
pub mod router;

// Public re-exports. Surface verified against ADR-0006 §"Decision".

// Provider trait + shared types (lifted unchanged from upstream).
pub use crate::provider::{
    Chunk, CompletionRequest, CompletionResponse, LlmError, LlmProvider, Message, Role,
    SamplingParams, TokenUsage,
};

// Provider implementations (lifted unchanged from upstream).
pub use crate::anthropic::AnthropicProvider;
pub use crate::openai::OpenAiProvider;

// Cache (public surface unchanged; default-path semantics changed per strip #5).
pub use crate::cache::{Cache, CacheKey};

// Ledger (task_tag generalised per strip #4).
pub use crate::ledger::{Ledger, LedgerEntry, Outcome};

// Config (RoutingEntry / StrategyName / DefaultStrategy removed per strip #3).
pub use crate::config::{ProviderConfig, ProviderKind, ProviderModel, RouterConfig};

// Router (RouterResponse → DispatchResponse per strip #6;
// Strategy::Consensus removed per strip #1; Task enum removed per strip #3).
pub use crate::router::{
    DispatchContext, DispatchResponse, RetryPolicy, Router, RouterBuilder, RouterError, Strategy,
};

/// Crate version.
#[must_use]
pub const fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_is_pkg_version() {
        assert_eq!(version(), env!("CARGO_PKG_VERSION"));
    }
}
