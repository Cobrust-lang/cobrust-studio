---
doc_kind: roadmap
roadmap_id: v0.4.x
last_verified_commit: 45ad600
status: live
---

# Roadmap — v0.4.x

> Forward-looking enumeration of what's queued + what's deferred +
> what's explicitly NOT a goal. Maintained between minor tags.
> Update each time a v0.x.0 ships or a deferred item is reclassified.

---

## Active sequence (v0.3.x → v0.4.x)

The v0.4.x direction changed on 2026-05-13 after direct user dogfood
feedback: Studio should become **desktop-first via Tauri** while keeping
the existing SvelteKit UI and Axum REST/SSE backend. ADR-0013 is now the
binding runtime posture.

The queue is therefore resequenced around a new runtime foundation wave:

### M8 — Persistent session across binary restart (ADR-0009)

- **Status**: merged locally on `main` at `a6fed82` (`merge: M8
  persistent session across restart`). Not pushed from this checkout.
- **Why now**: user dogfoods Studio against own projects daily +
  restarts the server frequently → passphrase re-entry friction was the
  highest user-felt issue. Mei v3 also flagged it. User dogfood
  feedback 2026-05-12 evening #1 ("没有持久化存储 api 端点信息") is
  exactly this fix.
- **Scope shipped**: OS keychain wrap + 0600 plaintext file fallback.
  Opt-in `--persist-session=keychain|file` (default `none`).
- **Closes**: Sarah v3/v4 Gate B + user dogfood #1.

### M9T — Tauri desktop shell (ADR-0013)

- **Status**: landed locally on `main` at `45ad600` (`feat(desktop): add
  Tauri loopback shell for M9T`). Not pushed from this checkout.
- **Why now**: user explicitly changed product direction on 2026-05-13:
  "有战略大变化,我希望前端使用tauri来实现". Runtime and packaging assumptions
  shifted before more UI-heavy work lands.
- **Scope shipped**: independent `src-tauri/` Tauri v2 crate; existing
  `web/` SvelteKit build reused; embedded Studio server binds
  `127.0.0.1:0`; dynamic WebView opens the resolved loopback URL; the
  `cobrust-studio serve` path remains compatibility/headless mode.
- **Gate emphasis**: cold desktop bundle builds are still intentionally
  deferred until disk space is comfortable, but the locked Tauri check,
  clippy, Svelte check, server smoke, and doc gates passed locally.

### M9 — `task_tag` plumbing (ADR-0010)

- **Status**: completed locally after M9T; not pushed from this checkout.
- **Scope**: `studio_router::DispatchContext` newtype +
  `Router::dispatch_ctx` method + per-iteration ledger entries via
  `task_tag`. Wire format additive (back-compat with v0.3.0 callers).
  Route-layer validation rejects tags over 256 bytes or containing
  control characters; empty string normalises to `None`.
- **Closes**: ADR-0006 §F-03 deferred decision; user-dogfooder
  cost-by-task-type ledger filtering.

### M10 — i18n zh/en UI toggle (ADR-0011)

- **Status**: Phase 1 spike landed (`102198c`); Phase 2 P9 dispatch
  queued after M9T so the language toggle is built inside the runtime
  shell the user wants.
- **Scope**: Custom Svelte 5 store + en/zh dicts, top-right
  `[ EN | 中 ]` toggle, localStorage persistence, ~80-150 keys initial
  scope (5-page chrome only).
- **Estimated**: ~90-120 min Sonnet 4.6.
- **Closes**: User dogfood feedback 2026-05-12 evening #3 ("没有中英文切换").

### M11 — Agent-loop tool-call environment (ADR-0012)

- **Status**: Phase 1 spike landed (`b6cc0b8`); Phase 2 P9 dispatch
  queued after M9T/M9/M10 because the timeline UI and tool-call UX
  should be validated inside the desktop-first runtime.
- **Scope**: `/api/agent-turn` route + agent_loop module + 5 read-only
  built-in tools (`fs.read`, `fs.list`, `git.status`, `git.diff`,
  `project_tree`) + 3 opt-in write tools (`fs.write`, `fs.delete`,
  `shell.exec`) gated behind `--enable-write-tools` + per-provider
  tool-call API translation + SvelteKit `/agent` page rewritten as
  iteration timeline.
- **Estimated**: ~180-240 min Opus-class P9 work.
- **Closes**: User dogfood feedback 2026-05-12 evening 大问题 ("没有建立
  Agent loop toolcall 环境").
- **Scope shift**: CLAUDE.md §1 explicitly deferred "MCP-based tool
  calls" + "runner adapters" to post-MVP. M11 promotes built-in tool
  calls (NOT MCP yet — MCP stays v0.5.x+) from deferred to v0.4.x
  shipped. Phase 2 lands the CLAUDE.md §1 amendment in the same commit.

### Tag plan

v0.4.0 should become the **desktop-first foundation release**: M8
persistent session + M9T Tauri shell are the minimum coherent cut. M9
may ride the same tag if it remains low-risk after the shell lands.
M10/M11 stay in v0.4.x but should not block the first desktop app bundle.

Release readiness now has two surfaces:

1. `cobrust-studio serve` tarballs remain green for headless/server use.
2. Tauri desktop bundles need their own matrix and smoke test.

Both surfaces must preserve the same REST/SSE contract until ADR-0012
stabilises the agent-loop protocol.

---

## v0.4.x candidates (autonomous-safe, not currently scheduled)

Ordered by user-dogfooder ROI after the active sequence above:

### 1. `wrong_passphrase` guard cleanup (Aleksandr v3 P3 #2 deferred)

- **What**: `login.rs` discards the decrypted `EndpointSecret` from the
  wrong-passphrase guard's `open()` call. Could reuse to detect
  "credentials unchanged on re-login" vs "rotation".
- **Why**: minor optimization + slight forward-compat improvement for
  any future fields the EndpointSecret gains. Not high-impact.
- **Scope**: tiny. ~10-20 LoC + one new test.

### 2. `zeroize` on `SessionKey` + `EndpointSecret` plaintext (Aleksandr v3 P2 deferred)

- **What**: M8 already adds `zeroize` workspace dep for the persist-
  backend passphrase string. Extend to `SessionKey.key` (`[u8; 32]`)
  and any `EndpointSecret` field holding plaintext.
- **Why**: Aleksandr v3 said "out-of-scope per ADR-0007 threat model"
  but with M8 already pulling the dep, marginal cost is zero and
  defense-in-depth is positive. Bumps Studio toward OWASP L2 key
  hygiene.
- **Scope**: ~20-30 LoC, no behavioural change.

### 3. Screenshots / GIF demo of 5 pages (Mei v2+v3 ask)

- **What**: One screenshot of `/adr` list page would be the highest-
  signal. The README is unusually text-dense for a UI product.
- **Why**: trust-builder for any first-time-user. Mei v2 + v3 both
  flagged this as the single biggest README polish missing.
- **Scope**: user can capture this in 30s with macOS screencap (the
  agent cannot capture UI directly inside the harness).
- **Blocker**: only the user can produce this — autonomous CTO cannot
  replace this work item.

### 4. `守闸` row in README vocabulary table — trim defensive phrasing (Mei v3 nit P4)

- **What**: Mei v3 said the "not to gatekeep" disclaimer reads slightly
  defensive. Replace with the term + English gloss only.
- **Scope**: 2-line README edit.

---

## v0.5.x+ candidates (longer horizon, may need P10 strategic call)

- **OAuth** — ADR-0003 §"Decision" deferred to v0.5.0. Anthropic OAuth
  + GitHub OAuth + OpenAI OAuth would unlock a "no passphrase needed at
  first launch" UX. Need P10 call on scope (which providers, what
  fallback when OAuth refresh fails).
- **Multi-provider parallel dispatch** — today dispatch goes to exactly
  one provider per call. Hedge-mode (send to N providers, return
  fastest) would be a new architectural primitive. Out of scope for
  v0.4.x; tracking.
- **Search across ADRs + findings** — currently the UI lists +
  detail-views per item. A `/search` page with full-text query across
  `docs/agent/{adr,findings}/` markdown is user-visible value once the
  dataset grows (post 20+ ADRs). Studio's own repo now has 13 ADRs;
  useful demo at this size.
- **Custom protocol / direct Tauri commands** — ADR-0013 deliberately
  keeps HTTP/SSE for v0.4.x. Reconsider after ADR-0012 stabilises the
  agent-loop protocol and the API boundary is no longer moving.
- **`/api/version` for the SvelteKit `/login` footer** — already shipped
  at v0.3.x polish (commit `c233367`). Listed here only for
  completeness; future work should not regress this.

---

## Explicitly NOT goals (out of scope unless P10 reverses)

These are guard-rails for autonomous CTO work — do not spike ADRs for
these without explicit user direction:

- **Multi-user / RBAC / multi-tenancy** — CLAUDE.md §1 hard-binds
  single-user / single-project / no RBAC for the MVP era. Sarah v4
  named this as a 200-person-team blocker, but user 2026-05-12 evening
  directive explicitly said design-partner adoption is not a project
  goal. Multi-user remains a non-goal for v0.4.x.
- **Database other than SQLite** — ADR-0004 binds SQLite + filesystem;
  Postgres / cloud DB is non-goal for the single-binary MVP.
- **Design-partner outreach** — Show HN posting, DM invites, case-study
  recruitment are P10 social actions the user has explicitly
  deprioritised. Studio's value-prop is documented; the rest is the
  user's call.
- **N=3 external case study** — depends on a non-author team adopting
  Studio + writing a postmortem. Cannot be driven by autonomous CTO;
  tracked but not pursued.

---

## How to read this doc

- **Active sequence**: merged, in-progress, or explicitly queued for
  the current v0.4.x direction.
- **Candidates**: enumerated, may be picked up next; ordered by ROI for
  user-dogfooder.
- **Out of scope**: do NOT spike these without explicit P10 direction.

Items move between categories as user direction shifts. Each minor tag
(v0.4.0, v0.5.0, ...) should refresh this doc to reflect current state.

---

## Cross-references

- `CLAUDE.md` §1 (MVP scope hard-bindings + ADR-0013 runtime posture)
- `docs/agent/adr/0007-secret-storage-aead-round-trip.md` (M6 AEAD
  round-trip; M8 builds on this)
- `docs/agent/adr/0008-multi-provider-login.md` (M7 multi-provider;
  shipped at v0.3.0)
- `docs/agent/adr/0009-persistent-session-across-restart.md` (M8;
  merged locally at `a6fed82`)
- `docs/agent/adr/0010-dispatch-context-task-tag.md` (M9 task_tag)
- `docs/agent/adr/0011-i18n-zh-en-toggle.md` (M10 i18n)
- `docs/agent/adr/0012-agent-loop-tool-calls.md` (M11 tool calls)
- `docs/agent/adr/0013-tauri-desktop-runtime.md` (M9T Tauri runtime)
- `CHANGELOG.md` for what actually shipped per tag
- Sarah v1-v4 / Aleksandr v1-v3 / Mei v1-v3 audit reports — inform the
  deferred-items list (the reports themselves live in the agent
  transcripts, not the repo)
