---
doc_kind: roadmap
roadmap_id: v0.4.x
last_verified_commit: 070b017
status: live
---

# Roadmap — v0.4.x

> Forward-looking enumeration of what's queued + what's deferred +
> what's explicitly NOT a goal. Maintained between minor tags.
> Update each time a v0.x.0 ships or a deferred item is reclassified.

---

## In flight (v0.3.x → v0.4.0)

### M8 — Persistent session across binary restart (ADR-0009)

- **Status**: Phase 2 P9 dispatch in flight on Opus 4.7
  (`aa99715d362150583`, ~180-240 min wall-clock)
- **Why now**: user dogfoods Studio against own projects daily +
  restarts the server frequently → passphrase re-entry friction is
  the highest user-felt issue. Mei v3 also flagged.
- **Scope**: OS keychain wrap (macOS Keychain / freedesktop secret-
  service / Windows DPAPI via `keyring` crate) + 0600 passphrase
  file fallback for environments without keychain access (Docker,
  systemd, etc.). Opt-in via `--persist-session=keychain|file`
  (default: `none`, preserves v0.3.0 behaviour).
- **Closes**: Sarah v3/v4 Gate B; design-partner priority list item
  #4 from README.

---

## v0.4.x candidates (autonomous-safe, not currently scheduled)

Ordered by user-dogfooder ROI:

### 1. `task_tag` plumbing through `CompletionRequest` (ADR-0006 §F-03)

- **What**: Let the caller tag each dispatch with a string (e.g.
  `"code-review"`, `"doc-write"`, `"test-gen"`) that flows through
  to the ledger entry. Enables ledger filtering / cost analysis by
  task type.
- **Why for user-dogfooder**: when running Studio daily across
  multiple work-streams, knowing "I spent $X on doc generation vs
  $Y on tests" is the kind of feedback the ledger doesn't surface
  today (`task_tag` defaults to `None` in v0.3.0).
- **Scope**: small. ADR-0006 §F-03 already names option (c)
  `DispatchContext` newtype as the binding choice. Plumb through
  CompletionRequest → router → ledger entry. ~80-120 LoC.
- **ADR**: ADR-0010 spike pending (CTO Phase 1 not yet written).

### 2. `wrong_passphrase` guard cleanup (Aleksandr v3 P3 #2 deferred)

- **What**: `login.rs` discards the decrypted `EndpointSecret` from
  the wrong-passphrase guard's `open()` call. Could reuse to detect
  "credentials unchanged on re-login" vs "rotation".
- **Why**: minor optimization + slight forward-compat improvement
  for any future fields the EndpointSecret gains. Not high-impact.
- **Scope**: tiny. ~10-20 LoC + one new test.

### 3. `zeroize` on `SessionKey` + `EndpointSecret` plaintext (Aleksandr v3 P2 deferred)

- **What**: M8 already adds `zeroize` workspace dep for the persist-
  backend passphrase string. Extend to `SessionKey.key` ([u8; 32])
  and any `EndpointSecret` field holding plaintext.
- **Why**: Aleksandr v3 said "out-of-scope per ADR-0007 threat
  model" but with M8 already pulling the dep, marginal cost is
  zero and defense-in-depth is positive. Bumps Studio toward OWASP
  L2 key hygiene.
- **Scope**: ~20-30 LoC, no behavioural change.

### 4. Screenshots / GIF demo of 5 pages (Mei v2+v3 ask)

- **What**: One screenshot of `/adr` list page would be the highest-
  signal. The README is unusually text-dense for a web UI product.
- **Why**: trust-builder for any first-time-user. Mei v2 + v3 both
  flagged this as the single biggest README polish missing.
- **Scope**: user can capture this in 30s with macOS screencap (the
  agent cannot capture UI directly inside the harness).
- **Blocker**: only the user can produce this — autonomous CTO
  cannot replace this work item.

### 5. `守闸` row in README vocabulary table — trim defensive phrasing (Mei v3 nit P4)

- **What**: Mei v3 said the "not to gatekeep" disclaimer reads
  slightly defensive. Replace with the term + English gloss only.
- **Scope**: 2-line README edit.

---

## v0.5.x+ candidates (longer horizon, may need P10 strategic call)

- **OAuth** — ADR-0003 §"Decision" deferred to v0.5.0. Anthropic
  OAuth + GitHub OAuth + OpenAI OAuth would unlock a "no
  passphrase needed at first launch" UX. Need P10 call on scope
  (which providers, what fallback when OAuth refresh fails).
- **Multi-provider parallel dispatch** — today dispatch goes to
  exactly one provider per call. Hedge-mode (send to N providers,
  return fastest) would be a new architectural primitive. Out of
  scope for v0.4.x; tracking.
- **Search across ADRs + findings** — currently the UI lists +
  detail-views per item. A `/search` page with full-text query
  across `docs/agent/{adr,findings}/` markdown is user-visible
  value once the dataset grows (post 20+ ADRs). Studio's own
  repo has 9 ADRs + 5 findings; useful demo at this size.
- **`/api/version` for the SvelteKit `/login` footer** — already
  shipped at v0.3.x polish (commit `c233367`). Listed here only
  for completeness; the M8 P9 dispatch should not regress this.

---

## Explicitly NOT goals (out of scope unless P10 reverses)

These are guard-rails for autonomous CTO work — do not spike ADRs
for these without explicit user direction:

- **Multi-user / RBAC / multi-tenancy** — CLAUDE.md §1 hard-binds
  single-user / single-project / no RBAC for the MVP era. Sarah v4
  named this as a 200-person-team blocker, but user 2026-05-12
  evening directive explicitly said design-partner adoption is not
  a project goal. Multi-user remains a non-goal for v0.4.x.
- **Database other than SQLite** — ADR-0004 binds SQLite +
  filesystem; Postgres / cloud DB is non-goal for the single-binary
  MVP.
- **Design-partner outreach** — Show HN posting, DM invites,
  case-study recruitment are P10 social actions the user has
  explicitly deprioritised. Studio's value-prop is documented;
  the rest is the user's call.
- **N=3 external case study** — depends on a non-author team
  adopting Studio + writing a postmortem. Cannot be driven by
  autonomous CTO; tracked but not pursued.

---

## How to read this doc

- **In flight**: actively being worked on (P9 dispatch / ADR spike /
  commit in progress).
- **Candidates**: enumerated, may be picked up next; ordered by ROI
  for user-dogfooder.
- **Out of scope**: do NOT spike these without explicit P10 direction.

Items move between categories as user direction shifts. Each minor
tag (v0.4.0, v0.5.0, ...) should refresh this doc to reflect
current state.

---

## Cross-references

- `CLAUDE.md` §1 (MVP scope hard-bindings)
- `docs/agent/adr/0007-secret-storage-aead-round-trip.md` (M6
  AEAD round-trip; M8 builds on this)
- `docs/agent/adr/0008-multi-provider-login.md` (M7 multi-provider;
  shipped at v0.3.0)
- `docs/agent/adr/0009-persistent-session-across-restart.md` (M8
  spike; Phase 2 P9 in flight)
- `CHANGELOG.md` for what actually shipped per tag
- Sarah v1-v4 / Aleksandr v1-v3 / Mei v1-v3 audit reports —
  inform the deferred-items list (the reports themselves live in
  the agent transcripts, not the repo)
