# Changelog

All notable changes to Cobrust Studio. Follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Changed

- **README** rewritten for v0.2.1 posture — replaces v0.1.2 status
  snapshot, reflects 5-platform first-time green, lists the seal-salt
  bug honestly in §"Honest status," renumbers design-partner priority
  list (gates #1-#2 now crossed-off).

- **`docs/outreach/show-hn-draft-v1.md`** rewritten to v2 reflecting
  v0.2.1 + Sarah v3 verdict. Posting checklist boxes now all ticked
  except the optional screenshot.

- **`router_init.rs`** logs a deprecation warning when any provider
  in `studio.toml` has a non-empty `api_key_env`. Closes Sarah v3
  audit #2 ("v0.3.x: deprecation warning when api_key_env is non-
  empty"). v0.3.x will introduce strict mode that errors instead.

- **`routes/login.rs`** salt-generation order cleanup — defers salt
  + derive to after the wrong-passphrase guard. Closes Sarah v3
  audit #4 ("salt generated speculatively before existing-blob
  check"). Happy path is now a single Argon2id derivation; the
  existing-blob key is reused for the subsequent seal call.

## [0.2.1] — 2026-05-12

**Release infrastructure patch.** v0.2.0 shipped 4 of 5 platform tarballs
because the GitHub-hosted `macos-13` (Intel) runner queue stalled the
`x86_64-apple-darwin` build for 30+ minutes — the same pattern Sarah v3
had explicitly flagged ("if this stalls again, consider whether the
cross-compile setup needs to change"). This release switches that
build to cross-compile from `macos-14` (Apple Silicon) using
`--target=x86_64-apple-darwin`. Rust + Apple clang natively support
this; the only change is the runner label. No code changes.

### Changed

- **`.github/workflows/release.yml`** — `x86_64-apple-darwin` build
  matrix entry now uses `runner: macos-14` (with the existing
  `--target=x86_64-apple-darwin` flag triggering cross-compile) instead
  of `runner: macos-13`. Eliminates the GitHub-hosted Intel macOS
  runner-queue dependency that blocked v0.1.3 + v0.2.0 from achieving
  5-platform green-first-time. **This is the Sarah v2 pilot-gate #3
  validation tag** — if v0.2.1's release.yml ships all 5 tarballs
  without further runner-queue stalls, the gate closes.

## [0.2.0] — 2026-05-12

**M6 AEAD round-trip release.** The `/login → dispatch` flow is end-to-end:
the SvelteKit form POSTs `(endpoint, api_key, model, passphrase)` to a new
`/api/login` route, the server runs Argon2id + AES-256-GCM to seal the
credentials in `session_kv`, and dispatch decrypts on every call from the
in-memory `SessionKey`. The `ANTHROPIC_API_KEY` env-var workaround is no
longer required (Sarah v2 pilot-gate #2 closed). The windows-latest test
matrix is now green end-to-end (pilot-gate #3 — code is ready; this is
the first tag that ships all 5 platforms after the windows-CI hardening).

### Added

- **M6 AEAD round-trip (ADR-0007 Phase 2)** — full `/login → dispatch`
  implementation:
  - **`crates/studio-server/src/secret.rs`** — `SessionKey` /
    `EndpointSecret` / `SecretError`. AES-256-GCM + Argon2id
    (m=64MiB / t=3 / p=1). Wire format: packed
    `salt(16) || nonce(12) || ciphertext+tag` under scheme
    `"aes-gcm-256/argon2id-v1"`. 7 unit tests including
    `seal_then_re_derive_then_open_round_trips` which locks the salt-
    reuse contract that the original 6 tests did not exercise (see
    §Fixed: "seal() salt mismatch" below).
  - **Routes**: `POST /api/login`, `POST /api/logout`,
    `GET /api/session/status`, `GET /api/session/endpoint`
    (debug-only, gated by `--debug-session`).
  - **`AppState.session_key`** — `Arc<RwLock<Option<SessionKey>>>`,
    `None` at boot.
  - **Dispatch integration** — `resolve_router()` checks `session_key`
    first; decrypts blob + builds per-request `AnthropicProvider`.
    Falls through to static `studio.toml` router. Returns 503 when both
    absent (backward compat preserved).
  - **`--dev-api-key` / `--dev-endpoint` / `--dev-model`** CLI flags +
    matching `COBRUST_DEV_*` env vars. `--debug-session` flag. Boot-time
    injection for CI / Playwright / headless flows.
  - **SvelteKit `/login` page rewrite** — adds a 4th input (Passphrase
    ≥ 8 chars), POSTs to `/api/login` (not the M2 `/api/auth/set-endpoint`
    stub), renames the button "Save endpoint" → "Unlock session".
  - **3 integration tests** in `tests/secret_roundtrip.rs`:
    `login_then_dispatch_with_in_memory_key` (wiremock Anthropic stub),
    `restart_drops_key_returns_401`, `wrong_passphrase_login_returns_401`,
    plus `short_passphrase_login_returns_400` (Sarah v3 #3 regression
    lock).
  - **Playwright E2E specs** — `login-aead.spec.ts` (API-level session
    + logout) and rewritten `login.spec.ts` (drives the SvelteKit form
    end-to-end + asserts `/api/login` plaintext shape + redirect to
    `/adr`).
  - **Docs** — `docs/human/{zh,en}/secret-storage.md` (zh/en parity);
    `docs/agent/modules/studio-server.md` Wave M6 section + SHA stamp.
  - **README** — new "Configuration" section documenting `/login` as
    primary flow + `--dev-api-key` as explicit opt-in. "Known
    limitations" `ANTHROPIC_API_KEY` workaround line removed.
  - **`scripts/smoke-dogfood.sh`** — new step `[5/5] POST /api/login +
    GET /api/session/status`.
  - **Workspace deps**: `aes-gcm`, `argon2`, `rand_core` added.

- **ADR-0007 (status: accepted)** — secret-storage AEAD round-trip
  decision. Documents algorithm pin (AES-256-GCM + Argon2id v1), wire
  format, the 4 options considered (client-side WebCrypto, server-side
  derive, disk-key file, no encryption), the in-scope vs out-of-scope
  threat model, and the falsifiable Done-means criteria. Phase 1 CTO
  spike at `cef7810`; Phase 2 P9 dispatch merged at `dd0b181`; status
  flipped to accepted at `b18418c`.

- **`Store::close() -> async ()`** — explicit SqlitePool shutdown helper
  for tests that unlink the db file. No-op on Unix (unlink semantics
  allow open-file delete); mandatory on Windows-CI where the OS holds
  exclusive file locks until every handle closes.

- **ADSD methodology back-port** (cross-repo) — catalogue v1.2.6 lands
  in [Cobrust-lang/agent-driven-development](https://github.com/Cobrust-lang/agent-driven-development)
  with 6 new entries (F25-F28 + F1.3/F1.4) extracted from Studio's M4/M5
  experience: tag→audit→patch pattern, recursive enforcement-script
  closure, continuous persona testing as dev-loop primitive, persona-as-
  validation epistemic risk, local-vs-CI gate definition drift, README-
  vs-release-tag drift. Studio is the **N=2 ADSD case study**.

### Fixed

- **`SessionKey::seal()` salt mismatch (M6 P9 implementation bug)** —
  the original implementation generated a **fresh random salt on every
  seal() call** and packed it into the blob header, but used the
  `SessionKey` derived from a **different** salt at login time. Result:
  packed_salt did not match derive_salt → any subsequent
  `derive(passphrase, blob[..16])` produced a different key → AEAD tag
  mismatch → false-positive `wrong_passphrase` 400 on every re-login
  with the correct passphrase. Symptom: Playwright login-aead.spec.ts
  test 2 + integration test `restart_drops_key_returns_401` reported
  `authenticated=false` after a valid re-login. **Root cause**: ADR-0007
  §"Wire format" stated "packed salt enables re-derive" but the P9 test
  corpus only ran `key.seal(); key.open()` (no re-derive) so the bug
  was structurally invisible to the test suite. Textbook ADSD F1.0
  (declared-invariant gap). Fix: `SessionKey` now carries its
  `derive_salt`; `seal()` packs `self.salt` (not a fresh random salt).
  Nonce remains fresh per seal (AES-GCM uniqueness requirement). New
  test `seal_then_re_derive_then_open_round_trips` locks the contract.

- **Server-side passphrase strength validation** (Sarah v3 audit #3) —
  SvelteKit form enforced `passphrase.length < 8` client-side but the
  server only checked `is_empty()`. A direct `curl POST /api/login`
  with a 1-char passphrase succeeded server-side, bypassing the
  minimum-strength bar. Server now returns 400
  `{code: "passphrase_too_short"}` for `len() < 8`. New integration
  test `short_passphrase_login_returns_400` locks the regression.

- **Windows-CI `cold_start_index_rebuild` test reliability** —
  `Store::close().await` alone is insufficient on Windows: sqlx pool
  graceful shutdown resolves, but the OS-level file handle (esp. SQLite
  WAL mmap regions) can take a few ms to release. Added a
  `windows_safe_remove_file()` retry helper with exponential backoff
  (10/20/40/80/160/320 ms, ~600 ms total). No-op on Unix where unlink
  semantics tolerate open handles.

- **Windows-CI `dispatch_synthetic_route` TOML escape** — Windows
  tempdir paths contain backslashes that TOML `"..."` strings interpret
  as escape sequences (`C:\Users\...` fails parse). Normalized to
  forward-slash via `.to_string_lossy().replace('\\', "/")` before
  interpolation.

- **Playwright e2e passphrase coordination** — login.spec.ts and
  login-aead.spec.ts initially used different passphrases against the
  same persistent `session_kv` blob. With the seal/derive salt fix
  above this is no longer strictly required, but the alignment to a
  single `playwright-test-passphrase-m6` constant prevents test-order
  drift in future additions.

### Changed

- **`Cargo.toml` workspace.package.version** — `0.1.3` → `0.2.0`.
  M6 is a minor bump (new public API surface: `secret` module +
  3 new routes + `AppState.session_key` field + 4 new CLI flags).

- **`docs/agent/modules/studio-server.md::last_verified_commit`** —
  `7507757` → `3753a2b` (post-seal-salt-fix verification anchor;
  F20 enforced by `scripts/doc-coverage.sh`).

### Methodology firsts (this cycle)

- First **two-phase dispatch SOP** application end-to-end on a single
  feature (ADR-0007 spike → test skeleton → P9 worktree dispatch →
  CTO 守闸 → merge --no-ff). Documented as Studio's contribution to
  ADSD §"Workflow Discipline."
- First **continuous persona testing v3** cycle — Sarah v1 (pilot-
  ready verdict) → v2 (3 pilot-gates) → v3 (gate #2 closed, gate #3
  one-tag-away). Used to catalogue ADSD F27 (continuous persona dev-
  loop) and F28 (persona-as-validation epistemic risk).
- First **bug surfaced by persona-test-driven E2E run** that the
  unit test corpus structurally missed: the seal/derive salt mismatch.
  Demonstrates why test corpus + persona test corpus + e2e are
  orthogonal layers (ADSD §"Deep-source-read" / §"Persona" coverage
  table).

## [0.1.3] — 2026-05-12

**M5 polish cycle — persona-audit-driven improvements + first multi-platform
release tarballs (5-platform CI matrix exercised).**

### Added

- **5-platform release tarball matrix** via `.github/workflows/release.yml`:
  - x86_64-unknown-linux-gnu / aarch64-unknown-linux-gnu
  - x86_64-apple-darwin / aarch64-apple-darwin
  - x86_64-pc-windows-msvc
  Each builds the web bundle + rust-embeds it + tarball/zip + sha256 → GitHub Release assets. Sarah persona's #2 adoption blocker
  (linux + windows pending) closed.
- **Full CI matrix** in `.github/workflows/ci.yml`: cross-platform build/test/clippy + frontend gates (pnpm install + check + test:unit + build) + `cargo audit` + **hermetic Playwright e2e against the live release binary on Linux** (14 specs + 2 dogfood specs run on every push). Aleksandr persona's "no CI matrix caught either P0" critique closed.
- **N=2 ADSD case study** published at
  https://github.com/Cobrust-lang/agent-driven-development/blob/main/plugins/adsd/skills/agent-driven-development/case-study/cobrust-studio-experience.md
  — 1370 lines documenting what Studio validated / stressed / extended in the methodology.
- **`.github/ISSUE_TEMPLATE/design-partner.md`** + `bug-report.md` —
  5-section design-partner intake form (mirrors Sarah persona's
  build-vs-buy questions) + structured bug template.
- **`CONTRIBUTING.md`** — reads-first-code-second contribution
  discipline; PR size to required artifacts mapping; explicit
  "what we won't accept" list.

### Changed

- **`Router::order_preferred()` allocation pattern** —
  `Strategy::Latency` ordering now sorts indices into
  `&self.preferred` instead of cloning `ProviderModel` into the
  intermediate sort buffer. Sort buffer per-element size drops from
  ~96+ bytes (`(f64, ProviderModel)` with String allocations) to
  24 bytes (`(f64, usize)`). Invisible at small N; material at
  M6 multi-provider scale. Aleksandr persona's PR #1.
- **README rewrite** — leads with "What it actually is (30-second
  version)" + methodology vocabulary table (ADR / finding / wave /
  Tx tag / 5 gates / 守闸 with English gloss) + "Why this and not
  Linear + git?" comparison matrix. "Honest status" section moved
  to the bottom (was competing with the Try-It CTA per Mei
  persona v2). "N=2 case study" framing softened to "Studio
  dogfoods ADSD."
- **`studio-router/Cargo.toml`** — removed dead deps `hex`,
  `tracing`, `unicode-normalization`, `uuid`. Lifted from
  upstream's Cargo.toml but the post-strip code path doesn't use
  any. Workspace.dependencies entries kept (other crates use
  them). Aleksandr persona's PR #2 (F-05 from M4 review).

### Fixed

- **doc-coverage CI shallow-clone**: `actions/checkout@v4` default
  shallow clone broke §5 git-reachability check on
  `last_verified_commit` SHAs. Set `fetch-depth: 0` on
  doc-coverage + playwright-hermetic jobs.

### Methodology firsts (this cycle)

- First **multi-platform CI matrix** for a cobrust-lang project
  (5 platforms × Rust 5-gate + frontend gates + audit + Playwright)
- First **persona-driven PR cycle** with two-round continuous
  testing (Mei v1 → v2; Aleksandr v1 → v2 in-flight)
- First **methodology back-port** from a downstream case study to
  the ADSD reference catalogue

## [0.1.2] — 2026-05-12

**Patch release fixing build-from-tag for v0.1.1.**

### Fixed

- **v0.1.1 Cargo.lock stale** — v0.1.1's commit shipped with
  `Cargo.lock` still referencing the v0.1.0 workspace versions
  (`studio-server v0.1.0` etc.). Any `cargo build --workspace
  --locked` against v0.1.1 (e.g. release-tarball.sh, CI builds,
  or M3-doc-recommended user clone) errored with "cannot update
  the lock file because --locked was passed." Regenerated via
  `cargo build` to align Cargo.lock at v0.1.1+. v0.1.2 ships the
  corrected lockfile.
- **`scripts/doc-coverage.sh` §6 paired gate** — the script's
  cargo-test enforcement used `set -e` only on the FAILED-grep
  count. It did NOT propagate `cargo test`'s non-zero exit code
  (e.g. 101 from lockfile mismatch). The v0.1.1 release passed
  doc-coverage despite cargo test exit 101, validating the gap.
  Hardened to fail on EITHER exit ≠ 0 OR FAILED count > 0. F20
  closes the recursive case.

### Carried known limitations

Same as v0.1.1 (see below). The SPA fallback fix from v0.1.1 is
unchanged.

### Upgrade

`v0.1.1` is **known-broken** for `--locked` builds. Users running
`scripts/build-release.sh` or `cargo build --locked` against v0.1.1
should upgrade:

```bash
git fetch origin && git checkout v0.1.2
bash scripts/build-release.sh
```

The router public surface is unchanged. Cargo.lock regenerated +
doc-coverage gate hardened.

## [0.1.1] — 2026-05-12

**Patch release fixing a critical SPA-routing bug shipped in v0.1.0.**

### Fixed

- **F-M4-01 (P0)** — `embed::serve_asset` fallback handler used
  `axum::extract::Path<String>` which does not work on routes
  mounted via `Router::fallback(...)`. Every SPA client-side route
  (`/login`, `/adr`, `/agent`, `/finding`, `/ledger`) returned the
  Axum error string "Wrong number of path arguments for `Path`" as
  the response body instead of the SvelteKit `index.html` shell.
  Replaced with `axum::http::Uri` extractor — see finding
  `m4-release-readiness-spa-fallback-extractor.md`. Locked against
  regression by `serve_asset_handles_spa_routes_login_agent_etc`
  unit test.

### Caught by

CTO 守闸 M4 release-readiness audit (hermetic Playwright harness +
direct binary probe). The audit caught the regression that
`scripts/smoke-dogfood.sh` (only probes `GET /` and `GET /api/*`)
and the embed.rs collocated unit test (called `serve_asset` directly
without going through the Axum router) both missed. F19 release-
readiness mandate validated: clean-shell execution by an
independent caller — Playwright in this case — catches what intent-
driven self-checks miss.

### Known limitations (carried from v0.1.0)

- WebCrypto m2-stub login blob still not server-decrypted; set
  `ANTHROPIC_API_KEY` or `OPENAI_API_KEY` env var before launching
  the binary for working `/api/dispatch`.
- Only `arm64-apple-darwin` tarball; linux x86_64 + linux aarch64
  pending CI matrix.
- 2 of 14 hermetic Playwright e2e specs still fail (finding modal
  text-matcher drift); tracked for v0.1.2.

### Upgrade

`v0.1.0` is **known-broken** for SPA navigation. All users should
upgrade:

```bash
git fetch origin && git checkout v0.1.1
bash scripts/build-release.sh
```

The router public surface is unchanged. The fix is server-side
only.

## [0.1.0] — 2026-05-12

Initial release. Methodology-as-a-service productizing the Cobrust ADR
+ finding + bilingual + wave + 5-gate + ADSD discipline as a self-hosted
control plane.

### Added

- **studio-router** crate (`crates/studio-router/`) — LLM dispatch
  forked from `cobrust-llm-router` @ SHA `61f2aff` (v0.1.1) per ADR-0005
  + ADR-0006 addendum. AnthropicProvider + OpenAiProvider + BLAKE3
  cache + JSONL ledger + retry. Consensus mode stripped for MVP.
- **studio-store** crate (`crates/studio-store/`) — ADR/finding
  markdown CRUD + SQLite materialized index + JSONL ledger reader
  + encrypted-blob session k/v + filesystem watcher (per ADR-0004).
- **studio-server** crate (`crates/studio-server/`) — Axum HTTP
  layer. 10 routes: GET/POST /api/adr (+/:id), GET/POST /api/finding,
  GET /api/project/current, POST /api/auth/set-endpoint, GET
  /api/ledger/recent, GET /api/events SSE, POST /api/dispatch SSE,
  GET /api/health, GET /api/version. Tracing + CORS middleware +
  JSON 404 envelope.
- **web/** — SvelteKit 5 frontend with 5 pages (login / adr /
  agent dispatch / finding / ledger). Tailwind v4. WebCrypto m2-stub
  auth (real AEAD scheme deferred). SSE chunk → done frame
  consumer with per-page lifecycle.
- **rust-embed** integration — `web/build/` static SPA baked into
  `target/release/cobrust-studio` (9.0 MiB self-contained binary).
  Per ADR-0002.
- **Dogfood smoke**: `scripts/smoke-dogfood.sh` spawns binary
  against this repo, verifies 6 constitutional ADRs visible via
  /api/adr.
- **Hermetic Playwright harness** for 14 e2e specs + dedicated
  dogfood spec (14 + 2 specs).
- 6 ADRs (stack / single-binary / auth / storage / router-lift /
  router-API-and-lift-provenance) + 4 findings (a1-1-strip-2-noop /
  f20-closure / cto-shougate-grep-leak / [M4 release-readiness if
  any]).
- ADSD v1.2.1 methodology absorbed; F19/F20/F21 enforced;
  doc-coverage gate enforces last_verified_commit real-SHA shape
  AND git-reachability.

### Methodology firsts

- First F20 systemic closure landed in Cobrust Studio
  (`f20-closure-last-verified-commit-enforcement`).
- First CTO 守闸 SOP leak caught + finding filed
  (`cto-shougate-test-gate-grep-leak`).
- N=2 dogfood validation of the ADSD methodology (Cobrust = N=1).

### Quick start

```bash
git clone https://github.com/Cobrust-lang/cobrust-studio && cd cobrust-studio
bash scripts/build-release.sh
./target/release/cobrust-studio serve --project . --port 7878
open http://localhost:7878
```

For working dispatch in M2-era release: set `ANTHROPIC_API_KEY` or
`OPENAI_API_KEY` env var before launching (the M2 stub login form
stores credential but server-side decrypt lands at M3+).

### Known limitations

- `/login` WebCrypto m2-stub blob is opaque to the server; real
  AEAD round-trip + decrypt at M3+. Use `*_API_KEY` env var meantime.
- Pre-built tarball not yet uploaded — build from source via
  `scripts/build-release.sh`.
- Hermetic Playwright e2e requires `bash scripts/build-release.sh`
  first.

### Cross-references

- ADR-0001 through ADR-0006 (+ ADR-0006 §"Addendum 2026-05-11")
- `docs/agent/modules/{studio-router,studio-store,studio-server,web-frontend}.md`
- `docs/agent/findings/*.md`
