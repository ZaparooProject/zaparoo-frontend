#!/usr/bin/env bash
# Zaparoo Frontend
# Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
# SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
#
# Fail if the committed rust/frontend/LICENSE.txt is stale against
# COPYING, LICENSES/ or the crate graph. The release bundles ship it and
# the binary embeds it, so any change to those inputs must be followed by
# `just notices`.
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)"
repo_root="$(cd -- "$script_dir/.." && pwd -P)"
notices="$repo_root/rust/frontend/LICENSE.txt"

if ! command -v cargo-about >/dev/null 2>&1; then
    echo "cargo-about not found; run this through \`just lint\` or \`just notices\`, which use the toolchain image" >&2
    exit 1
fi

before="$(mktemp)"
trap 'rm -f "$before"' EXIT
if [ -f "$notices" ]; then
    cp "$notices" "$before"
fi
"$script_dir/generate-notices.sh" > /dev/null

if ! diff -u "$before" "$notices"; then
    echo "rust/frontend/LICENSE.txt is stale; run \`just notices\` and commit the result." >&2
    exit 1
fi
