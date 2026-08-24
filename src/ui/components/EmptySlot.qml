// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

import QtQuick

// Genuinely blank grid slot — a structural placeholder, not a real Tile
// with nothing on it. PagedGrid resolves this in place of the normal
// per-item delegate for any model row whose `isEmpty` role is true (see
// `PagedGrid.qml`'s `emptyDelegate`). See docs/style.md -> "Empty slots"
// and HubScreen.qml's `_padToPageSize`.
//
// Paints nothing, ever — not even a focus ring. `PagedGrid.skipEmptyCells`
// (which the Hub sets outside a Move session) makes every cursor path treat
// an `isEmpty` row as unreachable, so the cursor cannot rest here in normal
// browsing; the one exception, a Move session picking a blank as its
// destination, swaps the real held tile into the cell within the same
// synchronous call, so no frame ever actually paints this component under
// the cursor either. There is therefore no "landed here, show a focus ring"
// state left for this component to render — a ring drawn over a face-less
// Item never read as a real focus cursor in the first place (it has nothing
// to wrap), which is why the ring was removed rather than restyled once
// landing here stopped being reachable.
//
// A `blank`-kind layout entry is an implementation detail (a deliberate
// gap), not an interactable object — Options never opens on one, and
// (per the above) the cursor never rests on one outside the single-frame
// Move swap. Only a REAL tile lands ON a gap (absorbing it); the gap
// itself is never picked up. See HubScreen.qml's `handleAction`
// context_menu dispatch.
Item {
    // Fills its cell even though it paints nothing -- geometry-only, kept
    // so this behaves like an ordinary delegate rather than collapsing to
    // 0x0, in case a future caller ever reads `TileLoader.item`'s size.
    anchors.fill: parent

    // Otherwise deliberately empty. A `Loader` does not forward its own
    // properties onto the loaded item, so (like Tile.qml) a future caller
    // that needs to read the delegate contract (`isSelected`/`isFocused`/
    // etc., set by PagedGrid's TileLoader on `parent`) would read it off
    // `parent` the same way Tile.qml does.
}
