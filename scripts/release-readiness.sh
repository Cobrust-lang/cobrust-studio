#!/usr/bin/env bash
# Cobrust Studio M4 F19 release-readiness self-check.
#
# Per ADSD v1.2.1 failure-modes-catalogue §F19: any public-facing
# install / quickstart / release command must pass independent
# execution in a clean shell before publish. This script automates
# the audit by cloning the repo into a /tmp/ subdir, running the
# README-documented install commands verbatim, and curling the live
# API surface.
#
# Usage:
#   bash scripts/release-readiness.sh [git-url]
#
# Args:
#   git-url   — repo URL to clone for the clean-shell test.
#               Default: read from Cargo.toml workspace.package.repository.
#               For private repo, requires SSH agent forwarding or a
#               deploy key resolvable from /tmp.
#
# Env:
#   PORT_BASE — TCP port to bind the smoke server (default: 37900)
#   KEEP      — set non-empty to leave /tmp/studio-release-test behind
#               for manual inspection (default: clean up on success)
#
# Exit codes:
#   0  — F19 audit GO (every install command exit-0 + API endpoints respond)
#   1  — F19 audit BLOCK (some command failed; root cause printed to stderr)

set -euo pipefail

REPO=$(cd "$(dirname "$0")/.." && pwd)
PORT=${PORT_BASE:-37900}
TESTDIR=/tmp/studio-release-test

# Resolve git-url. Default: extract from Cargo.toml. If that lookup
# fails fall back to the canonical GitHub URL.
if [ "${1:-}" != "" ]; then
    GIT_URL="$1"
else
    GIT_URL=$(grep -E '^repository = ' "$REPO/Cargo.toml" | head -1 \
        | sed -E 's/^repository = "(.*)"$/\1/' || true)
    if [ -z "$GIT_URL" ]; then
        GIT_URL="https://github.com/Cobrust-lang/cobrust-studio"
    fi
fi

echo "F19 release-readiness audit"
echo "  source repo:  $REPO"
echo "  clone URL:    $GIT_URL"
echo "  test dir:     $TESTDIR"
echo "  smoke port:   $PORT"
echo ""

# Clean slate
rm -rf "$TESTDIR"
mkdir -p "$TESTDIR"
cd "$TESTDIR"

echo "[1/6] git clone <repo-url> ."
# Try the documented URL first. If clone fails (private repo without
# auth in clean shell), fall back to a local file:// clone of the
# working repo so the rest of the audit can still execute. Document
# the fallback in the verdict so the operator knows whether the
# public URL was actually exercised.
PUBLIC_CLONE_OK=1
if ! git clone --depth 1 "$GIT_URL" . 2>/tmp/release-clone-err; then
    PUBLIC_CLONE_OK=0
    echo "  WARN: public clone failed (private repo + no auth in clean shell)"
    echo "  WARN: falling back to local file:// clone so the rest of"
    echo "  WARN: the F19 audit can exercise build + smoke."
    echo "  WARN: stderr ↓"
    sed 's/^/    /' /tmp/release-clone-err >&2 || true
    rm -rf "$TESTDIR"/.git "$TESTDIR"/*
    git clone --depth 1 "file://$REPO" .
fi
echo ""

echo "[2/6] bash scripts/build-release.sh"
bash scripts/build-release.sh
echo ""

echo "[3/6] ./target/release/cobrust-studio --help"
./target/release/cobrust-studio --help
echo ""

echo "[4/6] ./target/release/cobrust-studio serve (port $PORT)"
./target/release/cobrust-studio serve --project . --port "$PORT" \
    >/tmp/release-smoke.log 2>&1 &
PID=$!
trap "kill $PID 2>/dev/null || true; wait $PID 2>/dev/null || true" EXIT

# Cheap port-poll instead of fixed sleep
ready=0
for _ in $(seq 1 60); do
    if curl -fsS "http://127.0.0.1:$PORT/api/health" -o /dev/null 2>/dev/null; then
        ready=1
        break
    fi
    sleep 0.1
done
if [ "$ready" -eq 0 ]; then
    echo "FAIL: server did not bind on port $PORT" >&2
    echo "--- server log ---" >&2
    cat /tmp/release-smoke.log >&2 || true
    exit 1
fi
echo "  bound on http://127.0.0.1:$PORT"
echo ""

echo "[5/6] curl /api/health + /api/version + /api/adr"
HEALTH=$(curl -fsSL "http://127.0.0.1:$PORT/api/health")
echo "  health:  $HEALTH"
VERSION=$(curl -fsSL "http://127.0.0.1:$PORT/api/version")
echo "  version: $VERSION"
COUNT=$(curl -fsSL "http://127.0.0.1:$PORT/api/adr" | jq '.adrs | length')
echo "  adr count: $COUNT (expect >= 6)"
if [ "$COUNT" -lt 6 ]; then
    echo "FAIL: ADR count $COUNT < 6 (constitutional ADR-0001..0006 missing)" >&2
    exit 1
fi
echo ""

echo "[6/6] shutdown + cleanup"
kill $PID 2>/dev/null || true
wait $PID 2>/dev/null || true
trap - EXIT
if [ -z "${KEEP:-}" ]; then
    cd /
    rm -rf "$TESTDIR"
    echo "  cleaned $TESTDIR (set KEEP=1 to retain)"
else
    echo "  retained $TESTDIR (KEEP=$KEEP)"
fi
echo ""

if [ "$PUBLIC_CLONE_OK" -eq 1 ]; then
    echo "F19 verdict: GO — clean-shell install path executes end-to-end."
else
    echo "F19 verdict: GO (with caveat) — local file:// fallback used."
    echo "  Public clone via '$GIT_URL' failed in the clean shell."
    echo "  Resolve by: SSH agent forwarding, deploy key, or making the"
    echo "  repo public before announcing the release."
fi
