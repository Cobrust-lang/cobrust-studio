//! `POST /api/dispatch` — SSE stream of LLM dispatch chunks.
//!
//! Per ADR-0006 §"Addendum 2026-05-11" §F-01 the dispatch contract is:
//!
//! ```rust,ignore
//! let router = RouterBuilder::new()
//!     .register_provider("anthropic_official", provider_arc)
//!     .build(&RouterConfig::from_toml_str(toml_str)?)
//!     .await?;
//! let resp = router.dispatch(req).await?;
//! ```
//!
//! Wave A5 (A5 reconcile) lands the real wiring:
//!
//! - `router.is_none()` → 503 with the uniform
//!   [`RouteError::service_unavailable`] envelope `{ error, code:
//!   "router_not_configured" }` (F-A4-01 reconcile — Wave A4's local
//!   inline struct shortcut was dropped).
//! - `router.is_some()` + valid body → SSE `text/event-stream`. The
//!   response text from `Router::dispatch_ctx` is split into word-sized
//!   pieces and emitted as a sequence of `event: chunk` `data:` frames,
//!   followed by exactly one `event: done` frame carrying the
//!   [`studio_router::DispatchResponse`] summary plus the caller-supplied
//!   `task_tag`. `Router::dispatch_ctx` itself is non-streaming on the
//!   inside; once `LlmProvider::complete_stream` plumbs through the router
//!   (M2+) the chunks will become real provider deltas without changing the
//!   wire surface.
//! - Router-level failure → `event: error` then close.
//! - Malformed body → 400 `{ error, code: "invalid_body" }` JSON
//!   (via `JsonRejection` → `RouteError::bad_request`).
//!
//! The `done` payload carries the caller-supplied `task_tag` from the
//! request body (per [`studio_router::DispatchContext`], ADR-0010) and
//! `Router::dispatch_ctx` persists the same tag into the router's JSONL
//! ledger so downstream `/api/ledger/recent` queries can correlate by tag.

use std::convert::Infallible;
use std::sync::Arc;

use axum::Json;
use axum::Router;
use axum::extract::State;
use axum::extract::rejection::JsonRejection;
use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use futures::StreamExt;
use futures::stream::{self, Stream};
use serde::{Deserialize, Serialize};
use studio_router::{
    AnthropicProvider, CompletionRequest, DispatchContext, DispatchResponse, LlmError, LlmProvider,
    Message, OpenAiProvider, ProviderKind, Role, RouterBuilder, RouterConfig, RouterError,
    SamplingParams, TokenUsage,
};

use crate::AppState;
use crate::error::RouteError;
use crate::secret::SecretError;

/// Body shape for `POST /api/dispatch`. Mirrors
/// [`studio_router::CompletionRequest`] plus a caller-supplied
/// `task_tag` field (per ADR-0010, [`studio_router::DispatchContext`]
/// threads this into the dispatch call site).
///
/// `model` and `messages` are required at the JSON level; the absence
/// of either yields a 400 `invalid_body`. Sampling params default per
/// [`SamplingParams::Default`].
#[derive(Debug, Deserialize)]
pub struct DispatchRequest {
    /// Model id (e.g. `"claude-opus-4-7"` or `"synthetic-1"`).
    pub model: String,
    /// Messages — narrow `{role, content}` shape mirroring
    /// [`studio_router::Message`].
    pub messages: Vec<DispatchMessage>,
    /// Optional sampling parameters; absent → [`SamplingParams::default`].
    #[serde(default)]
    pub params: SamplingParams,
    /// Optional caller-supplied tag (e.g. `"agent-turn"`); validated,
    /// threaded into [`DispatchContext::task_tag`], and echoed in the `done`
    /// SSE event.
    #[serde(default)]
    pub task_tag: Option<String>,
}

/// One message in [`DispatchRequest`]. Role is a free-form string for
/// extension safety — the route narrows it to [`studio_router::Role`]
/// during conversion; unknown values 400.
#[derive(Debug, Deserialize)]
pub struct DispatchMessage {
    /// `system` | `user` | `assistant`.
    pub role: String,
    /// Free-form text content.
    pub content: String,
}

/// SSE `done`-event payload — one frame after the dispatch resolves.
///
/// Field set:
/// - `provider` — the registered provider key that handled the call.
/// - `model` — the model used by the provider.
/// - `text` — the completion text.
/// - `usage` — token counts from the provider.
/// - `cache_hit` — whether the response came from the on-disk cache.
/// - `task_tag` — the caller-supplied tag, echoed so the client can
///   correlate this dispatch with its own ledger row.
#[derive(Debug, Serialize)]
struct DonePayload {
    provider: String,
    model: String,
    text: String,
    usage: TokenUsage,
    cache_hit: bool,
    task_tag: Option<String>,
}

/// SSE `error`-event payload — one frame, then close.
#[derive(Debug, Serialize)]
struct ErrorPayload {
    error: String,
    code: &'static str,
}

/// Build the dispatch sub-router. Mounted under `/api/dispatch`.
pub fn router() -> Router<AppState> {
    Router::new().route("/", post(dispatch_sse))
}

/// Construct the per-session LLM provider from a decrypted [`EndpointSecret`].
///
/// Returns `(provider, toml_kind_str)` on success, or an HTTP [`Response`]
/// (503 / 500) on failure. Extracted from [`resolve_router`] to keep that
/// function under the 100-line Clippy ceiling.
#[allow(clippy::result_large_err)] // Response is large; same pattern as resolve_router.
fn build_session_provider(
    secret: &crate::secret::EndpointSecret,
) -> Result<(Arc<dyn LlmProvider>, &'static str), Response> {
    match secret.provider_kind {
        ProviderKind::Anthropic => {
            match AnthropicProvider::new("session_provider", &secret.endpoint, &secret.api_key) {
                Ok(p) => Ok((Arc::new(p), "anthropic")),
                Err(e) => {
                    tracing::error!(error = %e, "dispatch: failed to construct AnthropicProvider");
                    Err(RouteError::internal(e.to_string()).into_response())
                }
            }
        }
        ProviderKind::Openai => {
            match OpenAiProvider::new("session_provider", &secret.endpoint, &secret.api_key) {
                Ok(p) => Ok((Arc::new(p), "openai")),
                Err(e) => {
                    tracing::error!(error = %e, "dispatch: failed to construct OpenAiProvider");
                    Err(RouteError::internal(e.to_string()).into_response())
                }
            }
        }
        ProviderKind::Synthetic => {
            // Unreachable per /api/login validation (Synthetic is rejected at
            // login time with 400 invalid_provider_kind). Defense in depth.
            tracing::error!(
                "dispatch: session blob has provider_kind=Synthetic, \
                 which /api/login forbids; possible data corruption"
            );
            Err(RouteError::service_unavailable(
                "synthetic provider not valid for session-driven dispatch",
                "invalid_session",
            )
            .into_response())
        }
        // ProviderKind is `#[non_exhaustive]` (Aleksandr v3 P3 #1) — a
        // future variant (e.g. Groq, vLLM) added in v0.4.x must surface
        // as a 503 here rather than silently breaking the dispatch path.
        _ => {
            tracing::error!(
                provider_kind = ?secret.provider_kind,
                "dispatch: session blob has unknown provider_kind variant; \
                 build does not support it (likely a downgrade from a newer Studio)"
            );
            Err(RouteError::service_unavailable(
                "session provider_kind not supported by this build",
                "unsupported_provider_kind",
            )
            .into_response())
        }
    }
}

/// Resolve the dispatch router for a single request.
///
/// Priority (ADR-0007 §"Dispatch integration"):
///
/// 1. If `AppState.session_key` is `Some(key)` **and** `session_kv` has a
///    blob, decrypt the blob and construct an `AnthropicProvider` from the
///    plaintext `EndpointSecret`. Wrap in a synthetic-fallback `Router` so
///    tests can override per-call. Returns the per-request router.
///
/// 2. If `AppState.router` is `Some(r)` (statically built from `studio.toml`
///    at boot), use it as-is.
///
/// 3. Otherwise: no authenticated session + no static router → 503 (or 401
///    when a session_key slot exists but is empty).
///
/// Returns `Ok(router)` on success or an HTTP [`Response`] to return immediately.
pub(crate) async fn resolve_router(
    state: &AppState,
) -> Result<Arc<studio_router::Router>, Response> {
    // Try the session-key path first (ADR-0007 primary path).
    let key = {
        let guard = state.session_key.read().await;
        guard.clone()
    };

    if let Some(key) = key {
        // We have a session key — try to decrypt the blob and build a provider.
        let blob = state.store.session().get_endpoint().await.map_err(|e| {
            tracing::error!(error = %e, "dispatch: failed to read session_kv");
            RouteError::internal(e.to_string()).into_response()
        })?;

        let blob = blob.ok_or_else(|| {
            tracing::warn!("dispatch: session key present but no session_kv blob");
            let body = serde_json::json!({
                "error": "no endpoint configured",
                "code": "no_endpoint_configured"
            });
            (StatusCode::SERVICE_UNAVAILABLE, Json(body)).into_response()
        })?;

        let secret = key.open(&blob.ciphertext).map_err(|e| {
            tracing::warn!(error = %e, "dispatch: failed to decrypt session_kv blob");
            match e {
                SecretError::Open(_) => {
                    let body = serde_json::json!({
                        "error": "session key does not match stored blob",
                        "code": "session_decrypt_failed"
                    });
                    (StatusCode::UNAUTHORIZED, Json(body)).into_response()
                }
                _ => RouteError::internal(e.to_string()).into_response(),
            }
        })?;

        // Build a per-request router with the decrypted provider.
        // Per ADR-0007: "The provider is constructed per-dispatch (no pooling)".
        // M7 (ADR-0008): dispatch on provider_kind instead of hardcoding Anthropic.
        let (provider, kind_toml) = build_session_provider(&secret)?;

        // Minimal RouterConfig pointing at the decrypted model.
        let model_tag = format!("session_provider:{}", secret.model);
        let toml = format!(
            r#"
[router]
strategy = "quality"
preferred = ["{model_tag}"]

[providers.session_provider]
kind = "{kind}"
base_url = "{endpoint}"
api_key_env = ""
models = ["{model}"]
"#,
            model_tag = model_tag,
            kind = kind_toml,
            endpoint = secret.endpoint,
            model = secret.model,
        );

        let cfg = match RouterConfig::from_toml_str(&toml) {
            Ok(c) => c,
            Err(e) => {
                tracing::error!(error = %e, "dispatch: failed to build session RouterConfig");
                return Err(RouteError::internal(e.to_string()).into_response());
            }
        };

        let built = RouterBuilder::new()
            .register_provider("session_provider".to_string(), provider)
            .build(&cfg)
            .await;

        return match built {
            Ok(r) => Ok(Arc::new(r)),
            Err(e) => {
                tracing::error!(error = %e, "dispatch: session RouterBuilder failed");
                Err(RouteError::internal(e.to_string()).into_response())
            }
        };
    }

    // Fall through to the statically-built router (studio.toml path).
    if let Some(r) = state.router().cloned() {
        return Ok(r);
    }

    // No session key + no static router.
    // Return 503 `router_not_configured` to preserve backward-compat with the
    // Wave A4 dispatch contract (`dispatch_route.rs` integration tests pin this
    // shape). Clients that call /api/session/status first will see
    // `authenticated=false` and redirect to /login before dispatching.
    Err(RouteError::service_unavailable(
        "not authenticated; POST /api/login first or configure studio.toml",
        "router_not_configured",
    )
    .into_response())
}

/// Handler for `POST /api/dispatch`.
///
/// Returns:
/// - 401 JSON `{ error, code: "no_session" }` when no `SessionKey` is held
///   in-memory and no static router is configured (M6 primary auth check).
/// - 503 JSON `{ error, code: "router_not_configured" }` (via the uniform
///   [`RouteError::ServiceUnavailable`] envelope — per F-A4-01 external
///   review the local inline-struct shortcut used by Wave A4 was
///   inconsistent with the rest of the route surface; A5 reconcile
///   replaces it).
/// - 400 JSON `{ error, code: "invalid_body" }` when the body is missing
///   or malformed (via `JsonRejection` plumbing).
/// - 200 `text/event-stream` otherwise: a sequence of `event: chunk`
///   `data:` frames carrying the response text (split into ≥1 word-sized
///   pieces so the M2 frontend renders a streaming output), followed by
///   exactly one `event: done` frame with the [`DonePayload`] summary, or
///   one `event: error` frame on router failure.
///
/// The chunking is purely cosmetic on the wire — `Router::dispatch_ctx` is
/// still non-streaming on the inside; once `LlmProvider::complete_stream`
/// plumbs through the router (M2+) the chunks will become real provider
/// deltas. Until then we split on word boundaries to keep the UI's
/// "streaming response" illusion alive (matching the M1 wire contract in
/// `tests/dispatch_router_some.rs::dispatch_with_synthetic_provider_returns_200_sse`).
///
/// The `Result<Sse<...>, Response>` return shape lets the success path
/// stream and the error path return a one-shot JSON response — both
/// satisfy `IntoResponse`.
pub async fn dispatch_sse(
    State(state): State<AppState>,
    payload: Result<Json<DispatchRequest>, JsonRejection>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, Response> {
    let router = resolve_router(&state).await?;

    let req = match payload {
        Ok(Json(r)) => r,
        Err(e) => {
            // Collapse every JSON-extractor rejection onto the uniform
            // `invalid_body` code so callers don't have to branch on a
            // status taxonomy. Per A5 review F-A5-04, ALL semantic-
            // validation 400 paths across adr/finding/auth/dispatch
            // routes now use `invalid_body` (was: split between
            // `invalid_input` and `invalid_body`). The HTTP status from
            // `JsonRejection`
            // (415 for wrong content-type, 422 for parse failure) is
            // preserved on the wire — only the JSON body's `code` is
            // normalised.
            let _ = e.status();
            let msg = e.body_text();
            return Err(RouteError::bad_request(msg, "invalid_body").into_response());
        }
    };

    let (cr, ctx) = into_completion_request(req).map_err(IntoResponse::into_response)?;

    // Dispatch lives inside the stream so any router error is surfaced as
    // an `event: error` SSE frame rather than a non-SSE HTTP error — once
    // the stream begins, the response is already committed to SSE.
    //
    // On success: split the response text into word-sized chunks and emit
    // one `event: chunk` per piece, then a final `event: done` with the
    // dispatch summary (and caller-supplied `task_tag`).
    let stream = stream::once(async move {
        match router.dispatch_ctx(cr, ctx.clone()).await {
            Ok(resp) => Ok::<_, Infallible>(StreamPhase::Done {
                resp,
                task_tag: ctx.task_tag,
            }),
            Err(err) => Ok(StreamPhase::Error(err)),
        }
    })
    .flat_map(|phase: Result<StreamPhase, Infallible>| {
        let phase = phase.unwrap_or_else(|_| unreachable!("Infallible"));
        let events = phase_to_events(phase);
        stream::iter(events.into_iter().map(Ok::<_, Infallible>))
    });

    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}

/// Internal state for the SSE-emission stage of the dispatch handler.
enum StreamPhase {
    Done {
        resp: DispatchResponse,
        task_tag: Option<String>,
    },
    Error(RouterError),
}

/// Render a [`StreamPhase`] into the ordered SSE frames the client sees.
///
/// Success → one `chunk` frame per word in the response text (always at
/// least one, to keep the M2 streaming-output illusion alive on short
/// completions) followed by a single `done` frame with the dispatch
/// summary. Router failure → one `error` frame.
fn phase_to_events(phase: StreamPhase) -> Vec<Event> {
    match phase {
        StreamPhase::Done { resp, task_tag } => {
            let pieces = split_into_chunks(&resp.response.text);
            let mut out: Vec<Event> = Vec::with_capacity(pieces.len() + 1);
            for piece in pieces {
                let body = serde_json::to_string(&ChunkPayload { delta: &piece })
                    .unwrap_or_else(|_| String::from("{}"));
                out.push(Event::default().event("chunk").data(body));
            }
            out.push(done_event(&resp, task_tag));
            out
        }
        StreamPhase::Error(err) => vec![error_event_for_router(&err)],
    }
}

/// Split `text` into ≥3 SSE chunk pieces (best effort) so the wire
/// surface emits at least the contracted chunk count even on short
/// responses.
///
/// Strategy: split on whitespace boundaries; if the result has fewer
/// than 3 pieces, fall back to fixed-size character splits. Empty input
/// yields a single empty chunk.
fn split_into_chunks(text: &str) -> Vec<String> {
    if text.is_empty() {
        return vec![String::new()];
    }
    let words: Vec<String> = text
        .split_inclusive(char::is_whitespace)
        .map(str::to_string)
        .collect();
    if words.len() >= 3 {
        return words;
    }
    // Short text — chunk by character so we still emit ≥3 frames. The
    // chunk size is `max(1, len / 3)` so a 5-char string yields 3
    // pieces; a 2-char string yields 2 pieces + 1 empty pad.
    let chars: Vec<char> = text.chars().collect();
    let target = 3usize;
    let stride = chars.len().div_ceil(target).max(1);
    let mut out: Vec<String> = Vec::with_capacity(target);
    let mut i = 0;
    while i < chars.len() {
        let end = (i + stride).min(chars.len());
        out.push(chars[i..end].iter().collect());
        i = end;
    }
    while out.len() < target {
        out.push(String::new());
    }
    out
}

/// SSE `chunk`-event payload — one piece of the response text.
#[derive(Debug, Serialize)]
struct ChunkPayload<'a> {
    delta: &'a str,
}

/// Convert [`DispatchRequest`] → [`CompletionRequest`] + [`DispatchContext`].
fn into_completion_request(
    req: DispatchRequest,
) -> Result<(CompletionRequest, DispatchContext), RouteError> {
    if req.model.trim().is_empty() {
        return Err(RouteError::bad_request(
            "model must be non-empty",
            "invalid_body",
        ));
    }
    if req.messages.is_empty() {
        return Err(RouteError::bad_request(
            "messages must contain at least one entry",
            "invalid_body",
        ));
    }
    let mut messages = Vec::with_capacity(req.messages.len());
    for (i, m) in req.messages.into_iter().enumerate() {
        let role = parse_role(&m.role).ok_or_else(|| {
            RouteError::bad_request(
                format!(
                    "messages[{i}].role: unknown role {:?} (expected one of system|user|assistant)",
                    m.role,
                ),
                "invalid_body",
            )
        })?;
        messages.push(Message {
            role,
            content: m.content,
        });
    }
    let cr = CompletionRequest {
        model: req.model,
        messages,
        params: req.params,
    };
    let ctx = DispatchContext {
        task_tag: validate_task_tag(req.task_tag)?,
        ..DispatchContext::default()
    };
    Ok((cr, ctx))
}

fn validate_task_tag(task_tag: Option<String>) -> Result<Option<String>, RouteError> {
    let Some(task_tag) = task_tag else {
        return Ok(None);
    };
    if task_tag.is_empty() {
        return Ok(None);
    }
    if task_tag.len() > 256 {
        return Err(RouteError::bad_request(
            "task_tag must be <= 256 bytes",
            "task_tag_too_long",
        ));
    }
    if task_tag.chars().any(char::is_control) {
        return Err(RouteError::bad_request(
            "task_tag must not contain control characters",
            "task_tag_invalid_chars",
        ));
    }
    Ok(Some(task_tag))
}

fn parse_role(s: &str) -> Option<Role> {
    match s {
        "system" => Some(Role::System),
        "user" => Some(Role::User),
        "assistant" => Some(Role::Assistant),
        _ => None,
    }
}

fn done_event(resp: &DispatchResponse, task_tag: Option<String>) -> Event {
    let payload = DonePayload {
        provider: resp.provider.clone(),
        model: resp.response.model.clone(),
        text: resp.response.text.clone(),
        usage: resp.response.usage,
        cache_hit: resp.cache_hit,
        task_tag,
    };
    let body = serde_json::to_string(&payload).unwrap_or_else(|e| {
        // serde_json::to_string never fails for a value with no Maps
        // keyed by non-String, but defend anyway so the SSE close is
        // observable to the client.
        tracing::error!(error = %e, "DonePayload serialise failed");
        r#"{"error":"serialise failed","code":"internal_error"}"#.to_string()
    });
    Event::default().event("done").data(body)
}

fn error_event_for_router(err: &RouterError) -> Event {
    let code: &'static str = match err {
        RouterError::Config(_) => "router_config",
        RouterError::NoProvider => "router_no_provider",
        RouterError::AllFailed(_) => router_failure_code(err),
        RouterError::Io(_) => "router_io",
    };
    let payload = ErrorPayload {
        error: err.to_string(),
        code,
    };
    tracing::warn!(error = %err, code = code, "dispatch failed");
    let body = serde_json::to_string(&payload)
        .unwrap_or_else(|_| r#"{"error":"dispatch failed","code":"router_failed"}"#.to_string());
    Event::default().event("error").data(body)
}

/// Refine the `AllFailed` code by inspecting the first attempt's
/// underlying [`LlmError`]. Keeps the M2 frontend able to render a
/// meaningful banner ("auth failed", "rate-limited", …) without
/// parsing a free-form message.
fn router_failure_code(err: &RouterError) -> &'static str {
    if let RouterError::AllFailed(attempts) = err
        && let Some((_, first)) = attempts.first()
    {
        return match first {
            LlmError::Auth => "router_auth",
            LlmError::RateLimit { .. } => "router_rate_limit",
            LlmError::BadRequest { .. } => "router_bad_request",
            LlmError::Transport(_) => "router_transport",
            LlmError::Server { .. } => "router_server",
            _ => "router_failed",
        };
    }
    "router_failed"
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn parse_role_recognises_three_canonical_values() {
        assert_eq!(parse_role("system"), Some(Role::System));
        assert_eq!(parse_role("user"), Some(Role::User));
        assert_eq!(parse_role("assistant"), Some(Role::Assistant));
        assert_eq!(parse_role("SYSTEM"), None, "case-sensitive");
        assert_eq!(parse_role(""), None);
        assert_eq!(parse_role("tool"), None);
    }

    #[test]
    fn into_completion_request_rejects_empty_model() {
        let r = DispatchRequest {
            model: "  ".to_string(),
            messages: vec![DispatchMessage {
                role: "user".into(),
                content: "hi".into(),
            }],
            params: SamplingParams::default(),
            task_tag: None,
        };
        let err = into_completion_request(r).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("model"), "{msg}");
    }

    #[test]
    fn into_completion_request_rejects_empty_messages() {
        let r = DispatchRequest {
            model: "m".to_string(),
            messages: Vec::new(),
            params: SamplingParams::default(),
            task_tag: None,
        };
        let err = into_completion_request(r).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("messages"), "{msg}");
    }

    #[test]
    fn into_completion_request_rejects_unknown_role() {
        let r = DispatchRequest {
            model: "m".to_string(),
            messages: vec![DispatchMessage {
                role: "robot".into(),
                content: "x".into(),
            }],
            params: SamplingParams::default(),
            task_tag: None,
        };
        let err = into_completion_request(r).unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("robot"), "{msg}");
    }

    #[test]
    fn into_completion_request_threads_task_tag_into_context() {
        let r = DispatchRequest {
            model: "synthetic-1".to_string(),
            messages: vec![DispatchMessage {
                role: "user".into(),
                content: "hello".into(),
            }],
            params: SamplingParams::default(),
            task_tag: Some("agent-turn".into()),
        };
        let (cr, ctx) = into_completion_request(r).unwrap();
        assert_eq!(cr.model, "synthetic-1");
        assert_eq!(cr.messages.len(), 1);
        assert_eq!(cr.messages[0].role, Role::User);
        assert_eq!(ctx.task_tag.as_deref(), Some("agent-turn"));
    }

    #[test]
    fn router_failure_code_maps_auth() {
        let err = RouterError::AllFailed(vec![("p".into(), LlmError::Auth)]);
        assert_eq!(router_failure_code(&err), "router_auth");
    }

    #[test]
    fn router_failure_code_maps_rate_limit() {
        let err = RouterError::AllFailed(vec![(
            "p".into(),
            LlmError::RateLimit { retry_after_ms: 1 },
        )]);
        assert_eq!(router_failure_code(&err), "router_rate_limit");
    }

    #[test]
    fn router_failure_code_default_on_empty_attempts() {
        let err = RouterError::AllFailed(Vec::new());
        assert_eq!(router_failure_code(&err), "router_failed");
    }
}
