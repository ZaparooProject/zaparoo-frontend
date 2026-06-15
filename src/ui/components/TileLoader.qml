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
    property int favorite: 0
    property bool hidden: false
    // Optional pulse counter — incremented by the host when the user
    // commits on the focused tile (forward navigation or game launch, which
    // share one push-in cue). Tile.qml reads it via `parent.activatePulse`
    // and only fires its animation when it is the focused selection, so
    // hosts can safely forward the same counter to every TileLoader in a
    // row or grid.
    property int activatePulse: 0
    // Set true while the host screen is inactive (off-screen). Tile.qml
    // watches this via `delegateSettling` to reset `_activateScale` back
    // to 1.0 off-screen so a held push-in does not persist when the user
    // returns to the screen.
    property bool settling: false
    // Gates the Tile focus-ring fade. The host leaves it false until the
    // user takes control of focus (first input) so the programmatic reseat
    // during state restore snaps instead of cross-fading. Default true so
    // hosts that do not wire it keep the fade on every focus change.
    property bool ringFadeReady: true
}
