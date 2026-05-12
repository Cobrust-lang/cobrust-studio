---
name: Bug report
about: Something broke; here's how + what should happen instead
title: "[bug] <one-line symptom>"
labels: ["bug"]
assignees: []
---

## Symptom

<What happened. One paragraph.>

## Expected

<What you expected to happen. One paragraph.>

## Reproducer

Minimum sequence of commands / clicks that triggers it. Ideally something
that pastes into a fresh terminal against a clean clone:

```bash
git clone https://github.com/Cobrust-lang/cobrust-studio /tmp/studio-repro
cd /tmp/studio-repro
bash scripts/build-release.sh
./target/release/cobrust-studio serve --project . --port 7878 &
# … your specific steps here …
```

## Environment

- Studio version: <output of `./target/release/cobrust-studio --version` or `git describe HEAD`>
- OS + arch: <e.g. macOS 14.5 arm64 / Ubuntu 24.04 x86_64 / Windows 11 x64>
- Rust toolchain: <output of `rustc -V`>
- Node + pnpm: <output of `node -V && pnpm -V`>
- 6-gate status: <output of `bash scripts/doc-coverage.sh 2>&1 | tail -10`>
  - Including doc-coverage §6 which runs cargo test under exit-code-aware
    enforcement (any FAILED group + any non-zero cargo exit code fails the
    gate). If §6 is red locally for you, that's relevant context.

## Severity (your read; maintainer re-triages)

- [ ] P0 — release-broken (user can't use any of the documented features)
- [ ] P1 — feature-broken (a specific page / route / capability doesn't work)
- [ ] P2 — friction (works but has UX rough edge, surprising behaviour, etc.)
- [ ] P3 — paper cut (typo, ugly formatting, etc.)

## What you'd expect in a fix

- [ ] A regression test added in the same commit (Rust unit / Playwright
      spec / doc-coverage script step — whichever closes the failure mode)
- [ ] A finding under `docs/agent/findings/<slug>.md` if this is the kind of
      failure-mode-catalogue-worthy thing (silent miscompile, F19-class
      release-readiness gap, F20-class workflow-vs-mandate drift)
- [ ] CHANGELOG entry under the next patch version

## Anything else

(Logs, screenshots, stack traces, related-issue links.)
