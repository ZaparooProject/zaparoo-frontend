#!/usr/bin/env bash
# Zaparoo Frontend
# Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
# SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
#
# Build the Slint frontend for MiSTer. Docker owns a persistent static-musl
# ARM32 toolchain image; source builds run inside it without rebuilding it.

set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)"
PROJECT_ROOT="$(cd -- "$SCRIPT_DIR/.." && pwd -P)"
TARGET="armv7-unknown-linux-musleabihf"
BINARY="$PROJECT_ROOT/rust/target/$TARGET/release/frontend-slint"
DOCKER_PLATFORM="${DOCKER_PLATFORM:-linux/amd64}"
TOOLCHAIN_IMAGE="${SLINT_MISTER_IMAGE:-zaparoo-slint-mister:local}"
CROSS_IMAGE="${SLINT_MISTER_BASE_IMAGE:-ghcr.io/cross-rs/armv7-unknown-linux-musleabihf:main}"

if ! docker buildx version > /dev/null 2>&1; then
    echo "Error: Docker Buildx is required for the MiSTer Slint build." >&2
    exit 1
fi

if [ "${SLINT_MISTER_REBUILD_TOOLCHAIN:-0}" = "1" ] || \
        ! docker image inspect "$TOOLCHAIN_IMAGE" > /dev/null 2>&1; then
    echo "=== Creating MiSTer Slint toolchain image: $TOOLCHAIN_IMAGE ==="
    docker buildx build \
        --platform "$DOCKER_PLATFORM" \
        -f "$PROJECT_ROOT/Dockerfile.slint-arm32" \
        --build-arg "CROSS_IMAGE=$CROSS_IMAGE" \
        --target toolchain \
        --tag "$TOOLCHAIN_IMAGE" \
        --load \
        "$PROJECT_ROOT"
else
    echo "=== Reusing MiSTer Slint toolchain image: $TOOLCHAIN_IMAGE ==="
fi

echo "=== Building Slint frontend for MiSTer (${TARGET}, Cortex-A9) ==="
echo "Docker platform: $DOCKER_PLATFORM"
docker run --rm \
    --platform "$DOCKER_PLATFORM" \
    --mount "type=bind,source=$PROJECT_ROOT,target=/src" \
    --workdir /src/rust \
    --env ZAPAROO_RESOURCES_DIR=/src/resources \
    --env 'RUSTFLAGS=-C target-cpu=cortex-a9' \
    "$TOOLCHAIN_IMAGE" \
    cargo +1.97.0 build -p frontend-slint --release \
        --no-default-features --features mister \
        --target "$TARGET"

if [ ! -x "$BINARY" ]; then
    echo "Error: MiSTer binary was not written to $BINARY" >&2
    exit 1
fi
echo "Built: $BINARY"
