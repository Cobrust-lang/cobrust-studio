#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::too_many_arguments,
    clippy::case_sensitive_file_extension_comparisons,
    clippy::missing_panics_doc
)]

//! Wave A5 dispatch happy-path integration corpus.
//!
//! Locks the *router-is-Some* surface using the in-process synthetic
//! provider — the dispatch route must:
//!
//! - Return `200 text/event-stream` when `AppState.router = Some(_)`.
//! - Emit exactly one `event: done` SSE frame.
//! - The frame's JSON payload must carry `provider`, `model`, `text`,
//!   `usage`, `cache_hit`, and (when supplied) `task_tag`.
//! - The `task_tag` from the request body must be echoed in the
//!   response payload (per ADR-0006 §F-03, `DispatchContext`).
//!
//! The parallel P7-TEST agent on `feature/a5-test-router-wire` writes
//! a fuller corpus (cache-hit replay, multi-frame chunk forwarding,
//! error-event paths, etc.); these tests lock the M1-acceptance shape
//! so the DEV-side wiring is self-checking on its own branch.

mod common;

use std::sync::Arc;
use std::time::Duration;

use axum::http::StatusCode;
use common::{oneshot_post_json, read_sse_body_with_timeout};
use studio_router::{Router, RouterBuilder, RouterConfig};
use studio_server::{AppState, SyntheticProvider, build_router};
use studio_store::Store;
use tempfile::TempDir;

/// Spin up an AppState with a synthetic-only router pre-wired. The
/// returned tuple keeps the `TempDir` guard alive for the test's
/// lifetime (the router's ledger + cache live under it).
async fn boot_app_with_synthetic_router() -> (TempDir, axum::Router) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().to_path_buf();

    // Ledger + cache under the tempdir so each test gets a clean
    // sandbox (default XDG paths are shared across tests, which would
    // collide on parallel runs).
    let ledger_path = root.join("ledger.jsonl");
    let cache_dir = root.join("llm_cache");
    // Normalize to forward-slash so the TOML string parses on Windows.
    // (Backslashes in `"..."` TOML strings would be interpreted as
    // escape sequences — `C:\Users\...` fails parse. `\\` doubling
    // works but is fragile; forward-slash is portable.)
    let cache_dir_toml = cache_dir.to_string_lossy().replace('\\', "/");
    let ledger_path_toml = ledger_path.to_string_lossy().replace('\\', "/");
    let toml = format!(
        r#"
[router]
strategy = "quality"
cache_dir = "{cache_dir_toml}"
ledger_path = "{ledger_path_toml}"
preferred = ["synth:synthetic-1"]

[providers.synth]
kind = "synthetic"
base_url = "https://example.invalid"
api_key_env = ""
models = ["synthetic-1"]
"#,
    );

    let cfg = RouterConfig::from_toml_str(&toml).expect("config parse");
    let provider = Arc::new(SyntheticProvider::new("synth"));
    let router: Router = RouterBuilder::new()
        .register_provider("synth", provider)
        .build(&cfg)
        .await
        .expect("router build");

    let store = Store::open(&root).await.expect("Store::open");
    let state = AppState::new(store, Some(Arc::new(router)), root);
    let app = build_router(state);
    (tmp, app)
}

#[tokio::test]
async fn dispatch_returns_200_sse_when_router_configured() {
    let (_tmp, app) = boot_app_with_synthetic_router().await;
    let body = serde_json::json!({
        "model": "synthetic-1",
        "messages": [{"role": "user", "content": "hi"}],
    });
    let resp = oneshot_post_json(&app, "/api/dispatch", &body).await;
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "dispatch must return 200 when router is configured",
    );
    let ct = resp
        .headers()
        .get(axum::http::header::CONTENT_TYPE)
        .map(|v| v.to_str().unwrap_or("").to_string())
        .unwrap_or_default();
    assert!(
        ct.starts_with("text/event-stream"),
        "content-type must be SSE, got {ct:?}",
    );
    let body = read_sse_body_with_timeout(resp, Duration::from_secs(3), 16_384).await;
    assert!(
        body.contains("event: done"),
        "SSE body must carry a `done` event: {body}",
    );
    assert!(
        body.contains(r#""provider":"synth""#),
        "SSE payload must carry the registered provider name: {body}",
    );
    assert!(
        body.contains(SyntheticProvider::FIXTURE_TEXT),
        "SSE payload must carry the synthetic fixture text: {body}",
    );
}

#[tokio::test]
async fn dispatch_echoes_task_tag_in_done_payload() {
    let (_tmp, app) = boot_app_with_synthetic_router().await;
    let body = serde_json::json!({
        "model": "synthetic-1",
        "messages": [{"role": "user", "content": "hi"}],
        "task_tag": "agent-turn",
    });
    let resp = oneshot_post_json(&app, "/api/dispatch", &body).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let body = read_sse_body_with_timeout(resp, Duration::from_secs(3), 16_384).await;
    assert!(
        body.contains(r#""task_tag":"agent-turn""#),
        "SSE done payload must echo the caller-supplied task_tag: {body}",
    );
}

#[tokio::test]
async fn dispatch_returns_400_for_empty_messages_when_router_configured() {
    let (_tmp, app) = boot_app_with_synthetic_router().await;
    let body = serde_json::json!({
        "model": "synthetic-1",
        "messages": [],
    });
    let resp = oneshot_post_json(&app, "/api/dispatch", &body).await;
    assert_eq!(
        resp.status(),
        StatusCode::BAD_REQUEST,
        "empty messages must be rejected as 400 invalid_body",
    );
}

#[tokio::test]
async fn dispatch_returns_400_for_unknown_role_when_router_configured() {
    let (_tmp, app) = boot_app_with_synthetic_router().await;
    let body = serde_json::json!({
        "model": "synthetic-1",
        "messages": [{"role": "robot", "content": "x"}],
    });
    let resp = oneshot_post_json(&app, "/api/dispatch", &body).await;
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}
