#!/usr/bin/env bash
# Cobrust Studio M3 release-build orchestration.
#
# Single source of truth for the release-binary build path described in
# ADR-0002 (single-binary deployment). Runs the SvelteKit static export
# first so `web/build/` is populated, then `cargo build --release` so
# rust-embed bakes the bundle into the binary.
#
# Pinned reproducibility:
# - `pnpm install --frozen-lockfile` — pnpm-lock.yaml is binding.
# - `cargo build --release --workspace --locked` — Cargo.lock is binding.
#
# Usage:
#   bash scripts/build-release.sh
#
# Output:
#   target/release/cobrust-studio (single executable, ~30 MiB)
#
# Tx tag for downstream commits: M3 Wave.

set -euo pipefail

REPO=$(cd "$(dirname "$0")/.." && pwd)

echo "[1/3] pnpm install (frozen lockfile)"
cd "$REPO/web"
pnpm install --frozen-lockfile

echo "[2/3] pnpm run build (SvelteKit static export → web/build/)"
pnpm run build

echo "[3/3] cargo build --release --workspace --locked"
cd "$REPO"
cargo build --release --workspace --locked

BIN="$REPO/target/release/cobrust-studio"
if [ ! -x "$BIN" ]; then
    echo "FAIL: expected binary at $BIN, but it was not produced" >&2
    exit 1
fi

SIZE=$(du -h "$BIN" | cut -f1)
echo ""
echo "Release build complete."
echo "  Binary:     $BIN ($SIZE)"
echo "  Assets:     $REPO/web/build/ (baked into binary via rust-embed)"
echo ""
echo "Smoke-test against this repo:"
echo "  PORT=37878 bash scripts/smoke-dogfood.sh"
