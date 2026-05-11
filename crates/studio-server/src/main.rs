//! `cobrust-studio` binary entrypoint (M0).
//!
//! M1 will wire Axum routes + SSE dispatch. This M0 stub prints a banner
//! and exits 0 — sufficient to validate the 5-gate CI pipeline.

fn main() {
    tracing_subscriber::fmt::init();
    let version = env!("CARGO_PKG_VERSION");
    println!("cobrust-studio {version} (M0 scaffold)");
    println!("M1 routes land Day 2. See CLAUDE.md §6 milestones.");
}
