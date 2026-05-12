---
doc_kind: index
---

# Architecture Decision Records

Every decision affecting two or more files is documented here. Adding,
mutating, or superseding an ADR is itself a code change and ships in
the same atomic commit as the change it justifies.

## Status legend

- `proposed` — under discussion or queued for Phase 2 implementation
- `accepted` — current truth
- `superseded` — replaced (see `superseded_by`)
- `deprecated` — wound down

## Index

| ADR | Title | Status | Date |
|---|---|---|---|
| [0001](0001-stack-choice.md) | Stack choice: Rust + Axum + SvelteKit + shadcn-svelte + SQLite | accepted | 2026-05-11 |
| [0002](0002-single-binary-deployment.md) | Single-binary deployment via rust-embed | accepted | 2026-05-11 |
| [0003](0003-auth-endpoint-first.md) | Auth: custom-endpoint-first, OAuth deferred to M5 | accepted | 2026-05-11 |
| [0004](0004-storage-sqlite-filesystem.md) | Storage: SQLite + filesystem markdown for ADRs/findings | accepted | 2026-05-11 |
| [0005](0005-runner-router-lift.md) | Agent runner: lift cobrust-llm-router as studio-router | accepted | 2026-05-11 |
| [0006](0006-studio-router-api-and-lift-provenance.md) | studio-router public API surface + lift provenance pin | accepted | 2026-05-11 |
| [0007](0007-secret-storage-aead-round-trip.md) | M6 secret-storage — AEAD round-trip for /login → dispatch | accepted | 2026-05-12 |
| [0008](0008-multi-provider-login.md) | M7 multi-provider /login — Anthropic + OpenAI-compatible via the SvelteKit form | accepted | 2026-05-12 |
| [0009](0009-persistent-session-across-restart.md) | M8 persistent session across binary restart — OS keychain wrap + passphrase-file fallback | accepted | 2026-05-12 |
| [0010](0010-dispatch-context-task-tag.md) | M9 DispatchContext — task_tag plumbing + extensible dispatch metadata | accepted | 2026-05-12 |
| [0011](0011-i18n-zh-en-toggle.md) | M10 i18n — zh/en UI language toggle | accepted | 2026-05-12 |
| [0012](0012-agent-loop-tool-calls.md) | M11 agent-loop tool-call environment — built-in tools + iterative dispatch | proposed | 2026-05-12 |
| [0013](0013-tauri-desktop-runtime.md) | Tauri desktop runtime — desktop-first shell around the Studio UI | accepted | 2026-05-13 |
