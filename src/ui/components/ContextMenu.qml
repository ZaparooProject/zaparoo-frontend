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
    property int bottomUnsafeHeight: Sizing.helpBarClearance

    property int _activatePulse: 0
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
    // The height every row would need stacked with no clamp -- what
    // `panelHeight` used to just return outright. Kept separate so
    // `_scrollable` below can tell "clamped to fit the screen" apart from
    // "every row already fits".
    readonly property int _fullContentHeight: entries.length * rowHeight + Math.max(0, entries.length - 1) * rowSpacing + 2 * panelVerticalPadding
    readonly property int panelHeight: Math.min(_fullContentHeight, Math.max(0, _usableBottom - menu.margin))
    // True once the entry count needs more room than the anchor position
    // leaves on screen -- the viewport below scrolls to keep
    // `currentIndex` in view rather than silently stranding rows past
    // `panel`'s clipped edge. Both app-authored menus and data-driven ones
    // ("Discover alt. versions" lists up to MAX_ALT_RESULTS from
    // alternate_versions.rs) rely on this.
    readonly property bool _scrollable: _fullContentHeight > panelHeight
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
            menu._pendingId = "";
            // A stale scroll position from a previous open would otherwise
            // carry over -- `rowViewport.contentY` isn't reset by the
            // Repeater rebuilding its delegates below, only by this.
            if (rowViewport)
                rowViewport.contentY = 0;
            // A fresh open destroys and recreates every row's Repeater
            // delegate (new `entries` array reference). Each new row's
            // SelectionBar binds `activatePulse` to this counter at
            // construction; if it were left at a stale nonzero value from a
            // previous open's accept, that bind is itself a 0 -> N change
            // and replays the flash on whichever row starts focused, with
            // no real activation behind it. Resetting to 0 here matches
            // SelectionBar's own declared default, so the bind is a no-op.
            menu._activatePulse = 0;
        }
    }

    onEntriesChanged: {
        // Callers can swap `entries` on an already-open menu (the
        // "Discover alt. versions" submenu replaces the main list in
        // place) -- reset scroll position rather than carry over whatever
        // the previous list had scrolled to.
        if (rowViewport)
            rowViewport.contentY = 0;
        if (menu.entries.length <= 0) {
            currentIndex = 0;
            return;
        }
        if (menu.currentIndex >= menu.entries.length)
            currentIndex = menu.entries.length - 1;
    }

    // `Math.max(advanceWidth, boundingRect.width)` plus `Sizing.stroke(2)`
    // hinting slack — the same corrected idiom
    // `ListPickerModal._measureLabelWidth` uses. `Text.NativeRendering`
    // lays out on integer, hinted per-glyph advances, which can paint a
    // few px wider than `advanceWidth()` alone's fractional, unhinted
    // total; a zero-slack fit then elided text that should have fit.
    // `panelWidthMetrics`'s own weight is fixed (Font.Medium), so unlike
    // `rowLabelMetrics` below there is no live-weight dependency to lose by
    // calling these as methods here.
    function _widestEntryLabelWidth(source: var): int {
        let widest = 0;
        if (source === null || source === undefined)
            return widest;
        for (let i = 0; i < source.length; ++i) {
            const label = source[i] && source[i].label !== undefined ? String(source[i].label) : "";
            widest = Math.max(widest, Math.ceil(Math.max(panelWidthMetrics.advanceWidth(label), panelWidthMetrics.boundingRect(label).width)) + Sizing.stroke(2));
        }
        return widest;
    }

    function move(delta: int): void {
        if (menu.entries.length <= 0)
            return;
        menu.currentIndex = ((menu.currentIndex + delta) % menu.entries.length + menu.entries.length) % menu.entries.length;
    }

    // Slides `rowViewport.contentY` just enough to bring the focused row
    // back inside the visible band -- no animation, matching
    // ListPickerModal._scrollCurrentIntoView(). Only matters once
    // `_scrollable`; a no-op otherwise since the whole content already
    // fits and contentY stays at 0.
    function _scrollCurrentIntoView(): void {
        const stride = menu.rowHeight + menu.rowSpacing;
        const top = menu.currentIndex * stride;
        const bottom = top + menu.rowHeight;
        if (top < rowViewport.contentY) {
            rowViewport.contentY = top;
        } else if (bottom > rowViewport.contentY + rowViewport.height) {
            rowViewport.contentY = bottom - rowViewport.height;
        }
    }

    onCurrentIndexChanged: menu._scrollCurrentIntoView()

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

    // Play the inverse-video activation flash on the focused row, then emit
    // accepted(id) deferred so the flash completes before the caller acts.
    function _commitAccept(id: string): void {
        menu._pendingId = id;
        menu._activatePulse++;
        acceptCommit.arm();
    }

    DeferredAction {
        id: acceptCommit
        onDeferred: {
            const id = menu._pendingId;
            menu._pendingId = "";
            if (id !== "")
                menu.accepted(id);
        }
    }

    // Sizes `_desiredPanelWidth` at the selected row's weight (Font.Medium
    // — see SelectionBar.qml's contentWeight), never the resting
    // Font.Normal — a per-row FontMetrics tracking each row's own live
    // weight handles individual label centering (see `rowLabelMetrics`
    // in the row delegate below). The resting weight is narrower or
    // equal, so no row can overflow this panel width once selected
    // regardless of which row that turns out to be.
    FontMetrics {
        id: panelWidthMetrics
        font.family: Theme.fontUi
        font.pixelSize: Sizing.fontCaption
        font.weight: Font.Medium
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
        antialiasing: Sizing.cornerAntialiasing
        // Safety net for a menu taller than the space the anchor leaves:
        // rows past `panelHeight` are cut off cleanly instead of painting
        // past the panel's rounded corners. A menu that fits never reaches
        // this edge.
        clip: true

        // Non-interactive (key navigation drives contentY, not dragging)
        // scrolling viewport -- see `_scrollCurrentIntoView()` above. Only
        // engages once `_scrollable`; below that `rowColumn.height` fits
        // inside `rowViewport` and contentY stays pinned at 0, so this is
        // visually identical to a plain-Column layout for any menu that
        // fits. Mirrors ListPickerModal.qml's `viewport`/`rowColumn`
        // construction.
        Flickable {
            id: rowViewport

            objectName: "contextMenuRowViewport"
            anchors.fill: parent
            anchors.topMargin: menu.panelVerticalPadding
            anchors.bottomMargin: menu.panelVerticalPadding
            anchors.leftMargin: menu.panelSideMargin
            anchors.rightMargin: menu.panelSideMargin
            contentWidth: width
            contentHeight: rowColumn.height
            clip: true
            interactive: false
            boundsBehavior: Flickable.StopAtBounds

            Column {
                id: rowColumn

                width: rowViewport.width
                spacing: menu.rowSpacing

                Repeater {
                    model: menu.entries

                    Item {
                        id: row

                        required property int index
                        required property var modelData

                        readonly property bool focused: index === menu.currentIndex

                        objectName: "contextMenuRow-" + row.index
                        width: parent.width
                        height: menu.rowHeight

                        // Inverse-video row -- see SelectionBar.qml and
                        // docs/style.md -> "Two registers". A menu entry is a
                        // choice from a vertical list, not an object to press;
                        // the accent bar carries focus on its own.
                        SelectionBar {
                            id: bar
                            objectName: "contextMenuSelectionBar"
                            anchors.fill: parent
                            active: row.focused
                            activatePulse: menu._activatePulse
                            radius: Sizing.radiusSm
                        }

                        // Tracks this row's own live weight (Normal at rest,
                        // Medium selected — bar.contentWeight) so the label's
                        // own centering box (`_textWidth` below) always
                        // matches its actual rendered glyph width. Measuring
                        // every row at the shared, fixed-Medium
                        // `panelWidthMetrics` instead would either drift
                        // resting rows off true center or, worse, elide a
                        // selected row's own label against a too-narrow box.
                        //
                        // `TextMetrics` (not `FontMetrics` + a Q_INVOKABLE
                        // `advanceWidth(text)` call), and its own `text:`
                        // binding, deliberately — `advanceWidth`/`boundingRect`
                        // as *properties* here genuinely re-evaluate when
                        // `font.weight: bar.contentWeight` changes; a property
                        // binding that only ever calls a method does not
                        // reliably re-run when a property read *inside* that
                        // method changes. Round 8 shipped the method-call form:
                        // a selected row repainted at Font.Medium while
                        // `_textWidth` stayed pinned to its Font.Normal
                        // measurement, eliding a label that fit. Matches
                        // ScrollingCaption.qml's `nameMetrics`/`tagsMetrics`.
                        TextMetrics {
                            id: rowLabelMetrics
                            objectName: "contextMenuRowLabelMetrics"
                            text: row.modelData.label
                            font.family: Theme.fontUi
                            font.pixelSize: Sizing.fontCaption
                            font.weight: bar.contentWeight
                        }

                        MouseArea {
                            anchors.fill: parent
                            hoverEnabled: true
                            acceptedButtons: Qt.LeftButton
                            cursorShape: Qt.PointingHandCursor
                            onEntered: menu.currentIndex = row.index
                            onClicked: {
                                menu.currentIndex = row.index;
                                menu._commitAccept(row.modelData.id);
                            }
                        }

                        Text {
                            // Centered as a box sized to this row's own measured
                            // text, not anchored left+right with AlignHCenter —
                            // a glyph run that straddles a half-pixel softens
                            // under the software renderer. See "Integer-pixel
                            // rules" in docs/qml-gotchas.md.
                            readonly property int _availableWidth: Math.max(0, parent.width - 2 * menu.horizontalPadding)
                            // Union of advance and painted bounds, plus
                            // `Sizing.stroke(2)` hinting slack — the same
                            // corrected idiom `ListPickerModal._measureLabelWidth`
                            // uses. `Text.NativeRendering` lays out on integer,
                            // hinted per-glyph advances, which can paint a few
                            // px wider than `advanceWidth` alone reports; a
                            // zero-slack fit then elided text that should have
                            // fit.
                            readonly property int _textWidth: Math.min(Math.ceil(Math.max(rowLabelMetrics.advanceWidth, rowLabelMetrics.boundingRect.width)) + Sizing.stroke(2), _availableWidth)

                            objectName: "contextMenuRowLabel"
                            anchors.verticalCenter: parent.verticalCenter
                            x: Sizing.center(parent.width, _textWidth)
                            width: _textWidth
                            text: row.modelData.label
                            // Inverse video on selection, matching
                            // SettingsField/BrowseList -- see SelectionBar.qml.
                            color: bar.active ? bar.contentColor : Theme.textPrimary
                            font.family: Theme.fontUi
                            font.pixelSize: Sizing.fontCaption
                            font.weight: bar.contentWeight
                            elide: Text.ElideRight
                            renderType: Text.NativeRendering
                        }
                    }
                }
            }
        }
    }
}
