#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::too_many_arguments,
    clippy::case_sensitive_file_extension_comparisons,
    clippy::collapsible_if,
    clippy::redundant_closure_for_method_calls,
    clippy::used_underscore_items,
    clippy::used_underscore_binding,
    clippy::missing_panics_doc
)]

//! `AppState` constructor + invariant smoke — Wave A3 P7-TEST (red).
//!
//! These tests would normally live in `crates/studio-server/src/state.rs`
//! `#[cfg(test)]`, but DEV owns that file on the parallel worktree. The Wave
//! A3 dispatch directs us to exercise the SAME invariants from a TEST-corpus
//! integration test that only sees the public API surface.
//!
//! Anchored to the dispatch's "API names you reference":
//!
//! ```ignore
//! AppState { store, router: Option<...>, project_root, started_at }
//! ```
//!
//! Tests:
//! - `app_state_clone_shares_store`        — Store is `Arc`'d; clone is cheap
//!                                            (verified by behavioural identity,
//!                                            not by raw timing — timing-based
//!                                            cheapness asserts are flaky on CI)
//! - `app_state_started_at_is_in_the_past` — `started_at` is captured at
//!                                            construction (must precede a
//!                                            later `OffsetDateTime::now_utc()`)
//! - `app_state_project_root_matches_input`— public field returns the input
//! - `app_state_router_handle_is_optional` — `None` is accepted by `new`

mod common;

use std::time::Duration;

use studio_server::AppState;
use studio_store::Store;
use time::OffsetDateTime;

#[tokio::test]
async fn app_state_clone_shares_store() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().to_path_buf();
    let store = Store::open(&root).await.expect("Store::open");
    let state = AppState::new(store, None, root.clone());

    let cloned = state.clone();

    // Behavioural identity: both handles point at the same project root, so
    // the Store inside is observably the same logical handle. (Raw-pointer
    // equality on `Arc<StoreInner>` is private to studio-store; we use the
    // project_root pass-through as a stand-in. Verified separately:
    // `studio_store::Store` IS Clone via Arc<Inner> per src/lib.rs:79.)
    assert_eq!(
        cloned.store().project_root(),
        state.store().project_root(),
        "clone must share the same Store project_root",
    );
    assert_eq!(
        cloned.project_root(),
        state.project_root(),
        "clone must report the same project_root",
    );
}

#[tokio::test]
async fn app_state_started_at_is_in_the_past() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().to_path_buf();
    let store = Store::open(&root).await.expect("Store::open");

    let before = OffsetDateTime::now_utc();
    // Tiny sleep to make sure construction's `now_utc()` ≥ `before` even on a
    // monotonic-clock-coarse machine.
    tokio::time::sleep(Duration::from_millis(2)).await;

    let state = AppState::new(store, None, root.clone());

    tokio::time::sleep(Duration::from_millis(2)).await;
    let after = OffsetDateTime::now_utc();

    let started = state.started_at();
    assert!(
        started >= before,
        "started_at ({started}) must be >= pre-construction now ({before})",
    );
    assert!(
        started <= after,
        "started_at ({started}) must be <= post-construction now ({after})",
    );
}

#[tokio::test]
async fn app_state_project_root_matches_input() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().to_path_buf();
    let store = Store::open(&root).await.expect("Store::open");
    let state = AppState::new(store, None, root.clone());

    assert_eq!(
        state.project_root(),
        root.as_path(),
        "AppState.project_root must equal the value passed to AppState::new",
    );
}

#[tokio::test]
async fn app_state_router_handle_is_optional() {
    // The whole point of `router: Option<_>` is that A3 smoke tests can
    // construct an AppState WITHOUT a configured LLM router. If `AppState::new`
    // rejects `None` (e.g. by panicking on a missing router), the M0/M1 smoke
    // surface is unusable. This test pins that contract.
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().to_path_buf();
    let store = Store::open(&root).await.expect("Store::open");

    // Construct with None — must NOT panic.
    let state = AppState::new(store, None, root.clone());

    assert!(
        state.router().is_none(),
        "AppState::new(_, None, _) must produce an AppState whose router() is None",
    );
}
