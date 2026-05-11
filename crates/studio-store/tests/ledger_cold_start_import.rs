//! Ledger cold-start import — Wave A2 TDD red.
//!
//! Per ADR-0006 §"Addendum 2026-05-11" §F-02: `studio-router::Ledger` writes
//! the JSONL append-only file; `studio-store::ledger` is a **reader** that
//! builds a SQLite materialised view on cold start.
//!
//! Convention pin (P7-TEST assumption — CTO reconciles with DEV):
//!   `<root>/.cobrust-studio/router/ledger.jsonl`
//!
//! JSONL shape fixture cross-references `crates/studio-router/src/ledger.rs`
//! at HEAD of feature/a2-test-store-corpus. Outcome variants serialise as
//! `"ok"` / `"error_transient"` / `"error_permanent"`; ProviderKind variants
//! as `"anthropic"` / `"openai"` / `"synthetic"`.

mod common;

use studio_store::Store;
use studio_store::ledger::{LedgerEntry, Outcome};

use common::{fresh_studio_root, ledger_path, ok_line, write_jsonl};

/// Pre-populate JSONL with 5 entries, open the Store, expect `recent(5)` to
/// return all 5 with correct fields.
#[tokio::test]
async fn cold_start_imports_jsonl_entries() {
    let (_guard, root) = fresh_studio_root();
    let path = ledger_path(&root);

    let lines: Vec<String> = (1u32..=5)
        .map(|i| ok_line(i, Some(&format!("seq-{i}"))))
        .collect();
    write_jsonl(&path, &lines);

    let store = Store::open(&root).await.expect("Store::open");
    let recent: Vec<LedgerEntry> = store.ledger().recent(5).await.expect("recent(5)");
    assert_eq!(
        recent.len(),
        5,
        "all 5 pre-populated JSONL entries must surface; got {} entries",
        recent.len()
    );
    // Every returned entry must be an Ok outcome (per fixture).
    for e in &recent {
        assert!(
            matches!(e.outcome, Outcome::Ok),
            "fixture wrote outcome=ok only; got {:?}",
            e.outcome
        );
    }
    // task_tag fixture values must round-trip through JSONL→reader.
    let seen_tags: Vec<Option<String>> = recent.iter().map(|e| e.task_tag.clone()).collect();
    let mut sorted_tags = seen_tags
        .into_iter()
        .map(|t| t.unwrap_or_default())
        .collect::<Vec<_>>();
    sorted_tags.sort();
    assert_eq!(
        sorted_tags,
        vec!["seq-1", "seq-2", "seq-3", "seq-4", "seq-5"],
        "task_tag values must survive JSONL→materialised-view round-trip"
    );
}

/// Cold-start with an empty (existing) JSONL file is OK and yields empty
/// recent.
#[tokio::test]
async fn cold_start_with_empty_jsonl_returns_empty() {
    let (_guard, root) = fresh_studio_root();
    let path = ledger_path(&root);
    write_jsonl(&path, &[]);

    let store = Store::open(&root).await.expect("Store::open");
    let recent = store.ledger().recent(10).await.expect("recent(10)");
    assert!(
        recent.is_empty(),
        "empty jsonl → empty recent; got {recent:?}"
    );
}

/// Cold-start with no JSONL file at all is OK (router has not written yet)
/// and yields empty recent.
#[tokio::test]
async fn cold_start_with_missing_jsonl_returns_empty() {
    let (_guard, root) = fresh_studio_root();
    // Do NOT create ledger.jsonl; leave the parent dir present but file absent.
    let store = Store::open(&root).await.expect("Store::open");
    let recent = store
        .ledger()
        .recent(10)
        .await
        .expect("recent on missing JSONL must be Ok(empty)");
    assert!(
        recent.is_empty(),
        "missing jsonl → empty recent; got {recent:?}"
    );
}

/// Cold-start tolerates one trailing partial line in the JSONL (per
/// `studio_router::ledger` docstring: "Readers must tolerate at most one
/// trailing partial line in case of crash mid-write").
#[tokio::test]
async fn cold_start_tolerates_trailing_partial_line() {
    let (_guard, root) = fresh_studio_root();
    let path = ledger_path(&root);

    let mut body = String::new();
    body.push_str(&ok_line(1, Some("complete-1")));
    body.push('\n');
    body.push_str(&ok_line(2, Some("complete-2")));
    body.push('\n');
    // truncated trailing line (no newline, mid-JSON)
    body.push_str(r#"{"ts":"2026-04-30T01:23:99.000Z","task_tag":"trun"#);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, body).expect("write jsonl");

    let store = Store::open(&root).await.expect("Store::open");
    let recent = store.ledger().recent(10).await.expect("recent");
    assert_eq!(
        recent.len(),
        2,
        "partial-trailing-line tolerance: only the 2 complete lines should import"
    );
}

/// Serde-shape compat: hand-rolled JSONL line (byte-for-byte mirroring
/// `studio_router::ledger::LedgerEntry`) must parse to a `LedgerEntry` with
/// the correct field values.
#[tokio::test]
async fn cold_start_serde_shape_compat() {
    let (_guard, root) = fresh_studio_root();
    let path = ledger_path(&root);

    // Use the exact shape studio-router writes today.
    let line = common::make_jsonl_line(
        "2026-04-30T01:23:45.678Z",
        Some("agent-turn"),
        "anthropic_official",
        "anthropic",
        "claude-opus-4-7",
        "blake3:abcd",
        false,
        123,
        456,
        579,
        1234,
        1,
        "ok",
        None,
    );
    write_jsonl(&path, &[line]);

    let store = Store::open(&root).await.expect("Store::open");
    let recent = store.ledger().recent(1).await.expect("recent(1)");
    assert_eq!(recent.len(), 1);
    let e = &recent[0];
    assert_eq!(e.ts, "2026-04-30T01:23:45.678Z");
    assert_eq!(e.task_tag.as_deref(), Some("agent-turn"));
    assert_eq!(e.provider, "anthropic_official");
    assert_eq!(e.model, "claude-opus-4-7");
    assert_eq!(e.cache_key, "blake3:abcd");
    assert!(!e.cache_hit);
    assert_eq!(e.prompt_tokens, 123);
    assert_eq!(e.completion_tokens, 456);
    assert_eq!(e.total_tokens, 579);
    assert_eq!(e.latency_ms, 1234);
    assert_eq!(e.attempt, 1);
    assert!(matches!(e.outcome, Outcome::Ok));
    assert!(e.error_code.is_none());
}

/// Error-outcome lines (transient + permanent) parse to the matching Outcome
/// variant via the snake_case serde rename.
#[tokio::test]
async fn cold_start_parses_error_outcomes() {
    let (_guard, root) = fresh_studio_root();
    let path = ledger_path(&root);

    let transient = common::make_jsonl_line(
        "2026-04-30T01:23:10.000Z",
        None,
        "anthropic_official",
        "anthropic",
        "claude-opus-4-7",
        "blake3:t",
        false,
        0,
        0,
        0,
        5,
        1,
        "error_transient",
        Some("rate-limit"),
    );
    let permanent = common::make_jsonl_line(
        "2026-04-30T01:23:20.000Z",
        None,
        "anthropic_official",
        "anthropic",
        "claude-opus-4-7",
        "blake3:p",
        false,
        0,
        0,
        0,
        5,
        1,
        "error_permanent",
        Some("auth"),
    );
    write_jsonl(&path, &[transient, permanent]);

    let store = Store::open(&root).await.expect("Store::open");
    let recent = store.ledger().recent(2).await.expect("recent(2)");
    assert_eq!(recent.len(), 2);
    let outcomes: Vec<Outcome> = recent.iter().map(|e| e.outcome).collect();
    assert!(outcomes.contains(&Outcome::ErrorTransient));
    assert!(outcomes.contains(&Outcome::ErrorPermanent));
}
