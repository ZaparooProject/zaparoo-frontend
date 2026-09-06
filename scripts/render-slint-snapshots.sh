#!/usr/bin/env bash
# Zaparoo Frontend
# Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
# SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
#
# Render the Slint frontend's screens offline through the software renderer
# (the same pipeline the MiSTer build presents through) at the tiers the
# parity ledger compares: 240p CRT, 540p, 720p and 1080p. Output lands in
# output/snapshots/<screen>-<w>x<h>.png; put the matching Qt screenshots
# beside them for the side-by-side sheet (docs/plans/slint-migration.md,
# step 1 S4). Pass a language (e.g. `de`) as the first argument to render
# through a bundled translation.
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)"
repo_root="$(cd -- "$script_dir/.." && pwd -P)"
out="$repo_root/output/snapshots"
lang="${1:-}"

cd "$repo_root/rust"
cargo build -q -p frontend-slint --features snapshot --bin snapshot
snapshot="$repo_root/rust/target/debug/snapshot"
mkdir -p "$out"

digital_screens=(hub systems systems-list favorite-systems games games-list favorites favorites-list recents settings settings-page setup setup-picker about context letters dialog saver)
crt_screens=(crt-hub crt-systems crt-games crt-games-list crt-favorites crt-settings crt-settings-page crt-setup calibration)

render() {
    local w="$1" h="$2" screen="$3"
    local file="$out/${screen}-${w}x${h}${lang:+-$lang}.png"
    if [ -n "$lang" ]; then
        ZAPAROO_SNAPSHOT_LANG="$lang" "$snapshot" "$w" "$h" "$file" "$screen" > /dev/null
    else
        "$snapshot" "$w" "$h" "$file" "$screen" > /dev/null
    fi
    echo "$file"
}

for size in "960 540" "1280 720" "1920 1080"; do
    # shellcheck disable=SC2086
    set -- $size
    for screen in "${digital_screens[@]}"; do
        render "$1" "$2" "$screen"
    done
done
for screen in "${crt_screens[@]}"; do
    render 352 240 "$screen"
done
