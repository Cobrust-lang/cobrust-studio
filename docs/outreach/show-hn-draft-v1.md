# Show HN draft v2 (v0.2.1 — pilot-ready posture)

Draft for posting after v0.2.1 ships all 5 platforms first-time green
(Sarah v2 pilot-gate #3 closed) + Sarah v3 verdict moved from 3 months
→ 2 months → pilot-ready. Last gate remaining (Gate #1) is the social
action this post is itself meant to trigger: a second non-author
person engaging with the project.

**Source for headline**: Mei v2 + Aleksandr v2 both independently
landed on the same shape. Sarah v3 confirmed.

---

## Headline (HN title field)

> Show HN: Cobrust Studio – self-hosted web console for AI coding agents, 9 MiB single binary

(80 chars; HN cap is ~80. Drops marketing-tier "AI-driven development"
phrasing per Mei v2; leads with concrete artifacts per Aleksandr v2.)

## URL

https://github.com/Cobrust-lang/cobrust-studio/releases/tag/v0.2.1

(Direct release link, not the repo root — so the 5-platform tarball
list is the first thing a curious skimmer sees.)

## Body (HN top comment by author — convention)

Built this in 2 days as a self-hosted control plane for AI-driven
development. The wedge:

- ADRs and findings as plain markdown in `docs/agent/` (git-native,
  SQLite is just a materialized index)
- `/api/dispatch` SSE stream against Anthropic + OpenAI-compatible
  endpoints (Anthropic, DeepSeek, vLLM, OpenRouter, Together, Groq)
- Token ledger with provider/model/cost/latency per dispatch
- A 7-gate CI script (`scripts/doc-coverage.sh`) that fails merges
  missing ADR frontmatter / placeholder `last_verified_commit:` /
  fmt drift / any FAILED test group / etc.
- AES-256-GCM + Argon2id `/login` round-trip (server-side derive,
  in-memory session key, no plaintext on disk)
- 9 MiB single Rust binary, SvelteKit 5 frontend baked in via
  rust-embed, 5-platform tarballs (linux x86_64+aarch64, macOS
  x86_64+arm64, windows x86_64) on every tag

v0.2.1 is the first multi-platform release that ships all 5 tarballs
first-time green. v0.1.0, v0.1.1, and v0.2.0 each shipped with a
known regression that the post-tag audit caught (SPA fallback bug;
stale `Cargo.lock`; seal-salt mismatch in the AEAD module). The
CHANGELOG names each by file:line and which gate missed it. The
fourth tag is the first one I'd recommend for adoption.

Methodology layer is separate: https://github.com/Cobrust-lang/agent-driven-development
(ADSD — Agent-Driven Software Development). Studio is the N=2 case
study; the doc at `case-study/cobrust-studio-experience.md` is ~1370
lines on what the methodology validated, stressed, and extended over
the 2-day build + the M5/M6 hardening cycle.

Known limitations:
- Multi-provider `/login` not yet — the form-driven path hardcodes
  `AnthropicProvider` today; OpenAI-compat resolves via the
  `studio.toml` static path. ADR-0008 spike in flight.
- Session drops on binary restart; user re-enters passphrase. v0.3.x
  ADR for OS keychain wrap pending.
- Single-user / single-project / no RBAC (intentional for MVP)
- Bus factor 1 — actively looking for 3-5 design partners

If your team runs AI-driven development at multi-agent fidelity (3+
parallel agents, ADR/finding discipline), file an issue with the
`design-partner` label.

License: Apache-2.0 + MIT dual.

---

## Anticipated objections + responses

**"Why not just Linear + git?"** Answered in README via comparison
matrix. Studio is *not* competing with Linear; it's the niche of
"ADR + finding + live dispatch + token ledger + CI gate" that Linear
doesn't cover.

**"Three known-broken tags in 24h is a red flag."** Honest: the
patch dance is real. The defense is that each regression was caught
by post-tag audit (hermetic Playwright + clean-shell probe + the
seal-salt one specifically by an e2e test that exercised the round-
trip path the unit-test corpus structurally missed), not by users-
in-production. The CHANGELOG names each by file:line. v0.2.1 is the
first one I'd tell a design partner to install. If you'd prefer a
year-old tag that hides bugs, this isn't your project.

**"Methodology-as-a-service sounds like a parody HN post."** Mei
persona's v1 verdict. Fair. The v2 framing dropped that phrase. The
actual claim: the methodology has empirical evidence (Cobrust =
N=1, Studio = N=2). The case-study doc is the falsifiable artifact.
ADSD's failure-modes catalogue at v1.2.6 includes 6 entries (F25-
F28 + F1.3/F1.4) extracted from Studio's M4/M5/M6 experience —
that's the back-port part.

**"AI-built in 2 days = trust nothing."** Look at the CHANGELOG,
the findings, the doc-coverage script, and the seal-salt-bug
postmortem in particular — that bug was caught the day of v0.2.0
release by an e2e test specifically because the unit-test corpus
had a structural blind spot (only round-tripped through the same
key, never through re-derive). The catalogue entry for that
failure mode (F1.0 declared-invariant gap) was already in the
methodology before the bug; the methodology caught the bug; the
bug shows up in the methodology's case study as the validating
data point. Either that loop convinces you or it doesn't.

**"Single contributor + 2-day old + no users = pass."** Sarah
persona v1 verdict. Hard to argue. The design-partner outreach is
the response — if you're already 80% of the way there on
ADSD-style discipline, the adoption cost is just the tool import.
Sarah v3 audit (after M6 + cross-platform-fix) verdict: "ready
for a first design-partner pilot for a team of ≤5 that's already
doing ADR discipline."

---

## Posting checklist

- [x] v0.2.1 release.yml shipped 5-platform tarballs (linux
      x86_64+aarch64, macOS x86_64+arm64, windows x86_64) at
      https://github.com/Cobrust-lang/cobrust-studio/releases/tag/v0.2.1
- [x] CI badge green on README + 13/13 CI jobs green on the v0.2.1 tag commit
- [x] No P0 finding open (verify findings/README.md)
- [x] CONTRIBUTING.md + issue templates live (design-partner.md + bug-report.md)
- [x] N=2 case study lives in the public ADSD repo
- [x] AEAD round-trip closes Mei v2 R3 + Sarah v2 pilot-gate #2
- [ ] Optional: a screenshot / 30s GIF demo of the 5 pages (Mei v2's
      only remaining ask). Nice-to-have but not blocking.

## Timing

- US morning Pacific Time (HN front page warmest 7-9am PT)
- Avoid Sundays
- Avoid major-news days
- First-comment window: be online for first 60 min to answer
  technical questions in real time

## Author bio for the post

`hakureirm` (GitHub handle). Solo maintainer. ADSD methodology
author (https://github.com/Cobrust-lang/agent-driven-development).

---

*Draft v2 prepared after the v0.2.1 5-platform-first-time-green tag
+ Sarah v3 persona audit. The post itself is the social action
that triggers pilot-gate #1 (second non-author human contributor).*
