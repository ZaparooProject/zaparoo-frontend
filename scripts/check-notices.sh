#!/usr/bin/env bash
# Zaparoo Frontend
# Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
# SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
#
# Fail if the committed third-party notices are stale against the crate
# graph. The release packaging ships the file verbatim, so any dependency
# change must be followed by `just notices`.
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)"
repo_root="$(cd -- "$script_dir/.." && pwd -P)"
notices="$repo_root/rust/frontend/LICENSES/THIRD-PARTY-NOTICES.txt"

if ! command -v cargo-about >/dev/null 2>&1; then
    echo "cargo-about not found; run this through \`just lint\` or \`just notices\`, which use the toolchain image" >&2
    exit 1
fi

before="$(mktemp)"
trap 'rm -f "$before"' EXIT
cp "$notices" "$before"
"$script_dir/generate-notices.sh" > /dev/null

if ! diff -u "$before" "$notices"; then
    echo "rust/frontend/LICENSES/THIRD-PARTY-NOTICES.txt is stale; run \`just notices\` and commit the result." >&2
    exit 1
fi
