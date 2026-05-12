---
doc_kind: finding
finding_id: f20-closure-last-verified-commit-enforcement
last_verified_commit: 58054ea
discovered_by: ADSD v1.2.1 pull integration (studio-cto-session-002-opus47, 2026-05-12)
severity: P2
status: closed_by_a3.1
dependencies: [adr:0004]
related: [a1-1-strip-2-noop-at-pin-61f2aff]
---

# Finding: F20 closure — doc-coverage gate now enforces `last_verified_commit` is a real SHA

## Hypothesis

Per ADSD v1.2.0 failure-modes-catalogue F20 (constitution mandate
without workflow alignment): the constitution rule "every doc with
persistent claims has `last_verified_commit:` set to a real SHA" was
declared in CLAUDE.md (implicitly via the ADSD methodology) but **not
enforced** by `scripts/doc-coverage.sh`. Hypothesis: any module-doc
landing with the placeholder string `HEAD` would silently pass the
gate, accumulating sediment.

## Method

Empirical: tracked the gate behaviour across A2 and A3 module-doc
landings.

- **2026-05-11 Wave A2 merge (`36651a4`)**: `docs/agent/modules/
  studio-store.md` shipped with `last_verified_commit: HEAD`.
  doc-coverage gate PASSED. External review of A2 (studio-review-
  wave-a2-opus47) caught it as P2 finding F-A2-01.
- **2026-05-11 Wave A3 merge (`d26f3ac`)**: `docs/agent/modules/
  studio-server.md` shipped with `last_verified_commit: HEAD`.
  doc-coverage gate again PASSED. Caught visually post-merge while
  reading the system-reminder file content.

Both instances are F20-textbook: the **rule** ("stamp a real SHA")
exists at the methodology layer, but the **enforcement layer** (the
gate script) never grew the corresponding check. Two consecutive
hits in a 24-hour window satisfies the §"two strikes = systemic
blind spot" trigger.

## Result

Extended `scripts/doc-coverage.sh` §5 to enforce, against every
`docs/agent/modules/*.md` and `docs/agent/findings/*.md` (excluding
the findings index):

```bash
check_last_verified() {
    grep -q "^last_verified_commit:" "$file" || fail "missing frontmatter"
    sha=$(grep "^last_verified_commit:" "$file" | head -1 | sed -E 's/^last_verified_commit:[[:space:]]*//')
    if [ "$sha" = "HEAD" ] || [ -z "$sha" ]; then
        fail "placeholder (F20: must be a real git SHA)"
    fi
    echo "$sha" | grep -qE '^[0-9a-f]{7,40}$' || fail "not a SHA shape"
}
```

Verified against current `main` HEAD `58054ea`:

```
$ bash scripts/doc-coverage.sh
doc-coverage: M0 — all crates have agent-doc
doc-coverage: M0 — zh/en doc parity
doc-coverage: M0 — ADR frontmatter sanity
doc-coverage: M0 — ADR id monotonic
doc-coverage: M0 — last_verified_commit is a real SHA (F20 enforced)
doc-coverage: all gates passed
```

All 3 module-docs (`studio-server.md` @ `d26f3ac`, `studio-store.md`
@ `36651a4`, `studio-router.md` @ `a99d304`) + this finding + the
prior A1.1 strip-2-noop finding all carry real SHAs. Gate is green
and now actively enforcing.

## Conclusion

**Actionable takeaway**: F20 instances are not solved by fixing the
symptom (stamping the SHA on the current PR). They are solved by
adding the enforcement layer in the same PR as the mandate. The
ADSD v1.2.0 catalogue F20 §"Rule of thumb" prescribes this directly:

> Every binding constitution rule must have a paired enforcement
> step in the same PR that introduces it.

This finding is the **first F20-class fix landed in Cobrust Studio**.
Mechanism is now load-bearing: any future module-doc or finding
that lands with `last_verified_commit: HEAD` will be caught by CI
on the same PR that introduces it. The placeholder pattern is dead.

**Forward implications**:

- When new ADRs gain `last_verified_commit:` frontmatter (today
  they don't — ADRs are decisions, not specs), extend the gate to
  cover them.
- The gate uses bash + grep + sed — POSIX-compatible, works on
  both macOS BSD tools and Linux GNU tools (per the §4 sed fix
  from M0 doc-coverage F1.0).
- Findings index (`README.md` of findings dir) is skipped by the
  gate since it's an aggregator, not a finding.

## Cross-references

- ADSD v1.2.1 `reference/failure-modes-catalogue.md` §F20 (the
  meta-pattern this finding closes for Cobrust Studio)
- ADSD v1.2.1 `reference/evals-first-development.md` (the positive
  form of F20 — evals as workflow enforcement of test-first rule)
- A2 review finding F-A2-01 (the first F20 instance caught)
- src: `scripts/doc-coverage.sh` §5 (the enforcement code)
- Plan §H.6 (catalog of守闸 fixes; this is Wave A3.1 follow-up)
