---
doc_kind: module
module_id: studio-store
last_verified_commit: HEAD
dependencies: [adr:0003, adr:0004, adr:0006]
---

# Module: studio-store

## Purpose

Persistence layer per ADR-0004. Markdown files under `docs/agent/{adr,
findings}/` are **source-of-truth**; SQLite at
`<project_root>/.cobrust-studio/studio.db` is a **materialized index +
ledger view + encrypted session k/v**.

Per ADR-0006 §"Addendum 2026-05-11" F-02, the JSONL ledger written by
`studio_router::Ledger` is the source of truth for dispatch records; the
SQLite `ledger_view` table is a materialized view this crate maintains.

## Public surface (M1 — verified against `crates/studio-store/src/lib.rs`)

Aggregate entry point:

```rust
let store = studio_store::Store::open(project_root).await?;
```

Side effects of `Store::open`: creates `<root>/.cobrust-studio/`,
`docs/agent/{adr,findings}/`; opens `studio.db` in WAL mode; runs the
idempotent schema migration (4 tables); walks the markdown dirs and
repopulates the index.

Sub-handles (cheap to construct, borrow `&Store`):

```rust
// ADR markdown CRUD + watcher
store.adr().list().await                     -> Vec<AdrSummary>
store.adr().get(adr_id).await                -> Option<Adr>
store.adr().create(AdrDraft { .. }).await    -> Adr
store.adr().watch()                          -> impl Stream<Item = AdrChangeEvent>
store.adr().reindex().await                  -> ()   // cold-start re-walk

// Finding markdown CRUD + watcher (symmetric)
store.finding().list / get / create / watch / reindex

// Ledger materialized view (reader-only, per F-02)
store.ledger().append(&LedgerEntry).await    -> ()   // writes SQLite view only
store.ledger().recent(n).await               -> Vec<LedgerEntry>
store.ledger().count().await                 -> u64
store.ledger().sync_from_jsonl(path).await   -> usize  // truncate + re-import
store.ledger().clear().await                 -> ()

// Session encrypted-blob k/v
store.session().set_endpoint(EncryptedBlob).await   -> ()
store.session().get_endpoint().await                -> Option<EncryptedBlob>
store.session().set(key, &blob).await
store.session().get(key).await
store.session().remove(key).await
```

Types exported at crate root:

```rust
pub use studio_store::{
    Store, StoreError, DATA_DIR_NAME, DB_FILE_NAME, ADR_DIR, FINDING_DIR,
    Adr, AdrSummary, AdrDraft, AdrChangeEvent,
    Finding, FindingSummary, FindingDraft, FindingChangeEvent,
    LedgerEntry, Outcome,    // re-exports from studio_router::ledger
    EncryptedBlob,
};
```

### Type sketches

```rust
pub struct AdrSummary { adr_id, title, status, date: String, path: PathBuf }
pub struct Adr        { summary: AdrSummary, body: String,
                        supersedes, superseded_by: Vec<String> }
pub struct AdrDraft   { title, status, date, body: String,
                        adr_id: Option<String>, supersedes: Vec<String> }
pub enum   AdrChangeEvent { Added(PathBuf), Modified(PathBuf), Removed(PathBuf) }

// (Finding* symmetric; FindingSummary.date carries last_verified_commit
//  since findings don't always have a separate date field on-disk.)

// Re-exports from studio-router — identical wire shape, no shadow type.
pub use studio_router::ledger::{LedgerEntry, Outcome};

pub struct EncryptedBlob { ciphertext: Vec<u8>, nonce: Vec<u8>, scheme: String }
```

## Internal architecture (M1)

- `adr.rs` — markdown CRUD + frontmatter parser. `parse_adr(path, text)`
  is public so the watcher can re-parse on Modify without going through
  the index. Accepts both quoted `"NNNN"` and bare-numeric frontmatter
  id shapes, normalises to a canonical 4-digit string. Cold-start
  `reindex` soft-fails on malformed files (logs + skips).
- `finding.rs` — symmetric, with a title extractor that scans for the
  first `# ` H1 in the body and falls back to the slug when none found.
- `ledger.rs` — **reader-only** per ADR-0006 addendum F-02.
  `append(entry)` writes to the SQLite view ONLY (caller pairs with
  `studio_router::Ledger::append` for the canonical JSONL write).
  `sync_from_jsonl(path)` truncates and re-imports the entire JSONL —
  the natural way to converge after the router has been writing while
  the store was offline, or in response to a `notify` Modify event on
  the JSONL file's parent.
- `session.rs` — sqlx-backed k/v of opaque `EncryptedBlob` triples.
  studio-store does NOT decrypt (per ADR-0003); the auth layer in
  studio-server holds the user passphrase and does the AEAD round-trip.
- `watch.rs` — `notify::RecommendedWatcher` → `tokio::sync::mpsc`
  channel (cap 64, `try_send` so a slow consumer can't block the
  notify thread; dropped events re-converge via `reindex` on next
  `Store::open`). Exposes both raw `(rx, WatcherHandle)` and
  `WatchStream` (impl `Stream<Item = RawEvent>`) — the stream owns its
  handle so dropping the stream shuts the watcher down.
- `error.rs` — `StoreError` (thiserror). Coarse variants — `Io`,
  `Frontmatter`, `LedgerParse`, `Sqlite`, `NotFound`, `AlreadyExists`,
  `MissingFrontmatter`, `Watcher`, `InvalidInput`. Carries the
  offending path for I/O / parse errors. `is_not_found()` exposed for
  HTTP 404 mapping in studio-server.

### SQLite schema (single revision, inlined in `Store::migrate`)

- `adr_index(adr_id PRIMARY KEY, title, status, date, path, mtime_ns)`
- `finding_index(finding_id PRIMARY KEY, title, status, date, path, mtime_ns)`
- `ledger_view(rowid INTEGER PRIMARY KEY AUTOINCREMENT, ts, task_tag,
  provider, provider_kind, model, cache_key, cache_hit, prompt_tokens,
  completion_tokens, total_tokens, latency_ms, attempt, outcome,
  error_code, raw_json)` + index on `ts DESC`. The `raw_json` column
  holds the entire JSON-encoded `LedgerEntry` so `recent(n)` round-
  trips through `serde_json::from_str` without column-by-column
  reconstruction — preserves wire-shape parity with the JSONL by
  construction.
- `session_kv(key PRIMARY KEY, value BLOB, nonce BLOB, scheme,
  updated_at)`.

## Tests (23 collocated `#[cfg(test)] mod tests`)

ADR module:
- `slugify_basic`, `normalize_id_pads` — input-canonicalisation invariants.
- `split_frontmatter_basic` / `..._missing_fence_errs` — `---` fence parser.
- `parse_adr_quoted_id` / `parse_adr_numeric_id` — both frontmatter
  shapes round-trip.
- `parse_real_adr_files_round_trip` — walks the real
  `docs/agent/adr/*.md` files in the worktree, asserts all 6 parse,
  ids padded to 4 digits, title/status non-empty.

Finding module:
- `parse_minimal_finding` — handcrafted minimal frontmatter.
- `parse_real_finding_file` — parses the real
  `a1-1-strip-2-noop-at-pin-61f2aff.md`, asserts `status: closed_by_a1.1`
  and ADR-0006 dep edge.
- `extract_title_finds_first_h1` / `extract_title_fallback_when_no_h1`.

Ledger module (the F-02 binding tests):
- `append_then_recent_round_trips` — SQLite view newest-first ordering.
- `sync_from_jsonl_imports_router_format` — writes 3 entries via
  `studio_router::Ledger::append`, imports via
  `sync_from_jsonl`, asserts count = 3 and re-sync is idempotent.
  **This is the binding test for F-02 — proves wire-shape compat with
  zero shadow types.**
- `sync_from_jsonl_missing_file_is_noop`, `recent_zero_returns_empty`.

Session module:
- `endpoint_round_trips`, `set_endpoint_overwrites`, `remove_drops_slot`.

Watch module:
- `watcher_emits_create_event_for_new_file` — end-to-end notify
  integration on macOS FSEvents (linux inotify).
- `drop_handle_closes_channel` — drop-the-handle teardown.

Top-level:
- `version_is_pkg_version`, `store_open_creates_data_dirs_and_db`,
  `store_open_is_idempotent_across_reopens`.

## Cross-references

- ADR-0003 (encrypted credential blob; `EncryptedBlob` is opaque to
  this crate by design).
- ADR-0004 (storage architecture — markdown source-of-truth + SQLite
  index).
- ADR-0006 (router public API surface) and especially §"Addendum
  2026-05-11" F-02 — the binding decision that this crate is a JSONL
  **reader** of the router's ledger.
- src: `crates/studio-store/`
- depends on: `studio-router` (for the `LedgerEntry` / `Outcome`
  re-export); router does NOT depend on this crate.
