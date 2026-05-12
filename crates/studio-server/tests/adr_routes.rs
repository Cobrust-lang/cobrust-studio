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

//! ADR routes integration contract — Wave A4 P7-TEST (red).
//!
//! Locks the wire-shape binding for the four M1 ADR routes named in
//! `docs/agent/modules/studio-server.md` §"Wave A4 target":
//!
//! - `GET  /api/adr`        → `{ "adrs": [AdrSummary, ...] }`
//! - `GET  /api/adr/:id`    → `Adr` body or 404 `{ error, code: "adr_not_found" }`
//! - `POST /api/adr`        → 201 `Adr`
//!
//! API-shape assumptions made (DEV must satisfy; CTO reconciles drift):
//!
//! 1. **List envelope.** `GET /api/adr` returns a JSON object with one key
//!    `"adrs"` whose value is a JSON array of summary objects. This matches
//!    the dispatch spec verbatim. An accepted alternative is a bare array
//!    at the top level; the tests below tolerate that shape and document
//!    the looseness.
//! 2. **Summary shape.** Each list element carries at least
//!    `adr_id` / `title` / `status` / `date`. `adr_id` may be a JSON number
//!    OR a 4-digit zero-padded string ("0007"); both are accepted.
//! 3. **Detail shape.** `GET /api/adr/:id` returns at least
//!    `adr_id` / `title` / `status` / `date` / `body`. The `body` field is
//!    the markdown after the closing `---` frontmatter fence (per
//!    `studio_store::Adr::body`).
//! 4. **Create body.** `POST /api/adr` accepts a JSON document with at
//!    least `title` / `status` / `date` / `body`. `supersedes` (array of
//!    ids) is optional and defaults to empty.
//! 5. **Error envelope.** All `4xx` responses are JSON objects with at
//!    least `error` (string message) and `code` (machine-readable string).
//!    The 404-on-unknown-id `code` is `"adr_not_found"`.
//! 6. **Create returns 201 with the created Adr body.** The body MUST
//!    carry the server-assigned `adr_id` so the caller can subsequently
//!    GET it.

mod common;

use axum::http::StatusCode;
use common::{
    boot_app_with_empty_store, boot_app_with_seeded_adrs, boot_app_with_store, json_body,
    oneshot_get, oneshot_post_bytes, oneshot_post_json, status_and_json,
};
use serde_json::{Value, json};

/// Pluck the `adrs` list out of the response body, accepting either the
/// `{ "adrs": [...] }` envelope or a bare top-level array.
fn extract_adr_list(body: &Value) -> Vec<Value> {
    if let Some(arr) = body.get("adrs").and_then(|v| v.as_array()) {
        return arr.clone();
    }
    if let Some(arr) = body.as_array() {
        return arr.clone();
    }
    panic!("ADR list body must be `{{ \"adrs\": [...] }}` or bare array: {body}");
}

/// Coerce a JSON value that's either a number `7` or a string `"0007"` to
/// the canonical u32 id.
fn coerce_adr_id(v: &Value) -> Option<u32> {
    if let Some(n) = v.as_u64() {
        return Some(u32::try_from(n).unwrap_or(0));
    }
    if let Some(s) = v.as_str() {
        return s.trim().parse::<u32>().ok();
    }
    None
}

#[tokio::test]
async fn get_adr_list_empty_returns_200_empty_array() {
    let (_tmp, _root, app) = boot_app_with_empty_store().await;

    let resp = oneshot_get(&app, "/api/adr").await;
    let (status, body) = status_and_json(resp).await;

    assert_eq!(
        status,
        StatusCode::OK,
        "GET /api/adr on empty store must return 200: body={body}",
    );
    let list = extract_adr_list(&body);
    assert!(
        list.is_empty(),
        "fresh store must produce empty `adrs` list: got {list:?}",
    );
}

#[tokio::test]
async fn post_adr_then_list_includes_it() {
    let (_tmp, _root, app) = boot_app_with_empty_store().await;

    let draft = json!({
        "title": "Pick a thing",
        "status": "proposed",
        "date": "2026-05-12",
        "body": "## Context\n\nWe need to pick a thing.\n\n## Decision\n\nWe did.\n",
        "supersedes": [],
    });

    let resp = oneshot_post_json(&app, "/api/adr", &draft).await;
    let create_status = resp.status();
    let created = json_body(resp).await;
    assert_eq!(
        create_status,
        StatusCode::CREATED,
        "POST /api/adr must return 201 Created: body={created}",
    );
    let assigned_id = coerce_adr_id(
        created
            .get("adr_id")
            .unwrap_or_else(|| panic!("created adr must carry `adr_id`: {created}")),
    )
    .unwrap_or_else(|| panic!("`adr_id` must coerce to u32: {created}"));
    assert!(
        assigned_id >= 1,
        "assigned adr_id must be a positive integer, got {assigned_id}",
    );

    let list_resp = oneshot_get(&app, "/api/adr").await;
    let (status, list_body) = status_and_json(list_resp).await;
    assert_eq!(status, StatusCode::OK);
    let list = extract_adr_list(&list_body);
    assert_eq!(
        list.len(),
        1,
        "after one POST the list must have exactly one entry: {list:?}",
    );
    let entry = &list[0];
    let entry_id = coerce_adr_id(
        entry
            .get("adr_id")
            .unwrap_or_else(|| panic!("summary missing adr_id: {entry}")),
    )
    .expect("entry adr_id coerces");
    assert_eq!(
        entry_id, assigned_id,
        "list entry adr_id must equal the id returned by POST",
    );
    assert_eq!(
        entry.get("title").and_then(|v| v.as_str()),
        Some("Pick a thing"),
        "list entry title must roundtrip from the POST body: {entry}",
    );
}

#[tokio::test]
async fn get_adr_by_id_returns_full_body() {
    let (_tmp, _root, app) = boot_app_with_empty_store().await;

    let draft = json!({
        "title": "Decide a thing",
        "status": "accepted",
        "date": "2026-05-12",
        "body": "## Context\n\nWhy.\n\n## Decision\n\nBecause.\n",
        "supersedes": [],
    });
    let create_resp = oneshot_post_json(&app, "/api/adr", &draft).await;
    assert_eq!(create_resp.status(), StatusCode::CREATED);
    let created = json_body(create_resp).await;
    let id = coerce_adr_id(created.get("adr_id").expect("adr_id present")).expect("u32 id");

    let fetch_resp = oneshot_get(&app, &format!("/api/adr/{id}")).await;
    let (status, body) = status_and_json(fetch_resp).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "GET /api/adr/{id} on a freshly-POST'd id must return 200: body={body}",
    );

    let fetched_id = coerce_adr_id(body.get("adr_id").expect("fetched adr_id")).expect("u32");
    assert_eq!(fetched_id, id);
    assert_eq!(
        body.get("title").and_then(|v| v.as_str()),
        Some("Decide a thing"),
    );
    assert_eq!(
        body.get("status").and_then(|v| v.as_str()),
        Some("accepted"),
    );
    assert_eq!(
        body.get("date").and_then(|v| v.as_str()),
        Some("2026-05-12")
    );
    let body_field = body
        .get("body")
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| panic!("ADR detail must carry `body`: {body}"));
    assert!(
        body_field.contains("Because."),
        "ADR.body must contain the verbatim decision text; got: {body_field}",
    );
}

#[tokio::test]
async fn get_adr_unknown_id_returns_404() {
    let (_tmp, _root, app) = boot_app_with_empty_store().await;

    // 99 is impossibly large for a fresh store (next id is 1).
    let resp = oneshot_get(&app, "/api/adr/99").await;
    let (status, body) = status_and_json(resp).await;
    assert_eq!(
        status,
        StatusCode::NOT_FOUND,
        "GET /api/adr/99 on empty store must return 404: body={body}",
    );
    // The 404 envelope MUST carry `{ error, code }` per the dispatch spec.
    let code = body
        .get("code")
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| panic!("404 body must carry `code`: {body}"));
    assert_eq!(
        code, "adr_not_found",
        "unknown-adr 404 must use code `adr_not_found`, got `{code}`",
    );
    let err = body
        .get("error")
        .unwrap_or_else(|| panic!("404 body must carry `error` field: {body}"));
    assert!(
        err.is_string() || err.is_object(),
        "`error` must be string-or-object: {body}",
    );
}

#[tokio::test]
async fn post_adr_malformed_body_returns_400() {
    let (_tmp, _root, app) = boot_app_with_empty_store().await;

    // Empty object: missing `title` / `status` / `date` / `body`. axum's
    // `Json<T>` extractor rejects with 400 by default; the dispatch spec
    // says we forward as 400 with an error envelope.
    let resp = oneshot_post_json(&app, "/api/adr", &json!({})).await;
    let status = resp.status();
    let body = json_body(resp).await;
    assert!(
        status == StatusCode::BAD_REQUEST || status == StatusCode::UNPROCESSABLE_ENTITY,
        "POST /api/adr with empty body must be 400 (preferred) or 422, got {status}: body={body}",
    );
    // The body should still be a JSON object with at least an `error` key
    // — axum's default rejection is JSON-shaped when the route uses
    // `axum::Json<T>`, and the dispatch wraps it via `JsonRejection`.
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
async fn post_adr_non_json_body_returns_4xx() {
    let (_tmp, _root, app) = boot_app_with_empty_store().await;

    // Send literal garbage with a text/plain content-type — axum's Json
    // extractor must refuse. Lock 4xx (specifically 415 OR 400) and
    // confirm the body parser didn't accept it.
    let resp =
        oneshot_post_bytes(&app, "/api/adr", "text/plain", b"not json at all".to_vec()).await;
    let status = resp.status();
    assert!(
        status.is_client_error(),
        "non-JSON POST must be a client error: got {status}",
    );
}

#[tokio::test]
async fn get_adr_list_with_seeded_adrs_returns_ascending() {
    // Pre-seed 3 ADRs via the store API; the list endpoint must surface
    // all three in ascending id order (matching the store contract).
    let (_tmp, _root, _store, app) = boot_app_with_seeded_adrs(3).await;

    let resp = oneshot_get(&app, "/api/adr").await;
    let (status, body) = status_and_json(resp).await;
    assert_eq!(status, StatusCode::OK);

    let list = extract_adr_list(&body);
    assert_eq!(
        list.len(),
        3,
        "list must surface all 3 seeded ADRs: {list:?}",
    );
    let ids: Vec<u32> = list
        .iter()
        .map(|e| coerce_adr_id(e.get("adr_id").expect("entry has adr_id")).expect("u32"))
        .collect();
    let mut sorted = ids.clone();
    sorted.sort_unstable();
    assert_eq!(
        ids, sorted,
        "list must be ascending by adr_id (matches store contract): got {ids:?}",
    );
}

#[tokio::test]
async fn post_adr_persists_to_store() {
    // Round-trip via the store-side handle: the route MUST go through
    // studio_store::adr().create() so the file lands on disk AND in the
    // SQLite index. This test bypasses the HTTP layer for the read so a
    // future regression where the route shadows storage (in-memory cache,
    // etc.) is caught.
    let (_tmp, _root, store, app) = boot_app_with_store().await;
    let draft = json!({
        "title": "Persisted via HTTP",
        "status": "proposed",
        "date": "2026-05-12",
        "body": "## Context\n\nVia HTTP\n",
        "supersedes": [],
    });
    let resp = oneshot_post_json(&app, "/api/adr", &draft).await;
    assert_eq!(resp.status(), StatusCode::CREATED);
    let body = json_body(resp).await;
    let id = coerce_adr_id(body.get("adr_id").unwrap()).expect("u32");

    // Read back through the store API — the route's POST handler MUST
    // have persisted via studio_store, otherwise this returns None.
    let from_store = store
        .adr()
        .get(id)
        .await
        .expect("store.adr().get OK")
        .unwrap_or_else(|| panic!("ADR id={id} must be persisted to the store"));
    assert_eq!(from_store.title(), "Persisted via HTTP");
    assert!(from_store.body().contains("Via HTTP"));
}
