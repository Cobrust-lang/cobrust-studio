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
pub mod health;
pub mod version;

pub use adr::{AdrDraftBody, AdrListResponse};
pub use health::{HealthResponse, health};
pub use version::{VersionResponse, version};
