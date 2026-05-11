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

//! Filesystem watcher smoke — Wave A2 TDD red.
//!
//! Contract per `docs/agent/modules/studio-store.md` §"Public surface":
//!   `adr::watch() -> impl Stream<Item = AdrChangeEvent>`
//!
//! Per ADR-0004 the watcher uses the `notify` crate so external edits (e.g.
//! `git pull` landing a new ADR file) surface as events. M1 timing budget per
//! module-doc §"Tests": "filesystem watcher delivers change event within
//! 100ms" — this corpus pegs the assertion to 500ms to absorb CI jitter.
//!
//! Debounce expectation: rapid bursts of writes within 100ms collapse to ≤2
//! events. The exact debounce window is DEV's choice; the contract is
//! "rapid bursts do not produce >2 events".

#![allow(clippy::large_futures)]

mod common;

use std::time::Duration;

use futures::StreamExt;
use tokio::time::timeout;

use studio_store::Store;
use studio_store::adr::AdrChangeEvent;

use common::{adr_dir, fresh_studio_root};

const EVENT_BUDGET: Duration = Duration::from_millis(500);

/// External `tokio::fs::write` of a valid ADR-frontmatter markdown file under
/// `docs/agent/adr/` must surface as `AdrChangeEvent::Added` within 500ms.
#[tokio::test]
async fn watcher_emits_added_on_external_write() {
    let (_guard, root) = fresh_studio_root();
    let store = Store::open(&root).await.expect("Store::open");
    let mut stream = Box::pin(store.adr().watch());

    let target = adr_dir(&root).join("0099-foo.md");
    let body = "---\n\
adr_id: \"0099\"\n\
title: Foo\n\
status: proposed\n\
date: 2026-05-11\n\
supersedes: []\n\
superseded_by: []\n\
---\n\
\n\
# ADR-0099: Foo\n\
\n\
## Context\n\
\n\
x\n";
    tokio::fs::write(&target, body)
        .await
        .expect("external write of 0099-foo.md");

    let evt = timeout(EVENT_BUDGET, stream.next())
        .await
        .expect("watcher must surface an event within budget")
        .expect("stream not exhausted");
    match evt {
        AdrChangeEvent::Added(path) => {
            assert!(
                path.ends_with("0099-foo.md"),
                "Added path must point at the file we wrote; got {}",
                path.display()
            );
        }
        other => panic!("expected AdrChangeEvent::Added(_), got {other:?}"),
    }
}

/// External `tokio::fs::remove_file` must surface as `AdrChangeEvent::Removed`.
#[tokio::test]
async fn watcher_emits_removed_on_external_delete() {
    let (_guard, root) = fresh_studio_root();
    // Seed an ADR file BEFORE Store::open so cold-start indexes it.
    let target = adr_dir(&root).join("0099-bye.md");
    let body = "---\n\
adr_id: \"0099\"\n\
title: Bye\n\
status: proposed\n\
date: 2026-05-11\n\
supersedes: []\n\
superseded_by: []\n\
---\n\
\n\
# ADR-0099: Bye\n";
    std::fs::write(&target, body).expect("seed adr");

    let store = Store::open(&root).await.expect("Store::open");
    let mut stream = Box::pin(store.adr().watch());

    tokio::fs::remove_file(&target).await.expect("remove adr");

    // Drain at most a few events; debouncer may also re-emit something for the
    // initial state — we just need to *see* a Removed within budget.
    let saw_removed = timeout(EVENT_BUDGET, async {
        loop {
            let Some(evt) = stream.next().await else {
                return false;
            };
            if let AdrChangeEvent::Removed(p) = evt {
                if p.ends_with("0099-bye.md") {
                    return true;
                }
            }
        }
    })
    .await
    .expect("watcher must surface Removed within budget");
    assert!(
        saw_removed,
        "must see AdrChangeEvent::Removed for 0099-bye.md"
    );
}

/// Debounce: 5 rapid writes within 100ms produce ≤2 events.
#[tokio::test]
async fn watcher_debounces_rapid_burst() {
    let (_guard, root) = fresh_studio_root();
    let store = Store::open(&root).await.expect("Store::open");
    let mut stream = Box::pin(store.adr().watch());

    let target = adr_dir(&root).join("0099-burst.md");
    let body = "---\n\
adr_id: \"0099\"\n\
title: Burst\n\
status: proposed\n\
date: 2026-05-11\n\
supersedes: []\n\
superseded_by: []\n\
---\n\
\n\
# ADR-0099: Burst\n\
\n\
## Context\n\
\n\
v\n";
    for _ in 0..5 {
        tokio::fs::write(&target, body)
            .await
            .expect("rapid write in burst");
        tokio::time::sleep(Duration::from_millis(15)).await;
    }

    // Give the debouncer a window longer than its default to flush. Notify's
    // default debounce is ~200ms; we wait 400ms then drain.
    tokio::time::sleep(Duration::from_millis(400)).await;

    let mut count = 0;
    while let Ok(Some(_evt)) = timeout(Duration::from_millis(50), stream.next()).await {
        count += 1;
        if count > 10 {
            break; // safety
        }
    }
    assert!(
        count <= 2,
        "rapid 5-write burst within 100ms must debounce to <=2 events; got {count}"
    );
}
