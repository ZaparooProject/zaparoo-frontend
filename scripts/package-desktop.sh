#!/bin/bash
# Zaparoo Frontend
# Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
# SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
#
# Desktop tarball: a release cargo build, with every font and logo
# embedded, as output/release/zaparoo-frontend-<tag>-<os>-<arch>.tar.gz. No
# shared-library bundling: the binary links system fontconfig and opens
# wayland/X11 and GL at runtime. On macOS this produces a plain tarball
# too; there is no .app wrapper yet.
#
# For a build that runs on older glibc (Steam Deck), use `just x86-portable`
# and package that binary instead.
#
# Set ZAPAROO_SKIP_FRONTEND_BUILD=1 to reuse rust/target/release/frontend.
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
BIN="${PROJECT_ROOT}/rust/target/release/frontend"

cd "$PROJECT_ROOT"
if [ "${ZAPAROO_SKIP_FRONTEND_BUILD:-0}" = "1" ]; then
    echo "Skipping frontend build; reusing ${BIN}"
else
    (cd rust && cargo build --release -p frontend)
fi
if [ ! -x "$BIN" ]; then
    echo "Error: ${BIN} not found" >&2
    exit 1
fi

NAME="zaparoo-frontend-${TAG}-${OS}-${ARCH}"
STAGE="${RELEASE_DIR}/${NAME}"
ARCHIVE="${RELEASE_DIR}/${NAME}.tar.gz"
rm -rf "$STAGE" "$ARCHIVE"
mkdir -p "$STAGE"
NOTICES="$PROJECT_ROOT/rust/frontend/LICENSES/THIRD-PARTY-NOTICES.txt"
if [ ! -f "$NOTICES" ]; then
    echo "Error: missing $NOTICES; run 'just notices'" >&2
    exit 1
fi
install -m 0755 "$BIN" "$STAGE/frontend"
install -m 0644 "$PROJECT_ROOT/COPYING" "$STAGE/COPYING"
mkdir -p "$STAGE/LICENSES"
install -m 0644 "$PROJECT_ROOT"/LICENSES/* "$STAGE/LICENSES/"
install -m 0644 "$NOTICES" "$STAGE/LICENSES/THIRD-PARTY-NOTICES.txt"
cat > "$STAGE/README.txt" <<'EOF_README'
# Zaparoo Frontend (desktop)

Run ./frontend. Point it at a Zaparoo Core with
ZAPAROO_CORE_ENDPOINT=ws://<host>:7497/api/v0.1 or the [core] endpoint in
~/.config/zaparoo/frontend.toml.

The window is 1280x720 unless [video] width/height say otherwise. For a couch
or handheld install, run ./frontend --fullscreen, or set:

    [video]
    fullscreen = true

A fullscreen surface is sized by the compositor, so width and height are
ignored while it is on. --windowed overrides the config for one run.
EOF_README

tar -C "$RELEASE_DIR" -czf "$ARCHIVE" "$NAME"
echo "Desktop archive: $ARCHIVE"
