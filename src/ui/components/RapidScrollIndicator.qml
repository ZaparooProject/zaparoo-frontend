// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

import QtQuick
import Zaparoo.Theme

Item {
    id: root

    property string title: ""

    readonly property string _trimmedTitle: title.trim()
    readonly property string _letter: _trimmedTitle === "" ? "#" : _trimmedTitle.charAt(0).toUpperCase()

    width: Sizing.pctH(18)
    height: width

    Rectangle {
        anchors.fill: parent
        color: Theme.surfaceCard
        border.color: Theme.borderMid
        border.width: Sizing.stroke(1)
        radius: Sizing.cornerRadius
    }

    Text {
        id: letterText

        x: Sizing.center(parent.width, width)
        y: Sizing.center(parent.height, height)
        width: Math.min(parent.width, Math.ceil(letterMetrics.advanceWidth) + Sizing.px(2))
        height: Sizing.fontSize(10)
        text: root._letter
        color: Theme.textPrimary
        font.family: Theme.fontUi
        font.pixelSize: Sizing.fontSize(10)
        horizontalAlignment: Text.AlignLeft
        verticalAlignment: Text.AlignVCenter
        renderType: Text.NativeRendering
    }

    TextMetrics {
        id: letterMetrics
        text: letterText.text
        font.family: letterText.font.family
        font.pixelSize: letterText.font.pixelSize
    }
}
