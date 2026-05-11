//! ADR markdown CRUD + frontmatter parser.
//!
//! Per ADR-0004, ADR markdown files at `docs/agent/adr/NNNN-*.md` are the
//! source of truth; this module maintains a SQLite materialized index for
//! fast list queries.
//!
//! The frontmatter contract is defined by `docs/agent/conventions.md`:
//!
//! ```yaml
//! ---
//! adr_id: "NNNN"
//! title: <short title>
//! status: proposed | accepted | superseded | deprecated
//! date: YYYY-MM-DD
//! supersedes: [adr_id, ...]
//! superseded_by: []
//! ---
//! ```
//!
//! `adr_id` is permissively typed (some on-disk files use bare numerics,
//! others use quoted strings). The parser coerces both to a canonical
//! 4-digit `String` like `"0006"`.

use std::path::{Path, PathBuf};

use futures::{Stream, StreamExt};
use serde::{Deserialize, Serialize};

use crate::Store;
use crate::error::StoreError;
use crate::watch;

/// Summary projection of an ADR — the listing/index row.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct AdrSummary {
    /// Canonical 4-digit string id (`"0006"`).
    pub adr_id: String,
    /// One-line title.
    pub title: String,
    /// Lifecycle status: `proposed | accepted | superseded | deprecated`.
    pub status: String,
    /// ISO 8601 calendar date (`YYYY-MM-DD`).
    pub date: String,
    /// Absolute path on disk.
    pub path: PathBuf,
}

/// Full ADR — summary + body.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Adr {
    /// Embedded summary fields.
    pub summary: AdrSummary,
    /// Markdown body *after* the closing `---` fence (no leading newline).
    pub body: String,
    /// IDs this ADR supersedes (frontmatter `supersedes:`).
    pub supersedes: Vec<String>,
    /// IDs that supersede this one (frontmatter `superseded_by:`).
    pub superseded_by: Vec<String>,
}

/// Draft for [`AdrHandle::create`].
#[derive(Clone, Debug)]
pub struct AdrDraft {
    /// One-line title (used in filename slug).
    pub title: String,
    /// Lifecycle status (typically `"proposed"` for new drafts).
    pub status: String,
    /// ISO date.
    pub date: String,
    /// Markdown body — `## Context`, `## Decision`, etc.
    pub body: String,
    /// Optional explicit id; when `None`, allocator picks max(existing)+1.
    pub adr_id: Option<String>,
    /// IDs this ADR supersedes.
    pub supersedes: Vec<String>,
}

/// Event emitted by [`AdrHandle::watch`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AdrChangeEvent {
    /// New ADR file created.
    Added(PathBuf),
    /// Existing ADR file edited.
    Modified(PathBuf),
    /// ADR file removed.
    Removed(PathBuf),
}

/// Frontmatter shape — internal to the parser.
#[derive(Debug, Deserialize)]
struct AdrFrontmatter {
    #[serde(deserialize_with = "deserialize_adr_id")]
    adr_id: String,
    title: String,
    status: String,
    #[serde(deserialize_with = "deserialize_date")]
    date: String,
    #[serde(default)]
    supersedes: Vec<serde_yaml::Value>,
    #[serde(default)]
    superseded_by: Vec<serde_yaml::Value>,
}

/// Coerce `adr_id` from either string `"0006"` or integer `6` to a
/// canonical 4-digit string.
fn deserialize_adr_id<'de, D>(d: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let v = serde_yaml::Value::deserialize(d)?;
    match v {
        serde_yaml::Value::String(s) => {
            // Normalize: strip whitespace, left-pad with zeros to 4 chars
            // if numeric, else keep as-is.
            let trimmed = s.trim();
            if let Ok(n) = trimmed.parse::<u32>() {
                Ok(format!("{n:04}"))
            } else {
                Ok(trimmed.to_string())
            }
        }
        serde_yaml::Value::Number(n) => {
            let n = n.as_u64().ok_or_else(|| {
                serde::de::Error::custom("adr_id numeric must be a non-negative integer")
            })?;
            Ok(format!("{n:04}"))
        }
        other => Err(serde::de::Error::custom(format!(
            "adr_id must be string or integer, got {other:?}"
        ))),
    }
}

fn deserialize_date<'de, D>(d: D) -> Result<String, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let v = serde_yaml::Value::deserialize(d)?;
    match v {
        serde_yaml::Value::String(s) => Ok(s),
        // YAML parses unquoted YYYY-MM-DD as a Number or a date depending on
        // dialect; in serde_yaml 0.9 it surfaces as a Tagged value. We
        // accept anything stringifiable.
        other => serde_yaml::to_string(&other)
            .map(|s| s.trim().to_string())
            .map_err(serde::de::Error::custom),
    }
}

fn coerce_id_list(raw: Vec<serde_yaml::Value>) -> Vec<String> {
    raw.into_iter()
        .filter_map(|v| match v {
            serde_yaml::Value::String(s) => Some(s),
            serde_yaml::Value::Number(n) => n.as_u64().map(|n| format!("{n:04}")),
            _ => None,
        })
        .collect()
}

/// Split a markdown document into `(frontmatter_yaml, body)`.
///
/// Returns `Err(StoreError::MissingFrontmatter)` when the doc does not
/// start with `---\n`.
fn split_frontmatter(path: &Path, text: &str) -> Result<(String, String), StoreError> {
    let mut lines = text.lines();
    let first = lines.next();
    if first != Some("---") {
        return Err(StoreError::MissingFrontmatter(path.to_path_buf()));
    }
    let mut yaml = String::new();
    let mut found_close = false;
    let mut body = String::new();
    let mut in_body = false;
    for line in lines {
        if in_body {
            body.push_str(line);
            body.push('\n');
        } else if line == "---" {
            in_body = true;
            found_close = true;
        } else {
            yaml.push_str(line);
            yaml.push('\n');
        }
    }
    if !found_close {
        return Err(StoreError::MissingFrontmatter(path.to_path_buf()));
    }
    Ok((yaml, body))
}

/// Parse a full ADR markdown document.
///
/// # Errors
/// Returns [`StoreError::MissingFrontmatter`] if the `---` fence is
/// absent, or [`StoreError::Frontmatter`] if the YAML is malformed.
pub fn parse_adr(path: &Path, text: &str) -> Result<Adr, StoreError> {
    let (yaml, body) = split_frontmatter(path, text)?;
    let fm: AdrFrontmatter =
        serde_yaml::from_str(&yaml).map_err(|source| StoreError::Frontmatter {
            path: path.to_path_buf(),
            source,
        })?;
    Ok(Adr {
        summary: AdrSummary {
            adr_id: fm.adr_id,
            title: fm.title,
            status: fm.status,
            date: fm.date,
            path: path.to_path_buf(),
        },
        body: body.trim_start_matches('\n').to_string(),
        supersedes: coerce_id_list(fm.supersedes),
        superseded_by: coerce_id_list(fm.superseded_by),
    })
}

/// Sub-handle returned by [`Store::adr`].
#[derive(Debug)]
pub struct AdrHandle<'a> {
    store: &'a Store,
}

impl<'a> AdrHandle<'a> {
    pub(crate) fn new(store: &'a Store) -> Self {
        Self { store }
    }

    /// List all ADRs (summary projection) ordered by `adr_id` ascending.
    ///
    /// Reads from the SQLite index.
    ///
    /// # Errors
    /// SQLite errors bubble up.
    pub async fn list(&self) -> Result<Vec<AdrSummary>, StoreError> {
        let rows = sqlx::query_as::<_, (String, String, String, String, String)>(
            "SELECT adr_id, title, status, date, path FROM adr_index ORDER BY adr_id ASC",
        )
        .fetch_all(self.store.pool())
        .await?;
        Ok(rows
            .into_iter()
            .map(|(adr_id, title, status, date, path)| AdrSummary {
                adr_id,
                title,
                status,
                date,
                path: PathBuf::from(path),
            })
            .collect())
    }

    /// Fetch a single ADR by id (e.g. `"0006"`).
    ///
    /// Returns `None` when the id is not in the index OR when the index
    /// row points at a path that no longer exists on disk.
    ///
    /// # Errors
    /// SQLite or parse errors bubble up.
    pub async fn get(&self, adr_id: &str) -> Result<Option<Adr>, StoreError> {
        let row: Option<(String,)> = sqlx::query_as("SELECT path FROM adr_index WHERE adr_id = ?")
            .bind(adr_id)
            .fetch_optional(self.store.pool())
            .await?;
        let Some((path_str,)) = row else {
            return Ok(None);
        };
        let path = PathBuf::from(path_str);
        let text = match tokio::fs::read_to_string(&path).await {
            Ok(t) => t,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(StoreError::io(&path, e)),
        };
        parse_adr(&path, &text).map(Some)
    }

    /// Create a new ADR markdown file and insert into the index.
    ///
    /// Filename slug is derived from `draft.title` (lowercase, kebab-case,
    /// non-alphanumeric collapsed). If `draft.adr_id` is `None` the next
    /// id after `max(existing)` is allocated.
    ///
    /// # Errors
    /// I/O, validation, or SQLite errors bubble up.
    /// Returns [`StoreError::AlreadyExists`] if the target file already
    /// exists on disk.
    pub async fn create(&self, draft: AdrDraft) -> Result<Adr, StoreError> {
        if draft.title.trim().is_empty() {
            return Err(StoreError::InvalidInput("title must be non-empty".into()));
        }
        let adr_id = match draft.adr_id.as_deref() {
            Some(id) => normalize_id(id)?,
            None => self.allocate_next_id().await?,
        };

        let slug = slugify(&draft.title);
        let filename = format!("{adr_id}-{slug}.md");
        let path = self.store.adr_dir().join(&filename);

        if tokio::fs::try_exists(&path)
            .await
            .map_err(|e| StoreError::io(&path, e))?
        {
            return Err(StoreError::AlreadyExists(filename));
        }

        let body_trimmed = draft.body.trim_end().to_string();
        let supersedes_yaml = format_id_list(&draft.supersedes);
        let document = format!(
            "---\nadr_id: \"{adr_id}\"\ntitle: {title}\nstatus: {status}\ndate: {date}\nsupersedes: {supersedes}\nsuperseded_by: []\n---\n\n{body}\n",
            adr_id = adr_id,
            title = yaml_escape_inline(&draft.title),
            status = draft.status,
            date = draft.date,
            supersedes = supersedes_yaml,
            body = body_trimmed,
        );

        tokio::fs::write(&path, &document)
            .await
            .map_err(|e| StoreError::io(&path, e))?;

        // Re-parse off the file so the returned Adr is byte-equivalent to a
        // subsequent get(). Cheap; we just wrote it.
        let parsed = parse_adr(&path, &document)?;
        upsert_index(self.store, &parsed).await?;
        Ok(parsed)
    }

    /// Stream of [`AdrChangeEvent`] for the ADR directory.
    ///
    /// The returned stream lives for as long as the caller holds it; the
    /// underlying watcher is dropped when the stream is dropped.
    ///
    /// # Errors
    /// Watcher initialisation failures bubble up.
    pub fn watch(&self) -> Result<impl Stream<Item = AdrChangeEvent> + Send + 'static, StoreError> {
        let raw = watch::watch_dir_stream(self.store.adr_dir())?;
        // `WatchStream` owns its `WatcherHandle`; dropping the returned
        // stream drops the watcher.
        let stream = raw.filter_map(|evt| futures::future::ready(map_event_to_adr(&evt)));
        Ok(stream)
    }

    /// Cold-start re-index: walk the ADR dir, parse every `*.md`, upsert
    /// into the index, prune rows whose files no longer exist.
    ///
    /// # Errors
    /// Bubbles up I/O, parse, or SQLite errors.
    pub async fn reindex(&self) -> Result<(), StoreError> {
        let dir = self.store.adr_dir().to_path_buf();
        let mut entries = match tokio::fs::read_dir(&dir).await {
            Ok(r) => r,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(StoreError::io(&dir, e)),
        };
        let mut seen_ids: Vec<String> = Vec::new();
        while let Some(entry) = entries
            .next_entry()
            .await
            .map_err(|e| StoreError::io(&dir, e))?
        {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("md") {
                continue;
            }
            // Skip `README.md`, `_template.md`.
            let stem = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or_default();
            if stem.starts_with('_') || stem == "README" {
                continue;
            }
            let text = match tokio::fs::read_to_string(&path).await {
                Ok(t) => t,
                Err(e) => return Err(StoreError::io(&path, e)),
            };
            let Ok(adr) = parse_adr(&path, &text) else {
                // Soft-fail: skip malformed files but don't poison cold-start.
                tracing::warn!(?path, "skipping malformed ADR during reindex");
                continue;
            };
            upsert_index(self.store, &adr).await?;
            seen_ids.push(adr.summary.adr_id);
        }
        prune_index(self.store, "adr_index", "adr_id", &seen_ids).await?;
        Ok(())
    }

    /// Look up the next free 4-digit ADR id (max-existing + 1; `"0001"` if empty).
    async fn allocate_next_id(&self) -> Result<String, StoreError> {
        let row: Option<(String,)> = sqlx::query_as(
            "SELECT MAX(adr_id) FROM adr_index WHERE adr_id GLOB '[0-9][0-9][0-9][0-9]'",
        )
        .fetch_optional(self.store.pool())
        .await?;
        let next: u32 = match row.and_then(|(s,)| s.parse::<u32>().ok()) {
            Some(n) => n + 1,
            None => 1,
        };
        Ok(format!("{next:04}"))
    }
}

fn map_event_to_adr(evt: &watch::RawEvent) -> Option<AdrChangeEvent> {
    let path = evt.path.clone();
    if path.extension().and_then(|e| e.to_str()) != Some("md") {
        return None;
    }
    Some(match evt.kind {
        watch::EventKind::Create => AdrChangeEvent::Added(path),
        watch::EventKind::Modify => AdrChangeEvent::Modified(path),
        watch::EventKind::Remove => AdrChangeEvent::Removed(path),
    })
}

async fn upsert_index(store: &Store, adr: &Adr) -> Result<(), StoreError> {
    let path = adr.summary.path.to_string_lossy().to_string();
    let mtime_ns = mtime_ns_of(&adr.summary.path).await.unwrap_or(0);
    sqlx::query(
        "INSERT INTO adr_index (adr_id, title, status, date, path, mtime_ns)
         VALUES (?, ?, ?, ?, ?, ?)
         ON CONFLICT(adr_id) DO UPDATE SET
            title=excluded.title,
            status=excluded.status,
            date=excluded.date,
            path=excluded.path,
            mtime_ns=excluded.mtime_ns",
    )
    .bind(&adr.summary.adr_id)
    .bind(&adr.summary.title)
    .bind(&adr.summary.status)
    .bind(&adr.summary.date)
    .bind(&path)
    .bind(mtime_ns)
    .execute(store.pool())
    .await?;
    Ok(())
}

pub(crate) async fn mtime_ns_of(path: &Path) -> Option<i64> {
    let meta = tokio::fs::metadata(path).await.ok()?;
    let mt = meta.modified().ok()?;
    let dur = mt.duration_since(std::time::SystemTime::UNIX_EPOCH).ok()?;
    i64::try_from(dur.as_nanos()).ok()
}

pub(crate) async fn prune_index(
    store: &Store,
    table: &'static str,
    id_col: &'static str,
    keep_ids: &[String],
) -> Result<(), StoreError> {
    if keep_ids.is_empty() {
        // Delete all rows from the table.
        let sql = format!("DELETE FROM {table}");
        sqlx::query(&sql).execute(store.pool()).await?;
        return Ok(());
    }
    // Build placeholders. keep_ids is bounded by the on-disk file count so
    // dynamic SQL with ? placeholders is acceptable here.
    let placeholders = keep_ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");
    let sql = format!("DELETE FROM {table} WHERE {id_col} NOT IN ({placeholders})");
    let mut q = sqlx::query(&sql);
    for id in keep_ids {
        q = q.bind(id);
    }
    q.execute(store.pool()).await?;
    Ok(())
}

fn slugify(title: &str) -> String {
    let mut out = String::with_capacity(title.len());
    let mut prev_dash = false;
    for ch in title.chars().flat_map(char::to_lowercase) {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
            prev_dash = false;
        } else if !prev_dash && !out.is_empty() {
            out.push('-');
            prev_dash = true;
        }
    }
    while out.ends_with('-') {
        out.pop();
    }
    if out.is_empty() {
        "untitled".to_string()
    } else {
        out
    }
}

fn normalize_id(raw: &str) -> Result<String, StoreError> {
    let trimmed = raw.trim();
    let n: u32 = trimmed
        .parse()
        .map_err(|_| StoreError::InvalidInput(format!("adr_id must be numeric: {trimmed}")))?;
    Ok(format!("{n:04}"))
}

fn format_id_list(ids: &[String]) -> String {
    if ids.is_empty() {
        "[]".to_string()
    } else {
        let inner = ids
            .iter()
            .map(|s| format!("\"{s}\""))
            .collect::<Vec<_>>()
            .join(", ");
        format!("[{inner}]")
    }
}

fn yaml_escape_inline(s: &str) -> String {
    // Title appears inline in `title: ...` — quote only if it would
    // confuse the YAML parser (leading `[`, `{`, `&`, `*`, `!`, `|`, `>`,
    // `%`, `@`, `\``, or contains `: ` / `#`).
    let needs_quote = s.is_empty()
        || s.starts_with(['[', '{', '&', '*', '!', '|', '>', '%', '@', '`', '\'', '"'])
        || s.contains(": ")
        || s.contains(" #");
    if needs_quote {
        format!("\"{}\"", s.replace('"', "\\\""))
    } else {
        s.to_string()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn slugify_basic() {
        assert_eq!(slugify("Hello World"), "hello-world");
        assert_eq!(slugify("Mix-Case & Punct!"), "mix-case-punct");
        assert_eq!(slugify("   "), "untitled");
        assert_eq!(slugify("ADR-0006 lift!"), "adr-0006-lift");
    }

    #[test]
    fn normalize_id_pads() {
        assert_eq!(normalize_id("6").unwrap(), "0006");
        assert_eq!(normalize_id("0006").unwrap(), "0006");
        assert_eq!(normalize_id(" 42 ").unwrap(), "0042");
        assert!(normalize_id("abc").is_err());
    }

    #[test]
    fn split_frontmatter_basic() {
        let text = "---\nadr_id: \"0001\"\ntitle: t\n---\nbody\nmore body\n";
        let path = PathBuf::from("/tmp/foo.md");
        let (yaml, body) = split_frontmatter(&path, text).unwrap();
        assert!(yaml.contains("adr_id"));
        assert!(body.contains("body"));
        assert!(body.contains("more body"));
    }

    #[test]
    fn split_frontmatter_missing_fence_errs() {
        let text = "no fence here\n";
        let path = PathBuf::from("/tmp/foo.md");
        let err = split_frontmatter(&path, text).unwrap_err();
        assert!(matches!(err, StoreError::MissingFrontmatter(_)));
    }

    #[test]
    fn parse_adr_quoted_id() {
        let text = "---\nadr_id: \"0006\"\ntitle: A title\nstatus: accepted\ndate: 2026-05-11\n---\n\nBody here.\n";
        let path = PathBuf::from("/tmp/0006-foo.md");
        let adr = parse_adr(&path, text).unwrap();
        assert_eq!(adr.summary.adr_id, "0006");
        assert_eq!(adr.summary.title, "A title");
        assert_eq!(adr.summary.status, "accepted");
        assert!(adr.body.starts_with("Body"));
    }

    #[test]
    fn parse_adr_numeric_id() {
        let text =
            "---\nadr_id: 6\ntitle: A title\nstatus: accepted\ndate: 2026-05-11\n---\n\nBody.\n";
        let path = PathBuf::from("/tmp/foo.md");
        let adr = parse_adr(&path, text).unwrap();
        assert_eq!(adr.summary.adr_id, "0006");
    }

    #[test]
    fn parse_real_adr_files_round_trip() {
        // Walk the real docs/agent/adr/ directory in the worktree.
        let workspace_root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .to_path_buf();
        let adr_dir = workspace_root.join("docs/agent/adr");
        let mut count = 0;
        for entry in std::fs::read_dir(&adr_dir).expect("adr dir exists") {
            let path = entry.unwrap().path();
            if path.extension().and_then(|e| e.to_str()) != Some("md") {
                continue;
            }
            let stem = path.file_stem().unwrap().to_str().unwrap();
            if stem.starts_with('_') || stem == "README" {
                continue;
            }
            let text = std::fs::read_to_string(&path).unwrap();
            let adr =
                parse_adr(&path, &text).unwrap_or_else(|e| panic!("must parse {path:?}: {e}"));
            assert!(!adr.summary.title.is_empty(), "title for {path:?}");
            assert!(!adr.summary.status.is_empty(), "status for {path:?}");
            assert_eq!(adr.summary.adr_id.len(), 4, "id padded for {path:?}");
            count += 1;
        }
        assert!(
            count >= 6,
            "should have parsed at least 6 ADRs; got {count}"
        );
    }
}
