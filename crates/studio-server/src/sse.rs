//! Shared SSE fan-out hub for state-change events.
//!
//! `/api/events` exposes a single SSE stream of mixed events (ADR /
//! finding / ledger). Per ADR-0006 §F-07 the per-client buffer is
//! bounded (256) so a slow consumer can't backpressure the watcher
//! thread; lagging clients see a `lagged` event and resume from the
//! current head.
//!
//! Architecture:
//!
//! - One [`tokio::sync::broadcast::Sender<EventEnvelope>`] lives in
//!   [`crate::AppState`].
//! - A boot-time spawned task (in [`crate::serve`]) consumes
//!   `store.adr().watch()` + `store.finding().watch()` streams and
//!   publishes envelopes onto the broadcast.
//! - The `/api/events` handler subscribes a per-request
//!   `broadcast::Receiver`, wraps it in a stream that yields SSE
//!   events, and returns an `Sse<_>` response.
//!
//! Why broadcast (not mpsc): SSE is one-to-many — multiple browser tabs
//! / multiple frontend windows can all be live. broadcast fans out a
//! single `send` to every active subscriber.

use std::sync::Arc;

use serde::Serialize;
use tokio::sync::broadcast;

/// Bounded buffer per subscriber (per ADR-0006 §F-07 hard cap).
///
/// Tokio's broadcast channel uses a ring buffer shared across all
/// receivers, so this is effectively "how far behind the head a
/// subscriber may be before it sees a `lagged` error and skips
/// forward".
pub const SSE_BUFFER_CAP: usize = 256;

/// Wire-shape of one state-change event. Tagged JSON so the M2
/// frontend can `switch (event.kind)` without a separate header.
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EventEnvelope {
    /// ADR file added on disk.
    AdrAdded {
        /// Absolute path (display string).
        path: String,
    },
    /// ADR file modified on disk.
    AdrModified {
        /// Absolute path.
        path: String,
    },
    /// ADR file removed from disk.
    AdrRemoved {
        /// Absolute path.
        path: String,
    },
    /// Finding file added.
    FindingAdded {
        /// Absolute path.
        path: String,
    },
    /// Finding file modified.
    FindingModified {
        /// Absolute path.
        path: String,
    },
    /// Finding file removed.
    FindingRemoved {
        /// Absolute path.
        path: String,
    },
    // Note: per A5 review F-A5-02, no `Heartbeat` variant. SSE
    // liveness comes from `axum::response::sse::KeepAlive` comment
    // frames (line starts with `:`) — not a typed event. If M2+
    // needs a typed heartbeat (some proxies strip SSE comments),
    // re-introduce here AND wire an actual publisher in
    // `build_router` (`tokio::time::interval` spawn).
}

/// Fan-out hub. Cloning the hub yields more `Sender` handles backed by
/// the same channel; dropping the last `Sender` closes the channel.
#[derive(Clone, Debug)]
pub struct EventHub {
    sender: Arc<broadcast::Sender<EventEnvelope>>,
}

impl EventHub {
    /// Construct a new hub with the per-subscriber buffer cap set to
    /// [`SSE_BUFFER_CAP`].
    #[must_use]
    pub fn new() -> Self {
        let (tx, _rx) = broadcast::channel(SSE_BUFFER_CAP);
        Self {
            sender: Arc::new(tx),
        }
    }

    /// Subscribe a fresh receiver. Drop the receiver to unsubscribe.
    #[must_use]
    pub fn subscribe(&self) -> broadcast::Receiver<EventEnvelope> {
        self.sender.subscribe()
    }

    /// Publish an envelope. Failure here means there are no live
    /// receivers — we log at trace level and drop the event because
    /// dropping is the documented behaviour of broadcast::send when
    /// the receiver count is zero.
    pub fn publish(&self, evt: EventEnvelope) {
        // `send` returns Err iff there are no live receivers. That is
        // the normal idle case (no browser open yet), not an error.
        if let Err(e) = self.sender.send(evt) {
            tracing::trace!(error = ?e, "EventHub::publish — no live subscribers; event dropped");
        }
    }

    /// Approximate live-subscriber count. Useful for tracing /
    /// diagnostics; not a strong consistency guarantee (subscribers can
    /// come and go between observation and use).
    #[must_use]
    pub fn subscriber_count(&self) -> usize {
        self.sender.receiver_count()
    }
}

impl Default for EventHub {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn publish_then_subscribe_sees_subsequent_events() {
        let hub = EventHub::new();
        let mut rx = hub.subscribe();
        hub.publish(EventEnvelope::AdrAdded {
            path: "/tmp/adr.md".into(),
        });
        let got = rx.recv().await.unwrap();
        match got {
            EventEnvelope::AdrAdded { path } => assert_eq!(path, "/tmp/adr.md"),
            other => panic!("unexpected variant: {other:?}"),
        }
    }

    #[tokio::test]
    async fn publish_with_no_subscribers_does_not_panic() {
        let hub = EventHub::new();
        // No subscriber yet; publish drops the event silently.
        hub.publish(EventEnvelope::AdrAdded {
            path: "/tmp/probe.md".into(),
        });
        assert_eq!(hub.subscriber_count(), 0);
    }

    #[tokio::test]
    async fn second_subscriber_gets_independent_stream() {
        let hub = EventHub::new();
        let mut rx_a = hub.subscribe();
        let mut rx_b = hub.subscribe();
        hub.publish(EventEnvelope::AdrAdded {
            path: "/tmp/probe.md".into(),
        });
        // Each subscriber gets its own copy.
        assert!(matches!(
            rx_a.recv().await.unwrap(),
            EventEnvelope::AdrAdded { .. }
        ));
        assert!(matches!(
            rx_b.recv().await.unwrap(),
            EventEnvelope::AdrAdded { .. }
        ));
    }
}
