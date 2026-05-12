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
//! Wave A5 lands the real wiring:
//!
//! - `router.is_none()` → `503 router_not_configured` JSON (A4-stable).
//! - `router.is_some()` + valid body → SSE `text/event-stream` with one
//!   `event: done` payload (the full [`studio_router::DispatchResponse`]
//!   summary). `Router::dispatch` is non-streaming in M1 — the per-`Chunk`
//!   forward loop becomes meaningful once `LlmProvider::complete_stream`
//!   plumbs through the router (M2+).
//! - Router-level failure → `event: error` then close.
//! - Malformed body → 400 `{ code: "invalid_body" }` JSON.
//!
//! The `done` payload also carries the caller-supplied `task_tag` from the
//! request body (per [`crate::DispatchContext`], ADR-0006 §F-03), so the
//! client can correlate this dispatch with its own ledger row even though
//! `Router::dispatch` itself ignores tags today.

use std::convert::Infallible;

use axum::Json;
use axum::Router;
use axum::extract::State;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use futures::stream::{self, Stream};
use serde::{Deserialize, Serialize};
use studio_router::{
    CompletionRequest, DispatchResponse, LlmError, Message, Role, RouterError, SamplingParams,
    TokenUsage,
};

use crate::AppState;
use crate::error::RouteError;
use crate::state::DispatchContext;

/// Body shape for `POST /api/dispatch`. Mirrors
/// [`studio_router::CompletionRequest`] plus a caller-supplied
/// `task_tag` field (per ADR-0006 §F-03,
/// [`crate::DispatchContext`] threads this into the dispatch call site).
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
    /// Optional caller-supplied tag (e.g. `"agent-turn"`); threaded into
    /// [`DispatchContext::task_tag`] and echoed in the `done` SSE event.
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

/// 503 JSON body when the router is not configured. Wire shape is
/// `{ error, code: "router_not_configured" }`.
#[derive(Debug, Serialize)]
struct RouterNotConfigured {
    error: &'static str,
    code: &'static str,
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

/// Handler for `POST /api/dispatch`.
///
/// Returns:
/// - 503 JSON when [`AppState::router`] is `None`.
/// - 400 JSON when the body is missing or malformed.
/// - 200 `text/event-stream` SSE otherwise — exactly one `event: done`
///   on dispatch success, or one `event: error` on router failure.
///
/// The `Result<Sse<...>, Response>` return shape lets the success path
/// stream and the error path return a one-shot JSON response — both
/// satisfy `IntoResponse`.
pub async fn dispatch_sse(
    State(state): State<AppState>,
    body: Option<Json<DispatchRequest>>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, Response> {
    let Some(router) = state.router().cloned() else {
        return Err((
            axum::http::StatusCode::SERVICE_UNAVAILABLE,
            Json(RouterNotConfigured {
                error: "router not configured",
                code: "router_not_configured",
            }),
        )
            .into_response());
    };

    let Some(Json(req)) = body else {
        return Err(RouteError::bad_request(
            "request body missing or not application/json",
            "invalid_body",
        )
        .into_response());
    };

    let (cr, ctx) = into_completion_request(req).map_err(IntoResponse::into_response)?;

    // Dispatch lives inside the stream so any router error is surfaced as
    // an `event: error` SSE frame rather than a non-SSE HTTP error — once
    // the stream begins, the response is already committed to SSE.
    let stream = stream::once(async move {
        let event = match router.dispatch(cr).await {
            Ok(resp) => done_event(&resp, ctx.task_tag),
            Err(err) => error_event_for_router(&err),
        };
        Ok::<_, Infallible>(event)
    });

    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
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
        task_tag: req.task_tag,
    };
    Ok((cr, ctx))
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
