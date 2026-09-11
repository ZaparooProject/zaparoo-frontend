#!/usr/bin/env bash
# Zaparoo Frontend
# Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
# SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
#
# Build the Slint frontend for MiSTer. Cross launches Docker's static-musl
# ARM32 toolchain; Rust and the target dependencies stay in that container.

set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)"
PROJECT_ROOT="$(cd -- "$SCRIPT_DIR/.." && pwd -P)"
TARGET="armv7-unknown-linux-musleabihf"
BINARY="$PROJECT_ROOT/rust/target/$TARGET/release/frontend-slint"
CARGO_BIN="${CARGO_HOME:-${HOME}/.cargo}/bin"
# Cross's published ARM32-musl image is amd64-only. Docker Desktop otherwise
# selects the Apple Silicon host platform and fails before Cross can build.
# Keep both knobs because Cross uses `docker build` for Cross.toml's pre-build
# hook, then `docker run` for the actual compilation container.
CROSS_DOCKER_PLATFORM="${CROSS_DOCKER_PLATFORM:-linux/amd64}"

# `cargo install cross` writes next to rustup.  Make that bin directory
# available for this command even when the user's shell profile omitted it.
if [ -d "$CARGO_BIN" ]; then
    export PATH="$CARGO_BIN:$PATH"
fi
# Homebrew's rustup is keg-only.  Find it here rather than requiring a
# user-specific shell-profile edit before the project commands can work.
if ! command -v rustup > /dev/null 2>&1 && command -v brew > /dev/null 2>&1; then
    BREW_RUSTUP_BIN="$(brew --prefix rustup 2> /dev/null || true)/bin"
    if [ -x "$BREW_RUSTUP_BIN/rustup" ]; then
        export PATH="$BREW_RUSTUP_BIN:$PATH"
    fi
fi

if ! command -v rustup > /dev/null 2>&1; then
    cat >&2 <<'EOF'
Error: Rustup is required for the MiSTer Slint build.

Install it with:
  brew install rustup-init
  rustup-init -y --default-toolchain 1.97.0 --profile minimal
EOF
    exit 1
fi

if ! command -v cross > /dev/null 2>&1; then
    cat >&2 <<'EOF'
Error: Cross is required for the MiSTer Slint build.

Install it with:
  cargo install cross
EOF
    exit 1
fi

if ! docker info > /dev/null 2>&1; then
    echo "Error: Docker Desktop must be running for the MiSTer Slint build." >&2
    exit 1
fi

TOOLCHAIN_CARGO="$(rustup which --toolchain 1.97.0 cargo)"
TOOLCHAIN_RUSTC="$(rustup which --toolchain 1.97.0 rustc)"

echo "=== Cross-building Slint frontend for MiSTer (${TARGET}, Cortex-A9) ==="
echo "Docker platform: $CROSS_DOCKER_PLATFORM"
(cd "$PROJECT_ROOT/rust" && \
    ZAPAROO_RESOURCES_DIR="$PROJECT_ROOT/resources" \
    RUSTFLAGS="-C target-cpu=cortex-a9" \
    RUSTUP_TOOLCHAIN=1.97.0 \
    CARGO="$TOOLCHAIN_CARGO" \
    RUSTC="$TOOLCHAIN_RUSTC" \
    CROSS_BUILD_OPTS="${CROSS_BUILD_OPTS:---platform=$CROSS_DOCKER_PLATFORM}" \
    CROSS_CONTAINER_OPTS="${CROSS_CONTAINER_OPTS:---platform $CROSS_DOCKER_PLATFORM}" \
    rustup run 1.97.0 cross build -p frontend-slint --release \
        --no-default-features --features mister --target "$TARGET")
echo "Built: $BINARY"
