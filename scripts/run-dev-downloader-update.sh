#!/bin/bash
# Zaparoo Frontend
# Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
# SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
#
# Throwaway dev helper: runs the desktop dev frontend with the Update screen
# pointed at a local Downloader_MiSTer checkout. The tool mode mirrors
# Downloader_MiSTer's `src/debug.py local_run` environment and runs a freshly
# built Downloader artifact by default. Set
# ZAPAROO_DEV_DOWNLOADER_RUN_SOURCE=1 to run the source entry point directly.
#
# Mock mode accepts an optional CRT preview resolution after --mock:
#   ./scripts/run-dev-downloader-update.sh --mock ntsc-320b
#   ./scripts/run-dev-downloader-update.sh --mock 320x240

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
DEFAULT_DOWNLOADER_ROOT="${PROJECT_ROOT}/../Downloader_MiSTer"
DOWNLOADER_ROOT="${DOWNLOADER_MISTER_DIR:-$DEFAULT_DOWNLOADER_ROOT}"
MOCK_UPDATE_TOOL=0
MOCK_CRT_RESOLUTION=""
RUN_DEV_ARGS=()

is_crt_resolution_arg() {
    case "$1" in
        [0-9]*x[0-9]* | [0-9]*X[0-9]* | ntsc-* | pal-*)
            return 0
            ;;
        *)
            return 1
            ;;
    esac
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --mock)
            MOCK_UPDATE_TOOL=1
            if [[ $# -gt 1 && "$2" != --* ]] && is_crt_resolution_arg "$2"; then
                MOCK_CRT_RESOLUTION="$2"
                shift
            fi
            ;;
        --mock=*)
            MOCK_UPDATE_TOOL=1
            MOCK_CRT_RESOLUTION="${1#--mock=}"
            ;;
        *)
            RUN_DEV_ARGS+=("$1")
            ;;
    esac
    shift
done

if [[ "$MOCK_UPDATE_TOOL" == "1" && "${ZAPAROO_DEV_DOWNLOADER_UPDATE_TOOL:-}" != "1" ]]; then
    MOCK_DOWNLOADER="${PROJECT_ROOT}/../zaparoo-update/mock-downloader.py"
    if [[ ! -f "$MOCK_DOWNLOADER" ]]; then
        echo "Mock downloader not found: $MOCK_DOWNLOADER" >&2
        exit 1
    fi

    export ZAPAROO_UPDATE_TOOL="$MOCK_DOWNLOADER"
    export ZAPAROO_RETROACCOUNT_SYNC_TOOL="$MOCK_DOWNLOADER"
    if [[ -n "$MOCK_CRT_RESOLUTION" && -z "${ZAPAROO_CRT_PREVIEW_RESOLUTION:-}" ]]; then
        export ZAPAROO_CRT_PREVIEW_RESOLUTION="$MOCK_CRT_RESOLUTION"
    fi
    export ZAPAROO_DEBUG="${ZAPAROO_DEBUG:-1}"
    export ZAPAROO_LOG_TERMINAL_FD=3

    cd "$PROJECT_ROOT"
    just run-dev "${RUN_DEV_ARGS[@]}" 3>&2
    exit $?
fi

if ! DOWNLOADER_ROOT="$(cd "$DOWNLOADER_ROOT" 2>/dev/null && pwd -P)"; then
    echo "Downloader_MiSTer directory not found: ${DOWNLOADER_MISTER_DIR:-$DEFAULT_DOWNLOADER_ROOT}" >&2
    echo "Set DOWNLOADER_MISTER_DIR=/path/to/Downloader_MiSTer to override." >&2
    exit 1
fi

if [[ ! -f "${DOWNLOADER_ROOT}/src/build.sh" ]]; then
    echo "Downloader_MiSTer build helper not found: ${DOWNLOADER_ROOT}/src/build.sh" >&2
    echo "Set DOWNLOADER_MISTER_DIR=/path/to/Downloader_MiSTer to override." >&2
    exit 1
fi

if [[ "${ZAPAROO_DEV_DOWNLOADER_RUN_SOURCE:-}" == "1" && ! -f "${DOWNLOADER_ROOT}/src/__main__.py" ]]; then
    echo "Downloader_MiSTer entry point not found: ${DOWNLOADER_ROOT}/src/__main__.py" >&2
    echo "Set DOWNLOADER_MISTER_DIR=/path/to/Downloader_MiSTer to override." >&2
    exit 1
fi

if [[ "${ZAPAROO_DEV_DOWNLOADER_UPDATE_TOOL:-}" == "1" ]]; then
    LOCAL_DRV="${DOWNLOADER_ROOT}/.local_drv"
    mkdir -p "$LOCAL_DRV"

    export DEFAULT_BASE_PATH="$LOCAL_DRV"
    export DOWNLOADER_INI_PATH="${LOCAL_DRV}/downloader.ini"
    export LOGFILE="${LOCAL_DRV}/downloader.log"
    export CURL_SSL=""
    export DEBUG="true"
    export DOWNLOADER_OUTPUT="${DOWNLOADER_OUTPUT:-dlp1-ltsv}"
    export PYTHONUNBUFFERED=1
    export UPDATE_LINUX="false"
    export ALLOW_REBOOT="0"

    cd "$DOWNLOADER_ROOT"
    if [[ "${ZAPAROO_DEV_DOWNLOADER_RUN_SOURCE:-}" == "1" ]]; then
        exec python3 ./src/__main__.py
    fi

    BUILD_ZIP="${LOCAL_DRV}/downloader-local.pyz"
    BUILD_SCRIPT="${LOCAL_DRV}/downloader-local.sh"
    rm -f "$BUILD_ZIP" "$BUILD_SCRIPT"

    ZIP_FILE="$BUILD_ZIP" \
        SKIP_REMOVALS=true \
        DEBUG=true \
        MISTER=true \
        ./src/build.sh > "$BUILD_SCRIPT"
    chmod +x "$BUILD_ZIP" "$BUILD_SCRIPT"
    exec "$BUILD_SCRIPT"
fi

export DOWNLOADER_MISTER_DIR="$DOWNLOADER_ROOT"
export ZAPAROO_DEV_DOWNLOADER_UPDATE_TOOL=1
export ZAPAROO_UPDATE_TOOL="${SCRIPT_DIR}/run-dev-downloader-update.sh"
export ZAPAROO_DEBUG="${ZAPAROO_DEBUG:-1}"
export ZAPAROO_LOG_TERMINAL_FD=3

cd "$PROJECT_ROOT"
just run-dev "${RUN_DEV_ARGS[@]}" 3>&2
