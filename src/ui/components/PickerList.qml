// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import Zaparoo.Theme

// `entries` is a `var` array of plain JS objects (`{ id, label }`). The
// AOT compiler can't infer the shape of `var`, so reads of
// `entries.length` and `modelData.label` fall back to the JS interpreter
// and trip the compiler category. Suppress file-wide.
// qmllint disable compiler

// The "pick one of these" list body. Two kinds of host: `ListPickerModal`,
// the stand-alone picker every screen opens, and modals that offer a
// choice on one of their own rows (`ScrapeSetupModal`, `IndexSetupModal`),
// which swap their panel content to this list and back instead of opening
// a second modal on top of themselves -- see docs/style.md -> "Modal
// depth".
//
// Pure presentation, the contract `ListPickerModal` has always exposed:
// navigates `currentIndex` on up/down, emits `accepted(id)` on accept
// (after the row's push cue plays), `closeRequested()` on cancel. The host
// gives the list its width and sizes its own panel from
// `desiredContentWidth`; the list sizes its height from the entry count,
// capped at `_maxViewportHeight` with scroll arrows beyond that.
//
// `active` is "the host is showing this list right now": the picker
// modal's `open`, or a setup modal's picker page. Turning it on re-applies
// `initialId` and clears a pending accept (what `ListPickerModal`'s
// `onOpenChanged` did before the split); while it is off an entries swap
// leaves focus alone.
Item {
    id: list

    property bool active: true
    // Each entry is `{ id: string, label: string }`. `id` is the dispatch
    // key emitted by `accepted`; `label` is the localized display text.
    // Position-keyed dispatch is a footgun - dynamic entry sets silently
    // re-shuffle the index/action map.
    property var entries: []
    // Optional. When `active` flips true, sets `currentIndex` to the entry
    // whose id matches. Empty string or no match falls back to 0.
    property string initialId: ""
    property int currentIndex: 0
    // True while a host-owned save is in flight and the list must stay
    // frozen on the row it's showing progress for. Disables each row's
    // MouseArea entirely (hover-to-focus and click-to-accept both go
    // through it) -- the keyboard/gamepad path is gated by the host not
    // forwarding handleAction() at all while this is true (see Main.qml's
    // modalListPicker routing branch).
    property bool locked: false

    property int _activatePulse: 0
    property string _pendingId: ""

    signal accepted(string id)
    signal closeRequested

    readonly property int _rowHeight: Sizing.pctH(7)
    readonly property int _rowSpacing: Sizing.pctH(1)
    // Cap the viewport at a portion of the screen height so it never grows
    // past what a modal panel can reasonably contain. Visible row count
    // falls out of this - `floor((max + spacing) / (rowHeight + spacing))`
    // gives the row count whose viewport fits inside `_maxViewportHeight`,
    // with at least 1 row.
    readonly property int _maxViewportHeight: Sizing.pctH(60)
    readonly property int _visibleRows: Math.max(1, Math.min(entries.length, Math.floor((_maxViewportHeight + _rowSpacing) / (_rowHeight + _rowSpacing))))
    readonly property int _viewportHeight: _visibleRows * _rowHeight + Math.max(0, _visibleRows - 1) * _rowSpacing
    readonly property int _contentHeight: Math.max(1, entries.length) * _rowHeight + Math.max(0, entries.length - 1) * _rowSpacing
    readonly property bool _scrollable: entries.length > _visibleRows
    readonly property int _scrollArrowSize: Sizing.pctH(3)
    readonly property int _scrollArrowGap: Sizing.pctH(0.5)
    readonly property int _scrollIndicatorBand: _scrollable ? _scrollArrowSize + _scrollArrowGap : 0
    readonly property int _viewportSlotHeight: _viewportHeight + 2 * _scrollIndicatorBand
    readonly property bool _hasContentAbove: viewport.contentY > 1
    readonly property bool _hasContentBelow: viewport.contentY + viewport.height < viewport.contentHeight - 1

    // Content-driven width, mirroring ContextMenu.qml's
    // `_widestEntryLabelWidth` pattern (Modal.qml can't measure this itself --
    // shell content is an opaque caller-supplied Item to it) -- see
    // docs/style.md -> "Content-driven modal width".
    readonly property int _rowHorizontalPadding: Sizing.pctW(2)
    // `Math.max(advanceWidth, boundingRect.width)` plus one `Sizing.stroke(2)`
    // of slack (the TopStatusStrip.qml `_titleMeasuredWidth` pattern) -- the
    // label paints with `renderType: Text.NativeRendering`, which lays out on
    // integer, hinted per-glyph advances, while `advanceWidth()` alone
    // returns QFontMetricsF's fractional, unhinted total; a long string can
    // paint a few px wider than that alone measures, and a zero-slack fit
    // then elides text that should have fit. See docs/style.md -> "Preset
    // catalog" / picker sizing.
    //
    // `fontSize`/`fontFamily`/`fontWeight` are unused inside -- `metrics`'
    // own font.* bindings already keep it in sync -- but taking them as
    // explicit parameters, rather than letting only `metrics.font.*` carry
    // the dependency, is what makes callers re-evaluate on a
    // font/tier/weight change: `FontMetrics.advanceWidth()`/
    // `boundingRect()` are Q_INVOKABLE method calls, and a property binding
    // that only ever calls a method does not reliably re-run when a
    // property read *inside* that method changes. `fontWeight` was added
    // in round 9 -- a selected row's `bar.contentWeight` flips Normal ->
    // Medium (round 8's inverse-video weight step), and without it as a
    // captured parameter a row's own `_textWidth` stayed at its
    // Normal-weight measurement while the Text repainted bold, eliding a
    // label that had actually fit. Matches ContextMenu.qml's
    // `_widestEntryLabelWidth(entries)`.
    function _measureLabelWidth(metrics: FontMetrics, label: string, fontSize: int, fontFamily: string, fontWeight: int): int {
        return Math.ceil(Math.max(metrics.advanceWidth(label), metrics.boundingRect(label).width)) + Sizing.stroke(2);
    }
    // Measured at the selected row's weight (Font.Medium -- see
    // SelectionBar.qml's contentWeight), not the resting Font.Normal --
    // otherwise the widest label could elide once selected. Each row's
    // own `_textWidth` below tracks its own live weight via a per-row
    // `rowLabelMetrics` instead, so individual labels stay precisely
    // centered at rest too.
    readonly property int _widestEntryLabelWidth: {
        let widest = 0;
        for (let i = 0; i < list.entries.length; ++i) {
            const label = list.entries[i] && list.entries[i].label !== undefined ? String(list.entries[i].label) : "";
            widest = Math.max(widest, list._measureLabelWidth(panelWidthLabelMetrics, label, Sizing.fontBody, Theme.fontUi, Font.Medium));
        }
        return widest;
    }
    // Optional per-entry color-swatch preview (the color-scheme picker).
    // `entries[i].swatch` is a 3-color array or undefined; the list is
    // homogeneous (either every entry carries one or none do), so checking
    // the first entry is enough to flag the whole list into swatch layout.
    // See docs/style.md -> "Picker swatch preview".
    readonly property bool _hasSwatchPreview: list.entries.length > 0 && list.entries[0].swatch !== undefined
    readonly property int _swatchBoxSize: Sizing.pctH(2.4)
    readonly property int _swatchGap: Sizing.pctW(0.6)
    readonly property int _swatchLabelGap: Sizing.pctW(2)
    readonly property int _swatchBandWidth: list._hasSwatchPreview ? 3 * list._swatchBoxSize + 2 * list._swatchGap : 0
    // What the host adds its own title, margins and clearance to when it
    // sizes a panel around this list.
    readonly property int desiredContentWidth: list._widestEntryLabelWidth + (list._hasSwatchPreview ? list._swatchBandWidth + list._swatchLabelGap : 0) + 2 * list._rowHorizontalPadding

    height: list._viewportSlotHeight

    onActiveChanged: {
        if (!list.active) {
            // Disarm a pending accept so a press-then-leave inside the
            // deferred window cannot apply a selection after the host moved on.
            acceptCommit.stop();
            return;
        }
        list.reset();
    }

    // Hosts swap `entries` in place on an already-showing list (the launcher
    // flow replaces a one-row "saving" list with a three-row error list).
    // Without this the stale currentIndex can sit past the new list's end, so
    // no row renders focused and Accept is a dead key until the user moves.
    onEntriesChanged: {
        if (list.active)
            list._applyInitialIndex();
        // Any entries change -- active or not -- destroys and recreates every
        // row's Repeater delegate (new array reference). Each new row's
        // SelectionBar binds `activatePulse` to this counter at
        // construction; a stale nonzero value left over from a previous
        // accept is itself a 0 -> N change to that fresh binding, and
        // replays the flash on whichever row starts focused with no real
        // activation behind it. Resetting unconditionally (not just while
        // active) matches SelectionBar's own declared default, so every fresh
        // bind is a no-op until a real accept happens.
        list._activatePulse = 0;
    }

    // Keep the focused row in view. When the current index moves above
    // or below the visible band we slide contentY just enough to bring
    // it back into view, no animation - software renderer pays per-frame
    // for any motion under translucent content.
    onCurrentIndexChanged: list._scrollCurrentIntoView()

    // Re-apply `initialId` and drop any pending accept: what a host does when
    // it starts showing the list.
    function reset(): void {
        list._applyInitialIndex();
        list._pendingId = "";
    }

    function _applyInitialIndex(): void {
        let next = 0;
        if (list.initialId !== "") {
            for (let i = 0; i < list.entries.length; ++i) {
                if (list.entries[i].id === list.initialId) {
                    next = i;
                    break;
                }
            }
        }
        viewport.contentY = 0;
        list.currentIndex = next;
        list._scrollCurrentIntoView();
    }

    function _scrollCurrentIntoView(): void {
        const stride = list._rowHeight + list._rowSpacing;
        const top = list.currentIndex * stride;
        const bottom = top + list._rowHeight;
        if (top < viewport.contentY) {
            viewport.contentY = top;
        } else if (bottom > viewport.contentY + viewport.height) {
            viewport.contentY = bottom - viewport.height;
        }
    }

    function move(delta: int): void {
        if (list.entries.length <= 0)
            return;
        const len = list.entries.length;
        list.currentIndex = ((list.currentIndex + delta) % len + len) % len;
    }

    function handleAction(action: string): void {
        if (action === "up") {
            list.move(-1);
        } else if (action === "down") {
            list.move(1);
        } else if (action === "accept") {
            if (list.currentIndex >= 0 && list.currentIndex < list.entries.length)
                list._commitAccept(list.entries[list.currentIndex].id);
        } else if (action === "cancel") {
            list.closeRequested();
        }
    }

    function _commitAccept(id: string): void {
        list._pendingId = id;
        list._activatePulse++;
        acceptCommit.arm();
    }

    DeferredAction {
        id: acceptCommit
        onDeferred: {
            const id = list._pendingId;
            list._pendingId = "";
            list.accepted(id);
        }
    }

    FontMetrics {
        id: panelWidthLabelMetrics
        font.family: Theme.fontUi
        font.pixelSize: Sizing.fontBody
        font.weight: Font.Medium
    }

    Flickable {
        id: viewport

        anchors.fill: parent
        anchors.topMargin: list._scrollIndicatorBand
        anchors.bottomMargin: list._scrollIndicatorBand
        contentWidth: width
        contentHeight: list._contentHeight
        clip: true
        // Key navigation drives contentY; we don't want kinetic
        // dragging fighting with the focus tracker.
        interactive: false
        boundsBehavior: Flickable.StopAtBounds

        Column {
            id: rowColumn

            width: viewport.width
            spacing: list._rowSpacing

            Repeater {
                model: list.entries

                Item {
                    id: row

                    required property int index
                    required property var modelData

                    readonly property bool focused: row.index === list.currentIndex

                    objectName: "listPickerRow-" + row.index
                    width: rowColumn.width
                    height: list._rowHeight

                    // Inverse-video row -- see SelectionBar.qml and
                    // docs/style.md -> "Two registers". Picking a
                    // value from this list is the same interaction
                    // as a SettingsField row; both read the same way
                    // now instead of flipping registers mid-task.
                    SelectionBar {
                        id: bar
                        objectName: "listPickerSelectionBar"
                        anchors.fill: parent
                        active: row.focused
                        activatePulse: list._activatePulse
                        radius: Sizing.radiusSm
                    }

                    // Tracks this row's own live weight (Normal at
                    // rest, Medium selected -- bar.contentWeight) so
                    // both label variants' own centering/left-align
                    // box below always matches their actual
                    // rendered glyph width -- see
                    // `panelWidthLabelMetrics` above for why panel
                    // sizing measures at a separate, fixed weight
                    // instead.
                    //
                    // `TextMetrics` with its own `text:` binding,
                    // not a `FontMetrics` fed through
                    // `_measureLabelWidth`'s `metrics.advanceWidth(label)`
                    // method-call form (round 9's ContextMenu.qml
                    // fix uses the identical construction, for the
                    // identical reason). `advanceWidth`/
                    // `boundingRect` are Q_INVOKABLE methods on
                    // `FontMetrics`; a caller passing `fontWeight`
                    // as an explicit-but-otherwise-unused parameter
                    // to make its own binding "depend on" the
                    // weight does not reliably work under the AOT
                    // QML compiler here -- an argument that is
                    // never read inside the callee's body is a
                    // strong candidate for the compiler to treat as
                    // dead and drop the dependency it would
                    // otherwise establish, so a row's `_textWidth`
                    // stayed pinned to its stale weight's
                    // measurement across a selection change. On
                    // `TextMetrics`, `advanceWidth`/`boundingRect`
                    // are genuine properties that recompute
                    // whenever `text`/`font.*` change, so reading
                    // them from `_textWidth` below is an ordinary,
                    // reliable property dependency.
                    TextMetrics {
                        id: rowLabelMetrics
                        objectName: "listPickerRowLabelMetrics"
                        text: row.modelData.label
                        font.family: Theme.fontUi
                        font.pixelSize: Sizing.fontBody
                        font.weight: bar.contentWeight
                    }

                    MouseArea {
                        objectName: "listPickerRowMouseArea"
                        anchors.fill: parent
                        enabled: !list.locked
                        hoverEnabled: true
                        acceptedButtons: Qt.LeftButton
                        cursorShape: Qt.PointingHandCursor
                        onEntered: list.currentIndex = row.index
                        onClicked: {
                            list.currentIndex = row.index;
                            list._commitAccept(row.modelData.id);
                        }
                    }

                    // Plain centered label -- every picker except the
                    // color-scheme one (below). Untouched by the
                    // swatch-preview addition.
                    Text {
                        // Centered as a box sized to this row's own
                        // measured text, not full row width +
                        // AlignHCenter -- a glyph run that straddles a
                        // half-pixel softens under the software
                        // renderer. See "Integer-pixel rules" in
                        // docs/qml-gotchas.md, and ContextMenu.qml's
                        // identical row-label construction.
                        readonly property int _availableWidth: Math.max(0, parent.width - 2 * list._rowHorizontalPadding)
                        readonly property int _textWidth: Math.min(Math.ceil(Math.max(rowLabelMetrics.advanceWidth, rowLabelMetrics.boundingRect.width)) + Sizing.stroke(2), _availableWidth)

                        objectName: "listPickerRowLabelCentered"
                        visible: !list._hasSwatchPreview
                        anchors.verticalCenter: parent.verticalCenter
                        x: Sizing.center(parent.width, _textWidth)
                        width: _textWidth
                        text: row.modelData.label
                        // Inverse video on selection, matching
                        // SettingsField/BrowseList -- see
                        // SelectionBar.qml.
                        color: bar.active ? bar.contentColor : Theme.textPrimary
                        font.family: Theme.fontUi
                        font.pixelSize: Sizing.fontBody
                        font.weight: bar.contentWeight
                        elide: Text.ElideRight
                        renderType: Text.NativeRendering
                    }

                    // Label-left, swatch-right -- the color-scheme
                    // picker. Label is left-aligned rather than
                    // centered so it reads as a row leading into its
                    // own preview, not two independently-centered
                    // elements. See docs/style.md -> "Picker swatch
                    // preview".
                    Text {
                        readonly property int _availableWidth: Math.max(0, parent.width - 2 * list._rowHorizontalPadding - list._swatchBandWidth - list._swatchLabelGap)
                        readonly property int _textWidth: Math.min(Math.ceil(Math.max(rowLabelMetrics.advanceWidth, rowLabelMetrics.boundingRect.width)) + Sizing.stroke(2), _availableWidth)

                        objectName: "listPickerRowLabelSwatch"
                        visible: list._hasSwatchPreview
                        anchors.verticalCenter: parent.verticalCenter
                        x: list._rowHorizontalPadding
                        width: _textWidth
                        text: row.modelData.label
                        color: bar.active ? bar.contentColor : Theme.textPrimary
                        font.family: Theme.fontUi
                        font.pixelSize: Sizing.fontBody
                        font.weight: bar.contentWeight
                        elide: Text.ElideRight
                        renderType: Text.NativeRendering
                    }

                    Row {
                        objectName: "listPickerRowSwatches"
                        visible: list._hasSwatchPreview
                        anchors.verticalCenter: parent.verticalCenter
                        x: parent.width - list._rowHorizontalPadding - list._swatchBandWidth
                        spacing: list._swatchGap

                        Repeater {
                            model: list._hasSwatchPreview ? row.modelData.swatch : 0

                            Rectangle {
                                required property color modelData
                                required property int index

                                objectName: "listPickerRowSwatch-" + index
                                width: list._swatchBoxSize
                                height: list._swatchBoxSize
                                radius: Sizing.half(Sizing.radiusSm)
                                color: modelData
                                // A near-black or near-white swatch can
                                // sit at the same contrast as the row's
                                // resting background and disappear into
                                // it (item 1, round 6). `textLabel` is a
                                // mid neutral the semantic-tier
                                // guardrails already hold >=3:1 against
                                // bgDeep on every preset, so it separates
                                // either extreme from the row at rest.
                                // On a selected row the swatch sits on a
                                // solid `accent` fill instead, where
                                // `textLabel` isn't guaranteed to
                                // separate -- flip to `onAccent`
                                // (`bar.contentColor`) there, the same
                                // fix the favorite heart uses against
                                // SelectionBar (see docs/style.md ->
                                // "Inverse-video rows").
                                border.width: Sizing.cardBorderWidth
                                border.color: bar.active ? bar.contentColor : Theme.textLabel
                            }
                        }
                    }
                }
            }
        }
    }

    Image {
        source: Resources.iconUrl("ScrollUp", Theme.textPrimary)
        width: list._scrollArrowSize
        height: width
        sourceSize.width: Sizing.px(width)
        sourceSize.height: Sizing.px(height)
        anchors.bottom: viewport.top
        anchors.bottomMargin: list._scrollArrowGap
        anchors.horizontalCenter: viewport.horizontalCenter
        fillMode: Image.PreserveAspectFit
        smooth: true
        visible: list._hasContentAbove
    }

    Image {
        source: Resources.iconUrl("ScrollDown", Theme.textPrimary)
        width: list._scrollArrowSize
        height: width
        sourceSize.width: Sizing.px(width)
        sourceSize.height: Sizing.px(height)
        anchors.top: viewport.bottom
        anchors.topMargin: list._scrollArrowGap
        anchors.horizontalCenter: viewport.horizontalCenter
        fillMode: Image.PreserveAspectFit
        smooth: true
        visible: list._hasContentBelow
    }
}
