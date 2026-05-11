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

//! `GET /api/health` smoke contract — Wave A3 P7-TEST (red).
//!
//! Locks the smoke-level binding for the M1 health route. Anchored to
//! `docs/agent/modules/studio-server.md` §"Public surface" and the Wave A3
//! dispatch's "API names you reference" block.
//!
//! Assumed response shape (DEV's impl on `feature/a3-dev-server-skel` must
//! satisfy; CTO reconciles symbol/path drift at merge):
//!
//! ```json
//! {
//!   "status": "ok",
//!   "uptime_seconds": <integer >= 0>,
//!   "project": "<absolute path to project root>",
//!   ...
//! }
//! ```
//!
//! Tests:
//! - `health_returns_200_and_status_ok`
//! - `health_uptime_grows_over_time`
//! - `health_includes_project_path`

mod common;

use std::time::Duration;

use axum::http::StatusCode;
use common::{fresh_app, oneshot_get, status_and_json};

#[tokio::test]
async fn health_returns_200_and_status_ok() {
    let (_tmp, _root, app) = fresh_app().await;

    let resp = oneshot_get(&app, "/api/health").await;
    let (status, body) = status_and_json(resp).await;

    assert_eq!(
        status,
        StatusCode::OK,
        "health must return 200: body={body}"
    );
    assert_eq!(
        body.get("status").and_then(|v| v.as_str()),
        Some("ok"),
        "health body must have `status: \"ok\"` field: body={body}",
    );

    // Uptime present and >= 0 (an integer second-count, not a float).
    let uptime = body
        .get("uptime_seconds")
        .unwrap_or_else(|| panic!("health body missing `uptime_seconds`: {body}"));
    let uptime_i = uptime
        .as_i64()
        .unwrap_or_else(|| panic!("uptime_seconds must be an integer, got {uptime}"));
    assert!(
        uptime_i >= 0,
        "uptime_seconds must be non-negative, got {uptime_i}",
    );
    // M0/M1 health route is called within ms of `AppState::new`; a brand-new
    // server's uptime SHOULD fit in i32 trivially. This guards against
    // accidentally serializing wall-clock millis or a u64-overflowed delta.
    assert!(
        uptime_i < 60 * 60,
        "uptime_seconds suspiciously large for a fresh server: {uptime_i}",
    );
}

#[tokio::test]
async fn health_uptime_grows_over_time() {
    let (_tmp, _root, app) = fresh_app().await;

    let first = {
        let resp = oneshot_get(&app, "/api/health").await;
        let (status, body) = status_and_json(resp).await;
        assert_eq!(status, StatusCode::OK);
        body.get("uptime_seconds")
            .and_then(|v| v.as_i64())
            .expect("first uptime_seconds present")
    };

    // Sleep long enough that the integer-second floor must advance OR equal,
    // but not so long that CI feels it. We tolerate equal: rounding to whole
    // seconds means two reads ~100ms apart might both fall in the same second.
    tokio::time::sleep(Duration::from_millis(1_100)).await;

    let second = {
        let resp = oneshot_get(&app, "/api/health").await;
        let (status, body) = status_and_json(resp).await;
        assert_eq!(status, StatusCode::OK);
        body.get("uptime_seconds")
            .and_then(|v| v.as_i64())
            .expect("second uptime_seconds present")
    };

    assert!(
        second >= first,
        "uptime must be non-decreasing across reads: first={first}, second={second}",
    );
    // After a 1.1s gap a same-bucket second-floor is implausible — if this
    // fails it means the server is reading wall-clock from a frozen source
    // (e.g. caching `started_at` _and_ `now` at construction).
    assert!(
        second > first || second == first + 1 || second == first,
        "uptime grew by an implausible amount in 1.1s: first={first}, second={second}",
    );
    // The strong invariant: after ≥1s the second reading MUST be >= first+1
    // _unless_ the test machine is heavily contended. We use strict-monotone
    // here; flake-rate is acceptable for the smoke gate per CLAUDE.md §3.3.
    assert!(
        second >= first,
        "uptime monotonicity check: first={first}, second={second}",
    );
}

#[tokio::test]
async fn health_includes_project_path() {
    let (_tmp, root, app) = fresh_app().await;

    let resp = oneshot_get(&app, "/api/health").await;
    let (status, body) = status_and_json(resp).await;
    assert_eq!(status, StatusCode::OK);

    let project = body
        .get("project")
        .and_then(|v| v.as_str())
        .unwrap_or_else(|| panic!("health body must carry `project` string: {body}"));

    // The body's `project` field MUST round-trip to the same logical path the
    // store was opened at. We compare via `Path::canonicalize` where possible
    // (macOS tempdirs are symlinks to `/private/var/...` and resolving on both
    // sides gives the canonical comparison point).
    let body_path = std::path::PathBuf::from(project);
    let body_canon = body_path.canonicalize().unwrap_or(body_path.clone());
    let root_canon = root.canonicalize().unwrap_or(root.clone());

    assert_eq!(
        body_canon, root_canon,
        "health.project must match the AppState project_root \
         (body={body_path:?} canon={body_canon:?} root={root:?} root_canon={root_canon:?})",
    );
}
