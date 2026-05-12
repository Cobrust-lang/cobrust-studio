//! `POST /api/auth/set-endpoint` — store encrypted credentials.
//!
//! Per ADR-0003 §"Decision" the server is a pass-through for opaque
//! AEAD-encrypted credential triples; it never sees plaintext. The
//! browser (or any future CLI) is responsible for the
//! WebCrypto/argon2id-derived key + AES-GCM ciphertext; this route
//! just persists the triple under the `"endpoint"` slot in
//! `studio_store::session`.
//!
//! Wire shape (request):
//!
//! ```json
//! {
//!   "ciphertext": "<base64-bytes>",
//!   "nonce":      "<base64-bytes>",
//!   "scheme":     "aes-gcm-256/argon2id"
//! }
//! ```
//!
//! Wire shape (response on success): `200 OK` with
//! `{ "status": "stored" }`. Failures use the [`crate::RouteError`]
//! shape with `code = "invalid_body"` (base64 decode failure) /
//! `code = "internal_error"` (SQLite failure).

use axum::Json;
use axum::Router;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::post;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64;
use serde::{Deserialize, Serialize};
use studio_store::EncryptedBlob;

use crate::AppState;
use crate::error::RouteError;

/// Body shape for `POST /api/auth/set-endpoint`.
#[derive(Debug, Deserialize)]
pub struct SetEndpointRequest {
    /// Base64-encoded ciphertext bytes.
    pub ciphertext: String,
    /// Base64-encoded nonce / IV bytes.
    pub nonce: String,
    /// Scheme tag (e.g. `"aes-gcm-256/argon2id"`). Free-form; the
    /// server does not validate the contents — that's the auth layer's
    /// job on the read side.
    pub scheme: String,
}

/// Body shape for `POST /api/auth/set-endpoint` on success.
#[derive(Debug, Serialize)]
pub struct SetEndpointResponse {
    /// Always `"stored"` on the 200 path. Future variants may add
    /// `"rotated"` etc. — keep the field additive.
    pub status: &'static str,
}

/// Build the auth sub-router. Mounted under `/api/auth`.
pub fn router() -> Router<AppState> {
    Router::new().route("/set-endpoint", post(set_endpoint))
}

/// Handler for `POST /api/auth/set-endpoint`.
pub async fn set_endpoint(
    State(state): State<AppState>,
    Json(req): Json<SetEndpointRequest>,
) -> Result<Response, RouteError> {
    if req.scheme.trim().is_empty() {
        return Err(RouteError::bad_request(
            "scheme must be non-empty",
            "invalid_body",
        ));
    }
    let ciphertext = BASE64
        .decode(req.ciphertext.as_bytes())
        .map_err(|e| RouteError::bad_request(format!("ciphertext base64: {e}"), "invalid_body"))?;
    let nonce = BASE64
        .decode(req.nonce.as_bytes())
        .map_err(|e| RouteError::bad_request(format!("nonce base64: {e}"), "invalid_body"))?;
    if ciphertext.is_empty() {
        return Err(RouteError::bad_request(
            "ciphertext must be non-empty",
            "invalid_body",
        ));
    }
    let blob = EncryptedBlob {
        ciphertext,
        nonce,
        scheme: req.scheme,
    };
    state.store().session().set_endpoint(blob).await?;
    Ok((
        StatusCode::OK,
        Json(SetEndpointResponse { status: "stored" }),
    )
        .into_response())
}
