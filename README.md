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
[![Stage](https://img.shields.io/badge/stage-v0.1.2%20early-orange.svg)](CHANGELOG.md)
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

For working `/agent` dispatch in v0.1.2: set
`ANTHROPIC_API_KEY` or `OPENAI_API_KEY` (or any OpenAI-compatible endpoint) as
an env var **before** launching the binary. The `/login` form's WebCrypto stub
stores credentials but the server-side AEAD round-trip is M5 work; env var is
the actual auth path today. The `/login` UI says this in a banner.

Pre-built tarball at M5 (CI matrix for linux x86_64 + linux aarch64 + macos).
Today only the macos arm64 tarball is auto-built (`scripts/release-tarball.sh`).

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

## What's in this repo right now (v0.1.2)

- ✅ 3 crates compile clean, ~196 tests pass (32 ok groups, 0 FAILED)
- ✅ 14 Playwright e2e specs (13 active + 1 STUDIO_E2E_ROUTER-gated; all pass)
- ✅ 2 Playwright dogfood specs (Studio managing its own ADRs — both pass)
- ✅ Real-LLM e2e against an OpenAI-compatible endpoint — PASS
- ✅ `scripts/build-release.sh` produces a 9.0 MiB self-contained binary
- ✅ `scripts/smoke-dogfood.sh` end-to-end smoke (6 constitutional ADRs visible via `/api/adr`)
- ✅ doc-coverage CI gate enforces 6 invariants (crate agent-docs / zh-en parity / ADR frontmatter / ADR id monotonic / `last_verified_commit` is a real git SHA / cargo test exit 0 AND 0 FAILED groups)
- ✅ 5 findings filed (3 closed + 2 closed by the post-tag M4 audit)
- ⚠️ Single-platform tarball (macos arm64); linux pending CI matrix
- ⚠️ `/login` UI is a credential-blob stub; env var is the actual auth path today
- ⚠️ Single-user / single-project by design — no RBAC, no multi-tenancy in v0.1.x
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

1. **AEAD round-trip on `/login`** — kill the env-var workaround
2. **Linux + windows tarballs** via the CI matrix (release.yml landed; first cross-platform tag pending)
3. **A `--multi-user` mode** with proper RBAC + audit log (post-MVP, M6+)
4. **`task_tag` plumbing through `CompletionRequest`** (ADR-0006 §F-03 noted; partial today)
5. **Persona simulation in CI** — already-run human-in-the-loop, not yet automated

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

This is a 5-day MVP (built 2026-05-11 → 2026-05-12, single contributor).
v0.1.0 and v0.1.1 both shipped known-broken — v0.1.0 had a critical SPA
fallback bug
([`Path<String>` on `Router::fallback`](docs/agent/findings/m4-release-readiness-spa-fallback-extractor.md));
v0.1.1 had a stale `Cargo.lock`. Both were caught by the M4 release-readiness
audit running hermetic Playwright + a clean-shell probe against the released
binary — the audit pattern works as designed, catching things that
intent-driven self-checks missed.

**v0.1.2 is the first usable tag**. The CHANGELOG names every regression by
file:line and the gate that missed each one. If you'd prefer a year-old tag
where you don't see the patch dance, this isn't your project.

The methodology discipline runs throughout the repo — see
[`docs/agent/findings/cto-shougate-test-gate-grep-leak.md`](docs/agent/findings/cto-shougate-test-gate-grep-leak.md)
for the kind of self-incrimination postmortem the project writes about
itself.
