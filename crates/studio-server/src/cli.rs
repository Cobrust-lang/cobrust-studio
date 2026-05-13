//! `cobrust-studio` CLI parser (clap-derive).
//!
//! Wave A3 only ships the `serve` subcommand — `migrate` / `tail-ledger` /
//! `doctor` follow as Studio grows. The struct shape is `Cli { subcommand:
//! Command }` instead of bare flags on the root so future subcommands
//! append without ever breaking the `serve` invocation.
//!
//! ## M6 additions (ADR-0007)
//!
//! Four optional flags on `serve` support headless / test usage without
//! going through the `/login` UI:
//!
//! - `--dev-api-key <KEY>` — bypass `/login`; inject API key directly at boot.
//! - `--dev-endpoint <URL>` — override base URL when using `--dev-api-key`.
//! - `--dev-model <MODEL>` — override model when using `--dev-api-key`.
//! - `--debug-session` — expose `GET /api/session/endpoint` (debug introspection).
//!
//! ## M7 additions (ADR-0008)
//!
//! - `--dev-provider-kind <KIND>` — provider API kind for the `--dev-api-key`
//!   boot-time injection. Defaults to `anthropic` for v0.2.x backward compat.
//!
//! These are explicit opt-ins. The `/login` route is always the canonical
//! primary flow for interactive use; `--dev-api-key` is for CI, Playwright
//! fixtures, and headless scripts (per ADR-0007 §"Env-var path retention").

use std::path::PathBuf;

use clap::{Parser, Subcommand};
use studio_router::ProviderKind;

use crate::persist::PersistBackend;

/// Top-level CLI. Each subcommand is mutually exclusive.
#[derive(Parser, Debug)]
#[command(
    name = "cobrust-studio",
    version,
    about = "Cobrust Studio — AI agent team's project-management control plane",
    long_about = None,
)]
pub struct Cli {
    /// Active subcommand.
    #[command(subcommand)]
    pub command: Command,
}

/// Top-level subcommand selector.
#[derive(Subcommand, Debug)]
pub enum Command {
    /// Start the HTTP server.
    Serve(ServeArgs),
}

/// Arguments for `cobrust-studio serve`.
///
/// `Debug` is hand-written below to redact `dev_api_key` — `clap`
/// propagates auto-`Debug` to the parsed struct, and a panic or future
/// `tracing::debug!("{:?}", args)` would otherwise leak the key to
/// stderr / structured logs (Aleksandr v3 P1).
#[derive(clap::Args)]
pub struct ServeArgs {
    /// Path to the project root the server should manage. Created if
    /// missing (per [`studio_store::Store::open`] semantics — it ensures
    /// `.cobrust-studio/` and `docs/agent/{adr,findings}/` exist).
    #[arg(long, value_name = "PATH")]
    pub project: PathBuf,

    /// Port the HTTP server binds on. Default `7878` matches ADR-0002's
    /// dev-mode proxy target.
    #[arg(long, default_value_t = 7878)]
    pub port: u16,

    /// Bind address. Defaults to loopback so a fresh `serve` invocation
    /// does not silently expose the API to the LAN; flip to `0.0.0.0`
    /// when the operator means it.
    #[arg(long, default_value = "127.0.0.1")]
    pub host: String,

    // --- M6 escape hatches (ADR-0007 §"Env-var path retention") -----------
    /// Bypass `/login` and inject an API key directly at server boot.
    ///
    /// Intended for CI, Playwright fixtures, headless scripts. The key is
    /// stored in-memory (never written to disk). Must be combined with
    /// `--dev-endpoint` and `--dev-model` for a complete credential set.
    ///
    /// When set, the server boots in an already-authenticated state — no
    /// `POST /api/login` call needed.
    #[arg(long, value_name = "KEY", env = "COBRUST_DEV_API_KEY")]
    pub dev_api_key: Option<String>,

    /// Base URL override when using `--dev-api-key`.
    ///
    /// Defaults to `https://api.anthropic.com` if `--dev-api-key` is set
    /// but this flag is omitted.
    #[arg(
        long,
        value_name = "URL",
        default_value = "https://api.anthropic.com",
        env = "COBRUST_DEV_ENDPOINT"
    )]
    pub dev_endpoint: String,

    /// Model override when using `--dev-api-key`.
    ///
    /// Defaults to `claude-opus-4-7` if `--dev-api-key` is set but this
    /// flag is omitted.
    #[arg(
        long,
        value_name = "MODEL",
        default_value = "claude-opus-4-7",
        env = "COBRUST_DEV_MODEL"
    )]
    pub dev_model: String,

    /// Enable `GET /api/session/endpoint` debug route.
    ///
    /// Returns decrypted endpoint + model (never api_key) for E2E test
    /// introspection. Off by default; must be explicitly opted-in.
    #[arg(long)]
    pub debug_session: bool,

    /// Enable ADR-0012 write/exec tools in `/api/agent-turn`.
    #[arg(long, env = "COBRUST_ENABLE_WRITE_TOOLS")]
    pub enable_write_tools: bool,

    /// Provider kind for the `--dev-api-key` boot-time injection path
    /// (M7, ADR-0008). Defaults to `anthropic` for backward compat with
    /// v0.2.x callers that use `--dev-api-key` without specifying a kind.
    ///
    /// Accepted values: `anthropic`, `openai`, `synthetic`.
    #[arg(
        long,
        value_name = "KIND",
        default_value = "anthropic",
        value_parser = parse_dev_provider_kind,
        env = "COBRUST_DEV_PROVIDER_KIND"
    )]
    pub dev_provider_kind: ProviderKind,

    // --- M8 persistent session (ADR-0009) -----------------------------------
    /// Persistent-session backend. When set to `keychain` or `file`, the
    /// passphrase is wrapped at login time so the server auto-unlocks
    /// the in-memory `SessionKey` on the next boot — closes the "re-enter
    /// passphrase on every restart" friction for systemd / Docker / long-
    /// lived server deployments.
    ///
    /// Accepted values:
    /// - `none` (default) — v0.3.0 baseline; in-memory SessionKey drops
    ///   on restart, user re-enters passphrase via /login.
    /// - `keychain` — OS keychain (macOS Keychain / freedesktop secret-
    ///   service / Windows Credential Manager via DPAPI). Strongest
    ///   at-rest posture for cold-disk-theft.
    /// - `file` — `0600` mode plaintext file at the path supplied via
    ///   `--persist-session-file`. Fallback for environments without
    ///   a keychain (Docker, headless Linux without D-Bus). Same trust
    ///   model as `--dev-api-key` (operator-bounded; sysadmin/OS-user-
    ///   equivalent attacker still wins).
    ///
    /// See ADR-0009 §"Threat model (M8 additions)" + `docs/human/{zh,en}/
    /// secret-storage.md` §"Persistent session backends" for the
    /// security-vs-friction trade-off table.
    #[arg(
        long,
        value_name = "MODE",
        default_value = "none",
        value_parser = parse_persist_mode,
        env = "COBRUST_PERSIST_SESSION"
    )]
    pub persist_session: PersistBackend,

    /// File path for `--persist-session=file`. REQUIRED when the mode
    /// is `file`; ignored otherwise (validated at boot — boot fails fast
    /// if mode=file and the path is absent).
    ///
    /// Recommended: `/etc/cobrust-studio/passphrase` for a system-wide
    /// deployment, or `~/.config/cobrust-studio/passphrase` for a per-
    /// user run. The file is created (mode `0600` on Unix) on first
    /// `/api/login`; parent directories are created as needed.
    #[arg(long, value_name = "PATH", env = "COBRUST_PERSIST_SESSION_FILE")]
    pub persist_session_file: Option<PathBuf>,
}

impl std::fmt::Debug for ServeArgs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // `dev_api_key` is the only secret in this struct. Render the
        // presence/absence but never the bytes.
        let dev_key_state = match &self.dev_api_key {
            Some(_) => "Some([REDACTED])",
            None => "None",
        };
        f.debug_struct("ServeArgs")
            .field("project", &self.project)
            .field("port", &self.port)
            .field("host", &self.host)
            .field("dev_api_key", &format_args!("{dev_key_state}"))
            .field("dev_endpoint", &self.dev_endpoint)
            .field("dev_model", &self.dev_model)
            .field("debug_session", &self.debug_session)
            .field("enable_write_tools", &self.enable_write_tools)
            .field("dev_provider_kind", &self.dev_provider_kind)
            .field("persist_session", &self.persist_session)
            .field("persist_session_file", &self.persist_session_file)
            .finish()
    }
}

/// Parse `--dev-provider-kind` string to [`ProviderKind`].
fn parse_dev_provider_kind(s: &str) -> Result<ProviderKind, String> {
    match s {
        "anthropic" => Ok(ProviderKind::Anthropic),
        "openai" => Ok(ProviderKind::Openai),
        "synthetic" => Ok(ProviderKind::Synthetic),
        other => Err(format!(
            "unknown provider kind {other:?}; expected one of: anthropic, openai, synthetic"
        )),
    }
}

/// Parse `--persist-session` string to [`PersistBackend`].
///
/// Case-insensitive + whitespace-trim for ergonomic operator usage
/// (`--persist-session=Keychain` and `--persist-session=" file "` both
/// work). Unknown values produce a clap-rendered error listing the
/// three accepted modes.
///
/// Used by both the CLI parser and the `COBRUST_PERSIST_SESSION` env
/// var path (clap routes both through the same `value_parser`).
fn parse_persist_mode(s: &str) -> Result<PersistBackend, String> {
    match s.trim().to_ascii_lowercase().as_str() {
        "none" => Ok(PersistBackend::None),
        "keychain" => Ok(PersistBackend::Keychain),
        "file" => Ok(PersistBackend::File),
        other => Err(format!(
            "unknown persist mode {other:?}; expected one of: none, keychain, file"
        )),
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn parse_persist_mode_accepts_three_canonical_values() {
        assert_eq!(parse_persist_mode("none").unwrap(), PersistBackend::None);
        assert_eq!(
            parse_persist_mode("keychain").unwrap(),
            PersistBackend::Keychain
        );
        assert_eq!(parse_persist_mode("file").unwrap(), PersistBackend::File);
    }

    #[test]
    fn parse_persist_mode_is_case_insensitive() {
        assert_eq!(parse_persist_mode("NONE").unwrap(), PersistBackend::None);
        assert_eq!(
            parse_persist_mode("KeyChain").unwrap(),
            PersistBackend::Keychain
        );
        assert_eq!(parse_persist_mode("File").unwrap(), PersistBackend::File);
    }

    #[test]
    fn parse_persist_mode_trims_whitespace() {
        assert_eq!(
            parse_persist_mode("  keychain  ").unwrap(),
            PersistBackend::Keychain
        );
        assert_eq!(
            parse_persist_mode("\tfile\n").unwrap(),
            PersistBackend::File
        );
    }

    #[test]
    fn parse_persist_mode_rejects_unknown() {
        let err = parse_persist_mode("tpm").unwrap_err();
        assert!(err.contains("unknown persist mode"), "err={err}");
        assert!(err.contains("none, keychain, file"), "err={err}");
    }

    #[test]
    fn parse_persist_mode_rejects_empty() {
        let err = parse_persist_mode("").unwrap_err();
        assert!(err.contains("unknown persist mode"), "err={err}");
    }
}
