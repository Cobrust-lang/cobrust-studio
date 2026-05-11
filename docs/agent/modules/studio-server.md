---
doc_kind: module
module_id: studio-server
last_verified_commit: HEAD
dependencies: [adr:0001, adr:0002, adr:0003]
---

# Module: studio-server

## Purpose

Axum-based HTTP layer for Cobrust Studio. Owns the binary entrypoint
(`cobrust-studio serve`), serves embedded SvelteKit web assets (via
rust-embed per ADR-0002), and exposes the REST + SSE API consumed by
the frontend.

## Public surface (M1 target)

- `cobrust-studio serve --project <path> --port <N>` — CLI entry
- `POST /api/auth/set-endpoint` — store encrypted credentials
- `GET /api/project/current` — project metadata
- `GET /api/adr` — list ADRs
- `GET /api/adr/:id` — fetch one ADR
- `POST /api/adr` — create ADR (server-side schema validation)
- `GET /api/finding` — list findings
- `POST /api/finding` — create finding
- `POST /api/dispatch` — SSE stream of LLM dispatch
- `GET /api/ledger/recent` — recent ledger entries
- `GET /api/events` — SSE channel for state-change events

## Internal architecture (M1)

- `routes/` — one file per route group
- `state.rs` — `AppState { store, router, project }`
- `sse.rs` — fan-out hub for live events
- `embed.rs` — rust-embed for `web/build/`

## Tests

- M0: smoke test on `version()` only.
- M1: integration test per route (start server in tokio test, hit
  endpoint, assert response).

## Cross-references

- ADR-0001 (stack)
- ADR-0002 (single-binary)
- ADR-0003 (auth)
- src: `crates/studio-server/`
