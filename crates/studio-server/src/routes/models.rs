//! Model discovery routes — non-secret model metadata for login + agent UI.
//!
//! `POST /api/models/preview` lists models using the endpoint/key the user has
//! entered on `/login` without persisting that secret. `GET /api/models/session`
//! lists models from the already-authenticated sealed session and returns only
//! model/provider metadata.

use std::collections::BTreeSet;
use std::time::Duration;

use axum::Json;
use axum::Router;
use axum::extract::State;
use axum::extract::rejection::JsonRejection;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use serde::{Deserialize, Serialize};

use crate::AppState;
use crate::error::RouteError;
use crate::secret::{EndpointSecret, ProviderKind, SecretError};

const MODEL_LIST_TIMEOUT: Duration = Duration::from_secs(20);
const MAX_MODELS: usize = 512;

#[derive(Deserialize)]
pub struct ModelPreviewRequest {
    pub endpoint: String,
    pub api_key: String,
    pub provider_kind: ProviderKind,
}

impl std::fmt::Debug for ModelPreviewRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ModelPreviewRequest")
            .field("endpoint", &self.endpoint)
            .field("api_key", &"[REDACTED]")
            .field("provider_kind", &self.provider_kind)
            .finish()
    }
}

#[derive(Debug, Serialize)]
pub struct ModelListResponse {
    pub provider_kind: ProviderKind,
    pub selected_model: Option<String>,
    pub models: Vec<String>,
}

#[derive(Deserialize)]
struct ProviderModelsEnvelope {
    #[serde(default)]
    data: Vec<ProviderModelEntry>,
}

#[derive(Deserialize)]
struct ProviderModelEntry {
    #[serde(default)]
    id: Option<String>,
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/preview", post(preview_models))
        .route("/session", get(session_models))
}

pub async fn preview_models(
    payload: Result<Json<ModelPreviewRequest>, JsonRejection>,
) -> Result<Response, RouteError> {
    let Json(req) = payload.map_err(|e| RouteError::bad_request(e.body_text(), "invalid_body"))?;
    validate_preview_request(&req)?;
    let models = list_provider_models(&req.endpoint, &req.api_key, req.provider_kind).await?;
    Ok((
        StatusCode::OK,
        Json(ModelListResponse {
            provider_kind: req.provider_kind,
            selected_model: None,
            models,
        }),
    )
        .into_response())
}

pub async fn session_models(State(state): State<AppState>) -> Result<Response, RouteError> {
    let secret = decrypt_session_secret(&state).await?;
    let models =
        list_provider_models(&secret.endpoint, &secret.api_key, secret.provider_kind).await?;
    Ok((
        StatusCode::OK,
        Json(ModelListResponse {
            provider_kind: secret.provider_kind,
            selected_model: Some(secret.model),
            models,
        }),
    )
        .into_response())
}

fn validate_preview_request(req: &ModelPreviewRequest) -> Result<(), RouteError> {
    if req.provider_kind == ProviderKind::Synthetic {
        return Err(RouteError::bad_request(
            "synthetic provider cannot list remote models",
            "invalid_provider_kind",
        ));
    }
    if !matches!(
        req.provider_kind,
        ProviderKind::Anthropic | ProviderKind::Openai
    ) {
        return Err(RouteError::bad_request(
            "provider_kind not supported by this build",
            "unsupported_provider_kind",
        ));
    }
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
    Ok(())
}

async fn decrypt_session_secret(state: &AppState) -> Result<EndpointSecret, RouteError> {
    let key = {
        let guard = state.session_key.read().await;
        guard.clone()
    }
    .ok_or_else(|| RouteError::bad_request("not authenticated", "no_session"))?;

    let blob = state
        .store
        .session()
        .get_endpoint()
        .await?
        .ok_or_else(|| RouteError::not_found("no endpoint configured", "no_endpoint"))?;

    key.open(&blob.ciphertext).map_err(|e| {
        tracing::warn!(error = %e, "models/session: decrypt failed");
        match e {
            SecretError::Open(_) => RouteError::bad_request(
                "session key does not match stored blob",
                "session_decrypt_failed",
            ),
            _ => RouteError::internal(e.to_string()),
        }
    })
}

async fn list_provider_models(
    endpoint: &str,
    api_key: &str,
    provider_kind: ProviderKind,
) -> Result<Vec<String>, RouteError> {
    let client = reqwest::Client::builder()
        .timeout(MODEL_LIST_TIMEOUT)
        .build()
        .map_err(|e| RouteError::internal(e.to_string()))?;
    let base = endpoint.trim().trim_end_matches('/');
    let req = match provider_kind {
        ProviderKind::Anthropic => client
            .get(format!("{base}/v1/models"))
            .header("x-api-key", api_key.trim())
            .header("anthropic-version", "2023-06-01")
            .header("accept", "application/json"),
        ProviderKind::Openai => client
            .get(format!("{base}/models"))
            .bearer_auth(api_key.trim())
            .header("accept", "application/json"),
        ProviderKind::Synthetic => {
            return Err(RouteError::bad_request(
                "synthetic provider cannot list remote models",
                "invalid_provider_kind",
            ));
        }
        _ => {
            return Err(RouteError::bad_request(
                "provider_kind not supported by this build",
                "unsupported_provider_kind",
            ));
        }
    };

    let resp = req
        .send()
        .await
        .map_err(|e| RouteError::service_unavailable(e.to_string(), "model_list_transport"))?;
    let status = resp.status().as_u16();
    let bytes = resp
        .bytes()
        .await
        .map_err(|e| RouteError::service_unavailable(e.to_string(), "model_list_transport"))?;
    if !(200..300).contains(&status) {
        return Err(classify_model_list_status(status));
    }
    parse_model_ids(&bytes)
}

fn parse_model_ids(bytes: &[u8]) -> Result<Vec<String>, RouteError> {
    let parsed: ProviderModelsEnvelope = serde_json::from_slice(bytes).map_err(|e| {
        RouteError::bad_request(
            format!("model list decode failed: {e}"),
            "model_list_decode",
        )
    })?;
    let mut seen = BTreeSet::new();
    let mut ids = Vec::new();
    for entry in parsed.data {
        let Some(id) = entry.id else { continue };
        let id = id.trim();
        if id.is_empty() || !seen.insert(id.to_string()) {
            continue;
        }
        ids.push(id.to_string());
        if ids.len() >= MAX_MODELS {
            break;
        }
    }
    if ids.is_empty() {
        return Err(RouteError::bad_request(
            "provider returned no models",
            "model_list_empty",
        ));
    }
    Ok(ids)
}

fn classify_model_list_status(status: u16) -> RouteError {
    match status {
        401 | 403 => RouteError::bad_request(
            "provider rejected the API key while listing models",
            "model_list_auth",
        ),
        404 => RouteError::bad_request(
            "provider model-list endpoint was not found",
            "model_list_not_found",
        ),
        429 => RouteError::service_unavailable(
            "provider rate-limited model listing",
            "model_list_rate_limited",
        ),
        400..=499 => RouteError::bad_request(
            format!("provider model-list request failed with HTTP {status}"),
            "model_list_bad_request",
        ),
        _ => RouteError::service_unavailable(
            format!("provider model-list request failed with HTTP {status}"),
            "model_list_failed",
        ),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn parse_model_ids_dedupes_and_skips_empty() {
        let body = br#"{"data":[{"id":"a"},{"id":""},{"id":"a"},{"id":" b "}]}"#;
        assert_eq!(parse_model_ids(body).unwrap(), vec!["a", "b"]);
    }

    #[test]
    fn preview_validation_rejects_synthetic() {
        let req = ModelPreviewRequest {
            endpoint: "https://example.invalid".to_string(),
            api_key: "sk".to_string(),
            provider_kind: ProviderKind::Synthetic,
        };
        let err = validate_preview_request(&req).unwrap_err();
        assert!(err.to_string().contains("synthetic"));
    }
}
