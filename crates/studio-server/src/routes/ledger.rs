//! `GET /api/ledger/recent` — recent dispatch ledger entries.
//!
//! Per ADR-0006 §"Addendum 2026-05-11" F-02 the SQLite `ledger_view` is
//! the **materialised view** of the canonical JSONL written by
//! `studio_router::Ledger`. This route reads from the view via
//! [`studio_store::ledger::LedgerHandle::recent`] — fast, indexed by
//! `ts DESC`, no JSONL parse on the hot path.
//!
//! Query parameters:
//!
//! - `n` (optional, default `20`, max `1000`) — number of entries to
//!   return.
//!
//! Response shape:
//!
//! ```json
//! { "entries": [LedgerEntry, ...] }   // newest first
//! ```

use axum::Json;
use axum::Router;
use axum::extract::{Query, State};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use serde::{Deserialize, Serialize};
use studio_store::LedgerEntry;

use crate::AppState;
use crate::error::RouteError;

/// Default ledger fetch size when `?n=` is omitted.
pub const LEDGER_DEFAULT_N: usize = 20;

/// Hard cap on `?n=` — the materialised view is cheap to read but the
/// JSON serialisation cost on huge fetches is wasteful for a UI
/// "recent activity" widget.
pub const LEDGER_MAX_N: usize = 1000;

/// Query string for `GET /api/ledger/recent`.
#[derive(Debug, Deserialize)]
pub struct RecentQuery {
    /// Number of entries to return; clamped to [1, LEDGER_MAX_N].
    pub n: Option<usize>,
}

/// Body shape for `GET /api/ledger/recent`.
#[derive(Debug, Serialize)]
pub struct LedgerRecentResponse {
    /// Newest-first entries.
    pub entries: Vec<LedgerEntry>,
}

/// Build the ledger sub-router. Mounted under `/api/ledger`.
pub fn router() -> Router<AppState> {
    Router::new().route("/recent", get(recent))
}

/// Handler for `GET /api/ledger/recent`.
///
/// `?n=` semantics (per A5 reconcile, mirroring the M1 contract in
/// `tests/ledger_route.rs`):
/// - Omitted → [`LEDGER_DEFAULT_N`] (20).
/// - `?n=0`  → empty list (degenerate but well-defined; SQLite `LIMIT 0`
///   short-circuits and the store contract in
///   `studio_store::ledger::LedgerHandle::recent` already returns `[]`).
/// - `?n=N`  → clamped to `min(N, LEDGER_MAX_N)` (cap stays in force).
pub async fn recent(
    State(state): State<AppState>,
    Query(q): Query<RecentQuery>,
) -> Result<Response, RouteError> {
    let n = match q.n {
        Some(0) => 0,
        Some(n) => n.min(LEDGER_MAX_N),
        None => LEDGER_DEFAULT_N,
    };
    let entries = state.store().ledger().recent(n).await?;
    Ok(Json(LedgerRecentResponse { entries }).into_response())
}
