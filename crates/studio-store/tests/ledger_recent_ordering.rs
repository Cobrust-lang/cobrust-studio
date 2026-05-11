//! Ledger `recent(n)` ordering & bounds — Wave A2 TDD red.
//!
//! Contract per `docs/agent/modules/studio-store.md` §"Public surface":
//!   `ledger::recent(n: usize) -> Vec<LedgerEntry>`
//!
//! `recent(n)` returns up to the `n` most-recent entries in reverse
//! chronological order. Bounds:
//!   - `recent(0)` is empty
//!   - `recent(n)` on a ledger with `m < n` entries returns all `m`
//!   - `recent(n)` returns them most-recent-first by ledger timestamp

mod common;

use studio_store::Store;
use studio_store::ledger::LedgerEntry;

use common::{fresh_studio_root, ledger_path, ok_line, write_jsonl};

/// `recent(0)` on any ledger returns empty.
#[tokio::test]
async fn recent_zero_is_empty() {
    let (_guard, root) = fresh_studio_root();
    let path = ledger_path(&root);
    let lines: Vec<String> = (1u32..=5).map(|i| ok_line(i, None)).collect();
    write_jsonl(&path, &lines);

    let store = Store::open(&root).await.expect("Store::open");
    let recent = store.ledger().recent(0).await.expect("recent(0)");
    assert!(recent.is_empty(), "recent(0) must be empty; got {recent:?}");
}

/// `recent(100)` on a ledger with 5 entries returns all 5 (saturating).
#[tokio::test]
async fn recent_larger_than_ledger_returns_all_entries() {
    let (_guard, root) = fresh_studio_root();
    let path = ledger_path(&root);
    let lines: Vec<String> = (1u32..=5).map(|i| ok_line(i, None)).collect();
    write_jsonl(&path, &lines);

    let store = Store::open(&root).await.expect("Store::open");
    let recent = store.ledger().recent(100).await.expect("recent(100)");
    assert_eq!(
        recent.len(),
        5,
        "recent(100) on 5-entry ledger must saturate at 5; got {}",
        recent.len()
    );
}

/// After 10 entries appended (in monotonically increasing ts order), `recent(5)`
/// returns the 5 most-recent — i.e. seq 6..=10 in reverse-chronological order.
#[tokio::test]
async fn recent_five_returns_last_five_reverse_chronological() {
    let (_guard, root) = fresh_studio_root();
    let path = ledger_path(&root);
    // ok_line uses ts seconds equal to `seq`, so seq=10 is strictly latest.
    let lines: Vec<String> = (1u32..=10)
        .map(|i| ok_line(i, Some(&format!("seq-{i:02}"))))
        .collect();
    write_jsonl(&path, &lines);

    let store = Store::open(&root).await.expect("Store::open");
    let recent: Vec<LedgerEntry> = store.ledger().recent(5).await.expect("recent(5)");
    assert_eq!(
        recent.len(),
        5,
        "recent(5) must return exactly 5; got {recent:?}"
    );

    let tags: Vec<String> = recent
        .iter()
        .map(|e| e.task_tag.clone().unwrap_or_default())
        .collect();
    // Most-recent first.
    assert_eq!(
        tags,
        vec!["seq-10", "seq-09", "seq-08", "seq-07", "seq-06"],
        "recent(5) must be reverse-chronological (newest first)"
    );
}

/// `recent(n)` is monotonic in `n`: `recent(n)` is a prefix of `recent(n+k)` in
/// reverse-chronological order.
#[tokio::test]
async fn recent_is_monotonic_prefix() {
    let (_guard, root) = fresh_studio_root();
    let path = ledger_path(&root);
    let lines: Vec<String> = (1u32..=8)
        .map(|i| ok_line(i, Some(&format!("seq-{i:02}"))))
        .collect();
    write_jsonl(&path, &lines);

    let store = Store::open(&root).await.expect("Store::open");
    let three = store.ledger().recent(3).await.expect("recent(3)");
    let six = store.ledger().recent(6).await.expect("recent(6)");
    assert_eq!(three.len(), 3);
    assert_eq!(six.len(), 6);
    let three_tags: Vec<_> = three.iter().map(|e| e.task_tag.clone()).collect();
    let six_prefix: Vec<_> = six.iter().take(3).map(|e| e.task_tag.clone()).collect();
    assert_eq!(
        three_tags, six_prefix,
        "recent(3) must equal the 3-prefix of recent(6)"
    );
}

/// `recent(1)` returns exactly the latest entry.
#[tokio::test]
async fn recent_one_returns_latest_only() {
    let (_guard, root) = fresh_studio_root();
    let path = ledger_path(&root);
    let lines: Vec<String> = (1u32..=4)
        .map(|i| ok_line(i, Some(&format!("seq-{i}"))))
        .collect();
    write_jsonl(&path, &lines);

    let store = Store::open(&root).await.expect("Store::open");
    let recent = store.ledger().recent(1).await.expect("recent(1)");
    assert_eq!(recent.len(), 1);
    assert_eq!(recent[0].task_tag.as_deref(), Some("seq-4"));
}
