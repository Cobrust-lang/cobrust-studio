//! HTTP route handlers.
//!
//! Wave A3 only ships two trivial routes (`/api/health`, `/api/version`)
//! to prove the cross-crate wiring through [`crate::AppState`] is intact.
//! Wave A4 adds the real CRUD + dispatch surface enumerated in
//! `docs/agent/modules/studio-server.md` §"Public surface (M1 target)".
//!
//! Each route lives in its own submodule so A4's per-handler diffs land
//! cleanly without disturbing this file.

pub mod health;
pub mod version;

pub use health::{HealthResponse, health};
pub use version::{VersionResponse, version};
