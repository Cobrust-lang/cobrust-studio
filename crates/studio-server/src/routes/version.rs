//! `GET /api/version` — workspace crate versions.
//!
//! Returns the `version()` const each crate exposes (per the convention
//! locked in by the M0 scaffold) plus the workspace `rust-version` pin
//! (sourced from `CARGO_PKG_RUST_VERSION` — the MSRV, NOT the actual
//! `rustc -V` of the build). Wave M2 can wire a `build.rs` capturing
//! full rustc banner if M2 UI needs the binary discriminator. Static
//! — does not read [`crate::AppState`].
//!
//! ```json
//! {
//!   "studio_server": "0.0.1",
//!   "studio_store":  "0.0.1",
//!   "studio_router": "0.0.1",
//!   "rustc": "1.94"
//! }
//! ```
//!
//! Note: the `rustc` field is `"1.94"` not `"rustc 1.94.1 (sha date)"`.
//! Per A3 review F-A3-02 this is intentional for the 5-day MVP — the
//! workspace `rust-version` pin is the M1 contract, full banner is M2+
//! polish.

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde::Serialize;

/// Body shape of `/api/version`.
#[derive(Clone, Debug, Serialize)]
pub struct VersionResponse {
    /// `studio-server` crate version (`env!("CARGO_PKG_VERSION")`).
    pub studio_server: &'static str,
    /// `studio-store` crate version.
    pub studio_store: &'static str,
    /// `studio-router` crate version.
    pub studio_router: &'static str,
    /// Compile-time `rustc --version` string. `"unknown"` if not captured
    /// by the build environment.
    pub rustc: &'static str,
}

impl VersionResponse {
    /// Snapshot the three workspace crates' versions plus the `rustc`
    /// banner. All fields are `'static` — this constructor is `const`.
    #[must_use]
    pub const fn snapshot() -> Self {
        Self {
            studio_server: crate::version(),
            studio_store: studio_store::version(),
            studio_router: studio_router::version(),
            // `RUSTC_VERSION` is not a std env var; we fall back to the
            // host toolchain's `CARGO_PKG_RUST_VERSION` (which is the
            // workspace's `rust-version` pin). Honest provenance: this
            // is the **minimum** rustc, not the actual one. Wave A4 can
            // wire a build-script to capture `rustc -V` if we decide we
            // need the actual version in the response.
            rustc: env!("CARGO_PKG_RUST_VERSION"),
        }
    }
}

/// Handler for `GET /api/version`.
#[allow(clippy::unused_async)] // Axum requires async handlers.
pub async fn version() -> Response {
    (StatusCode::OK, Json(VersionResponse::snapshot())).into_response()
}
