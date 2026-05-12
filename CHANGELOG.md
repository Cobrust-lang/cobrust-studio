# Changelog

All notable changes to Cobrust Studio. Follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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
