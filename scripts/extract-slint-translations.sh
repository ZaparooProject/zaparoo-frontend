#!/usr/bin/env bash
# Zaparoo Frontend
# Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
# SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
#
# Regenerate the Slint frontend's gettext template from every .slint file.
# Paths are made relative to the repo root so the template is stable across
# checkouts; `--no-default-translation-context` matches build.rs.
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)"
repo_root="$(cd -- "$script_dir/.." && pwd -P)"
out="${1:-$repo_root/rust/frontend-slint/translations/frontend-slint.pot}"

cd "$repo_root"
mapfile -t files < <(find rust/frontend-slint/ui -name '*.slint' | sort)
slint-tr-extractor --no-default-translation-context \
    --package-name frontend-slint \
    -o "$out" "${files[@]}"
