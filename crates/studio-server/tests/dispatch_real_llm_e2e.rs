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

//! Real-LLM end-to-end smoke for `POST /api/dispatch` — Wave A5 P7-TEST.
//!
//! Gated `#[ignore]` so the default `cargo test` suite never reaches out
//! to a live provider. Run explicitly with:
//!
//! ```bash
//! STUDIO_E2E_API_KEY=... \
//! STUDIO_E2E_BASE_URL=https://example/v1 \
//! STUDIO_E2E_MODEL=gpt-5-mini \
//! cargo test -p studio-server --test dispatch_real_llm_e2e -- --ignored
//! ```
//!
//! Env vars (all required for the live path):
//! - `STUDIO_E2E_API_KEY` — secret bearer/api key passed to the provider.
//! - `STUDIO_E2E_BASE_URL` — provider HTTP base URL (OpenAI-compatible
//!   `/chat/completions` shape; the Studio codex-forwarder is the typical
//!   target).
//! - `STUDIO_E2E_MODEL` — model id (e.g. `gpt-5-mini`,
//!   `claude-opus-4-7`). Default `gpt-5-mini` if unset and we already
//!   resolved the other two.
//! - `STUDIO_E2E_PROVIDER_KIND` — optional, `openai` (default) or
//!   `anthropic`. Selects which `studio_router::*Provider::new` we wire.
//!
//! **Never hardcode credentials.** All values come from env vars; absence
//! short-circuits with an explanatory panic so the test fails clearly
//! rather than silently passing on missing config.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use axum::Router;
use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use serde_json::json;
use studio_router::config::{ProviderConfig, ProviderKind, RouterConfig, RouterSection, Strategy};
use studio_router::provider::LlmProvider;
use studio_router::{AnthropicProvider, OpenAiProvider, Router as LlmRouter, RouterBuilder};
use studio_server::{AppState, build_router};
use studio_store::{LEDGER_JSONL_PATH, Store};
use tempfile::TempDir;
use tower::ServiceExt;

const PROVIDER_KEY: &str = "e2e_provider";

fn require_env(name: &str) -> String {
    std::env::var(name)
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| {
            panic!(
                "env var `{name}` is required for the live-LLM e2e test; \
                 set it before running with `--ignored`",
            )
        })
}

fn provider_kind_from_env() -> ProviderKind {
    match std::env::var("STUDIO_E2E_PROVIDER_KIND")
        .ok()
        .as_deref()
        .map_or("openai", str::trim)
    {
        "anthropic" => ProviderKind::Anthropic,
        // Default + `openai` fall through to the OpenAI-compatible shape.
        _ => ProviderKind::Openai,
    }
}

fn model_from_env() -> String {
    std::env::var("STUDIO_E2E_MODEL")
        .ok()
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "gpt-5-mini".to_string())
}

/// Build a `LlmProvider` matching the env-selected kind. Never logs the
/// `api_key` — only `base_url` and `kind` are surfaced in panic messages.
fn build_live_provider(
    name: &str,
    kind: ProviderKind,
    base_url: &str,
    api_key: &str,
) -> Arc<dyn LlmProvider> {
    match kind {
        ProviderKind::Openai => Arc::new(
            OpenAiProvider::new(name.to_string(), base_url.to_string(), api_key.to_string())
                .expect("construct OpenAiProvider"),
        ),
        ProviderKind::Anthropic => Arc::new(
            AnthropicProvider::new(name.to_string(), base_url.to_string(), api_key.to_string())
                .expect("construct AnthropicProvider"),
        ),
        ProviderKind::Synthetic => {
            panic!("STUDIO_E2E_PROVIDER_KIND=synthetic is not a live provider")
        }
        _ => panic!(
            "STUDIO_E2E_PROVIDER_KIND has an unknown variant {kind:?}; \
             extend the e2e fixture or pin to an older binary"
        ),
    }
}

async fn build_live_router(
    root: &Path,
    kind: ProviderKind,
    base_url: &str,
    api_key: &str,
    model: &str,
) -> Arc<LlmRouter> {
    let mut providers = BTreeMap::new();
    providers.insert(
        PROVIDER_KEY.to_string(),
        ProviderConfig {
            kind,
            base_url: base_url.to_string(),
            api_key_env: String::new(), // we pass the key in directly
            models: vec![model.to_string()],
        },
    );
    let cfg = RouterConfig {
        router: RouterSection {
            strategy: Strategy::Quality,
            cache_dir: root.join(".cobrust-studio").join("router").join("cache"),
            ledger_path: root.join(LEDGER_JSONL_PATH),
            preferred: vec![format!("{PROVIDER_KEY}:{model}")],
        },
        providers,
    };
    let provider = build_live_provider(PROVIDER_KEY, kind, base_url, api_key);
    let router = RouterBuilder::new()
        .register_provider(PROVIDER_KEY, provider)
        .build(&cfg)
        .await
        .expect("RouterBuilder::build (live)");
    Arc::new(router)
}

async fn boot_app_with_live_router(
    kind: ProviderKind,
    base_url: &str,
    api_key: &str,
    model: &str,
) -> (TempDir, std::path::PathBuf, Router) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().to_path_buf();
    let router_dir = root.join(".cobrust-studio").join("router");
    tokio::fs::create_dir_all(&router_dir)
        .await
        .expect("mkdir router/");
    let live_router = build_live_router(&root, kind, base_url, api_key, model).await;
    let store = Store::open(&root).await.expect("Store::open");
    let state = AppState::new(store, Some(live_router), root.clone());
    let app = build_router(state);
    (tmp, root, app)
}

async fn drain_sse(resp: axum::response::Response, deadline: Duration) -> String {
    let (_parts, body) = resp.into_parts();
    let mut buf: Vec<u8> = Vec::new();
    let mut stream = body.into_data_stream();
    let until = tokio::time::Instant::now() + deadline;
    use futures::StreamExt;
    loop {
        let now = tokio::time::Instant::now();
        if now >= until {
            break;
        }
        let remaining = until - now;
        match tokio::time::timeout(remaining, stream.next()).await {
            Ok(Some(Ok(chunk))) => buf.extend_from_slice(&chunk),
            Ok(Some(Err(_)) | None) => break,
            Err(_) => break,
        }
    }
    String::from_utf8_lossy(&buf).into_owned()
}

#[tokio::test]
#[ignore = "requires STUDIO_E2E_API_KEY + STUDIO_E2E_BASE_URL env vars; run with `cargo test -- --ignored`"]
async fn dispatch_real_llm_returns_streaming_response() {
    let api_key = require_env("STUDIO_E2E_API_KEY");
    let base_url = require_env("STUDIO_E2E_BASE_URL");
    let model = model_from_env();
    let kind = provider_kind_from_env();

    let (_tmp, _root, app) = boot_app_with_live_router(kind, &base_url, &api_key, &model).await;

    let body = json!({
        "model": model,
        "messages": [
            { "role": "user", "content": "Say 'hello' and nothing more." }
        ],
        "params": {
            "max_tokens": 32,
            "temperature": 0.0
        }
    });
    let bytes = serde_json::to_vec(&body).expect("encode body");
    let req = Request::builder()
        .method(Method::POST)
        .uri("/api/dispatch")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::ACCEPT, "text/event-stream")
        .body(Body::from(bytes))
        .expect("build request");

    let start = Instant::now();
    let resp = app.oneshot(req).await.expect("oneshot");
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "live dispatch must return 200; status={}",
        resp.status(),
    );
    let ct = resp
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|h| h.to_str().ok())
        .unwrap_or("")
        .to_ascii_lowercase();
    assert!(
        ct.starts_with("text/event-stream"),
        "live dispatch must stream SSE; got content-type={ct:?}",
    );

    let sse_text = drain_sse(resp, Duration::from_secs(30)).await;
    let elapsed = start.elapsed();
    assert!(
        elapsed < Duration::from_secs(30),
        "live dispatch must complete in <30s; took {elapsed:?}",
    );

    // At least one non-empty `data:` line must surface in the body.
    let mut has_nonempty_data = false;
    for line in sse_text.lines() {
        if let Some(payload) = line.strip_prefix("data:")
            && !payload.trim().is_empty()
        {
            has_nonempty_data = true;
            break;
        }
    }
    assert!(
        has_nonempty_data,
        "SSE body must contain ≥1 non-empty `data:` line; body={sse_text:?}",
    );

    // Smoke: a real provider should produce *some* text. We don't pin the
    // exact wording (rate-limited / quota-exhausted retries can produce a
    // partial-but-valid response). The point of this test is to prove the
    // dispatch pipeline reaches a real endpoint and streams something back.
    let total_bytes = sse_text.len();
    assert!(
        total_bytes > 0,
        "live dispatch yielded an empty SSE body — pipeline is broken",
    );
}
