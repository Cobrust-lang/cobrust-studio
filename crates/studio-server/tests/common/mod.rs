#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::too_many_arguments,
    clippy::case_sensitive_file_extension_comparisons,
    clippy::collapsible_if,
    clippy::redundant_closure_for_method_calls,
    clippy::used_underscore_items,
    clippy::used_underscore_binding,
    clippy::missing_panics_doc,
    clippy::items_after_statements,
    clippy::match_same_arms,
    dead_code
)]

//! Shared test helpers for the Wave A3 studio-server smoke corpus.
//!
//! Anchors the API-shape assumptions documented in
//! `docs/agent/modules/studio-server.md` (M1 §"Public surface") and in the
//! Wave A3 dispatch:
//!
//! ```ignore
//! use studio_server::{AppState, build_router, version};
//! // AppState fields: store, router (Option<Router>), project_root, started_at
//! // build_router(state: AppState) -> axum::Router
//! // version() -> &'static str
//! ```
//!
//! All assumptions live behind these helpers so the merge with the parallel
//! DEV worktree (`feature/a3-dev-server-skel`) is a one-shim change if symbol
//! names drift.

use std::path::PathBuf;
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::http::{Method, Request, Response, StatusCode, header};
use http_body_util::BodyExt;
use serde_json::Value;
use studio_server::{AppState, build_router};
use studio_store::{AdrDraft, FindingDraft, Store};
use tempfile::TempDir;
use tower::ServiceExt;

/// Spin up an `AppState` rooted at a fresh tempdir and build the Axum app.
///
/// Returns the `TempDir` guard alongside the router + the root path so callers
/// can keep the tempdir alive for the test's lifetime and assert against the
/// concrete root path inside response bodies.
pub async fn fresh_app() -> (TempDir, PathBuf, Router) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().to_path_buf();
    let store = Store::open(&root).await.expect("Store::open");
    // The router sub-handle is `None` for smoke tests — A3 only exercises the
    // health/version surface, which must not require a configured LLM router.
    let state = AppState::new(store, None, root.clone());
    let app = build_router(state);
    (tmp, root, app)
}

/// Same as `fresh_app` but returns the `AppState` too so tests can assert on
/// its public fields (e.g. `started_at`, `project_root`, `Clone` cheapness).
pub async fn fresh_app_with_state() -> (TempDir, PathBuf, AppState, Router) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().to_path_buf();
    let store = Store::open(&root).await.expect("Store::open");
    let state = AppState::new(store, None, root.clone());
    let app = build_router(state.clone());
    (tmp, root, state, app)
}

/// Drive a single request through the router using `tower::ServiceExt::oneshot`.
///
/// `oneshot` consumes the router clone — we always pass a fresh `.clone()` so
/// callers can re-issue requests against the same logical app.
pub async fn oneshot_get(app: &Router, uri: &str) -> Response<Body> {
    let req = Request::builder()
        .method(Method::GET)
        .uri(uri)
        .body(Body::empty())
        .expect("build request");
    app.clone().oneshot(req).await.expect("oneshot")
}

/// As `oneshot_get` but for arbitrary methods (used by the fallback corpus).
pub async fn oneshot_method(app: &Router, method: Method, uri: &str) -> Response<Body> {
    let req = Request::builder()
        .method(method)
        .uri(uri)
        .body(Body::empty())
        .expect("build request");
    app.clone().oneshot(req).await.expect("oneshot")
}

/// Drain a response body to bytes and parse as JSON.
///
/// Panics with a diagnostic message if the body is not valid JSON — tests use
/// this to convert non-JSON responses (e.g. a stray text/plain fallback) into
/// a clear failure signal.
pub async fn json_body(resp: Response<Body>) -> Value {
    let (parts, body) = resp.into_parts();
    let bytes = body.collect().await.expect("collect body bytes").to_bytes();
    serde_json::from_slice::<Value>(&bytes).unwrap_or_else(|e| {
        panic!(
            "response body is not JSON (status={}, ct={:?}, len={}, err={e}): {:?}",
            parts.status,
            parts.headers.get(axum::http::header::CONTENT_TYPE),
            bytes.len(),
            String::from_utf8_lossy(&bytes),
        )
    })
}

/// Split a response into `(StatusCode, Value)` for ergonomic asserts.
pub async fn status_and_json(resp: Response<Body>) -> (StatusCode, Value) {
    let status = resp.status();
    let body = json_body(resp).await;
    (status, body)
}

/// POST a JSON body to `uri` and drive it through the router via
/// `tower::ServiceExt::oneshot`. The body is serialised from `value` and the
/// `content-type: application/json` header is set so DEV's JSON extractor
/// accepts the payload.
pub async fn oneshot_post_json(app: &Router, uri: &str, value: &Value) -> Response<Body> {
    let bytes = serde_json::to_vec(value).expect("encode JSON body");
    let req = Request::builder()
        .method(Method::POST)
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(bytes))
        .expect("build request");
    app.clone().oneshot(req).await.expect("oneshot")
}

/// POST a raw-byte body — used by malformed-body negative tests where the
/// payload is intentionally not valid JSON.
pub async fn oneshot_post_bytes(
    app: &Router,
    uri: &str,
    content_type: &str,
    bytes: Vec<u8>,
) -> Response<Body> {
    let req = Request::builder()
        .method(Method::POST)
        .uri(uri)
        .header(header::CONTENT_TYPE, content_type)
        .body(Body::from(bytes))
        .expect("build request");
    app.clone().oneshot(req).await.expect("oneshot")
}

/// Boot a fresh `AppState` with `router: None` and an empty store.
///
/// Returns `(TempDir, project_root, app)`. The `TempDir` guard must be kept
/// alive for the test's lifetime (the SQLite + docs/ dirs live under it).
pub async fn boot_app_with_empty_store() -> (TempDir, PathBuf, Router) {
    fresh_app().await
}

/// Boot a fresh `AppState`, also returning the underlying `Store` so the test
/// can pre-populate / verify state via the store API directly. This is the
/// preferred shape for round-trip tests (POST via HTTP → confirm with store).
pub async fn boot_app_with_store() -> (TempDir, PathBuf, Store, Router) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().to_path_buf();
    let store = Store::open(&root).await.expect("Store::open");
    // Clone the store so callers can keep using it AND the app can hold its
    // own handle — `Store` is `Clone` via `Arc<Inner>` so this is cheap.
    let state = AppState::new(store.clone(), None, root.clone());
    let app = build_router(state);
    (tmp, root, store, app)
}

/// Boot a fresh `AppState` seeded with `n` ADRs.
///
/// Each seeded ADR uses a deterministic title (`"Seeded ADR <i>"`) so tests
/// can correlate the listing back to its index.
pub async fn boot_app_with_seeded_adrs(n: usize) -> (TempDir, PathBuf, Store, Router) {
    let (tmp, root, store, app) = boot_app_with_store().await;
    for i in 0..n {
        let draft = AdrDraft {
            title: format!("Seeded ADR {i}"),
            status: "proposed".to_string(),
            date: "2026-05-12".to_string(),
            body: format!("## Context\n\nSeeded for test {i}\n"),
            supersedes: Vec::new(),
        };
        store.adr().create(draft).await.expect("seed adr");
    }
    (tmp, root, store, app)
}

/// Boot a fresh `AppState` seeded with `n` findings.
pub async fn boot_app_with_seeded_findings(n: usize) -> (TempDir, PathBuf, Store, Router) {
    let (tmp, root, store, app) = boot_app_with_store().await;
    for i in 0..n {
        let draft = FindingDraft {
            finding_id: format!("seed-{i:03}"),
            last_verified_commit: "0000000".to_string(),
            severity: "P3".to_string(),
            status: "open".to_string(),
            dependencies: vec!["adr:0006".to_string()],
            related: Vec::new(),
            title: format!("Seeded finding {i}"),
            body: format!("# Seeded finding {i}\n\nbody\n"),
        };
        store.finding().create(draft).await.expect("seed finding");
    }
    (tmp, root, store, app)
}

/// Drain (up to `max_bytes`) of an SSE response body or stop at `timeout`,
/// whichever comes first. Returns the accumulated bytes as a `String`.
///
/// SSE event framing is `event: <type>\ndata: <json>\n\n`; we don't parse
/// the framing here — tests scan for the substrings they care about.
pub async fn read_sse_body_with_timeout(
    resp: Response<Body>,
    timeout: Duration,
    max_bytes: usize,
) -> String {
    let (_, body) = resp.into_parts();
    let mut buf: Vec<u8> = Vec::new();
    let mut stream = body.into_data_stream();
    let deadline = tokio::time::Instant::now() + timeout;
    use futures::StreamExt;
    loop {
        let now = tokio::time::Instant::now();
        if now >= deadline || buf.len() >= max_bytes {
            break;
        }
        let remaining = deadline - now;
        match tokio::time::timeout(remaining, stream.next()).await {
            Ok(Some(Ok(chunk))) => {
                buf.extend_from_slice(&chunk);
            }
            Ok(Some(Err(_)) | None) => break,
            Err(_) => break, // timed out reading next chunk
        }
    }
    String::from_utf8_lossy(&buf).into_owned()
}

/// Build a GET request and return the raw `Response<Body>` so callers can
/// stream the body chunk-by-chunk (used by the SSE tests).
pub async fn oneshot_get_stream(app: &Router, uri: &str) -> Response<Body> {
    oneshot_get(app, uri).await
}
