#!/bin/bash
# Zaparoo Frontend
# Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
# SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
#
# Cross-compiles the frontend for Linux arm64 using Docker.
# Uses a pinned Qt + aarch64 cross toolchain image. The build writes
# output/frontend-arm64.
#
# Set USE_LOCAL_TOOLCHAIN=1 to build/use the local toolchain image from
# Dockerfile.toolchain.arm64 if the published image is not available.

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
OUTPUT_DIR="${PROJECT_ROOT}/output"
VERSION_FILE="${PROJECT_ROOT}/scripts/toolchain-arm64/VERSION"
DOCKER_PLATFORM="${DOCKER_PLATFORM:-linux/amd64}"
if [ ! -f "${VERSION_FILE}" ]; then
    echo "Error: arm64 toolchain version file not found at ${VERSION_FILE}" >&2
    echo "       (PROJECT_ROOT=${PROJECT_ROOT})" >&2
    exit 1
fi
TOOLCHAIN_VERSION="$(tr -d '[:space:]' < "${VERSION_FILE}")"
if ! printf '%s' "${TOOLCHAIN_VERSION}" | grep -Eq '^[A-Za-z0-9_][A-Za-z0-9_.-]{0,127}$'; then
    echo "Error: invalid arm64 toolchain version in ${VERSION_FILE}" >&2
    echo "       raw value: '${TOOLCHAIN_VERSION}'" >&2
    echo "       expected:  Docker tag [A-Za-z0-9_][A-Za-z0-9_.-]{0,127}" >&2
    exit 1
fi
if ! docker buildx version > /dev/null 2>&1; then
    echo "Error: Docker Buildx is required for the arm64 application build." >&2
    echo "       Docker Desktop includes Buildx." >&2
    exit 1
fi
LOCAL_TOOLCHAIN_IMAGE="zaparoo/qt6-arm64-toolchain:${TOOLCHAIN_VERSION}"
OFFICIAL_TOOLCHAIN_IMAGE="ghcr.io/zaparooproject/qt6-arm64-toolchain:${TOOLCHAIN_VERSION}"

if [ -z "${TOOLCHAIN_IMAGE:-}" ]; then
    if [ "${USE_LOCAL_TOOLCHAIN:-0}" = "1" ]; then
        TOOLCHAIN_IMAGE="${LOCAL_TOOLCHAIN_IMAGE}"
    else
        TOOLCHAIN_IMAGE="${OFFICIAL_TOOLCHAIN_IMAGE}"
    fi
fi

if [[ "${TOOLCHAIN_IMAGE}" == "${OFFICIAL_TOOLCHAIN_IMAGE}" ]] \
    && ! docker image inspect "${TOOLCHAIN_IMAGE}" > /dev/null 2>&1 \
    && ! docker manifest inspect "${TOOLCHAIN_IMAGE}" > /dev/null 2>&1; then
    echo "Error: official arm64 toolchain image is not available: ${TOOLCHAIN_IMAGE}" >&2
    echo "       If GHCR requires auth, run:" >&2
    echo "       gh auth refresh -h github.com -s read:packages" >&2
    echo "       gh auth token | docker login ghcr.io -u <github-user> --password-stdin" >&2
    echo "       To build the toolchain locally instead, run:" >&2
    echo "       USE_LOCAL_TOOLCHAIN=1 ./scripts/build-arm64.sh" >&2
    exit 1
fi

if [[ "${TOOLCHAIN_IMAGE}" == "${LOCAL_TOOLCHAIN_IMAGE}" ]] \
    && ! docker image inspect "${TOOLCHAIN_IMAGE}" > /dev/null 2>&1; then
    echo "Toolchain image '${TOOLCHAIN_IMAGE}' not found locally."
    echo "Building it now..."
    "${SCRIPT_DIR}/build-toolchain-arm64.sh"
fi

echo "=== Cross-compiling frontend for Linux arm64 ==="
echo "Using toolchain image: ${TOOLCHAIN_IMAGE}"
echo "Docker platform: ${DOCKER_PLATFORM}"
mkdir -p "${OUTPUT_DIR}"

ZAPAROO_BUILD_COMMIT="${ZAPAROO_BUILD_COMMIT:-$(git -C "${PROJECT_ROOT}" rev-parse --short=7 HEAD 2>/dev/null || true)}"
ZAPAROO_BUILD_DATE="${ZAPAROO_BUILD_DATE:-$(date -u +%Y-%m-%d)}"

docker buildx build \
    --platform "${DOCKER_PLATFORM}" \
    -f "${PROJECT_ROOT}/Dockerfile.arm64" \
    --build-arg "TOOLCHAIN_IMAGE=${TOOLCHAIN_IMAGE}" \
    --build-arg "ZAPAROO_OFFICIAL_BUILD=${ZAPAROO_OFFICIAL_BUILD:-}" \
    --build-arg "ZAPAROO_BUILD_COMMIT=${ZAPAROO_BUILD_COMMIT}" \
    --build-arg "ZAPAROO_BUILD_DATE=${ZAPAROO_BUILD_DATE}" \
    --output "type=local,dest=${OUTPUT_DIR}" \
    --target export \
    "${PROJECT_ROOT}"

if [ -f "${OUTPUT_DIR}/frontend-arm64" ]; then
    echo ""
    echo "=== arm64 build successful! ==="
    file "${OUTPUT_DIR}/frontend-arm64"
else
    echo "Build failed — binary not found in ${OUTPUT_DIR}"
    exit 1
fi
