---
doc_kind: module
module_id: studio-router
last_verified_commit: a99d304
dependencies: [adr:0005, adr:0006]
---

# Module: studio-router

## Purpose

LLM provider routing for Studio. Per ADR-0005 / ADR-0006 this is a
fork of the Cobrust project's `cobrust-llm-router` crate (pinned at
upstream SHA `61f2aff`, v0.1.1), with six strip operations applied
during the A1.1 lift. When upstream publishes a matching surface to
crates.io, this crate becomes a thin re-export facade.

## Public surface (M1 frozen)

Per ADR-0006 §Decision. Verified by
`crates/studio-router/tests/strip_invariants.rs`:

- `provider::{LlmProvider, CompletionRequest, CompletionResponse, Chunk, LlmError, Message, Role, SamplingParams, TokenUsage}`
- `anthropic::AnthropicProvider`
- `openai::OpenAiProvider`
- `cache::{Cache, CacheKey}`
- `ledger::{Ledger, LedgerEntry, Outcome}`
- `config::{ProviderConfig, ProviderKind, ProviderModel, RouterConfig}`
- `router::{Router, RouterBuilder, RouterError, RetryPolicy, DispatchResponse, Strategy}`

## Dispatch contract

```rust
let router = RouterBuilder::new()
    .register_provider("anthropic_official", provider_arc)
    .build(&RouterConfig::from_toml_str(toml_str)?)
    .await?;

let resp: DispatchResponse =
    router.dispatch(CompletionRequest { /* ... */ }).await?;
```

Single dispatch only — no `Task` enum at the call site, no
`Strategy::Consensus`. Ledger `task_tag` is `Option<String>` (caller-
supplied via future API surface; today defaults to `None`).

## Strip provenance (ADR-0006)

| # | What | Files affected |
|---|---|---|
| 1 | Consensus mode (multi-provider voting) | lib.rs, ledger.rs, config.rs, router.rs |
| 2 | ADR-0040 honest-gate hooks | router.rs, ledger.rs (NO-OP for `61f2aff` — verified empty grep) |
| 3 | Per-task routing tables | config.rs, router.rs |
| 4 | Translation-specific ledger fields | ledger.rs (`task_tag: Option<String>`) |
| 5 | Cobrust-specific cache paths | config.rs, cache.rs (`cobrust-studio` namespace) |
| 6 | Typed `RouterResponse` | router.rs, provider.rs (renamed to `DispatchResponse`) |

## Internal architecture

Same module split as upstream:

- `provider.rs` — trait + shared types
- `anthropic.rs` — Messages API adapter
- `openai.rs` — OpenAI-compatible adapter
- `cache.rs` — BLAKE3 content-addressed disk cache
- `ledger.rs` — JSONL append-only token ledger
- `config.rs` — `studio.toml` parser + defaults + `Strategy` enum
- `router.rs` — strategy + retry + dispatch (Strategy re-exported
  from config to satisfy ADR-0006's `pub use router::Strategy`)

## Tests

- 47 unit tests (collocated `#[cfg(test)]` blocks)
  - `provider.rs` × 5 — `TokenUsage` + `LlmError` classification
  - `anthropic.rs` × 2 — body builder + status classifier
  - `openai.rs` × 2 — body builder + status classifier
  - `cache.rs` × 12 — key determinism, namespace, perms (0600/0700)
  - `ledger.rs` × 12 — schema + concurrent writes + 0600 perms
  - `config.rs` × 7 — parse + validate + Strategy serde + defaults
  - `router.rs` × 4 — retry policy + EWMA + strategy exhaustiveness
- 4 integration tests under `tests/`
  - `strip_invariants.rs` × 2 — public-surface lock-in + exhaustive
    `Strategy` match (compile-fail on consensus re-introduction)
  - `cache_path_studio.rs` × 1 — strip #5 namespace
  - `ledger_generic_tag.rs` × 1 — strip #4 `task_tag` round-trip

## Cross-references

- ADR-0005 (router lift)
- ADR-0006 (public API surface + strip list)
- Upstream: `cobrust-lang/cobrust` `crates/cobrust-llm-router/` @ SHA
  `61f2aff` (v0.1.1)
- src: `crates/studio-router/`
