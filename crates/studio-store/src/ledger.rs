//! Ledger materialized view.
//!
//! Per ADR-0006 §"Addendum 2026-05-11" F-02, the **JSONL ledger written
//! by [`studio_router::Ledger`] is the canonical source of truth** for
//! dispatch records. This module:
//!
//! - Re-exports the wire types `LedgerEntry` / `Outcome` so the two
//!   crates share one shape (no shadow type, no manual serde-compat).
//! - Maintains a SQLite materialized view (`ledger_view` table) for fast
//!   `recent(n)` queries used by the UI.
//! - `append(entry)` writes the entry to the SQLite view ONLY — the
//!   caller (studio-server) is responsible for *also* appending the
//!   same entry to the router's JSONL via [`studio_router::Ledger`].
//!   When you want a hands-off cold-start sync, call
//!   [`LedgerHandle::sync_from_jsonl`].
//!
//! Choice rationale: writing to SQLite only (not to JSONL) keeps the
//! dependency direction clean — studio-store never writes the canonical
//! source, only reads/indexes it. The hot path is the router's JSONL
//! mutex; the materialized view trails by zero rows when the caller
//! pairs `Ledger::append` with `LedgerHandle::append` in the same
//! dispatch path (studio-server's responsibility).

use std::path::Path;

use crate::Store;
use crate::error::StoreError;

/// One record in the ledger.
///
/// Re-export of [`studio_router::ledger::LedgerEntry`] (F-02): the
/// JSONL ledger written by `studio-router::Ledger` and the SQLite
/// materialized view used by `studio-store::ledger` share this exact
/// wire shape so a JSONL line round-trips into the view and back out.
pub use studio_router::ledger::LedgerEntry;

/// Outcome label for a completion attempt. Re-export of
/// [`studio_router::ledger::Outcome`].
pub use studio_router::ledger::Outcome;

/// Sub-handle returned by [`Store::ledger`].
#[derive(Debug)]
pub struct LedgerHandle<'a> {
    store: &'a Store,
}

impl<'a> LedgerHandle<'a> {
    pub(crate) const fn new(store: &'a Store) -> Self {
        Self { store }
    }

    /// Append `entry` to the materialized view.
    ///
    /// This does NOT write to the JSONL file; that's the caller's
    /// responsibility (studio-server pairs this with a call into
    /// [`studio_router::Ledger::append`] on the same dispatch path).
    /// The studio-server wrapper that calls both can later be folded
    /// behind a single helper if the boilerplate gets old.
    ///
    /// # Errors
    /// SQLite or JSON-encoding errors bubble up.
    pub async fn append(&self, entry: &LedgerEntry) -> Result<(), StoreError> {
        let raw_json = serde_json::to_string(entry).map_err(|e| StoreError::LedgerParse {
            path: self.store.db_path().to_path_buf(),
            line: 0,
            source: e,
        })?;
        let provider_kind = serde_json::to_value(entry.provider_kind)
            .ok()
            .and_then(|v| v.as_str().map(str::to_string));
        let outcome = serde_json::to_value(entry.outcome)
            .ok()
            .and_then(|v| v.as_str().map(str::to_string))
            .unwrap_or_else(|| "ok".to_string());

        sqlx::query(
            "INSERT INTO ledger_view (
                ts, task_tag, provider, provider_kind, model, cache_key,
                cache_hit, prompt_tokens, completion_tokens, total_tokens,
                latency_ms, attempt, outcome, error_code, raw_json
             ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&entry.ts)
        .bind(entry.task_tag.as_deref())
        .bind(&entry.provider)
        .bind(provider_kind)
        .bind(&entry.model)
        .bind(&entry.cache_key)
        .bind(i64::from(entry.cache_hit))
        .bind(i64::from(entry.prompt_tokens))
        .bind(i64::from(entry.completion_tokens))
        .bind(i64::from(entry.total_tokens))
        .bind(i64::from(entry.latency_ms))
        .bind(i64::from(entry.attempt))
        .bind(outcome)
        .bind(entry.error_code.as_deref())
        .bind(raw_json)
        .execute(self.store.pool())
        .await?;
        Ok(())
    }

    /// Fetch the `n` most recent entries in reverse-chronological order
    /// (newest first).
    ///
    /// # Errors
    /// SQLite or JSON-decoding errors bubble up.
    pub async fn recent(&self, n: usize) -> Result<Vec<LedgerEntry>, StoreError> {
        let limit = i64::try_from(n).unwrap_or(i64::MAX);
        let rows: Vec<(String,)> =
            sqlx::query_as("SELECT raw_json FROM ledger_view ORDER BY ts DESC, rowid DESC LIMIT ?")
                .bind(limit)
                .fetch_all(self.store.pool())
                .await?;
        let mut out = Vec::with_capacity(rows.len());
        for (raw,) in rows {
            let entry: LedgerEntry =
                serde_json::from_str(&raw).map_err(|e| StoreError::LedgerParse {
                    path: self.store.db_path().to_path_buf(),
                    line: 0,
                    source: e,
                })?;
            out.push(entry);
        }
        Ok(out)
    }

    /// Count rows currently in the materialized view.
    ///
    /// # Errors
    /// SQLite errors bubble up.
    pub async fn count(&self) -> Result<u64, StoreError> {
        let (n,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM ledger_view")
            .fetch_one(self.store.pool())
            .await?;
        Ok(u64::try_from(n).unwrap_or(0))
    }

    /// Truncate the materialized view. Used by `sync_from_jsonl` and by
    /// tests; intentionally not part of the §"Public surface" trio
    /// (`append` / `recent` / sync) the module-doc names.
    ///
    /// # Errors
    /// SQLite errors bubble up.
    pub async fn clear(&self) -> Result<(), StoreError> {
        sqlx::query("DELETE FROM ledger_view")
            .execute(self.store.pool())
            .await?;
        Ok(())
    }

    /// Import the entire JSONL ledger at `path` into the view.
    ///
    /// **Replaces** the view contents; safe to call on cold start to
    /// converge after the router crate has been writing JSONL while
    /// the store was offline. To incrementally tail a live JSONL file,
    /// pair this with [`crate::watch::watch_dir`] on the file's parent
    /// dir and re-call `sync_from_jsonl` when a Modify event fires —
    /// for M1 we keep it explicit rather than auto-tailing.
    ///
    /// Per `studio_router::ledger` docstring: "readers must tolerate at
    /// most one trailing partial line in case of crash mid-write". The
    /// last line is treated as discardable when it fails to parse —
    /// earlier partial lines are still an error.
    ///
    /// # Errors
    /// I/O or JSON-parse errors bubble up (except the last-line tail).
    pub async fn sync_from_jsonl(&self, path: &Path) -> Result<usize, StoreError> {
        let bytes = match tokio::fs::read(path).await {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(0),
            Err(e) => return Err(StoreError::io(path, e)),
        };
        // Replace contents to keep the view in sync with the file
        // (rather than accumulating duplicates across repeated calls).
        self.clear().await?;

        let text = String::from_utf8_lossy(&bytes);
        // Detect a trailing partial line (final byte != '\n').
        let has_trailing_partial = !bytes.is_empty() && bytes.last() != Some(&b'\n');

        let lines: Vec<&str> = text.lines().collect();
        let total = lines.len();
        let mut imported = 0_usize;
        for (idx, line) in lines.iter().enumerate() {
            let line_no = idx + 1;
            if line.trim().is_empty() {
                continue;
            }
            // Tolerate the trailing partial line specifically: if parse
            // fails AND this is the last line AND the file does not end
            // with '\n', skip it silently (router crashed mid-write).
            let entry: LedgerEntry = match serde_json::from_str::<LedgerEntry>(line) {
                Ok(e) => e,
                Err(source) => {
                    if has_trailing_partial && line_no == total {
                        tracing::debug!(
                            ?path,
                            line = line_no,
                            "tolerating trailing partial JSONL line"
                        );
                        continue;
                    }
                    return Err(StoreError::LedgerParse {
                        path: path.to_path_buf(),
                        line: line_no,
                        source,
                    });
                }
            };
            self.append(&entry).await?;
            imported += 1;
        }
        Ok(imported)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use studio_router::config::ProviderKind;
    use studio_router::provider::TokenUsage;

    fn sample_entry(ts: &str) -> LedgerEntry {
        LedgerEntry::ok(
            ts.to_string(),
            Some("agent-turn".to_string()),
            "anthropic_official",
            Some(ProviderKind::Anthropic),
            "claude-opus-4-7",
            "blake3:abcd",
            false,
            TokenUsage {
                prompt_tokens: 10,
                completion_tokens: 20,
            },
            123,
            1,
        )
    }

    #[tokio::test]
    async fn append_then_recent_round_trips() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::open(tmp.path()).await.unwrap();
        let l = store.ledger();
        for i in 0..5 {
            let e = sample_entry(&format!("2026-05-11T00:00:0{i}Z"));
            l.append(&e).await.unwrap();
        }
        let recent = l.recent(3).await.unwrap();
        assert_eq!(recent.len(), 3);
        // Recent returns newest first; the 4th-indexed insertion ts ends in 4.
        assert!(
            recent[0].ts.ends_with("04Z"),
            "expected newest first, got {recent:?}"
        );
        assert!(recent[2].ts.ends_with("02Z"));
    }

    #[tokio::test]
    async fn sync_from_jsonl_imports_router_format() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::open(tmp.path()).await.unwrap();
        let jsonl_path = tmp.path().join("router.jsonl");

        // Write 3 entries via the router's own Ledger to ensure byte-
        // for-byte compatibility — this is the realistic flow.
        let router_ledger = studio_router::Ledger::open(jsonl_path.clone())
            .await
            .unwrap();
        router_ledger
            .append(&sample_entry("2026-05-11T00:00:01Z"))
            .await
            .unwrap();
        router_ledger
            .append(&sample_entry("2026-05-11T00:00:02Z"))
            .await
            .unwrap();
        router_ledger
            .append(&sample_entry("2026-05-11T00:00:03Z"))
            .await
            .unwrap();
        drop(router_ledger);

        let imported = store.ledger().sync_from_jsonl(&jsonl_path).await.unwrap();
        assert_eq!(imported, 3);
        let count = store.ledger().count().await.unwrap();
        assert_eq!(count, 3);

        // Re-syncing should idempotently replace not append.
        let imported2 = store.ledger().sync_from_jsonl(&jsonl_path).await.unwrap();
        assert_eq!(imported2, 3);
        assert_eq!(store.ledger().count().await.unwrap(), 3);
    }

    #[tokio::test]
    async fn sync_from_jsonl_missing_file_is_noop() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::open(tmp.path()).await.unwrap();
        let n = store
            .ledger()
            .sync_from_jsonl(&tmp.path().join("nope.jsonl"))
            .await
            .unwrap();
        assert_eq!(n, 0);
    }

    #[tokio::test]
    async fn recent_zero_returns_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::open(tmp.path()).await.unwrap();
        assert!(store.ledger().recent(0).await.unwrap().is_empty());
    }
}
