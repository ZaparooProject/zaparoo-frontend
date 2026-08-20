// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
// Wraps a Tile-shaped delegate Component and exposes the four
// properties the Tile parent contract reads (see Tile.qml). PagedGrid
// and HubScreen's static category row both need this exact shape;
// centralizing it here means the contract lives in one place and is
// enforced at compile time via `required property` rather than only
// at runtime via Tile's self-check.

import QtQuick

// The loaded delegate reads these through `parent.X` because QML
// doesn't surface Loader's user-defined properties on the loaded item
// directly.
Loader {
    required property bool isSelected
    required property bool isFocused
    required property string name
    required property string coverKey
    // Optional compact label rendered above tile art. Mixed-system media views
    // use it for the system name; all other grids leave it empty.
    property string topLabel: ""
    property int favorite: 0
    property bool hidden: false
    // Newline-joined disambiguating-tag tokens (region, disc, rev, ...).
    // Default empty so hosts that don't wire it render no variant badges.
    property string disambiguatingTags: ""
    // Optional pulse counter — incremented by the host when the user
    // commits on the focused tile (forward navigation or game launch, which
    // share one physical press cue). Tile.qml reads it via
    // `parent.activatePulse` and lowers only the focused selection, so
    // hosts can safely forward the same counter to every TileLoader in a
    // row or grid.
    property int activatePulse: 0
    // Optional release counter — incremented by the host to raise the tile
    // after a launch that keeps the frontend on the
    // same screen. Tile.qml reads it via `parent.releasePulse`. Default 0 so
    // hosts that do not wire it are no-ops.
    property int releasePulse: 0
    // Set true while the host screen is inactive (off-screen). Tile.qml
    // watches this via `delegateSettling` to raise the face off-screen so a
    // held press does not persist when the user
    // returns to the screen.
    property bool settling: false
    // Gates whether the Tile renders its focused styling at all (ring +
    // focused cover ramp). The host leaves it false until the screen's focus
    // index is finalized (restore or first input) so a default-index tile
    // never paints a ring before the real selection lands. Default true so
    // hosts that do not wire it focus normally.
    property bool focusReady: true
    // Controls whether Tile instantiates its tinted focused-ramp Image source.
    // Default true preserves existing Hub/media behavior; large system grids
    // can restrict this to the selected tile to avoid cold-rendering one extra
    // SVG for every hidden delegate.
    property bool loadFocusedCover: true
    // Decode bundled artwork (system logos, category and UI glyphs) inline on
    // the GUI thread so the icon paints in the tile's first frame instead of
    // popping in one or more frames later. Tile.qml honors it only for keys
    // that route through the tinted-svg provider; real cover art and
    // color-style system PNGs stay on the reader thread regardless. Default
    // true, so hosts with a handful of tiles — Hub, Settings — get instant
    // icons with no wiring. Hosts that can put many tiles in one binding pass
    // (PagedGrid) narrow it themselves.
    property bool coverSynchronous: true
}
