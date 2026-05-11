---
adr_id: "0004"
title: Storage — SQLite + filesystem markdown
status: accepted
date: 2026-05-11
supersedes: []
superseded_by: []
---

# ADR-0004: Storage layer

## Context

MVP needs to persist:
- ADRs (markdown content, frontmatter metadata)
- Findings (markdown content, frontmatter metadata)
- Ledger entries (LLM dispatch records)
- Session state (project path, credentials encrypted blob)
- Wave / Tx state (planned tasks, completion status)

Hard requirements:
- Zero-ops (no Postgres / Redis daemon)
- Survives Studio restart
- Markdown ADRs/findings remain editable via git/CLI without Studio
  (constitution §6 dogfood: ADRs are git-native, not DB-prisoner)

## Options considered

### Option 1 — Markdown files for content + SQLite for index/metadata

ADRs and findings live in `docs/agent/{adr,findings}/*.md` as the
**source of truth**, exactly mirroring Cobrust. SQLite maintains:
- Full-text search index
- frontmatter (adr_id, status, date) for fast list queries
- Ledger (append-only)
- Session state
- Wave / Tx state

`studio-store::adr::list()` walks the filesystem on cold start,
populates the SQLite index, watches via `notify` for live changes.

### Option 2 — SQLite as source of truth

ADRs/findings stored as DB rows. Editing requires Studio UI; CLI/git
editing is out-of-band.

### Option 3 — Pure filesystem, no DB

Markdown everywhere, walk filesystem on every query.

**Cons**: ledger needs append-only writes from concurrent agents (race
risk on filesystem); full-text search on hundreds of ADRs is slow
without an index.

## Decision

**Option 1**. Markdown as source of truth, SQLite as index.

Rationale:
1. ADRs/findings remain git-native (dogfood: Studio can manage its own
   ADRs which are also Studio's source code).
2. SQLite gives fast queries and concurrent ledger writes.
3. No daemon, single file, embedded.

`notify` crate watches `docs/agent/{adr,findings}/` for external
changes (e.g., user `git pull`s a new ADR).

## Consequences

- Enables: zero-ops install; CLI/git workflows; dogfood pattern.
- Forecloses: any feature that needs ACID across multiple ADRs (rare).
- Backup: just back up the git repo + `.cobrust-studio/studio.db`.
- Concurrency: ledger writes use SQLite WAL mode + single writer task.

## Cross-references

- ADR-0001 (stack choice — sqlx + sqlite)
- ADR-0003 (encrypted credential blob storage)
