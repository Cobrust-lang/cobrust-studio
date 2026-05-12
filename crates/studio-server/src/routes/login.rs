//! `POST /api/login` + `POST /api/logout` + `GET /api/session/status`
//! + `GET /api/session/endpoint` (debug-only) — M6 AEAD round-trip.
//!
//! Per ADR-0007 §"API surface change":
//!
//! | Method | Path | Behaviour |
//! |--------|------|-----------|
//! | POST | `/api/login` | Derives Argon2id key + seals `EndpointSecret`; writes `session_kv`; stashes `SessionKey` in `AppState`. |
//! | POST | `/api/logout` | Drops the in-memory `SessionKey`. Next dispatch returns 401. |
//! | GET | `/api/session/status` | Returns `{ "authenticated": bool }` for frontend redirect logic. |
//! | GET | `/api/session/endpoint` | (debug-only, `--debug-session` flag) decrypted endpoint + model; never api_key. |
//!
//! ## Security notes
//!
//! - The passphrase is received in the JSON POST body and held only in a
//!   stack-local `String` during the Argon2id derivation; it is not persisted
//!   anywhere in Studio.
//! - `api_key` is logged nowhere; only the `endpoint` and `model` fields appear
//!   in logs when tracing is enabled.
//! - `GET /api/session/endpoint` is gated behind `AppState.debug_session` (set
//!   by `--debug-session` CLI flag). It returns only endpoint + model —
//!   **never** the api_key — to allow E2E test introspection.

use axum::Json;
use axum::Router;
use axum::extract::State;
use axum::extract::rejection::JsonRejection;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use rand_core::{OsRng, RngCore};
use serde::{Deserialize, Serialize};
use studio_store::session::EncryptedBlob;

use crate::AppState;
use crate::error::RouteError;
use crate::secret::{EndpointSecret, ProviderKind, SCHEME, SecretError, SessionKey};

/// Request body for `POST /api/login`.
///
/// ## M7 addition (ADR-0008)
///
/// `provider_kind` is `#[serde(default)]` so v0.2.x callers that omit the
/// field continue to work with implicit `Anthropic`. `Synthetic` is rejected
/// with 400 `invalid_provider_kind` — it is a CLI/dev-only construct with no
/// real endpoint + key pair.
#[derive(Deserialize)]
pub struct LoginRequest {
    /// LLM provider base URL (e.g. `"https://api.anthropic.com"`).
    pub endpoint: String,
    /// API key — handled server-side; never logged.
    pub api_key: String,
    /// Model identifier (e.g. `"claude-opus-4-7"`).
    pub model: String,
    /// Passphrase used to derive the AES-256 key via Argon2id. Not persisted.
    pub passphrase: String,
    /// Provider API kind — defaults to `Anthropic` for v0.2.x back-compat.
    #[serde(default)]
    pub provider_kind: ProviderKind,
}

impl std::fmt::Debug for LoginRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Aleksandr v3 P1 — `api_key` and `passphrase` carry plaintext
        // secrets directly from the wire. A derived `Debug` would silently
        // spray them into any `tracing::instrument` / panic-format output.
        f.debug_struct("LoginRequest")
            .field("endpoint", &self.endpoint)
            .field("api_key", &"[REDACTED]")
            .field("model", &self.model)
            .field("passphrase", &"[REDACTED]")
            .field("provider_kind", &self.provider_kind)
            .finish()
    }
}

/// Response body for `POST /api/login` on success.
#[derive(Debug, Serialize)]
pub struct LoginResponse {
    /// Always `"ok"` on the 200 path.
    pub status: &'static str,
}

/// Response body for `GET /api/session/status`.
#[derive(Debug, Serialize)]
pub struct SessionStatusResponse {
    /// `true` when a `SessionKey` is held in-memory (user has logged in and
    /// the process has not restarted since).
    pub authenticated: bool,
}

/// Response body for `GET /api/session/endpoint` (debug-only).
#[derive(Debug, Serialize)]
pub struct SessionEndpointResponse {
    /// Decrypted endpoint URL.
    pub endpoint: String,
    /// Decrypted model string.
    pub model: String,
    // `api_key` is intentionally absent — even debug mode does not expose it.
}

/// Build the login + session sub-router. Mounted under `/api`.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/login", post(login))
        .route("/logout", post(logout))
        .route("/session/status", get(session_status))
        .route("/session/endpoint", get(session_endpoint))
}

/// `POST /api/login` handler.
///
/// 1. Validates the JSON body.
/// 2. Generates a random 16-byte salt.
/// 3. Derives a [`SessionKey`] via Argon2id (this is intentionally slow — ~500 ms).
/// 4. Seals `(endpoint, api_key, model)` into the packed wire blob.
/// 5. Persists the blob to `session_kv` under slot `"endpoint"`.
/// 6. Stores the derived key in `AppState.session_key`.
/// 7. Returns 200 `{ "status": "ok" }`.
///
/// On passphrase mismatch (wrong passphrase against an existing blob), returns
/// 401 `{ code: "wrong_passphrase" }` by attempting to verify the newly sealed
/// blob can be opened with the derived key — if the existing blob cannot be
/// opened with the new key that's a credential conflict.
///
/// Actually: per ADR-0007 §"Done means" item 2 sub-bullet 3, `POST /api/login`
/// with a **mismatched passphrase against an existing blob** must return 401.
/// We verify this by attempting to open the *existing* blob with the newly
/// derived key before overwriting.
pub async fn login(
    State(state): State<AppState>,
    payload: Result<Json<LoginRequest>, JsonRejection>,
) -> Result<Response, RouteError> {
    let Json(req) = payload.map_err(|e| RouteError::bad_request(e.body_text(), "invalid_body"))?;

    // M7 (ADR-0008): reject Synthetic — it is a CLI/dev-only construct
    // with no real-world endpoint + key pair.
    if req.provider_kind == ProviderKind::Synthetic {
        return Err(RouteError::bad_request(
            "synthetic provider not valid for /api/login; use --dev-api-key for synthetic dispatch",
            "invalid_provider_kind",
        ));
    }

    // Validate required fields.
    if req.endpoint.trim().is_empty() {
        return Err(RouteError::bad_request(
            "endpoint must be non-empty",
            "invalid_body",
        ));
    }
    if req.api_key.trim().is_empty() {
        return Err(RouteError::bad_request(
            "api_key must be non-empty",
            "invalid_body",
        ));
    }
    if req.model.trim().is_empty() {
        return Err(RouteError::bad_request(
            "model must be non-empty",
            "invalid_body",
        ));
    }
    if req.passphrase.is_empty() {
        return Err(RouteError::bad_request(
            "passphrase must be non-empty",
            "invalid_body",
        ));
    }
    if req.passphrase.len() < 8 {
        return Err(RouteError::bad_request(
            "passphrase must be at least 8 characters",
            "passphrase_too_short",
        ));
    }

    // Check for an existing blob — if present, the new passphrase must
    // successfully open it (proves the user knows the original passphrase
    // that sealed the existing blob).
    let existing_blob = state.store.session().get_endpoint().await?;

    // Compute the SessionKey. Two paths:
    //   (a) Existing blob with the current scheme → re-derive from the
    //       salt embedded in that blob (`blob[..16]`), verify by attempting
    //       open(). If open fails → 400 wrong_passphrase. If it succeeds →
    //       REUSE that key for the upcoming seal (avoids a second ~500ms
    //       Argon2id derivation and keeps the salt stable across re-logins
    //       with the same passphrase).
    //   (b) No existing blob, or existing blob has a pre-M6 scheme → fresh
    //       login. Generate a new salt and derive from scratch.
    //
    // Sarah v3 audit #4: salt was previously generated speculatively before
    // the wrong-passphrase guard ran, then discarded if the guard rejected.
    // This refactor defers salt + derive to after the guard, so the unused-
    // OsRng-output anti-pattern is gone and the happy path is a single derive.
    let key = if let Some(ref blob) = existing_blob
        && blob.scheme == SCHEME
        && blob.ciphertext.len() >= 16
    {
        let mut existing_salt = [0u8; 16];
        existing_salt.copy_from_slice(&blob.ciphertext[..16]);
        let existing_key = SessionKey::derive(&req.passphrase, &existing_salt).map_err(|e| {
            tracing::error!(error = %e, "argon2id derivation failed (existing-blob path)");
            RouteError::internal(format!("key derivation failed: {e}"))
        })?;
        if let Err(SecretError::Open(_)) = existing_key.open(&blob.ciphertext) {
            tracing::warn!(
                endpoint = %req.endpoint,
                "login rejected: passphrase does not match existing session_kv blob",
            );
            return Err(RouteError::BadRequest {
                message: "passphrase does not match existing credential blob".to_string(),
                code: "wrong_passphrase",
            });
        }
        existing_key
    } else {
        // Fresh login (no blob OR pre-M6 raw stub per ADR-0007 §Migration —
        // first M6 login overwrites a pre-M6 raw stub without passphrase
        // check, since the raw stub has no salt at blob[..16] to verify with).
        let mut salt = [0u8; 16];
        OsRng.fill_bytes(&mut salt);
        SessionKey::derive(&req.passphrase, &salt).map_err(|e| {
            tracing::error!(error = %e, "argon2id derivation failed (fresh-login path)");
            RouteError::internal(format!("key derivation failed: {e}"))
        })?
    };

    // Seal the EndpointSecret with the new key.
    let secret = EndpointSecret {
        endpoint: req.endpoint.trim().to_string(),
        api_key: req.api_key.trim().to_string(),
        model: req.model.trim().to_string(),
        provider_kind: req.provider_kind,
    };
    let ciphertext = key.seal(&secret).map_err(|e| {
        tracing::error!(error = %e, "aead seal failed at login");
        RouteError::internal(format!("seal failed: {e}"))
    })?;

    let encrypted_blob = EncryptedBlob {
        ciphertext,
        nonce: Vec::new(), // per ADR-0007: nonce packed into ciphertext
        scheme: SCHEME.to_string(),
    };

    // Persist to session_kv.
    state.store.session().set_endpoint(encrypted_blob).await?;

    // Store the derived key in AppState.
    {
        let mut guard = state.session_key.write().await;
        *guard = Some(key);
    }

    tracing::info!(
        endpoint = %secret.endpoint,
        model = %secret.model,
        provider_kind = ?secret.provider_kind,
        "login: session key derived and stored",
    );

    Ok((StatusCode::OK, Json(LoginResponse { status: "ok" })).into_response())
}

/// `POST /api/logout` handler.
///
/// Drops the in-memory [`SessionKey`]. The `session_kv` blob is preserved on
/// disk — only the passphrase is needed to re-derive on next login.
///
/// Returns 200 always (idempotent — logging out when already unauthenticated
/// is not an error).
#[allow(clippy::unused_async)]
pub async fn logout(State(state): State<AppState>) -> Response {
    let mut guard = state.session_key.write().await;
    let was_authenticated = guard.is_some();
    *guard = None;
    drop(guard);

    if was_authenticated {
        tracing::info!("logout: session key dropped");
    } else {
        tracing::debug!("logout: no active session (no-op)");
    }

    (StatusCode::OK, Json(serde_json::json!({ "status": "ok" }))).into_response()
}

/// `GET /api/session/status` handler.
///
/// Returns `{ "authenticated": true }` when a `SessionKey` is held in-memory,
/// `{ "authenticated": false }` otherwise. Used by the frontend to decide
/// whether to redirect to `/login`.
#[allow(clippy::unused_async)]
pub async fn session_status(State(state): State<AppState>) -> Response {
    let guard = state.session_key.read().await;
    let authenticated = guard.is_some();
    drop(guard);

    (
        StatusCode::OK,
        Json(SessionStatusResponse { authenticated }),
    )
        .into_response()
}

/// `GET /api/session/endpoint` handler.
///
/// Debug-only (gated behind `AppState.debug_session`). Returns the decrypted
/// `endpoint` + `model` for E2E test introspection — **never** the `api_key`.
///
/// Returns 403 if `debug_session` is `false`.
/// Returns 401 if no `SessionKey` is in-memory.
/// Returns 404 if `session_kv` has no blob.
#[allow(clippy::unused_async)]
pub async fn session_endpoint(State(state): State<AppState>) -> Result<Response, RouteError> {
    // Debug gate.
    if !state.debug_session {
        return Err(RouteError::BadRequest {
            message: "session/endpoint is only available with --debug-session".to_string(),
            code: "debug_only",
        });
    }

    let key = {
        let guard = state.session_key.read().await;
        guard.clone()
    };
    let key = key.ok_or_else(|| RouteError::BadRequest {
        message: "not authenticated".to_string(),
        code: "no_session",
    })?;

    let blob = state
        .store
        .session()
        .get_endpoint()
        .await?
        .ok_or_else(|| RouteError::not_found("no endpoint configured", "no_endpoint"))?;

    let secret = key.open(&blob.ciphertext).map_err(|e| {
        tracing::warn!(error = %e, "session/endpoint: open failed");
        RouteError::BadRequest {
            message: format!("decrypt failed: {e}"),
            code: "decrypt_failed",
        }
    })?;

    Ok((
        StatusCode::OK,
        Json(SessionEndpointResponse {
            endpoint: secret.endpoint,
            model: secret.model,
        }),
    )
        .into_response())
}
