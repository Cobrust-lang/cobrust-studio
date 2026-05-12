# Show HN draft v1

Draft for posting once v0.1.3 release.yml completes + multi-platform
tarballs are live on the GitHub Release page.

**Source for headline**: Mei v2 + Aleksandr v2 persona reports — both
landed on the same headline shape independently.

---

## Headline (HN title field)

> Show HN: Cobrust Studio – self-hosted web console for AI coding agents, 9 MiB single binary

(80 chars; HN cap is ~80. Drops marketing-tier "AI-driven development"
phrasing per Mei v2; leads with concrete artifacts per Aleksandr v2.)

## URL

https://github.com/Cobrust-lang/cobrust-studio

## Body (HN top comment by author — convention)

Built this in 2 days as a self-hosted control plane for AI-driven
development. The wedge:

- ADRs and findings as plain markdown in `docs/agent/` (git-native,
  SQLite is just a materialized index)
- `/api/dispatch` SSE stream against Anthropic + OpenAI-compatible
  endpoints (Anthropic, DeepSeek, vLLM, OpenRouter, Together, Groq)
- Token ledger with provider/model/cost/latency per dispatch
- A 6-gate CI script (`scripts/doc-coverage.sh`) that fails merges
  missing ADR frontmatter / placeholder `last_verified_commit:` /
  any FAILED test group / etc.
- 9 MiB single Rust binary, SvelteKit 5 frontend baked in via
  rust-embed

v0.1.3 is the first multi-platform tarball release (linux x86_64 +
aarch64, macOS x86_64 + aarch64, windows x86_64). v0.1.0 and v0.1.1
both shipped known-broken — the CHANGELOG names each regression by
file:line and which CI gate missed it. The third tag is the first
usable one.

Methodology layer is separate: https://github.com/Cobrust-lang/agent-driven-development
(ADSD — Agent-Driven Software Development). Studio is the N=2 case
study; case-study doc is at the bottom of that repo's
`case-study/cobrust-studio-experience.md` (~1370 lines on what the
methodology validated, stressed, and extended over the 2-day build).

Known limitations:
- `/login` WebCrypto blob is a stub; set `ANTHROPIC_API_KEY` env var
  before launching binary (M5+ for real AEAD round-trip)
- Single-user / single-project / no RBAC (intentional for MVP)
- Bus factor 1 — looking for 3-5 design partners

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

**"Two known-broken tags in 24h is a red flag."** Honest: yes, the
patch dance is real. The defense is that the regressions were caught
by post-tag audit (hermetic Playwright + clean-shell probe), not by
users-in-production. The CHANGELOG names each by file:line. If you'd
prefer a year-old tag that hides bugs, this isn't your project.

**"Methodology-as-a-service sounds like a parody HN post."** Mei
persona's v1 verdict. Fair. The v2 framing dropped that phrase from
body copy. The actual claim: the methodology has empirical evidence
(Cobrust = N=1, Studio = N=2). The case-study doc is the falsifiable
artifact.

**"AI-built in 2 days = trust nothing."** Look at the CHANGELOG,
the findings, the doc-coverage script. Either those convince you or
they don't. The project is honest about being agent-driven; the
discipline is the differentiator.

**"Single contributor + 2-day old + no users = pass."** Sarah
persona v1 verdict. Hard to argue. The design-partner outreach is
the response — if you're already 80% of the way there on
ADSD-style discipline, the adoption cost is just the tool import.

---

## Posting checklist

- [ ] v0.1.3 release.yml completed; tarballs visible on
      https://github.com/Cobrust-lang/cobrust-studio/releases/tag/v0.1.3
- [ ] At least one tarball downloads + sha256 verifies
- [ ] CI badge green on README
- [ ] No P0 finding open (verify findings/README.md)
- [ ] CONTRIBUTING.md + issue templates live
- [ ] Optional: a screenshot / 30s GIF demo of the 5 pages (Mei v2's
      only remaining ask). Can be added in v0.1.4 if delayed.

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

*Draft prepared during M5 polish cycle. Land + verify v0.1.3
release artifacts before posting.*
