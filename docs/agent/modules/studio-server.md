---
doc_kind: module
module_id: studio-server
last_verified_commit: 65937d6
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

### Wave A4 (as-built, verified against `crates/studio-server/src/routes/`)

All 10 M1 routes landed; each handler returns
`Result<axum::response::Response, RouteError>` and
[`RouteError::IntoResponse`] renders a JSON `{ error, code }` body.

- `POST /api/auth/set-endpoint`
  → 200 `{ status: "stored" }` on success
  → 400 `{ code: "invalid_body" }` on base64 / empty-scheme failure
  Body: `{ ciphertext: base64, nonce: base64, scheme: string }`.
  Per ADR-0003 the server is a pass-through for opaque AEAD triples.

- `GET /api/project/current`
  → 200 `{ project_root, started_at (rfc3339), version }`.

- `GET /api/adr` → 200 `{ adrs: [AdrSummary, ...] }` (id-ascending).
- `GET /api/adr/:id` → 200 `Adr` or 404 `{ code: "adr_not_found" }`.
- `POST /api/adr` → 201 `Adr` or 400/409. Defaults: status="proposed",
  date=today (UTC); store allocates `adr_id` (`MIN_NEW_ADR_ID..`).

- `GET /api/finding` → 200 `{ findings: [FindingSummary, ...] }`.
- `POST /api/finding` → 201 `Finding` or 400/409. Defaults: severity=
  "P3", status="open", last_verified_commit="HEAD" (the F20 gate then
  refuses to merge until a real SHA is stamped — by design).

- `GET /api/ledger/recent[?n=N]` → 200 `{ entries: [LedgerEntry, ...] }`.
  Default n=20; clamped to `[1, 1000]`. Reads from the SQLite
  materialised view per ADR-0006 §"Addendum 2026-05-11" §F-02.

- `GET /api/events`
  → `text/event-stream` of JSON-bodied state-change events
    (`adr_added | adr_modified | adr_removed | finding_added |
    finding_modified | finding_removed | heartbeat`). 15s
    keep-alive; lagged subscribers (256-event cap per ADR-0006 §F-07)
    skip forward — no Last-Event-ID reconnection in M1.

- `POST /api/dispatch`
  → 503 `{ code: "router_not_configured" }` while `AppState.router`
    is `None` (Wave A4 reality — A5 wires the construction per
    ADR-0006 §"Addendum 2026-05-11" §F-01).
  → SSE `text/event-stream` stub when router becomes `Some(_)`
    (placeholder body in A4; A5 replaces it with the real
    `router.dispatch(req).await` call).

#### Watcher bridge

`serve()` spawns two tokio tasks before binding the listener:

```text
store.adr().watch()      ──► AdrChangeEvent      ─► sse::EventEnvelope::Adr*  ─► EventHub
store.finding().watch()  ──► FindingChangeEvent  ─► sse::EventEnvelope::Find* ─► EventHub
```

Spawned via `pub fn spawn_watcher_bridge(state: &AppState)` so test
harnesses can pre-arm the bridge. Tasks live until the underlying
`notify` watcher closes (process shutdown drops the `Store`).

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

### Wave A4 layout (as-built)

```
crates/studio-server/src/
├── lib.rs               # AppState/serve/spawn_watcher_bridge re-exports
├── main.rs              # clap parse → tracing init → serve()
├── cli.rs               # clap-derive Cli/Command/ServeArgs
├── error.rs             # RouteError enum + IntoResponse (JSON {error,code})
├── state.rs             # AppState { store, router, project_root,
│                        #            started_at, events: EventHub }
├── sse.rs               # EventHub fan-out (broadcast::Sender + 256-cap)
├── app.rs               # build_router(state) — mounts 10 routes + fallback
└── routes/
    ├── mod.rs           # crate-level allow(missing_errors_doc)
    ├── adr.rs           # GET /api/adr (+/:id), POST /api/adr
    ├── auth.rs          # POST /api/auth/set-endpoint
    ├── dispatch.rs      # POST /api/dispatch (SSE, 503 in A4)
    ├── events.rs        # GET /api/events (SSE, watcher-bridge consumer)
    ├── finding.rs       # GET /api/finding, POST /api/finding
    ├── health.rs        # GET /api/health
    ├── ledger.rs        # GET /api/ledger/recent[?n=N]
    ├── project.rs       # GET /api/project/current
    └── version.rs       # GET /api/version
```

### Wave A5+ extensions

- `routes/dispatch.rs` body — replace the 503 placeholder with the
  real `router.dispatch(req).await` call once auth + router
  construction land (per ADR-0006 §"Addendum 2026-05-11" §F-01).
- `embed.rs` — rust-embed for `web/build/` (M3 dogfood).

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
