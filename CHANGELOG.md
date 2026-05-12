# Changelog

All notable changes to Cobrust Studio. Follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

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
