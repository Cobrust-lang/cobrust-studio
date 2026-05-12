<div align="center">

# Cobrust Studio

**A self-hosted web console for managing AI coding agents under engineering discipline.**

Point it at a git repo. It gives you a typed REST + SSE API + a 5-page web UI
to dispatch LLM completions, capture the resulting decisions as **Architecture
Decision Records** (markdown), capture the surprises as **findings** (markdown),
and ledger every token through a JSONL audit trail. All git-native — your `docs/`
tree stays plain markdown editable by `vim`.

[![License](https://img.shields.io/badge/license-Apache--2.0%20%2F%20MIT-blue.svg)](#license)
[![Release](https://img.shields.io/github/v/release/Cobrust-lang/cobrust-studio?label=release)](https://github.com/Cobrust-lang/cobrust-studio/releases)
[![Stage](https://img.shields.io/badge/stage-v0.3.0%20early-orange.svg)](CHANGELOG.md)
[![ADSD](https://img.shields.io/badge/methodology-ADSD-blue)](https://github.com/Cobrust-lang/agent-driven-development)

</div>

---

## What it actually is (30-second version)

If your team is using Cursor / Claude Code / OpenHands / similar **for serious
work** — not toy demos — you eventually want answers to:

1. *What did the agents decide?* → ADRs (one markdown file per decision)
2. *What went wrong?* → findings (one markdown file per failure / dead end / benchmark surprise)
3. *Where did the tokens go?* → ledger (one JSONL line per LLM dispatch — provider, model, cost, latency)
4. *Are we drifting from the plan?* → live `/api/events` SSE stream as ADRs and findings get added
5. *Is the methodology actually being followed?* → a 5-gate CI script that fails the merge if it isn't

Cobrust Studio is a 9 MiB single Rust binary that gives you all five, served as
a web UI + REST API against any git repo you point at. Markdown is source of
truth; SQLite is just a materialized index for fast queries. There's no SaaS,
no vendor lock-in, no per-seat pricing — you self-host, your data stays on
your laptop or server.

## Methodology vocabulary

The web UI surfaces these terms. Skim the table once and the rest of the
project makes sense:

| Term | What it is | Where it lives |
|---|---|---|
| **ADR** (Architecture Decision Record) | One file per design decision: context + options considered + decision + consequences. Frontmatter has `adr_id`, `title`, `status`, `date`. | `docs/agent/adr/NNNN-*.md` |
| **Finding** | One file per negative result, bug postmortem, benchmark surprise. Frontmatter has `finding_id`, `severity`, `status`. | `docs/agent/findings/*.md` |
| **Wave** | A batch of related commits, like an agile sprint. Named `A1`, `A2`, etc. Tx tags reference the wave. | Commit message scope tags |
| **Tx tag** | Conventional-commit scope plus a wave coordinate (`feat(server): A4.7 wire dispatch SSE`). | `git log` |
| **5 gates** | `cargo fmt --check` + `cargo clippy -D warnings` + `cargo build --locked` + `cargo test --locked` + `bash scripts/doc-coverage.sh`. Required green before any merge. | `scripts/doc-coverage.sh` |
| **守闸** (CTO gate-check) | Post-merge audit by the CTO role: trust-but-verify cold rebuild + 5-gate + read the diff. Done in Chinese to honor the methodology's origin, not to gatekeep — every gate name is also annotated in English. | Wave merge commit messages |

The methodology itself (ADSD — *Agent-Driven Software Development*) is a
separate open project distilled from a 9-week run building the Cobrust language
project: https://github.com/Cobrust-lang/agent-driven-development.

Studio dogfoods ADSD — meaning the same discipline that builds the tool is
the discipline the tool surfaces. You can read all the methodology in the
ADSD repo above without ever installing Studio.

---

## Try it (5 minutes, build from source)

You need `rustup` (Rust 1.94+), `pnpm` (Node 20+ for SvelteKit), and a git
repository to point Studio at.

```bash
git clone https://github.com/Cobrust-lang/cobrust-studio && cd cobrust-studio
bash scripts/build-release.sh
# → target/release/cobrust-studio (9.0 MiB self-contained binary)

# Dogfood mode: Studio managing its own repo
./target/release/cobrust-studio serve --project . --port 7878
open http://localhost:7878
```

You should see Studio's 5 pages:

- **/login** — paste your LLM endpoint + API key (custom endpoint or OpenAI-compatible)
- **/adr** — list/detail/create the 6 ADRs that live in this repo (`docs/agent/adr/`)
- **/agent** — write a prompt, submit, watch the SSE stream of completion chunks
- **/finding** — list the failures + bug postmortems captured during development
- **/ledger** — every LLM dispatch with provider, model, tokens, latency, cost

**Pre-built tarballs**: every release tag ships 5 platform builds —
linux x86_64 + aarch64, macOS x86_64 + arm64, windows x86_64. Grab
one from
[the releases page](https://github.com/Cobrust-lang/cobrust-studio/releases/latest)
and skip the `cargo build` step.

---

## Configuration

### Primary flow — `/login`

Use the `/login` page to configure your LLM endpoint. Fill in:

- **Endpoint URL** (e.g. `https://api.anthropic.com`)
- **API key**
- **Model** (e.g. `claude-opus-4-7`)
- **Passphrase** — used to encrypt the key before disk storage (never stored itself)

Studio derives an AES-256 key from your passphrase via Argon2id (intentionally slow
to resist brute force) and seals your credentials with AES-256-GCM. The encrypted
blob stays in SQLite; the derived key lives only in server memory. On process restart,
re-entering the passphrase re-derives the key — your API key stays encrypted at rest.

**Measured Argon2id wall-clock** (release-mode build, `m=64 MiB / t=3 / p=1`):
- Apple M4 (2024 MacBook): **~70 ms** (N=5, median)
- GitHub Actions Linux runner (2 vCPU shared, x86_64): ~300-400 ms estimated
- Old laptop (2018-era Intel i5): ~500-800 ms estimated

Hard ceiling is 2 s (`secret::tests::bench_argon2id_derive` enforces). If your
hardware exceeds that, file a finding — the m_cost parameter may need tuning.
Re-run the bench yourself with:
```
cargo test --release -p studio-server --lib -- --ignored --nocapture bench_argon2id_derive
```

**Rotating your passphrase**: delete the SQLite session_kv row and re-login. Today
there's no `POST /api/change-passphrase` route — that's an ADR-pending v0.3.x
enhancement. Procedure for now:
```bash
# while the server is stopped:
sqlite3 .cobrust-studio/studio.db "DELETE FROM session_kv WHERE key = 'endpoint';"
# then start the server and visit /login with the new passphrase
```

See `docs/human/en/secret-storage.md` for the full security model.

### Headless / CI escape hatch — `--dev-api-key`

For CI pipelines, Playwright fixtures, or scripted usage:

```bash
cobrust-studio serve \
  --project /path/to/project \
  --dev-api-key sk-ant-xxx \
  --dev-endpoint https://api.anthropic.com \
  --dev-model claude-opus-4-7
```

This bypasses `/login` and injects credentials at boot. Also available via env vars
`COBRUST_DEV_API_KEY`, `COBRUST_DEV_ENDPOINT`, `COBRUST_DEV_MODEL`.

The studio.toml `api_key_env` field (e.g. `ANTHROPIC_API_KEY`) also remains supported
for backward compatibility and for the studio.toml router path.

### Three credential paths — security hierarchy

Studio supports three ways to provide a credential, in **descending order of at-
rest security**:

| Path | At-rest encryption | Recommended for |
|---|---|---|
| `/login` (POST `/api/login` from browser or curl) | ✅ AES-256-GCM + Argon2id | **Default. Production / pilot use.** |
| `--dev-api-key` CLI flag + `COBRUST_DEV_*` env | ❌ plaintext in process memory + shell history | CI fixtures, Playwright, hermetic e2e |
| `studio.toml api_key_env = "ANTHROPIC_API_KEY"` + env var | ❌ plaintext on disk in `studio.toml` (the var name) + plaintext in env | Legacy / pre-M6 deployments (deprecated; v0.3.x will require migration) |

The first two work today and are documented. The third fires a `tracing::warn!`
at startup when detected — it's kept for backward compat but slated for removal.
Pick `/login` unless you have a specific reason not to.

---

## Why this and not Linear + git?

| | Linear / GitHub Projects | A handcrafted ADR repo | Cobrust Studio |
|---|---|---|---|
| Source of truth | SaaS DB | Git markdown | **Git markdown** |
| Vendor lock-in | High | None | None |
| Live dispatch + token ledger | No | No | **Yes (SSE)** |
| Per-decision audit trail | Free text in PR | Manual | **ADR with frontmatter + schema check** |
| Negative-results discipline | None | Manual `findings/` convention | **First-class finding type + CI gate** |
| CI gate forcing the discipline | None | Whatever you write | **`doc-coverage.sh` 6-step gate** |
| 5-min self-host | No (SaaS only) | N/A | **Yes (9 MiB single binary)** |
| Multi-agent dispatch parallelism | No | No | **Yes (4-way cap per ADSD §1)** |
| Cost | Per-seat-per-month | Free + your time | Free + your time |

If your engineering culture already does "ADR + plain markdown + git" and you
just want a fast web view + an LLM dispatch surface on top, Studio is the
shortest path.

If your team would benefit from being *forced* into the discipline by a CI
gate, Studio's `scripts/doc-coverage.sh` is the lever — it fails the merge if
ADRs are missing frontmatter, if any `last_verified_commit` is a `HEAD`
placeholder (vs a real git SHA), or if `cargo test` has any FAILED groups.

---

## Architecture

```
┌──────────────────────────────────────────────────────┐
│  SvelteKit 5 web (5 pages, baked into binary)        │
└─────────────────────┬────────────────────────────────┘
                      │ REST + SSE (axum 0.7)
┌─────────────────────▼────────────────────────────────┐
│  studio-server     /api/health      /api/version     │
│                    /api/auth        /api/project     │
│  (Axum + tokio)    /api/adr         /api/finding     │
│                    /api/ledger      /api/dispatch    │
│                    /api/events      404 JSON fallback│
└──────┬────────────────────────────────┬──────────────┘
       │ Store                          │ Router
       │                                │
┌──────▼─────────┐               ┌──────▼─────────────┐
│ studio-store   │               │ studio-router      │
│ markdown CRUD  │  JSONL ledger │ LlmProvider trait  │
│ + SQLite index │ ◄───────────► │ Anthropic provider │
│ + filesystem   │ (canonical)   │ OpenAI-compat      │
│   watcher      │               │ + BLAKE3 cache     │
└────────────────┘               └────────────────────┘
```

Three crates total (~3000 LOC of Rust + 1500 LOC of SvelteKit). The
`studio-router` crate is a stripped fork of [`cobrust-llm-router`](https://github.com/Cobrust-lang/cobrust) per ADR-0005 — consensus mode + per-task routing
tables dropped; `LlmProvider` trait + Anthropic/OpenAI providers + BLAKE3
content-addressed cache + JSONL ledger kept. Same upstream copyright headers
on every lifted file.

See [`docs/agent/adr/`](docs/agent/adr/) for design decisions and
[`docs/agent/modules/`](docs/agent/modules/) for per-crate agent-facing module
specs.

---

## What's in this repo right now (v0.2.1)

- ✅ 3 crates compile clean, ~200 tests pass (33 ok groups, 0 FAILED)
- ✅ 14 Playwright e2e specs (13 active + 1 STUDIO_E2E_ROUTER-gated; all pass)
  + 2 dedicated /login e2e specs (login.spec.ts drives the SvelteKit
  form; login-aead.spec.ts hits the API surface)
- ✅ 2 Playwright dogfood specs (Studio managing its own ADRs — both pass)
- ✅ Real-LLM e2e against an OpenAI-compatible endpoint — PASS
- ✅ `scripts/build-release.sh` produces a ~9 MiB self-contained binary
- ✅ `scripts/smoke-dogfood.sh` end-to-end 5-step smoke (incl. M6 `/api/login` round-trip)
- ✅ doc-coverage CI gate enforces 7 invariants (crate agent-docs / zh-en parity / ADR frontmatter / ADR id monotonic / `last_verified_commit` is a real git SHA / `cargo fmt --check` / cargo test exit 0 AND 0 FAILED groups)
- ✅ 5 findings filed (3 closed + 2 closed by the post-tag M4 audit)
- ✅ **5-platform tarballs ship first-time green on v0.2.1**: linux
  x86_64 + aarch64, macOS x86_64 + arm64, windows x86_64. macOS x86_64
  cross-compiles from `macos-14` (Apple Silicon) using
  `--target=x86_64-apple-darwin`, eliminating the `macos-13` runner-
  queue dependency. Sarah v2 pilot-gate #3 closed.
- ✅ M6: AEAD round-trip on `/login` shipped — AES-256-GCM + Argon2id
  (m=64MiB / t=3 / p=1). Server-side derive; in-memory `SessionKey`;
  re-derive round-trip exercised by `seal_then_re_derive_then_open`
  regression test. `--dev-api-key` escape hatch for CI/headless use.
  Sarah v2 pilot-gate #2 closed.
- ⚠️ Single-user / single-project by design — no RBAC, no multi-tenancy in v0.2.x
- ⚠️ Bus factor 1 (looking for design partners — see below)

---

## Looking for 3-5 design partners

If your team already runs **AI-driven development at multi-agent fidelity** —
3+ parallel agent workflows, ADR/finding discipline, you'd benefit from an
honest token ledger and a forced doc-coverage gate — open an issue with the
`design-partner` template describing your setup. Studio in concert with
[ADSD](https://github.com/Cobrust-lang/agent-driven-development) is a hard
adoption (you import a vocabulary + a CI gate, not just a UI), but a real
one if you're already 80% of the way there.

Top friction items design partners would file against me, in priority order:

1. ~~**AEAD round-trip on `/login`** — kill the env-var workaround~~ ✅ shipped in M6 (v0.2.0)
2. ~~**5-platform tarballs first-time green**~~ ✅ shipped in v0.2.1 (cross-compile patch)
3. ~~**Multi-provider `/login`**~~ ✅ shipped in M7 (v0.3.0) — `LoginRequest` + `EndpointSecret` gain `provider_kind` (Anthropic / OpenAI-compat); the SvelteKit form adds a Provider dropdown with URL-based auto-suggest. vLLM / DeepSeek / Together / OpenRouter / Groq / Ollama now work end-to-end via the session path. ADR-0008 Phase 2 merged.
4. **Persistent session across binary restart** — currently the in-memory `SessionKey` drops on restart and the user re-enters their passphrase. For systemd/Docker long-lived runs, wrap the key with OS keychain (macOS Keychain / freedesktop secret-service / Windows DPAPI). v0.3.x ADR pending.
5. **A `--multi-user` mode** with proper RBAC + audit log (post-MVP, M7+)
6. **`task_tag` plumbing through `CompletionRequest`** (ADR-0006 §F-03 noted; partial today)
7. **Persona simulation in CI** — already-run human-in-the-loop (Mei / Aleksandr / Sarah v1-v3 audits all landed), not yet automated

I'm `hakureirm` on GitHub. File issues with the `design-partner` label.

---

## Contributing

Reads-first, code-second: read [`CLAUDE.md`](CLAUDE.md) (the project's
constitution) before opening a PR. Every PR needs to keep all 6
`doc-coverage.sh` gates green. Conventional commits + Tx tags
(`feat(scope): A4.7 ...`) are enforced socially, not by hook (yet).

ADRs land for any cross-file decision. Findings land for any failure / bug
postmortem / benchmark surprise. Negative results are first-class deliverables
— see [ADSD §"Honest fail acceptance"](https://github.com/Cobrust-lang/agent-driven-development/blob/main/plugins/adsd/skills/agent-driven-development/SKILL.md#part-4--quality--verification).

---

## License

Dual-licensed under [Apache-2.0](LICENSE-APACHE) and [MIT](LICENSE-MIT) at
your option. Same as Rust itself and ADSD itself.

Studio's `studio-router` crate is a derivative work of `cobrust-llm-router`
(also Apache-2.0 OR MIT, same author lineage) per ADR-0005 §"License
attribution"; upstream copyright headers are preserved on every lifted file.

---

## Honest status

This started as a 5-day MVP (built 2026-05-11 → 2026-05-12, single
contributor) and has been continuously hardened since. The early
honest mistakes:

- **v0.1.0** shipped with a critical SPA fallback bug
  ([`Path<String>` on `Router::fallback`](docs/agent/findings/m4-release-readiness-spa-fallback-extractor.md))
- **v0.1.1** shipped with a stale `Cargo.lock`
- **v0.2.0** shipped with a subtle crypto bug — `SessionKey::seal()`
  packed a fresh random salt instead of the derive salt, silently
  breaking the re-derive round-trip. Caught the day of release by
  Playwright e2e test 2, fixed in `3753a2b` before any user touched
  the broken release.

Each was caught by the audit pattern (hermetic Playwright + clean-
shell probe + persona-driven re-test) and named in the CHANGELOG by
file:line.

**v0.2.1 is the current stable tag.** It's the first one to ship all
5 platform tarballs first-time green (Sarah v2 pilot-gate #3 closed).
The CHANGELOG names every regression that came before it and the
gate that missed each one. If you'd prefer a year-old tag where you
don't see the patch dance, this isn't your project.

The methodology discipline runs throughout the repo — see
[`docs/agent/findings/cto-shougate-test-gate-grep-leak.md`](docs/agent/findings/cto-shougate-test-gate-grep-leak.md)
for the kind of self-incrimination postmortem the project writes about
itself.
