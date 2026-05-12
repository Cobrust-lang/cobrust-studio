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
//! - [`serve`] binds the listener and runs `axum::serve` until the
//!   process is signalled (graceful drain deferred to Wave A6+).
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
pub mod error;
pub mod router_init;
pub mod routes;
pub mod sse;
pub mod state;
pub mod synthetic;

use std::net::SocketAddr;

use futures::StreamExt;
use studio_store::{AdrChangeEvent, FindingChangeEvent, Store};
use tokio::net::TcpListener;

pub use crate::app::build_router;
pub use crate::cli::{Cli, Command, ServeArgs};
pub use crate::error::RouteError;
pub use crate::routes::{HealthResponse, VersionResponse};
pub use crate::sse::{EventEnvelope, EventHub, SSE_BUFFER_CAP};
pub use crate::state::{AppState, DispatchContext};
pub use crate::synthetic::SyntheticProvider;

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
    // Wave A5: try to construct the router from `<project_root>/studio.toml`.
    // Soft-fail to `None` on any error (missing config, malformed TOML, no
    // credentials, build failure). See ADR-0006 §"Addendum 2026-05-11" F-01
    // for the binding contract, and `crate::router_init` for the resolution
    // order. The `None` path keeps Wave-A4 503 behavior intact.
    let router = router_init::try_build_router_from_project(&project_root, &store).await?;
    let state = AppState::new(store, router, project_root.clone());

    // Wave A4: spawn the watcher → EventHub bridge before binding the
    // listener so the first connected client never misses an event
    // that fired during boot.
    spawn_watcher_bridge(&state);

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

/// Spawn two background tasks that drain
/// [`studio_store::adr::AdrHandle::watch`] and
/// [`studio_store::finding::FindingHandle::watch`] into the SSE event
/// hub. Tasks live until the underlying `notify` watcher closes (which
/// happens when the `Store` is dropped at process shutdown).
///
/// Public so test harnesses can pre-arm the bridge before issuing
/// filesystem events.
pub fn spawn_watcher_bridge(state: &AppState) {
    let adr_stream = state.store().adr().watch();
    let events_adr = state.events().clone();
    tokio::spawn(async move {
        let mut s = std::pin::pin!(adr_stream);
        while let Some(evt) = s.next().await {
            let envelope = match evt {
                AdrChangeEvent::Added(p) => sse::EventEnvelope::AdrAdded {
                    path: p.display().to_string(),
                },
                AdrChangeEvent::Modified(p) => sse::EventEnvelope::AdrModified {
                    path: p.display().to_string(),
                },
                AdrChangeEvent::Removed(p) => sse::EventEnvelope::AdrRemoved {
                    path: p.display().to_string(),
                },
            };
            events_adr.publish(envelope);
        }
        tracing::debug!("adr watcher stream closed; bridge task exiting");
    });

    let finding_stream = state.store().finding().watch();
    let events_finding = state.events().clone();
    tokio::spawn(async move {
        let mut s = std::pin::pin!(finding_stream);
        while let Some(evt) = s.next().await {
            let envelope = match evt {
                FindingChangeEvent::Added(p) => sse::EventEnvelope::FindingAdded {
                    path: p.display().to_string(),
                },
                FindingChangeEvent::Modified(p) => sse::EventEnvelope::FindingModified {
                    path: p.display().to_string(),
                },
                FindingChangeEvent::Removed(p) => sse::EventEnvelope::FindingRemoved {
                    path: p.display().to_string(),
                },
            };
            events_finding.publish(envelope);
        }
        tracing::debug!("finding watcher stream closed; bridge task exiting");
    });
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
