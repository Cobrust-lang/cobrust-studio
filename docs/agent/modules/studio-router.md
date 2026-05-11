---
doc_kind: module
module_id: studio-router
last_verified_commit: HEAD
dependencies: [adr:0005]
---

# Module: studio-router

## Purpose

LLM provider routing for Studio. Per ADR-0005 this is a fork of the
Cobrust project's `cobrust-llm-router` crate, with consensus mode
dropped. When upstream publishes to crates.io, this becomes a thin
re-export facade.

## Public surface (M1 target)

Lifted from `cobrust-llm-router`:

- `Provider` trait + `AnthropicProvider` / `OpenAiProvider`
- `Router::dispatch(req: DispatchRequest) -> Stream<Chunk>`
- `Cache` (BLAKE3 content-addressed)
- `Ledger` integration point (delegated to `studio-store::ledger`)
- `RoutingTable` config

Dropped for MVP:
- Consensus mode (`Strategy::Consensus { n }`)

## Internal architecture

Same as `cobrust-llm-router`:
- `provider.rs` — trait + shared types
- `anthropic.rs` — Messages API adapter
- `openai.rs` — OpenAI-compatible adapter
- `cache.rs` — disk cache
- `router.rs` — strategy + retry + dispatch

## Tests

- M0: smoke test on `version()`.
- M1: HTTP adapter integration test (mock server); cache hit/miss;
  retry on transport failure.

## Cross-references

- ADR-0005 (router lift)
- Upstream: `cobrust-lang/cobrust` `crates/cobrust-llm-router/`
- src: `crates/studio-router/`
