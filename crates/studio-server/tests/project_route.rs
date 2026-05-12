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

//! `GET /api/project/current` integration contract — Wave A4 P7-TEST (red).
//!
//! Locks the wire-shape binding for the single project route named in
//! `docs/agent/modules/studio-server.md` §"Wave A4 target":
//!
//! - `GET /api/project/current` → `{ project_root, started_at, version }`
//!
//! API-shape assumptions made (DEV must satisfy; CTO reconciles drift):
//!
//! 1. **Response shape.** The body is a JSON object with at least the three
//!    fields named in the dispatch spec: `project_root` (absolute path
//!    string), `started_at` (RFC 3339 string), `version` (semver string).
//! 2. **`project_root` matches AppState.** The string must canonicalise to
//!    the same path the `Store` was opened at.
//! 3. **`version` matches the crate.** Equals `studio_server::version()`
//!    which itself equals `env!("CARGO_PKG_VERSION")`.
//! 4. **`started_at` is parseable RFC 3339.** Not arbitrary text.

mod common;

use axum::http::StatusCode;
use common::{fresh_app, oneshot_get, status_and_json};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

#[tokio::test]
async fn project_current_returns_200() {
    let (_tmp, _root, app) = fresh_app().await;
    let resp = oneshot_get(&app, "/api/project/current").await;
    let (status, body) = status_and_json(resp).await;
    assert_eq!(
        status,
        StatusCode::OK,
        "GET /api/project/current must return 200: body={body}",
    );
    assert!(
        body.is_object(),
        "project current body must be a JSON object: {body}",
    );
}

#[tokio::test]
async fn project_current_carries_project_root() {
    let (_tmp, root, app) = fresh_app().await;
    let resp = oneshot_get(&app, "/api/project/current").await;
    let (status, body) = status_and_json(resp).await;
    assert_eq!(status, StatusCode::OK);

    let pr = body
        .get("project_root")
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| panic!("body must carry `project_root` string: {body}"));
    let pr_path = std::path::PathBuf::from(pr);
    let pr_canon = pr_path.canonicalize().unwrap_or(pr_path.clone());
    let root_canon = root.canonicalize().unwrap_or(root.clone());
    assert_eq!(
        pr_canon, root_canon,
        "project_root must round-trip to the AppState project root \
         (body={pr:?} canon={pr_canon:?} root={root:?} root_canon={root_canon:?})",
    );
}

#[tokio::test]
async fn project_current_carries_started_at_rfc3339() {
    let before = OffsetDateTime::now_utc();
    let (_tmp, _root, app) = fresh_app().await;

    let resp = oneshot_get(&app, "/api/project/current").await;
    let (status, body) = status_and_json(resp).await;
    assert_eq!(status, StatusCode::OK);

    let started_str = body
        .get("started_at")
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| panic!("body must carry `started_at` string: {body}"));
    let started = OffsetDateTime::parse(started_str, &Rfc3339)
        .unwrap_or_else(|e| panic!("started_at must be valid RFC 3339; got {started_str:?}: {e}"));
    let after = OffsetDateTime::now_utc();
    // Two-second slack each side covers any clock-granularity jitter on
    // the build agents.
    assert!(
        started >= before - time::Duration::seconds(2),
        "started_at ({started}) must be no earlier than pre-construction ({before})",
    );
    assert!(
        started <= after + time::Duration::seconds(2),
        "started_at ({started}) must be no later than post-fetch ({after})",
    );
}

#[tokio::test]
async fn project_current_version_matches_crate() {
    let (_tmp, _root, app) = fresh_app().await;
    let resp = oneshot_get(&app, "/api/project/current").await;
    let (status, body) = status_and_json(resp).await;
    assert_eq!(status, StatusCode::OK);

    let v = body
        .get("version")
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| panic!("body must carry `version` string: {body}"));
    assert!(!v.is_empty(), "version must be non-empty: {body}");
    assert_eq!(
        v,
        studio_server::version(),
        "body.version must equal studio_server::version(); got {v} vs {}",
        studio_server::version(),
    );
    assert_eq!(
        v,
        env!("CARGO_PKG_VERSION"),
        "body.version must equal CARGO_PKG_VERSION; got {v} vs {}",
        env!("CARGO_PKG_VERSION"),
    );
}
