---
adr_id: "0010"
title: M9 DispatchContext — task_tag plumbing + extensible dispatch metadata
status: proposed
date: 2026-05-12
supersedes: []
superseded_by: []
---

# ADR-0010: DispatchContext newtype (M9)

## Context

ADR-0006 §F-03 addendum recorded the deferred decision for
caller-supplied dispatch metadata:

> "Three options surfaced by the review:
> (a) add field to `CompletionRequest`;
> (b) add second arg `dispatch(req, tag)`;
> (c) thread via a `DispatchContext` newtype.
> CTO call: **(c) DispatchContext** at A4 — most extensible (carries
> tag, span IDs, deadline hints in the future) without bloating
> `CompletionRequest`'s wire shape. Not blocking for A1.1 / A2 /
> A3; deferred to A4 task prompt."

A4 shipped without the plumbing (acceptable scope cut for M1).
Today, v0.3.x ledger entries always record `task_tag: None`. The
user dogfoods Studio across multiple workstreams (code-review,
doc generation, test scaffolding, agent dispatch) and lacks
ledger filtering / cost analysis by task type. M9 closes the gap
by landing the previously-deferred DispatchContext.

## Hard constraints from prior ADRs

- **ADR-0006 §"Decision"** — `studio-router` public surface
  exposes `Router::dispatch(req: CompletionRequest) -> Future<…>`.
  M9 extends but does not break this signature.
- **ADR-0006 §"M1 dispatch contract" addendum** — the actual API
  shape implemented is `Router::dispatch(req)`, no per-task
  routing context. M9 adds one optional parameter.
- **ADR-0006 §"Lift provenance"** — the studio-router crate was
  lifted from cobrust-llm-router pinned at `61f2aff`. Upstream
  may or may not gain a similar abstraction; M9 commits to a
  forked surface here (downstream-only).

## Threat model

- **Tag injection / log poisoning**: an attacker who can supply
  `task_tag` could store arbitrary strings in the ledger. Bound
  tag length (256 chars) + reject control characters. Sanitize
  for log emission (escape newlines / ANSI).
- **PII in tags**: callers may inadvertently encode sensitive
  info in tag strings (`"summarise-doc-X-for-user-bob@example.com"`).
  Document that tags appear in ledger entries (which the JSONL is
  cleartext on disk). Caller responsibility, not Studio's.

## Options considered

### Option A — Just add `task_tag: Option<String>` to `CompletionRequest`

```rust
pub struct CompletionRequest {
    pub messages: Vec<Message>,
    pub model: String,
    pub task_tag: Option<String>,   // ← new
    ...
}
```

**Pros**: zero new types; one-field change.

**Cons**: pollutes the wire-shape struct with non-LLM-payload
fields (the model + messages are what goes to the provider; tag
is metadata). The original ADR-0006 §F-03 review explicitly
ruled this out for that reason. Future-proof: if M10 wants to
add `deadline_hint`, `span_id`, `trace_parent`, the request
struct keeps growing into a god-object.

### Option B — Separate `dispatch_with_tag(req, tag)` method

Two methods on `Router`:
```rust
fn dispatch(&self, req: CompletionRequest) -> Future<…>;
fn dispatch_with_tag(&self, req: CompletionRequest, tag: &str) -> Future<…>;
```

**Pros**: explicit at the call site.

**Cons**: API surface doubles for every metadata field added
(`dispatch_with_tag_and_deadline`, ...). Doesn't scale.

### Option C — `DispatchContext` newtype passed alongside `req`

```rust
pub struct DispatchContext {
    pub task_tag: Option<String>,
    pub deadline_hint: Option<Duration>,  // reserved for M10+
    pub span_id: Option<String>,          // reserved for M10+
}

impl Router {
    pub async fn dispatch(&self, req: CompletionRequest) -> Result<DispatchResponse, RouterError>;
    pub async fn dispatch_ctx(&self, req: CompletionRequest, ctx: DispatchContext) -> Result<DispatchResponse, RouterError>;
}
```

Or, more idiomatic with builder-style:
```rust
let resp = router.dispatch(req).with_context(ctx).await?;
```

But that requires a fluent intermediate type. The two-method form
is simpler.

**Pros**: backward compat for v0.3.x callers (still call
`dispatch(req)`); explicit opt-in for callers wanting metadata;
the newtype scales — adding `deadline_hint` is an additive struct
field with `Default::default()` for back-compat. Aligns with the
ADR-0006 §F-03 decision verbatim.

**Cons**: two methods on `Router`. Minor.

### Option D — Make `DispatchContext` mandatory (breaking change)

```rust
pub async fn dispatch(&self, req: CompletionRequest, ctx: DispatchContext) -> Result<…>;
```

Callers always provide one. Default is `DispatchContext::default()`
(all-None fields).

**Pros**: single method; uniform call site.

**Cons**: breaks all v0.3.x callers (studio-server's dispatch
route handler, the M7 multi-provider tests, etc.). v0.4.x is a
minor bump, not major — breaking the public router API requires
v1.0+. Rejected.

## Decision

**Option C** — additive `DispatchContext` newtype + `dispatch_ctx`
sibling method.

### Type definition (in `studio-router::router`)

```rust
/// Caller-supplied dispatch metadata. Optional fields; pass
/// `DispatchContext::default()` when only the LLM payload matters.
///
/// `task_tag` is recorded in the ledger entry for cost analysis.
/// Other fields are reserved for future use; v0.4.x only consumes
/// `task_tag` at the ledger boundary.
#[derive(Clone, Debug, Default)]
pub struct DispatchContext {
    /// Free-form caller-supplied tag for ledger filtering. v0.4.x
    /// validates: ≤ 256 chars, no control characters, no embedded
    /// newlines. Empty / None = `None` in the ledger.
    pub task_tag: Option<String>,

    /// Reserved for M10+ (deadline-aware retry / circuit-break).
    pub deadline_hint: Option<std::time::Duration>,

    /// Reserved for M10+ (distributed-trace correlation).
    pub span_id: Option<String>,
}
```

### Router API surface

```rust
impl Router {
    /// Existing v0.3.x signature — unchanged. Internally delegates
    /// to `dispatch_ctx(req, DispatchContext::default())`.
    pub async fn dispatch(&self, req: CompletionRequest) -> Result<DispatchResponse, RouterError> {
        self.dispatch_ctx(req, DispatchContext::default()).await
    }

    /// NEW. Pass `DispatchContext` for ledger-filterable dispatches.
    pub async fn dispatch_ctx(&self, req: CompletionRequest, ctx: DispatchContext)
        -> Result<DispatchResponse, RouterError>;
}
```

### Wire format change at HTTP boundary

`POST /api/dispatch` body gains an optional `task_tag` field:

```json
{
    "model": "claude-opus-4-7",
    "messages": [...],
    "task_tag": "code-review"
}
```

When the field is absent, `task_tag` defaults to `None` (matches
v0.3.x behaviour). The studio-server `routes/dispatch.rs` handler
extracts the tag, builds a `DispatchContext`, and forwards to
`Router::dispatch_ctx`.

### Validation at the route layer

Before constructing `DispatchContext`:

```rust
fn validate_task_tag(tag: &str) -> Result<&str, RouteError> {
    if tag.len() > 256 {
        return Err(RouteError::bad_request(
            "task_tag must be ≤ 256 characters",
            "task_tag_too_long",
        ));
    }
    if tag.chars().any(|c| c.is_control()) {
        return Err(RouteError::bad_request(
            "task_tag must not contain control characters",
            "task_tag_invalid_chars",
        ));
    }
    Ok(tag)
}
```

Empty string is normalised to `None` (so callers can pass
`"task_tag": ""` and get the same behaviour as omitting it).

### Ledger integration

`studio-router::ledger::LedgerEntry` already has an
`Option<String>` slot for `task_tag` (per ADR-0006 §"Strip list" #4
— "Translation-specific ledger fields... generalize to
`task_tag: Option<String>`"). Currently always `None`. M9 plumbs
the value through:

`Router::dispatch_ctx(req, ctx)` → constructs `LedgerEntry { ...
task_tag: ctx.task_tag.clone(), ... }` → existing
`Ledger::record` call.

### SvelteKit frontend integration (optional in M9)

The `/agent` page's "submit a prompt" form gains an optional
"Task tag" text field. The field defaults empty (no tag sent).
If the user types one, it's included in the POST body. Ledger
page filtering by tag is a future SvelteKit enhancement, not
shipped in M9.

## Done means (falsifiable success criteria)

1. **Unit tests** in `studio-router/src/router.rs::tests`:
   - `dispatch_default_context_matches_legacy_dispatch` —
     `dispatch_ctx(req, Default)` produces identical
     `LedgerEntry.task_tag = None` to `dispatch(req)`.
   - `dispatch_ctx_task_tag_flows_to_ledger` — pass `ctx`
     with `task_tag: Some("code-review")` → ledger entry has
     `task_tag: Some("code-review")`.

2. **Integration tests** in `crates/studio-server/tests/
   dispatch_task_tag.rs` (NEW):
   - `dispatch_without_task_tag_works_v0_3_x_compat` — POST
     body without `task_tag` → 200 + ledger entry has tag=None.
   - `dispatch_with_task_tag_records_in_ledger` — POST body with
     `task_tag: "code-review"` → 200 + ledger entry has the tag.
   - `task_tag_too_long_returns_400` — 257-char tag → 400
     `task_tag_too_long`.
   - `task_tag_with_newline_returns_400` — embedded `\n` → 400
     `task_tag_invalid_chars`.
   - `task_tag_empty_string_normalises_to_none` — `""` → 200 +
     ledger tag=None.

3. **Doc-coverage 7-gate stays green**:
   - `docs/agent/modules/studio-router.md` updates for
     `DispatchContext` + `dispatch_ctx` public surface.
   - `docs/agent/modules/studio-server.md` updates for the
     `task_tag` field at the HTTP boundary.
   - zh/en human-track parity preserved.

4. **CHANGELOG entry** for v0.4.0:
   - References ADR-0010 + closes ADR-0006 §F-03.

5. **README** — the design-partner-friction priority list item
   for `task_tag` plumbing (#6 today) gets crossed off.

## Phase plan (per ADSD §"Two-phase dispatch SOP")

**Phase 1 (this commit, CTO solo)**:
- This ADR landed.
- No test skeleton required — the Phase 2 deliverable list is
  short enough (~5 unit + integration tests, plumb-through code)
  that a P9 dispatch is direct.

**Phase 2 (P9 dispatch, ~60-90 min sonnet/opus — small scope)**:
- Worktree: `feature/m9-dispatch-context`.
- Deliverables: `DispatchContext` struct + `dispatch_ctx` method
  + route-layer validation + LedgerEntry plumbing + 2 unit + 5
  integration tests + 2 module-doc updates + zh/en human-track +
  README cross-off + CHANGELOG entry.
- 7-gate green; CTO 守闸; merge --no-ff.

## Consequences

- **Enables**: ledger filtering by task type (user-dogfooder cost
  analysis). Closes ADR-0006 §F-03 deferred item.
- **Enables**: future M10+ work to extend `DispatchContext` with
  deadline / span / trace fields without breaking the public API.
- **Forecloses**: Option A (request-struct pollution) for M10+
  metadata. Future fields go on `DispatchContext`, not
  `CompletionRequest`.
- **Migration**: zero — `dispatch(req)` continues to work
  identically. `task_tag: null` or omitted in JSON body matches
  v0.3.x exact behaviour. **No breaking change.**
- **Performance**: ~16 bytes per dispatch for the optional tag
  copy in the ledger entry. Negligible.

## Cross-references

- ADR-0001 (stack — async tokio)
- ADR-0006 §F-03 addendum (originating deferred decision; M9
  closes it)
- ADR-0006 §"Strip list" #4 (`task_tag: Option<String>` already
  in `LedgerEntry`; M9 wires the source)
- src: `crates/studio-router/src/router.rs` (add DispatchContext +
  dispatch_ctx method)
- src: `crates/studio-router/src/ledger.rs` (already has
  task_tag Option<String>)
- src: `crates/studio-server/src/routes/dispatch.rs` (extract
  + validate at HTTP boundary)
- `docs/agent/roadmap-v0.4.x.md` §"v0.4.x candidates" #1
- user 2026-05-12 evening directive: prioritise user-dogfooder
  friction (this is one)
