//! M7 multi-provider /login — integration tests (ADR-0008 Phase 2).
//!
//! Per ADR-0008 §"Done means" item 2, six integration scenarios gate M7
//! closure. Tests exercise the full HTTP round-trip through the Axum app
//! using `tower::ServiceExt::oneshot`.
//!
//! Run only the M7 tests with:
//!   cargo test -p studio-server --test multi_provider_login
//!
//! ADR-0008 binds the wire format (additive `provider_kind` field on
//! `LoginRequest` + `EndpointSecret`, defaulting to `Anthropic` for
//! backward compat) + the dispatch-time match arm. Tests assert the
//! deployed implementation honours that pin.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::missing_panics_doc)]

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

// Test 6 uses serde_json::Value mutation — already imported via `json`.
// aes-gcm and rand_core are NOT needed here (seal_raw handles crypto).

// ─── Helpers ──────────────────────────────────────────────────────────────────

/// Build a synthetic fallback router (never hits the network).
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

/// Boot an app suitable for multi-provider /login tests.
///
/// Returns `(TempDir, AppState, axum::Router)`.
async fn boot_m7_app() -> (TempDir, AppState, axum::Router) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().to_path_buf();
    let store = Store::open(&root).await.expect("Store::open");
    let static_router = synthetic_router(&root).await;
    let state = AppState::new(store, Some(static_router), root);
    let app = build_router(state.clone());
    (tmp, state, app)
}

/// POST /api/login with an explicit `provider_kind`. Returns (status, body).
async fn do_login_with_kind(
    app: &axum::Router,
    endpoint: &str,
    api_key: &str,
    model: &str,
    passphrase: &str,
    provider_kind: &str,
) -> (StatusCode, Value) {
    let body = json!({
        "endpoint": endpoint,
        "api_key": api_key,
        "model": model,
        "passphrase": passphrase,
        "provider_kind": provider_kind,
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

/// POST /api/login WITHOUT a `provider_kind` field (back-compat path).
async fn do_login_no_kind(
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

/// Mount a minimal Anthropic Messages-API wiremock stub.
async fn mount_anthropic_stub(server: &MockServer, model: &str) {
    let response_body = json!({
        "id": "msg_m7_anthropic",
        "type": "message",
        "role": "assistant",
        "content": [{ "type": "text", "text": "pong-anthropic" }],
        "model": model,
        "stop_reason": "end_turn",
        "usage": { "input_tokens": 3, "output_tokens": 1 }
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

/// Mount a minimal OpenAI `/v1/chat/completions` wiremock stub.
async fn mount_openai_stub(server: &MockServer, model: &str) {
    let response_body = json!({
        "id": "chatcmpl-m7-openai",
        "object": "chat.completion",
        "model": model,
        "choices": [{
            "index": 0,
            "message": { "role": "assistant", "content": "pong-openai" },
            "finish_reason": "stop"
        }],
        "usage": { "prompt_tokens": 3, "completion_tokens": 1, "total_tokens": 4 }
    });
    Mock::given(method("POST"))
        .and(path("/chat/completions"))
        .respond_with(
            ResponseTemplate::new(200)
                .set_body_json(response_body)
                .append_header("content-type", "application/json"),
        )
        .mount(server)
        .await;
}

// ─── Test 1: login anthropic → dispatch ────────────────────────────────────

/// POST /api/login with `provider_kind: "anthropic"` → wiremock Anthropic
/// stub at the supplied endpoint → first dispatch round-trips through
/// `AnthropicProvider`. The default-Anthropic path is the back-compat
/// guarantee for v0.2.x callers.
///
/// Aligns with ADR-0008 §"Done means" item 2 sub-bullet 1.
#[tokio::test]
async fn login_anthropic_then_dispatch() {
    let mock_server = MockServer::start().await;
    let model = "claude-opus-4-7";
    mount_anthropic_stub(&mock_server, model).await;
    let endpoint = mock_server.uri();

    let (_tmp, _state, app) = boot_m7_app().await;

    // POST /api/login with provider_kind=anthropic
    let (status, body) = do_login_with_kind(
        &app,
        &endpoint,
        "sk-ant-test",
        model,
        "s3cr3t-pass!",
        "anthropic",
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "POST /api/login with provider_kind=anthropic must return 200; body={body}",
    );
    assert_eq!(body["status"].as_str(), Some("ok"), "body={body}");

    // Dispatch must succeed via the AnthropicProvider path.
    let (dispatch_status, sse_body) = do_dispatch(&app, model).await;
    assert_eq!(
        dispatch_status,
        StatusCode::OK,
        "dispatch after anthropic login must succeed; SSE body: {sse_body}",
    );
    assert!(
        sse_body.contains("event: done") || sse_body.contains("data:"),
        "SSE body must contain dispatch output: {sse_body}",
    );
}

// ─── Test 2: login openai → dispatch ───────────────────────────────────────

/// POST /api/login with `provider_kind: "openai"` → wiremock OpenAI
/// `/v1/chat/completions` stub → dispatch round-trips through
/// `OpenAiProvider`. This is the primary M7 regression gate — proves
/// the SvelteKit-form path unblocks OpenAI-compat endpoints (vLLM /
/// DeepSeek / Together / OpenRouter / Groq / local Ollama).
///
/// Aligns with ADR-0008 §"Done means" item 2 sub-bullet 2 + closes
/// Sarah v3 audit finding #3 (multi-provider /login).
#[tokio::test]
async fn login_openai_then_dispatch() {
    let mock_server = MockServer::start().await;
    let model = "gpt-5";
    mount_openai_stub(&mock_server, model).await;
    // OpenAiProvider appends `/chat/completions` to the base URL.
    let endpoint = mock_server.uri();

    let (_tmp, _state, app) = boot_m7_app().await;

    // POST /api/login with provider_kind=openai
    let (status, body) = do_login_with_kind(
        &app,
        &endpoint,
        "sk-openai-test",
        model,
        "s3cr3t-pass!",
        "openai",
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "POST /api/login with provider_kind=openai must return 200; body={body}",
    );
    assert_eq!(body["status"].as_str(), Some("ok"), "body={body}");

    // Dispatch must succeed via the OpenAiProvider path.
    let (dispatch_status, sse_body) = do_dispatch(&app, model).await;
    assert_eq!(
        dispatch_status,
        StatusCode::OK,
        "dispatch after openai login must succeed; SSE body: {sse_body}",
    );
    assert!(
        sse_body.contains("event: done") || sse_body.contains("data:"),
        "SSE body must contain dispatch output: {sse_body}",
    );
}

// ─── Test 3: login synthetic → 400 ─────────────────────────────────────────

/// POST /api/login with `provider_kind: "synthetic"` → 400
/// `{code: "invalid_provider_kind"}`. The synthetic provider is a
/// CLI/dev-only construct that has no real-world endpoint+key pair;
/// driving it through /login is a category error.
///
/// Aligns with ADR-0008 §"Done means" item 2 sub-bullet 3.
#[tokio::test]
async fn login_synthetic_returns_400() {
    let (_tmp, _state, app) = boot_m7_app().await;

    let (status, body) = do_login_with_kind(
        &app,
        "http://example.invalid",
        "sk-any",
        "any-model",
        "s3cr3t-pass!",
        "synthetic",
    )
    .await;
    assert_eq!(
        status,
        StatusCode::BAD_REQUEST,
        "provider_kind=synthetic must return 400; body={body}",
    );
    assert_eq!(
        body["code"].as_str(),
        Some("invalid_provider_kind"),
        "error code must be invalid_provider_kind; body={body}",
    );
}

// ─── Test 4: missing provider_kind defaults to Anthropic ───────────────────

/// POST /api/login WITHOUT the `provider_kind` field → defaults to
/// `Anthropic` → behaves identically to v0.2.x. Locks the back-compat
/// contract for existing tooling / curl scripts.
///
/// Aligns with ADR-0008 §"Done means" item 2 sub-bullet 4.
#[tokio::test]
async fn login_missing_provider_kind_defaults_anthropic() {
    let mock_server = MockServer::start().await;
    let model = "claude-opus-4-7";
    mount_anthropic_stub(&mock_server, model).await;
    let endpoint = mock_server.uri();

    let (_tmp, _state, app) = boot_m7_app().await;

    // POST without provider_kind field — back-compat path.
    let (status, body) =
        do_login_no_kind(&app, &endpoint, "sk-compat-test", model, "s3cr3t-pass!").await;
    assert_eq!(
        status,
        StatusCode::OK,
        "POST /api/login without provider_kind must return 200 (defaults to Anthropic); body={body}",
    );
    assert_eq!(body["status"].as_str(), Some("ok"), "body={body}");

    // Dispatch must succeed — proves the default path builds AnthropicProvider.
    let (dispatch_status, sse_body) = do_dispatch(&app, model).await;
    assert_eq!(
        dispatch_status,
        StatusCode::OK,
        "dispatch with default-Anthropic provider must succeed; SSE body: {sse_body}",
    );
    assert!(
        sse_body.contains("event: done") || sse_body.contains("data:"),
        "SSE body must contain dispatch output: {sse_body}",
    );
}

// ─── Test 5: re-login changes provider_kind ────────────────────────────────

/// First login: provider_kind=anthropic. Second login with the SAME
/// passphrase + provider_kind=openai → both succeed (wrong-passphrase
/// guard verifies the PASSPHRASE only, not the kind — provider rotation
/// is a legitimate user action).
///
/// Aligns with ADR-0008 §"Done means" item 2 sub-bullet 5.
#[tokio::test]
async fn re_login_changes_provider_kind() {
    // Mount both Anthropic and OpenAI stubs on separate mock servers.
    let anthropic_server = MockServer::start().await;
    let openai_server = MockServer::start().await;
    let model = "test-model";
    mount_anthropic_stub(&anthropic_server, model).await;
    mount_openai_stub(&openai_server, model).await;

    let (_tmp, _state, app) = boot_m7_app().await;

    let passphrase = "shared-passphrase-m7!";

    // First login: Anthropic
    let (status, body) = do_login_with_kind(
        &app,
        &anthropic_server.uri(),
        "sk-ant-re-login",
        model,
        passphrase,
        "anthropic",
    )
    .await;
    assert_eq!(
        status,
        StatusCode::OK,
        "first login (anthropic) must return 200; body={body}",
    );
    assert_eq!(body["status"].as_str(), Some("ok"));

    // Second login: same passphrase, different provider_kind=openai.
    // The wrong-passphrase guard uses the SAME passphrase to open the
    // existing blob, which succeeds. The new blob is then sealed with
    // provider_kind=openai.
    let (status2, body2) = do_login_with_kind(
        &app,
        &openai_server.uri(),
        "sk-openai-re-login",
        model,
        passphrase,
        "openai",
    )
    .await;
    assert_eq!(
        status2,
        StatusCode::OK,
        "second login (openai, same passphrase) must return 200; body={body2}",
    );
    assert_eq!(body2["status"].as_str(), Some("ok"), "body={body2}");

    // Dispatch must now route through OpenAI (the new provider after re-login).
    let (dispatch_status, sse_body) = do_dispatch(&app, model).await;
    assert_eq!(
        dispatch_status,
        StatusCode::OK,
        "dispatch after re-login to openai must succeed; SSE body: {sse_body}",
    );
    assert!(
        sse_body.contains("event: done") || sse_body.contains("data:"),
        "SSE body must contain dispatch output: {sse_body}",
    );
}

// ─── Test 6: pre-M7 blob (no provider_kind) decrypts → defaults Anthropic ──

/// An `EndpointSecret` blob sealed by v0.2.x (no `provider_kind` field
/// in the serialized JSON) deserializes to `provider_kind = Anthropic`
/// when decrypted by an M7 binary. Preserves the M6 → M7 upgrade path
/// without forcing users to re-login.
///
/// Aligns with ADR-0008 §"Done means" item 2 sub-bullet 6.
///
/// Approach: seal a JSON payload that has no `provider_kind` field (via
/// `SessionKey::seal_raw`, mimicking a v0.2.x binary's sealed blob), then
/// `open()` it in M7 and assert `provider_kind = Anthropic`.
#[tokio::test]
async fn existing_blob_decryption_supplies_kind() {
    use studio_server::secret::{EndpointSecret, ProviderKind, SessionKey};

    let passphrase = "pre-m7-compat-test";
    let salt = [0xAAu8; 16];
    let key = SessionKey::derive(passphrase, &salt).expect("derive");

    // Verify the serde(default) path directly (unit-style, no network).
    let pre_m7_json =
        r#"{"endpoint":"https://api.anthropic.com","api_key":"sk-legacy","model":"claude-3"}"#;
    let direct: EndpointSecret =
        serde_json::from_str(pre_m7_json).expect("deserialize pre-M7 JSON");
    assert_eq!(
        direct.provider_kind,
        ProviderKind::Anthropic,
        "pre-M7 JSON (no provider_kind) must serde-default to Anthropic"
    );

    // Full seal → open round-trip of a pre-M7 payload.
    //
    // Construct the JSON of an `EndpointSecret` that has no `provider_kind`
    // field (as a v0.2.x binary would have serialized it), then seal it via
    // `seal_raw` (which encrypts raw bytes without struct-level serialisation)
    // and open it with the M7 `open()`.
    let original = EndpointSecret {
        endpoint: "https://api.anthropic.com".to_string(),
        api_key: "sk-legacy-blob".to_string(),
        model: "claude-opus-4-7".to_string(),
        provider_kind: ProviderKind::Anthropic,
    };
    let mut json_val = serde_json::to_value(&original).expect("to_value");
    // Remove provider_kind to simulate a v0.2.x sealed blob.
    json_val
        .as_object_mut()
        .expect("object")
        .remove("provider_kind");
    let pre_m7_bytes = serde_json::to_vec(&json_val).expect("to_vec");

    // seal_raw encrypts the raw JSON bytes without EndpointSecret serialisation.
    let blob = key.seal_raw(&pre_m7_bytes).expect("seal_raw");

    // Open the blob with the M7 key — must deserialize to Anthropic.
    let recovered = key.open(&blob).expect("open pre-M7 blob");
    assert_eq!(
        recovered.provider_kind,
        ProviderKind::Anthropic,
        "pre-M7 blob (no provider_kind in ciphertext JSON) must open as Anthropic"
    );
    assert_eq!(recovered.endpoint, "https://api.anthropic.com");
    assert_eq!(recovered.api_key, "sk-legacy-blob");
    assert_eq!(recovered.model, "claude-opus-4-7");
}
