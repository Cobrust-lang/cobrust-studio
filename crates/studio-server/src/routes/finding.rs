//! Finding routes — `GET /api/finding`, `POST /api/finding`.
//!
//! Per the Wave A4 surface in `docs/agent/modules/studio-server.md`:
//!
//! - `GET /api/finding` returns `{ findings: [FindingSummary, ...] }`
//!   (ordering: by `finding_id` ascending — store layer's contract).
//! - `POST /api/finding` accepts [`FindingDraftBody`] JSON and returns
//!   `201` with the freshly written [`studio_store::Finding`].
//!
//! M1 does not need `GET /api/finding/:id` — the SvelteKit UI consumes
//! the full body via the file walk on the list view. The list-then-walk
//! pattern keeps the surface small for A4; add the singleton route in
//! M2 if the UI grows a detail page.

use axum::extract::State;
use axum::extract::rejection::JsonRejection;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use studio_store::{Finding, FindingDraft, FindingSummary};

use crate::AppState;
use crate::error::RouteError;

/// Body shape for `GET /api/finding`.
#[derive(Debug, Serialize)]
pub struct FindingListResponse {
    /// All finding summaries ordered by `finding_id` ascending.
    pub findings: Vec<FindingSummary>,
}

/// Body shape for `POST /api/finding` requests. Mirrors
/// [`studio_store::finding::FindingDraft`] field-for-field; kept as a
/// separate type so future wire changes (e.g. defaulted `severity`) do
/// not bleed into the store crate's public API.
#[derive(Debug, Deserialize)]
pub struct FindingDraftBody {
    /// Slug id; must be unique. Used as filename stem.
    pub finding_id: String,
    /// One-line title.
    pub title: String,
    /// `last_verified_commit` field; defaults to `"HEAD"` when omitted
    /// (the F20 doc-coverage gate will then refuse to merge until the
    /// caller stamps a real SHA — by design).
    #[serde(default)]
    pub last_verified_commit: Option<String>,
    /// Severity tag (`P1`/`P2`/`P3`/`P4`); defaults to `"P3"`.
    #[serde(default)]
    pub severity: Option<String>,
    /// Lifecycle status; defaults to `"open"`.
    #[serde(default)]
    pub status: Option<String>,
    /// `dependencies:` list (e.g. `["adr:0006"]`).
    #[serde(default)]
    pub dependencies: Vec<String>,
    /// `related:` list.
    #[serde(default)]
    pub related: Vec<String>,
    /// Markdown body.
    #[serde(default)]
    pub body: String,
}

impl FindingDraftBody {
    fn into_draft(self) -> FindingDraft {
        FindingDraft {
            finding_id: self.finding_id,
            title: self.title,
            last_verified_commit: self
                .last_verified_commit
                .filter(|s| !s.trim().is_empty())
                .unwrap_or_else(|| "HEAD".to_string()),
            severity: self
                .severity
                .filter(|s| !s.trim().is_empty())
                .unwrap_or_else(|| "P3".to_string()),
            status: self
                .status
                .filter(|s| !s.trim().is_empty())
                .unwrap_or_else(|| "open".to_string()),
            dependencies: self.dependencies,
            related: self.related,
            body: self.body,
        }
    }
}

/// Build the finding sub-router. Mounted under `/api/finding`.
pub fn router() -> Router<AppState> {
    Router::new().route("/", get(list_findings).post(create_finding))
}

/// Handler for `GET /api/finding`.
pub async fn list_findings(State(state): State<AppState>) -> Result<Response, RouteError> {
    let findings = state.store().finding().list().await?;
    Ok(Json(FindingListResponse { findings }).into_response())
}

/// Handler for `POST /api/finding`. Returns `201 Created`.
///
/// Body extracted via `Result<Json<_>, JsonRejection>` so malformed bodies
/// surface as `RouteError::BadRequest { code: "invalid_body" }` JSON,
/// matching the M1 contract (per A5 reconcile, parallel to
/// [`super::adr::create_adr`]).
pub async fn create_finding(
    State(state): State<AppState>,
    payload: Result<Json<FindingDraftBody>, JsonRejection>,
) -> Result<Response, RouteError> {
    let Json(body) = payload.map_err(|e| {
        // Every JSON-extractor rejection (missing field, wrong
        // content-type, garbage JSON) collapses onto a single
        // `invalid_body` code — see [`super::adr::create_adr`].
        let _ = e.status();
        RouteError::bad_request(e.body_text(), "invalid_body")
    })?;
    if body.finding_id.trim().is_empty() {
        return Err(RouteError::bad_request(
            "finding_id must be non-empty",
            "invalid_body",
        ));
    }
    if body.title.trim().is_empty() {
        return Err(RouteError::bad_request(
            "title must be non-empty",
            "invalid_body",
        ));
    }
    let draft = body.into_draft();
    let finding = state.store().finding().create(draft).await?;
    Ok((StatusCode::CREATED, Json::<Finding>(finding)).into_response())
}
