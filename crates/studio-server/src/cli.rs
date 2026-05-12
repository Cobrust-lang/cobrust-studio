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
#[derive(clap::Args, Debug)]
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
