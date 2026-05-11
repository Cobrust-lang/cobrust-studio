//! Error type for the studio-store crate.
//!
//! All public fallible APIs return `Result<T, StoreError>`. Variants are
//! coarse on purpose — callers (studio-server routes, UI handlers) care
//! about "did this fail" + a human message far more than they care about
//! distinguishing every libc errno. When a downstream crate needs to
//! branch on cause (e.g. `is_not_found`), inspect `&self` with a `match`.

use std::path::PathBuf;

/// Errors produced by `studio-store` public APIs.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    /// Filesystem I/O failure (read, write, mkdir, permissions).
    #[error("I/O error at {path}: {source}")]
    Io {
        /// The path the operation was attempting to touch (best effort —
        /// may be the project root when the failing op was not file-bound).
        path: PathBuf,
        /// Underlying `std::io::Error`.
        #[source]
        source: std::io::Error,
    },

    /// YAML frontmatter could not be parsed.
    #[error("frontmatter parse error in {path}: {source}")]
    Frontmatter {
        /// Markdown file the parser was reading.
        path: PathBuf,
        /// Underlying `serde_yaml::Error`.
        #[source]
        source: serde_yaml::Error,
    },

    /// JSONL ledger line could not be parsed.
    #[error("ledger entry parse error at line {line} in {path}: {source}")]
    LedgerParse {
        /// JSONL file that failed.
        path: PathBuf,
        /// 1-based line number in the JSONL file.
        line: usize,
        /// Underlying serde_json error.
        #[source]
        source: serde_json::Error,
    },

    /// SQLite operation failed.
    #[error("sqlite error: {0}")]
    Sqlite(#[from] sqlx::Error),

    /// File or row not found at the requested id/slug.
    #[error("not found: {0}")]
    NotFound(String),

    /// Caller asked to create an item that already exists.
    #[error("already exists: {0}")]
    AlreadyExists(String),

    /// Markdown body is missing the leading `---` frontmatter fence.
    #[error("missing frontmatter fence in {0}")]
    MissingFrontmatter(PathBuf),

    /// Filesystem watcher initialisation failed.
    #[error("watcher init error: {0}")]
    Watcher(#[from] notify::Error),

    /// Invalid input from caller (validation error before any side-effect).
    #[error("invalid input: {0}")]
    InvalidInput(String),
}

impl StoreError {
    /// Convenience: wrap a `std::io::Error` carrying the offending path.
    #[must_use]
    pub fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            source,
        }
    }

    /// True when the error is a "not found" miss. Callers translating
    /// store errors to HTTP 404 use this.
    #[must_use]
    pub fn is_not_found(&self) -> bool {
        matches!(self, Self::NotFound(_))
    }
}
