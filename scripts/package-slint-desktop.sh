#!/bin/bash
# Zaparoo Frontend
# Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
# SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
#
# Desktop tarball for the Slint frontend: a release cargo build plus the
# runtime fonts and logo assets the binary looks up next to itself, as
# output/release/zaparoo-frontend-<tag>-<os>-<arch>.tar.gz. No CMake, no
# Qt, no shared-library bundling: the binary links system fontconfig and
# opens wayland/X11 and GL at runtime. A macOS .app wrapper is a landing
# item (docs/plans/slint-migration.md); this script produces a plain
# tarball there too.
#
# Set ZAPAROO_SKIP_FRONTEND_BUILD=1 to reuse rust/target/release/frontend-slint.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
RELEASE_DIR="${PROJECT_ROOT}/output/release"
TAG="${1:-}"
if [ -z "$TAG" ]; then
    TAG="$(git -C "$PROJECT_ROOT" describe --tags --always)"
fi

OS="$(uname -s | tr '[:upper:]' '[:lower:]')"
ARCH="$(uname -m)"
BIN="${PROJECT_ROOT}/rust/target/release/frontend-slint"

cd "$PROJECT_ROOT"
if [ "${ZAPAROO_SKIP_FRONTEND_BUILD:-0}" = "1" ]; then
    echo "Skipping frontend build; reusing ${BIN}"
else
    (cd rust && cargo build --release -p frontend-slint)
fi
if [ ! -x "$BIN" ]; then
    echo "Error: ${BIN} not found" >&2
    exit 1
fi

NAME="zaparoo-frontend-${TAG}-${OS}-${ARCH}"
STAGE="${RELEASE_DIR}/${NAME}"
ARCHIVE="${RELEASE_DIR}/${NAME}.tar.gz"
rm -rf "$STAGE" "$ARCHIVE"
mkdir -p "$STAGE/fonts" "$STAGE/slint-assets"
install -m 0755 "$BIN" "$STAGE/frontend-slint"
install -m 0644 "$PROJECT_ROOT"/resources/fonts/runtime/*.ttf "$STAGE/fonts/"
rsync -a --delete "$PROJECT_ROOT/rust/frontend-slint/assets/systems/" "$STAGE/slint-assets/systems/"
install -m 0644 "$PROJECT_ROOT/COPYING" "$STAGE/COPYING"
cat > "$STAGE/README.txt" <<'EOF_README'
# Zaparoo Frontend (Slint, desktop)

Run ./frontend-slint from this directory. Keep the fonts and slint-assets
folders next to the binary. Point it at a Zaparoo Core with
ZAPAROO_CORE_ENDPOINT=ws://<host>:7497/api/v0.1 or the [core] endpoint in
~/.config/zaparoo/frontend.toml.
EOF_README

tar -C "$RELEASE_DIR" -czf "$ARCHIVE" "$NAME"
echo "Desktop archive: $ARCHIVE"
