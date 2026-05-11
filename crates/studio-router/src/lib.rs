//! `studio-router` — LLM provider routing for Cobrust Studio.
//!
//! M0 scaffold. **ADR-0005 binding**: M1 lifts the design of
//! `cobrust-llm-router` (provider trait, Anthropic + OpenAI-compatible
//! adapters, BLAKE3 cache, JSONL ledger). The consensus mode is dropped
//! for MVP — Studio dispatches one provider per call.
//!
//! Once `cobrust-llm-router` is published to crates.io (Phase F.x of the
//! parent project), this crate becomes a thin facade re-exporting from
//! the upstream crate.
//!
//! See `docs/agent/modules/studio-router.md`.

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
