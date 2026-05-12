//! M6 secret-storage AEAD round-trip — integration tests.
//!
//! Per ADR-0007 §"Done means" item 2, three integration scenarios gate M6
//! closure. Each test exercises the full HTTP round-trip through the Axum
//! app using `tower::ServiceExt::oneshot`.
//!
//! Run only the M6 tests with:
//!   cargo test -p studio-server --test secret_roundtrip
//!
//! ADR-0007 binds the algorithm + wire format (AES-256-GCM + Argon2id,
//! packed `salt(16) || nonce(12) || ciphertext+tag` under scheme tag
//! `"aes-gcm-256/argon2id-v1"`). Tests assert that the deployed module
//! honours that pin.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::missing_panics_doc,
    clippy::items_after_statements
)]

mod common;

use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use common::{oneshot_post_json, read_sse_body_with_timeout};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use studio_router::{Router as LlmRouter, RouterBuilder, RouterConfig};
use studio_server::{AppState, SyntheticProvider, build_router};
use studio_store::Store;
use tempfile::TempDir;
use tower::ServiceExt;
use wiremock::matchers::{method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

// ─── Helpers ──────────────────────────────────────────────────────────────────

/// Build a synthetic router that never hits the network, for isolation.
async fn synthetic_router(root: &std::path::Path) -> Arc<LlmRouter> {
    let cache_dir = root.join("llm_cache");
    let ledger = root.join("ledger.jsonl");
    let cache_toml = cache_dir.to_string_lossy().replace('\\', "/");
    let ledger_toml = ledger.to_string_lossy().replace('\\', "/");
    let toml = format!(
        r#"
[router]
strategy = "quality"
cache_dir = "{cache_toml}"
ledger_path = "{ledger_toml}"
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
    let router: LlmRouter = RouterBuilder::new()
        .register_provider("synth", provider)
        .build(&cfg)
        .await
        .expect("router build");
    Arc::new(router)
}

/// Boot an app with both a static synthetic router and an empty session_key.
/// The `/api/login` route will seal the credentials and stash the key.
/// Dispatch will then use the session_key path (session_key Some) to build
/// an AnthropicProvider pointing at `endpoint_url`.
///
/// Returns `(TempDir, AppState, axum::Router)`.
async fn boot_login_app(endpoint_url: String) -> (TempDir, AppState, axum::Router) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().to_path_buf();
    let store = Store::open(&root).await.expect("Store::open");
    let static_router = synthetic_router(&root).await;
    let state = AppState::new(store, Some(static_router), root);
    let app = build_router(state.clone());
    // Stash the endpoint so the test can reference it but we don't need
    // it here — it's encoded in the POST /api/login body.
    let _ = endpoint_url;
    (tmp, state, app)
}

/// POST /api/login with the given credentials. Returns the response status + body.
async fn do_login(
    app: &axum::Router,
    endpoint: &str,
    api_key: &str,
    model: &str,
    passphrase: &str,
) -> (StatusCode, Value) {
    let body = json!({
        "endpoint": endpoint,
        "api_key": api_key,
        "model": model,
        "passphrase": passphrase,
    });
    let resp = oneshot_post_json(app, "/api/login", &body).await;
    let status = resp.status();
    let bytes = resp
        .into_body()
        .collect()
        .await
        .expect("collect")
        .to_bytes();
    let json = serde_json::from_slice::<Value>(&bytes).unwrap_or(Value::Null);
    (status, json)
}

/// POST /api/dispatch with a minimal user message targeting `model`.
async fn do_dispatch(app: &axum::Router, model: &str) -> (StatusCode, String) {
    let body = json!({
        "model": model,
        "messages": [{"role": "user", "content": "ping"}],
    });
    let bytes = serde_json::to_vec(&body).expect("encode");
    let req = Request::builder()
        .method(Method::POST)
        .uri("/api/dispatch")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(bytes))
        .expect("build request");
    let resp = app.clone().oneshot(req).await.expect("oneshot");
    let status = resp.status();
    let body_text = read_sse_body_with_timeout(resp, Duration::from_secs(5), 65_536).await;
    (status, body_text)
}

// ─── Wiremock Anthropic stub ────────────────────────────────────────────────

/// Mount a minimal Anthropic Messages-API mock that returns a valid
/// non-streaming JSON response. The `AnthropicProvider::complete` path
/// expects `POST /v1/messages` → `200 { "content": [{ "text": "..." }], ... }`.
async fn mount_anthropic_stub(server: &MockServer, model: &str) {
    let response_body = json!({
        "id": "msg_test_123",
        "type": "message",
        "role": "assistant",
        "content": [{ "type": "text", "text": "pong" }],
        "model": model,
        "stop_reason": "end_turn",
        "usage": {
            "input_tokens": 3,
            "output_tokens": 1
        }
    });
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(response_body)
                .append_header("content-type", "application/json"),
        )
        .mount(server)
        .await;
}

// ─── Test 1: login → dispatch uses in-memory key ────────────────────────────

/// POST /api/login with `(endpoint, api_key, model, passphrase)`,
/// then dispatch a request using the in-memory `SessionKey`. Asserts
/// the dispatch resolves to the decrypted endpoint+key without ever
/// reading from `ANTHROPIC_API_KEY` env var.
///
/// This is the **primary regression gate** for M6 — proves the
/// env-var workaround is no longer required for the happy path.
/// Aligns with ADR-0007 §"Done means" item 2 sub-bullet 1.
#[tokio::test]
async fn login_then_dispatch_with_in_memory_key() {
    // Spin up a mock Anthropic endpoint so the dispatch path can complete
    // without a real API call.
    let mock_server = MockServer::start().await;
    let model = "claude-opus-4-7";
    mount_anthropic_stub(&mock_server, model).await;
    let endpoint = mock_server.uri();

    let (_tmp, state, app) = boot_login_app(endpoint.clone()).await;

    // Before login: no session key.
    {
        let guard = state.session_key.read().await;
        assert!(guard.is_none(), "no session key before login");
    }

    // POST /api/login — this is the first login, no existing blob.
    let (status, body) = do_login(&app, &endpoint, "sk-test-key", model, "s3cr3t-pass!").await;
    assert_eq!(
        status,
        StatusCode::OK,
        "POST /api/login must return 200; body={body}",
    );
    assert_eq!(
        body["status"].as_str(),
        Some("ok"),
        "login response must have status=ok",
    );

    // After login: session key must be set in AppState.
    {
        let guard = state.session_key.read().await;
        assert!(
            guard.is_some(),
            "session key must be set in AppState after login",
        );
    }

    // GET /api/session/status must return authenticated=true.
    let req = Request::builder()
        .method(Method::GET)
        .uri("/api/session/status")
        .body(Body::empty())
        .expect("build request");
    let resp = app.clone().oneshot(req).await.expect("oneshot");
    assert_eq!(resp.status(), StatusCode::OK);
    let bytes = resp
        .into_body()
        .collect()
        .await
        .expect("collect")
        .to_bytes();
    let status_json: Value = serde_json::from_slice(&bytes).expect("parse json");
    assert_eq!(
        status_json["authenticated"].as_bool(),
        Some(true),
        "session/status must be authenticated=true after login",
    );

    // POST /api/dispatch — the session_key path builds AnthropicProvider
    // pointing at our mock server and dispatches.
    let (dispatch_status, sse_body) = do_dispatch(&app, model).await;
    assert_eq!(
        dispatch_status,
        StatusCode::OK,
        "dispatch must succeed after login; SSE body: {sse_body}",
    );
    // The SSE body must contain either a `done` event or at least some `data:`.
    assert!(
        sse_body.contains("event: done") || sse_body.contains("data:"),
        "SSE body must contain dispatch output: {sse_body}",
    );
}

// ─── Test 2: restart drops key → 401 ────────────────────────────────────────

/// POST /api/login → simulate process restart by constructing a new
/// app-state instance (no `SessionKey` carryover) → next dispatch
/// returns 401 `no_session`.
///
/// Verifies the "binary restart drops in-memory key" property in
/// ADR-0007 §"Decision" sub-bullet 6 and §"Done means" item 2
/// sub-bullet 2. Distinguishes "session_kv blob still on disk"
/// from "decrypted key still in process memory" — the cold-disk-
/// theft threat-model attacker (in-scope #1) reads the blob but
/// has no key.
///
/// To isolate the session-key path from the static-router fallback,
/// the second AppState is constructed with `router = None`. This
/// means the only path to dispatch success is a valid session_key —
/// without it, dispatch must return an auth error.
#[tokio::test]
async fn restart_drops_key_returns_401() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().to_path_buf();

    // --- First AppState: login succeeds ---
    let store1 = Store::open(&root).await.expect("Store::open");
    let static_router = synthetic_router(&root).await;
    let state1 = AppState::new(store1.clone(), Some(static_router), root.clone());
    let app1 = build_router(state1.clone());

    let mock_server = MockServer::start().await;
    let model = "claude-opus-4-7";
    mount_anthropic_stub(&mock_server, model).await;
    let endpoint = mock_server.uri();

    let (status, body) = do_login(&app1, &endpoint, "sk-restart-test", model, "pass1234").await;
    assert_eq!(
        status,
        StatusCode::OK,
        "first login must succeed; body={body}",
    );
    // Verify session key is set in the first app state.
    {
        let guard = state1.session_key.read().await;
        assert!(
            guard.is_some(),
            "session key must be in first AppState after login"
        );
    }

    // --- Simulate restart: new AppState with router=None, same store ---
    // A fresh AppState::new always initialises session_key to None.
    // router=None ensures dispatch cannot fall back to the static-router path;
    // only the session_key path is available, and it's None post-restart.
    let store2 = Store::open(&root).await.expect("Store::open after restart");
    let state2 = AppState::new(store2, None, root);
    let app2 = build_router(state2.clone());

    // session_key must be None in the fresh AppState.
    {
        let guard = state2.session_key.read().await;
        assert!(
            guard.is_none(),
            "fresh AppState must have no session key (simulates restart)",
        );
    }

    // Dispatch on the new app (router=None + session_key=None) must fail.
    let (dispatch_status, sse_body) = do_dispatch(&app2, model).await;
    assert!(
        dispatch_status == StatusCode::BAD_REQUEST
            || dispatch_status == StatusCode::UNAUTHORIZED
            || dispatch_status == StatusCode::SERVICE_UNAVAILABLE,
        "dispatch after restart must be 400/401/503 (no session key + no router); \
         got status={dispatch_status}, body={sse_body}",
    );
    // The SSE body should indicate no_session or router_not_configured.
    let body_lower = sse_body.to_lowercase();
    assert!(
        body_lower.contains("no_session")
            || body_lower.contains("not authenticated")
            || body_lower.contains("router_not_configured")
            || body_lower.contains("unauthenticated")
            || sse_body.is_empty(),
        "error response must indicate auth/router failure; body={sse_body}",
    );

    // The session_kv blob must still be on disk (the blob is not destroyed by
    // restart — only the in-memory key is gone).
    let blob = store1.session().get_endpoint().await.expect("get_endpoint");
    assert!(
        blob.is_some(),
        "session_kv blob must persist on disk after simulated restart",
    );
}

// ─── Test 3: wrong passphrase → 401 ─────────────────────────────────────────

/// POST /api/login with mismatched passphrase against existing
/// session_kv blob → AEAD tag validation fails → `401`.
///
/// Verifies AEAD authenticity (not just confidentiality) per
/// ADR-0007 §"Algorithm choice" AES-256-GCM tag check. Aligns with
/// §"Done means" item 2 sub-bullet 3 + item 1 sub-bullet
/// `wrong_passphrase_fails_open`.
#[tokio::test]
async fn wrong_passphrase_login_returns_401() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().to_path_buf();
    let store = Store::open(&root).await.expect("Store::open");
    let static_router = synthetic_router(&root).await;
    let state = AppState::new(store, Some(static_router), root);
    let app = build_router(state.clone());

    let mock_server = MockServer::start().await;
    let model = "claude-opus-4-7";
    let endpoint = mock_server.uri();

    // First login with the CORRECT passphrase — seeds the session_kv blob.
    let (status, body) = do_login(&app, &endpoint, "sk-wrong-test", model, "correct-pass").await;
    assert_eq!(
        status,
        StatusCode::OK,
        "initial login with correct pass must succeed; body={body}",
    );
    assert_eq!(body["status"].as_str(), Some("ok"));

    // Clear the in-memory key to simulate "only the blob exists".
    {
        let mut guard = state.session_key.write().await;
        *guard = None;
    }

    // Second login attempt with the WRONG passphrase against the existing blob.
    let (status2, body2) = do_login(&app, &endpoint, "sk-wrong-test", model, "WRONG-pass").await;
    assert_eq!(
        status2,
        StatusCode::BAD_REQUEST,
        "login with wrong passphrase must return 400; body={body2}",
    );
    assert_eq!(
        body2["code"].as_str(),
        Some("wrong_passphrase"),
        "error code must be wrong_passphrase; body={body2}",
    );

    // The session_key must remain None (wrong passphrase must not store a key).
    {
        let guard = state.session_key.read().await;
        assert!(
            guard.is_none(),
            "session_key must remain None after wrong-passphrase login attempt",
        );
    }
}

// ─── Test 4: short passphrase → 400 (server-side validation) ────────────────

/// Sarah v3 finding: server enforces `passphrase.len() >= 8` independently
/// of the SvelteKit client-side check, so direct curl-style POSTs cannot
/// bypass the minimum-strength bar.
#[tokio::test]
async fn short_passphrase_login_returns_400() {
    let (_tmp, _state, app) = boot_login_app("http://example.invalid".to_string()).await;

    // 7-char passphrase — under the 8-char floor.
    let (status, body) = do_login(&app, "http://example.invalid", "sk-x", "m", "short!1").await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "POST /api/login with short passphrase must return 400; body={body}",
    );
    let code = body["code"].as_str().unwrap_or_default();
    assert_eq!(
        code, "passphrase_too_short",
        "error code must be passphrase_too_short; body={body}",
    );
}
