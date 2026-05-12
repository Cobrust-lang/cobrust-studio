#!/usr/bin/env bash
# Cobrust Studio M3 dogfood smoke test.
#
# Boots the release binary against THIS repo and verifies the three
# user-journey-critical surfaces respond correctly:
#
#   1. `GET /api/health`          — server is up + responsive.
#   2. `GET /api/adr` count ≥ 6   — Studio sees its own 6 constitutional
#                                   ADRs (ADR-0001..0006).
#   3. `GET /`                    — SPA shell (or M3 dev-stub HTML) is
#                                   served by the embedded asset handler.
#
# This is the M3 "Studio manages its own ADRs via Studio UI" milestone
# proof point. If this script PASSes, the binary is dogfood-ready.
#
# Usage:
#   PORT=37878 bash scripts/smoke-dogfood.sh
#
# Env:
#   PORT      — TCP port to bind (default: 7878)
#   BIN       — override binary path (default: <repo>/target/release/cobrust-studio)
#   STARTUP_S — seconds to wait for binary to bind (default: 3)

set -euo pipefail

PORT=${PORT:-7878}
REPO=$(cd "$(dirname "$0")/.." && pwd)
BIN=${BIN:-"$REPO/target/release/cobrust-studio"}
STARTUP_S=${STARTUP_S:-3}

if [ ! -x "$BIN" ]; then
    echo "FAIL: binary not found at $BIN — run scripts/build-release.sh first" >&2
    exit 1
fi

echo "Smoke target: $BIN"
echo "Project:      $REPO"
echo "Port:         $PORT"
echo ""

# Boot in background; trap to ensure cleanup even on script error.
"$BIN" serve --project "$REPO" --port "$PORT" >/tmp/cobrust-studio-smoke.log 2>&1 &
PID=$!
trap "kill $PID 2>/dev/null || true; wait $PID 2>/dev/null || true" EXIT

# Wait for bind. Cheap port poll instead of fixed sleep.
ready=0
for _ in $(seq 1 $((STARTUP_S * 10))); do
    if curl -fsS "http://127.0.0.1:$PORT/api/health" -o /dev/null 2>/dev/null; then
        ready=1
        break
    fi
    sleep 0.1
done
if [ "$ready" -eq 0 ]; then
    echo "FAIL: server did not bind on port $PORT within ${STARTUP_S}s" >&2
    echo "--- server log ---" >&2
    cat /tmp/cobrust-studio-smoke.log >&2 || true
    exit 1
fi

echo "[1/3] GET /api/health"
HEALTH=$(curl -fsSL "http://127.0.0.1:$PORT/api/health")
echo "$HEALTH" | jq .
STATUS=$(echo "$HEALTH" | jq -r '.status')
if [ "$STATUS" != "ok" ]; then
    echo "FAIL: /api/health status=$STATUS (expected ok)" >&2
    exit 1
fi
echo ""

echo "[2/3] GET /api/adr"
ADR_RESP=$(curl -fsSL "http://127.0.0.1:$PORT/api/adr")
COUNT=$(echo "$ADR_RESP" | jq '.adrs | length')
echo "  count: $COUNT"
if [ "$COUNT" -lt 6 ]; then
    echo "FAIL: expected ≥6 ADRs (constitutional ADR-0001..0006), got $COUNT" >&2
    echo "  body: $ADR_RESP" >&2
    exit 1
fi
echo ""

echo "[3/3] GET /"
ROOT=$(curl -fsSL "http://127.0.0.1:$PORT/")
HEAD_LINE=$(echo "$ROOT" | head -1)
echo "  first line: $HEAD_LINE"
# Either the real SPA index (doctype html, possibly minified) or the
# embed dev-stub (also starts with <!doctype html>). Both are HTML.
if ! echo "$ROOT" | head -c 200 | grep -qi "html"; then
    echo "FAIL: GET / did not return HTML — body head: $(echo "$ROOT" | head -c 200)" >&2
    exit 1
fi
echo ""

echo "Dogfood smoke PASS — Studio manages its own ADRs via /api/adr ($COUNT entries)."
