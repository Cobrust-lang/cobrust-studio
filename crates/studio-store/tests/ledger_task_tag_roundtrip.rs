#![allow(
    clippy::unwrap_used,
    clippy::too_many_arguments,
    clippy::case_sensitive_file_extension_comparisons,
    clippy::collapsible_if,
    clippy::redundant_closure_for_method_calls,
    clippy::used_underscore_items,
    clippy::used_underscore_binding,
    clippy::missing_panics_doc
)]

//! `task_tag: Option<String>` roundtrip across JSONL → SQLite → recent.
//!
//! Cross-references the studio-router unit-test `task_tag_round_trips_when_*`
//! in `crates/studio-router/src/ledger.rs`. The router test pins serde shape
//! at the source. This test pins the same shape on the *reader* side: studio-
//! store's materialised view must produce identical `task_tag` values.
//!
//! ADR-0006 strip #4: generalised the upstream Cobrust `task: String` enum-key
//! to a free-form `Option<String>`.

mod common;

use studio_store::Store;
use studio_store::ledger::LedgerEntry;

use common::{fresh_studio_root, ledger_path, write_jsonl};

/// `task_tag: Some("agent-turn")` round-trips through JSONL → reader → recent.
#[tokio::test]
async fn task_tag_some_round_trips() {
    let (_guard, root) = fresh_studio_root();
    let path = ledger_path(&root);
    let line = common::make_jsonl_line(
        "2026-04-30T01:23:45.000Z",
        Some("agent-turn"),
        "anthropic_official",
        "anthropic",
        "claude-opus-4-7",
        "blake3:00",
        false,
        10,
        20,
        30,
        100,
        1,
        "ok",
        None,
    );
    write_jsonl(&path, &[line]);

    let store = Store::open(&root).await.expect("Store::open");
    let recent: Vec<LedgerEntry> = store.ledger().recent(1).await.expect("recent(1)");
    assert_eq!(recent.len(), 1);
    assert_eq!(
        recent[0].task_tag.as_deref(),
        Some("agent-turn"),
        "task_tag Some(\"agent-turn\") must survive JSONL → reader round-trip"
    );
}

/// `task_tag: None` round-trips. JSONL writes `"task_tag":null`; reader must
/// surface `None` (NOT `Some("")` or `Some("null")`).
#[tokio::test]
async fn task_tag_none_round_trips() {
    let (_guard, root) = fresh_studio_root();
    let path = ledger_path(&root);
    let line = common::make_jsonl_line(
        "2026-04-30T01:23:45.000Z",
        None,
        "anthropic_official",
        "anthropic",
        "claude-opus-4-7",
        "blake3:00",
        false,
        10,
        20,
        30,
        100,
        1,
        "ok",
        None,
    );
    write_jsonl(&path, &[line]);

    let store = Store::open(&root).await.expect("Store::open");
    let recent = store.ledger().recent(1).await.expect("recent(1)");
    assert_eq!(recent.len(), 1);
    assert!(
        recent[0].task_tag.is_none(),
        "task_tag null in JSONL must map to None; got {:?}",
        recent[0].task_tag
    );
}

/// Mixed Some/None in same JSONL stream: both round-trip, ordering preserved.
#[tokio::test]
async fn task_tag_mixed_some_none_in_stream() {
    let (_guard, root) = fresh_studio_root();
    let path = ledger_path(&root);

    let lines = vec![
        common::make_jsonl_line(
            "2026-04-30T01:23:01.000Z",
            Some("tag-a"),
            "anthropic_official",
            "anthropic",
            "m",
            "blake3:01",
            false,
            1,
            1,
            2,
            10,
            1,
            "ok",
            None,
        ),
        common::make_jsonl_line(
            "2026-04-30T01:23:02.000Z",
            None,
            "anthropic_official",
            "anthropic",
            "m",
            "blake3:02",
            false,
            1,
            1,
            2,
            10,
            1,
            "ok",
            None,
        ),
        common::make_jsonl_line(
            "2026-04-30T01:23:03.000Z",
            Some("tag-c"),
            "anthropic_official",
            "anthropic",
            "m",
            "blake3:03",
            false,
            1,
            1,
            2,
            10,
            1,
            "ok",
            None,
        ),
    ];
    write_jsonl(&path, &lines);

    let store = Store::open(&root).await.expect("Store::open");
    let recent = store.ledger().recent(10).await.expect("recent(10)");
    assert_eq!(recent.len(), 3);
    // Reverse-chronological: tag-c, None, tag-a
    assert_eq!(recent[0].task_tag.as_deref(), Some("tag-c"));
    assert!(recent[1].task_tag.is_none());
    assert_eq!(recent[2].task_tag.as_deref(), Some("tag-a"));
}
