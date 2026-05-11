//! Finding roundtrip contract — Wave A2 TDD red.
//!
//! Mirrors `adr_roundtrip` but for `finding::{list,get,create}` and exercises
//! the distinct frontmatter shape from `docs/agent/conventions.md` §"Finding
//! format": `doc_kind: finding`, `finding_id`, `last_verified_commit`,
//! `severity`, `status`, `dependencies`, `related`.
//!
//! Fixture template: the live finding `a1-1-strip-2-noop-at-pin-61f2aff.md` in
//! this repo, used to verify the parser respects the real on-disk shape.

mod common;

use studio_store::Store;
use studio_store::finding::{Finding, FindingDraft, FindingSummary};

use common::{finding_dir, fresh_studio_root};

/// `create(FindingDraft { .. }) -> Result<Finding, StoreError>` writes the
/// markdown file under `docs/agent/findings/` with the expected frontmatter.
#[tokio::test]
async fn create_writes_finding_markdown() {
    let (_guard, root) = fresh_studio_root();
    let store = Store::open(&root).await.expect("Store::open");

    let draft = FindingDraft {
        finding_id: "a2-1-roundtrip-smoke".to_string(),
        last_verified_commit: "deadbeef".to_string(),
        severity: "P2".to_string(),
        status: "open".to_string(),
        dependencies: vec!["adr:0004".to_string()],
        related: Vec::new(),
        title: "Roundtrip smoke".to_string(),
        body: "## Hypothesis\n\nIt round-trips.\n\n## Result\n\nIt did.\n".to_string(),
    };
    let created: Finding = store.finding().create(draft).await.expect("create OK");
    assert_eq!(created.finding_id(), "a2-1-roundtrip-smoke");
    assert_eq!(created.severity(), "P2");
    assert_eq!(created.status(), "open");

    // File on disk under docs/agent/findings/, named by finding_id.
    let dir = finding_dir(&root);
    let entries: Vec<_> = std::fs::read_dir(&dir)
        .expect("readdir findings")
        .filter_map(Result::ok)
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    assert!(
        entries
            .iter()
            .any(|n| n.contains("a2-1-roundtrip-smoke") && n.ends_with(".md")),
        "finding markdown for {} missing among {entries:?}",
        "a2-1-roundtrip-smoke"
    );
}

/// `list()` returns FindingSummary entries.
#[tokio::test]
async fn list_returns_created_finding() {
    let (_guard, root) = fresh_studio_root();
    let store = Store::open(&root).await.expect("Store::open");

    let initial: Vec<FindingSummary> = store.finding().list().await.expect("list");
    assert_eq!(initial.len(), 0, "empty studio root → empty finding list");

    let _ = store
        .finding()
        .create(FindingDraft {
            finding_id: "a2-1-listed".to_string(),
            last_verified_commit: "0000".to_string(),
            severity: "P3".to_string(),
            status: "open".to_string(),
            dependencies: Vec::new(),
            related: Vec::new(),
            title: "Listed".to_string(),
            body: "## Hypothesis\n\nx\n".to_string(),
        })
        .await
        .expect("create");

    let listed = store.finding().list().await.expect("list after create");
    assert_eq!(listed.len(), 1);
    let one = &listed[0];
    assert_eq!(one.finding_id(), "a2-1-listed");
    assert_eq!(one.severity(), "P3");
    assert_eq!(one.status(), "open");
}

/// `get(finding_id)` returns the same finding that was created.
#[tokio::test]
async fn get_returns_just_created_finding() {
    let (_guard, root) = fresh_studio_root();
    let store = Store::open(&root).await.expect("Store::open");
    let body = "## Hypothesis\n\nA.\n\n## Result\n\nB.\n".to_string();
    let _ = store
        .finding()
        .create(FindingDraft {
            finding_id: "a2-1-get".to_string(),
            last_verified_commit: "abcd".to_string(),
            severity: "P1".to_string(),
            status: "open".to_string(),
            dependencies: vec!["adr:0006".to_string()],
            related: Vec::new(),
            title: "Get me".to_string(),
            body: body.clone(),
        })
        .await
        .expect("create");

    let fetched: Option<Finding> = store.finding().get("a2-1-get").await.expect("get OK");
    let fetched = fetched.expect("finding must round-trip");
    assert_eq!(fetched.finding_id(), "a2-1-get");
    assert_eq!(fetched.severity(), "P1");
    assert_eq!(fetched.dependencies(), &["adr:0006".to_string()]);
    assert!(fetched.body().contains("Result"));
}

/// `get(missing)` is `Ok(None)`, not an error.
#[tokio::test]
async fn get_absent_finding_returns_none() {
    let (_guard, root) = fresh_studio_root();
    let store = Store::open(&root).await.expect("Store::open");
    let result = store.finding().get("does-not-exist").await.expect("get OK");
    assert!(result.is_none(), "got {result:?}");
}

/// Parser respects the real on-disk finding frontmatter: severity, status,
/// dependencies must round-trip through `list()`.
#[tokio::test]
async fn list_parses_real_finding_fixture() {
    let (_guard, root) = fresh_studio_root();

    let repo_root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates")
        .parent()
        .expect("repo root");
    let fixture_src = repo_root
        .join("docs/agent/findings")
        .join("a1-1-strip-2-noop-at-pin-61f2aff.md");
    let body = std::fs::read(&fixture_src)
        .unwrap_or_else(|e| panic!("read finding fixture {}: {e}", fixture_src.display()));
    let dst = finding_dir(&root).join("a1-1-strip-2-noop-at-pin-61f2aff.md");
    std::fs::write(&dst, &body).expect("write finding fixture");

    let store = Store::open(&root).await.expect("Store::open");
    let listed: Vec<FindingSummary> = store.finding().list().await.expect("list");
    let one = listed
        .iter()
        .find(|s| s.finding_id() == "a1-1-strip-2-noop-at-pin-61f2aff")
        .expect("finding fixture must appear in list");
    assert_eq!(
        one.severity(),
        "P3",
        "severity must match fixture frontmatter verbatim"
    );
    assert_eq!(
        one.status(),
        "closed_by_a1.1",
        "status must match fixture frontmatter verbatim"
    );
}
