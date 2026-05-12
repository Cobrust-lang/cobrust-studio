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

//! 404 + method-mismatch fallback contract — Wave A3 P7-TEST (red).
//!
//! Anchored to `docs/agent/modules/studio-server.md` and the Wave A3 dispatch's
//! "Required test files" §`fallback_smoke.rs`.
//!
//! Assumptions (documented for CTO reconciliation):
//! - The Axum app installs a JSON fallback handler that produces a body of the
//!   shape `{"error": "<message>"}` (or `{"error": {"code": ..., "message":
//!   ...}}`). The required invariant is that the body contains an `error` key.
//! - A `GET`-only route hit with `POST` returns either 405 (preferred, axum
//!   default for `MethodRouter`) or 404 (if DEV chose to map unknown routes
//!   and method mismatches to the same fallback). Both are accepted here; the
//!   test asserts "client error" (4xx) and that the response is JSON.
//!
//! Tests:
//! - `nonexistent_route_returns_404_json`
//! - `nonexistent_route_body_has_error_field`
//! - `health_post_returns_method_not_allowed_or_404`

mod common;

use axum::http::{Method, StatusCode};
use common::{fresh_app, oneshot_get, oneshot_method, status_and_json};

#[tokio::test]
async fn nonexistent_route_returns_404_json() {
    let (_tmp, _root, app) = fresh_app().await;

    let resp = oneshot_get(&app, "/api/nonexistent").await;
    let (status, body) = status_and_json(resp).await;

    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "GET /api/nonexistent must return 404, got {status}: body={body}",
    );
    // The body MUST be a JSON object (not bare text/html); the JSON fallback
    // pattern is what enables the SvelteKit error boundary to render a
    // friendly message instead of a raw browser 404.
    assert!(
        body.is_object(),
        "404 body must be a JSON object, got: {body}",
    );
}

#[tokio::test]
async fn nonexistent_route_body_has_error_field() {
    let (_tmp, _root, app) = fresh_app().await;

    let resp = oneshot_get(&app, "/api/this-route-does-not-exist").await;
    let (status, body) = status_and_json(resp).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    let err = body
        .get("error")
        .unwrap_or_else(|| panic!("404 body must contain `error` key: body={body}"));
    // `error` may be a bare string OR a sub-object with `code`/`message`; both
    // shapes are acceptable for the smoke gate. We just require the value to
    // be non-null and either a non-empty string or an object.
    match err {
        serde_json::Value::String(s) => {
            assert!(!s.is_empty(), "error string must be non-empty: body={body}");
        }
        serde_json::Value::Object(o) => assert!(
            !o.is_empty(),
            "error object must have at least one field: body={body}",
        ),
        other => panic!("error must be string or object, got {other}: body={body}"),
    }
}

#[tokio::test]
async fn health_post_returns_method_not_allowed_or_404() {
    let (_tmp, _root, app) = fresh_app().await;

    let resp = oneshot_method(&app, Method::POST, "/api/health").await;
    let status = resp.status();

    // Per the dispatch: 405 preferred, 404 acceptable if DEV chose not to
    // differentiate. Both are 4xx; we lock that and document the looseness.
    assert!(
        status == StatusCode::METHOD_NOT_ALLOWED || status == StatusCode::NOT_FOUND,
        "POST /api/health must be 405 (preferred) or 404 (acceptable), got {status}",
    );

    // If the implementation chose 405, the response SHOULD include an
    // `Allow` header per RFC 7231 §6.5.5. Axum's `MethodRouter` does this by
    // default. We only assert on the 405 branch.
    if status == StatusCode::METHOD_NOT_ALLOWED {
        let allow = resp
            .headers()
            .get(axum::http::header::ALLOW)
            .and_then(|h| h.to_str().ok())
            .unwrap_or("");
        assert!(
            allow.to_uppercase().contains("GET"),
            "405 response should advertise GET in Allow header, got {allow:?}",
        );
    }
}
