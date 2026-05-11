---
adr_id: "0001"
title: Stack choice — Rust + Axum + SvelteKit + shadcn-svelte + SQLite
status: accepted
date: 2026-05-11
supersedes: []
superseded_by: []
---

# ADR-0001: Stack choice

## Context

Cobrust Studio MVP targets 5-day delivery with a "elegant, natural,
beautiful, high-performance" UI bar. Stack choice on Day 1 determines
whether Day 4 polish is trivial or impossible.

Hard requirements:
- Single binary deployment (no `npm install` on user side)
- Live data streaming (SSE/WebSocket) for agent monitoring
- Type-safe long-running daemon (24/7 supervision tree)
- Reusable LLM router (lift from Cobrust)
- Sub-100ms UI responsiveness
- Dark-mode-first calm-tech aesthetic with low custom-CSS budget

## Options considered

### Option 1 — Rust + Axum + SvelteKit + shadcn-svelte + SQLite

**Backend**: Rust 1.94+ + Axum + tokio + sqlx. Lift `cobrust-llm-router`
as `studio-router`. Use `gix` for git ops (when M2 lands worktrees).

**Frontend**: SvelteKit 5 with runes (smaller bundle than React, more
naturally reactive). shadcn-svelte for component primitives (copy into
repo, deep customisation, escape MUI sameness). Tailwind for styling.

**Storage**: SQLite via sqlx + filesystem markdown for ADRs / findings.
Litestream is M5+ stretch.

**Pros**: matches Cobrust ecosystem; performance budget abundant; type
safety reduces long-daemon bugs; small frontend bundle; calm-tech
aesthetic is default not retrofit.

**Cons**: SvelteKit 5 runes are newer (mid-2025); some shadcn-svelte
components less mature than shadcn-react.

### Option 2 — TypeScript everywhere (Bun + Hono + React + shadcn)

Single language. Faster onboarding for TS devs.

**Pros**: one language; Vercel-style "it just works" DX.

**Cons**: cannot lift `cobrust-llm-router` directly; long-daemon TS in
production has worse memory profile than Rust; SSE+WebSocket in TS
servers requires more ceremony; React bundle size larger; less type
safety at runtime.

### Option 3 — Go + HTMX + Templ

Minimal JS; server-side rendering; tiny binary.

**Pros**: fast deployment; small footprint.

**Cons**: complex live-update UX in HTMX (we have N parallel agent
streams); harder to build the rich monitoring views; lose `cobrust-llm-
router` reuse; aesthetic ceiling lower without modern frontend tooling.

## Decision

**Option 1**. Rust backend + SvelteKit 5 + shadcn-svelte + SQLite.

The 5-day target hinges on **stack-as-aesthetic** (selections at Day 1
that mean Day 4 polish is mostly free), and Option 1 wins on every
front-end axis we explicitly care about (bundle size, dark mode,
reactivity, calm-tech defaults). Backend is non-negotiable Rust to lift
`cobrust-llm-router` and keep daemon stability.

## Consequences

- Enables: single-binary release via rust-embed (ADR-0002); reuse of
  `cobrust-llm-router` (ADR-0005); SQLite zero-ops storage (ADR-0004).
- Forecloses: shipping a CLI-only mode without frontend in MVP (server
  always serves embedded web).
- Migration: when `cobrust-llm-router` publishes to crates.io,
  `studio-router` becomes a re-export facade (ADR-0005).

## Cross-references

- ADR-0002 (single-binary deployment)
- ADR-0004 (storage)
- ADR-0005 (router lift)
- Cobrust repo `crates/cobrust-llm-router`
