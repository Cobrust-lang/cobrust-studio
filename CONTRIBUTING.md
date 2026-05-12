# Contributing to Cobrust Studio

This project follows [ADSD](https://github.com/Cobrust-lang/agent-driven-development)
methodology — agent-driven software development. Read the methodology link
first if you haven't; the discipline encoded here is non-obvious to a first
reader.

## Before you start

1. Read [`CLAUDE.md`](CLAUDE.md) — the project's constitution. It's short
   (~150 lines). Engineering bar (no `.unwrap()` in non-test code;
   `expect("rationale")` instead), doc bar (zh + en + agent-track in every
   change), merge bar (5 gates green, no exceptions).
2. Skim [`docs/agent/adr/`](docs/agent/adr/) — 6 ADRs, ~150 lines each. These
   are the binding architectural decisions. Don't re-litigate them in a PR
   without an ADR amendment.
3. Skim [`docs/agent/findings/`](docs/agent/findings/) — 4 findings to date.
   Negative results / bug postmortems. Use them as evidence for why a
   specific failure mode is real.

## How to make a change

### Tiny change (typo, comment fix, doc clarification)

PR with a [conventional commit](https://www.conventionalcommits.org/) + the
Tx tag (`docs(server): fix typo in dispatch handler comment`). One commit,
no ADR needed.

### Small change (single-crate code fix, single-test addition)

PR with conventional commits. Each commit should be atomic: code + test +
doc change for one logical thing.

### Medium change (new feature, cross-crate refactor)

1. Open an issue describing the design tradeoff first.
2. Wait for sign-off on which option you're picking.
3. PR with at minimum: ADR file in `docs/agent/adr/NNNN-*.md`, the code,
   the tests, the module-doc update with `last_verified_commit:` stamped
   to your branch HEAD.

### Large change (new milestone, breaking API surface, license change)

Irreversible territory. Issue first, design-partner consultation, then ADR
Phase 1 spike, then Phase 2 implementation per
[ADSD §"Two-phase dispatch SOP"](https://github.com/Cobrust-lang/agent-driven-development/blob/main/plugins/adsd/skills/agent-driven-development/SKILL.md).

## The 6-gate verification you MUST pass locally

```bash
source $HOME/.cargo/env  # ensure cargo on PATH for §6
cargo build --workspace --all-targets --locked
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked --no-fail-fast
bash scripts/doc-coverage.sh  # runs 6 sub-gates incl. cargo test exit-code + FAILED-grep
```

Plus frontend (if `web/` touched):

```bash
cd web
pnpm install --frozen-lockfile
pnpm run check
pnpm run test:unit
pnpm run build
```

CI matrix re-runs all of the above on Linux + macOS (+ Windows for build).
PR fails to merge if any sub-gate is red.

## Findings: when to file one

Per [`CLAUDE.md`](CLAUDE.md) §3.2 + ADSD §"Honest fail acceptance":

- A benchmark surprise (you measured X, expected Y, X ≠ Y) → finding
- A bug postmortem (something broke; here's why; here's the fix; here's
  the regression test) → finding
- A negative result (you tried approach A, it didn't work, here's the
  evidence + what you'd try next) → finding
- A failure-mode catch (Studio's own discipline was leaking; here's the
  leak + the script-level fix) → finding

Findings are first-class deliverables, not afterthoughts. The CTO 守闸
publishes findings in the same atomic commit as the fix.

## Code style

- `cargo fmt --all` settles it. Don't argue with rustfmt.
- `clippy::pedantic` is `warn` workspace-wide. `unwrap_used`, `todo`,
  `dbg_macro` all `warn`.
- TypeScript: `pnpm run check` + Prettier-default.
- Commit messages: `<type>(<scope>): <Tx-tag if applicable> <description>`.

## License of contributions

By submitting a PR you agree to dual-license your contribution under
Apache-2.0 OR MIT (same as the rest of the project, per ADR-0001).

## Where to ask

- Bug: GitHub issue with the `bug-report` template
- Design-partner inquiry: GitHub issue with the `design-partner` template
- Methodology question: ADSD repo, not here. https://github.com/Cobrust-lang/agent-driven-development

## What we won't accept

- PRs that disable existing 6-gate checks without an ADR amendment
- PRs that introduce `.unwrap()` outside `#[cfg(test)]` blocks
- PRs that ship a new module-doc with `last_verified_commit: HEAD`
  placeholder (doc-coverage §5 will fail the merge anyway)
- PRs that add a finding without an attached fix + regression test
- PRs from agents whose commits sign as plain "review-claude" without
  session-ID suffix (F21 identity hygiene)
