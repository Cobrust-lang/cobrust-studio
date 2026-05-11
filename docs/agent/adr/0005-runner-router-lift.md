---
adr_id: "0005"
title: Agent runner — lift cobrust-llm-router as studio-router
status: accepted
date: 2026-05-11
supersedes: []
superseded_by: []
---

# ADR-0005: Agent runner architecture

## Context

The Cobrust project's `cobrust-llm-router` crate already implements:
- Provider trait + `AnthropicProvider` + `OpenAiProvider` (compatible
  with DeepSeek, vLLM, OpenRouter, Together, Groq, ...)
- BLAKE3 content-addressed cache
- JSONL ledger
- Retry with exponential backoff
- Per-task routing (Quality / Cost / Latency / Consensus)
- SSE streaming

Studio needs all of this. Re-implementing would be duplicative.

Constraint: `cobrust-llm-router` is currently a path-dep crate inside
the Cobrust workspace. Not yet on crates.io.

## Options considered

### Option 1 — Fork as `studio-router` crate, plan to converge

Copy the source. Add `studio-router` to the workspace as an internal
crate. Track upstream changes manually until Cobrust publishes
`cobrust-llm-router 1.0` to crates.io.

**Pros**: zero-day delivery; no upstream coordination.

**Cons**: ongoing manual sync.

### Option 2 — git-submodule cobrust into studio repo

`crates/studio-router` is a path-dep into a submodule.

**Pros**: live upstream tracking.

**Cons**: git-submodule UX pain; couples build to Cobrust's monorepo
layout.

### Option 3 — Wait for `cobrust-llm-router` on crates.io

Delays Studio MVP by weeks.

### Option 4 — Re-implement from scratch in `studio-router`

Cleanroom. ~1 week of work duplicating existing tested code.

## Decision

**Option 1**. Fork. Adopt the design and code under MIT/Apache (matching
Cobrust's license), keep `studio-router` as an internal crate. Once
upstream publishes to crates.io, `studio-router` becomes a thin
re-export facade.

For MVP we additionally **drop the consensus mode** (`Strategy::Consensus
{ n }`) — single-provider-per-call is sufficient for Studio's use case
(real-time interactive UX, not batch translation). Consensus may return
in M5+ if user demand surfaces.

## Consequences

- Enables: Day 2 backend MVP without re-implementing router.
- Forecloses: independent evolution of router API (we shadow upstream
  changes until convergence).
- Maintenance: when Cobrust ships `cobrust-llm-router 0.x.y` to
  crates.io, replace the forked code with a `cargo add` + re-export.
- License attribution: keep the copyright headers from Cobrust.

## Cross-references

- Cobrust `crates/cobrust-llm-router/`
- Cobrust ADR-0004 (router architecture)
- ADR-0001 (stack)
