---
doc_kind: module
module_id: studio-server
last_verified_commit: abfd1c7
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

Library re-exports at crate root (Wave A5):

```rust
pub use studio_server::{
    AppState, DispatchContext, ServerError,
    SyntheticProvider,
    build_router, serve, version,
    Cli, Command, ServeArgs,
    EventEnvelope, EventHub, SSE_BUFFER_CAP,
    HealthResponse, VersionResponse, RouteError,
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
    finding_modified | finding_removed`). 15s SSE keep-alive comment
    frames (not a typed `heartbeat` event — F-A4-02 reconcile dropped
    the doc claim because the watcher bridge never publishes that
    variant; the M2 frontend should treat the comment frames as raw
    aliveness signals). Lagged subscribers (256-event cap per ADR-0006
    §F-07) skip forward — no Last-Event-ID reconnection in M1.

- `POST /api/dispatch`
  → 503 `{ code: "router_not_configured" }` while `AppState.router`
    is `None` (e.g. project has no `studio.toml`, or the config
    parses but no provider survives credential resolution — see
    Wave A5 router-construction notes below).
  → 400 `{ code: "invalid_body" }` when the JSON body is missing, the
    `model` field is empty, `messages` is empty, or a message carries
    an unknown role (anything other than `system|user|assistant`).
  → SSE `text/event-stream` when router is `Some(_)` and body is
    valid (Wave A5 as-built). Frame sequence:
    - `event: chunk` (≥ 1 frames) — JSON `{ delta: string }` emitted
      as the response text streams. **Today** these are cosmetic
      word-boundary splits of the full router response (deterministic
      via `Router::dispatch_with_tag`); **M2+** plumbs real
      [`studio_router::LlmProvider::complete_stream`] deltas without
      changing the wire shape. Clients must concatenate `delta`
      values verbatim (whitespace preserving).
    - `event: done` (exactly 1 frame, terminal) — JSON
      `{ provider, model, text, usage, cache_hit, task_tag }`. The
      `task_tag` echoes [`DispatchContext::task_tag`] from the
      request body for client-side ledger correlation.
    - `event: error` (terminal, replaces `done` on router failure) —
      JSON `{ error, code }` with refined codes (`router_auth |
      router_rate_limit | router_bad_request | router_transport |
      router_server | router_failed | router_no_provider |
      router_config | router_io`).

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

### Wave A5 layout (as-built)

```
crates/studio-server/src/
├── lib.rs               # AppState/serve/spawn_watcher_bridge re-exports
│                        # + DispatchContext + SyntheticProvider re-exports
├── main.rs              # clap parse → tracing init → serve()
├── cli.rs               # clap-derive Cli/Command/ServeArgs
├── error.rs             # RouteError enum + IntoResponse (JSON {error,code})
├── state.rs             # AppState { store, router, project_root,
│                        #            started_at, events: EventHub }
│                        # + DispatchContext { task_tag } newtype
├── sse.rs               # EventHub fan-out (broadcast::Sender + 256-cap)
├── app.rs               # build_router(state) — mounts 10 routes + fallback
├── synthetic.rs         # SyntheticProvider — in-process LlmProvider impl
│                        # (deterministic Chunk stream; test/dev scaffolding)
├── router_init.rs       # try_build_router_from_project (studio.toml
│                        # → Option<Arc<Router>>; soft-fail to None)
└── routes/
    ├── mod.rs
    ├── adr.rs           # GET /api/adr (+/:id), POST /api/adr
    ├── auth.rs          # POST /api/auth/set-endpoint
    ├── dispatch.rs      # POST /api/dispatch (SSE done|error; A5 wired)
    ├── events.rs        # GET /api/events (SSE, watcher-bridge consumer)
    ├── finding.rs       # GET /api/finding, POST /api/finding
    ├── health.rs        # GET /api/health
    ├── ledger.rs        # GET /api/ledger/recent[?n=N]
    ├── project.rs       # GET /api/project/current
    └── version.rs       # GET /api/version
```

### Wave A6+ extensions

- Per-`Chunk` SSE streaming on `/api/dispatch` (requires plumbing
  `LlmProvider::complete_stream` through `studio_router::Router`).
- `embed.rs` — rust-embed for `web/build/` (M3 dogfood).
- M2 auth flow — real AEAD decryption replaces the A5 raw-bytes
  EncryptedBlob stub in `router_init::resolve_api_key`.

### AppState.router contract

`AppState.router: Option<Arc<studio_router::Router>>`. Wave A3 always
left this `None`. Wave A5 populates it via
[`crate::router_init::try_build_router_from_project`] which:

1. Reads `<project_root>/studio.toml` (primary) or
   `cobrust-studio.toml` (alternate). Missing config → `None`.
2. Calls `RouterConfig::from_toml_str(&toml)?`.
3. For each `[providers.<name>]` block, constructs a
   `Arc<dyn LlmProvider>`:
   - `kind = "synthetic"` →
     `studio_server::SyntheticProvider::new(name)` (no creds).
   - `kind = "anthropic"` → `AnthropicProvider::new` with the API
     key resolved as `env::var(api_key_env)` first, then the
     session-blob ciphertext bytes (A5 stub; real AEAD round-trip
     is M2).
   - `kind = "openai"` → `OpenAiProvider::new`, same credential
     order.
4. `RouterBuilder::build(&cfg).await` — failure (e.g. preferred
   list references an unregistered provider) → `None` with a
   `tracing::warn!`.

The fallback to `None` is intentional: the dispatch route's
`503 router_not_configured` shape is the M2 frontend's "router not
configured yet" banner trigger; A5 must not break that UX when a
project has no `studio.toml`. Per ADR-0006 §"Addendum 2026-05-11"
F-01 the underlying call:

```rust
let cfg = RouterConfig::from_toml_str(&toml)?;
let provider: Arc<dyn LlmProvider> = Arc::new(AnthropicProvider::new(/*…*/)?);
let router = RouterBuilder::new()
    .register_provider("anthropic_official", provider)
    .build(&cfg)
    .await?;
```

### DispatchContext (Wave A5)

Per ADR-0006 §"Addendum 2026-05-11" §F-03 the CTO chose option (c) —
a caller-supplied newtype threaded alongside `CompletionRequest` —
over (a) bloating `CompletionRequest` or (b) overloading
`Router::dispatch`. The struct is intentionally tiny so future
fields (span IDs, deadline hints) can land without a wire-format
break:

```rust
pub struct DispatchContext {
    pub task_tag: Option<String>,
    // Future: deadline_ms, parent_span_id, ...
}
```

`Router::dispatch` ignores caller tags today
(`task_tag_for_request` returns `None`); the Wave-A5 dispatch route
records the tag in its server-side SSE `done` payload so clients can
correlate. Router-internal ledger plumbing is post-M1.

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

### Wave A5 additions

Collocated `#[cfg(test)]` in `src/`:

- `synthetic.rs` × 4 — fixture text, deterministic 4-delta+1-done
  stream, name round-trip, `ProviderKind::Synthetic` reporting.
- `router_init.rs` × 5 — `Ok(None)` paths (no config, malformed TOML,
  missing creds), `Ok(Some)` paths (synthetic-only studio.toml,
  alternate filename `cobrust-studio.toml`).
- `routes/dispatch.rs` × 7 — role parsing, body validation rejects
  (empty model, empty messages, unknown role), `task_tag` threading
  via `DispatchContext`, `RouterError::AllFailed` code refinement.

Integration corpus at `tests/dispatch_synthetic_route.rs` × 4 — boots
an in-process `Router` from a synthesised `studio.toml` (writing
`ledger.jsonl` + `llm_cache` under the test's `TempDir` so parallel
runs do not collide), then drives `POST /api/dispatch` through the
existing `tower::ServiceExt::oneshot` harness:

- `dispatch_returns_200_sse_when_router_configured` — content-type
  is `text/event-stream`, body contains `event: done` and the
  registered provider name and the synthetic fixture text.
- `dispatch_echoes_task_tag_in_done_payload` — caller-supplied
  `task_tag` round-trips into the SSE payload (DispatchContext F-03).
- `dispatch_returns_400_for_empty_messages_when_router_configured`.
- `dispatch_returns_400_for_unknown_role_when_router_configured`.

The pre-existing `tests/dispatch_route.rs` × 4 continues to pass —
the 503 path is unchanged.

### Wave M1 target

- Integration test per route (start server in tokio test, hit
  endpoint via `reqwest`, assert response shape + status). 5
  route-test corpora (`adr_routes`, `auth_route`, `events_route`,
  `finding_routes`, `ledger_route`) carry pre-existing failures on
  the A5 base branch (`6775cce`) that are unrelated to dispatch
  wiring — tracked separately, not blocked on Wave A5.

## Cross-references

- ADR-0001 (stack — Rust + Axum + tokio)
- ADR-0002 (single-binary — rust-embed lands at M3)
- ADR-0003 (auth — `EncryptedBlob` round-trip lives in
  studio-store::session today; auth route in A4; A5 stub treats the
  blob ciphertext as raw bytes — real AEAD decryption is M2)
- ADR-0006 §"Addendum 2026-05-11" (M1 dispatch contract; AppState
  router shape; F-03 DispatchContext landed at A5)
- src: `crates/studio-server/`
- depends on: `studio-store`, `studio-router`
