//! Axum [`Router`] builder + fallback handlers.
//!
//! Splitting the router build away from the binary main means integration
//! tests (P7 TEST agent's `tests/` dir) can boot the same app without
//! binding to a TCP socket — they invoke `app.oneshot(req)` or
//! `axum::Router::into_make_service()` directly against a
//! `tokio::net::TcpListener` on an ephemeral port.
//!
//! # Wave M3: root split + embed fallback
//!
//! Before M3 the entire app was a single flat [`Router`] with a JSON
//! 404 fallback that matched every unknown path. Wave M3 (ADR-0002)
//! adds rust-embed asset serving for the SvelteKit bundle, which means
//! the root path `/` and every non-`/api` path must now serve HTML
//! (the SPA shell) instead of JSON.
//!
//! The refactor:
//!
//! - [`api_router`] returns the `/api/*` surface as its own
//!   [`Router<AppState>`] with the JSON-404 fallback. M2 frontend
//!   clients hitting an unknown `/api/typo` still get
//!   `{"error":"route not found","code":"not_found"}` — no change.
//! - [`build_router`] nests the API router under `/api`, mounts the
//!   embed handlers on `/` and as the *root* fallback, and applies the
//!   middleware stack at the root level so both halves get tracing +
//!   CORS uniformly.
//!
//! Middleware stack (outermost to innermost):
//! - [`TraceLayer`] — request/response tracing spans.
//! - [`CorsLayer::permissive`] — M2 SvelteKit dev mode on port 5173
//!   pings the API on 7878. M3 same-origin makes the layer a no-op
//!   for embedded-bundle traffic.

use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde::Serialize;
use tower_http::cors::CorsLayer;
use tower_http::trace::TraceLayer;

use crate::AppState;
use crate::embed;
use crate::routes;
use crate::spawn_watcher_bridge;

/// JSON body returned by the `/api/*` 404 fallback. Structured so the
/// M2 frontend can render a friendly "route not found" instead of
/// parsing HTML.
#[derive(Clone, Debug, Serialize)]
struct NotFoundResponse {
    error: &'static str,
    code: &'static str,
}

/// The `/api/*` half of the surface as its own router so it can carry
/// its own JSON-404 fallback independent of the embed-fallback root.
///
/// Mounting note: callers nest this under `/api`, *not* at the root.
/// The handler paths inside are written as if `/api` was the root
/// (e.g. `/health` becomes `/api/health` after nesting).
fn api_router() -> Router<AppState> {
    Router::new()
        .route("/health", get(routes::health))
        .route("/version", get(routes::version))
        .nest("/adr", routes::adr::router())
        .nest("/finding", routes::finding::router())
        .nest("/project", routes::project::router())
        .nest("/auth", routes::auth::router())
        .nest("/ledger", routes::ledger::router())
        .nest("/events", routes::events::router())
        .nest("/dispatch", routes::dispatch::router())
        .fallback(api_not_found)
}

/// Build the Axum [`Router`] with all middleware + routes wired.
///
/// Side effect: spawns the filesystem-watcher → SSE-event-hub bridge
/// (via [`crate::spawn_watcher_bridge`]) on the ambient tokio runtime
/// so `/api/events` subscribers receive ADR / finding change events
/// without the caller having to remember to call
/// `spawn_watcher_bridge` separately.
///
/// Must be called from within a tokio runtime (`tokio::spawn` is used);
/// integration tests using `#[tokio::test]` and `serve()` both satisfy
/// this.
pub fn build_router(state: AppState) -> Router {
    spawn_watcher_bridge(&state);
    Router::new()
        .nest("/api", api_router())
        // Wave M3: SPA shell at root + embed-fallback for everything
        // else. Per ADR-0002, the SvelteKit static export under
        // `web/build/` is baked into the binary (release builds) or
        // served as a dev-stub HTML (`cargo build` without
        // `pnpm build`). Any non-/api path that does not resolve to
        // an embedded file falls back to `index.html` so SvelteKit's
        // client-side router can take over.
        .route("/", get(embed::serve_index))
        .fallback(embed::serve_asset)
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive())
        .with_state(state)
}

/// `/api/*` 404 handler. Returns a structured JSON body so frontends
/// can render a typed "route not found" without parsing HTML.
///
/// Routes outside `/api/*` fall through to
/// [`crate::embed::serve_asset`] which serves the SPA shell.
#[allow(clippy::unused_async)] // Axum requires async handlers.
async fn api_not_found() -> Response {
    let body = NotFoundResponse {
        error: "route not found",
        code: "not_found",
    };
    (StatusCode::NOT_FOUND, Json(body)).into_response()
}
