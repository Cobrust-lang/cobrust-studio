//! `studio-server` — Axum HTTP layer for Cobrust Studio.
//!
//! Wave A3 lands the cross-crate skeleton:
//!
//! - [`AppState`] holds a [`studio_store::Store`] handle + an optional
//!   [`studio_router::Router`] (per ADR-0006 §"Addendum 2026-05-11" the
//!   real router needs a `RouterConfig` + ≥1 provider, so A3 leaves it
//!   `None` and Wave A4 / A5 wires the construction).
//! - [`build_router`] returns the configured Axum [`axum::Router`] with
//!   `/api/health` + `/api/version` mounted, tracing + CORS middleware
//!   layered, and a JSON 404 fallback.
//! - [`serve`] binds the listener and runs the app until Ctrl-C.
//!
//! Wave A4 extends with the real route surface listed in
//! `docs/agent/modules/studio-server.md` §"Public surface (M1 target)":
//! - `POST /api/auth/set-endpoint`
//! - `GET /api/project/current`
//! - `GET|POST /api/adr`, `GET /api/adr/:id`
//! - `GET|POST /api/finding`
//! - `POST /api/dispatch` (SSE, gated on `AppState.router.is_some()`)
//! - `GET /api/ledger/recent`
//! - `GET /api/events` (SSE state-change channel)

pub mod app;
pub mod cli;
pub mod routes;
pub mod state;

use std::net::SocketAddr;

use studio_store::Store;
use tokio::net::TcpListener;

pub use crate::app::build_router;
pub use crate::cli::{Cli, Command, ServeArgs};
pub use crate::routes::{HealthResponse, VersionResponse};
pub use crate::state::AppState;

/// Crate version exposed via the `/api/version` route.
#[must_use]
pub const fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Server-level error. Wraps the surfaces the binary entrypoint can hit
/// during startup; per-route errors are not modelled here (routes
/// return [`axum::response::Response`] directly).
#[derive(Debug, thiserror::Error)]
pub enum ServerError {
    /// [`studio_store::Store::open`] failed during startup.
    #[error("store: {0}")]
    Store(#[from] studio_store::StoreError),
    /// `bind` / `accept` / shutdown signal handling failed.
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
}

/// Run the server end-to-end: open the store, build the app, bind the
/// listener, serve until shutdown signal.
///
/// `host` and `port` mirror [`ServeArgs`]. Returns once the listener
/// stops accepting connections (currently `axum::serve` runs until the
/// process is signalled — graceful Ctrl-C handling lands with the SSE
/// fan-out work in Wave A6+).
///
/// # Errors
/// Bubbles up [`ServerError`] from store open, bind, or serve loop.
pub async fn serve(args: &ServeArgs) -> Result<(), ServerError> {
    let project_root = args.project.clone();
    let store = Store::open(project_root.clone()).await?;
    // Wave A3: router stays `None`; A4/A5 plumb the real construction.
    let state = AppState::new(store, None, project_root.clone());

    let addr: SocketAddr =
        format!("{}:{}", args.host, args.port)
            .parse()
            .map_err(|e: std::net::AddrParseError| {
                std::io::Error::new(std::io::ErrorKind::InvalidInput, e.to_string())
            })?;
    let listener = TcpListener::bind(addr).await?;
    let bound = listener.local_addr()?;
    tracing::info!(
        %bound,
        project = %project_root.display(),
        "cobrust-studio serving on http://{bound}",
    );
    println!(
        "cobrust-studio serving on http://{bound} project={}",
        project_root.display(),
    );

    let app = build_router(state);
    axum::serve(listener, app.into_make_service()).await?;
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn version_is_pkg_version() {
        assert_eq!(version(), env!("CARGO_PKG_VERSION"));
    }

    #[tokio::test]
    async fn build_router_smokes_with_real_state() {
        // Cross-crate integration probe: prove the server can open a
        // Store and wrap it in an AppState the Axum app accepts.
        let tmp = tempfile::tempdir().expect("tempdir");
        let store = Store::open(tmp.path()).await.expect("Store::open");
        let state = AppState::new(store, None, tmp.path().to_path_buf());
        let _router = build_router(state);
        // The fact we got here means the type plumbing compiles + runs;
        // P7 TEST's hyper-level integration tests assert the response
        // bodies.
    }

    #[tokio::test]
    async fn uptime_is_monotonic_nondecreasing() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let store = Store::open(tmp.path()).await.expect("Store::open");
        let state = AppState::new(store, None, tmp.path().to_path_buf());
        let a = state.uptime_seconds();
        // No sleep — just two reads; the assertion is `>=`, not `>`.
        let b = state.uptime_seconds();
        assert!(b >= a, "uptime went backwards: {a} -> {b}");
    }
}
