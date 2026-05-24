// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
// Three-slot top strip shared by the Systems and Games screens. Owns
// layout only — callers compute and pass `title`, `currentPage`,
// `totalPages`, and `totalText` from their own model. Each slot is
// capped at one third of the parent width with `elide: ElideRight` so
// long strings (3-digit page counts, 5-digit file totals, multi-word
// titles) can't collide on a 240p MiSTer screen.
// Slots:
//   left   — total-count badge (visible when `totalText !== ""`)
//   center — screen title (category / system name)
//   right  — "Page N / M" counter (visible when `totalPages > 1`)

import QtQuick
import Zaparoo.Theme

// Software-rendering safe: only Item + Text, no transforms, no shaders.
Item {
    id: status

    property string title: ""
    property int currentPage: 0 // 0-indexed; displayed as N+1
    property int totalPages: 1
    property string totalText: "" // formatted; empty hides the slot
    property string rightTextOverride: "" // formatted; non-empty replaces Page N / M
    property int slotMargin: Sizing.pctW(5)
    readonly property int _slotWidth: Sizing.px(status.width / 3)
    readonly property int _textMeasureSlack: Theme.crtNativePath ? 0 : 2
    readonly property int _titleMeasuredWidth: Math.ceil(Math.max(titleMetrics.advanceWidth, titleMetrics.boundingRect.width) + status._textMeasureSlack)
    readonly property int _titleTextWidth: Math.min(status._slotWidth, status._titleMeasuredWidth)

    // Page counter and total badge sit on the same baseline as the
    // title's lower edge — bottom-aligned to the strip — so the trio
    // reads as a single line of header text rather than three loose
    // chips. Counter/total drop one step in font size so the title
    // stays the visual anchor.
    Text {
        id: totalBadge

        visible: status.totalText !== ""
        anchors.left: parent.left
        anchors.leftMargin: status.slotMargin
        anchors.bottom: titleText.bottom
        width: status._slotWidth - status.slotMargin
        elide: Text.ElideRight
        horizontalAlignment: Text.AlignLeft
        text: status.totalText
        font.family: Theme.fontUi
        font.pixelSize: Sizing.fontSize(2.9)
        color: Theme.textPrimary
        renderType: Text.NativeRendering
    }

    TextMetrics {
        id: titleMetrics

        text: status.title
        font.family: Theme.fontUi
        font.pixelSize: Sizing.fontSize(4)
        font.weight: Font.Medium
    }

    Text {
        id: titleText

        x: Sizing.center(parent.width, width)
        y: Sizing.center(parent.height, height)
        width: status._titleTextWidth
        height: Sizing.fontSize(4)
        elide: Text.ElideRight
        horizontalAlignment: Text.AlignLeft
        text: status.title
        font.family: Theme.fontUi
        font.pixelSize: Sizing.fontSize(4)
        font.weight: Font.Medium
        color: Theme.textPrimary
        renderType: Text.NativeRendering
    }

    Text {
        id: pageCounter

        visible: status.rightTextOverride !== "" || status.totalPages > 1
        anchors.right: parent.right
        anchors.rightMargin: status.slotMargin
        anchors.bottom: titleText.bottom
        width: status._slotWidth - status.slotMargin
        elide: Text.ElideRight
        horizontalAlignment: Text.AlignRight
        text: status.rightTextOverride !== "" ? status.rightTextOverride : qsTr("Page %1 / %2").arg(status.currentPage + 1).arg(status.totalPages)
        font.family: Theme.fontUi
        font.pixelSize: Sizing.fontSize(2.9)
        color: Theme.textPrimary
        renderType: Text.NativeRendering
    }
}
