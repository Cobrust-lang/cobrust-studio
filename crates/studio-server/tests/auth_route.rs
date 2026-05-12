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

//! `POST /api/auth/set-endpoint` integration contract — Wave A4 P7-TEST (red).
//!
//! Locks the wire-shape binding for the single auth route named in
//! `docs/agent/modules/studio-server.md` §"Wave A4 target":
//!
//! - `POST /api/auth/set-endpoint` body: `{ ciphertext, nonce, scheme }`
//!   (each as base64 strings); store as opaque `EncryptedBlob` per ADR-0003.
//!
//! API-shape assumptions made (DEV must satisfy; CTO reconciles drift):
//!
//! 1. **Request body.** JSON with `ciphertext` and `nonce` as base64 strings
//!    (standard alphabet) plus `scheme` (free-form string).
//! 2. **Success status.** 200 OK or 204 No Content. The body, if any, is
//!    JSON-shaped.
//! 3. **Storage side effect.** The route writes through
//!    `store.session().set_endpoint(EncryptedBlob)` so the studio-store
//!    handle round-trips the blob (verified directly via the store API
//!    from the test, since no `GET /api/auth/endpoint` exists in M1).
//! 4. **Malformed body → 4xx.** Missing fields, non-base64 ciphertext,
//!    or non-JSON body all return a 4xx response with a JSON error envelope.

mod common;

use axum::http::StatusCode;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;
use common::{boot_app_with_store, json_body, oneshot_post_bytes, oneshot_post_json};
use serde_json::json;

const SAMPLE_CIPHERTEXT: &[u8] = &[0xde, 0xad, 0xbe, 0xef, 0x00, 0x11, 0x22, 0x33];
const SAMPLE_NONCE: &[u8] = &[
    0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c,
];
const SAMPLE_SCHEME: &str = "aes-gcm-256/argon2id";

#[tokio::test]
async fn set_endpoint_with_valid_body_returns_2xx() {
    let (_tmp, _root, _store, app) = boot_app_with_store().await;

    let body = json!({
        "ciphertext": B64.encode(SAMPLE_CIPHERTEXT),
        "nonce": B64.encode(SAMPLE_NONCE),
        "scheme": SAMPLE_SCHEME,
    });
    let resp = oneshot_post_json(&app, "/api/auth/set-endpoint", &body).await;
    let status = resp.status();
    assert!(
        status == StatusCode::OK || status == StatusCode::NO_CONTENT,
        "POST /api/auth/set-endpoint with a valid body must be 200 or 204, got {status}",
    );
}

#[tokio::test]
async fn set_endpoint_persists_blob_to_store() {
    // Round-trip via the store-side handle: the route MUST go through
    // `studio_store::SessionHandle::set_endpoint`, otherwise the blob is
    // unrecoverable by the rest of the stack.
    let (_tmp, _root, store, app) = boot_app_with_store().await;

    assert!(
        store
            .session()
            .get_endpoint()
            .await
            .expect("get OK")
            .is_none(),
        "fresh store must have no endpoint blob set",
    );

    let body = json!({
        "ciphertext": B64.encode(SAMPLE_CIPHERTEXT),
        "nonce": B64.encode(SAMPLE_NONCE),
        "scheme": SAMPLE_SCHEME,
    });
    let resp = oneshot_post_json(&app, "/api/auth/set-endpoint", &body).await;
    assert!(
        resp.status().is_success(),
        "POST must succeed before we can assert persistence; got {}",
        resp.status(),
    );

    let blob = store
        .session()
        .get_endpoint()
        .await
        .expect("get_endpoint OK")
        .unwrap_or_else(|| panic!("endpoint blob must be persisted after POST"));
    assert_eq!(
        blob.ciphertext, SAMPLE_CIPHERTEXT,
        "ciphertext bytes must roundtrip verbatim through base64",
    );
    assert_eq!(
        blob.nonce, SAMPLE_NONCE,
        "nonce bytes must roundtrip verbatim through base64",
    );
    assert_eq!(blob.scheme, SAMPLE_SCHEME);
}

#[tokio::test]
async fn set_endpoint_overwrites_prior_blob() {
    let (_tmp, _root, store, app) = boot_app_with_store().await;

    let first = json!({
        "ciphertext": B64.encode(b"first-cipher"),
        "nonce": B64.encode(SAMPLE_NONCE),
        "scheme": "v1",
    });
    assert!(
        oneshot_post_json(&app, "/api/auth/set-endpoint", &first)
            .await
            .status()
            .is_success(),
    );
    let second = json!({
        "ciphertext": B64.encode(b"second-cipher"),
        "nonce": B64.encode(SAMPLE_NONCE),
        "scheme": "v2",
    });
    assert!(
        oneshot_post_json(&app, "/api/auth/set-endpoint", &second)
            .await
            .status()
            .is_success(),
    );

    let blob = store
        .session()
        .get_endpoint()
        .await
        .expect("get OK")
        .expect("blob present");
    assert_eq!(blob.ciphertext, b"second-cipher".to_vec());
    assert_eq!(blob.scheme, "v2");
}

#[tokio::test]
async fn set_endpoint_malformed_returns_4xx() {
    let (_tmp, _root, _store, app) = boot_app_with_store().await;

    // Empty object — missing every required field.
    let resp = oneshot_post_json(&app, "/api/auth/set-endpoint", &json!({})).await;
    let status = resp.status();
    let body = json_body(resp).await;
    assert!(
        status.is_client_error(),
        "POST /api/auth/set-endpoint with empty body must be 4xx, got {status}: {body}",
    );
    assert!(body.is_object(), "4xx body must be a JSON object: {body}",);
}

#[tokio::test]
async fn set_endpoint_non_base64_ciphertext_returns_4xx() {
    let (_tmp, _root, _store, app) = boot_app_with_store().await;

    // `scheme` is fine but `ciphertext` contains non-base64 chars (`*`).
    let body = json!({
        "ciphertext": "not*base64!",
        "nonce": B64.encode(SAMPLE_NONCE),
        "scheme": SAMPLE_SCHEME,
    });
    let resp = oneshot_post_json(&app, "/api/auth/set-endpoint", &body).await;
    let status = resp.status();
    assert!(
        status.is_client_error(),
        "non-base64 ciphertext must be rejected with 4xx, got {status}",
    );
}

#[tokio::test]
async fn set_endpoint_non_json_body_returns_4xx() {
    let (_tmp, _root, _store, app) = boot_app_with_store().await;

    let resp = oneshot_post_bytes(
        &app,
        "/api/auth/set-endpoint",
        "text/plain",
        b"definitely not json".to_vec(),
    )
    .await;
    assert!(
        resp.status().is_client_error(),
        "non-JSON body must be 4xx: got {}",
        resp.status(),
    );
}
