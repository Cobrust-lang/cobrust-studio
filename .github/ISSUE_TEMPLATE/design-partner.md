---
name: Design partner inquiry
about: Tell us about your team if you'd consider being a design partner
title: "[design-partner] <your-org / your-team>"
labels: ["design-partner"]
assignees: []
---

Thanks for considering Cobrust Studio as a design partner. The §13
positioning says we're looking for 3-5 partners — concrete asks, real
adoption friction, willingness to file findings against the methodology.

The 5 sections below mirror the questions a tech-lead persona (Sarah,
from our M5 persona audit) said she'd want answered before signing
anything. Be as concrete as you can; "we're a 12-person platform team
doing AI-driven dev with Claude Code + Cursor + custom orchestrator"
beats "we're interested in ADR tooling."

---

## 1. Team profile

- Team size + composition (e.g. 14 engineers, 3 staff-eng, 1 EM)
- Current AI-driven-dev surface (Cursor / Claude Code / OpenHands /
  custom / mix)
- ADR / finding / wave discipline status: already practicing it / want
  to adopt it / never heard of it
- Existing CI gate culture: 3-gate / 5-gate / 10-gate / "if it
  compiles ship it"
- Languages / stack (we're stack-agnostic; this just helps us understand
  fit)

## 2. Current setup

- How are you tracking agent decisions today? (Jira? Wiki? Free-text PRs?
  Nothing systematic?)
- How are you tracking agent failures / dead ends today?
- How are you tracking LLM token spend per dispatch?
- What's your dispatch surface today? (Cursor chat / Claude Code CLI /
  raw API / something else)
- Multi-agent parallelism cap (1 agent / 2 / 4 / ad-hoc)

## 3. What you'd want from Studio

Pick from the list (multi-select) + add anything not covered:

- [ ] Replace ad-hoc ADR culture with the typed `docs/agent/adr/` tree
- [ ] Capture findings systematically with the typed schema
- [ ] Token ledger with provider + model + cost per dispatch
- [ ] Live SSE stream of agent dispatches across the team
- [ ] CI gate that fails merges missing ADRs / findings / `last_verified_commit`
- [ ] N=3 case-study contribution to the ADSD methodology
- [ ] Multi-user / RBAC (M6+ work; tells us your scale demands it)
- [ ] On-prem / air-gapped deployment story (the single-binary design
      supports this but it's untested at scale)
- Other:

## 4. What would block adoption today

Honest list. Some friction is already known (M2 auth stub; pre-built
tarball pipeline; single-platform). Yours might be different:

- Security review: SOC 2 / ISO 27001 / FedRAMP / corporate policy
- Compliance: audit-log requirements / data residency / encryption-at-rest
  beyond the AES-GCM-stub
- Performance: dispatch throughput floor / SSE concurrent-client cap
- Frontend gap: missing pages / specific UX paths we don't surface
- Build / deploy: needs Docker / k8s manifest / brew tap / apt package
- Other:

## 5. Permission to cite

Do you want to be cited publicly as a design partner once we ship?
(Yes / Yes after launch / No / Decide later)

If yes: name of org + your role, as you'd want it to appear in the
README §"Design partners" section.

---

I'll respond within 72 hours. If we're not a fit today, I'll say so
explicitly rather than waste your time. If we are, we'll set up a
30-min call to walk through your friction list against the current
release.
