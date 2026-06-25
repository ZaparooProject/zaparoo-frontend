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
LOCAL_TOOLCHAIN=0

while [ "$#" -gt 0 ]; do
    case "$1" in
        --skip-build)
            SKIP_BUILD=1
            shift
            ;;
        --local-toolchain)
            LOCAL_TOOLCHAIN=1
            shift
            ;;
        -h|--help)
            echo "Usage: $0 [--skip-build] [--local-toolchain]"
            echo ""
            echo "Builds output/frontend and deploys it to MiSTer."
            echo "  --skip-build       Deploy existing output/frontend without rebuilding"
            echo "  --local-toolchain  Build with the local Docker toolchain image"
            exit 0
            ;;
        *)
            echo "Error: unknown argument: $1" >&2
            echo "Usage: $0 [--skip-build] [--local-toolchain]" >&2
            exit 1
            ;;
    esac
done

if [ ! -f "${ENV_FILE}" ]; then
    echo "Error: .env file not found at ${ENV_FILE}"
    echo "Create it with: echo 'MISTER_IP=<your-mister-ip>' > .env"
    exit 1
fi

# shellcheck source=/dev/null
source "${ENV_FILE}"

if [ -z "${MISTER_IP}" ]; then
    echo "Error: MISTER_IP is not set in ${ENV_FILE}"
    exit 1
fi

SSH_OPTS=(-o StrictHostKeyChecking=accept-new)
USE_SSHPASS=0
if [ -n "${MISTER_PW:-}" ]; then
    if ! command -v sshpass > /dev/null 2>&1; then
        echo "Error: MISTER_PW is set in ${ENV_FILE}, but sshpass is not installed." >&2
        echo "Install sshpass or remove MISTER_PW to use SSH keys/password prompts." >&2
        exit 1
    fi
    USE_SSHPASS=1
fi

run_ssh() {
    if [ "${USE_SSHPASS}" -eq 1 ]; then
        SSHPASS="${MISTER_PW}" sshpass -e ssh "${SSH_OPTS[@]}" "$@"
    else
        ssh "${SSH_OPTS[@]}" "$@"
    fi
}

run_scp() {
    if [ "${USE_SSHPASS}" -eq 1 ]; then
        SSHPASS="${MISTER_PW}" sshpass -e scp "${SSH_OPTS[@]}" "$@"
    else
        scp "${SSH_OPTS[@]}" "$@"
    fi
}

if [ "${SKIP_BUILD}" -eq 1 ]; then
    echo "=== Skipping ARM32 build ==="
    if [ ! -f "${BINARY}" ]; then
        echo "Error: ${BINARY} does not exist; run ${SCRIPT_DIR}/build-arm32.sh first" >&2
        exit 1
    fi
else
    echo "=== Building ARM32 binary ==="
    if [ "${LOCAL_TOOLCHAIN}" -eq 1 ]; then
        USE_LOCAL_TOOLCHAIN=1 "${SCRIPT_DIR}/build-arm32.sh"
    else
        "${SCRIPT_DIR}/build-arm32.sh"
    fi
fi

echo ""
echo "=== Deploying to MiSTer at ${MISTER_IP} ==="

# Upload to a side path first so an interrupted transfer can never clobber
# the working binary. The old flow scp'd straight over the live path and
# pre-rotated it to .bak unconditionally: a failed transfer then left a
# truncated frontend AND the next run would overwrite the good backup with
# that stub. Here we only rotate after a size-verified upload, then force
# the write to the card with `sync` — exFAT has no journal, so a metadata
# update lost to a power cut is what leaks clusters.
# `wc -c` is portable (GNU + BSD/macOS); `stat -c` is GNU-only. The remote
# size check below runs on the MiSTer (always Linux) so it keeps `stat -c`.
LOCAL_SIZE="$(wc -c < "${BINARY}" | tr -d '[:space:]')"
run_scp "${BINARY}" "root@${MISTER_IP}:${REMOTE_PATH}.new"

run_ssh "root@${MISTER_IP}" "
    set -e
    new_size=\$(stat -c %s '${REMOTE_PATH}.new' 2>/dev/null || echo 0)
    if [ \$new_size -ne ${LOCAL_SIZE} ]; then
        echo \"Upload incomplete (\$new_size of ${LOCAL_SIZE} bytes); existing binary left untouched\" >&2
        rm -f '${REMOTE_PATH}.new'
        exit 1
    fi
    if [ -f '${REMOTE_PATH}' ]; then
        mv '${REMOTE_PATH}' '${REMOTE_PATH}.bak'
    fi
    mv '${REMOTE_PATH}.new' '${REMOTE_PATH}'
    sync
    echo 'Installed new binary (previous kept as ${REMOTE_PATH}.bak)'
"
echo "Deployed ${BINARY} → root@${MISTER_IP}:${REMOTE_PATH}"

run_ssh "root@${MISTER_IP}" "
    rm -f /tmp/zaparoo/frontend.log
    # Flush pending card writes before disrupting the frontend so it is never
    # pulled mid-write. The signal stays SIGKILL on purpose: MiSTer's wrapper
    # respawns the frontend ~1s later with the new binary, whereas a clean
    # SIGTERM exit is misclassified as an 'escape' and the wrapper refuses to
    # respawn. (SIGKILL counts as a crash toward the 3-strike give-up limit,
    # so after 3 deploys without a clean exit, killall MiSTer_Zaparoo to reset.)
    sync
    killall -KILL frontend 2>/dev/null && echo 'Killed running frontend; MiSTer will respawn it' || echo 'No running frontend to kill'
"
