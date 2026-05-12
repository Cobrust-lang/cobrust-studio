#!/usr/bin/env bash
# Cobrust Studio M4 release-tarball build.
#
# Build release binary + tarball per F19 release-readiness contract.
# Run with: bash scripts/release-tarball.sh <version>
#
# Output: dist/cobrust-studio-v<version>-<arch>-<os>.tar.gz + .sha256
#
# This script re-uses scripts/build-release.sh (M3 deliverable) so the
# tarball always contains a binary that bakes web/build/ via rust-embed.
# Stages binary + README + CHANGELOG + LICENSE-{APACHE,MIT} into a
# versioned dist/ directory then gzips it.

set -euo pipefail
VERSION="${1:-}"
if [ -z "$VERSION" ]; then
    echo "usage: $0 <version>" >&2
    exit 1
fi

REPO=$(cd "$(dirname "$0")/.." && pwd)
cd "$REPO"

# Build the release binary (re-uses M3's build-release.sh)
bash scripts/build-release.sh

# Detect target triple (macOS aarch64 → aarch64-apple-darwin, etc.)
ARCH=$(uname -m)
OS=$(uname -s | tr '[:upper:]' '[:lower:]')
case "$OS" in
    darwin) TARGET="${ARCH}-apple-darwin" ;;
    linux)  TARGET="${ARCH}-unknown-linux-gnu" ;;
    *)      TARGET="${ARCH}-${OS}" ;;
esac

TARBALL="cobrust-studio-v${VERSION}-${TARGET}.tar.gz"
mkdir -p dist
cd dist

# Stage the artifact tree
STAGE="cobrust-studio-v${VERSION}-${TARGET}"
rm -rf "$STAGE" && mkdir -p "$STAGE"
cp "$REPO/target/release/cobrust-studio" "$STAGE/cobrust-studio"
cp "$REPO/README.md" "$STAGE/"
cp "$REPO/CHANGELOG.md" "$STAGE/"
cp "$REPO/LICENSE-APACHE" "$STAGE/" 2>/dev/null || true
cp "$REPO/LICENSE-MIT" "$STAGE/" 2>/dev/null || true

tar czf "$TARBALL" "$STAGE"
shasum -a 256 "$TARBALL" > "${TARBALL}.sha256"
rm -rf "$STAGE"

SHA=$(awk '{print $1}' < "${TARBALL}.sha256")
SIZE=$(du -h "$TARBALL" | cut -f1)

echo "OK Built: $REPO/dist/$TARBALL ($SIZE)"
echo "OK SHA-256: $SHA"
echo ""
echo "Next steps (M4 release flow):"
echo "  1. gh release create v${VERSION} dist/${TARBALL} dist/${TARBALL}.sha256 \\"
echo "       --notes-file <(awk '/^## \\[${VERSION}\\]/,/^## /' ${REPO}/CHANGELOG.md | head -n -1)"
echo "  2. Or for private repo: keep tarballs in dist/ + skip gh release"
