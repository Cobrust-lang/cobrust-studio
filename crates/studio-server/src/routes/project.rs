//! `GET /api/project/current` — project metadata.
//!
//! Returns the project root the server was started against, the UTC
//! timestamp captured at startup, and the `studio-server` crate version
//! — enough for the M2 frontend to render the project banner and the
//! "About" pane without hitting a second endpoint.
//!
//! ```json
//! {
//!   "project_root": "/abs/path/to/project",
//!   "started_at":   "2026-05-12T03:14:15Z",
//!   "version":      "0.0.1"
//! }
//! ```

use axum::Json;
use axum::Router;
use axum::extract::State;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use serde::Serialize;
use time::format_description::well_known::Rfc3339;

use crate::AppState;
use crate::error::RouteError;

/// Body shape for `GET /api/project/current`.
#[derive(Debug, Serialize)]
pub struct ProjectCurrentResponse {
    /// Absolute path the server was started against. Display string —
    /// callers should not interpret as bytes (`Path::display` may have
    /// done lossy conversion on non-UTF8 paths).
    pub project_root: String,
    /// RFC-3339 UTC timestamp captured at startup.
    pub started_at: String,
    /// `studio-server` crate version (for clients diffing API surface
    /// over time).
    pub version: &'static str,
}

/// Build the project sub-router. Mounted under `/api/project`.
pub fn router() -> Router<AppState> {
    Router::new().route("/current", get(current))
}

/// Handler for `GET /api/project/current`.
pub async fn current(State(state): State<AppState>) -> Result<Response, RouteError> {
    let started_at = state
        .started_at()
        .format(&Rfc3339)
        .map_err(|e| RouteError::internal(format!("rfc3339 format: {e}")))?;
    let body = ProjectCurrentResponse {
        project_root: state.project_root().display().to_string(),
        started_at,
        version: crate::version(),
    };
    Ok(Json(body).into_response())
}
