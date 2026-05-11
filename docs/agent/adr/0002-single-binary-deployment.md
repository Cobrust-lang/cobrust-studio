---
adr_id: "0002"
title: Single-binary deployment via rust-embed
status: accepted
date: 2026-05-11
supersedes: []
superseded_by: []
---

# ADR-0002: Single-binary deployment

## Context

User journey requirement: download tarball, run binary, open browser.
No `npm install`, no Docker, no reverse proxy, no nginx config. A
team wanting to try Studio in 5 minutes cannot afford a multi-step
install.

## Options considered

### Option 1 — rust-embed bundles compiled web into the binary

`web/build/` (SvelteKit static export) is embedded at compile time via
the `rust-embed` crate. Axum serves the bundle from memory under `/`,
API under `/api/*`. Binary size ~30 MiB compressed.

### Option 2 — Docker compose (server + nginx + static volume)

Production-grade but requires Docker, breaks the 5-minute install.

### Option 3 — Separate frontend + backend processes (port :3000 + :7878)

Cleanest dev experience but doubles the deploy surface; users have to
think about CORS, two ports, two processes.

## Decision

**Option 1**. Single binary via rust-embed for v0.1.0.

Option 2 ships as a `docker-compose.yml` example only, not the primary
path. Option 3 is the **dev mode** (`pnpm dev` proxies to `:7878`), but
release builds always embed.

## Consequences

- Enables: 5-minute first-dispatch experience; no CORS; single port.
- Forecloses: hot-swapping the frontend without re-deploying.
- Build: `cargo build --release` requires `web/build/` to exist. The
  release workflow runs `pnpm build` first, then `cargo build`.

## Cross-references

- ADR-0001 (stack choice)
