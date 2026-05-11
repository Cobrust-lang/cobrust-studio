---
doc_kind: module
module_id: studio-store
last_verified_commit: HEAD
dependencies: [adr:0004]
---

# Module: studio-store

## Purpose

Persistence layer per ADR-0004. Markdown files in `docs/agent/{adr,
findings}/` are source-of-truth; SQLite is a materialized index +
ledger + session-state store.

## Public surface (M1 target)

- `adr::list() -> Vec<AdrSummary>`
- `adr::get(id) -> Option<Adr>`
- `adr::create(adr: AdrDraft) -> Result<Adr, StoreError>`
- `adr::watch() -> impl Stream<Item = AdrChangeEvent>`
- `finding::list/get/create/watch` — symmetric
- `ledger::append(entry: LedgerEntry) -> Result<(), StoreError>`
- `ledger::recent(n: usize) -> Vec<LedgerEntry>`
- `session::set_endpoint(blob: EncryptedBlob)`
- `session::get_endpoint() -> Option<EncryptedBlob>`

## Internal architecture (M1)

- `adr/` — markdown CRUD + frontmatter parser (serde + yaml-rust)
- `finding/` — symmetric
- `ledger/` — append-only JSONL writer + SQLite WAL view
- `session/` — sqlx-backed key/value
- `watch/` — `notify` crate for filesystem change events

## Tests

- M0: smoke test on `version()`.
- M1: ADR roundtrip (write → read → diff); ledger 100-write concurrency;
  filesystem watcher delivers change event within 100ms.

## Cross-references

- ADR-0004 (storage)
- src: `crates/studio-store/`
