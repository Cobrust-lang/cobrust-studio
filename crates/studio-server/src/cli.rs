//! `cobrust-studio` CLI parser (clap-derive).
//!
//! Wave A3 only ships the `serve` subcommand — `migrate` / `tail-ledger` /
//! `doctor` follow as Studio grows. The struct shape is `Cli { subcommand:
//! Command }` instead of bare flags on the root so future subcommands
//! append without ever breaking the `serve` invocation.

use std::path::PathBuf;

use clap::{Parser, Subcommand};

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
}
