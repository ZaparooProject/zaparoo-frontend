#!/bin/bash
# Zaparoo Frontend
# Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
# SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
#
# Builds the ARM32 binary and deploys it to a MiSTer FPGA over SSH/SCP.
# Pass --skip-build to deploy an existing output/frontend without rebuilding.
# Reads MISTER_IP from a .env file in the project root.

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
ENV_FILE="${PROJECT_ROOT}/.env"
REMOTE_PATH="/media/fat/zaparoo/frontend"
BINARY="${PROJECT_ROOT}/output/frontend"
SKIP_BUILD=0

while [ "$#" -gt 0 ]; do
    case "$1" in
        --skip-build)
            SKIP_BUILD=1
            shift
            ;;
        -h|--help)
            echo "Usage: $0 [--skip-build]"
            echo ""
            echo "Builds output/frontend and deploys it to MiSTer."
            echo "  --skip-build  Deploy existing output/frontend without rebuilding"
            exit 0
            ;;
        *)
            echo "Error: unknown argument: $1" >&2
            echo "Usage: $0 [--skip-build]" >&2
            exit 1
            ;;
    esac
done

if [ ! -f "${ENV_FILE}" ]; then
    echo "Error: .env file not found at ${ENV_FILE}"
    echo "Create it with: echo 'MISTER_IP=<your-mister-ip>' > .env"
    exit 1
fi

set -a
# shellcheck source=/dev/null
source "${ENV_FILE}"
set +a

if [ -z "${MISTER_IP}" ]; then
    echo "Error: MISTER_IP is not set in ${ENV_FILE}"
    exit 1
fi

if [ "${SKIP_BUILD}" -eq 1 ]; then
    echo "=== Skipping ARM32 build ==="
    if [ ! -f "${BINARY}" ]; then
        echo "Error: ${BINARY} does not exist; run ${SCRIPT_DIR}/build-arm32.sh first" >&2
        exit 1
    fi
else
    echo "=== Building ARM32 binary ==="
    "${SCRIPT_DIR}/build-arm32.sh"
fi

echo ""
echo "=== Deploying to MiSTer at ${MISTER_IP} ==="

ssh "root@${MISTER_IP}" "
    if [ -f '${REMOTE_PATH}' ]; then
        mv '${REMOTE_PATH}' '${REMOTE_PATH}.bak'
        echo 'Moved existing binary to ${REMOTE_PATH}.bak'
    fi
"

scp "${BINARY}" "root@${MISTER_IP}:${REMOTE_PATH}"
echo "Deployed ${BINARY} → root@${MISTER_IP}:${REMOTE_PATH}"

ssh "root@${MISTER_IP}" "
    rm -f /tmp/zaparoo/frontend.log
    # SIGKILL the running frontend; MiSTer's wrapper respawns it
    # ~1s later with the new binary. SIGTERM here would be misclassified as
    # an 'escape' and MiSTer would refuse to respawn. Note: counts as a crash
    # toward the 3-strike give-up limit, so if you deploy 3 times without
    # cleanly exiting the frontend in between, killall MiSTer_Zaparoo to reset.
    killall -KILL frontend 2>/dev/null && echo 'Killed running frontend; MiSTer will respawn it' || echo 'No running frontend to kill'
"
