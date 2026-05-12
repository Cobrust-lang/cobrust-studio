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
//! Wave A4 cannot construct the [`studio_router::Router`] yet — that
//! requires loaded credentials (the auth route lands in A4 but the
//! decryption + provider construction is A5 work). Until then this
//! route returns `503 router_not_configured` with the documented JSON
//! body shape.
//!
//! When A5 lands, the body of [`dispatch_sse`] will be replaced with:
//!
//! - Parse [`DispatchRequest`] JSON body.
//! - Snapshot a [`studio_router::CompletionRequest`].
//! - Call `router.dispatch(req).await` (non-streaming for M1; the
//!   `Chunk` stream variant is wave M2+ once
//!   [`studio_router::LlmProvider::complete_stream`] is plumbed
//!   through the router).
//! - Emit a single SSE `done` event with the full
//!   [`studio_router::DispatchResponse`] payload, then close the
//!   stream.
//!
//! The 503 path is hit by the M2 frontend "router not configured yet"
//! banner — clean, parseable, M2-friendly.

use std::convert::Infallible;

use axum::Json;
use axum::Router;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use futures::stream::{self, Stream};
use serde::{Deserialize, Serialize};

use crate::AppState;

/// Body shape for `POST /api/dispatch`. Wave A4 declares the shape so
/// integration tests have something to send; Wave A5 wires the real
/// router call. Fields mirror [`studio_router::CompletionRequest`]
/// plus a caller-supplied `task_tag` placeholder (per ADR-0006 §F-03,
/// `DispatchContext` lands at the call site once the router supports
/// it).
#[derive(Debug, Deserialize)]
pub struct DispatchRequest {
    /// Model id (e.g. `"claude-opus-4-7"`).
    #[serde(default)]
    pub model: Option<String>,
    /// Messages — narrow `{role, content}` shape mirroring
    /// `studio_router::Message`. Optional so A4's 503 path can short-
    /// circuit without a 400 on missing body fields.
    #[serde(default)]
    pub messages: Vec<DispatchMessage>,
    /// Caller-supplied task tag (e.g. `"agent-turn"`); persisted into
    /// the router's ledger by A5.
    #[serde(default)]
    pub task_tag: Option<String>,
}

/// One message in [`DispatchRequest`].
#[derive(Debug, Deserialize)]
pub struct DispatchMessage {
    /// `system` | `user` | `assistant`.
    pub role: String,
    /// Free-form text content.
    pub content: String,
}

/// 503 JSON body when the router is not configured.
#[derive(Debug, Serialize)]
struct RouterNotConfigured {
    error: &'static str,
    code: &'static str,
}

/// Build the dispatch sub-router. Mounted under `/api/dispatch`.
pub fn router() -> Router<AppState> {
    Router::new().route("/", post(dispatch_sse))
}

/// Handler for `POST /api/dispatch`.
///
/// A4 always returns 503 because [`AppState::router`] is `None` until
/// A5 wires construction. The `Result<Sse<...>, Response>` shape lets
/// the success path return a streaming response and the error path
/// return a plain JSON 503 — both compile because both implement
/// [`IntoResponse`].
pub async fn dispatch_sse(
    State(state): State<AppState>,
    body: Option<Json<DispatchRequest>>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, Response> {
    let Some(router) = state.router().cloned() else {
        return Err((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(RouterNotConfigured {
                error: "router not configured",
                code: "router_not_configured",
            }),
        )
            .into_response());
    };
    // A5 will reach here; for now the body is a placeholder so the
    // type machinery compiles and the 503 path is the live code.
    //
    // When A5 lands:
    //   let cr = into_completion_request(body)?;
    //   match router.dispatch(cr).await { ... }
    //
    // Until then keep an explicit `let _ = (router, body)` to silence
    // unused-binding lints without `#[allow]`.
    let _ = (router, body);
    let stream = stream::iter(std::iter::once(Ok::<_, Infallible>(
        Event::default()
            .event("done")
            .data("{\"status\":\"router-not-wired-yet\"}"),
    )));
    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}
