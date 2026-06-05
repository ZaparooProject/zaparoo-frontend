#!/bin/bash
# Zaparoo Frontend
# Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
# SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
OUTPUT_DIR="${PROJECT_ROOT}/output"
RELEASE_DIR="${OUTPUT_DIR}/release"
MENU_REPO="ZaparooProject/Menu_MiSTer"
MENU_TAG="v20260510"
MENU_ASSET="menu_zaparoo.rbf"
MAIN_REPO="ZaparooProject/Main_MiSTer"
MAIN_ASSET="MiSTer_Zaparoo"

usage() {
    cat >&2 <<'EOF_USAGE'
Usage: scripts/package-mister-release.sh [vX.Y.Z]

Builds the official MiSTer frontend binary, downloads the required MiSTer
wrapper assets, and writes output/release/zaparoo-frontend-vX.Y.Z.zip.

Set ZAPAROO_SKIP_FRONTEND_BUILD=1 to reuse output/frontend for packaging tests.
EOF_USAGE
}

error() {
    echo "Error: $*" >&2
    exit 1
}

require_command() {
    if ! command -v "$1" > /dev/null 2>&1; then
        error "required command not found: $1"
    fi
}

resolve_tag() {
    if [ $# -gt 1 ]; then
        usage
        exit 2
    fi

    if [ $# -eq 1 ] && [ -n "$1" ]; then
        printf '%s\n' "$1"
        return
    fi

    if [ -n "${GITHUB_REF_NAME:-}" ]; then
        printf '%s\n' "${GITHUB_REF_NAME}"
        return
    fi

    if tag=$(git -C "${PROJECT_ROOT}" describe --tags --exact-match 2> /dev/null); then
        printf '%s\n' "$tag"
        return
    fi

    error "release tag not provided and HEAD is not on an exact tag"
}

extract_cmake_version() {
    python3 - "$PROJECT_ROOT/CMakeLists.txt" <<'PY'
import re
import sys
text = open(sys.argv[1], encoding="utf-8").read()
match = re.search(r"\bproject\s*\([^)]*?\bVERSION\s+([0-9]+(?:\.[0-9]+){2})\b", text, re.S)
if not match:
    raise SystemExit("could not parse project VERSION from CMakeLists.txt")
print(match.group(1))
PY
}

extract_cargo_version() {
    python3 - "$PROJECT_ROOT/rust/Cargo.toml" <<'PY'
import re
import sys
text = open(sys.argv[1], encoding="utf-8").read()
section = re.search(r"(?ms)^\[workspace\.package\]\s*(.*?)(?:^\[|\Z)", text)
if not section:
    raise SystemExit("could not parse [workspace.package] from rust/Cargo.toml")
match = re.search(r'(?m)^version\s*=\s*"([^"]+)"', section.group(1))
if not match:
    raise SystemExit("could not parse workspace package version from rust/Cargo.toml")
print(match.group(1))
PY
}

write_readme() {
    cat > "$1" <<'EOF_README'
# Zaparoo Frontend

1. Copy the `zaparoo` folder to root/top of SD card
2. In `MiSTer.ini`, add following to the `[MiSTer]` or `[Menu]` section:

```ini
main=zaparoo/MiSTer_Zaparoo
```

3. Start or reboot your MiSTer, Frontend will start automatically
EOF_README
}

TAG="$(resolve_tag "$@")"
VERSION="${TAG#v}"
BASE_VERSION="${VERSION%%-*}"
BASE_VERSION="${BASE_VERSION%%+*}"

if [ "$TAG" = "$VERSION" ]; then
    error "release tag must start with v: $TAG"
fi
if ! printf '%s' "$TAG" | grep -Eq '^v[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z][0-9A-Za-z.-]*)?(\+[0-9A-Za-z][0-9A-Za-z.-]*)?$'; then
    error "release tag must look like v1.2.3 or v1.2.3-rc.1: $TAG"
fi

require_command git
require_command gh
require_command python3
require_command zip
require_command install
require_command rsync
if [ "${ZAPAROO_SKIP_FRONTEND_BUILD:-0}" != "1" ]; then
    require_command just
fi

CMAKE_VERSION="$(extract_cmake_version)"
CARGO_VERSION="$(extract_cargo_version)"
if [ "$BASE_VERSION" != "$CMAKE_VERSION" ]; then
    error "tag base version $BASE_VERSION does not match CMake project version $CMAKE_VERSION"
fi
if [ "$BASE_VERSION" != "$CARGO_VERSION" ]; then
    error "tag base version $BASE_VERSION does not match Rust workspace version $CARGO_VERSION"
fi

cd "$PROJECT_ROOT"
if [ "${ZAPAROO_SKIP_FRONTEND_BUILD:-0}" = "1" ]; then
    echo "Skipping frontend build; reusing ${OUTPUT_DIR}/frontend"
else
    just release
fi

FRONTEND_BIN="${OUTPUT_DIR}/frontend"
if [ ! -f "$FRONTEND_BIN" ]; then
    error "frontend binary not found at $FRONTEND_BIN"
fi

mkdir -p "$RELEASE_DIR"
STAGE="${RELEASE_DIR}/zaparoo-frontend-${TAG}"
ARCHIVE="${RELEASE_DIR}/zaparoo-frontend-${TAG}.zip"
TMP_DIR="$(mktemp -d "${RELEASE_DIR}/download.XXXXXX")"
trap 'rm -rf "$TMP_DIR"' EXIT

rm -rf "$STAGE" "$ARCHIVE"
mkdir -p "$STAGE/zaparoo"

MENU_DIR="$TMP_DIR/menu"
MAIN_DIR="$TMP_DIR/main"
mkdir -p "$MENU_DIR" "$MAIN_DIR"

echo "Downloading ${MENU_REPO}@${MENU_TAG}/${MENU_ASSET}"
gh release download "$MENU_TAG" \
    --repo "$MENU_REPO" \
    --pattern "$MENU_ASSET" \
    --dir "$MENU_DIR" \
    --clobber

MAIN_TAG="${MAIN_MISTER_TAG:-${MAIN_TAG:-}}"
if [ -z "$MAIN_TAG" ]; then
    error "MAIN_MISTER_TAG is required; pass the exact ${MAIN_REPO} release tag to package reproducibly"
fi

echo "Downloading ${MAIN_REPO}@${MAIN_TAG}/${MAIN_ASSET}"
gh release download "$MAIN_TAG" \
    --repo "$MAIN_REPO" \
    --pattern "$MAIN_ASSET" \
    --dir "$MAIN_DIR" \
    --clobber

if [ ! -f "$MENU_DIR/$MENU_ASSET" ]; then
    error "downloaded menu asset missing: $MENU_DIR/$MENU_ASSET"
fi
if [ ! -f "$MAIN_DIR/$MAIN_ASSET" ]; then
    error "downloaded MiSTer wrapper missing: $MAIN_DIR/$MAIN_ASSET"
fi

install -m 0644 "$MENU_DIR/$MENU_ASSET" "$STAGE/zaparoo/menu_zaparoo.rbf"
install -m 0755 "$MAIN_DIR/$MAIN_ASSET" "$STAGE/zaparoo/MiSTer_Zaparoo"
install -m 0755 "$FRONTEND_BIN" "$STAGE/zaparoo/frontend"
install -m 0644 "$PROJECT_ROOT/COPYING" "$STAGE/COPYING"
rsync -a --delete "$PROJECT_ROOT/src/LICENSES/" "$STAGE/LICENSES/"
write_readme "$STAGE/README.txt"

(
    cd "$STAGE"
    zip -r "$ARCHIVE" zaparoo LICENSES README.txt COPYING
)

unzip -t "$ARCHIVE" > /dev/null

echo "Release archive: $ARCHIVE"
echo "Menu_MiSTer tag: $MENU_TAG"
echo "Main_MiSTer tag: $MAIN_TAG"
