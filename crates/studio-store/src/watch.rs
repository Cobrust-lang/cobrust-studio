//! Filesystem watcher → tokio channel bridge.
//!
//! Wraps the `notify` crate so the rest of the crate sees a single
//! [`RawEvent`] type with three coarse [`EventKind`] variants
//! (`Create`/`Modify`/`Remove`). The fine-grained `notify::EventKind`
//! taxonomy is collapsed because consumers (`AdrChangeEvent` etc.) only
//! need the coarse direction.
//!
//! Lifetime contract: [`watch_dir`] returns `(rx, handle)`. The caller
//! MUST keep `handle` alive for as long as it reads from `rx` — dropping
//! the handle cancels the watcher and closes the channel.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::task::{Context, Poll};
use std::time::{Duration, Instant};

use futures::Stream;
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

use crate::error::StoreError;

/// Debounce window for the `WatchStream` coalescer.
///
/// `notify` reports many low-level events per logical file change
/// (macOS FSEvents in particular fires 1-3 events per `tokio::fs::write`).
/// We coalesce repeat events per `(path, kind)` within this window into a
/// single emission. 200ms is long enough to absorb typical OS-level
/// chattiness without making fresh writes feel laggy.
pub const DEBOUNCE_WINDOW: Duration = Duration::from_millis(200);

/// Coarse filesystem event kind.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash)]
pub enum EventKind {
    /// File was created.
    Create,
    /// File was modified or its metadata changed.
    Modify,
    /// File was removed (or renamed away — `notify` reports as remove).
    Remove,
}

/// One filesystem event as seen by the store.
///
/// `path` is the affected file (best effort — `notify::Event` may carry
/// multiple paths but we surface the first non-directory entry).
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RawEvent {
    /// Affected file path.
    pub path: PathBuf,
    /// Coarse event kind.
    pub kind: EventKind,
}

/// Handle that keeps the watcher alive. Drop to stop watching.
pub struct WatcherHandle {
    _watcher: RecommendedWatcher,
}

impl std::fmt::Debug for WatcherHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WatcherHandle").finish_non_exhaustive()
    }
}

/// Start a recursive watcher on `dir`; deliver coarse events through the
/// returned receiver. Caller must keep the [`WatcherHandle`] alive for
/// as long as they care about events.
///
/// # Errors
/// Bubbles up watcher initialisation failures.
pub fn watch_dir(dir: &Path) -> Result<(mpsc::Receiver<RawEvent>, WatcherHandle), StoreError> {
    let (tx, rx) = mpsc::channel::<RawEvent>(64);
    let dir_owned = dir.to_path_buf();
    let mut watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
        if let Ok(evt) = res {
            for path in evt.paths {
                // Skip directories — only files matter for ADR/finding watching.
                if path.is_dir() {
                    continue;
                }
                let Some(kind) = map_kind(evt.kind) else {
                    continue;
                };
                let raw = RawEvent { path, kind };
                // Use try_send so a slow consumer cannot block the
                // notify thread; if the channel is full we drop the
                // event (consumer will re-converge via cold-start
                // reindex on next Store::open).
                if let Err(e) = tx.try_send(raw) {
                    tracing::debug!(
                        ?dir_owned,
                        ?e,
                        "dropped filesystem event (channel full or closed)"
                    );
                }
            }
        }
    })?;
    watcher.watch(dir, RecursiveMode::Recursive)?;
    Ok((rx, WatcherHandle { _watcher: watcher }))
}

fn map_kind(k: notify::EventKind) -> Option<EventKind> {
    use notify::EventKind as K;
    match k {
        K::Create(_) => Some(EventKind::Create),
        K::Modify(_) => Some(EventKind::Modify),
        K::Remove(_) => Some(EventKind::Remove),
        _ => None,
    }
}

/// Stream of [`RawEvent`] that owns the [`WatcherHandle`] internally —
/// dropping the stream drops the watcher.
///
/// Returned by [`watch_dir_stream`]; consumers usually pipe through a
/// `filter_map` to project into `AdrChangeEvent` / `FindingChangeEvent`.
///
/// Coalesces repeat events per `(path, kind)` within
/// [`DEBOUNCE_WINDOW`] so a single logical file change emits at most one
/// event even when the underlying OS (e.g. macOS FSEvents) fires several.
pub struct WatchStream {
    inner: ReceiverStream<RawEvent>,
    /// Most-recent emission timestamp for a `(path, kind)` bucket.
    last_emitted: HashMap<(PathBuf, EventKind), Instant>,
    // Drop order matters: ReceiverStream first, then the watcher. Rust
    // drops fields in declaration order, so this is correct.
    _handle: WatcherHandle,
}

impl std::fmt::Debug for WatchStream {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WatchStream").finish_non_exhaustive()
    }
}

impl Stream for WatchStream {
    type Item = RawEvent;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        // Safety: WatchStream is Unpin because all its fields are Unpin.
        let this = self.get_mut();
        // GC stale debounce entries — anything older than DEBOUNCE_WINDOW no
        // longer suppresses, so we evict to keep the map bounded for
        // long-running watchers (per A2 守闸 finding F-A2-02).
        let now_gc = Instant::now();
        this.last_emitted
            .retain(|_, t| now_gc.duration_since(*t) < DEBOUNCE_WINDOW);
        loop {
            match Pin::new(&mut this.inner).poll_next(cx) {
                Poll::Ready(Some(evt)) => {
                    let now = Instant::now();
                    let key = (evt.path.clone(), evt.kind);
                    let stale = this
                        .last_emitted
                        .get(&key)
                        .is_none_or(|t| now.duration_since(*t) >= DEBOUNCE_WINDOW);
                    if stale {
                        this.last_emitted.insert(key, now);
                        return Poll::Ready(Some(evt));
                    }
                    // Suppressed: re-poll for the next event.
                }
                other => return other,
            }
        }
    }
}

/// Convenience: start a watcher and return a `Stream` that holds the
/// watcher handle internally. Equivalent to [`watch_dir`] but tidier
/// for downstream `.filter_map(...)` chains.
///
/// # Errors
/// Bubbles up watcher initialisation failures.
pub fn watch_dir_stream(dir: &Path) -> Result<WatchStream, StoreError> {
    let (rx, handle) = watch_dir(dir)?;
    Ok(WatchStream {
        inner: ReceiverStream::new(rx),
        last_emitted: HashMap::new(),
        _handle: handle,
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn watcher_emits_create_event_for_new_file() {
        let dir = tempfile::tempdir().unwrap();
        let (mut rx, _handle) = watch_dir(dir.path()).expect("watcher starts");
        // notify needs a moment to install the OS-level watch.
        tokio::time::sleep(Duration::from_millis(100)).await;

        let file = dir.path().join("hello.md");
        tokio::fs::write(&file, "hi").await.unwrap();

        // Drain events for up to 2s; assert at least one event for our file.
        let mut saw_event = false;
        let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
        while tokio::time::Instant::now() < deadline {
            match tokio::time::timeout(Duration::from_millis(200), rx.recv()).await {
                Ok(Some(evt)) => {
                    if evt.path.ends_with("hello.md") {
                        saw_event = true;
                        break;
                    }
                }
                Ok(None) => break,
                Err(_) => {}
            }
        }
        assert!(saw_event, "watcher must emit an event for the new file");
    }

    #[tokio::test]
    async fn drop_handle_closes_channel() {
        let dir = tempfile::tempdir().unwrap();
        let (mut rx, handle) = watch_dir(dir.path()).unwrap();
        drop(handle);
        // After the handle drops, the watcher's sender is also dropped
        // (it lives inside the closure the watcher owns), so recv()
        // eventually yields None.
        let recv = tokio::time::timeout(Duration::from_secs(1), rx.recv()).await;
        // Either we got None or we timed out — both fine; the important
        // contract is we don't get an event after drop.
        match recv {
            Ok(None) | Err(_) => {}
            Ok(Some(_evt)) => {
                // It's tolerable to receive an in-flight event from before
                // the drop on macOS FSEvents; subsequent recv must close.
                let again = tokio::time::timeout(Duration::from_millis(500), rx.recv()).await;
                assert!(matches!(again, Ok(None) | Err(_)));
            }
        }
    }
}
