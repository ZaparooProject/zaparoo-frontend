#!/usr/bin/env bash
# Zaparoo Frontend
# Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
# SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
#
# Desktop dev run of the Slint frontend against the mock Core.
#
# The mock is started here when nothing is already serving the dev port,
# and stopped again when the frontend exits, so one command is the whole
# loop. A mock (or a real Core) already on the port is left alone: it is
# not ours to stop.
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)"
repo_root="$(cd -- "$script_dir/.." && pwd -P)"
host="127.0.0.1"
# Deliberately offset from the real Core's 7497 so dev never collides
# with a running Core.
port="27497"
endpoint="ws://${host}:${port}/api/v0.1"
mock_log="$repo_root/output/mock-core-dev.log"

mock_pid=""
cleanup() {
    if [ -n "$mock_pid" ] && kill -0 "$mock_pid" 2> /dev/null; then
        echo "stopping the mock Core (pid $mock_pid)"
        kill "$mock_pid" 2> /dev/null || true
        wait "$mock_pid" 2> /dev/null || true
    fi
}
trap cleanup EXIT INT TERM

serving() {
    (exec 3<> "/dev/tcp/${host}/${port}") 2> /dev/null
}

if [ -n "${ZAPAROO_CORE_ENDPOINT:-}" ]; then
    echo "using the requested Core ${ZAPAROO_CORE_ENDPOINT}"
elif serving; then
    echo "using the Core already serving ${endpoint}"
else
    # Build first and run the binary directly: `cargo run` would leave
    # the mock orphaned behind the cargo process when this script exits.
    (cd "$repo_root/rust" && cargo build --bin mock-core)
    mkdir -p "$(dirname "$mock_log")"
    "$repo_root/rust/target/debug/mock-core" > "$mock_log" 2>&1 &
    mock_pid=$!
    echo "started the mock Core (pid $mock_pid), logging to $mock_log"
    for _ in $(seq 1 100); do
        if serving; then
            break
        fi
        if ! kill -0 "$mock_pid" 2> /dev/null; then
            echo "the mock Core exited before it served ${endpoint}:" >&2
            tail -n 20 "$mock_log" >&2
            exit 1
        fi
        sleep 0.1
    done
    if ! serving; then
        echo "the mock Core did not start serving ${endpoint} in time" >&2
        exit 1
    fi
fi

cd "$repo_root/rust"
export ZAPAROO_CORE_ENDPOINT="${ZAPAROO_CORE_ENDPOINT:-$endpoint}"
# Slint's embedded MCP server is opt-in and development-only. Debug
# metadata enables element discovery; the normal dev/release build is
# unchanged when no port is requested.
cargo_args=()
if [ -n "${SLINT_MCP_PORT:-}" ]; then
    export SLINT_EMIT_DEBUG_INFO=1
    cargo_args+=(--features slint/mcp)
fi
cargo run -p frontend-slint "${cargo_args[@]}" -- "$@"
