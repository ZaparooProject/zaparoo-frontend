// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import Zaparoo.Theme
import Zaparoo.Ui

Item {
    id: root

    property bool open: false

    visible: root.open
    z: 300

    Modal {
        id: shell

        open: root.open
        kind: "shell"
        title: qsTr("Write with App")
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
                text: qsTr("Scan this code with the Zaparoo App to write this game to a Zaparoo token.")
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
        }
    }
}
