// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
// Big single-line caption for the focused-tile name. Mounted under the
// grid on Systems and Games, and directly under the categories row on
// the Hub. Same typography as the screen-title slot in TopStatusStrip
// so the two big captions read as a matched pair.
// Single line with `elide: ElideRight` — long names are cut, never
// wrapped. Two-line wrap would shift the help-bar baseline by a row
// every time the focus crossed between a short and long entry, which
// reads as visible chop on a busy directional-input session.

import QtQuick
import Zaparoo.Theme

// Software-rendering safe: only Item + Text, no transforms, no
// shaders, no opacity tweens.
Item {
    id: root

    property string text: ""
    readonly property int _textMeasureSlack: Theme.crtNativePath ? 0 : 2
    readonly property int _measuredTextWidth: Math.ceil(Math.max(labelMetrics.advanceWidth, labelMetrics.boundingRect.width) + root._textMeasureSlack)
    readonly property int _textWidth: Math.min(root.width - 2 * Sizing.pctW(5), root._measuredTextWidth)

    TextMetrics {
        id: labelMetrics

        text: root.text
        font.family: Theme.fontUi
        font.pixelSize: Sizing.fontSize(4)
        font.weight: Font.Medium
    }

    Text {
        x: Sizing.center(parent.width, width)
        y: Sizing.center(parent.height, height)
        width: root._textWidth
        height: Sizing.fontSize(4)
        text: root.text
        font.family: Theme.fontUi
        font.pixelSize: Sizing.fontSize(4)
        font.weight: Font.Medium
        color: Theme.textPrimary
        horizontalAlignment: Text.AlignLeft
        verticalAlignment: Text.AlignVCenter
        elide: Text.ElideRight
        renderType: Text.NativeRendering
    }
}
