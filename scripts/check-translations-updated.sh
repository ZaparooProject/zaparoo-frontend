#!/usr/bin/env bash
set -euo pipefail

build_dir="${1:-build}"
script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)"
repo_root=""
for candidate in "$script_dir/.." "${GITHUB_WORKSPACE:-}" "$PWD"; do
    if [[ -z "$candidate" ]]; then
        continue
    fi
    if top="$(git -c safe.directory="$candidate" -C "$candidate" rev-parse --show-toplevel 2>/dev/null)"; then
        repo_root="$top"
        break
    fi
done
if [[ -z "$repo_root" ]]; then
    echo "Unable to find git repository root for translation check." >&2
    echo "script_dir=$script_dir" >&2
    echo "GITHUB_WORKSPACE=${GITHUB_WORKSPACE:-}" >&2
    echo "PWD=$PWD" >&2
    exit 1
fi
before="$(mktemp)"
after="$(mktemp)"
trap 'rm -f "$before" "$after"' EXIT

git -c safe.directory="$repo_root" -C "$repo_root" diff -- src/ui/translations >"$before"
cmake --build "$repo_root/$build_dir" --target update_translations
git -c safe.directory="$repo_root" -C "$repo_root" diff -- src/ui/translations >"$after"

if cmp -s "$before" "$after"; then
    exit 0
fi

if [[ "${GITHUB_ACTIONS:-}" == "true" ]]; then
    echo "::error::Qt translation catalogs are stale. Run cmake --build $build_dir --target update_translations and commit the src/ui/translations changes."
else
    echo "Qt translation catalogs are stale. Run cmake --build $build_dir --target update_translations and commit the src/ui/translations changes." >&2
fi

git -c safe.directory="$repo_root" -C "$repo_root" diff -- src/ui/translations
exit 1
