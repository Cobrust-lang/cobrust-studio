---
adr_id: "0006"
title: studio-router public API surface + lift provenance pin
status: accepted
date: 2026-05-11
supersedes: []
superseded_by: []
---

# ADR-0006: studio-router public API surface + lift provenance

## Context

ADR-0005 binds Studio to **fork** `cobrust-llm-router` as the
internal `studio-router` crate. That ADR set the *direction*; this
ADR sets the *binding contract*:

1. **Lift provenance** — which upstream commit we're forking from,
   so the strip-and-modify work has a stable anchor and reviewers
   can diff against it.
2. **Public API surface** — what `studio-router` exposes to
   `studio-server` (the M1 consumer) once strip is complete.

These two questions are entangled: the strip list (plan §H.3) is
defined in terms of upstream symbols, and the resulting public
surface is what survives strip. Landing them in one ADR avoids
the F1.0 sediment risk of "API spec drifts from what the lifted
code actually exposes".

Hard constraints carried from prior ADRs:

- ADR-0001: Rust 2024 edition, `tokio` async, no panics in lib code.
- ADR-0004: ledger writes go through `studio-store::ledger`, not
  directly to filesystem from router.
- ADR-0005: drop consensus mode (`Strategy::Consensus { n }`);
  Studio dispatches one provider per call.

## Lift provenance (binding)

**Upstream**: `github.com/Cobrust-lang/cobrust`
**Pinned SHA**: `61f2aff` (v0.1.1 tag, *"fix(meta): bump workspace
version 0.1.0-beta → 0.1.1, README badge sync"*, dated
2026-05-11 11:42:16 +0800)
**Local clone**: `~/repos/cobrust-source-pin/` (8.8 MB, full repo
history retained for blame-trail).
**Lift scope**: `crates/cobrust-llm-router/` — eight `.rs` files
under `src/` (`anthropic.rs` / `cache.rs` / `config.rs` /
`ledger.rs` / `lib.rs` / `openai.rs` / `provider.rs` / `router.rs`).
**Lift target**: `crates/studio-router/src/`.

**Equivalent pins**: v0.1.2 tag is also valid — `git diff
v0.1.1..v0.1.2 -- crates/cobrust-llm-router/` is empty (no router
changes between releases). Future bumps of the pin without
re-stripping are permitted only when this command continues to
return empty.

**License attribution** (carried per ADR-0005): keep the upstream
copyright headers in each `.rs` file we lift; reference the
upstream ADR-0004 (LLM router architecture) in `lib.rs` docs.

## Strip list (per plan §H.3, mapped to upstream files)

Six entanglement points must be removed during lift. Each item
states the *what* and the *files that change*:

| # | What to strip | Upstream files affected |
|---|---|---|
| 1 | Consensus mode (multi-provider voting) | `lib.rs`, `ledger.rs`, `config.rs`, `provider.rs`, `router.rs` |
| 2 | ADR-0040 honest-gate hooks (L2 verdict typing) | `router.rs`, `ledger.rs` |
| 3 | Per-task routing tables (`spec_extract` / `translate` / `repair`) | `config.rs`, `router.rs` (collapse to single dispatch) |
| 4 | Translation-specific ledger fields (L0–L3 task tags) | `ledger.rs` (generalize to `task_tag: Option<String>`) |
| 5 | Cobrust-specific cache paths (`.cobrust/llm_cache`) | `cache.rs`, `config.rs` (move to `$XDG_DATA_HOME/cobrust-studio/llm_cache` or fallback `~/.cache/cobrust-studio/`) |
| 6 | Typed `RouterResponse` with Cobrust task tags | `router.rs`, `provider.rs` (rename to `DispatchResponse`, drop Cobrust-specific variants) |

## Options considered (API surface shape)

### Option 1 — Lift-preserve: keep upstream symbol names where strip allows

Names that survive strip stay identical to upstream (`LlmProvider`,
`AnthropicProvider`, `Cache`, `Ledger`, etc.). Renames only happen
where strip semantically changes the symbol (e.g.
`RouterResponse` → `DispatchResponse` per strip #6;
`Task` enum → removed per strip #3).

**Pros**:
- Minimal cognitive load when reading upstream source side-by-side
  during lift work.
- When `cobrust-llm-router` publishes to crates.io and we facade-
  re-export (per ADR-0005), the rename surface is small.
- The eventual diff (upstream vs studio-router post-lift) is the
  literal strip list — auditable.

**Cons**:
- Reader sees `LlmProvider` and may assume identical semantics to
  upstream, missing the consensus-mode drop. Mitigation: module
  docstring in `lib.rs` explicitly calls out the strip list.

### Option 2 — Studio-prefix everything (`StudioProvider`, `StudioCache`, …)

Rename every public symbol to start with `Studio` for namespace
clarity.

**Pros**: zero ambiguity vs upstream.

**Cons**: every facade-converge work doubles (rename in studio-
router + rename re-export); diff against upstream becomes harder
to audit; reader-paper-trail is muddier.

### Option 3 — Re-export via facade NOW (path-dep across repo
boundary)

Make `studio-router` a thin shim that path-deps to
`~/repos/cobrust-source-pin/crates/cobrust-llm-router/` and re-
exports.

**Pros**: zero lift work today.

**Cons**: explicitly rejected by ADR-0005 Option 2 (git-submodule
UX pain, layout coupling). Same issue applies to path-dep across
repo boundary — non-portable to CI/CD.

## Decision

**Option 1**. Lift-preserve symbol names; strip per the §"Strip
list" above; renames isolated to semantic changes.

**Public API surface of `studio-router` v0.0.1** (M1 target;
v0.1.0 freeze deferred to M4):

```rust
// Provider trait + shared types (UNCHANGED from upstream)
pub use provider::{
    LlmProvider, CompletionRequest, CompletionResponse, Chunk,
    LlmError, Message, Role, SamplingParams, TokenUsage,
};

// Provider implementations (UNCHANGED from upstream)
pub use anthropic::AnthropicProvider;
pub use openai::OpenAiProvider;

// Cache (UNCHANGED public surface; cache-path config changes per strip #5)
pub use cache::{Cache, CacheKey};

// Ledger (task_tag generalized per strip #4)
pub use ledger::{Ledger, LedgerEntry, Outcome};

// Config (RoutingEntry / StrategyName / DefaultStrategy REMOVED per strip #3)
pub use config::{ProviderConfig, ProviderKind, ProviderModel, RouterConfig};

// Router (RouterResponse → DispatchResponse per strip #6;
//         Strategy::Consensus REMOVED per strip #1;
//         Task enum REMOVED per strip #3)
pub use router::{
    Router, RouterBuilder, RouterError, RetryPolicy,
    DispatchResponse, Strategy,
};
```

**M1 dispatch contract** (consumed by `studio-server::dispatch`):

```rust
let router = RouterBuilder::new()
    .with_config(RouterConfig::from_toml(&path)?)
    .with_cache(Cache::open(cache_dir)?)
    .with_ledger(ledger)  // studio_store::ledger::Ledger impl
    .build()?;

let resp: DispatchResponse =
    router.dispatch(CompletionRequest { /* ... */ }).await?;
```

No `Task` enum at the call site (single dispatch only); no
`Strategy::Consensus` variant; ledger entry `task_tag` is
`Option<String>` the caller fills (Studio passes `Some("agent-
turn")` or `None`).

## Consequences

- **Enables**: A1.1 P7 dev/test pair has unambiguous API target to
  lift against; M1 `studio-server::dispatch` route has stable type
  signatures to compile against.
- **Forecloses**: independent API evolution of `studio-router` vs
  upstream until v0.1.0 freeze (M4). Any cross-cutting API change
  costs an ADR addendum.
- **Migration plan**: when `cobrust-llm-router` publishes to
  crates.io with a 0.x.y tag whose post-strip surface matches this
  ADR (or is a superset), `studio-router` becomes a thin facade
  re-exporting from the upstream crate. Strip-list parity check
  is the gate: empty diff between `(upstream public surface −
  consensus mode − honest-gate hooks − per-task routing − Cobrust
  cache path)` and `(studio-router public surface)` ⇒ migrate.
- **Verification gate for A1.1 P7 DEV**: post-lift, the symbols
  enumerated under §"Decision" must all resolve in `cargo
  check --workspace --locked`; consensus mode tests deleted (not
  ignored); per-task routing tests deleted; cache path test
  asserts `$XDG_DATA_HOME/cobrust-studio/llm_cache` or
  `~/.cache/cobrust-studio/`.

## Cross-references

- ADR-0001 (stack — async tokio, Rust 2024 edition)
- ADR-0004 (storage — ledger lives in `studio-store`, router
  delegates)
- ADR-0005 (lift `cobrust-llm-router` as `studio-router`,
  consensus dropped)
- Plan §H.2 (lift-vs-build reversal preserving B.1/B.2/B.3 audit
  trail)
- Plan §H.3 (strip list)
- Upstream: `~/repos/cobrust-source-pin/crates/cobrust-llm-router/`
  @ SHA `61f2aff` (v0.1.1)
- Upstream ADR-0004 (LLM router architecture, the design we're
  carrying)
