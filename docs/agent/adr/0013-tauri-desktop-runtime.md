---
adr_id: "0013"
title: Tauri desktop runtime — desktop-first shell around the Studio UI
status: accepted
date: 2026-05-13
supersedes: []
superseded_by: []
---

# ADR-0013: Tauri desktop runtime

## Context

The v0.1.x → v0.3.x product proved the core Studio loop as a self-
hosted web console: Rust/Axum backend, SvelteKit static frontend,
SQLite/filesystem store, and release tarballs with the web bundle baked
into the binary via `rust-embed`.

User dogfood feedback on 2026-05-13 changes the product direction:

> "有战略大变化,我希望前端使用tauri来实现"

This is a CTO-level pivot because the runtime shell affects packaging,
install UX, security boundaries, keychain behavior, file-picker/project
selection, and how much web-server ceremony the user sees day-to-day.
It does **not** imply throwing away the existing SvelteKit UI: Tauri's
highest-ROI path is to reuse the current web frontend inside a desktop
WebView while moving the primary user journey from "run server, open
browser" to "open Studio.app".

Hard constraints:

- Preserve the Rust backend crates (`studio-server`, `studio-store`,
  `studio-router`) and their HTTP/SSE contract until ADR-0012 agent-loop
  tool calls land.
- Preserve the existing SvelteKit 5 UI source so M10 i18n and M11 agent
  timeline work are not rewritten.
- Keep `cobrust-studio serve` as a supported compatibility/headless mode
  for CI, dogfood automation, and remote/server deployments.
- Desktop must improve user-as-dogfooder friction: project selection,
  restart survival, local keychain integration, and no manual browser
  tab management.

## Options considered

### Option A — Keep browser-first web app; add Tauri later

Continue M9/M10/M11 exactly as planned, then wrap the finished web app
in Tauri near v0.5.0.

**Pros**: zero near-term disruption; M9/M10/M11 plans remain unchanged.

**Cons**: burns multiple frontend waves on browser assumptions that the
user has now explicitly deprioritised. Delays the highest-leverage UX
shift and risks building web-only affordances that are awkward inside a
desktop shell.

### Option B — Tauri shell + embedded loopback Studio server

Add a `src-tauri/` app that starts the existing Studio server on
`127.0.0.1:<ephemeral>` and loads the SvelteKit frontend inside the
Tauri WebView. The WebView talks to the server through the same REST/SSE
surface used today. Release artifacts become desktop apps; the existing
`cobrust-studio serve` binary remains the headless/server mode.

**Pros**:
- Reuses current SvelteKit UI, Axum routes, Playwright/Vitest corpus,
  and Rust backend crates.
- Preserves ADR-0002's single-download spirit while giving users a real
  desktop app.
- Keeps the HTTP/SSE contract stable for M9 `task_tag`, M10 i18n, and
  M11 agent-loop work.
- Lets Tauri add desktop-native affordances incrementally: project
  picker, tray, keychain integration, file dialogs, window state.

**Cons**:
- Still has a local HTTP server internally, so the app must manage random
  ports and shutdown cleanly.
- Packaging now has two release surfaces: server tarballs and desktop
  app bundles.
- Tauri CI/build requirements add platform-specific dependencies beyond
  the current Rust+Node release pipeline.

### Option C — Tauri custom protocol + direct Rust commands

Serve the SvelteKit bundle through Tauri's custom protocol and replace
REST/SSE calls with Tauri commands/events.

**Pros**: strongest desktop-native boundary; no loopback port; cleaner
long-term security model.

**Cons**: rewrites the frontend API layer and duplicates/replaces the
Axum route surface before ADR-0012 stabilises the agent-loop protocol.
This is premature and would invalidate much of the existing integration
corpus.

### Option D — Native Rust GUI rewrite

Replace SvelteKit with egui/Iced/Slint.

**Pros**: no web runtime; Rust-only stack.

**Cons**: throws away the M2 frontend, lowers UI/aesthetic ceiling, and
slows down the agent-loop UI work. Rejected.

## Decision

**Option B**. Cobrust Studio becomes **desktop-first via Tauri**, while
preserving the existing SvelteKit UI and Axum REST/SSE backend as the
internal contract.

Binding product posture:

1. Primary user journey becomes: install/open Tauri app → choose project
   → login once → use Studio without manually managing a browser tab.
2. `cobrust-studio serve` remains supported as compatibility/headless
   mode, not the default product narrative.
3. The current `web/` SvelteKit app remains the UI implementation. Tauri
   is the runtime shell and packaging layer, not a frontend rewrite.
4. The Tauri shell starts an embedded Studio server on loopback with an
   ephemeral port for v0.4.x. A custom-protocol/direct-command migration
   can be reconsidered after ADR-0012 stabilises tool-call semantics.

## Consequences

### Positive

- Aligns Studio with actual user dogfood: a local desktop control plane
  for AI-agent work, not a browser tab the user has to babysit.
- Lets M10 i18n and M11 agent-loop UI ship once inside the runtime the
  user wants.
- Improves security ergonomics: OS keychain and local project filesystem
  access are natural desktop affordances.
- Preserves all existing server/API tests and keeps web/headless mode
  available for CI and future server deployments.

### Negative

- Adds Tauri toolchain and platform packaging complexity to release CI.
- The existing 5-platform tarball streak is no longer the only release
  gate; desktop bundles need their own matrix and smoke tests.
- Local loopback must be managed carefully: bind only `127.0.0.1`, use
  ephemeral ports, shut down on app exit, and avoid leaking auth state to
  arbitrary browser contexts.

### Migration

- ADR-0001 remains valid for language/framework choice, but its runtime
  interpretation changes from browser-first SvelteKit to Tauri-hosted
  SvelteKit.
- ADR-0002 remains valid for server/headless mode; Tauri becomes the new
  primary packaging path for desktop use.
- README and module docs must distinguish `desktop app` vs `serve` mode.

## Done means

Phase 1 (this ADR):

- ADR-0013 accepted.
- CLAUDE.md notes Tauri desktop-first posture.
- v0.4.x roadmap resequenced so Tauri shell lands before major new UI
  work.

Phase 2 (P9 implementation dispatch):

- Add `src-tauri/` with Tauri v2 configuration and dev/build scripts.
- Reuse `web/` SvelteKit build output; do not rewrite the UI framework.
- Start embedded Studio server on `127.0.0.1:0`; pass resolved base URL
  to the WebView without hardcoding a public port.
- Add a minimal desktop smoke test or scripted build gate that verifies
  the app shell can load `/login` against the embedded server.
- Keep `cobrust-studio serve` tests and release path green.
- Update README + `docs/agent/modules/web-frontend.md` +
  `docs/agent/modules/studio-server.md` for dual runtime mode.

## Dispatch contract

Suggested next wave: **M9T — Tauri desktop shell**.

- DIFFICULTY-RATING: D4 (multi-runtime packaging + Rust/server lifecycle
  + frontend build pipeline + release CI impact).
- MODEL-DEV: Opus 4.7 preferred.
- MODEL-TEST: Opus 4.7 or Sonnet 4.6 with explicit Tauri build/smoke
  scope.
- Pair: yes. Spawn TEST first if feasible; otherwise DEV + external
  release-readiness reviewer in parallel because packaging failures are
  F19-class.
- Gate budget: avoid cold clean builds unless disk space is confirmed;
  reuse existing `target/` and Node cache.

## Cross-references

- ADR-0001 (stack choice)
- ADR-0002 (single-binary deployment)
- ADR-0009 (persistent session / keychain)
- ADR-0011 (i18n UI toggle)
- ADR-0012 (agent-loop tool calls)
- `web/` SvelteKit frontend
- `crates/studio-server` Axum backend
