---
doc_kind: finding
finding_id: a1-1-strip-2-noop-at-pin-61f2aff
last_verified_commit: a99d304
discovered_by: studio-p7-a1-1-opus47 during Wave A1.1 lift+strip; ratified by studio-cto-session-002-opus47 守闸
severity: P3
status: closed_by_a1.1
dependencies: [adr:0005, adr:0006]
related: []
---

# Finding: Strip #2 (ADR-0040 honest-gate hooks) is a no-op at upstream pin `61f2aff`

## Hypothesis

ADR-0006 §"Strip list" item #2 directs the A1.1 lift to remove
"ADR-0040 honest-gate hooks (L2 gate verdict typing)" from
`router.rs` + `ledger.rs`. The strip list was authored from the
Studio handoff doc's plan-time view of upstream entanglement. The
hypothesis to verify before declaring strip #2 complete:

> Symbols `L2Verdict`, `gate_verdict`, and ADR-0040 honest-gate-
> related types **exist** in `cobrust-llm-router` at pin SHA
> `61f2aff` and need to be removed during lift.

## Method

Empirical search across the pinned upstream tree
(`~/repos/cobrust-source-pin/crates/cobrust-llm-router/` at
SHA `61f2aff`, v0.1.1):

```
grep -rn "L2Verdict\|gate_verdict\|L2.*Verdict\|HonestGate" \
  ~/repos/cobrust-source-pin/crates/cobrust-llm-router/src/
```

And the broader "honest" / ADR-0040 reference search:

```
grep -rn "honest\|ADR-0040" \
  ~/repos/cobrust-source-pin/crates/cobrust-llm-router/src/
```

## Result

- `L2Verdict` / `gate_verdict` / `HonestGate`: **zero hits**.
- `honest`: one hit, a docstring in `config.rs` using the word
  descriptively (not a load-bearing type or hook).
- `ADR-0040`: zero hits in `cobrust-llm-router` source at the pin.

ADR-0040's "honest gate" surface evidently lives elsewhere in the
upstream Cobrust workspace (likely the translation pipeline crates,
not the router crate). At the pinned router-crate SHA, there is
**nothing to strip**.

The lift therefore proceeded with strip #2 as a verified no-op.
Module agent-doc (`docs/agent/modules/studio-router.md`) records
strip #2's no-op status in the strip-provenance table; `lib.rs`
module docstring includes a brief note pointing to this finding.

## Conclusion

**Actionable takeaway**: ADSD §"Atomic commits" + §"5-gate
verification" both demand that *declared* invariants get *verified*
in the same commit as the code they constrain. Strip #2 declared an
invariant ("no honest-gate hooks in studio-router"). The honest
verification was "they were never in scope at this pin"; recording
that explicitly closes the F1.0 / F19 risk class — namely, future
readers seeing strip #2 in ADR-0006 might assume there must have
been code removed, look in vain, and either re-add bogus honest-gate
machinery to "fix" what they think went missing, or distrust ADR-
0006's other strip claims.

**Forward implications**:

- If a future pin bump (e.g. `61f2aff` → `v0.1.x`) brings new
  honest-gate-related code into `cobrust-llm-router/src/`, strip
  #2 becomes a real strip again. The pin-bump procedure outlined
  in ADR-0006 §"Lift provenance" (run `git diff <old>..<new> --
  crates/cobrust-llm-router/`) catches this — any non-empty diff
  retriggers strip review before merging the bump.
- The `strip_invariants.rs` integration test already includes a
  static check that `studio_router::L2Verdict` etc. do not exist
  as public symbols (commented-out for now since they were never
  there). Uncommenting would surface re-introduction immediately.

## Cross-references

- ADR-0005 (router lift)
- ADR-0006 (strip list — §"Strip list" item #2)
- Upstream pin: `~/repos/cobrust-source-pin/` @ `61f2aff` (v0.1.1)
- ADSD §F1 (Sediment family — declared invariants must be
  verified, not assumed)
- ADSD §F19 (Release-readiness untested — declared empty must be
  observed empty)
- src note: `crates/studio-router/src/lib.rs` module docstring
- module doc: `docs/agent/modules/studio-router.md` §"Strip
  provenance" row #2
