// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import Zaparoo.Theme
import Zaparoo.Ui

// Generic "scan this code" surface. The caller owns the payload (it must
// call `Browse.QrCode.generate()` and check `size > 0` before opening,
// since that singleton is a single shared slot) and supplies the wording,
// so the same chrome serves both writing a token and pointing at the
// documentation. Defaults reproduce the original token-write copy.
Item {
    id: root

    property bool open: false
    property string title: qsTr("Write with App")
    property string instructionText: qsTr("Scan this code with the Zaparoo App to write this game to a Zaparoo token.")
    // Optional plain-text URL under the matrix. A QR is unscannable at
    // 240p over composite, so anywhere the destination is a web page the
    // readable URL is the real affordance and the code is the shortcut.
    property string urlText: ""

    visible: root.open
    z: 300

    Modal {
        id: shell

        open: root.open
        kind: "shell"
        title: root.title
        panelMaxWidth: Sizing.pctH(105)

        Column {
            width: parent.width
            spacing: Sizing.pctH(2)

            TextMetrics {
                id: instructionsMetrics

                font.family: Theme.fontUi
                font.pixelSize: Sizing.fontCaption
                text: instructions.text
            }

            Text {
                id: instructions

                x: Sizing.center(parent.width, width)
                width: Math.min(parent.width, Sizing.px(instructionsMetrics.advanceWidth))
                text: root.instructionText
                font.family: Theme.fontUi
                font.pixelSize: Sizing.fontCaption
                color: Theme.textPrimary
                horizontalAlignment: Text.AlignLeft
                wrapMode: Text.WordWrap
                renderType: Text.NativeRendering
            }

            QrMatrix {
                anchors.horizontalCenter: parent.horizontalCenter
                maxQrPixels: Math.min(Sizing.pctW(36), Sizing.pctH(48))
            }

            Text {
                width: parent.width
                visible: root.urlText !== ""
                text: root.urlText
                font.family: Theme.fontUi
                font.pixelSize: Sizing.fontSmall
                color: Theme.textLabel
                horizontalAlignment: Text.AlignHCenter
                wrapMode: Text.WrapAnywhere
                renderType: Text.NativeRendering
            }
        }
    }
}
