---
adr_id: "0012"
title: M11 agent-loop tool-call environment — built-in tools + iterative dispatch
status: proposed
date: 2026-05-12
supersedes: []
superseded_by: []
---

# ADR-0012: agent-loop tool-call environment (M11)

## Context

User dogfood feedback at commit `102198c` (2026-05-12 evening):

> "发现大问题,没有建立 Agent loop toolcall 环境"

The `/agent` page header literally reads "One-shot dispatch via the
configured LLM router." User typed *"查看当前目录位置"* expecting
the agent to investigate and answer; instead got text explaining
how to run `pwd` manually. The page is named "Agent" but the
underlying behaviour is **single prompt → single completion → done**.

This is a fundamental UX mismatch: every modern LLM-driven dev tool
(Cursor, Claude Code, OpenHands, Aider, Cline) runs an **iterative
loop** with tool calls — read files, execute shell, write code,
observe results, iterate. Studio's "Agent" page does not. Two
historical reasons:

1. **CLAUDE.md §1 MVP scope** listed "Direct LLM agent runner (lifts
   `cobrust-llm-router`)" as in-scope but explicitly deferred:
   - "MCP-based tool calls"
   - "Claude Code / OpenHands / Codex runner adapters"

   So the MVP intentionally shipped dispatch-only.

2. The `studio-router` crate lifted from `cobrust-llm-router` is a
   single-shot dispatch primitive. The router does not loop; that's
   the caller's responsibility.

The user is now dogfooding Studio against their own projects and
the `/agent` page is the most directly user-visible page. The
mismatch between "Agent" labelling and dispatch-only behaviour is
the highest-friction item in the current product.

## Hard constraints from prior ADRs

- **ADR-0001** — async tokio, Rust 2024, no `.unwrap()` outside tests.
- **ADR-0004** — SQLite + filesystem; all tool execution state is
  ledger-recordable.
- **ADR-0006 §F-03** — `task_tag` already plumbs through; M11 adds
  per-iteration `task_tag = "agent-turn-N"` for ledger filtering.
- **ADR-0007 / 0008** — credentials flow through `SessionKey` +
  `provider_kind`. M11 reuses; no auth-layer changes.
- **CLAUDE.md §3.2 Scientific** — every dispatch ledgered; M11
  ledgers each iteration of the loop, not just the outer turn.
- **CLAUDE.md §1 single-user / single-project** — M11 tool sandbox
  is the `--project` root; no global filesystem access.

## Threat model (M11-specific)

The biggest new attack surface is **arbitrary command execution
suggested by the LLM**. If a malicious endpoint (compromised
upstream / prompt injection via ADR content) returns a tool_use
block requesting `shell.exec("rm -rf ~")`, Studio must NOT execute
it without a per-tool-class safety policy.

| Threat | Defence |
|---|---|
| LLM produces destructive shell command | Tools split into **read-only** (default-enabled) vs **write/exec** (opt-in via `--enable-write-tools`). Destructive ops require explicit user-side flag at boot. |
| Path traversal: tool reads outside `--project` | All `fs.*` tools resolve paths to absolute, then check they live under the configured `--project` root. Reject otherwise. |
| Prompt injection from ADR markdown asks the LLM to call dangerous tools | Same defence as #1; cannot circumvent the write-tools-disabled-by-default flag at runtime. |
| Resource exhaustion (LLM loops forever) | Hard cap: `agent_loop.max_iterations = 16` per `/api/agent-turn`. Configurable per request (≤ 32). |
| Token cost runaway | Each iteration ledgers tokens; user-side cost dashboard renders cumulative. Hard cap at `max_iterations` is the safety net. |

## Options considered

### Option A — Build agent-loop into Studio (M11 = self-contained agent)

Studio hosts its own loop + tool implementations.

- Loop: req → provider.complete(req, tools) → tool_use? execute → tool_result → repeat
- Built-in tools: `fs.read`, `fs.list`, `fs.write` (opt-in), `shell.exec` (opt-in), `git.status`, `git.diff`, `project_tree`
- New `/api/agent-turn` route distinct from `/api/dispatch` (which stays single-shot for raw LLM access)

**Pros**: self-contained, no external dependencies, full Studio
ownership of the iteration trace. Ledger gets per-iteration entries.
SSE stream renders the loop in the UI naturally.

**Cons**: duplicates work other agent platforms already do (Claude
Code, OpenHands, Aider). Maintenance burden — every new tool the
ecosystem adopts (e.g. MCP servers) is internal work.

### Option B — Adapter for external agent platforms via MCP

Studio's `/agent` page POSTs to an MCP server (Claude Code SDK,
OpenHands runner, etc.) which runs the loop externally. Studio
streams the trace back for ledger + UI rendering.

**Pros**: leverages existing ecosystem; future-proof.

**Cons**: MCP is recent + the SDKs are evolving; pinning Studio's
roadmap to MCP stability is risky. Adds operational complexity
(user must run an MCP server beside Studio).

### Option C — Hybrid: built-in tools (Option A) for v0.4.x, MCP adapters (Option B) for v0.5.x+

Ship the minimum useful built-in tool set in v0.4.x to unblock
user-dogfooder friction immediately. Add MCP adapters as a v0.5.x
capability that **augments** (not replaces) the built-in tools.

**Pros**: user gets immediate value; future-proof for the
ecosystem. Built-in tools serve as the canonical reference impl;
MCP support is additive.

**Cons**: nothing significant. The maintenance burden of built-in
tools is bounded (~6-8 tools total, plain Rust).

### Option D — Defer (status quo)

Rename `/agent` to `/dispatch` to honestly label single-shot
behaviour. Wait until external integrators ask before building
agent-loop.

**Pros**: zero engineering cost.

**Cons**: user has already explicitly flagged the gap as 大问题.
Deferring contradicts the user's directive on prioritising
dogfooder friction.

## Decision

**Option C** — built-in tools for v0.4.x, MCP integration as
v0.5.x+ work.

### Loop algorithm

```
POST /api/agent-turn { messages, model, max_iterations: 16, tools_allowed: [...] }
                                         ↓
        loop:
            i = 0
            while i < max_iterations:
                resp = provider.complete(messages, tools)
                emit SSE event: { kind: "iteration", n: i, response: resp }
                if resp.stop_reason == "tool_use":
                    for tool_use_block in resp:
                        result = execute_tool(tool_use_block)
                        emit SSE event: { kind: "tool_result", tool: ..., output: result }
                        messages.append(tool_use_block + tool_result)
                    i += 1
                    continue
                else:
                    emit SSE event: { kind: "done", final: resp }
                    break
            if i == max_iterations:
                emit SSE event: { kind: "max_iterations_reached" }
```

Each iteration is its own `studio-router` dispatch → its own
ledger entry (`task_tag = "agent-turn"` parent; sub-tags like
`"agent-iter-3"` for nested attribution).

### Built-in tools (v0.4.x scope)

**Always enabled (read-only, scoped to `--project`):**

| Tool | Function | Safety |
|---|---|---|
| `fs.read(path)` | Read a file's contents | Path must resolve under `--project`; size cap 1 MiB; UTF-8 only (binary refused). |
| `fs.list(dir)` | List directory entries | Path-scope check. Hidden files (`.git/` etc.) optionally filtered. |
| `git.status` | Run `git status --porcelain` in `--project` | Read-only `git` invocation; no `git push`/`commit`. |
| `git.diff(paths?)` | Run `git diff` | Read-only. |
| `project_tree(max_depth?)` | Walk `--project` filesystem | Respect `.gitignore`; size cap 10 000 entries. |

**Opt-in via `--enable-write-tools`** (default OFF):

| Tool | Function | Safety |
|---|---|---|
| `fs.write(path, content)` | Write/overwrite a file | Path-scope check; size cap 1 MiB. |
| `fs.delete(path)` | Delete a file | Path-scope; refuse if path is a git tracked file's only copy (defence in depth). |
| `shell.exec(cmd, cwd?, timeout?)` | Run a shell command | `cwd` defaults to `--project`; reject `cd ..` escapes. Timeout default 30s, max 5 min. No `sudo`. |

**Out of scope for v0.4.x** (defer to v0.5.x):

- Network access tools (`http.get`, `http.post`) — security review needed
- Long-running background tasks (`shell.exec --background`)
- Multi-file atomic transactions (Aider-style edit blocks)
- MCP (Model Context Protocol) external tool servers — v0.5.x+

## Multi-subagent dispatch (OpenCode-borrowed pattern)

User directive at commit `bc38a93` (2026-05-12 evening): borrow
multi-subagent patterns from OpenCode-style projects. The
single-agent loop above is the foundation; multi-subagent
dispatch sits on top.

### Pattern: orchestrator-and-subagent

The user-facing prompt goes to an **orchestrator agent** (`role:
orchestrator` in the wire format). For non-trivial tasks the
orchestrator may decide to delegate a sub-task to a **subagent**
via a built-in `agent.spawn` tool call. Each subagent runs its
own inner agent-loop with a private conversation history; results
flow back to the orchestrator as the `tool_result` for the
spawning `tool_use`. The user-visible `/agent` timeline renders
this as a nested timeline (subagent runs collapsed by default,
expandable).

This mirrors ADSD §1 P10 → P9 → P7 hierarchy: orchestrator =
P9 tech-lead, subagents = P7 engineers. Studio's project-
management role surfaces naturally.

### New built-in tool: `agent.spawn`

| Tool | Function | Safety |
|---|---|---|
| `agent.spawn` | Spawn a subagent with `{system, model, prompt, tools_allowed, max_iterations, label}`. Returns subagent's final text + ledger ref. | Hard cap on depth (≤ 2 — orchestrator can spawn subagents but subagents cannot spawn sub-subagents in v0.4.x). Hard cap on parallel spawns (≤ 4 per orchestrator iteration — matches ADSD §1 4-way cap). Subagent's `tools_allowed` MUST be a subset of orchestrator's. |

Per-spawn ledger entries inherit the orchestrator's `task_tag`
with a sub-tag: `task_tag = "agent-turn"`, sub-tag
`"subagent-N-iter-M"` (M9 plumbing is the killer use case).

### Subagent wire format

`agent.spawn` tool input:

```json
{
    "system": "You are a sub-task specialist for: <description>",
    "model": "claude-haiku-4-5",
    "prompt": "Read crates/studio-router/src/router.rs and report the public types",
    "tools_allowed": ["fs.read", "fs.list"],
    "max_iterations": 8,
    "label": "router-surface-survey"
}
```

Output: `{ "final_text": "...", "iterations": N, "tokens": {...}, "ledger_ref": "..." }`.

### Hierarchy constraints

- **Max depth: 2** (orchestrator → subagent; no sub-subagents).
  v0.5.x may lift to 3 once UI nesting renders cleanly.
- **Max parallel spawns: 4 per orchestrator iteration** (ADSD §1
  cap). Sequential spawns across iterations bounded only by
  orchestrator `max_iterations`.
- **Tool allow-list inheritance**: subagent's set MUST be subset
  of orchestrator's. Server validates at spawn-time.
- **Cost accounting**: per-subagent tokens roll up into the
  orchestrator's iteration ledger entry's `subagent_cost` field;
  UI cost summary shows both flat (orchestrator-only) and
  rolled-up (incl. subagents).

### Cancellation propagation

SSE client disconnect → orchestrator stops mid-iteration →
cancel signal fans out to all in-flight subagent loops.
Subagents finish their current `provider.complete` call
(cannot interrupt LLM mid-token) but skip further tool calls +
iterations.

## UI design notes (美观 + 信息量合适)

The `/agent` page rewrite is the user's most-touched surface.
Borrowing from OpenCode + Claude Code + Cursor patterns:

### Two-pane layout (desktop ≥ 1024px; stacked on mobile)

```
┌──────────────────────────────┬─────────────────────────────────┐
│ Conversation timeline        │ Right rail (collapsible)        │
│ (chronological, expandable)  │                                 │
│                              │ • Cumulative cost: $0.0024      │
│ [User]                       │ • Tokens in/out: 1,536 / 435    │
│   查看当前目录位置            │ • Iterations: 3                  │
│                              │ • Tool calls: 2 (git.status,    │
│ [Iteration 1 · gpt-5.5]      │   fs.list)                      │
│   ↳ git.status               │ • Subagents spawned: 0          │
│      [clean working tree]     │ • Elapsed: 4.2s                 │
│                              │                                 │
│ [Iteration 2 · gpt-5.5]      │ [Cancel]   [Copy ledger ID]     │
│   ↳ fs.list("/")             │                                 │
│      [Cargo.toml, src/, ...]  │                                 │
│                              │                                 │
│ [Iteration 3 · end_turn]     │                                 │
│ [Assistant]                  │                                 │
│   当前项目位于 /Users/...     │                                 │
└──────────────────────────────┴─────────────────────────────────┘
```

### Visual style guidelines

- **Each iteration = a card** with 1px subtle border + 8px
  padding. Default = 1-line summary
  (`Iter 2 · gpt-5.5 · 145 tokens · 1 tool call · 1.4s`);
  click to expand. **"信息量合适" = dense info on demand, not
  spammed by default.**
- **Tool calls = indented sub-cards** under iteration cards.
  Header = monospace tool name + duration ms; input + output
  collapsed.
- **Subagent runs = nested timeline cards** with
  `border-l-4 border-blue-500` accent. Collapsed default to
  1-line summary ("subagent X: 3 iterations, 2 tool calls,
  847 tokens, $0.0008"). Click to drill into the subagent's
  inner timeline.
- **Live counters** in right rail update as SSE events arrive.
  Smooth number-counter increments; no layout re-flow.
- **Color discipline**:
  - User messages: neutral foreground.
  - Iteration cards: subtle grey-tinted bg.
  - Tool call: monospace tool name, muted color.
  - Tool result: light green if exit_code=0; light red if
    non-zero. (Matches terminal mental model.)
  - Subagent border accent: blue. Reserved red for errored
    subagents.
  - Cancellation banner: subtle yellow.
- **No spinner animations except** during active streaming.
  Once a stream completes, spinner → static summary line. The
  timeline IS the progress indicator.
- **Keyboard shortcuts** (polish, post-v0.4.0):
  `Esc`=cancel, `j`/`k`=navigate cards, `Enter`=expand focused.

### Anti-OpenCode-bloat (NOT in v0.4.0)

- No per-iteration model picker (orchestrator's choice carries;
  subagents can override at spawn but not mid-iteration).
- No code-block syntax highlighting beyond monospace (defer
  v0.5.x — Highlight.js / Shiki adds ~200 KB bundle weight).
- No diff renderer for `fs.write` previews (defer v0.5.x;
  v0.4.0 shows raw `new_content` monospace).
- No collaborative cursors / multi-user — permanently
  out-of-scope per CLAUDE.md §1.

### Mobile

Single-column stacked. Right rail → sticky-bottom mini-bar with
cost + iteration count; tap to expand. Studio is desktop-first;
mobile is "functional, not optimised."

### Wire format

`POST /api/agent-turn` body:

```json
{
    "model": "claude-opus-4-7",
    "system": "You are a calm, precise assistant. Use tools to investigate.",
    "messages": [{ "role": "user", "content": "查看当前目录位置" }],
    "max_iterations": 16,
    "tools_allowed": ["fs.read", "fs.list", "git.status", "git.diff"],
    "task_tag": "agent-turn"
}
```

SSE response stream — same `text/event-stream` content type as
`/api/dispatch`, with extended event types:

```
event: iteration
data: { "n": 0, "tokens_in": 421, "tokens_out": 87, "stop_reason": "tool_use" }

event: tool_call
data: { "tool": "git.status", "input": {} }

event: tool_result
data: { "tool": "git.status", "output": "...", "ms": 12 }

event: iteration
data: { "n": 1, "tokens_in": 503, "tokens_out": 145, "stop_reason": "tool_use" }

event: tool_call
data: { "tool": "fs.list", "input": { "dir": "src/" } }

event: tool_result
data: { "tool": "fs.list", "output": [...] }

event: iteration
data: { "n": 2, "tokens_in": 612, "tokens_out": 203, "stop_reason": "end_turn" }

event: done
data: { "final_text": "...", "iterations": 3, "total_tokens": { "in": 1536, "out": 435 } }
```

### Provider tool-call API translation

- **Anthropic Messages API**: native tool-call support via `tools`
  param and `stop_reason: "tool_use"` response. Map directly.
- **OpenAI compat (OpenAI / vLLM / DeepSeek / Together / OpenRouter /
  Groq)**: function-calling via `tools` param with `type: "function"`.
  Translate Studio's tool def → OpenAI function schema; translate
  OpenAI `tool_calls` response → Studio's tool_call event.
- Provider-specific quirks live in `studio-router::{anthropic,openai}`
  modules. The loop in `studio-server` is provider-agnostic.

### `/agent` page UX redesign

The Svelte page renders the loop as a vertically stacked timeline:

```
┌─────────────────────────────────────────────────┐
│ User: 查看当前目录位置                         │
├─────────────────────────────────────────────────┤
│ Iteration 1 (gpt-5.5, 87 tokens)               │
│   ↳ tool call: git.status                       │
│     output: [clean working tree]                │
├─────────────────────────────────────────────────┤
│ Iteration 2 (gpt-5.5, 145 tokens)              │
│   ↳ tool call: fs.list("/")                     │
│     output: [Cargo.toml, src/, ...]             │
├─────────────────────────────────────────────────┤
│ Iteration 3 (gpt-5.5, 203 tokens — end_turn)   │
│ Assistant: 当前项目位于 /Users/.../cobrust-...  │
│           包含 Rust workspace + SvelteKit ...   │
└─────────────────────────────────────────────────┘
[Cumulative cost: $0.0024  ·  Tool calls: 2  ·  Iterations: 3]
```

The single textarea prompt UX stays — what changes is the rendered
output. Old `/api/dispatch` path continues to work for single-shot
needs; `/agent` page now POSTs to `/api/agent-turn`.

Cancel button: streaming SSE → if user clicks Stop, server gets the
disconnect signal, halts the loop mid-iteration, ledger gets a
final `cancelled: true` entry.

## Done means (falsifiable success criteria)

1. **Unit tests** in `crates/studio-server/src/agent_loop/`:
   - Path-traversal-rejection on `fs.read`/`fs.list` (input
     `../../../etc/passwd` → rejected with explicit error).
   - Max-iterations clamp at 16; >16 in request body → 400.
   - Each built-in tool round-trips: input shape → result shape.

2. **Integration test** (`crates/studio-server/tests/agent_turn.rs`,
   NEW):
   - Boot synthetic provider stub that returns: iter1 = tool_use
     git.status; iter2 = tool_use fs.list; iter3 = end_turn text.
   - POST /api/agent-turn → assert 3 iterations + 2 tool_call SSE
     events + final assistant text.
   - Cancellation: client disconnects mid-iter-2 → server logs
     cancelled, no further tool calls.

3. **E2E test** (`web/tests/e2e/agent-loop.spec.ts`, NEW): drive
   the `/agent` form, observe the timeline renders ≥ 2 iterations,
   ≥ 1 tool call, final assistant text.

4. **Safety regression tests**:
   - Without `--enable-write-tools`, request `shell.exec` →
     400 `tool_not_allowed`.
   - With `--enable-write-tools`, request `shell.exec("rm -rf /")`
     → executes in `--project` cwd (cannot escape via `cd /`); the
     `rm -rf` does NOT affect outside the project. Verify via a
     temp project root that the tool can't traverse up.

5. **Docs**:
   - `docs/agent/modules/studio-server.md` — new `agent_loop`
     submodule documented.
   - `docs/human/{zh,en}/agent-loop.md` (NEW) — user-facing
     explanation of the loop + tool list + safety model.
   - README — replace "/agent — write a prompt, submit, watch the
     SSE stream of completion chunks" with the loop-aware
     description.

6. **CHANGELOG entry** for v0.4.0:
   - References ADR-0012 + closes the user dogfood feedback gap.

## Phase plan

**Phase 1 (this commit, CTO solo)**:
- This ADR landed.

**Phase 2 (P9 dispatch, ~180-240 min Opus 4.7 — biggest M-wave
since M0; queue post-M8 + M9 + M10 merges, OR run in parallel if
worktree surface stays disjoint)**:
- Worktree: `feature/m11-agent-loop`.
- Deliverables: `agent_loop` module + 5+3 built-in tools +
  per-provider tool-call translation + new `/api/agent-turn` route
  + SvelteKit `/agent` page rewrite + 4 categories of tests +
  3 doc updates + 1 ADR-cross-ref + CLAUDE.md §1 amendment
  (removing tool calls from "deferred" list).

## Consequences

- **Enables**: Studio becomes a real agent platform, not a
  dispatch console. User dogfood feedback closed at architectural
  layer.
- **Enables**: M9 task_tag plumbing finds its richest use case
  (per-iteration ledger entries with sub-tags).
- **Enables**: per-iteration cost dashboard (already supported by
  ledger; UI rendering is incremental work).
- **Forecloses**: claiming "no scope creep beyond MVP" — this is
  scope expansion. CLAUDE.md §1 needs an addendum noting M11's
  promotion of "tool calls" from deferred to shipped.
- **Migration**: zero — existing `/api/dispatch` route unchanged.
  New route `/api/agent-turn` is additive.
- **Bundle size**: ~150-300 LoC Rust agent_loop module + ~80 LoC
  per built-in tool × 8 tools ≈ ~800-1000 LoC server-side.
  ~150-200 LoC SvelteKit timeline component. Manageable.

## Cross-references

- ADR-0001 (stack)
- ADR-0006 §F-03 + ADR-0010 (task_tag — agent-loop is the killer
  use case)
- ADR-0007 / 0008 (credentials — unchanged)
- CLAUDE.md §1 (MVP scope — M11 amends "tool calls" from
  deferred-list to shipped-list)
- `docs/agent/roadmap-v0.4.x.md` — M11 added to "In flight" once
  Phase 2 P9 dispatch fires
- User dogfood feedback at commit `102198c` 2026-05-12 evening
  (the one-shot dispatch screenshot + "发现大问题" reaction)
