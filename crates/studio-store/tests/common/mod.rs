//! Shared test helpers for the studio-store integration corpus (Wave A2).
//!
//! Per ADR-0004 Studio's root layout has:
//!
//! ```text
//! <root>/
//!   docs/agent/adr/NNNN-*.md         <- ADR markdown source-of-truth
//!   docs/agent/findings/*.md         <- finding markdown source-of-truth
//!   .cobrust-studio/
//!     studio.db                      <- SQLite materialised index
//!     router/
//!       ledger.jsonl                 <- JSONL written by studio-router::Ledger,
//!                                       read by studio-store::ledger (ADR-0006 F-02)
//! ```
//!
//! The `tests/` corpus pins this layout via the helper functions below. If
//! DEV's `Store::open` chose a different relative ledger path the helpers are
//! the single point of change.

#![allow(dead_code)]

use std::path::{Path, PathBuf};

use tempfile::TempDir;

/// Convention pinned by Wave A2 P7-TEST: where studio-router writes JSONL and
/// where studio-store::ledger reads it from. If DEV picked a different relative
/// path under `<root>`, change this constant.
pub const LEDGER_REL_PATH: &str = ".cobrust-studio/router/ledger.jsonl";

/// `docs/agent/adr` relative to the studio root.
pub const ADR_REL_DIR: &str = "docs/agent/adr";

/// `docs/agent/findings` relative to the studio root.
pub const FINDING_REL_DIR: &str = "docs/agent/findings";

/// Build a fresh empty project root with the conventional layout pre-created
/// (empty `docs/agent/{adr,findings}/` + `.cobrust-studio/router/`).
///
/// Returns the `TempDir` guard alongside the root path so callers can keep it
/// alive for the test's lifetime.
pub fn fresh_studio_root() -> (TempDir, PathBuf) {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().to_path_buf();
    std::fs::create_dir_all(root.join(ADR_REL_DIR)).expect("mkdir adr");
    std::fs::create_dir_all(root.join(FINDING_REL_DIR)).expect("mkdir findings");
    std::fs::create_dir_all(root.join(LEDGER_REL_PATH).parent().expect("ledger parent"))
        .expect("mkdir ledger parent");
    (dir, root)
}

/// Absolute path to the conventional ledger JSONL file inside `root`.
#[must_use]
pub fn ledger_path(root: &Path) -> PathBuf {
    root.join(LEDGER_REL_PATH)
}

/// Absolute path to `docs/agent/adr/` inside `root`.
#[must_use]
pub fn adr_dir(root: &Path) -> PathBuf {
    root.join(ADR_REL_DIR)
}

/// Absolute path to `docs/agent/findings/` inside `root`.
#[must_use]
pub fn finding_dir(root: &Path) -> PathBuf {
    root.join(FINDING_REL_DIR)
}

/// A serde-compatible JSONL entry mirroring `studio_router::ledger::LedgerEntry`
/// (see `crates/studio-router/src/ledger.rs` at HEAD of feature/a2-test-store-corpus).
///
/// Used to pre-populate the ledger JSONL file in tests without taking a direct
/// dep on studio-router. If DEV chose to re-export `LedgerEntry` from
/// studio-store::ledger, the on-disk shape MUST be byte-identical to what
/// `serde_json::to_string` produces for `studio_router::ledger::LedgerEntry`
/// because ADR-0006 §F-02 makes the JSONL the source of truth.
#[must_use]
pub fn make_jsonl_line(
    ts: &str,
    task_tag: Option<&str>,
    provider: &str,
    provider_kind: &str,
    model: &str,
    cache_key: &str,
    cache_hit: bool,
    prompt_tokens: u32,
    completion_tokens: u32,
    total_tokens: u32,
    latency_ms: u32,
    attempt: u8,
    outcome: &str,
    error_code: Option<&str>,
) -> String {
    // Match exact field order + serde rename used by studio-router::LedgerEntry.
    // Outcome is `#[serde(rename_all = "snake_case")]` → "ok" / "error_transient" / "error_permanent".
    // ProviderKind is `#[serde(rename_all = "lowercase")]` → "anthropic" / "openai" / "synthetic".
    let task_tag_field = match task_tag {
        Some(t) => format!(
            r#""task_tag":{},"#,
            serde_json::Value::String(t.to_string())
        ),
        None => r#""task_tag":null,"#.to_string(),
    };
    let error_code_field = match error_code {
        Some(c) => format!(
            r#""error_code":{}"#,
            serde_json::Value::String(c.to_string())
        ),
        None => r#""error_code":null"#.to_string(),
    };
    format!(
        r#"{{"ts":"{ts}",{task_tag_field}"provider":"{provider}","provider_kind":"{provider_kind}","model":"{model}","cache_key":"{cache_key}","cache_hit":{cache_hit},"prompt_tokens":{prompt_tokens},"completion_tokens":{completion_tokens},"total_tokens":{total_tokens},"latency_ms":{latency_ms},"attempt":{attempt},"outcome":"{outcome}",{error_code_field}}}"#
    )
}

/// Write `lines` as JSONL (one entry per line, `\n`-terminated) into `path`.
/// Creates parents as needed.
pub fn write_jsonl(path: &Path, lines: &[String]) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("mkdir jsonl parent");
    }
    let body = if lines.is_empty() {
        String::new()
    } else {
        let mut s = lines.join("\n");
        s.push('\n');
        s
    };
    std::fs::write(path, body).expect("write jsonl");
}

/// Stamp out a synthetic `ok` JSONL line with a deterministic timestamp ordered
/// by `seq` (later seq → later UTC).
#[must_use]
pub fn ok_line(seq: u32, task_tag: Option<&str>) -> String {
    let ts = format!("2026-04-30T01:23:{:02}.000Z", seq.min(59));
    make_jsonl_line(
        &ts,
        task_tag,
        "anthropic_official",
        "anthropic",
        "claude-opus-4-7",
        &format!("blake3:seq{seq:04}"),
        false,
        100,
        200,
        300,
        1234,
        1,
        "ok",
        None,
    )
}
