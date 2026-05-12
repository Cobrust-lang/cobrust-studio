# Changelog

All notable changes to Cobrust Studio. Follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- **M10 zh/en UI toggle (ADR-0011 Phase 2)** — closes the user dogfood
  feedback "没有中英文切换" by adding a custom Svelte 5 i18n store,
  typed English/Chinese message catalogs, and a visible `[ EN | 中 ]`
  toggle in both the app chrome and the first-run `/login` screen.
  - `web/src/lib/i18n.ts` exports `Locale`, `MessageKey`, `locale`,
    `setLocale()`, and derived `t` with simple named interpolation.
    Locale choice persists in `localStorage['cobrust-studio-locale']`;
    no external i18n framework or runtime locale fetch is introduced.
  - `/login`, `/adr`, `/agent`, `/finding`, `/ledger`, the root loading
    page, and the shared modal close label now render page chrome through
    `$t(...)`; ADR/finding markdown bodies and provider/error codes remain
    source data.
  - Coverage: `src/lib/i18n.test.ts` adds 3 Vitest tests for default
    English, zh persistence, and interpolation. `tests/e2e/i18n.spec.ts`
    adds 2 client-only Playwright specs for the toggle and reload
    persistence. Frontend unit coverage is now 35 tests.

- **M9 `task_tag` dispatch metadata (ADR-0010 Phase 2)** —
  closes ADR-0006 §F-03 and the README design-partner friction item
  for ledger cost analysis by task type.
  - `studio-router` now exports `DispatchContext` and
    `Router::dispatch_ctx(req, ctx)`. The existing `Router::dispatch(req)`
    remains compatible and delegates to `DispatchContext::default()`.
  - `POST /api/dispatch` forwards optional `task_tag` through
    `DispatchContext::task_tag`, echoes it in the SSE `done` frame, and
    records it into the router JSONL ledger.
  - Route-layer validation rejects tags over 256 bytes with
    `task_tag_too_long`, rejects control characters with
    `task_tag_invalid_chars`, and normalises `""` to `None`.
  - Coverage: 2 router unit tests for `dispatch_ctx` ledger flow and 4
    dispatch-route integration tests for omitted / empty / too-long /
    newline task tags.

- **M8 persistent session across binary restart (ADR-0009 Phase 2)** —
  closes Sarah v3/v4 audit Gate B + README §"Looking for 3-5 design
  partners" item 4 + the dogfooder friction the user named as the
  daily-usability gap. The in-memory `SessionKey` now optionally
  survives a binary restart by wrapping the user's `/api/login`
  passphrase in one of three backends:
  - `--persist-session=none` (default) — v0.3.0 baseline; restart
    drops the in-memory key and the user re-enters their passphrase
    via `/login`.
  - `--persist-session=keychain` — OS keychain (macOS Keychain /
    freedesktop secret-service / Windows Credential Manager via
    DPAPI). Strongest cold-disk-theft posture; the passphrase
    lives in the user-scoped keychain, never on the disk image.
  - `--persist-session=file --persist-session-file=<PATH>` — `0600`
    mode plaintext file fallback for environments without a
    keychain (Docker, headless Linux without D-Bus, NixOS modules,
    Kubernetes operators). Same trust model as `--dev-api-key`
    (operator-bounded).
  - New `crates/studio-server/src/persist.rs` module: 470 LOC,
    `PersistBackend` enum + `PersistStore` trait + three concrete
    backends (`NullStore` / `KeychainStore` / `FileStore`).
  - 12 unit tests + 7 integration tests in `tests/persistent_
    session.rs` (6 always-run + 1 `#[ignore]`'d keychain test).
    The integration tests drive `studio_server::auto_unlock_on_
    boot()` directly (F1.5 deep-source-read discipline; tests the
    SAME path `serve()` walks, not a same-instance round-trip —
    the lesson the M6 seal-salt-mismatch bug taught us).
  - Boot-flow auto-unlock verifies the derived key actually opens
    the blob before stashing it (M6 seal-salt-mismatch lesson
    applied). On `open()` failure → auto-clear persist + fall
    through to `/login` so a stale persist entry never masquerades
    as a successful unlock.
  - `POST /api/logout?purge=true` clears the persist backend in
    addition to dropping the in-memory key — for "I want to fully
    forget this credential" workflows. Default `POST /api/logout`
    preserves the persist backend so the next restart can still
    auto-unlock (matches ADR-0009 §"On /api/logout" decision).
  - Workspace deps: `keyring = "3"` (with `apple-native +
    windows-native + sync-secret-service + crypto-rust` features)
    + `zeroize = "1.8"` (the `Zeroizing<String>` wrapper wipes the
    passphrase heap allocation on drop — Aleksandr v3 P2 memory-
    hygiene mitigation extended into M8).
  - Documentation: `docs/agent/modules/studio-server.md` §"Wave
    M8" + `docs/human/{zh,en}/secret-storage.md` §"Persistent
    session backends" + README §"Configuration" §"Persistent
    session (long-lived deployments)".
  - **Closes ADR-0009 Phase 2** (Phase 1 CTO spike landed at
    `bc9e624`).

- **Mei v3 — release-notes auto-wiring + dynamic /login footer**
  (`c233367`):
  - `release.yml` now extracts the matching `## [x.y.z]` CHANGELOG
    section into `release-notes.md` (awk one-liner on ubuntu-latest
    runner) and feeds it to `softprops/action-gh-release@v2` via
    `body_path:`. Future tags get release-page bodies wired
    automatically. **Closes Mei v3 P1** (the empty-release-body
    pre-fix would have been a must-fix-before-HN-post issue).
  - v0.2.0 / v0.2.1 / v0.3.0 release pages backfilled via
    `gh release edit --notes-file` (one-time manual fix).
  - SvelteKit `/login` footer now calls `getVersion()` on mount and
    renders `v.studio_server` — always matches the running binary.
    **Closes Mei v3 P2** ("v0.1.0" hardcoded footer drift).
  - README §"Try it" reordered to lead with the pre-built tarball
    path (~60s, no Rust/Node prereqs); build-from-source demoted
    to alternative sub-section. **Closes Mei v3 P2** (tarball-
    first should be the primary path for non-Rust audience).
  - `/login` passphrase placeholder text "used to derive the AEAD
    key" → "encrypts your API key at rest" — Python-data-scientist-
    friendly. **Closes Mei v3 P3** (AEAD jargon leak).
  - README §"Honest status" v0.2.0 crypto-bug paragraph now links
    to `docs/agent/findings/m6-aead-seal-salt-mismatch.md` (matches
    SPA fallback bug link pattern). **Closes Mei v3 P3** (audit
    trail discoverable).
  - "What's in this repo right now" / "current stable tag" wording
    bumped to v0.3.0.

### Changed

- **Aleksandr v3 cargo-audit hard-fail + `SecretError`
  non-exhaustive** (`79ba7bd`):
  - `ci.yml::cargo audit` step upgraded from `--deny warnings ||
    true` (M5 soft-warn) to `--deny warnings --ignore
    RUSTSEC-2023-0071`. Hard-fails on any NEW advisory; the single
    ignore is the Marvin Attack on `rsa 0.9.x` pulled transitively
    via `sqlx-mysql 0.8.6` (unreachable at runtime since Studio
    uses sqlx only via the `sqlite` feature). Re-evaluate when
    `sqlx 0.9` stable lands. **Closes Aleksandr v3 P3 #4**.
  - `SecretError::UnknownScheme` gains `#[allow(dead_code)]` + a
    documented "reserved for future scheme-guard" docstring. The
    variant is preserved because v2+ AEAD scheme transitions
    (chacha20poly1305 / Argon2id param rev) will surface unknown
    schemes through it. `SecretError` enum gains `#[non_exhaustive]`
    so future variants don't break downstream match arms. **Closes
    Aleksandr v3 P3 #5**.

- **Aleksandr v3 audit hardening** — Rust-senior code review of the
  M6/M7 crypto surface surfaced 6 actionable items, all landed:
  - **`Debug` redaction** on `LoginRequest` / `EndpointSecret` /
    `ServeArgs`. Auto-derived `Debug` impls were a latent secret-
    leak hazard (a future `tracing::instrument` on the login
    handler, or any panic handler that formats `req`, would have
    sprayed plaintext `api_key` / `passphrase` / `dev_api_key`
    into structured logs). Hand-written impls now redact secrets
    using the same pattern `SessionKey` already used. Blast
    radius today is zero — no production code currently formats
    these structs — but the latent hazard is gone. **Aleksandr
    v3 P1**.
  - **`aes-gcm` hardware acceleration** — `Cargo.toml` flips from
    `aes-gcm = "0.10"` to `aes-gcm = { version = "0.10", features =
    ["aes"] }`. Unlocks AES-NI on x86_64 + ARMv8 Crypto Extensions
    on aarch64 across all 5 platforms Studio ships. No correctness
    change; pure performance + future-proof for any hot-path AES
    use beyond the login-dominated Argon2id path. **Aleksandr v3
    P2 #3**.
  - **`ProviderKind` is now `#[non_exhaustive]`** — prevents adding
    Groq / vLLM / etc. variants in v0.4.x from being a semver-
    breaking change for any downstream code matching the enum
    exhaustively. Internal `match` sites in `dispatch.rs`,
    `router_init.rs`, and `dispatch_real_llm_e2e.rs` gain `_ =>`
    arms that surface `unsupported_provider_kind` rather than
    panic on unknown variants. **Aleksandr v3 P3 #1**.
  - **`SessionKey::seal_raw` marked `#[doc(hidden)]`** — signals
    internal/test-only use in rustdoc. Stays `pub` (rather than
    `pub(crate)`) because the integration test that uses it lives
    in `tests/` which is external to the lib crate; `pub(crate)`
    would have broken the existing access pattern. **Aleksandr v3
    P3 #6**.

  Aleksandr v3 verdict overall: "WITH-CAVEATS" — would `cargo
  install`, would file PRs, would trust in a dep tree. Crypto
  design is senior-grade.

  Deferred Aleksandr v3 findings (documented in audit report,
  not in this changelog entry):
  - P2 zeroize on SessionKey/passphrase — out-of-scope per
    ADR-0007 threat model
  - P2 timing asymmetry (no-blob vs wrong-passphrase) — single-
    user MVP context
  - P3 wrong_passphrase guard discards decrypted EndpointSecret
  - P3 SecretError::UnknownScheme dead variant
  - P3 cargo-audit not installed locally

## [0.3.0] — 2026-05-12

**M7 multi-provider /login release.** The `/login` page now supports
both Anthropic and OpenAI-compatible endpoints (vLLM / DeepSeek /
Together / OpenRouter / Groq / Ollama) via an explicit `provider_kind`
field with URL-based auto-suggest in the SvelteKit form. Closes Sarah
v3/v4 Gate A. Sarah v4 verdict ("pilot-ready NOW for 1-5 person
teams") is the headline shift: this release removes the last code-
level blocker, leaving only social/outreach work for the remaining
gates.

Polish from the v0.2.1 follow-up cycle also lands here: Argon2id
wall-clock benchmark with M4 baseline (70 ms median), passphrase
rotation procedure documented, deprecation warning on
`studio.toml::api_key_env` at boot, login.rs salt-generation order
cleanup.

### Added

- **M7 multi-provider /login (ADR-0008 Phase 2)** — `LoginRequest`
  + `EndpointSecret` gain a `provider_kind` field (Anthropic /
  OpenAI / Synthetic) defaulting to Anthropic for v0.2.x back-compat.
  The SvelteKit `/login` form adds a Provider dropdown with URL-based
  auto-suggest (Svelte 5 `$effect`). Dispatch `resolve_router()` selects
  `AnthropicProvider` or `OpenAiProvider` at runtime based on the sealed
  `provider_kind`; `Synthetic` returns 503 as defense-in-depth.
  `--dev-provider-kind <KIND>` CLI flag + `COBRUST_DEV_PROVIDER_KIND`
  env var extend the `--dev-api-key` headless path. Closes Sarah v3
  audit finding #3 and Sarah v4 Gate A. 6 integration tests gate the
  round-trip (wiremock Anthropic + OpenAI stubs) + 1 new Playwright
  E2E asserts URL-hint auto-selection. References ADR-0008.

- **ADR-0008 (status: accepted)** — multi-provider /login design.
  Documents Option C (explicit field + URL hint), wire-format
  additivity, dispatch match arm, and the back-compat path for
  pre-M7 sealed blobs.

- **`secret::tests::bench_argon2id_derive`** — release-mode timing
  benchmark for `SessionKey::derive`. M4 measured at median 70 ms
  (7x faster than ADR-0007 ~500 ms target). 2 s hard ceiling
  enforced; `#[ignore]` by default. Closes Sarah v3 v0.3.x
  "Argon2id wall-clock benchmark" gate.

- **Passphrase rotation documentation** in
  `docs/human/{zh,en}/secret-storage.md` — documents the
  delete-blob-and-re-login procedure for v0.2.x (no
  `/api/change-passphrase` route yet; v0.3.x+ ADR pending).
  Closes Sarah v4 audit #3.

- **README "Three credential paths — security hierarchy" table** —
  makes explicit which paths preserve at-rest encryption vs bypass
  it. Closes Sarah v4 audit #5.

### Changed

- **`Cargo.toml` workspace.package.version** — `0.2.1` → `0.3.0`.
  Minor bump: new public API surface (`LoginRequest.provider_kind`,
  `EndpointSecret.provider_kind`, `ProviderKind` re-exported from
  `studio_server::secret`, `--dev-provider-kind` CLI flag).

- **`router_init.rs`** logs a deprecation warning when any provider
  in `studio.toml` has a non-empty `api_key_env`. Closes Sarah v3
  audit #2. v0.4.x will introduce strict mode that errors instead.

- **`routes/login.rs`** salt-generation order cleanup — defers salt
  + derive to after the wrong-passphrase guard. Closes Sarah v3
  audit #4. Happy path is now a single Argon2id derivation; the
  existing-blob key is reused for the subsequent seal call.

- **README rewritten for v0.2.1+ posture** — replaces v0.1.2 status
  snapshot, reflects 5-platform first-time green, lists the seal-
  salt bug honestly in §"Honest status," renumbers design-partner
  priority list (gates #1, #2, #3 now crossed-off).

- **`docs/outreach/show-hn-draft-v1.md`** rewritten to v2 reflecting
  v0.2.1 + Sarah v3/v4 verdicts. Posting checklist boxes ticked
  except the optional screenshot.

- **`docs/agent/modules/studio-server.md::last_verified_commit`** —
  bumped to the v0.3.0 release commit.

### Methodology firsts (this cycle)

- First **two consecutive ADSD two-phase dispatch SOPs** on adjacent
  waves (M6 ADR-0007 → M7 ADR-0008) without intervening P10
  checkpoint friction. Validates the autonomous-loop discipline +
  the §"Two-phase dispatch" SOP as a repeatable pattern.

- First **Sarah persona v4 cycle** with a one-version-gap verdict
  shift: v3 ("2 months out") → v4 ("pilot-ready NOW for 1-5 person
  teams"). Demonstrates continuous persona testing as the
  methodology's pilot-readiness oracle.

- First **persona-found bug fixed in the same cycle as the audit**
  (Sarah v4 #1 README stale-text → fixed in the same Sarah-v4-
  follow-up commit). Tight closure loop.

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
