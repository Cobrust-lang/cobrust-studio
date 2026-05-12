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

//! `GET /api/ledger/recent` integration contract — Wave A4 P7-TEST (red).
//!
//! Locks the wire-shape binding for the ledger route per
//! `docs/agent/modules/studio-server.md` §"Wave A4 target" and ADR-0006
//! §"Addendum 2026-05-11" F-02 (router JSONL is source-of-truth, store reads).
//!
//! - `GET /api/ledger/recent?n=20` → `{ "entries": [LedgerEntry, ...] }`
//!
//! API-shape assumptions made (DEV must satisfy; CTO reconciles drift):
//!
//! 1. **List envelope.** `{"entries": [...]}` (preferred) or a bare array
//!    at the top level (tolerated).
//! 2. **Entry shape.** Each entry carries the
//!    `studio_router::ledger::LedgerEntry` serde wire shape (see the module
//!    docstring) — at minimum `ts`, `provider`, `model`, `cache_key`,
//!    `outcome`.
//! 3. **Default `n`.** When `n` is omitted, default to 20 (or any sensible
//!    constant) — we only assert that ≤20 items come back.
//! 4. **`?n=` cap.** `n` above 1000 is clamped to 1000 server-side.
//! 5. **Reverse-chronological.** Newest first (newer `ts` comes earlier).
//! 6. **Cold-start sync.** The route serves entries from the SQLite view
//!    that `Store::open` populates from the router JSONL. Tests pre-seed
//!    via `studio_router::Ledger::append` and a fresh `Store::open` to
//!    drive `sync_from_jsonl`.

mod common;

use std::path::Path;

use axum::http::StatusCode;
use common::{boot_app_with_empty_store, boot_app_with_store, oneshot_get, status_and_json};
use serde_json::Value;
use studio_router::Ledger as RouterLedger;
use studio_router::ledger::{LedgerEntry, Outcome};
use studio_server::{AppState, build_router};
use studio_store::{LEDGER_JSONL_PATH, Store};

/// Pluck the entries array from a `{"entries": [...]}` envelope, accepting a
/// bare array as a fall-through.
fn extract_entries(body: &Value) -> Vec<Value> {
    if let Some(arr) = body.get("entries").and_then(|v| v.as_array()) {
        return arr.clone();
    }
    if let Some(arr) = body.as_array() {
        return arr.clone();
    }
    panic!("ledger body must be `{{ \"entries\": [...] }}` or bare array: {body}");
}

/// Build a synthetic `LedgerEntry` with a known `ts` so the reverse-chrono
/// assertion has a stable order to compare against. `i` is a small index
/// that makes the timestamp monotonic (`2026-05-12T00:00:0{i}Z`).
fn synth_entry(i: u32, model: &str) -> LedgerEntry {
    LedgerEntry {
        ts: format!("2026-05-12T00:00:{i:02}Z"),
        task_tag: Some("test".to_string()),
        provider: "anthropic_official".to_string(),
        provider_kind: None,
        model: model.to_string(),
        cache_key: format!("blake3:test-{i}"),
        cache_hit: false,
        prompt_tokens: 10 * i,
        completion_tokens: 5 * i,
        total_tokens: 15 * i,
        latency_ms: 1000,
        attempt: 1,
        outcome: Outcome::Ok,
        error_code: None,
    }
}

/// Pre-populate the router JSONL at `root` with `count` synthetic entries
/// using the canonical `studio_router::Ledger::append`. This is the
/// canonical writer path per F-02; the `Store::open` cold-start sync then
/// imports them into the SQLite view.
async fn seed_router_jsonl(root: &Path, count: u32) -> Vec<LedgerEntry> {
    let jsonl_path = root.join(LEDGER_JSONL_PATH);
    let ledger = RouterLedger::open(jsonl_path)
        .await
        .expect("RouterLedger::open");
    let mut entries = Vec::with_capacity(count as usize);
    for i in 0..count {
        let entry = synth_entry(i, &format!("claude-{i}"));
        ledger.append(&entry).await.expect("router ledger append");
        entries.push(entry);
    }
    entries
}

/// Boot a fresh app where the router JSONL has already been written by
/// `studio_router::Ledger` BEFORE `Store::open`, so `Store::open`'s cold
/// start runs `sync_from_jsonl` and the materialised view is hydrated.
async fn boot_app_with_seeded_ledger(
    count: u32,
) -> (tempfile::TempDir, std::path::PathBuf, axum::Router) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().to_path_buf();
    // The router JSONL goes inside `<root>/.cobrust-studio/router/`; create
    // the directory tree first.
    let jsonl_parent = root.join(".cobrust-studio").join("router");
    tokio::fs::create_dir_all(&jsonl_parent)
        .await
        .expect("mkdir router/");
    let _entries = seed_router_jsonl(&root, count).await;
    // Now open the store — `Store::open` will sync JSONL → SQLite view.
    let store = Store::open(&root).await.expect("Store::open");
    let state = AppState::new(store, None, root.clone());
    let app = build_router(state);
    (tmp, root, app)
}

#[tokio::test]
async fn ledger_recent_empty_returns_200_empty_array() {
    let (_tmp, _root, app) = boot_app_with_empty_store().await;

    let resp = oneshot_get(&app, "/api/ledger/recent").await;
    let (status, body) = status_and_json(resp).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "GET /api/ledger/recent on empty store must return 200: body={body}",
    );
    let entries = extract_entries(&body);
    assert!(
        entries.is_empty(),
        "fresh store must yield empty ledger: {entries:?}",
    );
}

#[tokio::test]
async fn ledger_recent_after_router_jsonl_writes() {
    // Pre-seed the JSONL via the canonical writer; `Store::open`'s
    // cold-start `sync_from_jsonl` should hydrate the view.
    let (_tmp, _root, app) = boot_app_with_seeded_ledger(3).await;

    let resp = oneshot_get(&app, "/api/ledger/recent").await;
    let (status, body) = status_and_json(resp).await;
    assert_eq!(status, StatusCode::OK);
    let entries = extract_entries(&body);
    assert_eq!(
        entries.len(),
        3,
        "all 3 seeded JSONL entries must surface via the route: {entries:?}",
    );

    // Reverse-chronological: the first entry must be the newest seeded one
    // (synth_entry uses `00:00:0{i}Z` so i=2 is newest).
    let first_ts = entries[0]
        .get("ts")
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| panic!("first entry must have `ts`: {entries:?}"));
    let last_ts = entries[entries.len() - 1]
        .get("ts")
        .and_then(|v| v.as_str())
        .expect("last entry has ts");
    assert!(
        first_ts >= last_ts,
        "entries must be reverse-chronological: first_ts={first_ts}, last_ts={last_ts}",
    );
}

#[tokio::test]
async fn ledger_recent_with_n_query_param() {
    // Seed 10 entries, request only 5 → must get exactly 5.
    let (_tmp, _root, app) = boot_app_with_seeded_ledger(10).await;

    let resp = oneshot_get(&app, "/api/ledger/recent?n=5").await;
    let (status, body) = status_and_json(resp).await;
    assert_eq!(status, StatusCode::OK);
    let entries = extract_entries(&body);
    assert_eq!(
        entries.len(),
        5,
        "`?n=5` must limit the response to 5 entries: got {}",
        entries.len(),
    );
}

#[tokio::test]
async fn ledger_recent_cap_at_1000() {
    // The cap protects the SQLite view from a runaway query; even when
    // the caller asks for a huge `n`, the server clamps to 1000.
    // We only seed a small number so we just verify the route doesn't 500.
    let (_tmp, _root, app) = boot_app_with_seeded_ledger(2).await;

    let resp = oneshot_get(&app, "/api/ledger/recent?n=99999").await;
    let (status, body) = status_and_json(resp).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "huge `?n=` must clamp server-side, not 500: body={body}",
    );
    let entries = extract_entries(&body);
    // We seeded only 2; cap of 1000 means we get back at most 1000 — here 2.
    assert!(
        entries.len() <= 1000,
        "clamp must hold even when caller asks for more: got {}",
        entries.len(),
    );
    assert_eq!(entries.len(), 2);
}

#[tokio::test]
async fn ledger_recent_n_zero_returns_empty_list() {
    // `?n=0` is a degenerate but well-defined case: return nothing.
    let (_tmp, _root, app) = boot_app_with_seeded_ledger(3).await;

    let resp = oneshot_get(&app, "/api/ledger/recent?n=0").await;
    let (status, body) = status_and_json(resp).await;
    assert_eq!(status, StatusCode::OK);
    let entries = extract_entries(&body);
    assert!(
        entries.is_empty(),
        "`?n=0` must return empty list, got {entries:?}",
    );
}

#[tokio::test]
async fn ledger_route_appends_dispatched_entries_are_recovered() {
    // A more end-to-end binding: append to BOTH the router JSONL AND the
    // store's materialised view (matching the dispatch path the server
    // will use). The route should surface every entry.
    let (_tmp, root, store, app) = boot_app_with_store().await;

    let jsonl_parent = root.join(".cobrust-studio").join("router");
    tokio::fs::create_dir_all(&jsonl_parent)
        .await
        .expect("mkdir router/");
    let jsonl_path = root.join(LEDGER_JSONL_PATH);
    let router_ledger = RouterLedger::open(jsonl_path)
        .await
        .expect("RouterLedger::open");

    for i in 0..2 {
        let entry = synth_entry(i, "claude-test");
        router_ledger.append(&entry).await.expect("router append");
        store.ledger().append(&entry).await.expect("store append");
    }

    let resp = oneshot_get(&app, "/api/ledger/recent?n=10").await;
    let (status, body) = status_and_json(resp).await;
    assert_eq!(status, StatusCode::OK);
    let entries = extract_entries(&body);
    assert_eq!(
        entries.len(),
        2,
        "both appended entries must surface via the route: {entries:?}",
    );
}
