# Cobrust Studio — Agent Constitution

> AI-agent project-management & monitoring console. **Login** with API endpoint
> + key (OAuth deferred). Adopts the Cobrust methodology (ADR + finding +
> bilingual docs + multi-agent waves + Tx commit tags + doc-coverage CI gate)
> from minute one. **Studio is self-hosted by Studio from Day 4.**
>
> When this document is silent, write an ADR and proceed.

---

## 0. Identity

- **Project name**: Cobrust Studio
- **One-line pitch**: AI agent team's project-management control plane —
  login, point at a repo, and the Cobrust methodology starts working.
- **Audience**: engineering teams running AI-driven development at the
  ADR/finding/wave fidelity Cobrust introduced.
- **License**: Apache-2.0 + MIT dual (ADR-0001 binding).
- **Relationship to Cobrust**: independent repo. Re-uses
  `cobrust-llm-router` design (lifted into `studio-router` crate, see
  ADR-0005). Not coupled to the Cobrust language.

## 1. MVP scope (5-day target)

The 5-day MVP ships:

- Custom endpoint + API key auth (no OAuth)
- Single-project mode (one git repo per Studio instance)
- Single-user (no RBAC, no multi-tenant)
- ADR / finding / ledger storage on filesystem + SQLite index
- Direct LLM agent runner (lifts `cobrust-llm-router`)
- Live dispatch stream (SSE)
- SvelteKit UI: login / project / adr / agent / finding / ledger

Explicitly deferred to post-MVP:

- OAuth (Anthropic / OpenAI / GitHub)
- Multiple projects / multiple users
- MCP-based tool calls
- Claude Code / OpenHands / Codex runner adapters
- Live Wave Kanban with drag-drop
- PR review surface

## 2. Methodology adopted from Cobrust

| Practice | How it lives in Studio |
|---|---|
| ADR-driven decision capture | `docs/agent/adr/NNNN-*.md` with frontmatter |
| Finding-driven failure capture | `docs/agent/findings/*.md` |
| Bilingual zh/en + agent-doc tracks | `docs/human/{zh,en}/` + `docs/agent/` |
| Wave-based commit batching | Tx commit tags `feat(scope): A1.3 ...` |
| Doc-coverage CI gate | `scripts/doc-coverage.sh` |
| 5 CI gates | fmt + clippy `-D warnings` + build + test + doc-coverage |
| External review cycle | review-claude template in `.github/` |

## 3. Engineering standards

### 3.1 Elegant

- Public APIs use newtypes where invariants exist.
- No `.unwrap()` in non-test code; use `.expect("rationale")`.
- Default visibility is private; `pub` is opt-in.
- No struct has more than 7 public fields; document if it must.

### 3.2 Scientific

- Every design decision lives in `docs/agent/adr/NNNN-*.md`.
- Every benchmark is reproducible: scripted, seeded, hardware-tagged.
- Negative results are documented under `docs/agent/findings/`.

### 3.3 Efficient

- Single binary deployment (rust-embed for web assets — ADR-0003).
- SSE not WebSocket for one-way streams (simpler, lower overhead).
- SQLite + filesystem (no Postgres in MVP).
- No allocation inside hot paths if it can be avoided.

## 4. Style tokens

- Identifiers: `snake_case` for values, `UpperCamelCase` for types.
- File names: `snake_case.rs`.
- Commit messages: conventional commits + Tx tag.
  Example: `feat(server): A1.4 wire SSE dispatch route (Wave A1)`
- Errors: `thiserror` on the Rust side; structured JSON over the wire.
- Tests: collocated `#[cfg(test)] mod tests`; integration in `tests/`.
- No `TODO` without an issue link.

## 5. Operating instructions for agents

- **Default to proceed.** When a decision is reversible and within this
  constitution, make it and document via ADR. Don't ask.
- **Ask only for irreversible decisions** — license, name conflicts,
  public API freezes, breaking changes after v0.1.0.
- **When you write code, write all three doc tracks (zh/en/agent) in
  the same change.** CI doc-coverage enforces.
- **Every Tx is its own commit.** Wave merges are git merges of Tx
  branches.
- **5 gates green before any merge.** No exceptions.

## 6. Milestones

| M | Scope | Done means |
|---|---|---|
| M0 | scaffold + 5 ADR + 5 CI gates green | Day 1; `cargo build` + all gates pass; this file landed |
| M1 | backend MVP — Axum + routes + studio-router lift | Day 2; integration tests on all routes; SSE dispatch works |
| M2 | frontend MVP — SvelteKit + 4 pages | Day 3; full E2E flow login→ADR→dispatch→commit |
| M3 | dogfood + polish + single binary | Day 4; Studio manages its own ADRs via Studio UI |
| M4 | release v0.1.0 + demo + external reviewer invite | Day 5; tarball downloadable, ≤5min to first dispatch |

---

**End of constitution. Begin with M0.**
