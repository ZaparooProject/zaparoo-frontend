#!/bin/bash
# Zaparoo Frontend
# Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
# SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
FRONTEND="${PROJECT_ROOT}/build/bin/frontend"

if [ ! -x "${FRONTEND}" ]; then
    echo "Error: macOS frontend not found or not executable: ${FRONTEND}" >&2
    echo "Build it first with: cmake --build build-macos" >&2
    exit 1
fi

export ZAPAROO_CORE_ENDPOINT="ws://192.168.1.176:7497/api/v0.1"
export ZAPAROO_CRT_PREVIEW_SCALE=3
exec "${FRONTEND}" --crt
# exec "${FRONTEND}" --crt
