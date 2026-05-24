#!/bin/bash
# Zaparoo Frontend
# Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
# SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
#
# Cross-compiles the frontend for ARM32 / MiSTer FPGA using Docker.
# Uses the official prebuilt toolchain image by default; builds the application
# layer only (~1 min after the image is cached).
#
# Set USE_LOCAL_TOOLCHAIN=1 to build and use the local toolchain image from
# Dockerfile.toolchain instead (~45 min one-time). Subsequent runs are fast.

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
OUTPUT_DIR="${PROJECT_ROOT}/output"
VERSION_FILE="${PROJECT_ROOT}/scripts/toolchain/VERSION"
DOCKER_PLATFORM="${DOCKER_PLATFORM:-linux/amd64}"
if [ ! -f "${VERSION_FILE}" ]; then
    echo "Error: toolchain version file not found at ${VERSION_FILE}" >&2
    echo "       (PROJECT_ROOT=${PROJECT_ROOT})" >&2
    exit 1
fi
# tr -d strips the trailing newline and guards against stray whitespace
# that would silently corrupt the Docker tag.
TOOLCHAIN_VERSION="$(tr -d '[:space:]' < "${VERSION_FILE}")"
if ! printf '%s' "${TOOLCHAIN_VERSION}" | grep -Eq '^[A-Za-z0-9_][A-Za-z0-9_.-]{0,127}$'; then
    echo "Error: invalid toolchain version in ${VERSION_FILE}" >&2
    echo "       raw value: '${TOOLCHAIN_VERSION}'" >&2
    echo "       expected:  Docker tag [A-Za-z0-9_][A-Za-z0-9_.-]{0,127}" >&2
    exit 1
fi
if ! docker buildx version > /dev/null 2>&1; then
    echo "Error: Docker Buildx is required for the ARM32 application build." >&2
    echo "       Docker Desktop includes Buildx." >&2
    exit 1
fi
LOCAL_TOOLCHAIN_IMAGE="zaparoo/qt6-arm32-mister:${TOOLCHAIN_VERSION}"
OFFICIAL_TOOLCHAIN_IMAGE="ghcr.io/zaparooproject/qt6-arm32-mister:${TOOLCHAIN_VERSION}"

# Local dev defaults to the official toolchain image published by this
# repository's toolchain-build workflow. Set USE_LOCAL_TOOLCHAIN=1 to rebuild
# Qt and the MiSTer ARM GCC toolchain from Dockerfile.toolchain instead.
if [ -z "${TOOLCHAIN_IMAGE:-}" ]; then
    if [ "${USE_LOCAL_TOOLCHAIN:-0}" = "1" ]; then
        TOOLCHAIN_IMAGE="${LOCAL_TOOLCHAIN_IMAGE}"
    else
        TOOLCHAIN_IMAGE="${OFFICIAL_TOOLCHAIN_IMAGE}"
    fi
fi

# Skip the registry probe when the image is already in the local daemon, so
# offline rebuilds and expired GHCR tokens still work as long as it has been
# pulled at least once.
if [[ "${TOOLCHAIN_IMAGE}" == "${OFFICIAL_TOOLCHAIN_IMAGE}" ]] \
    && ! docker image inspect "${TOOLCHAIN_IMAGE}" > /dev/null 2>&1 \
    && ! docker manifest inspect "${TOOLCHAIN_IMAGE}" > /dev/null 2>&1; then
    echo "Error: official toolchain image is not available: ${TOOLCHAIN_IMAGE}" >&2
    echo "       If GHCR requires auth, run:" >&2
    echo "       gh auth refresh -h github.com -s read:packages" >&2
    echo "       gh auth token | docker login ghcr.io -u <github-user> --password-stdin" >&2
    echo "       To build the toolchain locally instead, run:" >&2
    echo "       USE_LOCAL_TOOLCHAIN=1 ./scripts/build-arm32.sh" >&2
    exit 1
fi

# Build the toolchain image locally if it is missing and we are using the local
# tag. When TOOLCHAIN_IMAGE points at a registry, docker build will pull it.
if [[ "${TOOLCHAIN_IMAGE}" == "${LOCAL_TOOLCHAIN_IMAGE}" ]] \
    && ! docker image inspect "${TOOLCHAIN_IMAGE}" > /dev/null 2>&1; then
    echo "Toolchain image '${TOOLCHAIN_IMAGE}' not found locally."
    echo "Building it now (~45 minutes)..."
    "${SCRIPT_DIR}/build-toolchain.sh"
fi

echo "=== Cross-compiling frontend for ARM32 ==="
echo "Using toolchain image: ${TOOLCHAIN_IMAGE}"
echo "Docker platform: ${DOCKER_PLATFORM}"
mkdir -p "${OUTPUT_DIR}"

# Resolve build provenance on the host. The Dockerfile only COPYs source
# dirs (no `.git/`), so without forwarding these the in-container build.rs
# falls back to commit = "unknown". Empty values fall back to build.rs's
# own defaults inside the container.
ZAPAROO_BUILD_COMMIT="${ZAPAROO_BUILD_COMMIT:-$(git -C "${PROJECT_ROOT}" rev-parse --short=7 HEAD 2>/dev/null || true)}"
ZAPAROO_BUILD_DATE="${ZAPAROO_BUILD_DATE:-$(date -u +%Y-%m-%d)}"

docker buildx build \
    --platform "${DOCKER_PLATFORM}" \
    -f "${PROJECT_ROOT}/Dockerfile.arm32" \
    --build-arg "TOOLCHAIN_IMAGE=${TOOLCHAIN_IMAGE}" \
    --build-arg "ZAPAROO_OFFICIAL_BUILD=${ZAPAROO_OFFICIAL_BUILD:-}" \
    --build-arg "ZAPAROO_BUILD_COMMIT=${ZAPAROO_BUILD_COMMIT}" \
    --build-arg "ZAPAROO_BUILD_DATE=${ZAPAROO_BUILD_DATE}" \
    --output "type=local,dest=${OUTPUT_DIR}" \
    --target export \
    "${PROJECT_ROOT}"

if [ -f "${OUTPUT_DIR}/frontend" ]; then
    echo ""
    echo "=== Build successful! ==="
    file "${OUTPUT_DIR}/frontend"
else
    echo "Build failed — binary not found in ${OUTPUT_DIR}"
    exit 1
fi
