#!/bin/bash
# Zaparoo Frontend
# Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
# SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
#
# Install the frontend on a Steam Deck and add it to the Steam library as a
# non-Steam game. Run this on the Deck itself, with Steam running.
#
# Being Steam-owned is the point: Steam then gives the frontend the Steam
# Input layout, the overlay and the Quick Access Menu, none of which reach a
# process Steam did not launch. The frontend still claims the compositor for
# itself (rust/frontend/src/gamescope.rs), because Steam hands focus back to
# its own shell when a launched game exits.
#
# The eventual home for this is Core's installer, which already adds the
# Zaparoo Runtime shortcut the same way; this script is how the frontend
# gets there until then.
#
# Usage: install-steamos.sh [path-to-binary]
# Default binary: rust/target/docker/x86_64-unknown-linux-gnu/release/frontend
# (`just x86-portable`). A plain `cargo build --release` links the
# build host's glibc and will not start here.
set -euo pipefail

APP_NAME="Zaparoo"
BIN_DEST="$HOME/.local/bin/zaparoo-frontend"
DESKTOP_DEST="$HOME/.local/share/applications/zaparoo-frontend.desktop"

if [ "$(id -u)" = "0" ]; then
    echo "Error: run as the deck user, not root" >&2
    exit 1
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"
SRC="${1:-$PROJECT_ROOT/rust/target/docker/x86_64-unknown-linux-gnu/release/frontend}"
if [ ! -x "$SRC" ]; then
    echo "Error: no executable at $SRC" >&2
    echo "Build one with: just x86-portable" >&2
    exit 1
fi

mkdir -p "$(dirname "$BIN_DEST")" "$(dirname "$DESKTOP_DEST")"
# Staged copy so an install over a running frontend does not hit ETXTBSY.
install -m 0755 "$SRC" "$BIN_DEST.new"
mv -f "$BIN_DEST.new" "$BIN_DEST"
echo "Installed $BIN_DEST"

# No --fullscreen: SteamOS is its own runtime and starts fullscreen already.
cat > "$DESKTOP_DEST" <<EOF_DESKTOP
[Desktop Entry]
Type=Application
Name=$APP_NAME
Comment=Browse and launch your library
Exec="$BIN_DEST"
Icon=zaparoo
Terminal=false
Categories=Game;
EOF_DESKTOP
echo "Wrote $DESKTOP_DEST"

shortcuts=("$HOME"/.steam/steam/userdata/*/config/shortcuts.vdf)
for shortcuts_file in "${shortcuts[@]}"; do
    if [ -f "$shortcuts_file" ] && strings "$shortcuts_file" | grep -F "$BIN_DEST" >/dev/null; then
        echo "Steam shortcut already present; binary updated in place."
        exit 0
    fi
done

if ! command -v steamos-add-to-steam >/dev/null; then
    echo "steamos-add-to-steam not found. Add $DESKTOP_DEST to Steam by hand." >&2
    exit 1
fi
steamos-add-to-steam "$DESKTOP_DEST"
echo
echo "Added '$APP_NAME' to Steam. Restart Steam or reboot once if it does not"
echo "appear under Library > Non-Steam, then launch it from Game Mode."
