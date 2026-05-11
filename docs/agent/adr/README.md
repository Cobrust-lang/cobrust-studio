---
doc_kind: index
---

# Architecture Decision Records

Every decision affecting two or more files is documented here. Adding,
mutating, or superseding an ADR is itself a code change and ships in
the same atomic commit as the change it justifies.

## Status legend

- `proposed` — under discussion
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
