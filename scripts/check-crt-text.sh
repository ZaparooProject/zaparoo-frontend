#!/usr/bin/env bash
# Zaparoo Frontend
# Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
# SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
#
# Fail when any CRT snapshot draws a label that starts off a whole pixel.
# Slint's software renderer places glyphs at quarter-pixel positions, which
# smears the 6x8 bitmap face, so every CRT label has to start on the pixel
# grid (docs/style.md, "Integer-pixel drawing"). Renders each CRT fixture at
# NTSC and PAL height, and at NTSC with the help bar hidden, with the
# snapshot tool's text probe on.
#
# The report names the label and where it starts. Build with
# SLINT_EMIT_DEBUG_INFO=1 to have it name the .slint elements as well.
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)"
repo_root="$(cd -- "$script_dir/.." && pwd -P)"

cd "$repo_root/rust"
cargo build -q -p frontend --features snapshot --bin snapshot
snapshot="${CARGO_TARGET_DIR:-target}/debug/snapshot"
frame="$(mktemp --suffix=.png 2> /dev/null || mktemp)"
trap 'rm -f -- "$frame"' EXIT

# shellcheck source=scripts/snapshot-screens.sh
. "$script_dir/snapshot-screens.sh"

failures=0
for pass in "240 on" "288 on" "240 off"; do
    read -r height help_bar <<< "$pass"
    for screen in "${crt_screens[@]}"; do
        report="$(ZAPAROO_SNAPSHOT_TEXT_PROBE=1 ZAPAROO_SNAPSHOT_HELP_BAR="$help_bar" \
            "$snapshot" 352 "$height" "$frame" "$screen" | grep '^off-grid text:' || true)"
        if [ -n "$report" ]; then
            while IFS= read -r line; do
                echo "$screen 352x$height help bar $help_bar: ${line#off-grid text: }"
            done <<< "$report"
            failures=$((failures + $(wc -l <<< "$report")))
        fi
    done
done

if [ "$failures" -gt 0 ]; then
    echo "$failures CRT label(s) start off the pixel grid" >&2
    exit 1
fi
echo "CRT labels: all on the pixel grid"
