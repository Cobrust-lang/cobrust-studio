//! ADR routes — `GET /api/adr`, `GET /api/adr/:id`, `POST /api/adr`.
//!
//! Per the Wave A4 surface in `docs/agent/modules/studio-server.md`:
//!
//! - `GET /api/adr` returns `{ adrs: [AdrSummary, ...] }`, ordered by
//!   `adr_id` ascending (the order [`studio_store::adr::AdrHandle::list`]
//!   guarantees).
//! - `GET /api/adr/:id` returns the full [`studio_store::Adr`] JSON or a
//!   `404 adr_not_found`.
//! - `POST /api/adr` accepts an [`AdrDraftBody`] JSON, hands it to
//!   [`studio_store::adr::AdrHandle::create`], and returns `201` with the
//!   freshly written [`studio_store::Adr`].
//!
//! Validation errors (`title.trim().is_empty()` etc.) surface as `400
//! invalid_input`; conflicts (duplicate file on disk) as `409
//! already_exists`; SQL/IO failures as `500 internal_error`. See
//! [`crate::RouteError`] for the full mapping.

use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use studio_store::{Adr, AdrDraft, AdrSummary};

use crate::AppState;
use crate::error::RouteError;

/// Body shape for `GET /api/adr`.
#[derive(Debug, Serialize)]
pub struct AdrListResponse {
    /// All ADR summaries, ordered by `adr_id` ascending.
    pub adrs: Vec<AdrSummary>,
}

/// Body shape for `POST /api/adr` requests.
///
/// Mirrors [`studio_store::adr::AdrDraft`] but stays a server-local type
/// so future wire-shape evolution (e.g. defaulting `status` to
/// `"proposed"`) does not leak into the store crate's public surface.
#[derive(Debug, Deserialize)]
pub struct AdrDraftBody {
    /// One-line title; used in the filename slug.
    pub title: String,
    /// Lifecycle status; defaults to `"proposed"` when omitted.
    #[serde(default)]
    pub status: Option<String>,
    /// ISO date (`YYYY-MM-DD`); defaults to today (UTC) when omitted.
    #[serde(default)]
    pub date: Option<String>,
    /// Markdown body.
    #[serde(default)]
    pub body: String,
    /// IDs this ADR supersedes; defaults to empty.
    #[serde(default)]
    pub supersedes: Vec<u32>,
}

impl AdrDraftBody {
    fn into_draft(self) -> AdrDraft {
        AdrDraft {
            title: self.title,
            status: self
                .status
                .filter(|s| !s.trim().is_empty())
                .unwrap_or_else(|| "proposed".to_string()),
            date: self
                .date
                .filter(|s| !s.trim().is_empty())
                .unwrap_or_else(today_utc_string),
            body: self.body,
            supersedes: self.supersedes,
        }
    }
}

fn today_utc_string() -> String {
    let now = time::OffsetDateTime::now_utc();
    format!(
        "{:04}-{:02}-{:02}",
        now.year(),
        u8::from(now.month()),
        now.day(),
    )
}

/// Build the ADR sub-router. Mounted under `/api/adr` by [`crate::app`].
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/", get(list_adrs).post(create_adr))
        .route("/:id", get(get_adr))
}

/// Handler for `GET /api/adr`.
pub async fn list_adrs(State(state): State<AppState>) -> Result<Response, RouteError> {
    let adrs = state.store().adr().list().await?;
    Ok(Json(AdrListResponse { adrs }).into_response())
}

/// Handler for `GET /api/adr/:id`.
pub async fn get_adr(
    State(state): State<AppState>,
    Path(id): Path<u32>,
) -> Result<Response, RouteError> {
    match state.store().adr().get(id).await? {
        Some(adr) => Ok(Json::<Adr>(adr).into_response()),
        None => Err(RouteError::not_found(
            format!("adr {id:04} not found"),
            "adr_not_found",
        )),
    }
}

/// Handler for `POST /api/adr`. Returns `201 Created`.
pub async fn create_adr(
    State(state): State<AppState>,
    Json(body): Json<AdrDraftBody>,
) -> Result<Response, RouteError> {
    if body.title.trim().is_empty() {
        return Err(RouteError::bad_request(
            "title must be non-empty",
            "invalid_input",
        ));
    }
    let draft = body.into_draft();
    let adr = state.store().adr().create(draft).await?;
    Ok((StatusCode::CREATED, Json::<Adr>(adr)).into_response())
}
