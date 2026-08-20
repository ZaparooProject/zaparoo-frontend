// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

import QtQuick
import Zaparoo.Theme

// Compact pill badge for tile state overlays. Filled accent surface with a
// dark label and border that stay readable over dimmed art and system icons.
// A preset authors two colors, so state markers share the accent rather than
// introducing a third; the pill shape and dark label keep it distinct from the
// focus ring.
Rectangle {
    id: root

    property string label: ""

    readonly property int _horizontalPadding: Sizing.pctH(1.3)
    readonly property int _verticalPadding: Sizing.pctH(0.6)

    width: labelText.implicitWidth + 2 * root._horizontalPadding
    height: labelText.implicitHeight + 2 * root._verticalPadding
    radius: Sizing.half(height)
    color: Theme.accent
    border.width: Sizing.stroke(1)
    border.color: Theme.bgBar
    antialiasing: true

    Text {
        id: labelText

        anchors.centerIn: parent
        text: root.label
        color: Theme.bgBar
        font.family: Theme.fontUi
        font.pixelSize: Sizing.fontSize(1.8)
        font.bold: true
        renderType: Text.NativeRendering
    }
}
