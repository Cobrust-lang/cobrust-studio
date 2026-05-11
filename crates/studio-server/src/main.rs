//! `cobrust-studio` binary entrypoint.
//!
//! Wave A3 wires CLI → tracing init → [`studio_server::serve`]. Run via:
//!
//! ```text
//! cobrust-studio serve --project /path/to/repo --port 7878
//! ```
//!
//! The binary delegates all real work to the library so integration
//! tests can boot the same app without going through `main`.

use std::process::ExitCode;

use clap::Parser;
use studio_server::{Cli, Command, serve};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> ExitCode {
    init_tracing();

    let cli = Cli::parse();
    let result = match &cli.command {
        Command::Serve(args) => serve(args).await,
    };

    if let Err(e) = result {
        tracing::error!(error = %e, "cobrust-studio exited with error");
        eprintln!("error: {e}");
        return ExitCode::from(1);
    }
    ExitCode::SUCCESS
}

/// Initialise `tracing-subscriber` with the env-filter (`RUST_LOG` honoured;
/// defaults to `info`). Idempotency: the binary calls this once at startup;
/// tests don't call it (they use `tokio::test` without tracing).
fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .try_init();
}
