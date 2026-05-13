//! HTTP route handlers.
//!
//! Wave A3 only ships two trivial routes (`/api/health`, `/api/version`)
//! to prove the cross-crate wiring through [`crate::AppState`] is intact.
//! Wave A4 adds the real CRUD + dispatch surface enumerated in
//! `docs/agent/modules/studio-server.md` §"Public surface (M1 target)".
//!
//! Each route lives in its own submodule so A4's per-handler diffs land
//! cleanly without disturbing this file.
//!
//! Handlers return `Result<axum::response::Response, RouteError>` and
//! the [`crate::error::RouteError`] `IntoResponse` impl renders the JSON
//! body. We allow `missing_errors_doc` crate-wide here because the
//! "errors" surface is Axum status codes documented at the route level,
//! not at the function signature level.

#![allow(clippy::missing_errors_doc)]

pub mod adr;
pub mod agent_turn;
pub mod auth;
pub mod dispatch;
pub mod events;
pub mod finding;
pub mod health;
pub mod ledger;
pub mod login;
pub mod models;
pub mod project;
pub mod version;

pub use adr::{AdrDraftBody, AdrListResponse};
pub use auth::{SetEndpointRequest, SetEndpointResponse};
pub use finding::{FindingDraftBody, FindingListResponse};
pub use health::{HealthResponse, health};
pub use ledger::{LEDGER_DEFAULT_N, LEDGER_MAX_N, LedgerRecentResponse};
pub use login::{LoginRequest, LoginResponse, SessionStatusResponse};
pub use models::ModelListResponse;
pub use project::ProjectCurrentResponse;
pub use version::{VersionResponse, version};
