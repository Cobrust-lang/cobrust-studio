---
doc_kind: finding
finding_id: cto-shougate-test-gate-grep-leak
last_verified_commit: 6775cce
discovered_by: studio-p7-a5-dev-opus47 (during Wave A5 dispatch — gates green on a5-dev branch but flagged 5 pre-existing test files failing on base 6775cce)
severity: P1
status: open
dependencies: []
related: [f20-closure-last-verified-commit-enforcement]
---

# Finding: CTO 守闸 test-gate grep leak — A4 merged with 9 failing tests

## Hypothesis

The CTO 守闸 protocol's "test gate green" verification used a shell
pipeline like:

```bash
cargo test --workspace --locked --no-fail-fast 2>&1 \
  | grep "^test result" | wc -l \
  | xargs -I{} echo "{} test groups all green"
```

This **counts lines that start with `test result:`** but does NOT
distinguish between `test result: ok.` and `test result: FAILED.`.
Cargo emits both shapes; both match the grep. Hypothesis: any wave
merged via this守闸 protocol may have shipped with red tests
silently.

## Method

Empirical: ran the actual 5-gate sequence on main HEAD `6775cce`
(A4.1 守闸 commit, which my own report described as "5 gates green").

```bash
cargo test --workspace --locked --no-fail-fast 2>&1 \
  | grep "FAILED"
```

## Result

**9 test failures present on `6775cce`**, hidden from守闸 grep:

| File | Failed tests |
|---|---|
| `tests/adr_routes.rs` | 4 (post_adr_malformed_body, get_adr_by_id, post_adr_then_list, post_adr_persists) |
| `tests/auth_route.rs` | 1 (set_endpoint_malformed) |
| `tests/events_route.rs` | 1 (events_sse_emits_on_adr_create) |
| `tests/finding_routes.rs` | 2 (post_finding_malformed, post_finding_then_list) |
| `tests/ledger_route.rs` | 1 (ledger_recent_n_zero) |

The failures look like API-shape drift between A4 P7 DEV's
implementation and A4 P7 TEST's contract assumptions — same class
of divergence as A2 reconcile, but uncaught because:

1. The TEST agent ran `cargo check` (compile) but not `cargo test`
   (run) before reporting `[P7-TEST-CORPUS-READY]`. (Tests using
   `tower::ServiceExt::oneshot` compile against the runtime Router,
   not internal types, so check-pass doesn't imply test-pass.)
2. The DEV agent ran `cargo test` and reported "PASS" — likely on
   a slightly different state or only on src/ collocated tests
   (37 unit), not the integration corpus that hadn't merged yet.
3. The CTO reconcile gate check used the broken grep above; counted
   22 "result" lines, called all green, merged.

**Atomic-commit invariant violated**: ADSD §"Atomic commits" says
"each commit must keep `cargo build` working" plus the implicit
"5-gate green at merge time". A4 merge `8d5475f` shipped 9 failing
tests. Subsequent A4.1守闸 `6775cce` did not fix them — it
addressed clippy / lib doc edits, did not touch the failing
integration tests.

## Conclusion

**Actionable takeaway (immediate)**:

The CTO 守闸 protocol must replace the test-result line-count check
with an explicit FAILED-grep:

```bash
# WRONG (counts both ok + FAILED as "test groups")
cargo test ... 2>&1 | grep "^test result" | wc -l

# RIGHT (explicit pass/fail accounting)
test_output=$(cargo test --workspace --locked --no-fail-fast 2>&1)
echo "$test_output" | grep -c "^test result: FAILED" \
  | xargs -I{} bash -c '[ "{}" -eq 0 ] || { echo "TEST GATE RED: {} failed groups"; exit 1; }'
echo "$test_output" | grep -E "^test result: ok\." | wc -l \
  | xargs -I{} echo "test gate: PASS — {} green groups"
```

Or simpler: rely on cargo's exit code (non-zero on any test failure)
with `set -e`. The `2>&1 | grep ... | wc -l` pipeline I used
swallowed the non-zero exit via the pipe.

**Actionable takeaway (forward)**:

1. CTO 守闸 SOP step "verify 5-gate green" must use exit-code-aware
   checks. Either propagate cargo's exit code or grep for FAILED
   explicitly.

2. P7 TEST agents must run BOTH `cargo check` AND `cargo test` (with
   acceptance that test FAIL is expected at TDD-red — but the agent
   should REPORT the failure shape, not claim "all green").

3. Same-PR enforcement: extend `scripts/doc-coverage.sh` (or a new
   pre-merge gate) to run `cargo test` and explicitly check the
   summary line. This is the F20 closure for "atomic commit
   invariant" → script-level enforcement.

**This is the second F1.0 catch in my own pipeline this session**
(first was F-A2-01: doc-coverage's BSD-sed silent failure → fixed
in M0 commit with `sed -E`). Pattern: I declare an invariant
("5 gates green") and the verification mechanism leaks the failure.
That's exactly F20 (constitution-vs-workflow alignment) applied
to the CTO 守闸 procedure itself.

## Cross-references

- F20 closure finding `f20-closure-last-verified-commit-enforcement.md`
- ADSD v1.2.1 §F20 (constitution-vs-workflow)
- ADSD §"5-gate verification" (the bar I claimed but didn't enforce)
- A4 merge `8d5475f` (the merge that shipped 9 failing tests)
- A4.1 守闸 `6775cce` (the守闸 that I claimed green but wasn't)
- Wave A5 dispatch — discovered by P7 DEV agent flagging "pre-existing
  failures on base branch" mid-flight
