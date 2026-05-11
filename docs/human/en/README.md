# Cobrust Studio — Human Docs (English)

AI agent team's project-management & monitoring console.

## What this is

Cobrust Studio is a standalone control plane that adopts the [Cobrust
language project's](https://github.com/cobrust-lang/cobrust) methodology
— ADR-driven decisions, finding-driven failures, wave-based delivery,
doc-coverage CI gate, bilingual + agent-doc tracks — and packages it
behind a beautiful web UI.

Login with your LLM provider's API endpoint + key. Point at a git repo.
Studio orchestrates AI agents, captures every decision as an ADR,
every failure as a finding, every dispatch in a token ledger.

## Status

- **M0 — Scaffold (current)**: workspace + 5 ADRs + CI 5 gates green.
- **M1 — Backend MVP**: Axum routes, SSE dispatch, LLM router lift.
- **M2 — Frontend MVP**: SvelteKit UI, 4 core pages.
- **M3 — Dogfood + polish**: Studio manages its own ADRs via Studio UI.
- **M4 — v0.1.0 release**: single binary, demo, external review.

5-day target from M0 to M4. See [`../../../CLAUDE.md`](../../../CLAUDE.md) §6.

## Quick start (after M2)

```bash
./cobrust-studio serve --project ~/my-repo --port 7878
open http://localhost:7878
```

## Architecture

```
SvelteKit web (embedded) ──REST + SSE──> studio-server (Axum)
                                              │
                          ┌───────────────────┴───────────────┐
                          ▼                                   ▼
                   studio-store                        studio-router
                   (markdown + SQLite)                 (LLM providers)
```

See `../../agent/adr/` for design decisions.

## Languages

- English: `docs/human/en/` (this directory)
- 中文: `docs/human/zh/`

## License

Dual-licensed Apache-2.0 + MIT.
