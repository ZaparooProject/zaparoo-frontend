#!/bin/bash
# Zaparoo Frontend
# Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
# SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
#
# Builds the static ARM32 binary in the toolchain image (`just arm32`) and
# deploys it to a MiSTer over SSH/SCP as /media/fat/zaparoo/frontend, the path the
# MiSTer_Zaparoo wrapper starts. It is one static file with every font and
# logo embedded; nothing else is copied.
# Reads MISTER_IP (and optional MISTER_PW) from .env in the repo root.

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
ENV_FILE="${PROJECT_ROOT}/.env"
# Static musl build: the MiSTer rootfs ships an old glibc, so a dynamic
# build fails at load time with GLIBC_2.3x errors. Container builds use
# their own target directory (see scripts/toolchain.sh).
TARGET=armv7-unknown-linux-musleabihf
BINARY="${PROJECT_ROOT}/rust/target/docker/${TARGET}/release/frontend"
REMOTE_PATH="/media/fat/zaparoo/frontend"
SKIP_BUILD=0

while [ "$#" -gt 0 ]; do
    case "$1" in
        --skip-build) SKIP_BUILD=1; shift ;;
        -h|--help)
            echo "Usage: $0 [--skip-build]"
            echo ""
            echo "Builds the MiSTer binary and deploys it to ${REMOTE_PATH}."
            echo "  --skip-build  Deploy the existing binary without rebuilding"
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
    echo "Error: .env file not found at ${ENV_FILE}" >&2
    echo "Create it with: echo 'MISTER_IP=<your-mister-ip>' > .env" >&2
    exit 1
fi
# shellcheck source=/dev/null
source "${ENV_FILE}"
if [ -z "${MISTER_IP}" ]; then
    echo "Error: MISTER_IP is not set in ${ENV_FILE}" >&2
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
else
    echo "=== Building the MiSTer binary (${TARGET}, Cortex-A9) ==="
    (cd "${PROJECT_ROOT}" && just arm32)
fi

if [ ! -f "${BINARY}" ]; then
    echo "Error: ${BINARY} does not exist; run 'just arm32' first" >&2
    exit 1
fi

echo ""
echo "=== Deploying to MiSTer at ${MISTER_IP} ==="

# Upload to a side path first so an interrupted transfer can never clobber
# the working binary, and only rotate the old binary to .bak after a
# size-verified upload. `sync` forces the write to the card: exFAT has no
# journal, so a metadata update lost to a power cut is what leaks clusters.
# rename() replaces the directory entry even while the old binary is
# executing, where a cp over it would fail with ETXTBSY.
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
    chmod +x '${REMOTE_PATH}.new'
    if [ -f '${REMOTE_PATH}' ]; then
        mv '${REMOTE_PATH}' '${REMOTE_PATH}.bak'
    fi
    mv '${REMOTE_PATH}.new' '${REMOTE_PATH}'
    sync
    echo 'Installed new binary (previous kept as ${REMOTE_PATH}.bak)'
"
echo "Deployed ${BINARY} -> root@${MISTER_IP}:${REMOTE_PATH}"

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
