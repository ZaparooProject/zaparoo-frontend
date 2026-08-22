// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

import QtQuick
import Zaparoo.Theme

// Genuinely blank grid slot — a structural placeholder, not a real Tile
// with nothing on it. PagedGrid resolves this in place of the normal
// per-item delegate for any model row whose `isEmpty` role is true (see
// `PagedGrid.qml`'s `emptyDelegate`). Paints nothing at rest; the cursor
// can still land on one (PagedGrid's own navigation treats every row the
// same regardless of `isEmpty` — it's the caller, e.g. HubScreen's
// activation guards, that refuses to act on one), and it shows the same
// accent focus ring every real tile uses so a landed-on slot still reads
// as "here". See docs/style.md -> "Empty slots" and HubScreen.qml's
// `_padToPageSize`.
//
// Not held-aware, unlike Tile.qml: a `blank`-kind layout entry is an
// implementation detail (a deliberate gap), not an interactable object —
// Options never opens on one, so it can never be armed for Move. Only a
// REAL tile lands ON a gap (absorbing it); the gap itself is never picked
// up. See HubScreen.qml's `handleAction` context_menu dispatch.
//
// A `Loader` does not forward its own properties onto the loaded item, so
// (like Tile.qml) this reads the contract off `parent` — its parent is
// PagedGrid's TileLoader, which sets these regardless of which component
// it ends up loading.
Item {
    id: root

    // qmllint disable missing-property compiler
    readonly property bool delegateIsSelected: parent.isSelected
    readonly property bool delegateIsFocused: parent.isFocused
    readonly property bool delegateFocusReady: parent.focusReady ?? true
    // qmllint enable missing-property compiler
    readonly property bool _focusedSelection: root.delegateIsSelected && root.delegateIsFocused && root.delegateFocusReady

    // No `layoutProfile` plumbing (unlike Tile.qml) — the only caller today
    // is the Hub, which never sets one, so its own `Sizing.radiusMd`/
    // `Sizing.focusRingWidth` defaults are exactly right. Add the same
    // `_surfaceProfile.cardRadius` override Tile.qml uses if a future
    // caller needs it.
    readonly property int _cornerRadius: Sizing.radiusMd
    readonly property int _outlineGap: Sizing.pctH(0.4)
    readonly property int _outlineWidth: Sizing.focusRingWidth

    anchors.fill: parent

    // A true hollow outline, NOT the filled-rect-punching-a-filled-rect
    // technique Tile.qml/PressableSurface.qml use for their ring (chosen
    // there to dodge Qt software AA's visible corner-stepping on thin
    // rounded borders, QTBUG-123210). That technique only works because
    // Tile always sits on a known opaque face color to punch back to
    // (`Theme.surfaceCard`) — an empty slot has no face at all, genuinely
    // blank, so there is no single color that's guaranteed correct to
    // punch back to behind it; a flat fill (even the page's own
    // `Theme.bgDeep`) would paint over whatever's actually behind the grid
    // if that's ever not a flat color. A `border`-based ring never
    // occludes anything, at the cost of the same corner-AA softness
    // Tile.qml avoids — acceptable here since this ring is far less
    // prominent (only visible while the cursor rests on a gap) than
    // Tile's, which is ubiquitous across the whole app.
    Rectangle {
        // Qt draws `border` inset from the Rectangle's own bounding box
        // (the band occupies the outer `border.width` px, inward) — same
        // footprint as the filled version's two rects: outer edge at
        // `_outlineGap` from the parent, band `_outlineWidth` px wide.
        anchors.fill: parent
        anchors.margins: root._outlineGap
        radius: Math.max(0, root._cornerRadius - root._outlineGap)
        color: "transparent"
        border.color: Theme.accent
        border.width: root._outlineWidth
        antialiasing: true
        visible: root._focusedSelection
    }
}
