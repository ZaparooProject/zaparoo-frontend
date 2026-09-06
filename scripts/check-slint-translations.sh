#!/usr/bin/env bash
# Zaparoo Frontend
# Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
# SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
#
# Fail if the committed gettext template for the Slint frontend is stale
# against the `@tr()` strings in the .slint files. Mirrors what
# check-translations-updated.sh does for the Qt catalogs: source locations
# are part of the template, so any .slint edit that moves a translatable
# string must be followed by `just slint-tr-extract`.
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)"
repo_root="$(cd -- "$script_dir/.." && pwd -P)"
pot="$repo_root/rust/frontend-slint/translations/frontend-slint.pot"

if ! command -v slint-tr-extractor >/dev/null 2>&1; then
    echo "slint-tr-extractor not found; install it with: cargo install slint-tr-extractor --version 1.17.1" >&2
    exit 1
fi

fresh="$(mktemp)"
trap 'rm -f "$fresh"' EXIT
"$script_dir/extract-slint-translations.sh" "$fresh"

# The creation date is the only field that legitimately differs.
if ! diff -u <(grep -v '^"POT-Creation-Date' "$pot") <(grep -v '^"POT-Creation-Date' "$fresh"); then
    echo "rust/frontend-slint/translations/frontend-slint.pot is stale; run \`just slint-tr-extract\` and commit the result." >&2
    exit 1
fi
