//! `GET /api/events` — SSE channel for state-change events.
//!
//! Long-running response: the handler subscribes to the broadcast
//! [`crate::sse::EventHub`], wraps the receiver in a [`Stream`] that
//! converts each [`crate::sse::EventEnvelope`] into a JSON-bodied SSE
//! event, and returns an axum [`Sse`] response.
//!
//! Keepalive: 15 s comment-frame interval. Browsers' EventSource closes
//! after ~30 s idle without bytes, so 15 s gives a 2x safety margin.
//!
//! Reconnection (per task brief): `Last-Event-ID` is **not** required
//! for M1 — the client just re-establishes when the stream drops and
//! sees subsequent events. Backfill of missed events is a separate
//! concern handled at the consumer (re-list via `/api/adr`,
//! `/api/finding`, etc).

use std::convert::Infallible;
use std::time::Duration;

use axum::Router;
use axum::extract::State;
use axum::response::Sse;
use axum::response::sse::{Event, KeepAlive};
use axum::routing::get;
use futures::stream::{Stream, StreamExt};
use tokio_stream::wrappers::BroadcastStream;

use crate::AppState;
use crate::sse::EventEnvelope;

/// Build the events sub-router. Mounted under `/api/events`.
pub fn router() -> Router<AppState> {
    Router::new().route("/", get(events))
}

/// Handler for `GET /api/events`.
pub async fn events(
    State(state): State<AppState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let rx = state.events().subscribe();
    let stream = BroadcastStream::new(rx).filter_map(|item| async move {
        match item {
            Ok(envelope) => Some(Ok(envelope_to_event(&envelope))),
            Err(e) => {
                tracing::warn!(error = ?e, "sse client lagged past buffer cap; skipping forward");
                None
            }
        }
    });
    Sse::new(stream).keep_alive(
        KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("keep-alive"),
    )
}

/// Render one envelope into an SSE `Event`. JSON-encoding failures
/// fall back to a plain-text comment that the EventSource will ignore
/// — this branch is only reachable if serde fails on our own
/// internal enum, which is essentially impossible.
fn envelope_to_event(envelope: &EventEnvelope) -> Event {
    match serde_json::to_string(envelope) {
        Ok(json) => Event::default().event(event_kind_tag(envelope)).data(json),
        Err(e) => {
            tracing::error!(error = %e, "failed to encode EventEnvelope; emitting empty event");
            Event::default().event("error").data("{}")
        }
    }
}

fn event_kind_tag(envelope: &EventEnvelope) -> &'static str {
    match envelope {
        EventEnvelope::AdrAdded { .. } => "adr_added",
        EventEnvelope::AdrModified { .. } => "adr_modified",
        EventEnvelope::AdrRemoved { .. } => "adr_removed",
        EventEnvelope::FindingAdded { .. } => "finding_added",
        EventEnvelope::FindingModified { .. } => "finding_modified",
        EventEnvelope::FindingRemoved { .. } => "finding_removed",
    }
}
