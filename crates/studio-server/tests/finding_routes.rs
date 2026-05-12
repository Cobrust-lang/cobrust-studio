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

//! Finding routes integration contract — Wave A4 P7-TEST (red).
//!
//! Symmetric to `adr_routes.rs`. Locks the wire-shape binding for:
//!
//! - `GET  /api/finding` → `{ "findings": [FindingSummary, ...] }`
//! - `POST /api/finding` → 201 `Finding`
//!
//! API-shape assumptions made (DEV must satisfy; CTO reconciles drift):
//!
//! 1. **List envelope.** `GET /api/finding` returns `{"findings": [...]}`
//!    (preferred) or a bare array (tolerated).
//! 2. **Summary shape.** Each list element carries at least
//!    `finding_id` (slug string) / `title` / `status` / `severity`.
//! 3. **Create body.** `POST /api/finding` accepts a JSON document whose
//!    field names mirror `studio_store::FindingDraft` serde-shape:
//!    `finding_id`, `last_verified_commit`, `severity`, `status`,
//!    `dependencies`, `related`, `title`, `body`.
//! 4. **Error envelope.** All `4xx` responses are JSON objects with at
//!    least `error` (string) and `code` (machine-readable string).

mod common;

use axum::http::StatusCode;
use common::{
    boot_app_with_empty_store, boot_app_with_seeded_findings, boot_app_with_store, json_body,
    oneshot_get, oneshot_post_bytes, oneshot_post_json, status_and_json,
};
use serde_json::{Value, json};

/// Pluck the `findings` list from the response, accepting either the
/// `{ "findings": [...] }` envelope or a bare top-level array.
fn extract_finding_list(body: &Value) -> Vec<Value> {
    if let Some(arr) = body.get("findings").and_then(|v| v.as_array()) {
        return arr.clone();
    }
    if let Some(arr) = body.as_array() {
        return arr.clone();
    }
    panic!("Finding list body must be `{{ \"findings\": [...] }}` or bare array: {body}");
}

fn make_finding_draft_json(id: &str, title: &str) -> Value {
    json!({
        "finding_id": id,
        "last_verified_commit": "3bb9aa6",
        "severity": "P3",
        "status": "open",
        "dependencies": ["adr:0006"],
        "related": [],
        "title": title,
        "body": format!("# {title}\n\n## Hypothesis\n\nbody\n"),
    })
}

#[tokio::test]
async fn get_finding_list_empty_returns_200_empty_array() {
    let (_tmp, _root, app) = boot_app_with_empty_store().await;

    let resp = oneshot_get(&app, "/api/finding").await;
    let (status, body) = status_and_json(resp).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "GET /api/finding on empty store must return 200: body={body}",
    );
    let list = extract_finding_list(&body);
    assert!(
        list.is_empty(),
        "fresh store must produce empty findings list: got {list:?}",
    );
}

#[tokio::test]
async fn post_finding_then_list_includes_it() {
    let (_tmp, _root, app) = boot_app_with_empty_store().await;

    let draft = make_finding_draft_json("a4-test-fixture-01", "First finding under test");
    let resp = oneshot_post_json(&app, "/api/finding", &draft).await;
    let create_status = resp.status();
    let created = json_body(resp).await;
    assert_eq!(
        create_status,
        StatusCode::CREATED,
        "POST /api/finding must return 201: body={created}",
    );
    let created_id = created
        .get("finding_id")
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| panic!("created finding must carry `finding_id`: {created}"));
    assert_eq!(created_id, "a4-test-fixture-01");

    let list_resp = oneshot_get(&app, "/api/finding").await;
    let (status, list_body) = status_and_json(list_resp).await;
    assert_eq!(status, StatusCode::OK);
    let list = extract_finding_list(&list_body);
    assert_eq!(
        list.len(),
        1,
        "list must surface the single POST'd finding: {list:?}",
    );
    let entry = &list[0];
    assert_eq!(
        entry.get("finding_id").and_then(|v| v.as_str()),
        Some("a4-test-fixture-01"),
    );
    assert_eq!(
        entry.get("severity").and_then(|v| v.as_str()),
        Some("P3"),
        "summary severity must roundtrip: {entry}",
    );
}

#[tokio::test]
async fn post_finding_malformed_body_returns_400() {
    let (_tmp, _root, app) = boot_app_with_empty_store().await;

    // Empty object: missing the required `finding_id` / `title` / `body`
    // / etc. fields.
    let resp = oneshot_post_json(&app, "/api/finding", &json!({})).await;
    let status = resp.status();
    let body = json_body(resp).await;
    assert!(
        status == StatusCode::BAD_REQUEST || status == StatusCode::UNPROCESSABLE_ENTITY,
        "POST /api/finding with empty body must be 400 (preferred) or 422, got {status}: body={body}",
    );
    assert!(
        body.is_object(),
        "4xx body for malformed POST must be a JSON object: got {body}",
    );
    assert!(
        body.get("error").is_some() || body.get("code").is_some(),
        "4xx body should carry `error` or `code`: got {body}",
    );
}

#[tokio::test]
async fn post_finding_non_json_returns_4xx() {
    let (_tmp, _root, app) = boot_app_with_empty_store().await;

    let resp = oneshot_post_bytes(
        &app,
        "/api/finding",
        "text/plain",
        b"not json at all".to_vec(),
    )
    .await;
    assert!(resp.status().is_client_error());
}

#[tokio::test]
async fn get_finding_list_with_seeded_findings() {
    let (_tmp, _root, _store, app) = boot_app_with_seeded_findings(2).await;

    let resp = oneshot_get(&app, "/api/finding").await;
    let (status, body) = status_and_json(resp).await;
    assert_eq!(status, StatusCode::OK);
    let list = extract_finding_list(&body);
    assert_eq!(list.len(), 2, "expected 2 seeded findings, got: {list:?}");

    // Seeded ids are `seed-000`, `seed-001`.
    let ids: Vec<&str> = list
        .iter()
        .filter_map(|e| e.get("finding_id").and_then(|v| v.as_str()))
        .collect();
    assert!(ids.contains(&"seed-000"), "missing seed-000 in {ids:?}");
    assert!(ids.contains(&"seed-001"), "missing seed-001 in {ids:?}");
}

#[tokio::test]
async fn post_finding_persists_to_store() {
    // Round-trip via the store-side handle: the route MUST persist via
    // studio_store::finding().create() so the file lands on disk.
    let (_tmp, _root, store, app) = boot_app_with_store().await;
    let draft = make_finding_draft_json("a4-roundtrip-01", "Roundtrip via HTTP");
    let resp = oneshot_post_json(&app, "/api/finding", &draft).await;
    assert_eq!(resp.status(), StatusCode::CREATED);

    let from_store = store
        .finding()
        .get("a4-roundtrip-01")
        .await
        .expect("store.finding().get OK")
        .unwrap_or_else(|| panic!("Finding must be persisted to the store"));
    assert_eq!(from_store.title(), "Roundtrip via HTTP");
    assert!(from_store.body().contains("Hypothesis"));
}
