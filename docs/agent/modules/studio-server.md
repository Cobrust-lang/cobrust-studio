---
doc_kind: module
module_id: studio-server
last_verified_commit: HEAD
dependencies: [adr:0001, adr:0002, adr:0003, adr:0006]
---

# Module: studio-server

## Purpose

Axum-based HTTP layer for Cobrust Studio. Owns the binary entrypoint
(`cobrust-studio serve`), serves embedded SvelteKit web assets (via
rust-embed per ADR-0002 — M3 dogfood), and exposes the REST + SSE API
consumed by the frontend.

## Public surface

### Wave A3 (as-built, verified against `crates/studio-server/src/`)

CLI:
- `cobrust-studio serve --project <path> --port <N> [--host <addr>]`
  — default port `7878`, default host `127.0.0.1`.

HTTP routes (mounted by `build_router(AppState) -> axum::Router`):
- `GET /api/health` → `HealthResponse { status, uptime_seconds, project }`
- `GET /api/version` → `VersionResponse { studio_server, studio_store,
  studio_router, rustc }`
- 404 fallback (any other path) → JSON `{ "error": "route not found",
  "code": "not_found" }`

Library re-exports at crate root:

```rust
pub use studio_server::{
    AppState, ServerError,
    build_router, serve, version,
    Cli, Command, ServeArgs,
    HealthResponse, VersionResponse,
};
```

Middleware stack (applied by `build_router`):
- `tower_http::trace::TraceLayer::new_for_http()` — request spans.
- `tower_http::cors::CorsLayer::permissive()` — M2 SvelteKit dev mode
  on `localhost:5173` calls Studio on `localhost:7878`; M3 embedded
  build flips to same-origin and the permissive layer becomes a no-op.

### Wave A4 target (per ADR-0006 §"Addendum 2026-05-11" + CTO planning)

- `POST /api/auth/set-endpoint` — store encrypted credentials
- `GET /api/project/current` — project metadata
- `GET /api/adr` — list ADRs
- `GET /api/adr/:id` — fetch one ADR
- `POST /api/adr` — create ADR (server-side schema validation)
- `GET /api/finding` — list findings
- `POST /api/finding` — create finding
- `POST /api/dispatch` — SSE stream of LLM dispatch (gated on
  `AppState.router.is_some()`; per ADR-0006 §F-03 `DispatchContext`
  threads `task_tag` and future span / deadline hints into the router)
- `GET /api/ledger/recent` — recent ledger entries
- `GET /api/events` — SSE channel for state-change events

## Internal architecture

### Wave A3 layout (as-built)

```
crates/studio-server/src/
├── lib.rs            # AppState/serve re-exports + ServerError + module roots
├── main.rs           # clap parse → tracing init → serve()
├── cli.rs            # clap-derive Cli/Command/ServeArgs
├── state.rs          # AppState { store, router: Option<Arc<Router>>,
│                     #            project_root, started_at }
├── app.rs            # build_router(state) + 404 JSON fallback
└── routes/
    ├── mod.rs
    ├── health.rs     # GET /api/health
    └── version.rs    # GET /api/version
```

### Wave A4+ extensions

- `routes/adr.rs`, `routes/finding.rs`, `routes/dispatch.rs`,
  `routes/ledger.rs`, `routes/auth.rs`, `routes/events.rs`
- `sse.rs` — fan-out hub for live events (broadcast channel per
  client; bounded buffer per ADR-0006 §F-07).
- `embed.rs` — rust-embed for `web/build/` (M3).

### AppState.router contract

`AppState.router: Option<Arc<studio_router::Router>>`. `None` for A3
because [`studio_router::RouterBuilder::build`] requires every name
in `RouterConfig.providers` to be `register_provider`'d on the
builder, and Wave A3 has no config / credentials in flight. Routes
that need it (`/api/dispatch`) must return `503` with code
`router_not_configured` until A4/A5 wires the construction per
ADR-0006 §"Addendum 2026-05-11" F-01:

```rust
let cfg = RouterConfig::from_toml_str(&toml)?;
let provider: Arc<dyn LlmProvider> = Arc::new(AnthropicProvider::new(/*…*/)?);
let router = RouterBuilder::new()
    .register_provider("anthropic_official", provider)
    .build(&cfg)
    .await?;
```

## Tests

### Wave A3 (collocated `#[cfg(test)] mod tests`, src/lib.rs)

- `version_is_pkg_version` — `version()` const fn returns
  `CARGO_PKG_VERSION`.
- `build_router_smokes_with_real_state` (`#[tokio::test]`) —
  cross-crate probe: `Store::open(tempdir)` → `AppState::new` →
  `build_router` returns successfully. Proves the Axum + Store +
  optional Router type graph compiles and runs end-to-end.
- `uptime_is_monotonic_nondecreasing` (`#[tokio::test]`) — two
  reads of `AppState::uptime_seconds()` never go backwards.

A separate P7 TEST agent on `feature/a3-test-server-smoke` ships
hyper-level smoke tests under `tests/` that exercise `/api/health`
and `/api/version` via a live `tokio::net::TcpListener` on an
ephemeral port. Those tests are not in this branch.

### Wave M1 target

- Integration test per route (start server in tokio test, hit
  endpoint via `reqwest`, assert response shape + status).

## Cross-references

- ADR-0001 (stack — Rust + Axum + tokio)
- ADR-0002 (single-binary — rust-embed lands at M3)
- ADR-0003 (auth — `EncryptedBlob` round-trip lives in
  studio-store::session today; auth route in A4)
- ADR-0006 §"Addendum 2026-05-11" (M1 dispatch contract; AppState
  router shape; F-03 DispatchContext deferred to A4)
- src: `crates/studio-server/`
- depends on: `studio-store`, `studio-router`
