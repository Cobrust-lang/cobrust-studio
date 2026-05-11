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

//! `GET /api/version` smoke contract — Wave A3 P7-TEST (red).
//!
//! Locks the smoke-level binding for the M1 version route. Anchored to
//! `docs/agent/modules/studio-server.md` and the Wave A3 dispatch.
//!
//! Assumed response shape (DEV's impl must satisfy):
//!
//! ```json
//! {
//!   "studio_server": "<semver>",
//!   "studio_store":  "<semver>",
//!   "studio_router": "<semver>",
//!   "rustc":         "<rustc -V output or `RUSTC_VERSION` env at build time>"
//! }
//! ```
//!
//! Tests:
//! - `version_returns_crate_versions`
//! - `version_matches_cargo_pkg_version`

mod common;

use axum::http::StatusCode;
use common::{fresh_app, oneshot_get, status_and_json};

const REQUIRED_KEYS: &[&str] = &["studio_server", "studio_store", "studio_router", "rustc"];

#[tokio::test]
async fn version_returns_crate_versions() {
    let (_tmp, _root, app) = fresh_app().await;

    let resp = oneshot_get(&app, "/api/version").await;
    let (status, body) = status_and_json(resp).await;

    assert_eq!(
        status,
        StatusCode::OK,
        "version must return 200: body={body}",
    );
    let obj = body
        .as_object()
        .unwrap_or_else(|| panic!("version body must be a JSON object, got: {body}"));

    for key in REQUIRED_KEYS {
        let v = obj
            .get(*key)
            .unwrap_or_else(|| panic!("version body missing `{key}` field: body={body}"));
        let s = v
            .as_str()
            .unwrap_or_else(|| panic!("version body `{key}` must be a string, got: {v}"));
        assert!(
            !s.is_empty(),
            "version body `{key}` must be non-empty: body={body}",
        );
    }
}

#[tokio::test]
async fn version_matches_cargo_pkg_version() {
    let (_tmp, _root, app) = fresh_app().await;

    let resp = oneshot_get(&app, "/api/version").await;
    let (status, body) = status_and_json(resp).await;
    assert_eq!(status, StatusCode::OK);

    let body_studio_server = body
        .get("studio_server")
        .and_then(|v| v.as_str())
        .expect("studio_server key present");

    // Two independent anchors must agree:
    //   (a) `env!("CARGO_PKG_VERSION")` — Cargo's compile-time injection,
    //       read from this test crate (== studio-server, since `tests/`
    //       integration tests compile inside the studio-server crate's
    //       package and inherit its `CARGO_PKG_VERSION`).
    //   (b) `studio_server::version()` — the function the server itself
    //       uses to populate the response.
    // Both must equal the body's `studio_server` field — otherwise the
    // server is reporting a divergent version (the Day-4 "what's running?"
    // failure mode CLAUDE.md §3.2 rules out).
    assert_eq!(
        body_studio_server,
        env!("CARGO_PKG_VERSION"),
        "body.studio_server ({body_studio_server}) must equal env! CARGO_PKG_VERSION ({})",
        env!("CARGO_PKG_VERSION"),
    );
    assert_eq!(
        body_studio_server,
        studio_server::version(),
        "body.studio_server ({body_studio_server}) must equal studio_server::version() ({})",
        studio_server::version(),
    );
}
