//! `studio-server` — Axum HTTP layer for Cobrust Studio.
//!
//! M0 scaffold. M1 lands routes per ADR-0006 (TBD Day 2):
//! - `POST /api/auth/set-endpoint`
//! - `GET /api/project/current`
//! - `GET|POST /api/adr`
//! - `GET|POST /api/finding`
//! - `POST /api/dispatch` (SSE)
//! - `GET /api/ledger/recent`
//!
//! See `docs/agent/modules/studio-server.md` for the agent-facing spec.

/// Crate version exposed via the `/api/health` route (M1).
#[must_use]
pub const fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_is_pkg_version() {
        assert_eq!(version(), env!("CARGO_PKG_VERSION"));
    }
}
