#!/bin/bash
# Zaparoo Frontend
# Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
# SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
#
# Build the Qt + Linux arm64 toolchain base image. Qt upstream version is pinned
# in Dockerfile.toolchain.arm64; the image tag is read from
# scripts/toolchain-arm64/VERSION so CI/local builds share one source of truth.
# Run this only when the arm64 toolchain image is missing or needs a version bump.

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
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
    echo "Error: Docker Buildx is required for the arm64 toolchain build." >&2
    echo "       Docker Desktop includes Buildx." >&2
    exit 1
fi
IMAGE_TAG="zaparoo/qt6-arm64-toolchain:${TOOLCHAIN_VERSION}"

echo "=== Building Qt 6.10.3 arm64 toolchain image (${TOOLCHAIN_VERSION}) ==="
echo "Tag: ${IMAGE_TAG}"
echo "Docker platform: ${DOCKER_PLATFORM}"
echo "This can take a while on first run."
echo ""

docker buildx build \
    --platform "${DOCKER_PLATFORM}" \
    -f "${PROJECT_ROOT}/Dockerfile.toolchain.arm64" \
    -t "${IMAGE_TAG}" \
    --load \
    "${PROJECT_ROOT}"

echo ""
echo "=== arm64 toolchain image built successfully ==="
echo "Tag: ${IMAGE_TAG}"
echo ""
echo "You can now run ./scripts/build-arm64.sh to build the application."
