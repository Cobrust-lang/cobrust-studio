//! Finding markdown CRUD — symmetric to the ADR module.
//!
//! Finding frontmatter per `docs/agent/conventions.md`:
//!
//! ```yaml
//! ---
//! doc_kind: finding
//! finding_id: <slug>
//! last_verified_commit: <sha>
//! severity: P1 | P2 | P3 | P4
//! status: open | closed_by_aX.Y
//! dependencies: [adr:NNNN, finding:slug, ...]
//! related: [...]
//! ---
//! ```
//!
//! All frontmatter fields except `finding_id` are optional; the parser fills
//! sensible defaults (`status` → `"open"`, `severity` → `"P3"`) when absent.

use std::path::{Path, PathBuf};

use futures::{Stream, StreamExt};
use serde::{Deserialize, Serialize};

use crate::Store;
use crate::adr::{mtime_ns_of, prune_index};
use crate::error::StoreError;
use crate::watch;

/// Summary projection of a finding — the listing/index row.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct FindingSummary {
    /// Slug id (e.g. `"a1-1-strip-2-noop-at-pin-61f2aff"`).
    pub finding_id: String,
    /// One-line title (the first `# ` heading in the body, or
    /// `finding_id` if no heading found).
    pub title: String,
    /// Lifecycle status (`open`, `closed_by_aX.Y`, etc.). Defaults to
    /// `"open"` when the file omits the field.
    pub status: String,
    /// Severity tag (`P1`/`P2`/`P3`/`P4`). Defaults to `"P3"`.
    pub severity: String,
    /// Date stamp — uses `last_verified_commit` when no `date` field
    /// is present (kept as a free-form string so callers can render
    /// either a SHA or an ISO date).
    pub date: String,
    /// Absolute path on disk.
    pub path: PathBuf,
}

impl FindingSummary {
    /// Slug id.
    #[must_use]
    pub fn finding_id(&self) -> &str {
        &self.finding_id
    }

    /// One-line title.
    #[must_use]
    pub fn title(&self) -> &str {
        &self.title
    }

    /// Lifecycle status string.
    #[must_use]
    pub fn status(&self) -> &str {
        &self.status
    }

    /// Severity tag.
    #[must_use]
    pub fn severity(&self) -> &str {
        &self.severity
    }

    /// Date stamp / verified-commit field.
    #[must_use]
    pub fn date(&self) -> &str {
        &self.date
    }

    /// Absolute path on disk.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// Full finding — summary + body.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct Finding {
    /// Embedded summary fields.
    pub summary: FindingSummary,
    /// Markdown body after the closing `---` fence.
    pub body: String,
    /// `dependencies: [...]` frontmatter list, surfaced as raw strings.
    pub dependencies: Vec<String>,
    /// `related: [...]` frontmatter list, surfaced as raw strings.
    pub related: Vec<String>,
}

impl Finding {
    /// Slug id.
    #[must_use]
    pub fn finding_id(&self) -> &str {
        &self.summary.finding_id
    }

    /// One-line title.
    #[must_use]
    pub fn title(&self) -> &str {
        &self.summary.title
    }

    /// Lifecycle status string.
    #[must_use]
    pub fn status(&self) -> &str {
        &self.summary.status
    }

    /// Severity tag.
    #[must_use]
    pub fn severity(&self) -> &str {
        &self.summary.severity
    }

    /// Date stamp / verified-commit field.
    #[must_use]
    pub fn date(&self) -> &str {
        &self.summary.date
    }

    /// Markdown body (after the closing `---` fence).
    #[must_use]
    pub fn body(&self) -> &str {
        &self.body
    }

    /// Absolute path on disk.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.summary.path
    }

    /// `dependencies: [...]` list.
    #[must_use]
    pub fn dependencies(&self) -> &[String] {
        &self.dependencies
    }

    /// `related: [...]` list.
    #[must_use]
    pub fn related(&self) -> &[String] {
        &self.related
    }
}

/// Draft for [`FindingHandle::create`].
#[derive(Clone, Debug)]
pub struct FindingDraft {
    /// Slug id; must be unique. Used as the filename stem.
    pub finding_id: String,
    /// `last_verified_commit` field.
    pub last_verified_commit: String,
    /// Severity tag (`P1`/`P2`/`P3`/`P4`).
    pub severity: String,
    /// Lifecycle status; defaults to `"open"`.
    pub status: String,
    /// `dependencies` list (e.g. `["adr:0006"]`).
    pub dependencies: Vec<String>,
    /// `related` list (free-form references).
    pub related: Vec<String>,
    /// One-line title (rendered as `# {title}` at the top of body).
    pub title: String,
    /// Markdown body (sections `## Hypothesis` etc.).
    pub body: String,
}

/// Event emitted by [`FindingHandle::watch`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum FindingChangeEvent {
    /// New finding file created.
    Added(PathBuf),
    /// Existing finding file edited.
    Modified(PathBuf),
    /// Finding file removed.
    Removed(PathBuf),
}

#[derive(Debug, Deserialize)]
struct FindingFrontmatter {
    finding_id: String,
    #[serde(default = "default_status")]
    status: String,
    #[serde(default = "default_severity")]
    severity: String,
    #[serde(default)]
    last_verified_commit: String,
    #[serde(default)]
    dependencies: Vec<serde_yaml::Value>,
    #[serde(default)]
    related: Vec<serde_yaml::Value>,
}

fn default_status() -> String {
    "open".to_string()
}

fn default_severity() -> String {
    "P3".to_string()
}

fn coerce_str_list(raw: Vec<serde_yaml::Value>) -> Vec<String> {
    raw.into_iter()
        .filter_map(|v| match v {
            serde_yaml::Value::String(s) => Some(s),
            _ => None,
        })
        .collect()
}

fn split_frontmatter(path: &Path, text: &str) -> Result<(String, String), StoreError> {
    let mut lines = text.lines();
    let first = lines.next();
    if first != Some("---") {
        return Err(StoreError::MissingFrontmatter(path.to_path_buf()));
    }
    let mut yaml = String::new();
    let mut body = String::new();
    let mut found_close = false;
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

/// Extract the first `# Heading` line from the body, falling back to
/// the slug when none exists.
fn extract_title(body: &str, fallback: &str) -> String {
    for raw in body.lines() {
        let trimmed = raw.trim_start();
        if let Some(rest) = trimmed.strip_prefix("# ") {
            return rest.trim().to_string();
        }
    }
    fallback.to_string()
}

/// Parse a full finding markdown document.
///
/// # Errors
/// Returns [`StoreError::MissingFrontmatter`] / [`StoreError::Frontmatter`]
/// when the document is malformed.
pub fn parse_finding(path: &Path, text: &str) -> Result<Finding, StoreError> {
    let (yaml, body) = split_frontmatter(path, text)?;
    let fm: FindingFrontmatter =
        serde_yaml::from_str(&yaml).map_err(|source| StoreError::Frontmatter {
            path: path.to_path_buf(),
            source,
        })?;
    let body_trimmed = body.trim_start_matches('\n').to_string();
    let title = extract_title(&body_trimmed, &fm.finding_id);
    Ok(Finding {
        summary: FindingSummary {
            finding_id: fm.finding_id,
            title,
            status: fm.status,
            severity: fm.severity,
            date: fm.last_verified_commit,
            path: path.to_path_buf(),
        },
        body: body_trimmed,
        dependencies: coerce_str_list(fm.dependencies),
        related: coerce_str_list(fm.related),
    })
}

/// Sub-handle returned by [`Store::finding`].
#[derive(Debug)]
pub struct FindingHandle<'a> {
    store: &'a Store,
}

impl<'a> FindingHandle<'a> {
    pub(crate) const fn new(store: &'a Store) -> Self {
        Self { store }
    }

    /// List all findings (summary projection) ordered by `finding_id`.
    ///
    /// # Errors
    /// SQLite errors bubble up.
    pub async fn list(&self) -> Result<Vec<FindingSummary>, StoreError> {
        let rows = sqlx::query_as::<_, (String, String, String, String, String, String)>(
            "SELECT finding_id, title, status, severity, date, path FROM finding_index ORDER BY finding_id ASC",
        )
        .fetch_all(self.store.pool())
        .await?;
        Ok(rows
            .into_iter()
            .map(
                |(finding_id, title, status, severity, date, path)| FindingSummary {
                    finding_id,
                    title,
                    status,
                    severity,
                    date,
                    path: PathBuf::from(path),
                },
            )
            .collect())
    }

    /// Fetch one finding by id.
    ///
    /// # Errors
    /// SQLite or parse errors bubble up.
    pub async fn get(&self, finding_id: &str) -> Result<Option<Finding>, StoreError> {
        let row: Option<(String,)> =
            sqlx::query_as("SELECT path FROM finding_index WHERE finding_id = ?")
                .bind(finding_id)
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
        parse_finding(&path, &text).map(Some)
    }

    /// Create a new finding markdown file and insert into the index.
    ///
    /// # Errors
    /// Validation / I/O / SQLite errors bubble up;
    /// [`StoreError::AlreadyExists`] when the slug collides with an
    /// existing on-disk file.
    pub async fn create(&self, draft: FindingDraft) -> Result<Finding, StoreError> {
        if draft.finding_id.trim().is_empty() {
            return Err(StoreError::InvalidInput(
                "finding_id must be non-empty".into(),
            ));
        }
        if draft.title.trim().is_empty() {
            return Err(StoreError::InvalidInput("title must be non-empty".into()));
        }
        let slug = draft.finding_id.trim();
        let filename = format!("{slug}.md");
        let path = self.store.finding_dir().join(&filename);
        if tokio::fs::try_exists(&path)
            .await
            .map_err(|e| StoreError::io(&path, e))?
        {
            return Err(StoreError::AlreadyExists(filename));
        }

        let status = if draft.status.trim().is_empty() {
            "open".to_string()
        } else {
            draft.status.clone()
        };
        let severity = if draft.severity.trim().is_empty() {
            "P3".to_string()
        } else {
            draft.severity.clone()
        };
        let deps_yaml = format_str_list(&draft.dependencies);
        let related_yaml = format_str_list(&draft.related);

        let document = format!(
            "---\ndoc_kind: finding\nfinding_id: {slug}\nlast_verified_commit: {commit}\nseverity: {severity}\nstatus: {status}\ndependencies: {deps}\nrelated: {related}\n---\n\n# {title}\n\n{body}\n",
            slug = slug,
            commit = draft.last_verified_commit,
            severity = severity,
            status = status,
            deps = deps_yaml,
            related = related_yaml,
            title = draft.title,
            body = draft.body.trim_end(),
        );
        tokio::fs::write(&path, &document)
            .await
            .map_err(|e| StoreError::io(&path, e))?;

        let parsed = parse_finding(&path, &document)?;
        upsert_index(self.store, &parsed).await?;
        Ok(parsed)
    }

    /// Stream of [`FindingChangeEvent`].
    ///
    /// Watcher initialisation failures are surfaced as an empty stream.
    pub fn watch(&self) -> impl Stream<Item = FindingChangeEvent> + Send + 'static {
        match watch::watch_dir_stream(self.store.finding_dir()) {
            Ok(raw) => futures::future::Either::Left(
                raw.filter_map(|evt| futures::future::ready(map_event_to_finding(&evt))),
            ),
            Err(e) => {
                tracing::warn!(error = ?e, "finding().watch() failed to initialise; returning empty stream");
                futures::future::Either::Right(futures::stream::empty())
            }
        }
    }

    /// Cold-start re-index of `docs/agent/findings/`.
    ///
    /// # Errors
    /// Bubbles up I/O / parse / SQLite errors.
    pub async fn reindex(&self) -> Result<(), StoreError> {
        let dir = self.store.finding_dir().to_path_buf();
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
            let stem = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or_default();
            if stem.starts_with('_') || stem == "README" {
                continue;
            }
            let text = tokio::fs::read_to_string(&path)
                .await
                .map_err(|e| StoreError::io(&path, e))?;
            let Ok(finding) = parse_finding(&path, &text) else {
                tracing::warn!(?path, "skipping malformed finding during reindex");
                continue;
            };
            upsert_index(self.store, &finding).await?;
            seen_ids.push(finding.summary.finding_id);
        }
        prune_index(self.store, "finding_index", "finding_id", &seen_ids).await?;
        Ok(())
    }
}

fn map_event_to_finding(evt: &watch::RawEvent) -> Option<FindingChangeEvent> {
    let path = evt.path.clone();
    if path.extension().and_then(|e| e.to_str()) != Some("md") {
        return None;
    }
    Some(match evt.kind {
        watch::EventKind::Create => FindingChangeEvent::Added(path),
        watch::EventKind::Modify => FindingChangeEvent::Modified(path),
        watch::EventKind::Remove => FindingChangeEvent::Removed(path),
    })
}

fn format_str_list(items: &[String]) -> String {
    if items.is_empty() {
        "[]".to_string()
    } else {
        let inner = items
            .iter()
            .map(|d| format!("\"{d}\""))
            .collect::<Vec<_>>()
            .join(", ");
        format!("[{inner}]")
    }
}

async fn upsert_index(store: &Store, finding: &Finding) -> Result<(), StoreError> {
    let path = finding.summary.path.to_string_lossy().to_string();
    let mtime_ns = mtime_ns_of(&finding.summary.path).await.unwrap_or(0);
    sqlx::query(
        "INSERT INTO finding_index (finding_id, title, status, severity, date, path, mtime_ns)
         VALUES (?, ?, ?, ?, ?, ?, ?)
         ON CONFLICT(finding_id) DO UPDATE SET
            title=excluded.title,
            status=excluded.status,
            severity=excluded.severity,
            date=excluded.date,
            path=excluded.path,
            mtime_ns=excluded.mtime_ns",
    )
    .bind(&finding.summary.finding_id)
    .bind(&finding.summary.title)
    .bind(&finding.summary.status)
    .bind(&finding.summary.severity)
    .bind(&finding.summary.date)
    .bind(&path)
    .bind(mtime_ns)
    .execute(store.pool())
    .await?;
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn parse_minimal_finding() {
        let text = "---\ndoc_kind: finding\nfinding_id: foo-bar\nlast_verified_commit: abc123\ndependencies: [adr:0006]\n---\n\n# Foo bar\n\nbody\n";
        let path = PathBuf::from("/tmp/foo-bar.md");
        let f = parse_finding(&path, text).unwrap();
        assert_eq!(f.finding_id(), "foo-bar");
        assert_eq!(f.title(), "Foo bar");
        assert_eq!(f.status(), "open"); // default
        assert_eq!(f.severity(), "P3"); // default
        assert_eq!(f.date(), "abc123");
        assert_eq!(f.dependencies(), &["adr:0006".to_string()]);
    }

    #[test]
    fn parse_real_finding_file() {
        let workspace_root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .parent()
            .unwrap()
            .to_path_buf();
        let path = workspace_root.join("docs/agent/findings/a1-1-strip-2-noop-at-pin-61f2aff.md");
        let text = std::fs::read_to_string(&path).unwrap();
        let f = parse_finding(&path, &text).unwrap();
        assert_eq!(f.finding_id(), "a1-1-strip-2-noop-at-pin-61f2aff");
        assert_eq!(f.status(), "closed_by_a1.1");
        assert!(!f.title().is_empty());
        assert!(f.dependencies().iter().any(|d| d == "adr:0006"));
    }

    #[test]
    fn extract_title_finds_first_h1() {
        let body = "Some preamble\n# The Title\nmore\n# nested\n";
        assert_eq!(extract_title(body, "fallback"), "The Title");
    }

    #[test]
    fn extract_title_fallback_when_no_h1() {
        let body = "No headings here.\n";
        assert_eq!(extract_title(body, "fallback"), "fallback");
    }
}
