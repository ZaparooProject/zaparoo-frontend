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
}
