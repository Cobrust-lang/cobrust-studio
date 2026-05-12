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

//! `POST /api/dispatch` integration contract — Wave A4 P7-TEST (red).
//!
//! Locks the *router-not-configured* surface only — the live-dispatch path
//! (router=`Some(_)`) is A5's scope, not A4.
//!
//! Per `docs/agent/modules/studio-server.md` §"AppState.router contract"
//! (the "must return 503 with code `router_not_configured`" rule):
//!
//! - `POST /api/dispatch` body: `CompletionRequest` → **503** with body
//!   `{ error, code: "router_not_configured" }` when
//!   `AppState.router.is_none()`.
//!
//! A4 default is `router: None` (the `boot_app_*` helpers all construct
//! `AppState::new(store, None, root)`). So every POST in this file targets
//! the 503 branch.

mod common;

use axum::http::StatusCode;
use common::{boot_app_with_empty_store, oneshot_post_bytes, oneshot_post_json, status_and_json};
use serde_json::json;

/// A minimal valid `studio_router::CompletionRequest`-shaped body. The
/// dispatch route may or may not validate the body before checking the
/// router-is-configured precondition — both orderings are acceptable
/// (DEV's choice). The 503 must fire when the precondition fails
/// regardless of body validity.
fn sample_completion_request() -> serde_json::Value {
    json!({
        "model": "claude-opus-4-7",
        "messages": [
            { "role": "user", "content": "Hello." }
        ],
        "params": {
            "max_tokens": 128,
            "temperature": 0.0
        }
    })
}

#[tokio::test]
async fn dispatch_returns_503_when_router_not_configured() {
    let (_tmp, _root, app) = boot_app_with_empty_store().await;

    let body = sample_completion_request();
    let resp = oneshot_post_json(&app, "/api/dispatch", &body).await;
    let (status, body) = status_and_json(resp).await;

    assert_eq!(
        status,
        StatusCode::SERVICE_UNAVAILABLE,
        "POST /api/dispatch with router=None MUST return 503: body={body}",
    );

    let code = body
        .get("code")
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| panic!("503 body must carry `code`: {body}"));
    assert_eq!(
        code, "router_not_configured",
        "503 code must be `router_not_configured`, got `{code}`",
    );
    let err = body
        .get("error")
        .unwrap_or_else(|| panic!("503 body must carry `error`: {body}"));
    assert!(
        err.is_string() || err.is_object(),
        "`error` field must be string-or-object: {body}",
    );
}

#[tokio::test]
async fn dispatch_503_even_for_empty_body_when_router_missing() {
    // Whether DEV validates the body before or after the router-check, the
    // contract is "503 when router is None" — so an empty body must also
    // get 503 (or, if DEV chose to validate first, a 4xx). Lock the
    // disjunction so both orderings pass.
    let (_tmp, _root, app) = boot_app_with_empty_store().await;
    let resp = oneshot_post_json(&app, "/api/dispatch", &json!({})).await;
    let status = resp.status();
    assert!(
        status == StatusCode::SERVICE_UNAVAILABLE
            || status == StatusCode::BAD_REQUEST
            || status == StatusCode::UNPROCESSABLE_ENTITY,
        "dispatch with empty body must be 503 (router-first) or 400/422 (body-first), got {status}",
    );
}

#[tokio::test]
async fn dispatch_503_for_non_json_body_when_router_missing() {
    // Non-JSON: same disjunction — either 4xx (body-rejected) or 503
    // (router-checked first).
    let (_tmp, _root, app) = boot_app_with_empty_store().await;
    let resp = oneshot_post_bytes(&app, "/api/dispatch", "text/plain", b"not json".to_vec()).await;
    let status = resp.status();
    assert!(
        status.is_client_error() || status == StatusCode::SERVICE_UNAVAILABLE,
        "dispatch with non-JSON body must be 4xx or 503, got {status}",
    );
}

#[tokio::test]
async fn dispatch_503_body_is_json_envelope() {
    // The 503 body MUST be a JSON object with `error` AND `code` — the
    // SvelteKit error boundary depends on the same envelope shape across
    // all 4xx/5xx responses.
    let (_tmp, _root, app) = boot_app_with_empty_store().await;
    let resp = oneshot_post_json(&app, "/api/dispatch", &sample_completion_request()).await;
    let (status, body) = status_and_json(resp).await;
    assert_eq!(status, StatusCode::SERVICE_UNAVAILABLE);
    assert!(body.is_object(), "503 body must be a JSON object: {body}");
    assert!(body.get("error").is_some());
    assert!(body.get("code").is_some());
}
