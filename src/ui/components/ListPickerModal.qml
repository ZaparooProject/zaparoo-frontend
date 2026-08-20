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

// Software-rendering safe centered list-picker modal. Wraps the shared
// `Modal` shell in `kind: "shell"` so it inherits the standard chrome
// (scrim, panel fill, corner radius, title) used by every other modal.
//
// Use this for "pick one of these" prompts that are not anchored to a
// tile. Anchored selectors should use `ContextMenu.qml` instead.
//
// Pure presentation. Routing - mounting and dispatching `handleAction` -
// belongs to whichever consumer plumbs the modal into `Main.qml`'s modal
// stack. The component renders, navigates `currentIndex` on up/down,
// emits `accepted(id)` on accept, and `closeRequested()` on cancel.
Item {
    id: modal

    property bool open: false
    property string title: ""
    // Each entry is `{ id: string, label: string }`. `id` is the dispatch
    // key emitted by `accepted`; `label` is the localized display text.
    // Position-keyed dispatch is a footgun - dynamic entry sets silently
    // re-shuffle the index/action map.
    property var entries: []
    // Optional. When `open` flips true, sets `currentIndex` to the entry
    // whose id matches. Empty string or no match falls back to 0.
    property string initialId: ""
    property int currentIndex: 0

    property bool _pressed: false
    property string _pendingId: ""

    signal accepted(string id)
    signal closeRequested

    readonly property int _rowHeight: Sizing.pctH(7)
    readonly property int _rowSpacing: Sizing.pctH(1)
    // Cap the picker viewport at a portion of the screen height so it
    // never grows past what the modal shell can reasonably contain.
    // Visible row count falls out of this - `floor((max + spacing) /
    // (rowHeight + spacing))` gives the row count whose viewport fits
    // inside `_maxViewportHeight`, with at least 1 row.
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

    // Content-driven panel width, mirroring ContextMenu.qml's
    // `_widestEntryLabelWidth` pattern (Modal.qml can't measure this itself —
    // shell content is an opaque caller-supplied Item to it) — see
    // docs/style.md -> "Content-driven modal width".
    readonly property int _rowHorizontalPadding: Sizing.pctW(2)
    readonly property int _contentHorizontalMargin: Sizing.pctW(4)
    readonly property int _widestEntryLabelWidth: {
        let widest = 0;
        for (let i = 0; i < modal.entries.length; ++i) {
            const label = modal.entries[i] && modal.entries[i].label !== undefined ? String(modal.entries[i].label) : "";
            widest = Math.max(widest, Math.ceil(_rowLabelMetrics.advanceWidth(label)));
        }
        return widest;
    }
    readonly property int _titleWidth: modal.title !== "" ? Math.ceil(_titleLabelMetrics.advanceWidth(modal.title)) : 0
    // Optional per-entry color-swatch preview (the color-scheme picker).
    // `entries[i].swatch` is a 3-color array or undefined; the picker is
    // homogeneous (either every entry carries one or none do), so checking
    // the first entry is enough to flag the whole modal into swatch layout.
    // See docs/style.md -> "Picker swatch preview".
    readonly property bool _hasSwatchPreview: modal.entries.length > 0 && modal.entries[0].swatch !== undefined
    readonly property int _swatchBoxSize: Sizing.pctH(2.4)
    readonly property int _swatchGap: Sizing.pctW(0.6)
    readonly property int _swatchLabelGap: Sizing.pctW(2)
    readonly property int _swatchBandWidth: modal._hasSwatchPreview ? 3 * modal._swatchBoxSize + 2 * modal._swatchGap : 0
    readonly property int _desiredPanelWidth: Math.max(modal._widestEntryLabelWidth + (modal._hasSwatchPreview ? modal._swatchBandWidth + modal._swatchLabelGap : 0) + 2 * modal._rowHorizontalPadding, modal._titleWidth) + 2 * modal._contentHorizontalMargin
    // Degenerate-case floor only, matching Modal.qml's own floor.
    readonly property int _minPanelWidth: Sizing.pctW(30)

    visible: modal.open
    anchors.fill: parent
    z: 300

    onOpenChanged: {
        if (!modal.open) {
            // Disarm a pending accept so a press-then-close inside the deferred
            // window cannot apply a selection after the modal is dismissed.
            acceptCommit.stop();
            return;
        }
        modal._applyInitialIndex();
        modal._pressed = false;
        modal._pendingId = "";
    }

    // Callers swap `entries` in place on an already-open modal (the launcher
    // flow replaces a one-row "saving" list with a three-row error list).
    // Without this the stale currentIndex can sit past the new list's end, so
    // no row renders focused and Accept is a dead key until the user moves.
    onEntriesChanged: {
        if (modal.open)
            modal._applyInitialIndex();
    }

    function _applyInitialIndex(): void {
        let next = 0;
        if (modal.initialId !== "") {
            for (let i = 0; i < modal.entries.length; ++i) {
                if (modal.entries[i].id === modal.initialId) {
                    next = i;
                    break;
                }
            }
        }
        viewport.contentY = 0;
        modal.currentIndex = next;
        modal._scrollCurrentIntoView();
    }

    function _scrollCurrentIntoView(): void {
        const stride = modal._rowHeight + modal._rowSpacing;
        const top = modal.currentIndex * stride;
        const bottom = top + modal._rowHeight;
        if (top < viewport.contentY) {
            viewport.contentY = top;
        } else if (bottom > viewport.contentY + viewport.height) {
            viewport.contentY = bottom - viewport.height;
        }
    }

    function move(delta: int): void {
        if (modal.entries.length <= 0)
            return;
        const len = modal.entries.length;
        modal.currentIndex = ((modal.currentIndex + delta) % len + len) % len;
    }

    function handleAction(action: string): void {
        if (action === "up") {
            modal.move(-1);
        } else if (action === "down") {
            modal.move(1);
        } else if (action === "accept") {
            if (modal.currentIndex >= 0 && modal.currentIndex < modal.entries.length)
                modal._commitAccept(modal.entries[modal.currentIndex].id);
        } else if (action === "cancel") {
            modal.closeRequested();
        }
    }

    function _commitAccept(id: string): void {
        modal._pendingId = id;
        modal._pressed = true;
        acceptCommit.arm();
    }

    DeferredAction {
        id: acceptCommit
        onDeferred: {
            const id = modal._pendingId;
            modal._pendingId = "";
            modal._pressed = false;
            modal.accepted(id);
        }
    }

    FontMetrics {
        id: _rowLabelMetrics
        font.family: Theme.fontUi
        font.pixelSize: Sizing.fontBody
    }

    FontMetrics {
        id: _titleLabelMetrics
        font.family: Theme.fontUi
        font.pixelSize: Sizing.fontTitle
    }

    Modal {
        id: shell

        open: modal.open
        kind: "shell"
        title: modal.title
        panelMaxWidth: Math.max(modal._minPanelWidth, modal._desiredPanelWidth)

        Item {
            id: viewportSlot

            width: parent.width
            height: modal._viewportSlotHeight

            Flickable {
                id: viewport

                anchors.fill: parent
                anchors.topMargin: modal._scrollIndicatorBand
                anchors.bottomMargin: modal._scrollIndicatorBand
                contentWidth: width
                contentHeight: modal._contentHeight
                clip: true
                // Key navigation drives contentY; we don't want kinetic
                // dragging fighting with the focus tracker.
                interactive: false
                boundsBehavior: Flickable.StopAtBounds

                Column {
                    id: rowColumn

                    width: viewport.width
                    spacing: modal._rowSpacing

                    Repeater {
                        model: modal.entries

                        PressableSurface {
                            id: row

                            required property int index
                            required property var modelData

                            objectName: "listPickerRow-" + row.index
                            width: rowColumn.width
                            height: modal._rowHeight
                            focused: row.index === modal.currentIndex
                            pressed: modal._pressed && row.modelData.id === modal._pendingId
                            pointerAcceptedButtons: Qt.LeftButton
                            pointerHoverEnabled: true
                            onPointerEntered: modal.currentIndex = row.index
                            onPointerClicked: modal._commitAccept(row.modelData.id)

                            // Plain centered label — every picker except the
                            // color-scheme one (below). Untouched by the
                            // swatch-preview addition.
                            Text {
                                // Centered as a box sized to this row's own
                                // measured text, not full row width +
                                // AlignHCenter — a glyph run that straddles a
                                // half-pixel softens under the software
                                // renderer. See "Integer-pixel rules" in
                                // docs/qml-gotchas.md, and ContextMenu.qml's
                                // identical row-label construction.
                                readonly property int _availableWidth: Math.max(0, parent.width - 2 * modal._rowHorizontalPadding)
                                readonly property int _textWidth: Math.min(Math.ceil(_rowLabelMetrics.advanceWidth(row.modelData.label)), _availableWidth)

                                objectName: "listPickerRowLabelCentered"
                                visible: !modal._hasSwatchPreview
                                anchors.verticalCenter: parent.verticalCenter
                                x: Sizing.center(parent.width, _textWidth)
                                width: _textWidth
                                text: row.modelData.label
                                color: Theme.textPrimary
                                font.family: Theme.fontUi
                                font.pixelSize: Sizing.fontBody
                                elide: Text.ElideRight
                                renderType: Text.NativeRendering
                            }

                            // Label-left, swatch-right — the color-scheme
                            // picker. Label is left-aligned rather than
                            // centered so it reads as a row leading into its
                            // own preview, not two independently-centered
                            // elements. See docs/style.md -> "Picker swatch
                            // preview".
                            Text {
                                readonly property int _availableWidth: Math.max(0, parent.width - 2 * modal._rowHorizontalPadding - modal._swatchBandWidth - modal._swatchLabelGap)
                                readonly property int _textWidth: Math.min(Math.ceil(_rowLabelMetrics.advanceWidth(row.modelData.label)), _availableWidth)

                                objectName: "listPickerRowLabelSwatch"
                                visible: modal._hasSwatchPreview
                                anchors.verticalCenter: parent.verticalCenter
                                x: modal._rowHorizontalPadding
                                width: _textWidth
                                text: row.modelData.label
                                color: Theme.textPrimary
                                font.family: Theme.fontUi
                                font.pixelSize: Sizing.fontBody
                                elide: Text.ElideRight
                                renderType: Text.NativeRendering
                            }

                            Row {
                                objectName: "listPickerRowSwatches"
                                visible: modal._hasSwatchPreview
                                anchors.verticalCenter: parent.verticalCenter
                                x: parent.width - modal._rowHorizontalPadding - modal._swatchBandWidth
                                spacing: modal._swatchGap

                                Repeater {
                                    model: modal._hasSwatchPreview ? row.modelData.swatch : 0

                                    Rectangle {
                                        required property color modelData
                                        required property int index

                                        objectName: "listPickerRowSwatch-" + index
                                        width: modal._swatchBoxSize
                                        height: modal._swatchBoxSize
                                        radius: Sizing.half(Sizing.radiusSm)
                                        color: modelData
                                    }
                                }
                            }
                        }
                    }
                }
            }

            Image {
                source: Resources.iconUrl("ScrollUp", Theme.textPrimary)
                width: modal._scrollArrowSize
                height: width
                sourceSize.width: Sizing.px(width)
                sourceSize.height: Sizing.px(height)
                anchors.bottom: viewport.top
                anchors.bottomMargin: modal._scrollArrowGap
                anchors.horizontalCenter: viewport.horizontalCenter
                fillMode: Image.PreserveAspectFit
                smooth: true
                visible: modal._hasContentAbove
            }

            Image {
                source: Resources.iconUrl("ScrollDown", Theme.textPrimary)
                width: modal._scrollArrowSize
                height: width
                sourceSize.width: Sizing.px(width)
                sourceSize.height: Sizing.px(height)
                anchors.top: viewport.bottom
                anchors.topMargin: modal._scrollArrowGap
                anchors.horizontalCenter: viewport.horizontalCenter
                fillMode: Image.PreserveAspectFit
                smooth: true
                visible: modal._hasContentBelow
            }
        }
    }

    // Keep the focused row in view. When the current index moves above
    // or below the visible band we slide contentY just enough to bring
    // it back into view, no animation - software renderer pays per-frame
    // for any motion under translucent content.
    Connections {
        target: modal
        function onCurrentIndexChanged(): void {
            modal._scrollCurrentIntoView();
        }
    }
}
