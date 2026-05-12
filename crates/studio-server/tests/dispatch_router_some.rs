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
    clippy::doc_markdown,
    dead_code
)]

//! `POST /api/dispatch` integration contract — Wave A5 P7-TEST (red).
//!
//! Locks the live-dispatch (router=`Some(_)`) surface. Sibling
//! `dispatch_route.rs` (Wave A4) covered the 503 path; this file covers
//! everything once `AppState.router = Some(Arc<Router>)`.
//!
//! API-shape assumptions made (DEV must satisfy; CTO reconciles drift):
//!
//! 1. **Success status + content-type.** A valid `CompletionRequest` body
//!    must return `200 OK` with `Content-Type: text/event-stream` (params
//!    allowed, case-insensitive).
//! 2. **SSE chunk events.** The route streams at least 3 `data:` events
//!    carrying the provider's delta text (one per `Chunk::Delta`). DEV
//!    may use [`studio_router::LlmProvider::complete_stream`] directly or
//!    call [`studio_router::Router::dispatch`] and chunk the resulting
//!    text — either is acceptable provided the wire shape yields ≥3
//!    `data:` lines whose payloads concatenate to the full text.
//! 3. **Chunk order.** The chunks arrive in dispatch order; the synthetic
//!    provider emits `["hello", " ", "world"]` and assertions look for
//!    those substrings in order anywhere in the SSE body bytes (not
//!    strict frame parsing — many wire encodings are acceptable).
//! 4. **`event: done` at stream end.** The last SSE event is named
//!    `done` and its `data:` payload is a JSON object carrying at least
//!    the [`studio_router::LedgerEntry`] summary fields (`provider`,
//!    `model`, `cache_hit`, `total_tokens`). DEV may add fields freely
//!    (e.g. `task_tag`, `latency_ms`).
//! 5. **Task tag propagation.** Body field `task_tag: "test-run-1"`
//!    must end up in the recorded ledger entry. Either the dispatch
//!    route writes through both the router JSONL AND the store
//!    materialised view, OR the test re-opens the store post-dispatch
//!    to trigger the cold-start sync. We use the second strategy so
//!    DEV's implementation only needs to plumb task_tag into the
//!    `Router::dispatch()` call.
//! 6. **Malformed body → 400 + JSON envelope.** A non-JSON or
//!    schema-violating body returns `400 Bad Request` with body
//!    `{ error, code: "invalid_input" }`. DEV may also accept `422`
//!    or use an alternate code (e.g. `invalid_body`) — both are
//!    documented as acceptable in §"API-shape assumptions" but we
//!    assert the spec-mandated `invalid_input` first and document any
//!    drift as a finding.
//! 7. **Router=Some but provider mismatch → SSE `error` event, not 503.**
//!    When AppState.router is `Some` but the inner `Router::dispatch`
//!    fails (e.g. unknown provider), the route emits an SSE
//!    `event: error` frame (not a 503). The 503 path is gated on the
//!    `is_none()` check alone.
//!
//! Per ADR-0006 §"Addendum 2026-05-11" §F-01 the binding contract:
//!
//! ```rust,ignore
//! let cfg = RouterConfig::from_toml_str(&toml)?;
//! let provider: Arc<dyn LlmProvider> = Arc::new(SyntheticProvider::new());
//! let router = RouterBuilder::new()
//!     .register_provider("synth", provider)
//!     .build(&cfg)
//!     .await?;
//! let resp = router.dispatch(req).await?;
//! ```
//!
//! Per ADR-0006 §F-03 the task_tag plumbing lands at A4/A5 via
//! `DispatchContext` (or equivalent caller-supplied field). The tests
//! assume DEV chose option (a) "add `task_tag` to the request body".

mod common;

use std::collections::BTreeMap;
use std::path::Path;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use common::{oneshot_post_bytes, oneshot_post_json, read_sse_body_with_timeout, status_and_json};
use futures::stream::{self, Stream};
use serde_json::{Value, json};
use studio_router::config::{ProviderConfig, ProviderKind, RouterConfig, RouterSection, Strategy};
use studio_router::provider::{
    Chunk, CompletionRequest, CompletionResponse, LlmError, LlmProvider, TokenUsage,
};
use studio_router::{Router as LlmRouter, RouterBuilder};
use studio_server::{AppState, build_router};
use studio_store::{LEDGER_JSONL_PATH, Store};
use tempfile::TempDir;
use tower::ServiceExt;

// ---------------------------------------------------------------------------
// Synthetic provider — deterministic in-process `LlmProvider` for tests.
// ---------------------------------------------------------------------------

/// Three fixed chunks emitted by the synthetic provider's streaming path.
/// `complete()` joins them to form the non-streaming text response.
const SYNTHETIC_CHUNKS: &[&str] = &["hello", " ", "world"];

/// Provider key registered with the builder + referenced in
/// `RouterConfig.providers` and `RouterConfig.router.preferred`.
const SYNTH_KEY: &str = "synth";

/// Fixed model id the synthetic provider serves under.
const SYNTH_MODEL: &str = "fixed-model";

/// Synthetic deterministic [`LlmProvider`] for integration tests.
///
/// - `complete()` returns `CompletionResponse { text: "hello world", .. }`.
/// - `complete_stream()` yields `Chunk::Delta("hello")`,
///   `Chunk::Delta(" ")`, `Chunk::Delta("world")`, `Chunk::Done(_)`.
/// - `name()` returns `"synth"`.
/// - `kind()` returns [`ProviderKind::Synthetic`].
#[derive(Clone, Debug, Default)]
struct SyntheticProvider {
    /// Optional knob — when `true`, `complete_stream()` yields one
    /// `Chunk::Delta("partial")` and then an error, exercising the
    /// router's stream-error path. Off by default.
    err_mid_stream: bool,
}

impl SyntheticProvider {
    fn new() -> Self {
        Self::default()
    }

    #[allow(dead_code)]
    fn with_mid_stream_error() -> Self {
        Self {
            err_mid_stream: true,
        }
    }
}

#[async_trait::async_trait]
impl LlmProvider for SyntheticProvider {
    fn name(&self) -> &str {
        SYNTH_KEY
    }

    fn kind(&self) -> ProviderKind {
        ProviderKind::Synthetic
    }

    async fn complete(&self, _req: CompletionRequest) -> Result<CompletionResponse, LlmError> {
        let text: String = SYNTHETIC_CHUNKS.concat();
        let prompt_tokens: u32 = 3;
        #[allow(clippy::cast_possible_truncation)]
        let completion_tokens = u32::try_from(text.split_whitespace().count()).unwrap_or(2);
        Ok(CompletionResponse {
            text,
            model: SYNTH_MODEL.to_string(),
            usage: TokenUsage {
                prompt_tokens,
                completion_tokens,
            },
        })
    }

    fn complete_stream(
        &self,
        _req: CompletionRequest,
    ) -> Pin<Box<dyn Stream<Item = Result<Chunk, LlmError>> + Send>> {
        let err_mid = self.err_mid_stream;
        let mut items: Vec<Result<Chunk, LlmError>> = Vec::new();
        if err_mid {
            items.push(Ok(Chunk::Delta("partial".to_string())));
            items.push(Err(LlmError::Stream("synthetic mid-stream failure".into())));
        } else {
            for s in SYNTHETIC_CHUNKS {
                items.push(Ok(Chunk::Delta((*s).to_string())));
            }
            items.push(Ok(Chunk::Done(TokenUsage {
                prompt_tokens: 3,
                completion_tokens: 2,
            })));
        }
        Box::pin(stream::iter(items))
    }
}

// ---------------------------------------------------------------------------
// Boot helpers — construct AppState with router=Some(_) and a synthetic
// provider wired in.
// ---------------------------------------------------------------------------

/// Build a [`RouterConfig`] rooted at the given tempdir. The cache + ledger
/// paths live inside the tempdir so the test is hermetic. The ledger path
/// is the canonical `<root>/.cobrust-studio/router/ledger.jsonl` so a
/// subsequent `Store::open` cold-start can replay it into the SQLite view.
fn synthetic_router_config(root: &Path) -> RouterConfig {
    let mut providers = BTreeMap::new();
    providers.insert(
        SYNTH_KEY.to_string(),
        ProviderConfig {
            kind: ProviderKind::Synthetic,
            base_url: String::new(),
            api_key_env: String::new(),
            models: vec![SYNTH_MODEL.to_string()],
        },
    );
    RouterConfig {
        router: RouterSection {
            strategy: Strategy::Quality,
            cache_dir: root.join(".cobrust-studio").join("router").join("cache"),
            ledger_path: root.join(LEDGER_JSONL_PATH),
            preferred: vec![format!("{SYNTH_KEY}:{SYNTH_MODEL}")],
        },
        providers,
    }
}

/// Build a `Router` registering the synthetic provider against the given
/// tempdir-rooted config.
async fn build_synthetic_router(root: &Path) -> Arc<LlmRouter> {
    let cfg = synthetic_router_config(root);
    let provider: Arc<dyn LlmProvider> = Arc::new(SyntheticProvider::new());
    let router = RouterBuilder::new()
        .register_provider(SYNTH_KEY, provider)
        .build(&cfg)
        .await
        .expect("build synthetic router");
    Arc::new(router)
}

/// Boot an `AppState` with `router=Some(synth_router)` and a fresh store.
/// Returns the `TempDir` guard, project root, and the built axum `Router`.
async fn boot_app_with_synthetic_router() -> (TempDir, std::path::PathBuf, axum::Router) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().to_path_buf();
    // Ensure the router subdir exists before Cache::new / Ledger::open are
    // called inside RouterBuilder::build — the cache root is created by
    // the cache itself but creating it explicitly is harmless.
    let router_dir = root.join(".cobrust-studio").join("router");
    tokio::fs::create_dir_all(&router_dir)
        .await
        .expect("mkdir router/");
    let synth_router = build_synthetic_router(&root).await;
    let store = Store::open(&root).await.expect("Store::open");
    let state = AppState::new(store, Some(synth_router), root.clone());
    let app = build_router(state);
    (tmp, root, app)
}

/// Boot an `AppState` whose Router is `Some(_)` but whose configured
/// preferred provider is NOT registered with the builder. The builder
/// validation rejects this at construction time, so to exercise the
/// "router=Some but dispatch fails" path we instead register a synthetic
/// provider under a key the config doesn't declare in `preferred` — and
/// then the dispatch finds an empty preferred list, returning
/// `RouterError::NoProvider` from `Router::dispatch`.
///
/// Used by `dispatch_with_router_some_but_no_providers_returns_error_event`.
async fn boot_app_with_router_some_no_providers() -> (TempDir, std::path::PathBuf, axum::Router) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().to_path_buf();
    let router_dir = root.join(".cobrust-studio").join("router");
    tokio::fs::create_dir_all(&router_dir)
        .await
        .expect("mkdir router/");

    let mut providers = BTreeMap::new();
    providers.insert(
        SYNTH_KEY.to_string(),
        ProviderConfig {
            kind: ProviderKind::Synthetic,
            base_url: String::new(),
            api_key_env: String::new(),
            models: vec![SYNTH_MODEL.to_string()],
        },
    );
    let cfg = RouterConfig {
        router: RouterSection {
            strategy: Strategy::Quality,
            cache_dir: root.join(".cobrust-studio").join("router").join("cache"),
            ledger_path: root.join(LEDGER_JSONL_PATH),
            preferred: Vec::new(), // empty preferred → Router::dispatch returns NoProvider
        },
        providers,
    };
    let provider: Arc<dyn LlmProvider> = Arc::new(SyntheticProvider::new());
    let router = Arc::new(
        RouterBuilder::new()
            .register_provider(SYNTH_KEY, provider)
            .build(&cfg)
            .await
            .expect("build router with empty preferred"),
    );
    let store = Store::open(&root).await.expect("Store::open");
    let state = AppState::new(store, Some(router), root.clone());
    let app = build_router(state);
    (tmp, root, app)
}

// ---------------------------------------------------------------------------
// Request/response helpers
// ---------------------------------------------------------------------------

/// A minimal valid `studio_router::CompletionRequest`-shaped body. The
/// `task_tag` field is optional and propagates to ledger entries when set.
fn sample_completion_request(task_tag: Option<&str>) -> Value {
    let mut body = json!({
        "model": SYNTH_MODEL,
        "messages": [
            { "role": "user", "content": "Hello, synthetic." }
        ],
        "params": {
            "max_tokens": 64,
            "temperature": 0.0
        }
    });
    if let Some(tag) = task_tag {
        body["task_tag"] = Value::String(tag.to_string());
    }
    body
}

/// POST a JSON body and return the raw `Response<Body>` (for streaming
/// reads). The `Accept: text/event-stream` header hints to DEV which
/// content-type to serve, though SSE handlers typically ignore Accept and
/// just set the response content-type explicitly.
async fn oneshot_post_sse(app: &Router, uri: &str, value: &Value) -> axum::response::Response {
    let bytes = serde_json::to_vec(value).expect("encode JSON body");
    let req = Request::builder()
        .method(Method::POST)
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::ACCEPT, "text/event-stream")
        .body(Body::from(bytes))
        .expect("build request");
    app.clone().oneshot(req).await.expect("oneshot")
}

/// Substring search "in order": returns `true` iff `needles` appear in
/// `haystack` in the given order (allowing arbitrary text between them).
fn contains_in_order(haystack: &str, needles: &[&str]) -> bool {
    let mut cursor = 0usize;
    for n in needles {
        match haystack[cursor..].find(n) {
            Some(off) => cursor += off + n.len(),
            None => return false,
        }
    }
    true
}

/// Count occurrences of `"data:"` line prefixes in an SSE body buffer.
/// Tolerates `"data:"` and `"data: "` (with or without space).
fn count_data_events(sse: &str) -> usize {
    sse.lines().filter(|line| line.starts_with("data:")).count()
}

/// Find the JSON payload that follows the LAST `event: done` line, or
/// `None` if there isn't one. Tolerates `event:done` and `event: done`.
fn extract_done_json(sse: &str) -> Option<Value> {
    let mut last_done_data: Option<String> = None;
    let mut in_done_frame = false;
    for line in sse.lines() {
        let trimmed = line.trim_end();
        if trimmed.is_empty() {
            in_done_frame = false;
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix("event:") {
            in_done_frame = rest.trim() == "done";
            continue;
        }
        if in_done_frame && let Some(rest) = trimmed.strip_prefix("data:") {
            last_done_data = Some(rest.trim().to_string());
        }
    }
    last_done_data.and_then(|s| serde_json::from_str(&s).ok())
}

/// Has the SSE body emitted at least one frame with `event: error`?
fn has_error_event(sse: &str) -> bool {
    sse.lines()
        .any(|line| line.trim_end().strip_prefix("event:").map(|r| r.trim()) == Some("error"))
}

// ---------------------------------------------------------------------------
// Tests — required by the Wave A5 P7-TEST task prompt
// ---------------------------------------------------------------------------

#[tokio::test]
async fn dispatch_with_synthetic_provider_returns_200_sse() {
    let (_tmp, _root, app) = boot_app_with_synthetic_router().await;
    let body = sample_completion_request(None);
    let resp = oneshot_post_sse(&app, "/api/dispatch", &body).await;

    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "POST /api/dispatch with router=Some(synth) MUST return 200; status={}",
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
        "dispatch must serve text/event-stream; got content-type={ct:?}",
    );

    // Drain the body — should observe at least 3 `data:` events.
    let sse_text = read_sse_body_with_timeout(resp, Duration::from_secs(5), 64 * 1024).await;
    let n = count_data_events(&sse_text);
    assert!(
        n >= 3,
        "synthetic dispatch must emit ≥3 `data:` events; got {n}, body={sse_text:?}",
    );
}

#[tokio::test]
async fn dispatch_with_synthetic_provider_emits_chunks_in_order() {
    let (_tmp, _root, app) = boot_app_with_synthetic_router().await;
    let body = sample_completion_request(None);
    let resp = oneshot_post_sse(&app, "/api/dispatch", &body).await;
    assert_eq!(resp.status(), StatusCode::OK);

    let sse_text = read_sse_body_with_timeout(resp, Duration::from_secs(5), 64 * 1024).await;

    // The three synthetic chunks must appear in order somewhere in the body.
    // We don't require strict SSE frame parsing — any encoding that surfaces
    // the substrings in order is acceptable.
    assert!(
        contains_in_order(&sse_text, SYNTHETIC_CHUNKS),
        "SSE body must contain chunks {SYNTHETIC_CHUNKS:?} in order; body={sse_text:?}",
    );
}

#[tokio::test]
async fn dispatch_emits_done_event_at_stream_end() {
    let (_tmp, _root, app) = boot_app_with_synthetic_router().await;
    let body = sample_completion_request(None);
    let resp = oneshot_post_sse(&app, "/api/dispatch", &body).await;
    assert_eq!(resp.status(), StatusCode::OK);

    let sse_text = read_sse_body_with_timeout(resp, Duration::from_secs(5), 64 * 1024).await;

    let done = extract_done_json(&sse_text).unwrap_or_else(|| {
        panic!("SSE body must end with an `event: done` frame carrying JSON: {sse_text:?}");
    });
    assert!(
        done.is_object(),
        "`done` event payload must be a JSON object: {done}",
    );
    // Minimum LedgerEntry summary fields — DEV may add more.
    for field in &["provider", "model"] {
        assert!(
            done.get(field).is_some(),
            "`done` event JSON must carry `{field}`: {done}",
        );
    }
    // The provider must be the synthetic one we registered.
    if let Some(p) = done.get("provider").and_then(|v| v.as_str()) {
        assert_eq!(
            p, SYNTH_KEY,
            "`done.provider` must be the synthetic provider key: {done}",
        );
    }
}

#[tokio::test]
async fn dispatch_propagates_task_tag_to_ledger() {
    let (_tmp, root, app) = boot_app_with_synthetic_router().await;
    let task_tag = "test-run-1";
    let body = sample_completion_request(Some(task_tag));
    let resp = oneshot_post_sse(&app, "/api/dispatch", &body).await;
    assert_eq!(
        resp.status(),
        StatusCode::OK,
        "dispatch with task_tag must still 200",
    );
    // Drain the SSE body so the server-side dispatch task finishes writing
    // its ledger entry before we re-open the store.
    let _drain = read_sse_body_with_timeout(resp, Duration::from_secs(5), 64 * 1024).await;

    // The router writes to JSONL synchronously inside `Router::dispatch()`
    // (which the route awaited before closing the stream). Verify the
    // task_tag landed in the canonical JSONL source-of-truth.
    let jsonl_path = root.join(LEDGER_JSONL_PATH);
    let jsonl_bytes = tokio::fs::read(&jsonl_path)
        .await
        .expect("router ledger JSONL must exist after dispatch");
    let jsonl_text = String::from_utf8(jsonl_bytes).expect("ledger is UTF-8");
    let lines: Vec<&str> = jsonl_text.split('\n').filter(|s| !s.is_empty()).collect();
    assert!(
        !lines.is_empty(),
        "dispatch must append at least one JSONL line; got empty file",
    );
    // The last (most recent) line should carry our task_tag.
    let last: Value =
        serde_json::from_str(lines.last().unwrap()).expect("ledger lines must be valid JSON");
    assert_eq!(
        last.get("task_tag").and_then(|v| v.as_str()),
        Some(task_tag),
        "last ledger entry must carry task_tag={task_tag:?}; got entry={last}",
    );

    // Round-trip through the route: reopen the store so the cold-start
    // `sync_from_jsonl` populates the SQLite materialised view, then issue
    // GET /api/ledger/recent?n=1.
    let store2 = Store::open(&root).await.expect("Store::reopen");
    let synth_router = build_synthetic_router(&root).await;
    let state2 = AppState::new(store2, Some(synth_router), root.clone());
    let app2 = build_router(state2);

    let req = Request::builder()
        .method(Method::GET)
        .uri("/api/ledger/recent?n=1")
        .body(Body::empty())
        .expect("build request");
    let resp2 = app2.oneshot(req).await.expect("oneshot");
    let (status, ledger_body) = status_and_json(resp2).await;
    assert_eq!(status, StatusCode::OK);
    let entries = ledger_body
        .get("entries")
        .and_then(|v| v.as_array())
        .cloned()
        .or_else(|| ledger_body.as_array().cloned())
        .unwrap_or_else(|| panic!("ledger body shape unrecognised: {ledger_body}"));
    assert_eq!(
        entries.len(),
        1,
        "?n=1 must return exactly one entry; got {entries:?}",
    );
    assert_eq!(
        entries[0].get("task_tag").and_then(|v| v.as_str()),
        Some(task_tag),
        "ledger entry surfaced by /api/ledger/recent must carry task_tag={task_tag:?}",
    );
}

#[tokio::test]
async fn dispatch_without_task_tag_records_none_in_ledger() {
    let (_tmp, root, app) = boot_app_with_synthetic_router().await;
    let body = sample_completion_request(None);
    let resp = oneshot_post_sse(&app, "/api/dispatch", &body).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let _drain = read_sse_body_with_timeout(resp, Duration::from_secs(5), 64 * 1024).await;

    let jsonl_path = root.join(LEDGER_JSONL_PATH);
    let jsonl_text = tokio::fs::read_to_string(&jsonl_path)
        .await
        .expect("router ledger JSONL must exist after dispatch");
    let last_line = jsonl_text
        .lines()
        .rev()
        .find(|line| !line.is_empty())
        .expect("ledger line");
    let last: Value = serde_json::from_str(last_line).expect("ledger line JSON");
    assert!(
        last.get("task_tag").is_some_and(Value::is_null),
        "omitted task_tag must record JSON null; got entry={last}",
    );
}

#[tokio::test]
async fn dispatch_empty_task_tag_normalises_to_none() {
    let (_tmp, root, app) = boot_app_with_synthetic_router().await;
    let body = sample_completion_request(Some(""));
    let resp = oneshot_post_sse(&app, "/api/dispatch", &body).await;
    assert_eq!(resp.status(), StatusCode::OK);
    let sse_text = read_sse_body_with_timeout(resp, Duration::from_secs(5), 64 * 1024).await;
    assert!(
        sse_text.contains(r#""task_tag":null"#),
        "empty task_tag must echo as null in done payload: {sse_text}",
    );

    let jsonl_path = root.join(LEDGER_JSONL_PATH);
    let jsonl_text = tokio::fs::read_to_string(&jsonl_path)
        .await
        .expect("router ledger JSONL must exist after dispatch");
    let last_line = jsonl_text
        .lines()
        .rev()
        .find(|line| !line.is_empty())
        .expect("ledger line");
    let last: Value = serde_json::from_str(last_line).expect("ledger line JSON");
    assert!(
        last.get("task_tag").is_some_and(Value::is_null),
        "empty task_tag must record JSON null; got entry={last}",
    );
}

#[tokio::test]
async fn dispatch_task_tag_too_long_returns_400() {
    let (_tmp, _root, app) = boot_app_with_synthetic_router().await;
    let long_tag = "x".repeat(257);
    let body = sample_completion_request(Some(&long_tag));
    let resp = oneshot_post_sse(&app, "/api/dispatch", &body).await;
    let (status, body) = status_and_json(resp).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(
        body.get("code").and_then(Value::as_str),
        Some("task_tag_too_long")
    );
}

#[tokio::test]
async fn dispatch_task_tag_with_newline_returns_400() {
    let (_tmp, _root, app) = boot_app_with_synthetic_router().await;
    let body = sample_completion_request(Some("code-review\nretry"));
    let resp = oneshot_post_sse(&app, "/api/dispatch", &body).await;
    let (status, body) = status_and_json(resp).await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(
        body.get("code").and_then(Value::as_str),
        Some("task_tag_invalid_chars"),
    );
}

#[tokio::test]
async fn dispatch_with_invalid_body_returns_400() {
    let (_tmp, _root, app) = boot_app_with_synthetic_router().await;
    // Send raw non-JSON bytes; the JSON extractor must reject before
    // the SSE machinery starts.
    let resp = oneshot_post_bytes(&app, "/api/dispatch", "text/plain", b"not json".to_vec()).await;
    let status = resp.status();
    assert!(
        status == StatusCode::BAD_REQUEST
            || status == StatusCode::UNPROCESSABLE_ENTITY
            || status == StatusCode::UNSUPPORTED_MEDIA_TYPE,
        "malformed body must return 400/415/422 (DEV's choice); got {status}",
    );

    // The 400 envelope must be JSON with `code: "invalid_input"` per the
    // spec — but tolerate `invalid_body` (the auth route's choice) too.
    let (_status, body) = status_and_json(resp).await;
    assert!(body.is_object(), "error body must be JSON object: {body}");
    let code = body
        .get("code")
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| panic!("error body must carry `code`: {body}"));
    assert!(
        code == "invalid_input" || code == "invalid_body" || code == "invalid_request",
        "error code should be `invalid_input` (preferred) or `invalid_body`/`invalid_request`; got {code:?}",
    );
}

#[tokio::test]
async fn dispatch_with_invalid_json_body_returns_400() {
    // Malformed JSON (parseable as JSON-ish but failing the dispatch
    // schema): an empty object missing required fields. DEV may either
    // 400 immediately (preferred) or 200 + SSE error event. Accept both.
    let (_tmp, _root, app) = boot_app_with_synthetic_router().await;
    let resp = oneshot_post_json(&app, "/api/dispatch", &json!({})).await;
    let status = resp.status();
    assert!(
        status == StatusCode::BAD_REQUEST
            || status == StatusCode::UNPROCESSABLE_ENTITY
            || status == StatusCode::OK,
        "empty-body dispatch must be 400/422 (body-first) or 200 + SSE error \
         (lenient body parse); got {status}",
    );
    if status == StatusCode::OK {
        // The lenient-parse branch must surface an error event in the SSE body.
        let sse_text = read_sse_body_with_timeout(resp, Duration::from_secs(3), 16 * 1024).await;
        assert!(
            has_error_event(&sse_text),
            "lenient-parse must emit `event: error` for invalid schema; body={sse_text:?}",
        );
    }
}

#[tokio::test]
async fn dispatch_with_router_some_but_no_providers_returns_error_event() {
    // Edge case: AppState.router = Some(_) but the inner Router was built
    // with an empty `preferred` list → `Router::dispatch` returns
    // `RouterError::NoProvider`. The dispatch route MUST surface this as
    // an in-stream SSE `event: error` (not a 503).
    let (_tmp, _root, app) = boot_app_with_router_some_no_providers().await;
    let body = sample_completion_request(None);
    let resp = oneshot_post_sse(&app, "/api/dispatch", &body).await;
    let status = resp.status();

    // Two acceptable shapes:
    //   (a) 200 OK + SSE body containing `event: error` (preferred — the
    //       client UI is already wired for SSE, so a same-stream error
    //       avoids a special-case branch).
    //   (b) 5xx with a JSON error envelope where `code != "router_not_configured"`
    //       (the 503 path is reserved for the `is_none()` branch).
    if status == StatusCode::OK {
        let sse_text = read_sse_body_with_timeout(resp, Duration::from_secs(3), 16 * 1024).await;
        assert!(
            has_error_event(&sse_text),
            "router=Some+empty-preferred must emit SSE `event: error`; body={sse_text:?}",
        );
    } else {
        assert!(
            status.is_server_error() || status.is_client_error(),
            "non-OK status must be 4xx/5xx; got {status}",
        );
        // If the server chose to respond with a JSON envelope, make sure
        // it's NOT misclassified as `router_not_configured` (that code is
        // reserved for AppState.router.is_none()).
        let (_s, jbody) = status_and_json(resp).await;
        if let Some(code) = jbody.get("code").and_then(|v| v.as_str()) {
            assert_ne!(
                code, "router_not_configured",
                "router=Some(_) MUST NOT return code `router_not_configured`; body={jbody}",
            );
        }
    }
}
