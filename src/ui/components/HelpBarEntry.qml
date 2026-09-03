// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import Zaparoo.Theme

// Atomic help-bar group. Wrapping happens between these groups, never between
// a controller glyph and the label describing it.
Item {
    id: root

    property int entryIndex: -1
    required property var entry
    readonly property var buttonList: root.entry.buttons !== undefined ? root.entry.buttons : (root.entry.button !== undefined ? [root.entry.button] : [])
    readonly property int _iconSize: Sizing.pctH(4)

    objectName: "helpBarEntry-" + root.entryIndex
    implicitWidth: entryRow.implicitWidth
    // Bitmap font line boxes exceed their visible 8px glyphs. Flow must use
    // painted icon/text height or two compact rows consume four rows' space.
    implicitHeight: Math.max(root._iconSize, Sizing.fontBody)
    height: implicitHeight

    Row {
        id: entryRow

        anchors.centerIn: parent
        spacing: Sizing.pctW(0.6)

        Repeater {
            model: root.buttonList

            delegate: Image {
                required property string modelData

                anchors.verticalCenter: entryRow.verticalCenter
                height: root._iconSize
                width: height
                fillMode: Image.PreserveAspectFit
                sourceSize.height: Sizing.px(height)
                sourceSize.width: Sizing.px(width)
                source: Resources.iconUrl(modelData, Theme.textPrimary)
                smooth: true
            }
        }

        Text {
            anchors.verticalCenter: entryRow.verticalCenter
            height: root._iconSize
            verticalAlignment: Text.AlignVCenter
            text: root.entry.label
            font.family: Theme.fontUi
            font.pixelSize: Sizing.fontBody
            color: Theme.textPrimary
            renderType: Text.NativeRendering
        }
    }
}
