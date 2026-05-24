#!/bin/bash
# Zaparoo Frontend
# Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
# SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
#
# Build and run a desktop dev build (ZAPAROO_DEV=ON).
# Uses build-dev/ to avoid clobbering the regular build/.
# Reconfigures only when CMakeLists.txt changes; always rebuilds before running.

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
BUILD_DIR="${PROJECT_ROOT}/build-dev"

cmake -S "${PROJECT_ROOT}" -B "${BUILD_DIR}" \
    -DCMAKE_BUILD_TYPE=Debug \
    -DZAPAROO_DEV=ON \
    -DZAPAROO_BUILD_TESTS=OFF

cmake --build "${BUILD_DIR}"

# Point the dev frontend at mock-core's port (27497) by default. The real
# Core's 7497 stays the production default so a shipping desktop install
# still finds a live Core unmodified. Any pre-set value (e.g. from the
# shell, or pointing at a real Core on the network) wins.
export ZAPAROO_CORE_ENDPOINT="${ZAPAROO_CORE_ENDPOINT:-ws://127.0.0.1:27497/api/v0.1}"

exec "${BUILD_DIR}/bin/frontend"
