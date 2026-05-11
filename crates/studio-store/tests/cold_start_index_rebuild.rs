//! Cold-start SQLite index rebuild — Wave A2 TDD red.
//!
//! ADR-0004 §"Decision" pins: "Markdown as source of truth, SQLite as index."
//! Implication: if `.cobrust-studio/studio.db` is missing or wiped, opening
//! the store must walk the markdown filesystem and repopulate the SQLite
//! materialised view. The user-visible behaviour (`adr::list()` returns the
//! same set of ADRs) must be invariant under index deletion.

mod common;

use studio_store::Store;
use studio_store::adr::AdrSummary;

use common::{adr_dir, fresh_studio_root};

/// 4-digit prefixed ADR markdown — minimal valid frontmatter shape per
/// `docs/agent/conventions.md`.
fn write_adr_fixture(dir: &std::path::Path, id: u32, title: &str, status: &str) {
    let body = format!(
        "---\n\
adr_id: \"{id:04}\"\n\
title: {title}\n\
status: {status}\n\
date: 2026-05-11\n\
supersedes: []\n\
superseded_by: []\n\
---\n\
\n\
# ADR-{id:04}: {title}\n\
\n\
## Context\n\
\n\
Synthetic context for cold-start rebuild test.\n\
\n\
## Decision\n\
\n\
Synthetic decision.\n"
    );
    let slug: String = title
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();
    let path = dir.join(format!("{id:04}-{slug}.md"));
    std::fs::write(&path, body).expect("write adr fixture");
}

/// Pre-populated `docs/agent/adr/` with 3 files + no studio.db ⇒
/// `Store::open` succeeds and `list()` returns all 3.
#[tokio::test]
async fn cold_start_indexes_markdown_when_db_absent() {
    let (_guard, root) = fresh_studio_root();
    let adr_d = adr_dir(&root);
    write_adr_fixture(&adr_d, 1, "First", "proposed");
    write_adr_fixture(&adr_d, 2, "Second", "accepted");
    write_adr_fixture(&adr_d, 3, "Third", "proposed");

    // db must not exist yet.
    let db_path = root.join(".cobrust-studio/studio.db");
    assert!(!db_path.exists(), "test precondition: db must not exist");

    let store = Store::open(&root).await.expect("Store::open must succeed");
    let listed: Vec<AdrSummary> = store.adr().list().await.expect("list");
    assert_eq!(
        listed.len(),
        3,
        "all 3 markdown ADRs must surface via cold-start index rebuild; got {listed:?}"
    );
    let ids: Vec<u32> = listed.iter().map(AdrSummary::adr_id).collect();
    assert!(ids.contains(&1));
    assert!(ids.contains(&2));
    assert!(ids.contains(&3));
}

/// `Store::open` creates the db file (under `.cobrust-studio/`) on first open.
#[tokio::test]
async fn cold_start_creates_db_under_dot_dir() {
    let (_guard, root) = fresh_studio_root();
    write_adr_fixture(&adr_dir(&root), 1, "Solo", "proposed");

    let _store = Store::open(&root).await.expect("Store::open");

    let dot_dir = root.join(".cobrust-studio");
    assert!(
        dot_dir.exists() && dot_dir.is_dir(),
        ".cobrust-studio/ must exist after Store::open"
    );
    // The db file's exact name is DEV's choice (e.g. studio.db); just assert
    // *some* `.db` file landed under `.cobrust-studio/`.
    let any_db = std::fs::read_dir(&dot_dir)
        .expect("readdir .cobrust-studio")
        .filter_map(Result::ok)
        .any(|e| {
            e.path()
                .extension()
                .and_then(|s| s.to_str())
                .is_some_and(|s| s == "db")
        });
    assert!(any_db, "expected a *.db file under .cobrust-studio/");
}

/// Wiping the db file and reopening must re-index markdown — list still
/// returns the same 3 ADRs (markdown is source of truth).
#[tokio::test]
async fn index_rebuild_after_db_wipe_recovers_all_adrs() {
    let (_guard, root) = fresh_studio_root();
    let adr_d = adr_dir(&root);
    write_adr_fixture(&adr_d, 1, "Alpha", "proposed");
    write_adr_fixture(&adr_d, 2, "Beta", "proposed");
    write_adr_fixture(&adr_d, 3, "Gamma", "proposed");

    {
        let store = Store::open(&root).await.expect("first open");
        let initial = store.adr().list().await.expect("list");
        assert_eq!(initial.len(), 3, "first-open list must return all 3");
    }

    // Wipe all .db files under .cobrust-studio/.
    let dot_dir = root.join(".cobrust-studio");
    for entry in std::fs::read_dir(&dot_dir).expect("readdir") {
        let entry = entry.expect("dirent");
        let p = entry.path();
        if p.is_file()
            && p.extension()
                .and_then(|s| s.to_str())
                .is_some_and(|s| s == "db")
        {
            std::fs::remove_file(&p).expect("rm db");
        }
        // Also remove any sidecar -wal / -shm files SQLite might leave behind.
        let name = p
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_string();
        if name.ends_with(".db-wal") || name.ends_with(".db-shm") {
            let _ = std::fs::remove_file(&p);
        }
    }

    // Reopen with markdown intact → index rebuilds → list still returns all 3.
    let store2 = Store::open(&root).await.expect("reopen after db wipe");
    let rebuilt: Vec<AdrSummary> = store2.adr().list().await.expect("list after wipe");
    assert_eq!(
        rebuilt.len(),
        3,
        "after db wipe, markdown source-of-truth must repopulate the index"
    );
}

/// Markdown is source-of-truth: deleting the db then adding a new markdown
/// file outside Studio (e.g. `git pull`) makes the file appear on next list.
#[tokio::test]
async fn cold_start_picks_up_externally_added_markdown() {
    let (_guard, root) = fresh_studio_root();
    let adr_d = adr_dir(&root);
    write_adr_fixture(&adr_d, 1, "Existing", "proposed");

    {
        let _store = Store::open(&root).await.expect("first open");
        // ensure db materialises.
    }

    // External add (mimics git pull landing a new ADR), then drop the db so
    // next open is a clean cold-start.
    write_adr_fixture(&adr_d, 2, "Externally Added", "accepted");
    let dot_dir = root.join(".cobrust-studio");
    for entry in std::fs::read_dir(&dot_dir).expect("readdir") {
        let p = entry.expect("dirent").path();
        if p.is_file()
            && p.extension()
                .and_then(|s| s.to_str())
                .is_some_and(|s| s == "db")
        {
            std::fs::remove_file(&p).expect("rm db");
        }
    }

    let store = Store::open(&root).await.expect("reopen");
    let listed = store.adr().list().await.expect("list");
    assert_eq!(
        listed.len(),
        2,
        "external markdown ADR must appear; got {listed:?}"
    );
    assert!(listed.iter().any(|s| s.adr_id() == 2));
}
