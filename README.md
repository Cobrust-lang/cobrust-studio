<div align="center">

# Cobrust Studio

**AI agent team's project-management & monitoring console.**

*Login, point at a repo, and the [Cobrust methodology](https://github.com/cobrust-lang/cobrust) starts working — ADR-driven decisions, finding-driven failures, wave-based delivery, doc-coverage CI gate.*

[![License](https://img.shields.io/badge/license-Apache--2.0%20%2F%20MIT-blue.svg)](#license)
[![Stage](https://img.shields.io/badge/stage-M0%20scaffold-orange.svg)](docs/agent/adr/)

</div>

---

## What this is

Cobrust Studio is a **standalone control plane** for AI-driven development that adopts the Cobrust language project's methodology — ADR + finding + multi-agent waves + Tx commit tags + bilingual docs + 5-gate CI — and provides a web UI to manage it all.

**Not** the Cobrust language. **Not** the Cobrust translator. Just the methodology, productized.

## Quick start (after M2)

```bash
# Download single binary
curl -L https://github.com/cobrust-lang/cobrust-studio/releases/latest/download/cobrust-studio-$(uname -sm | tr ' ' '-').tar.gz | tar xz

# Run pointing at a git repo
./cobrust-studio serve --project ~/my-repo --port 7878

# Open browser
open http://localhost:7878
```

Login with custom endpoint + API key (OAuth deferred to M5).

## Current status (M0)

- ✅ Workspace scaffold + 3 crates building clean
- ✅ 5 CI gates wired (fmt / clippy / build / test / doc-coverage)
- ✅ 5 ADRs landed (ADR-0001..0005)
- ✅ Constitution (`CLAUDE.md`) + bilingual READMEs
- 🚧 M1 backend MVP (Axum routes + SSE dispatch) — Day 2
- 🚧 M2 frontend MVP (SvelteKit + 4 pages) — Day 3
- 🚧 M3 dogfood + polish + single binary — Day 4
- 🚧 M4 v0.1.0 release — Day 5

See [`CLAUDE.md`](CLAUDE.md) §6 milestones for the 5-day plan.

## Architecture (target)

```
┌────────────────────────────────────────┐
│  SvelteKit web (embedded via rust-embed)│
└─────────────────┬──────────────────────┘
                  │ REST + SSE
┌─────────────────▼──────────────────────┐
│  studio-server (Axum + tokio)          │
│  - /api/auth                            │
│  - /api/project                         │
│  - /api/adr (CRUD)                      │
│  - /api/finding (CRUD)                  │
│  - /api/dispatch (SSE)                  │
│  - /api/ledger                          │
└──────┬──────────────┬──────────────────┘
       │              │
┌──────▼──────┐  ┌────▼─────────┐
│studio-store │  │studio-router  │
│markdown+SQL │  │ Anthropic +   │
│             │  │ OpenAI-compat │
└─────────────┘  └───────────────┘
```

## Contributing

We use the Cobrust methodology on ourselves:
- Decisions → ADRs in `docs/agent/adr/`
- Failures → findings in `docs/agent/findings/`
- Code changes → conventional commits with Tx tags (`feat(scope): A2.3 ...`)
- All public items → entries in `docs/human/{zh,en}/` + `docs/agent/`

See [`CLAUDE.md`](CLAUDE.md) for the full constitution.

## License

Dual-licensed under Apache-2.0 + MIT at your option.

---

*Cobrust Studio M0 — scaffold. v0.1.0 target: 5 days.*
