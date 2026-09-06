#!/bin/bash
# Zaparoo Frontend
# Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
# SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
#
# Slint frontend: cross-builds the ARM32 binary with `cross` and deploys
# it to a MiSTer over SSH/SCP. It is one static file with every font and
# logo embedded, like the Qt binary; nothing else is copied. By default it
# lands beside the Qt frontend as /media/fat/zaparoo/frontend-slint for
# manual testing; --replace backs up the Qt frontend and installs the
# Slint build in its place so Main's spawn path launches it.
# Reads MISTER_IP (and optional MISTER_PW) from .env in the repo root.

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
ENV_FILE="${PROJECT_ROOT}/.env"
# Static musl build: the MiSTer image ships glibc 2.31 and cross's
# gnueabihf image links newer glibc symbols, so dynamic builds fail
# at load time with GLIBC_2.3x errors.
TARGET=armv7-unknown-linux-musleabihf
BINARY="${PROJECT_ROOT}/rust/target/${TARGET}/release/frontend-slint"
REMOTE_DIR="/media/fat/zaparoo"
SKIP_BUILD=0
REPLACE=0

while [ "$#" -gt 0 ]; do
    case "$1" in
        --skip-build) SKIP_BUILD=1; shift ;;
        --replace)    REPLACE=1; shift ;;
        -h|--help)
            echo "Usage: $0 [--skip-build] [--replace]"
            echo "  --skip-build  Deploy the existing binary without rebuilding"
            echo "  --replace     Install as ${REMOTE_DIR}/frontend (backs up the Qt binary once)"
            exit 0
            ;;
        *) echo "Error: unknown argument: $1" >&2; exit 1 ;;
    esac
done

if [ "${SKIP_BUILD}" -eq 0 ]; then
    echo "=== Cross-building frontend-slint (${TARGET}, Cortex-A9) ==="
    (cd "${PROJECT_ROOT}/rust" &&
        ZAPAROO_RESOURCES_DIR="${PROJECT_ROOT}/resources" \
        RUSTFLAGS="-C target-cpu=cortex-a9" \
        cross build -p frontend-slint --release \
            --no-default-features --features mister --target "${TARGET}")
fi

if [ ! -f "${BINARY}" ]; then
    echo "Error: ${BINARY} does not exist" >&2
    exit 1
fi

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
        echo "Error: MISTER_PW is set but sshpass is not installed." >&2
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

echo "=== Deploying to ${MISTER_IP} ==="
run_scp "${BINARY}" "root@${MISTER_IP}:${REMOTE_DIR}/frontend-slint"
run_ssh "root@${MISTER_IP}" "chmod +x ${REMOTE_DIR}/frontend-slint"

if [ "${REPLACE}" -eq 1 ]; then
    echo "=== Installing as ${REMOTE_DIR}/frontend (Qt binary backed up) ==="
    # cp over the live binary fails with ETXTBSY while it is executing;
    # rename() replaces the directory entry regardless, so stage a copy
    # and mv it into place.
    run_ssh "root@${MISTER_IP}" "
        set -e
        if [ -f ${REMOTE_DIR}/frontend ] && [ ! -f ${REMOTE_DIR}/frontend.qt-backup ]; then
            cp ${REMOTE_DIR}/frontend ${REMOTE_DIR}/frontend.qt-backup
        fi
        cp ${REMOTE_DIR}/frontend-slint ${REMOTE_DIR}/frontend.new
        mv ${REMOTE_DIR}/frontend.new ${REMOTE_DIR}/frontend
        sync
    "
    echo "Restore the Qt build with:"
    echo "  ssh root@${MISTER_IP} 'cp ${REMOTE_DIR}/frontend.qt-backup ${REMOTE_DIR}/frontend'"
else
    echo "Deployed as ${REMOTE_DIR}/frontend-slint (Qt frontend untouched)."
    echo "Test over SSH: kill the running frontend, then run it manually:"
    echo "  ssh root@${MISTER_IP} '${REMOTE_DIR}/frontend-slint'"
fi
echo "=== Done ==="
