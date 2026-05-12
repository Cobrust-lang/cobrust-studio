#!/usr/bin/env bash
# Cobrust Studio doc-coverage gate.
#
# Enforces the constitution's three-track doc rule:
# - Every public crate has a `docs/agent/modules/<crate>.md`
# - Every public module has zh + en entries in `docs/human/{zh,en}/`
# - Every ADR has matching frontmatter + cross-refs valid
# - Every module-doc + finding `last_verified_commit:` is a real SHA,
#   not the `HEAD` placeholder (F20 enforcement — ADSD v1.2.0
#   failure-modes-catalogue: constitution mandate without workflow
#   alignment; this gate IS the workflow that enforces the rule)
#
# Exits non-zero on missing coverage so CI fails loudly.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO_ROOT"

fail() {
    echo "doc-coverage: FAIL — $*" >&2
    exit 1
}

ok() {
    echo "doc-coverage: $*"
}

# --- 1. Each crate has an agent-doc -----------------------------------
for crate_dir in crates/*/; do
    crate=$(basename "$crate_dir")
    if [ ! -f "docs/agent/modules/${crate}.md" ]; then
        fail "missing docs/agent/modules/${crate}.md"
    fi
done
ok "M0 — all crates have agent-doc"

# --- 2. zh + en parity on top-level docs ------------------------------
for f in docs/human/zh/*.md; do
    base=$(basename "$f")
    [ -f "docs/human/en/$base" ] || fail "zh has $base, en missing"
done
for f in docs/human/en/*.md; do
    base=$(basename "$f")
    [ -f "docs/human/zh/$base" ] || fail "en has $base, zh missing"
done
ok "M0 — zh/en doc parity"

# --- 3. ADR frontmatter sanity ---------------------------------------
for adr in docs/agent/adr/0*-*.md; do
    [ -f "$adr" ] || continue
    grep -q "^adr_id:" "$adr" || fail "$adr missing adr_id frontmatter"
    grep -q "^status:" "$adr" || fail "$adr missing status frontmatter"
    grep -q "^date:" "$adr" || fail "$adr missing date frontmatter"
done
ok "M0 — ADR frontmatter sanity"

# --- 4. ADR id monotonic ----------------------------------------------
last=-1
for adr in $(ls docs/agent/adr/0*-*.md 2>/dev/null | sort); do
    n=$(basename "$adr" | sed -E 's/^([0-9]+).*/\1/' | sed -E 's/^0+//')
    [ -z "$n" ] && n=0
    if [ "$n" -le "$last" ]; then
        fail "ADR ordering broken at $adr (id=$n, prev=$last)"
    fi
    last=$n
done
ok "M0 — ADR id monotonic"

# --- 5. Module-doc + finding last_verified_commit is a real SHA (F20) -
# Per ADSD failure-modes-catalogue F20 (constitution-vs-workflow
# alignment): the rule "every doc with persistent claims has a real
# SHA in last_verified_commit" must be enforced at the gate layer,
# not just the doc layer. Catches `HEAD` placeholders that survive
# a doc edit without a stamp-update.
check_last_verified() {
    local file="$1"
    grep -q "^last_verified_commit:" "$file" || fail "$file missing last_verified_commit frontmatter"
    local sha
    sha=$(grep "^last_verified_commit:" "$file" | head -1 | sed -E 's/^last_verified_commit:[[:space:]]*//')
    if [ "$sha" = "HEAD" ] || [ -z "$sha" ]; then
        fail "$file last_verified_commit='$sha' is a placeholder (F20: must be a real git SHA stamped at last review/merge)"
    fi
    if ! echo "$sha" | grep -qE '^[0-9a-f]{7,40}$'; then
        fail "$file last_verified_commit='$sha' does not look like a git SHA (F20 enforcement; expected 7-40 hex chars)"
    fi
    # F-A3-01 closure: verify the SHA actually exists as a reachable
    # commit. Hex-shape alone passes typos like `deadbee` (valid hex
    # but not a real commit). git cat-file -e is the canonical
    # reachability check; works in subshell so we can suppress stderr.
    if ! git cat-file -e "${sha}^{commit}" 2>/dev/null; then
        fail "$file last_verified_commit='$sha' is hex-shaped but does NOT resolve to a reachable git commit (F20 enforcement: SHA must exist in repo history)"
    fi
}
for md in docs/agent/modules/*.md; do
    [ -f "$md" ] || continue
    check_last_verified "$md"
done
for fnd in docs/agent/findings/*.md; do
    [ -f "$fnd" ] || continue
    # findings/README.md is the index; skip
    base=$(basename "$fnd")
    [ "$base" = "README.md" ] && continue
    check_last_verified "$fnd"
done
ok "M0 — last_verified_commit is a real SHA (F20 enforced)"

# --- 5b. cargo fmt --check — catches local-vs-CI fmt drift before push ---
# Sarah-v2 caught v0.1.3 CI fmt failure because local 6-gate didn't run
# fmt. Adding here as §5b (not §6 — that's the test gate) so the script
# enforces the same fmt bar CI does.
if command -v cargo >/dev/null 2>&1; then
    if cargo fmt --all -- --check >/dev/null 2>&1; then
        ok "M0 — cargo fmt --check clean"
    else
        echo "doc-coverage: FAIL — cargo fmt --check found drift; run \`cargo fmt --all\`" >&2
        cargo fmt --all -- --check 2>&1 | grep -v "^Warning" | head -20 >&2
        exit 1
    fi
fi

# --- 6. Test gate — script-enforces "no FAILED test groups" (M4.1 ---
#         closes cto-shougate-test-gate-grep-leak finding).
# Prior CTO 守闸 SOP used `grep "^test result" | wc -l` which counts
# both "ok" and "FAILED" lines as "test groups." A4 merge shipped 9
# failing tests silently. This gate exit-code-checks cargo test and
# greps for explicit FAILED — exactly the proper-grep replacement
# the cto-shougate finding's §"Actionable takeaway forward #3"
# prescribed. Tied to F20: declared invariant ("5 gates green") now
# has script-level enforcement on the test gate.
if command -v cargo >/dev/null 2>&1; then
    test_log=$(mktemp -t cobrust-studio-test.XXXXXX)
    trap 'rm -f "$test_log"' EXIT
    # Use cargo exit code AND FAILED-grep as paired gates. The exit
    # code catches compile errors, panics, and lockfile mismatches
    # (--locked) that don't produce a `test result: FAILED` line at
    # all. F-M4-01 follow-up: v0.1.1 release shipped with a stale
    # Cargo.lock; cargo test --locked exited 101 but the grep
    # returned 0 FAILED, so the script passed green. v0.1.2 closes.
    if ! cargo test --workspace --locked --no-fail-fast > "$test_log" 2>&1; then
        cargo_exit=$?
        echo "doc-coverage: FAIL — cargo test exited $cargo_exit (lockfile mismatch / compile error / panic)" >&2
        tail -20 "$test_log" >&2
        exit 1
    fi
    failed_count=$(grep -c '^test result: FAILED' "$test_log" || true)
    if [ "${failed_count:-0}" -ne 0 ]; then
        echo "doc-coverage: FAIL — cargo test reported $failed_count failed test groups (closes cto-shougate-test-gate-grep-leak)" >&2
        grep -E '^test .* FAILED' "$test_log" | head -10 >&2 || true
        exit 1
    fi
    ok_count=$(grep -c '^test result: ok' "$test_log" || echo 0)
    ok "M0 — cargo test green ($ok_count ok groups, exit 0, 0 FAILED; script-enforced per cto-shougate + F-M4-01 followup)"
else
    echo "doc-coverage: WARN — cargo not on PATH; skipping test gate. Run \`source \$HOME/.cargo/env\` before this script for full enforcement." >&2
fi

ok "all gates passed"
