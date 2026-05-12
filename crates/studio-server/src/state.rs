//! Shared application state — `AppState` + per-request `DispatchContext`.
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
//! - Wave A5 wires the real construction in [`crate::serve`] per ADR-0006
//!   §"Addendum 2026-05-11" — `RouterConfig::from_toml_str(&toml)?` +
//!   `register_provider(...)` + `.build(&cfg).await?`.
//!
//! Wave A5 also lands [`DispatchContext`] (ADR-0006 §"Addendum 2026-05-11"
//! §F-03) — the caller-supplied newtype that threads `task_tag` (and
//! future fields: span id, deadline) into the dispatch call site without
//! bloating [`studio_router::CompletionRequest`]'s wire shape.
//!
//! All `AppState` fields are documented; the struct is `Clone` because
//! Axum needs to hand a copy to each request future, and the underlying
//! [`studio_store::Store`] is `Arc`-shared internally.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use studio_router::Router;
use studio_store::Store;
use time::OffsetDateTime;
use tokio::sync::RwLock;

use crate::secret::SessionKey;
use crate::sse::EventHub;

/// Application state shared across all Axum handlers.
///
/// Clones are cheap: [`studio_store::Store`] is `Arc`-shared internally
/// and the optional router is wrapped in [`Arc`] so cloning the state
/// only bumps reference counts. The `session_key` is behind an
/// `Arc<RwLock<_>>` so the login route can write the derived key while
/// dispatch routes hold concurrent read locks.
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

    /// In-memory AES-256 session key (ADR-0007 M6).
    ///
    /// Set by `POST /api/login` after Argon2id derivation + AES-GCM seal.
    /// Cleared by `POST /api/logout`. Dropped on process restart.
    ///
    /// `None` → unauthenticated; `/api/dispatch` returns 401 `NoSession`.
    /// `Some(key)` → authenticated; dispatch decrypts the `session_kv` blob
    /// and passes the plaintext `EndpointSecret` to `AnthropicProvider::new`.
    ///
    /// The `Arc` is shared across `AppState` clones so all request handlers
    /// see the same live key without a deep-copy per request.
    pub session_key: Arc<RwLock<Option<SessionKey>>>,

    /// When `true`, `GET /api/session/endpoint` is exposed (debug-only).
    ///
    /// Set by `--debug-session` CLI flag at boot. Never `true` in production
    /// builds. The endpoint returns decrypted `endpoint` + `model` for E2E
    /// test introspection (never the `api_key`).
    pub debug_session: bool,
}

impl AppState {
    /// Construct a new [`AppState`] from the resolved components.
    ///
    /// Callers (`studio_server::serve`, tests, future bench harnesses)
    /// pass the already-opened [`Store`] and the optional [`Router`].
    /// Wave A3 always passes `None` for `router`.
    ///
    /// `session_key` initialises to `None` — the user must `POST /api/login`
    /// (or the binary must start with `--dev-api-key`) to authenticate.
    /// `debug_session` is `false` by default.
    #[must_use]
    pub fn new(store: Store, router: Option<Arc<Router>>, project_root: PathBuf) -> Self {
        Self {
            store,
            router,
            project_root,
            started_at: OffsetDateTime::now_utc(),
            events: EventHub::new(),
            session_key: Arc::new(RwLock::new(None)),
            debug_session: false,
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

/// Caller-supplied dispatch context.
///
/// Per ADR-0006 §"Addendum 2026-05-11" §F-03 the CTO chose Option (c) —
/// a newtype carried alongside [`studio_router::CompletionRequest`] at
/// the dispatch call site — over (a) "add field to `CompletionRequest`"
/// or (b) "second arg `dispatch(req, tag)`". This shape leaves the
/// router's wire types untouched while giving the server layer an
/// extension point for future cross-cutting fields (span IDs, deadline
/// hints, etc.).
///
/// Wave A5 only consumes `task_tag` — the studio-router `Router::dispatch`
/// API today ignores caller-supplied tags (see
/// `studio_router::router::Router::task_tag_for_request`, which returns
/// `None`). The Wave-A5 dispatch route therefore records the tag in its
/// own server-side dispatch log (or future ledger sidecar); plumbing
/// through router-internal ledger writes is post-M1.
///
/// Construct via `DispatchContext::default()` for the no-tag case, or
/// the field literal `DispatchContext { task_tag: Some("agent-turn".into()) }`
/// when the caller has a domain label to record.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct DispatchContext {
    /// Free-form caller-supplied tag (e.g. `"agent-turn"`, `"adr-draft"`).
    /// Mirrors [`studio_router::LedgerEntry::task_tag`] — `None` when the
    /// caller has no domain label to record.
    pub task_tag: Option<String>,
}

impl DispatchContext {
    /// Construct a context with just a `task_tag` set.
    #[must_use]
    pub fn with_task_tag(task_tag: impl Into<String>) -> Self {
        Self {
            task_tag: Some(task_tag.into()),
        }
    }
}
