//! Bakes `web/build/` static assets into the release binary per
//! ADR-0002 single-binary deployment.
//!
//! # Modes
//!
//! - **Release builds** (after `pnpm --dir web build`): every file under
//!   `web/build/` is embedded by [`WebAssets`]. The handlers serve from
//!   memory, no filesystem lookup, no copy.
//! - **Dev / scaffold builds** (no `web/build/` populated yet, or
//!   `pnpm dev` proxying separately): [`WebAssets`] resolves zero files,
//!   so [`serve_path`] falls through to a static HTML stub that tells the
//!   operator what to do.
//!
//! # SPA fallback
//!
//! SvelteKit client-side routing means a request to `/adr/3` must return
//! the same `index.html` shell that `/` does (the JS router then takes
//! over). The [`serve_asset`] handler:
//!
//! 1. Strips the leading `/`.
//! 2. If [`WebAssets::get`] resolves the literal path, serves it.
//! 3. Otherwise serves `index.html` (or the dev stub if even that is
//!    missing).
//!
//! `/api/*` paths never enter this fallback because the router mounts
//! them as nested routers above the [`Router::fallback`] hook in
//! [`crate::app::build_router`].

use axum::http::{StatusCode, Uri, header};
use axum::response::{IntoResponse, Response};
use rust_embed::RustEmbed;

/// Compile-time embed of `web/build/` (SvelteKit static export).
///
/// The folder is resolved relative to this crate's `CARGO_MANIFEST_DIR`
/// (`crates/studio-server/`), so the `../../web/build/` path walks back
/// out to the workspace root. Per ADR-0002 the workspace build script
/// (`scripts/build-release.sh`) runs `pnpm --dir web build` before
/// `cargo build --release`, populating that folder. If the folder is
/// missing or empty, [`WebAssets::get`] returns `None` for every key
/// and the dev-stub fallback in [`serve_path`] takes over — `cargo
/// build` in dev mode therefore never fails on a missing
/// `web/build/`.
#[derive(RustEmbed)]
#[folder = "../../web/build/"]
#[allow_missing = true]
pub struct WebAssets;

/// Axum handler bound to [`axum::Router::fallback`]: serves the
/// requested path from the embedded set, with SPA `index.html`
/// fallback for any non-asset path.
///
/// Note the route mounting (see [`crate::app::build_router`]) places
/// every `/api/*` nested router *before* this fallback, so this
/// handler only sees web-asset paths (root, `/adr/3`, `/agent`, …) and
/// the SvelteKit-generated asset paths under `_app/`.
///
/// Uses [`axum::http::Uri`] extractor (the request's raw URI) rather
/// than [`axum::extract::Path`] — `Path<T>` extracts named parameters
/// from a matched route pattern, and `Router::fallback` has no pattern
/// to match against. Past M4 release-readiness audit caught this as
/// an F19 regression: pre-fix, every SPA route (`/login`, `/adr`,
/// `/agent`, …) returned the Axum "Wrong number of path arguments"
/// error string instead of `index.html`.
pub async fn serve_asset(uri: Uri) -> Response {
    serve_path(uri.path())
}

/// Axum handler bound to `GET /`: serves the SPA shell
/// (`index.html`) directly, no path extraction. Splitting this from
/// [`serve_asset`] avoids the empty-path edge case that
/// [`axum::extract::Path`] does not handle for `GET /`.
#[allow(clippy::unused_async)] // Axum requires async handlers.
pub async fn serve_index() -> Response {
    serve_path("index.html")
}

/// Inner resolver: looks up `path` in [`WebAssets`], falls back to
/// `index.html` for SPA client-side routing, finally falls back to a
/// static dev-mode HTML stub if even `index.html` is not embedded
/// (i.e. `web/build/` was never populated).
fn serve_path(path: &str) -> Response {
    // Trim the leading slash so the embed key matches the rust-embed
    // convention (paths are relative to the embedded folder root).
    let path = path.trim_start_matches('/');

    // SPA fallback: if the literal path doesn't resolve to an embedded
    // file, try `index.html` instead. SvelteKit's client-side router
    // takes the URL from there.
    let candidate = if WebAssets::get(path).is_some() {
        path
    } else {
        "index.html"
    };

    if let Some(file) = WebAssets::get(candidate) {
        // rust-embed's `mime-guess` feature resolves the MIME at embed
        // time and stamps it onto `file.metadata.mimetype()`. We surface
        // that string directly — no per-request `mime_guess::from_path`
        // call needed, no second workspace dep.
        let mime = file.metadata.mimetype().to_owned();
        return (
            StatusCode::OK,
            [(header::CONTENT_TYPE, mime)],
            file.data.into_owned(),
        )
            .into_response();
    }

    // Dev / scaffold fallback: `web/build/` is empty so even
    // `index.html` is missing. Return a static HTML stub explaining
    // how to populate the embed.
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
        DEV_STUB_HTML.as_bytes().to_vec(),
    )
        .into_response()
}

/// Static HTML stub served when `web/build/` is empty. Kept here as a
/// `const` so the binary always carries enough to render a useful
/// landing page even without the SvelteKit bundle — `cobrust-studio
/// serve` on a fresh checkout still returns something coherent.
const DEV_STUB_HTML: &str = r#"<!doctype html>
<html lang="en">
<head><meta charset="utf-8"><title>Cobrust Studio (dev mode)</title></head>
<body style="font-family: system-ui, -apple-system, sans-serif; max-width: 720px; margin: 4em auto; padding: 0 1em;">
<h1>Cobrust Studio (dev mode)</h1>
<p>The embedded <code>web/build/</code> bundle is empty, so this is a
fallback page. The HTTP API is live on <code>/api/*</code> and the
SvelteKit static export was not bundled into this binary.</p>
<h2>Populate the bundle (release build)</h2>
<pre>bash scripts/build-release.sh</pre>
<p>This script runs <code>pnpm --dir web build</code> and then
<code>cargo build --release --workspace --locked</code>.</p>
<h2>Run the dev frontend separately</h2>
<pre>pnpm --dir web dev   # in another terminal — proxies to :7878</pre>
<p>See <code>docs/agent/adr/0002-single-binary-deployment.md</code> for
the design rationale.</p>
</body></html>"#;

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use axum::body::to_bytes;

    #[tokio::test]
    async fn serve_index_returns_200_and_html_content_type() {
        let resp = serve_index().await;
        assert_eq!(resp.status(), StatusCode::OK);
        let ct = resp
            .headers()
            .get(header::CONTENT_TYPE)
            .expect("content-type header")
            .to_str()
            .expect("ascii content-type")
            .to_owned();
        // Either the real index.html (text/html) or the dev stub
        // (text/html; charset=utf-8). Both start with "text/html".
        assert!(
            ct.starts_with("text/html"),
            "content-type should be html-ish, got {ct}"
        );
        let body = to_bytes(resp.into_body(), 64 * 1024)
            .await
            .expect("read body");
        assert!(!body.is_empty(), "index/dev-stub body must be non-empty");
    }

    #[tokio::test]
    async fn serve_asset_falls_back_to_index_for_unknown_path() {
        // SvelteKit client-side route. Pre-M4.2 fix this used
        // `Path<String>` extractor which fails on fallback handlers —
        // the F19 audit caught the regression. Post-fix, the `Uri`
        // extractor takes any URI and serves index.html for SPA
        // routes. Status is 200 either way (real index.html in
        // release; dev stub if web/build/ unpopulated).
        let uri: Uri = "/adr/3".parse().expect("valid uri");
        let resp = serve_asset(uri).await;
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn serve_asset_handles_spa_routes_login_agent_etc() {
        // The exact set the M3 review forecast "should pass" but M4
        // release-readiness audit caught as 13/14 Playwright fails.
        // Lock the regression here so a future change to the fallback
        // handler can't reintroduce the bug.
        for path in ["/login", "/adr", "/agent", "/finding", "/ledger"] {
            let uri: Uri = path.parse().expect("valid uri");
            let resp = serve_asset(uri.clone()).await;
            assert_eq!(
                resp.status(),
                StatusCode::OK,
                "SPA route {path:?} should return 200 (index.html shell), not 404 or 5xx",
            );
        }
    }

    #[test]
    fn dev_stub_mentions_build_script() {
        // Cheap doc-test: the stub must guide the operator to the
        // canonical build path so a fresh checkout never strands them.
        assert!(DEV_STUB_HTML.contains("scripts/build-release.sh"));
    }
}
