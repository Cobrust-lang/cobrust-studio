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

use axum::Router;
use axum::body::Body;
use axum::http::{Method, Request, Response, StatusCode};
use http_body_util::BodyExt;
use serde_json::Value;
use studio_server::{AppState, build_router};
use studio_store::Store;
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
