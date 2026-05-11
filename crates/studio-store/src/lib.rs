//! `studio-store` — persistence layer for Cobrust Studio.
//!
//! Per ADR-0004:
//! - Markdown files under `docs/agent/{adr,findings}/` are the **source of
//!   truth** for decision records and findings.
//! - SQLite (`<project_root>/.cobrust-studio/studio.db`, WAL mode) is a
//!   **materialized index** rebuilt from disk on cold start and kept in
//!   sync via the `notify` filesystem watcher.
//!
//! Per ADR-0006 §"Addendum 2026-05-11" F-02:
//! - The JSONL ledger written by [`studio_router::Ledger`] is the source
//!   of truth for dispatch records. [`ledger`] here is a **reader** —
//!   imports the JSONL into a SQLite materialized view for fast
//!   `recent(n)` queries.
//! - [`LedgerEntry`] and [`Outcome`] are re-exported from `studio-router`
//!   so the two crates share one wire shape.
//!
//! # Shape of the public surface
//!
//! The crate exposes an aggregate [`Store`] handle:
//!
//! ```ignore
//! let store = studio_store::Store::open(project_root).await?;
//! let adrs = store.adr().list().await?;
//! let entries = store.ledger().recent(20).await?;
//! ```
//!
//! Per-module free-function aliases (`adr::list_in(...)` etc.) are
//! available for callers that don't want to thread the `Store` handle —
//! they take a `&Store` as their first arg. The naming intentionally
//! mirrors `docs/agent/modules/studio-store.md` §"Public surface (M1
//! target)".

#![allow(clippy::missing_errors_doc)] // each fn documents its errors inline

pub mod adr;
pub mod error;
pub mod finding;
pub mod ledger;
pub mod session;
pub mod watch;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use sqlx::SqlitePool;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};

pub use crate::adr::{Adr, AdrChangeEvent, AdrDraft, AdrSummary};
pub use crate::error::StoreError;
pub use crate::finding::{Finding, FindingChangeEvent, FindingDraft, FindingSummary};
pub use crate::ledger::{LedgerEntry, Outcome};
pub use crate::session::EncryptedBlob;

/// Default project-root-relative directory for the SQLite index.
pub const DATA_DIR_NAME: &str = ".cobrust-studio";

/// Default SQLite filename inside [`DATA_DIR_NAME`].
pub const DB_FILE_NAME: &str = "studio.db";

/// Default ADR directory relative to project root.
pub const ADR_DIR: &str = "docs/agent/adr";

/// Default findings directory relative to project root.
pub const FINDING_DIR: &str = "docs/agent/findings";

/// Aggregate handle for the studio-store persistence layer.
///
/// Cheap to clone; all clones share the same underlying connection pool
/// and project-root paths. Construct via [`Store::open`].
#[derive(Clone, Debug)]
pub struct Store {
    inner: Arc<StoreInner>,
}

#[derive(Debug)]
struct StoreInner {
    project_root: PathBuf,
    adr_dir: PathBuf,
    finding_dir: PathBuf,
    db_path: PathBuf,
    pool: SqlitePool,
}

impl Store {
    /// Open (or create) a store rooted at `project_root`.
    ///
    /// Side effects:
    /// - Creates `<project_root>/.cobrust-studio/` if missing.
    /// - Opens (or creates) `studio.db` in WAL mode.
    /// - Runs schema migrations.
    /// - Walks `docs/agent/{adr,findings}/` and repopulates the index.
    ///
    /// # Errors
    /// Bubbles up I/O, SQLite, or parse failures during the cold-start walk.
    pub async fn open(project_root: impl Into<PathBuf>) -> Result<Self, StoreError> {
        let project_root = project_root.into();
        let data_dir = project_root.join(DATA_DIR_NAME);
        let db_path = data_dir.join(DB_FILE_NAME);
        let adr_dir = project_root.join(ADR_DIR);
        let finding_dir = project_root.join(FINDING_DIR);

        tokio::fs::create_dir_all(&data_dir)
            .await
            .map_err(|e| StoreError::io(&data_dir, e))?;
        // The ADR / finding dirs may legitimately not exist yet (e.g. a
        // brand-new project pointed at Studio). Create them so the watcher
        // and create() flows have somewhere to write.
        tokio::fs::create_dir_all(&adr_dir)
            .await
            .map_err(|e| StoreError::io(&adr_dir, e))?;
        tokio::fs::create_dir_all(&finding_dir)
            .await
            .map_err(|e| StoreError::io(&finding_dir, e))?;

        let options = SqliteConnectOptions::new()
            .filename(&db_path)
            .create_if_missing(true)
            .journal_mode(sqlx::sqlite::SqliteJournalMode::Wal)
            .synchronous(sqlx::sqlite::SqliteSynchronous::Normal)
            .foreign_keys(true);

        let pool = SqlitePoolOptions::new()
            .max_connections(8)
            .connect_with(options)
            .await?;

        let inner = StoreInner {
            project_root,
            adr_dir,
            finding_dir,
            db_path,
            pool,
        };

        let store = Self {
            inner: Arc::new(inner),
        };

        store.migrate().await?;
        store.adr().reindex().await?;
        store.finding().reindex().await?;

        Ok(store)
    }

    /// Project root the store is anchored at.
    #[must_use]
    pub fn project_root(&self) -> &Path {
        &self.inner.project_root
    }

    /// Directory where ADR markdown lives.
    #[must_use]
    pub fn adr_dir(&self) -> &Path {
        &self.inner.adr_dir
    }

    /// Directory where finding markdown lives.
    #[must_use]
    pub fn finding_dir(&self) -> &Path {
        &self.inner.finding_dir
    }

    /// Path to the SQLite database file.
    #[must_use]
    pub fn db_path(&self) -> &Path {
        &self.inner.db_path
    }

    /// Borrow the underlying connection pool.
    #[must_use]
    pub fn pool(&self) -> &SqlitePool {
        &self.inner.pool
    }

    /// Sub-handle for ADR markdown operations.
    #[must_use]
    pub fn adr(&self) -> adr::AdrHandle<'_> {
        adr::AdrHandle::new(self)
    }

    /// Sub-handle for finding markdown operations.
    #[must_use]
    pub fn finding(&self) -> finding::FindingHandle<'_> {
        finding::FindingHandle::new(self)
    }

    /// Sub-handle for ledger materialized-view operations.
    #[must_use]
    pub fn ledger(&self) -> ledger::LedgerHandle<'_> {
        ledger::LedgerHandle::new(self)
    }

    /// Sub-handle for session state (encrypted endpoint blob).
    #[must_use]
    pub fn session(&self) -> session::SessionHandle<'_> {
        session::SessionHandle::new(self)
    }

    /// Run schema migrations. Idempotent.
    async fn migrate(&self) -> Result<(), StoreError> {
        // Inline schema (no separate migrations dir for M1 — single revision).
        let schema = r"
            CREATE TABLE IF NOT EXISTS adr_index (
                adr_id     TEXT PRIMARY KEY,
                title      TEXT NOT NULL,
                status     TEXT NOT NULL,
                date       TEXT NOT NULL,
                path       TEXT NOT NULL,
                mtime_ns   INTEGER NOT NULL DEFAULT 0
            );

            CREATE TABLE IF NOT EXISTS finding_index (
                finding_id TEXT PRIMARY KEY,
                title      TEXT NOT NULL,
                status     TEXT NOT NULL,
                date       TEXT NOT NULL,
                path       TEXT NOT NULL,
                mtime_ns   INTEGER NOT NULL DEFAULT 0
            );

            CREATE TABLE IF NOT EXISTS ledger_view (
                rowid      INTEGER PRIMARY KEY AUTOINCREMENT,
                ts         TEXT NOT NULL,
                task_tag   TEXT,
                provider   TEXT NOT NULL,
                provider_kind TEXT,
                model      TEXT NOT NULL,
                cache_key  TEXT NOT NULL,
                cache_hit  INTEGER NOT NULL,
                prompt_tokens     INTEGER NOT NULL,
                completion_tokens INTEGER NOT NULL,
                total_tokens      INTEGER NOT NULL,
                latency_ms INTEGER NOT NULL,
                attempt    INTEGER NOT NULL,
                outcome    TEXT NOT NULL,
                error_code TEXT,
                raw_json   TEXT NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_ledger_ts ON ledger_view(ts DESC);

            CREATE TABLE IF NOT EXISTS session_kv (
                key   TEXT PRIMARY KEY,
                value BLOB NOT NULL,
                nonce BLOB NOT NULL,
                scheme TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );
        ";
        sqlx::raw_sql(schema).execute(&self.inner.pool).await?;
        Ok(())
    }
}

/// Crate version.
#[must_use]
pub const fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn version_is_pkg_version() {
        assert_eq!(version(), env!("CARGO_PKG_VERSION"));
    }

    #[tokio::test]
    async fn store_open_creates_data_dirs_and_db() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Store::open(tmp.path()).await.unwrap();
        assert!(store.db_path().exists());
        assert!(tmp.path().join(DATA_DIR_NAME).is_dir());
        assert!(tmp.path().join(ADR_DIR).is_dir());
        assert!(tmp.path().join(FINDING_DIR).is_dir());
    }

    #[tokio::test]
    async fn store_open_is_idempotent_across_reopens() {
        let tmp = tempfile::tempdir().unwrap();
        let _s1 = Store::open(tmp.path()).await.unwrap();
        let _s2 = Store::open(tmp.path()).await.unwrap();
        // Should not have crashed; the schema migrations are idempotent.
    }
}
