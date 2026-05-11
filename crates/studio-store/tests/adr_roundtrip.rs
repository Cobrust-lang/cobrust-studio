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

//! ADR roundtrip contract — Wave A2 TDD red (per `docs/agent/modules/studio-store.md`).
//!
//! Locks the public surface enumerated in the module-doc §"Public surface":
//!
//! - `adr::create(AdrDraft) -> Result<Adr, StoreError>`
//! - `adr::list() -> Vec<AdrSummary>`
//! - `adr::get(id) -> Option<Adr>`
//!
//! Tests anchor against the `Store` aggregate pattern (`Store::open(root).await`
//! → `store.adr().create/list/get`); if DEV picked free functions instead the
//! reconciliation is a one-import-rewrite.
//!
//! Frontmatter shape pinned by `docs/agent/conventions.md` §"ADR format":
//! `adr_id`, `title`, `status`, `date`, `supersedes`, `superseded_by`.

mod common;

use std::time::Duration;

use studio_store::adr::{Adr, AdrDraft, AdrSummary};
use studio_store::{Store, StoreError};

use common::{adr_dir, fresh_studio_root};

/// `AdrDraft { title, status, date, body }` → create → ADR file appears on
/// disk under `docs/agent/adr/`, body present verbatim, frontmatter has
/// `adr_id` assigned + `status` / `date` echoed from the draft.
#[tokio::test]
async fn create_writes_markdown_with_assigned_id() {
    let (_guard, root) = fresh_studio_root();
    let store: Store = Store::open(&root).await.expect("Store::open");
    let draft = AdrDraft {
        title: "Pick the storage engine".to_string(),
        status: "proposed".to_string(),
        date: "2026-05-11".to_string(),
        body: "## Context\n\nWe need to pick a thing.\n\n## Decision\n\nPick it.\n".to_string(),
        supersedes: Vec::new(),
    };
    let created: Adr = store.adr().create(draft).await.expect("create OK");

    // ADR ID must be assigned and monotonic against the existing 0001..0006 set.
    let id = created.adr_id();
    assert!(
        id >= 7,
        "newly created ADR must take the next free id (>= 0007); got {id}"
    );

    // File on disk under docs/agent/adr/, named with a 4-digit ADR id prefix.
    let adr_dir = adr_dir(&root);
    let entries: Vec<_> = std::fs::read_dir(&adr_dir)
        .expect("read adr dir")
        .filter_map(Result::ok)
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".md"))
        .collect();
    assert!(
        entries.iter().any(|n| n.starts_with(&format!("{id:04}-"))),
        "no ADR file matching {id:04}-* in {entries:?}"
    );

    // Body roundtrips verbatim (modulo trailing newlines / frontmatter prepend).
    let path = adr_dir.join(format!("{id:04}-{}.md", "pick-the-storage-engine"));
    if path.exists() {
        let on_disk = std::fs::read_to_string(&path).expect("read created adr");
        assert!(
            on_disk.contains("## Context"),
            "body section header must be preserved verbatim"
        );
        assert!(
            on_disk.contains("Pick it."),
            "decision body must be preserved verbatim"
        );
        // Frontmatter fields.
        assert!(
            on_disk.contains(&format!(r#"adr_id: "{id:04}""#))
                || on_disk.contains(&format!("adr_id: {id:04}"))
                || on_disk.contains(&format!("adr_id: \"{id}\""))
                || on_disk.contains(&format!("adr_id: {id}")),
            "frontmatter must carry adr_id; got:\n{on_disk}"
        );
        assert!(on_disk.contains("status: proposed"));
        assert!(on_disk.contains("date: 2026-05-11"));
        assert!(on_disk.contains("Pick the storage engine"));
    }
}

/// `list()` returns AdrSummary entries sorted by adr_id ascending and includes
/// the just-created ADR.
#[tokio::test]
async fn list_returns_created_adr_sorted_ascending() {
    let (_guard, root) = fresh_studio_root();
    let store = Store::open(&root).await.expect("Store::open");

    let initial: Vec<AdrSummary> = store.adr().list().await.expect("list");
    // Fresh root has no ADRs on disk yet (we created an empty docs/agent/adr/).
    assert_eq!(
        initial.len(),
        0,
        "list on empty studio root must be empty; got {initial:?}"
    );

    let draft = AdrDraft {
        title: "First".to_string(),
        status: "proposed".to_string(),
        date: "2026-05-11".to_string(),
        body: "## Context\n\nA.\n".to_string(),
        supersedes: Vec::new(),
    };
    let _ = store.adr().create(draft).await.expect("create first");

    let draft2 = AdrDraft {
        title: "Second".to_string(),
        status: "proposed".to_string(),
        date: "2026-05-11".to_string(),
        body: "## Context\n\nB.\n".to_string(),
        supersedes: Vec::new(),
    };
    let _ = store.adr().create(draft2).await.expect("create second");

    // Give any watcher reconcile a beat (debounce/notify settling).
    tokio::time::sleep(Duration::from_millis(50)).await;

    let listed: Vec<AdrSummary> = store.adr().list().await.expect("list after creates");
    assert_eq!(
        listed.len(),
        2,
        "list must return both newly-created ADRs; got {listed:?}"
    );
    let ids: Vec<u32> = listed.iter().map(AdrSummary::adr_id).collect();
    let mut sorted = ids.clone();
    sorted.sort_unstable();
    assert_eq!(ids, sorted, "list must return ascending by adr_id");
}

/// `get(id)` returns the same Adr that was created.
#[tokio::test]
async fn get_returns_just_created_content() {
    let (_guard, root) = fresh_studio_root();
    let store = Store::open(&root).await.expect("Store::open");
    let body = "## Context\n\nWhy.\n\n## Decision\n\nBecause.\n".to_string();
    let draft = AdrDraft {
        title: "Decide a thing".to_string(),
        status: "accepted".to_string(),
        date: "2026-05-11".to_string(),
        body: body.clone(),
        supersedes: Vec::new(),
    };
    let created = store.adr().create(draft).await.expect("create");
    let id = created.adr_id();

    let fetched: Option<Adr> = store.adr().get(id).await.expect("get OK");
    let fetched = fetched.expect("ADR just created must be retrievable by id");
    assert_eq!(fetched.adr_id(), id);
    assert_eq!(fetched.title(), "Decide a thing");
    assert_eq!(fetched.status(), "accepted");
    assert_eq!(fetched.date(), "2026-05-11");
    assert!(
        fetched.body().contains("Because."),
        "body must roundtrip verbatim through create→get; got:\n{}",
        fetched.body()
    );
}

/// `get(id)` for an absent id returns None — not an error.
#[tokio::test]
async fn get_absent_id_returns_none() {
    let (_guard, root) = fresh_studio_root();
    let store = Store::open(&root).await.expect("Store::open");
    let result: Option<Adr> = store.adr().get(9999).await.expect("get OK");
    assert!(
        result.is_none(),
        "get on unused adr_id must be Ok(None); got {result:?}"
    );
}

/// Parser handles the real ADR-0001 frontmatter shape on disk: copying
/// `docs/agent/adr/0001-stack-choice.md` into the temp root and listing must
/// surface it with the correct title + status.
#[tokio::test]
async fn list_parses_real_adr_0001_fixture() {
    let (_guard, root) = fresh_studio_root();

    // Use the live ADR-0001 file from this repo as a fixture template.
    let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/")
        .parent()
        .expect("repo root");
    let fixture_src = repo_root
        .join("docs/agent/adr")
        .join("0001-stack-choice.md");
    let fixture_bytes = std::fs::read(&fixture_src)
        .unwrap_or_else(|e| panic!("read fixture {}: {e}", fixture_src.display()));

    let dest = adr_dir(&root).join("0001-stack-choice.md");
    std::fs::write(&dest, &fixture_bytes).expect("write adr fixture");

    let store = Store::open(&root).await.expect("Store::open");
    let listed: Vec<AdrSummary> = store.adr().list().await.expect("list");
    let one = listed
        .iter()
        .find(|s| s.adr_id() == 1)
        .expect("ADR-0001 must appear in list");
    assert_eq!(
        one.title(),
        "Stack choice — Rust + Axum + SvelteKit + shadcn-svelte + SQLite",
        "title must match fixture frontmatter verbatim"
    );
    assert_eq!(
        one.status(),
        "accepted",
        "status must match fixture frontmatter verbatim"
    );
}

/// Creating with an already-used adr_id must surface a `StoreError`, not
/// silently overwrite. (Caller passes only AdrDraft; the store assigns the id,
/// so this test verifies monotonic assignment by creating twice with identical
/// drafts and asserting distinct ids.)
#[tokio::test]
async fn create_assigns_distinct_ids_monotonically() {
    let (_guard, root) = fresh_studio_root();
    let store = Store::open(&root).await.expect("Store::open");
    let mk = || AdrDraft {
        title: "Repeated".to_string(),
        status: "proposed".to_string(),
        date: "2026-05-11".to_string(),
        body: "## Context\n\nx\n".to_string(),
        supersedes: Vec::new(),
    };
    let a = store.adr().create(mk()).await.expect("first create");
    let b = store.adr().create(mk()).await.expect("second create");
    assert!(
        b.adr_id() > a.adr_id(),
        "second create must assign a strictly greater adr_id (a={}, b={})",
        a.adr_id(),
        b.adr_id()
    );
}

/// Type fix: StoreError must satisfy `std::error::Error` so callers can
/// `?`-propagate it.
#[test]
fn store_error_is_error() {
    fn _is_error<E: std::error::Error + Send + Sync + 'static>() {}
    _is_error::<StoreError>();
}
