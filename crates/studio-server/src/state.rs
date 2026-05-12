//! Shared application state — `AppState`.
//!
//! Wave A3 wires the minimum cross-crate integration required to prove the
//! Axum app can hold a [`studio_store::Store`] handle and a future
//! [`studio_router::Router`]. The `router` slot is [`Option`] because:
//!
//! - A3 does not need a live LLM router (only `/api/health` + `/api/version`
//!   trivial routes ship this wave).
//! - [`studio_router::RouterBuilder::build`] requires at least one
//!   registered provider that matches the parsed
//!   [`studio_router::RouterConfig`]; A3 has no config file in flight.
//! - Wave A4 / A5 will plumb the real construction (per ADR-0006
//!   §"Addendum 2026-05-11" — `RouterConfig::from_toml_str(&toml)?` +
//!   `register_provider(...)` + `.build(&cfg).await?`).
//!
//! All `AppState` fields are documented; the struct is `Clone` because
//! Axum needs to hand a copy to each request future, and the underlying
//! [`studio_store::Store`] is `Arc`-shared internally.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use studio_router::Router;
use studio_store::Store;
use time::OffsetDateTime;

use crate::sse::EventHub;

/// Application state shared across all Axum handlers.
///
/// Clones are cheap: [`studio_store::Store`] is `Arc`-shared internally
/// and the optional router is wrapped in [`Arc`] so cloning the state
/// only bumps reference counts.
#[derive(Clone, Debug)]
pub struct AppState {
    /// Persistence layer (ADR-0004). Constructed by [`Store::open`] at
    /// startup and shared by every route that touches ADR / finding /
    /// ledger / session storage.
    pub store: Store,

    /// LLM dispatch router (ADR-0006). `None` for A3 — Wave A4 / A5 will
    /// populate it from a project-level `studio.toml` + the user's
    /// credentials. Routes that need the router (`/api/dispatch`) must
    /// return a `503 router-not-configured` JSON error when `None`.
    pub router: Option<Arc<Router>>,

    /// Resolved absolute path to the project root the server was started
    /// against (`cobrust-studio serve --project <path>`). Routes use this
    /// to render `project` fields in API responses.
    pub project_root: PathBuf,

    /// UTC timestamp captured immediately before
    /// [`axum::serve`] starts accepting connections. Powers the
    /// `uptime_seconds` field on `/api/health`.
    pub started_at: OffsetDateTime,

    /// SSE fan-out hub for state-change events. The boot-time watcher
    /// task ([`crate::serve`]) publishes ADR/finding events; the
    /// `/api/events` handler subscribes per-request. Cloning the state
    /// shares the same hub via `Arc` internally.
    pub events: EventHub,
}

impl AppState {
    /// Construct a new [`AppState`] from the resolved components.
    ///
    /// Callers (`studio_server::serve`, tests, future bench harnesses)
    /// pass the already-opened [`Store`] and the optional [`Router`].
    /// Wave A3 always passes `None` for `router`.
    #[must_use]
    pub fn new(store: Store, router: Option<Arc<Router>>, project_root: PathBuf) -> Self {
        Self {
            store,
            router,
            project_root,
            started_at: OffsetDateTime::now_utc(),
            events: EventHub::new(),
        }
    }

    /// Whole seconds since [`Self::started_at`]. Saturates at zero on
    /// clock skew (the system clock moved backwards while the server
    /// was running). Whole seconds because `/api/health` returns an
    /// integer field — Studio doesn't need sub-second resolution there.
    #[must_use]
    pub fn uptime_seconds(&self) -> u64 {
        let now = OffsetDateTime::now_utc();
        let delta = now - self.started_at;
        u64::try_from(delta.whole_seconds()).unwrap_or(0)
    }

    /// Borrow the persistence layer.
    #[must_use]
    pub fn store(&self) -> &Store {
        &self.store
    }

    /// Borrow the optional dispatch router. Returns `None` until Wave A4/A5
    /// wires the construction.
    #[must_use]
    pub fn router(&self) -> Option<&Arc<Router>> {
        self.router.as_ref()
    }

    /// Borrow the absolute project root the server was started against.
    #[must_use]
    pub fn project_root(&self) -> &Path {
        &self.project_root
    }

    /// Server-start timestamp (UTC). Used to derive `uptime_seconds` and as
    /// a `last_modified`-style hint for future health-detail responses.
    #[must_use]
    pub fn started_at(&self) -> OffsetDateTime {
        self.started_at
    }

    /// Borrow the SSE event hub. `/api/events` subscribes through this.
    #[must_use]
    pub fn events(&self) -> &EventHub {
        &self.events
    }
}
