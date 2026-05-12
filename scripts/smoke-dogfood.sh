#!/usr/bin/env bash
# Cobrust Studio M3 dogfood smoke test.
#
# Boots the release binary against THIS repo and verifies the four
# user-journey-critical surfaces respond correctly:
#
#   1. `GET /api/health`          — server is up + responsive.
#   2. `GET /api/adr` count ≥ 6   — Studio sees its own 6 constitutional
#                                   ADRs (ADR-0001..0006).
#   3. `GET /`                    — SPA shell (or M3 dev-stub HTML) is
#                                   served by the embedded asset handler.
#   4. `GET /login`               — SPA fallback resolves non-root client-
#                                   side routes through the embed Uri
#                                   extractor (NOT the v0.1.0-pre-M4 Path
#                                   extractor that returned an Axum error
#                                   string for every non-root path). This
#                                   probe is the F19 forward-implication
#                                   from finding m4-release-readiness-spa-
#                                   fallback-extractor.md §"Forward
#                                   implications": smoke-dogfood.sh SHOULD
#                                   probe a SPA route to catch this class
#                                   of regression at script level.
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

echo "[1/4] GET /api/health"
HEALTH=$(curl -fsSL "http://127.0.0.1:$PORT/api/health")
echo "$HEALTH" | jq .
STATUS=$(echo "$HEALTH" | jq -r '.status')
if [ "$STATUS" != "ok" ]; then
    echo "FAIL: /api/health status=$STATUS (expected ok)" >&2
    exit 1
fi
echo ""

echo "[2/4] GET /api/adr"
ADR_RESP=$(curl -fsSL "http://127.0.0.1:$PORT/api/adr")
COUNT=$(echo "$ADR_RESP" | jq '.adrs | length')
echo "  count: $COUNT"
if [ "$COUNT" -lt 6 ]; then
    echo "FAIL: expected ≥6 ADRs (constitutional ADR-0001..0006), got $COUNT" >&2
    echo "  body: $ADR_RESP" >&2
    exit 1
fi
echo ""

echo "[3/4] GET /"
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

# F-M4-01 forward-implication: smoke-dogfood SHOULD probe a SPA client-
# side route. v0.1.0 shipped with `Path<String>` on Router::fallback
# which returned the Axum runtime error text for every non-root path
# (/login, /adr, /agent, /finding, /ledger). The prior 3-step smoke
# only hit `/` — covered by `serve_index`, NOT `serve_asset` — so
# the bug walked through unchallenged. v0.1.1+ uses `Uri` extractor
# and SPA routes return the index.html shell. This probe locks the
# regression.
echo "[4/4] GET /login (SPA fallback regression probe — F-M4-01)"
LOGIN=$(curl -fsSL "http://127.0.0.1:$PORT/login")
LOGIN_HEAD=$(echo "$LOGIN" | head -c 200)
# v0.1.0-style bug returned "Wrong number of path arguments for `Path`"
# as a text/plain body. Verify HTML response shape instead.
if ! echo "$LOGIN_HEAD" | grep -qi "html"; then
    echo "FAIL: GET /login did not return HTML — body head: $LOGIN_HEAD" >&2
    echo "  (This is the F-M4-01 regression — verify embed::serve_asset" >&2
    echo "   uses Uri extractor, not Path<String>.)" >&2
    exit 1
fi
if echo "$LOGIN" | grep -qi "Wrong number of path arguments"; then
    echo "FAIL: GET /login returned Axum Path-extractor error — F-M4-01 REGRESSION" >&2
    exit 1
fi
echo "  $(echo "$LOGIN" | head -1 | head -c 60)..."
echo ""

echo "[5/5] POST /api/login + GET /api/session/status (M6 AEAD round-trip smoke)"
# Verify the login + session endpoints respond with the expected JSON shape.
# We do NOT provide real credentials here — we only probe the wire shape.
# A passphrase-without-credentials attempt returns 400 invalid_body (expected),
# but the endpoint itself must be reachable and return JSON.
SESSION_STATUS=$(curl -fsSL "http://127.0.0.1:$PORT/api/session/status")
echo "  session/status: $SESSION_STATUS"
AUTHENTICATED=$(echo "$SESSION_STATUS" | jq -r '.authenticated')
if [ "$AUTHENTICATED" != "true" ] && [ "$AUTHENTICATED" != "false" ]; then
    echo "FAIL: /api/session/status did not return {authenticated: bool}; got: $SESSION_STATUS" >&2
    exit 1
fi
echo "  /api/session/status returned authenticated=$AUTHENTICATED (M6 session route live)"
# Probe POST /api/login shape — empty body must return 400 invalid_body (not 404).
LOGIN_PROBE=$(curl -s -o /dev/null -w "%{http_code}" \
    -X POST "http://127.0.0.1:$PORT/api/login" \
    -H "Content-Type: application/json" \
    -d '{}')
if [ "$LOGIN_PROBE" != "400" ]; then
    echo "FAIL: POST /api/login with empty body must return 400 (got $LOGIN_PROBE)" >&2
    exit 1
fi
echo "  POST /api/login (empty body) → $LOGIN_PROBE (400 expected — route is live)"
echo ""

echo "Dogfood smoke PASS — Studio manages its own ADRs via /api/adr ($COUNT entries) + SPA routes resolve to index.html (F-M4-01 lock) + M6 /api/login + /api/session/status live."
