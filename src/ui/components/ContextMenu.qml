// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import Zaparoo.Theme

// `entries` is a `var` array of plain JS objects (`{ id, label }`). The
// AOT compiler can't infer the shape of `var`, so every binding that
// reads `entries.length` or `modelData.label` falls back to the JS
// interpreter and trips the compiler category. Suppress file-wide.
// qmllint disable compiler

// Software-rendering safe contextual menu. It positions itself next to an
// anchor rectangle and clamps to the window bounds so edge tiles never push
// the menu off-screen. The scrim is drawn as four bands around `anchorRect`
// so the anchored tile stays bright while the rest of the screen dims —
// a full-screen scrim would defeat the "this menu is about *that* tile"
// affordance. When `anchorRadius` is set, four baked corner masks cut the
// bands' square hole down to the anchor's actual rounded silhouette instead
// of leaving bright square notches past its arcs.
Item {
    id: menu

    property bool open: false
    property rect anchorRect: Qt.rect(0, 0, 0, 0)
    // Corner radius of the anchored tile/row, in px. 0 (the default) keeps
    // today's square hole byte-identical -- this property is purely
    // additive. See Resources.cornerCutUrl() for the baked radius range.
    property int anchorRadius: 0
    // Each entry is `{ id: string, label: string }`. `id` is the dispatch
    // key the router switches on (e.g. "launch_game", "qr_code"); `label`
    // is the localized text. Position-keyed dispatch was a footgun —
    // dynamic per-owner menus silently re-shuffled the index/action map.
    property var entries: []
    property int currentIndex: 0
    property int bottomUnsafeHeight: Sizing.pctH(6) + Sizing.pctH(2)

    property bool _pressed: false
    property string _pendingId: ""

    signal accepted(string id)
    signal closeRequested

    readonly property int margin: Sizing.pctH(2)
    readonly property int gap: Sizing.pctW(1.2)
    readonly property int rowHeight: Sizing.pctH(6)
    readonly property int rowSpacing: Sizing.pctH(1)
    readonly property int horizontalPadding: Sizing.pctW(2)
    readonly property int panelSideMargin: Sizing.pctW(1)
    readonly property int _widestLabelWidth: _widestEntryLabelWidth(entries)
    readonly property int _usableBottom: Math.max(menu.margin, height - menu.bottomUnsafeHeight - menu.margin)
    // Only a floor against degenerate cases (a single tiny label) — real
    // menus size around `_desiredPanelWidth`, which tracks the longest
    // entry.
    readonly property int _minPanelWidth: Sizing.pctW(12)
    readonly property int _desiredPanelWidth: _widestLabelWidth + 2 * horizontalPadding + 2 * panelSideMargin + 2 * Sizing.stroke(2)
    readonly property int panelWidth: Math.min(Math.max(_minPanelWidth, _desiredPanelWidth), Math.max(0, width - 2 * margin))
    // Panel padding is independent of corner shape so tightening the radius
    // never crowds the first and last focused rows.
    readonly property int panelRadius: Sizing.radiusMd
    readonly property int panelVerticalPadding: Sizing.pctH(1.5)
    readonly property int panelHeight: Math.min(entries.length * rowHeight + Math.max(0, entries.length - 1) * rowSpacing + 2 * panelVerticalPadding, Math.max(0, _usableBottom - menu.margin))
    readonly property bool _fitsRight: anchorRect.x + anchorRect.width + gap + panelWidth <= width - margin
    readonly property bool _fitsLeft: anchorRect.x - gap - panelWidth >= margin
    readonly property bool _placeBesideAnchor: _fitsRight || _fitsLeft
    readonly property bool _fitsBelow: anchorRect.y + anchorRect.height + gap + panelHeight <= _usableBottom
    readonly property int preferredX: _placeBesideAnchor ? (_fitsRight ? Sizing.px(anchorRect.x + anchorRect.width + gap) : Sizing.px(anchorRect.x - gap - panelWidth)) : Sizing.px(anchorRect.x + Sizing.center(anchorRect.width, panelWidth))
    readonly property int preferredY: _placeBesideAnchor ? Sizing.px(anchorRect.y + Sizing.center(anchorRect.height, panelHeight)) : (_fitsBelow ? Sizing.px(anchorRect.y + anchorRect.height + gap) : Sizing.px(anchorRect.y - gap - panelHeight))

    visible: open
    enabled: visible
    anchors.fill: parent
    z: 250

    onOpenChanged: {
        if (open) {
            currentIndex = 0;
            menu._pressed = false;
            menu._pendingId = "";
        }
    }

    onEntriesChanged: {
        if (menu.entries.length <= 0) {
            currentIndex = 0;
            return;
        }
        if (menu.currentIndex >= menu.entries.length)
            currentIndex = menu.entries.length - 1;
    }

    function _widestEntryLabelWidth(source: var): int {
        let widest = 0;
        if (source === null || source === undefined)
            return widest;
        for (let i = 0; i < source.length; ++i) {
            const label = source[i] && source[i].label !== undefined ? String(source[i].label) : "";
            widest = Math.max(widest, Math.ceil(labelMetrics.advanceWidth(label)));
        }
        return widest;
    }

    function move(delta: int): void {
        if (menu.entries.length <= 0)
            return;
        menu.currentIndex = ((menu.currentIndex + delta) % menu.entries.length + menu.entries.length) % menu.entries.length;
    }

    function handleAction(action: string): void {
        if (action === "up")
            menu.move(-1);
        else if (action === "down")
            menu.move(1);
        else if (action === "accept") {
            if (menu.currentIndex >= 0 && menu.currentIndex < menu.entries.length)
                menu._commitAccept(menu.entries[menu.currentIndex].id);
        } else if (action === "cancel" || action === "context_menu")
            menu.closeRequested();
    }

    // Play the push-in cue on the focused row, then emit accepted(id)
    // deferred so the animation completes before the caller acts.
    function _commitAccept(id: string): void {
        menu._pendingId = id;
        menu._pressed = true;
        acceptCommit.arm();
    }

    DeferredAction {
        id: acceptCommit
        onDeferred: {
            const id = menu._pendingId;
            menu._pendingId = "";
            menu._pressed = false;
            if (id !== "")
                menu.accepted(id);
        }
    }

    FontMetrics {
        id: labelMetrics
        font.family: Theme.fontUi
        font.pixelSize: Sizing.fontCaption
    }

    // Catches dismiss-clicks on the dimmed area around the anchor.
    // Sits beneath the four scrim bands and the panel; per-row
    // MouseAreas inside the panel win for clicks on rows because the
    // panel is declared after this MouseArea, so row pointer handlers
    // sit on top in z-order. Clicks on the anchor area also hit this
    // MouseArea (the bands don't cover the anchor) and close the menu.
    // Clicks inside the panel chrome (top/bottom radius padding, side
    // margins, row spacing) are filtered out by the bounding-rect
    // check so a stray press on padding doesn't dismiss.
    //
    // Swallows hover and every mouse button so neither hover events
    // nor right-clicks bleed through to the underlying grid while the
    // menu is open. Without `hoverEnabled` and `Qt.AllButtons` the
    // grid below would highlight tiles under the scrim and a right
    // click on the dim area would land on the grid's context handler.
    MouseArea {
        anchors.fill: parent
        hoverEnabled: true
        acceptedButtons: Qt.AllButtons
        onClicked: mouse => {
            if (mouse.x < panel.x || mouse.y < panel.y || mouse.x > panel.x + panel.width || mouse.y > panel.y + panel.height)
                menu.closeRequested();
        }
    }

    // Shared, Sizing.px()-rounded hole edges. The four bands and the four
    // corner Images below both key off these so a corner piece always lands
    // flush with its two bands. Rounding a sum in one place and a difference
    // in another (the old per-band computation) could disagree by a pixel.
    readonly property int _holeLeft: Sizing.px(Math.max(0, menu.anchorRect.x))
    readonly property int _holeTop: Sizing.px(Math.max(0, menu.anchorRect.y))
    readonly property int _holeRight: Sizing.px(Math.max(0, menu.anchorRect.x + menu.anchorRect.width))
    readonly property int _holeBottom: Sizing.px(Math.max(0, menu.anchorRect.y + menu.anchorRect.height))
    // Skip the corner pieces rather than overlap them when the anchor is too
    // small to fit two radii across either axis.
    readonly property bool _canCutCorners: menu.anchorRadius > 0 && (menu._holeRight - menu._holeLeft) >= 2 * menu.anchorRadius && (menu._holeBottom - menu._holeTop) >= 2 * menu.anchorRadius

    // Four scrim bands framing `anchorRect`. The anchor area itself is
    // intentionally not painted so the tile the menu is *about* stays
    // bright. `Math.max(0, ...)` clamps every dimension so an anchor
    // flush against an edge collapses the matching band rather than
    // overflowing into negative territory.
    Rectangle {
        x: 0
        y: 0
        width: menu.width
        height: menu._holeTop
        color: Theme.scrim
    }
    Rectangle {
        x: 0
        y: menu._holeBottom
        width: menu.width
        height: Math.max(0, menu.height - menu._holeBottom)
        color: Theme.scrim
    }
    Rectangle {
        x: 0
        y: menu._holeTop
        width: menu._holeLeft
        height: Math.max(0, menu._holeBottom - menu._holeTop)
        color: Theme.scrim
    }
    Rectangle {
        x: menu._holeRight
        y: menu._holeTop
        width: Math.max(0, menu.width - menu._holeRight)
        height: Math.max(0, menu._holeBottom - menu._holeTop)
        color: Theme.scrim
    }

    // Baked antialiased quarter-disc masks (Part 5) cutting the four square
    // notches the bands above would otherwise leave past the anchor's real
    // rounded corners down to its actual silhouette. Each mask's alpha is
    // the exact complement of the tile's own corner coverage, so tile and
    // scrim coverage sum to 1 by construction -- no seam to tune.
    Image {
        objectName: "contextMenuCornerTl"
        visible: menu._canCutCorners
        x: menu._holeLeft
        y: menu._holeTop
        width: menu.anchorRadius
        height: menu.anchorRadius
        sourceSize.width: menu.anchorRadius
        sourceSize.height: menu.anchorRadius
        smooth: false
        cache: true
        source: menu._canCutCorners ? Resources.cornerCutUrl(menu.anchorRadius, "tl", Theme.scrim) : ""
    }
    Image {
        objectName: "contextMenuCornerTr"
        visible: menu._canCutCorners
        x: menu._holeRight - menu.anchorRadius
        y: menu._holeTop
        width: menu.anchorRadius
        height: menu.anchorRadius
        sourceSize.width: menu.anchorRadius
        sourceSize.height: menu.anchorRadius
        smooth: false
        cache: true
        source: menu._canCutCorners ? Resources.cornerCutUrl(menu.anchorRadius, "tr", Theme.scrim) : ""
    }
    Image {
        objectName: "contextMenuCornerBl"
        visible: menu._canCutCorners
        x: menu._holeLeft
        y: menu._holeBottom - menu.anchorRadius
        width: menu.anchorRadius
        height: menu.anchorRadius
        sourceSize.width: menu.anchorRadius
        sourceSize.height: menu.anchorRadius
        smooth: false
        cache: true
        source: menu._canCutCorners ? Resources.cornerCutUrl(menu.anchorRadius, "bl", Theme.scrim) : ""
    }
    Image {
        objectName: "contextMenuCornerBr"
        visible: menu._canCutCorners
        x: menu._holeRight - menu.anchorRadius
        y: menu._holeBottom - menu.anchorRadius
        width: menu.anchorRadius
        height: menu.anchorRadius
        sourceSize.width: menu.anchorRadius
        sourceSize.height: menu.anchorRadius
        smooth: false
        cache: true
        source: menu._canCutCorners ? Resources.cornerCutUrl(menu.anchorRadius, "br", Theme.scrim) : ""
    }

    Rectangle {
        id: panel

        x: Sizing.px(Math.max(menu.margin, Math.min(menu.preferredX, menu.width - menu.margin - menu.panelWidth)))
        y: Sizing.px(Math.max(menu.margin, Math.min(menu.preferredY, menu._usableBottom - menu.panelHeight)))
        width: menu.panelWidth
        height: menu.panelHeight
        color: Theme.bgPanel
        radius: menu.panelRadius

        Column {
            anchors.fill: parent
            anchors.topMargin: menu.panelVerticalPadding
            anchors.bottomMargin: menu.panelVerticalPadding
            anchors.leftMargin: menu.panelSideMargin
            anchors.rightMargin: menu.panelSideMargin
            spacing: menu.rowSpacing

            Repeater {
                model: menu.entries

                PressableSurface {
                    id: row

                    required property int index
                    required property var modelData

                    width: parent.width
                    height: menu.rowHeight
                    focused: index === menu.currentIndex
                    pressed: menu._pressed && row.modelData.id === menu._pendingId
                    pointerAcceptedButtons: Qt.LeftButton
                    pointerHoverEnabled: true
                    onPointerEntered: menu.currentIndex = row.index
                    onPointerClicked: menu._commitAccept(row.modelData.id)

                    Text {
                        // Centered as a box sized to this row's own measured
                        // text, not anchored left+right with AlignHCenter —
                        // a glyph run that straddles a half-pixel softens
                        // under the software renderer. See "Integer-pixel
                        // rules" in docs/qml-gotchas.md.
                        readonly property int _availableWidth: Math.max(0, parent.width - 2 * menu.horizontalPadding)
                        readonly property int _textWidth: Math.min(Math.ceil(labelMetrics.advanceWidth(row.modelData.label)), _availableWidth)

                        anchors.verticalCenter: parent.verticalCenter
                        x: Sizing.center(parent.width, _textWidth)
                        width: _textWidth
                        text: row.modelData.label
                        // Mirrors Tile.qml's caption: dim at rest, bright
                        // when focused. The ring alone is not the whole
                        // focus cue -- see PressableSurface.qml's doc
                        // comment on `_ringGap`/`_ringWidth`.
                        color: row.focused ? Theme.textPrimary : Theme.textLabel
                        font.family: Theme.fontUi
                        font.pixelSize: Sizing.fontCaption
                        elide: Text.ElideRight
                        renderType: Text.NativeRendering
                    }
                }
            }
        }
    }
}
