//! Append-only JSONL token ledger.
//!
//! Lifted from `cobrust-llm-router` @ `61f2aff` (v0.1.1) per ADR-0005 /
//! ADR-0006. Strip #1 drops `consensus_group`; strip #4 generalises the
//! Cobrust task-tag (`L0..L3` / `translate` / `repair`) to a caller-supplied
//! free-form `task_tag: Option<String>`.
//!
//! Schema:
//! ```json
//! {
//!   "ts":               "2026-04-30T01:23:45.678Z",
//!   "task_tag":         "agent-turn",
//!   "provider":         "anthropic_official",
//!   "provider_kind":    "anthropic",
//!   "model":            "claude-opus-4-7",
//!   "cache_key":        "blake3:<hex>",
//!   "cache_hit":        false,
//!   "prompt_tokens":    123,
//!   "completion_tokens":456,
//!   "total_tokens":     579,
//!   "latency_ms":       1234,
//!   "attempt":          1,
//!   "outcome":          "ok",
//!   "error_code":       null
//! }
//! ```
//!
//! `provider_kind` is `"anthropic"` | `"openai"` | `"synthetic"`. Pre-lift
//! Cobrust ledger lines that carried `task` (free string) and
//! `consensus_group` are not migrated; Studio starts with a fresh ledger.
//!
//! Writes go through a single `tokio::sync::Mutex<File>` opened with
//! `O_APPEND`, ensuring no two writers tear a line. Each line is terminated
//! by `\n`. Readers must tolerate at most one trailing partial line in case
//! of crash mid-write.

// Upstream copyright: The Cobrust Project. Licensed under Apache-2.0 OR MIT.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tokio::fs::OpenOptions;
use tokio::io::AsyncWriteExt;
use tokio::sync::Mutex;

use crate::config::ProviderKind;
use crate::provider::TokenUsage;

/// Outcome label for a completion attempt.
#[derive(Copy, Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Ok,
    ErrorTransient,
    ErrorPermanent,
}

/// One record in the JSONL ledger.
///
/// `task_tag` is caller-supplied (e.g. `Some("agent-turn")`); pass `None`
/// when no domain tag is appropriate. Strip #4 from ADR-0006 generalised the
/// upstream Cobrust `task: String` enum-key field to this free-form option.
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct LedgerEntry {
    pub ts: String,
    /// Free-form caller tag. `None` when the caller has no domain label to
    /// record. Strip #4 (ADR-0006).
    #[serde(default)]
    pub task_tag: Option<String>,
    pub provider: String,
    /// Wire-protocol kind. Legacy ledger lines without this field deserialise
    /// to `None` thanks to `#[serde(default)]`.
    #[serde(default)]
    pub provider_kind: Option<ProviderKind>,
    pub model: String,
    pub cache_key: String,
    pub cache_hit: bool,
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
    pub latency_ms: u32,
    pub attempt: u8,
    pub outcome: Outcome,
    pub error_code: Option<String>,
}

impl LedgerEntry {
    /// Construct a successful entry from the usage data.
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub fn ok(
        ts: String,
        task_tag: Option<String>,
        provider: impl Into<String>,
        provider_kind: Option<ProviderKind>,
        model: impl Into<String>,
        cache_key: impl Into<String>,
        cache_hit: bool,
        usage: TokenUsage,
        latency_ms: u32,
        attempt: u8,
    ) -> Self {
        Self {
            ts,
            task_tag,
            provider: provider.into(),
            provider_kind,
            model: model.into(),
            cache_key: cache_key.into(),
            cache_hit,
            prompt_tokens: usage.prompt_tokens,
            completion_tokens: usage.completion_tokens,
            total_tokens: usage.total(),
            latency_ms,
            attempt,
            outcome: Outcome::Ok,
            error_code: None,
        }
    }

    /// Construct an error entry. `transient` chooses between
    /// [`Outcome::ErrorTransient`] and [`Outcome::ErrorPermanent`].
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub fn err(
        ts: String,
        task_tag: Option<String>,
        provider: impl Into<String>,
        provider_kind: Option<ProviderKind>,
        model: impl Into<String>,
        cache_key: impl Into<String>,
        latency_ms: u32,
        attempt: u8,
        error_code: impl Into<String>,
        transient: bool,
    ) -> Self {
        Self {
            ts,
            task_tag,
            provider: provider.into(),
            provider_kind,
            model: model.into(),
            cache_key: cache_key.into(),
            cache_hit: false,
            prompt_tokens: 0,
            completion_tokens: 0,
            total_tokens: 0,
            latency_ms,
            attempt,
            outcome: if transient {
                Outcome::ErrorTransient
            } else {
                Outcome::ErrorPermanent
            },
            error_code: Some(error_code.into()),
        }
    }
}

/// Append-only ledger handle. Cheap to clone; all clones share the same
/// underlying mutex so concurrent writers serialise.
#[derive(Clone, Debug)]
pub struct Ledger {
    path: PathBuf,
    file: Arc<Mutex<tokio::fs::File>>,
}

impl Ledger {
    /// Open or create the JSONL file at `path` for append-only writes.
    /// The parent directory is created if missing.
    ///
    /// On Unix the ledger file is created with mode **0600** (owner read/write
    /// only). The ledger records every LLM prompt dispatch including cache
    /// keys derived from prompt content; restricting access prevents other
    /// local users from harvesting usage metadata on shared hosts.
    ///
    /// # Errors
    /// I/O failures bubble up.
    pub async fn open(path: PathBuf) -> std::io::Result<Self> {
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await?;
        }
        #[cfg(unix)]
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .mode(0o600)
            .open(&path)
            .await?;
        #[cfg(not(unix))]
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .await?;
        Ok(Self {
            path,
            file: Arc::new(Mutex::new(file)),
        })
    }

    /// Returns the ledger path.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Append one JSONL record. The line is followed by `\n`.
    ///
    /// # Errors
    /// I/O or JSON serialisation failures bubble up.
    pub async fn append(&self, entry: &LedgerEntry) -> std::io::Result<()> {
        let mut line = serde_json::to_vec(entry)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?;
        line.push(b'\n');
        let mut guard = self.file.lock().await;
        guard.write_all(&line).await?;
        guard.flush().await?;
        Ok(())
    }
}

/// Format the current UTC instant as RFC3339 with millisecond precision.
#[must_use]
pub fn now_rfc3339() -> String {
    let now = time::OffsetDateTime::now_utc();
    // RFC3339 with millisecond precision and `Z` suffix.
    now.format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| "1970-01-01T00:00:00Z".to_string())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::provider::TokenUsage;

    fn make_ok() -> LedgerEntry {
        LedgerEntry::ok(
            "2026-04-30T01:23:45.678Z".to_string(),
            Some("agent-turn".to_string()),
            "anthropic_official",
            Some(ProviderKind::Anthropic),
            "claude-opus-4-7",
            "blake3:abcd",
            false,
            TokenUsage {
                prompt_tokens: 100,
                completion_tokens: 200,
            },
            1234,
            1,
        )
    }

    #[test]
    fn ok_entry_computes_total() {
        let e = make_ok();
        assert_eq!(e.total_tokens, 300);
        assert!(matches!(e.outcome, Outcome::Ok));
    }

    #[test]
    fn err_entry_marks_outcome_correctly() {
        let transient = LedgerEntry::err(
            "ts".into(),
            None,
            "p",
            Some(ProviderKind::Openai),
            "m",
            "blake3:00",
            5,
            1,
            "rate-limit",
            true,
        );
        assert!(matches!(transient.outcome, Outcome::ErrorTransient));
        let permanent = LedgerEntry::err(
            "ts".into(),
            None,
            "p",
            Some(ProviderKind::Openai),
            "m",
            "blake3:00",
            5,
            1,
            "auth",
            false,
        );
        assert!(matches!(permanent.outcome, Outcome::ErrorPermanent));
    }

    #[test]
    fn task_tag_round_trips_when_some() {
        let e = make_ok();
        let json = serde_json::to_string(&e).unwrap();
        assert!(
            json.contains(r#""task_tag":"agent-turn""#),
            "task_tag must serialise as a JSON string: {json}"
        );
        let back: LedgerEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(back.task_tag.as_deref(), Some("agent-turn"));
    }

    #[test]
    fn task_tag_round_trips_when_none() {
        let e = LedgerEntry::ok(
            "ts".into(),
            None,
            "p",
            Some(ProviderKind::Anthropic),
            "m",
            "blake3:00",
            false,
            TokenUsage::default(),
            10,
            1,
        );
        let json = serde_json::to_string(&e).unwrap();
        let back: LedgerEntry = serde_json::from_str(&json).unwrap();
        assert!(back.task_tag.is_none());
    }

    #[tokio::test]
    async fn ledger_appends_jsonl_line_per_entry() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ledger.jsonl");
        let ledger = Ledger::open(path.clone()).await.unwrap();
        ledger.append(&make_ok()).await.unwrap();
        ledger.append(&make_ok()).await.unwrap();
        let bytes = tokio::fs::read(&path).await.unwrap();
        let text = String::from_utf8(bytes).unwrap();
        let lines: Vec<&str> = text.split('\n').filter(|s| !s.is_empty()).collect();
        assert_eq!(lines.len(), 2);
        for line in lines {
            let parsed: LedgerEntry = serde_json::from_str(line).unwrap();
            assert_eq!(parsed, make_ok());
        }
    }

    #[tokio::test]
    async fn ledger_is_append_only_across_reopens() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ledger.jsonl");
        let ledger1 = Ledger::open(path.clone()).await.unwrap();
        ledger1.append(&make_ok()).await.unwrap();
        drop(ledger1);
        let ledger2 = Ledger::open(path.clone()).await.unwrap();
        ledger2.append(&make_ok()).await.unwrap();
        let bytes = tokio::fs::read(&path).await.unwrap();
        let text = String::from_utf8(bytes).unwrap();
        let line_count = text.split('\n').filter(|s| !s.is_empty()).count();
        assert_eq!(line_count, 2, "second open must NOT truncate");
    }

    #[tokio::test]
    async fn ledger_concurrent_writes_do_not_tear_lines() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ledger.jsonl");
        let ledger = Ledger::open(path.clone()).await.unwrap();
        let mut joins = Vec::new();
        for _ in 0..32 {
            let l = ledger.clone();
            joins.push(tokio::spawn(async move {
                for _ in 0..10 {
                    l.append(&make_ok()).await.unwrap();
                }
            }));
        }
        for j in joins {
            j.await.unwrap();
        }
        let bytes = tokio::fs::read(&path).await.unwrap();
        let text = String::from_utf8(bytes).unwrap();
        let lines: Vec<&str> = text.split('\n').filter(|s| !s.is_empty()).collect();
        assert_eq!(lines.len(), 320, "expected 320 well-formed lines");
        for line in lines {
            let _: LedgerEntry = serde_json::from_str(line).expect("every line must be valid JSON");
        }
    }

    #[test]
    fn entry_round_trips_provider_kind_anthropic() {
        let e = make_ok();
        assert_eq!(e.provider_kind, Some(ProviderKind::Anthropic));
        let json = serde_json::to_string(&e).unwrap();
        assert!(
            json.contains(r#""provider_kind":"anthropic""#),
            "must serialise as `anthropic`: {json}"
        );
        let back: LedgerEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(back.provider_kind, Some(ProviderKind::Anthropic));
    }

    #[test]
    fn entry_round_trips_provider_kind_openai() {
        let e = LedgerEntry::ok(
            "ts".into(),
            None,
            "deepseek",
            Some(ProviderKind::Openai),
            "deepseek-v3",
            "blake3:00",
            false,
            TokenUsage::default(),
            10,
            1,
        );
        let json = serde_json::to_string(&e).unwrap();
        assert!(json.contains(r#""provider_kind":"openai""#));
        let back: LedgerEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(back.provider_kind, Some(ProviderKind::Openai));
    }

    #[test]
    fn entry_round_trips_provider_kind_synthetic() {
        let e = LedgerEntry::ok(
            "ts".into(),
            None,
            "synthetic",
            Some(ProviderKind::Synthetic),
            "fixture",
            "blake3:00",
            false,
            TokenUsage::default(),
            0,
            1,
        );
        let json = serde_json::to_string(&e).unwrap();
        assert!(json.contains(r#""provider_kind":"synthetic""#));
        let back: LedgerEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(back.provider_kind, Some(ProviderKind::Synthetic));
    }

    #[test]
    fn now_rfc3339_is_well_formed() {
        let s = now_rfc3339();
        // Must end with Z (UTC) and contain a date-time delimiter.
        assert!(s.ends_with('Z'), "{s}");
        assert!(s.contains('T'), "{s}");
    }

    /// B7: ledger file must be created with mode 0600 on Unix.
    /// The ledger records every LLM prompt dispatch; restricting to owner-only
    /// prevents other local users from reading usage metadata on shared hosts.
    #[cfg(unix)]
    #[tokio::test]
    async fn ledger_file_has_mode_0600() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ledger.jsonl");
        let ledger = Ledger::open(path.clone()).await.unwrap();
        ledger.append(&make_ok()).await.unwrap();
        drop(ledger);
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(
            mode & 0o777,
            0o600,
            "ledger file must be 0600 (owner r/w only); got {:04o}",
            mode & 0o777
        );
    }
}
