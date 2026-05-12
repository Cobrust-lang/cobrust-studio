# Cobrust Studio — Human Docs (English)

Desktop-first project-management and monitoring console for AI agent teams.

## What this is

Cobrust Studio is a standalone control plane that adopts the [Cobrust
language project's](https://github.com/cobrust-lang/cobrust) methodology
— ADR-driven decisions, finding-driven failures, wave-based delivery,
doc-coverage CI gate, bilingual + agent-doc tracks — and packages it in
a desktop-first Tauri shell around the same SvelteKit UI and Rust backend.

Login with your LLM provider's API endpoint + key. Point at a git repo.
Studio orchestrates AI agents, captures every decision as an ADR,
every failure as a finding, and every dispatch in a token ledger. M9 adds
optional `task_tag` dispatch metadata so the ledger can be analysed by
work type without changing the provider request shape.

## Status

- **M0 — Scaffold (current)**: workspace + 5 ADRs + CI 5 gates green.
- **M1 — Backend MVP**: Axum routes, SSE dispatch, LLM router lift.
- **M2 — Frontend MVP**: SvelteKit UI, 4 core pages.
- **M3 — Dogfood + polish**: Studio manages its own ADRs via Studio UI.
- **M4 — v0.1.0 release**: single binary, demo, external review.
- **M9T/M9 — v0.4.x desktop + ledger metadata**: Tauri shell,
  persistent session, and `task_tag` ledger plumbing.

5-day target from M0 to M4. See [`../../../CLAUDE.md`](../../../CLAUDE.md) §6.

## Quick start

```bash
# Desktop shell from source
export COBRUST_STUDIO_PROJECT=$PWD
pnpm --dir web install
pnpm --dir web tauri:dev

# Headless/server compatibility mode
./cobrust-studio serve --project ~/my-repo --port 7878
open http://localhost:7878
```

## Architecture

```
Tauri desktop shell ──loopback HTTP──> studio-server (Axum)
        │                                      │
        ▼                                      ▼
  SvelteKit UI                         REST + SSE API
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
