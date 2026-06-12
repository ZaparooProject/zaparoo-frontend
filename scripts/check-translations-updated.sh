#!/usr/bin/env bash
set -euo pipefail

build_dir="${1:-build}"
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"
before="$(mktemp)"
after="$(mktemp)"
trap 'rm -f "$before" "$after"' EXIT

git diff -- src/ui/translations >"$before"
cmake --build "$build_dir" --target update_translations
git diff -- src/ui/translations >"$after"

if cmp -s "$before" "$after"; then
    exit 0
fi

if [[ "${GITHUB_ACTIONS:-}" == "true" ]]; then
    echo "::error::Qt translation catalogs are stale. Run cmake --build $build_dir --target update_translations and commit the src/ui/translations changes."
else
    echo "Qt translation catalogs are stale. Run cmake --build $build_dir --target update_translations and commit the src/ui/translations changes." >&2
fi

git diff -- src/ui/translations
exit 1
