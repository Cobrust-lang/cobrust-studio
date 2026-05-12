//! Axum [`Router`] builder + fallback handler.
//!
//! Splitting the router build away from the binary main means integration
//! tests (P7 TEST agent's `tests/` dir) can boot the same app without
//! binding to a TCP socket — they invoke `app.oneshot(req)` or
//! `axum::Router::into_make_service()` directly against a
//! `tokio::net::TcpListener` on an ephemeral port.
//!
//! Middleware stack (outermost to innermost):
//! - [`TraceLayer`] — request/response tracing spans (per ADR-0001's
//!   "long-running daemon needs structured observability" pillar).
//! - [`CorsLayer::permissive`] — M2 SvelteKit frontend pings the API
//!   during dev (`pnpm dev` on port 5173 hits Studio on 7878). Once we
//!   embed the frontend (ADR-0002), CORS becomes a no-op because both
//!   are served from the same origin — but the layer adds zero overhead
//!   when there are no cross-origin requests and saves us a config knob.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde::Serialize;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

use crate::AppState;
use crate::routes;

/// JSON body returned by the 404 fallback. Structured so the M2 frontend
/// can render a friendly "route not found" instead of parsing HTML.
#[derive(Clone, Debug, Serialize)]
struct NotFoundResponse {
    error: &'static str,
    code: &'static str,
}

/// Build the Axum [`Router`] with all middleware + routes wired.
///
/// Pure function — no I/O, no async — so tests can call it freely.
pub fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/api/health", get(routes::health))
        .route("/api/version", get(routes::version))
        .nest("/api/adr", routes::adr::router())
        .nest("/api/finding", routes::finding::router())
        .nest("/api/project", routes::project::router())
        .nest("/api/auth", routes::auth::router())
        .fallback(not_found)
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive())
        .with_state(state)
}

/// Catch-all 404 handler. Returns a structured JSON body rather than
/// Axum's default empty 404 so the frontend can show "route not found"
/// without parsing HTML.
#[allow(clippy::unused_async)] // Axum requires async handlers.
async fn not_found() -> Response {
    let body = NotFoundResponse {
        error: "route not found",
        code: "not_found",
    };
    (StatusCode::NOT_FOUND, Json(body)).into_response()
}
