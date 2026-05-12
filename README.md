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

## Quick start (build from source, M3+)

```bash
# Build the release binary (bakes web/build/ via rust-embed)
git clone https://github.com/Cobrust-lang/cobrust-studio && cd cobrust-studio
bash scripts/build-release.sh
# → target/release/cobrust-studio (~9 MiB single binary)

# Run pointing at a git repo (dogfood Studio against itself):
./target/release/cobrust-studio serve --project . --port 7878

# Open browser
open http://localhost:7878
```

Login with custom endpoint + API key (OAuth deferred to M5).

> **M2 status note**: `/login` stores an opaque AES-GCM stub blob;
> real AEAD decrypt lands at M3 polish. For working dispatch today,
> set `ANTHROPIC_API_KEY` or `OPENAI_API_KEY` env var before
> launching the binary (router_init reads env first, blob second).
> M4 release will close this loop.

Pre-built release tarballs land at M4 (v0.1.0). Until then, build
from source as above.

## Current status (M3)

- ✅ M0 — Workspace scaffold + 5 ADRs + 5-gate CI green (Day 1)
- ✅ M1 — Backend MVP — Axum + 10 routes + SSE dispatch + studio-router lift (Day 2)
- ✅ M2 — Frontend MVP — SvelteKit 5 + 5 pages + Vitest unit + Playwright e2e gated (Day 3)
- ✅ M3 — Single-binary deployment via rust-embed + dogfood smoke (Day 4)
- 🚧 M4 — v0.1.0 release tag + F19 release-readiness audit (Day 5)

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
