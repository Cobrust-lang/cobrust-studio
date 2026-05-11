//! `GET /api/health` — liveness + uptime + project anchor.
//!
//! Trivial route by design — Wave A3 only needs to prove the Axum app
//! can read [`crate::AppState`] inside a handler. Returns:
//!
//! ```json
//! { "status": "ok", "uptime_seconds": 42, "project": "/path/to/project" }
//! ```
//!
//! Future waves may extend with subsystem health (router reachable,
//! store db reachable) — keep the additive field convention and don't
//! repurpose `status` to anything other than `"ok" | "degraded"`.

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Serialize;

use crate::AppState;

/// Body shape of `/api/health`. Kept narrow on purpose — A3 only proves
/// the wiring; A4+ may add subsystem flags.
#[derive(Clone, Debug, Serialize)]
pub struct HealthResponse {
    /// Always `"ok"` while the process is accepting requests. Future
    /// waves may introduce `"degraded"` when a subsystem (store, router)
    /// fails its periodic probe.
    pub status: &'static str,
    /// Whole seconds since [`AppState::started_at`]. See
    /// [`AppState::uptime_seconds`] for the saturation contract.
    pub uptime_seconds: u64,
    /// Absolute path the server was started against. Frontend uses this
    /// for the project-banner display (M2).
    pub project: String,
}

/// Handler for `GET /api/health`.
#[allow(clippy::unused_async)] // Axum requires async handlers.
pub async fn health(State(state): State<AppState>) -> Response {
    let body = HealthResponse {
        status: "ok",
        uptime_seconds: state.uptime_seconds(),
        project: state.project_root.display().to_string(),
    };
    (StatusCode::OK, Json(body)).into_response()
}
