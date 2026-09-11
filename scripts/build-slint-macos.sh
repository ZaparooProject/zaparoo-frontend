#!/usr/bin/env bash
# Zaparoo Frontend
# Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
# SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
#
# Build the Slint frontend for the host macOS machine.  The repository's
# rust-toolchain.toml is authoritative; use rustup so Homebrew Rust cannot
# silently build it with a different compiler.

set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)"
PROJECT_ROOT="$(cd -- "$SCRIPT_DIR/.." && pwd -P)"
CARGO_BIN="${CARGO_HOME:-${HOME}/.cargo}/bin"

# Cargo-installed tools (including cross) live here.  Homebrew's rustup is
# keg-only, so also expose its bin directory when it is present.
if [ -d "$CARGO_BIN" ]; then
    export PATH="$CARGO_BIN:$PATH"
fi
if ! command -v rustup > /dev/null 2>&1 && command -v brew > /dev/null 2>&1; then
    BREW_RUSTUP_BIN="$(brew --prefix rustup 2> /dev/null || true)/bin"
    if [ -x "$BREW_RUSTUP_BIN/rustup" ]; then
        export PATH="$BREW_RUSTUP_BIN:$PATH"
    fi
fi

if ! command -v rustup > /dev/null 2>&1; then
    cat >&2 <<'EOF'
Error: Rustup is required for Slint builds.

Install it with:
  brew install rustup-init
  rustup-init -y --default-toolchain 1.97.0 --profile minimal
EOF
    exit 1
fi

echo "=== Building Slint frontend for macOS ==="
(cd "$PROJECT_ROOT/rust" && rustup run 1.97.0 cargo build -p frontend-slint --release)
echo "Built: $PROJECT_ROOT/rust/target/release/frontend-slint"
