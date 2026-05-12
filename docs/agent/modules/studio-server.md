---
doc_kind: module
module_id: studio-server
last_verified_commit: 45ad600
dependencies: [adr:0001, adr:0002, adr:0003, adr:0006, adr:0007, adr:0008, adr:0009, adr:0010, adr:0013]
---

# Module: studio-server

## Purpose

Axum-based HTTP layer for Cobrust Studio. Owns the headless/server
binary entrypoint (`cobrust-studio serve`), serves embedded SvelteKit web
assets for compatibility mode (via rust-embed per ADR-0002 — M3
dogfood), and exposes the REST + SSE API consumed by both the browser
frontend and the desktop-first Tauri shell (ADR-0013).

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
  → 400 `{ code: "task_tag_too_long" }` when optional `task_tag` is over
    256 bytes.
  → 400 `{ code: "task_tag_invalid_chars" }` when optional `task_tag`
    contains control characters. Empty string normalises to `None`.
  → SSE `text/event-stream` when router is `Some(_)` and body is
    valid (Wave A5 as-built). Frame sequence:
    - `event: chunk` (≥ 1 frames) — JSON `{ delta: string }` emitted
      as the response text streams. **Today** these are cosmetic
      word-boundary splits of the full router response (deterministic
      via `Router::dispatch_ctx`); **M2+** plumbs real
      [`studio_router::LlmProvider::complete_stream`] deltas without
      changing the wire shape. Clients must concatenate `delta`
      values verbatim (whitespace preserving).
    - `event: done` (exactly 1 frame, terminal) — JSON
      `{ provider, model, text, usage, cache_hit, task_tag }`. The
      `task_tag` echoes [`studio_router::DispatchContext::task_tag`]
      from the request body and the same value is written to the router
      JSONL ledger for cost-by-task analysis.
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

### Wave M3 (as-built, verified against `crates/studio-server/src/`)

Single-binary deployment per ADR-0002. The release binary bakes the
SvelteKit static export under `web/build/` via `rust-embed`, served
from memory by Axum so the compatibility/headless user journey
"download tarball, run binary, open browser" requires no static-file
path, no nginx, no volume mount.

ADR-0013 changes the primary product narrative after M3: desktop users
should open a Tauri app, not manually manage a browser tab. The M3
embedded-web path remains supported as `cobrust-studio serve` for CI,
dogfood automation, remote/server deployments, and any workflow where a
browser-first server is intentional.

New file `crates/studio-server/src/embed.rs`:

```text
WebAssets struct        — #[derive(RustEmbed)] folder = "../../web/build/",
                          allow_missing = true (so `cargo build` in a fresh
                          checkout without pnpm-build does not fail).
serve_index             — Axum handler bound to `GET /`; serves
                          index.html (or DEV_STUB_HTML if web/build/ is
                          empty).
serve_asset(Path)       — Axum handler bound as `Router::fallback`; serves
                          the embedded path with SPA fallback to
                          index.html, then dev-stub fallback if missing.
DEV_STUB_HTML           — static HTML const explaining how to populate
                          the embed (links to scripts/build-release.sh
                          and `pnpm --dir web dev`).
```

MIME types come from rust-embed's stamped `EmbeddedFile.metadata
.mimetype()` — the `mime-guess` crate feature pre-resolves at embed
time, so the handler does not need a direct `mime_guess` dep.

Wave-A4's flat `build_router` with a single JSON 404 fallback is
refactored. `app.rs` now ships:

```text
fn api_router() -> Router<AppState>   — /api/* surface only, with
                                        the JSON-404 fallback
                                        (`api_not_found`).
fn build_router(state) -> Router      — nests api_router under /api,
                                        binds GET / to embed::serve_index,
                                        sets Router::fallback to
                                        embed::serve_asset, then layers
                                        TraceLayer + CorsLayer::permissive.
```

This preserves the M2 frontend contract that `/api/typo` returns JSON
`{"error":"route not found","code":"not_found"}` while every non-`/api`
path returns the SPA shell (or the dev stub).

Build-pipeline integration via `scripts/build-release.sh`:

```text
pnpm install --frozen-lockfile      (in web/)
pnpm run build                      (SvelteKit → web/build/)
cargo build --release --workspace --locked
```

Output: `target/release/cobrust-studio`, ~9 MiB on darwin-arm64 with
the M3 ADR/finding bundle baked in.

Dogfood smoke via `scripts/smoke-dogfood.sh`:

```text
boot:  $BIN serve --project $REPO --port $PORT
assert: GET /api/health  → status == "ok"
assert: GET /api/adr     → .adrs | length >= 6
assert: GET /            → body starts with HTML
cleanup: trap-kill on exit
```

Verified locally (commit 3b56d0e): release build produces the binary,
smoke run with PORT=37878 PASSes — Studio sees its own 6 constitutional
ADRs (0001..0006) via the same HTTP surface the M2 frontend uses.

### Wave M9T (as-built — ADR-0013 Tauri desktop runtime)

Tauri becomes the primary desktop shell while this crate remains the
HTTP/SSE backend boundary. The route surface is unchanged: the desktop
app starts an embedded Studio server bound to `127.0.0.1:0`, discovers
the resolved port, and opens the WebView at that loopback origin. Because
the WebView loads the same HTTP origin as the API, existing frontend
relative `/api/*` calls keep working without a custom protocol rewrite.

New public server helper in `crates/studio-server/src/lib.rs`:

```text
serve_embedded(args: &ServeArgs) -> EmbeddedServer
  - reuses the same store/router/persist/dev-key boot path as serve()
  - enforces loopback bind for embedded mode
  - returns base_url() with the OS-chosen ephemeral port
  - shutdown() sends graceful shutdown and waits for the serve task
```

`EmbeddedServer::drop` also sends shutdown and aborts the task, so a
Tauri app exit does not leave the loopback listener running.

M9T preserves:

- `cobrust-studio serve --project <path> --port <N> [--host <addr>]` as
  the headless/server entrypoint.
- `build_router(AppState)`, watcher bridge, M8 `auto_unlock_on_boot`, and
  `--dev-api-key` injection as single shared boot logic.
- The REST/SSE contract used by `web/src/lib/api.ts` until ADR-0012
  stabilises tool-call semantics.

New smoke gate:

```text
crates/studio-server/tests/embedded_server.rs
  embedded_server_binds_ephemeral_loopback_and_serves_login
    - starts serve_embedded() with port 0
    - asserts loopback + non-zero port
    - GETs <base_url>/login and expects 200
    - shuts the embedded server down
```

### Wave M8 (as-built — ADR-0009 persistent session across restart)

M8 closes Sarah v3/v4 audit Gate B (persistent session across binary
restart). The in-memory `SessionKey` now optionally survives a restart
by wrapping the user's passphrase in an OS-keychain entry or a `0600`
plaintext file. Closes the README §"Looking for 3-5 design partners"
item 4 ("Persistent session across binary restart").

New module `crates/studio-server/src/persist.rs` (~470 LOC, 12 unit tests):

```text
PersistBackend                 — enum { None, Keychain, File }; clap
                                 value_parser (case-insensitive trim)
PersistError                   — thiserror; #[non_exhaustive]
  Keychain(String)             — OS keychain access denied / unavailable
  File { path, source }        — I/O failure on the file path
  InsecurePermissions { mode } — Unix-only; rejects non-0600 mode
  NotConfigured                — mode=File without a path

PersistStore (trait)           — Send + Sync; object-safe
  .save(passphrase: &str)
  .load() -> Option<Zeroizing<String>>
  .clear()

NullStore                      — PersistBackend::None; all ops are noops
KeychainStore                  — PersistBackend::Keychain; keyring crate
  service = "cobrust-studio"
  username = "session-passphrase"
FileStore { path }             — PersistBackend::File; 0600 mode on Unix
  .save: O_CREAT|O_TRUNC|O_WRONLY mode 0600 + write + sync + chmod 0600
  .load: rejects mode != 0600 → InsecurePermissions
  .clear: remove_file (idempotent on missing)

build_store(backend, path) -> Result<Box<dyn PersistStore>, PersistError>
```

Boot-flow integration in `studio_server::serve`:

```text
on_serve_start:
  persist_arc = persist::build_store(args.persist_session, args.persist_session_file)?
  state = AppState::with_persist(store, router, project_root, persist_arc)
  state.debug_session = args.debug_session
  // M6 --dev-api-key path stays unchanged
  // M8 auto-unlock
  studio_server::auto_unlock_on_boot(&state).await
  // 1. persist.load() → passphrase (or None → return)
  // 2. read session_kv blob (or auto-clear orphan + return)
  // 3. derive(passphrase, blob[..16]) → candidate key
  // 4. VERIFY key.open(blob.ciphertext) — auto-clear on mismatch (M6 lesson)
  // 5. stash verified key into state.session_key
```

Login + logout integration:

| Method | Path | M8 addition |
|--------|------|-------------|
| POST | `/api/login` | On successful seal+store: also `state.persist.save(passphrase)`. NullStore = noop. Failures logged, never escalate. |
| POST | `/api/logout` | Unchanged for default; `?purge=true` ALSO calls `state.persist.clear()`. |

CLI additions (`ServeArgs`):

```text
--persist-session <MODE>         # none | keychain | file (default: none)
                                 # env: COBRUST_PERSIST_SESSION
--persist-session-file <PATH>    # required when MODE=file
                                 # env: COBRUST_PERSIST_SESSION_FILE
```

New `AppState` field:

```rust
pub persist: Arc<dyn PersistStore + Send + Sync>,
```

Constructor split — `AppState::new(store, router, root)` defaults to
`Arc::new(NullStore)`; M8 callers use `AppState::with_persist(store,
router, root, persist)`.

New crate-root public symbol — `pub async fn auto_unlock_on_boot(state:
&AppState)`. Exposed for the integration test corpus to drive the
same path `serve()` walks (F1.5 deep-source-read discipline; the M6
seal-salt-mismatch lesson taught us to test the boot path, not just
a same-instance round-trip).

Wave M8 layout addition:

```
crates/studio-server/src/
├── persist.rs           # NEW — PersistBackend / PersistError / PersistStore
│                        # + NullStore / KeychainStore / FileStore + build_store
├── lib.rs               # + auto_unlock_on_boot(state) public function
├── state.rs             # + AppState.persist field + with_persist() constructor
├── cli.rs               # + --persist-session, --persist-session-file flags
└── routes/
    └── login.rs         # + state.persist.save() mirror on success
                         # + LogoutQuery { purge: bool } + state.persist.clear()
```

Integration tests (`tests/persistent_session.rs`, 7 tests + 1 #[ignore]'d):

- `file_persist_path_survives_restart` — primary regression gate
  (POST /api/login with file backend → simulate restart with fresh
  AppState pointing at SAME store + SAME file → assert auto-unlock).
- `none_persist_path_drops_key_on_restart` — v0.3.0 baseline preserved.
- `logout_purge_clears_file_persist` — `?purge=true` deletes the file
  + restart cannot auto-unlock.
- `regular_logout_preserves_file_persist_for_next_restart` — default
  logout drops in-memory key but keeps persist (re-login by restart).
- `wrong_persist_passphrase_invalidates_and_clears` — stale persist
  (passphrase mismatches blob) → boot-unlock auto-clears.
- `orphaned_persist_with_no_blob_auto_clears` — persist has
  passphrase but session_kv has no blob → boot-unlock auto-clears.
- `keychain_path_survives_restart` (#[ignore]) — same as file path
  but via OS keychain. Run locally with `--ignored --nocapture`;
  CI runners may not have a usable keychain (ADR-0009 §"Phase 2
  caveats").

Workspace deps added (per ADR-0009):

```toml
keyring = { version = "3", default-features = false, features = [
    "apple-native",         # macOS Security.framework
    "windows-native",       # Windows Credential Manager (DPAPI)
    "sync-secret-service",  # Linux org.freedesktop.secrets D-Bus
    "crypto-rust",          # pure-Rust AES-GCM for D-Bus transport
] }
zeroize = { version = "1.8", features = ["std", "zeroize_derive"] }
```

The `Zeroizing<String>` wrapper on the load path wipes the passphrase
heap allocation on drop (Aleksandr v3 P2 memory-hygiene mitigation
extended into M8 — ADR-0007 says memory dump is out of scope, but
M8 explicitly raises the bar since the new code paths handle the raw
passphrase string outside the brief login-handler scope).

### Wave M7 (as-built — ADR-0008 multi-provider /login)

M7 closes Sarah v3 audit finding #3 (multi-provider /login is a v0.3.x blocker).
The session-driven dispatch path now supports both `AnthropicProvider` and
`OpenAiProvider` (vLLM / DeepSeek / Together / OpenRouter / Groq / Ollama) via
an explicit `provider_kind` field on `LoginRequest` and `EndpointSecret`.

Changes per ADR-0008:

**`crates/studio-router/src/config.rs`**:
- `ProviderKind` gains `#[default]` (= `Anthropic`) so `#[serde(default)]` on
  `EndpointSecret::provider_kind` yields `Anthropic` for pre-M7 blobs.

**`crates/studio-server/src/secret.rs`**:
- `EndpointSecret` gains `#[serde(default)] pub provider_kind: ProviderKind`.
  Pre-M7 blobs (no field in ciphertext JSON) deserialize to `Anthropic`.
- `pub use studio_router::ProviderKind` re-exported from the crate root.
- `SessionKey::seal_raw(&[u8]) -> Result<Vec<u8>, SecretError>` added for
  integration tests that construct pre-M7 payloads by hand.

**`crates/studio-server/src/routes/login.rs`**:
- `LoginRequest` gains `#[serde(default)] pub provider_kind: ProviderKind`.
  `Synthetic` is rejected with `400 { code: "invalid_provider_kind" }`.
  `provider_kind` is plumbed into the constructed `EndpointSecret`.

**`crates/studio-server/src/routes/dispatch.rs`**:
- `build_session_provider(&EndpointSecret) → Result<(Arc<dyn LlmProvider>,
  &'static str), Response>` helper extracted.
- `resolve_router()` calls `build_session_provider` instead of hardcoding
  `AnthropicProvider::new`. Match arms: `Anthropic` → `AnthropicProvider`,
  `Openai` → `OpenAiProvider`, `Synthetic` → 503 defense-in-depth.

**`crates/studio-server/src/cli.rs`**:
- `ServeArgs` gains `--dev-provider-kind <KIND>` (default `anthropic`,
  env `COBRUST_DEV_PROVIDER_KIND`).

**`crates/studio-server/src/lib.rs`**:
- `serve()` plumbs `args.dev_provider_kind` into the `--dev-api-key`
  boot-time `EndpointSecret`.

Integration tests (`tests/multi_provider_login.rs`, 6 tests):
- `login_anthropic_then_dispatch` — wiremock Anthropic stub → 200.
- `login_openai_then_dispatch` — wiremock OpenAI stub → 200.
- `login_synthetic_returns_400` — 400 + code `invalid_provider_kind`.
- `login_missing_provider_kind_defaults_anthropic` — back-compat 200.
- `re_login_changes_provider_kind` — same passphrase, different kind, both 200.
- `existing_blob_decryption_supplies_kind` — pre-M7 JSON blob opens as Anthropic.

```text
crates/studio-server/src/
├── secret.rs            # + provider_kind: ProviderKind field + seal_raw()
│                        # + pub use studio_router::ProviderKind
└── routes/
    ├── login.rs         # + provider_kind: ProviderKind + Synthetic rejection
    └── dispatch.rs      # + build_session_provider() + match arm on kind
```

### Wave M6 (as-built — ADR-0007 AEAD round-trip)

M6 closes the open AEAD round-trip stub that shipped in v0.1.x. The
server now derives an AES-256 key from the user's passphrase via
Argon2id and seals `(endpoint, api_key, model)` in AES-256-GCM before
writing to `session_kv`. The in-memory key enables per-dispatch
decryption without a second passphrase entry.

New module `crates/studio-server/src/secret.rs`:

```text
SessionKey([u8; 32])
  .derive(passphrase, salt)  — Argon2id m=64MiB t=3 p=1 → 32B key
  .seal(EndpointSecret)      — AES-256-GCM → packed blob
  .open(&[u8])               — decrypt + deserialise → EndpointSecret

EndpointSecret { endpoint, api_key, model }
SecretError   { Kdf, Seal, Open, Malformed, UnknownScheme }

SCHEME = "aes-gcm-256/argon2id-v1"
Wire format: salt(16) || nonce(12) || ciphertext+tag
```

New AppState fields:

```rust
pub session_key: Arc<tokio::sync::RwLock<Option<SessionKey>>>,
pub debug_session: bool,
```

New routes (merged under `/api` in `app.rs`):

| Method | Path | Behaviour |
|--------|------|-----------|
| POST | `/api/login` | Derive key, seal secret, write `session_kv`, store key in `AppState`. |
| POST | `/api/logout` | Drop `session_key`. |
| GET | `/api/session/status` | `{ authenticated: bool }`. |
| GET | `/api/session/endpoint` | Debug-only (`--debug-session`); decrypted endpoint+model, never api_key. |

Dispatch integration: `resolve_router()` in `routes/dispatch.rs` checks
`session_key` first; if present, decrypts blob and builds per-request
`AnthropicProvider`. Falls through to static `AppState.router` (studio.toml).
Returns `503 router_not_configured` when both are absent.

CLI additions (`ServeArgs`): `--dev-api-key`, `--dev-endpoint`,
`--dev-model`, `--debug-session`. The `--dev-api-key` flag bypasses
`/login` and injects a synthetic credential at boot for CI/Playwright.
Env vars `COBRUST_DEV_*` mirror the flags.

Wave M6 layout addition:

```
crates/studio-server/src/
├── secret.rs            # SessionKey / EndpointSecret / SecretError
│                        # + 6 unit tests in #[cfg(test)]
└── routes/
    └── login.rs         # POST /api/login, /api/logout,
                         # GET /api/session/status, /api/session/endpoint
```

Integration tests (`tests/secret_roundtrip.rs`, 3 tests):
- `login_then_dispatch_with_in_memory_key` — POST /api/login →
  wiremock Anthropic stub → dispatch resolves without ANTHROPIC_API_KEY.
- `restart_drops_key_returns_401` — fresh AppState (no key) →
  dispatch returns 503.
- `wrong_passphrase_login_returns_401` — second login with wrong
  passphrase → 400 wrong_passphrase.

### Wave A6+ extensions

- Tauri embedded-server lifecycle helper for ADR-0013 M9T (loopback
  `127.0.0.1:0`, resolved base URL returned to the desktop shell).
- Per-`Chunk` SSE streaming on `/api/dispatch` (requires plumbing
  `LlmProvider::complete_stream` through `studio_router::Router`).
- Multi-user key derivation (per-user salt + per-user key map, deferred
  to v0.3.x per ADR-0007 §"Consequences").

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
