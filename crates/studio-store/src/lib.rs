//! `studio-store` — ADR / finding / ledger persistence for Cobrust Studio.
//!
//! M0 scaffold. M1 lands per ADR-0004:
//! - [`adr`] module — markdown CRUD with frontmatter validation
//! - [`finding`] module — markdown CRUD for negative results
//! - [`ledger`] module — append-only JSONL + SQLite materialized view
//!
//! See `docs/agent/modules/studio-store.md`.

/// Crate version.
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
