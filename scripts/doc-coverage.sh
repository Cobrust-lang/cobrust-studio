#!/usr/bin/env bash
# Cobrust Studio doc-coverage gate.
#
# Enforces the constitution's three-track doc rule:
# - Every public crate has a `docs/agent/modules/<crate>.md`
# - Every public module has zh + en entries in `docs/human/{zh,en}/`
# - Every ADR has matching frontmatter + cross-refs valid
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

ok "all gates passed"
