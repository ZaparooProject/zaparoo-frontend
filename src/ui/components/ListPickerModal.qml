// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import Zaparoo.Theme

// `entries` is a `var` array of plain JS objects (`{ id, label }`); see
// PickerList.qml for why the compiler category is suppressed file-wide.
// qmllint disable compiler

// Software-rendering safe centered list-picker modal. Wraps the shared
// `Modal` shell in `kind: "shell"` so it inherits the standard chrome
// (scrim, panel fill, corner radius, title) used by every other modal, and
// hosts a `PickerList` for the rows themselves.
//
// Use this for "pick one of these" prompts opened from a screen and not
// anchored to a tile. Anchored selectors should use `ContextMenu.qml`; a
// modal that needs a choice on one of its own rows hosts `PickerList` on a
// page of its own panel instead of opening this on top of itself (see
// docs/style.md -> "Modal depth").
//
// Pure presentation. Routing - mounting and dispatching `handleAction` -
// belongs to whichever consumer plumbs the modal into `Main.qml`'s modal
// stack. The component renders, navigates `currentIndex` on up/down,
// emits `accepted(id)` on accept, and `closeRequested()` on cancel.
Item {
    id: modal

    property bool open: false
    property string title: ""
    property alias entries: list.entries
    property alias initialId: list.initialId
    property alias currentIndex: list.currentIndex
    property alias locked: list.locked

    signal accepted(string id)
    signal closeRequested

    // List geometry, forwarded so consumers (and the sizing tests) keep
    // reading it off the modal.
    readonly property int _rowHeight: list._rowHeight
    readonly property int _rowSpacing: list._rowSpacing
    readonly property int _maxViewportHeight: list._maxViewportHeight
    readonly property int _visibleRows: list._visibleRows
    readonly property int _viewportHeight: list._viewportHeight
    readonly property int _contentHeight: list._contentHeight
    readonly property int _rowHorizontalPadding: list._rowHorizontalPadding
    readonly property int _widestEntryLabelWidth: list._widestEntryLabelWidth
    readonly property bool _hasSwatchPreview: list._hasSwatchPreview
    readonly property int _swatchBandWidth: list._swatchBandWidth

    // Content-driven panel width: the list's own measured content, the
    // title, the shell's horizontal margins, and breathing room -- see
    // docs/style.md -> "Content-driven modal width".
    readonly property int _contentHorizontalMargin: Sizing.pctW(4)
    readonly property int _titleWidth: modal.title !== "" ? modal._measureLabelWidth(_titleLabelMetrics, modal.title, Sizing.fontTitle, Theme.fontUi, Font.Normal) : 0
    // Breathing room between the widest row's text box and the width the row
    // actually has, deliberately generous rather than a pixel-exact fit.
    //
    // `_widestEntryLabelWidth` and the row's own `_textWidth` apply the
    // *same* `Sizing.stroke(2)` hinting allowance, so they cancel: without a
    // term here the widest entry is handed exactly its measured width and not
    // one pixel more, and `Text` elides it wherever the hinted integer glyph
    // advances paint wider than `advanceWidth()`'s fractional, unhinted total.
    // Chasing that difference exactly is a losing game -- it varies by font
    // build, weight synthesis and hinting, so a panel tuned until one label
    // fits just truncates the next label someone adds. An em and a half of
    // slack scales with the text being measured (the error scales with glyph
    // size, not with the screen) and costs a slightly wider panel, which is
    // the cheap side of this trade: a picker is a list of names, and a name
    // that reads in full matters more than a snug panel.
    readonly property int _labelClearance: Sizing.px(Sizing.fontBody * 1.5)
    readonly property int _desiredPanelWidth: Math.max(list.desiredContentWidth, modal._titleWidth) + 2 * modal._contentHorizontalMargin + modal._labelClearance
    // Degenerate-case floor only, matching Modal.qml's own floor.
    readonly property int _minPanelWidth: Sizing.pctW(30)

    visible: modal.open
    anchors.fill: parent
    z: 300

    function _measureLabelWidth(metrics: FontMetrics, label: string, fontSize: int, fontFamily: string, fontWeight: int): int {
        return list._measureLabelWidth(metrics, label, fontSize, fontFamily, fontWeight);
    }

    function move(delta: int): void {
        list.move(delta);
    }

    function handleAction(action: string): void {
        list.handleAction(action);
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
        contentSized: true

        PickerList {
            id: list

            width: parent.width
            active: modal.open
            onAccepted: id => modal.accepted(id)
            onCloseRequested: modal.closeRequested()
        }
    }
}
