// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import Zaparoo.Theme
import Zaparoo.Browse as Browse

// cxx-qt 0.8 patches `isFinal: true` on singleton properties but the
// qmltypes schema has no `isFinal` slot, so every read trips qmllint's
// "Member can be shadowed" check. Until the schema grows the slot,
// suppress the compiler category file-wide.
// qmllint disable compiler

Item {
    id: root

    property bool open: false
    property int quietZone: 4

    readonly property int matrixSize: Browse.QrCode.size
    readonly property int maxQrPixels: Math.min(Sizing.pctW(42), Sizing.pctH(68))
    readonly property int moduleSize: matrixSize > 0 ? Math.max(1, Math.floor(maxQrPixels / (matrixSize + quietZone * 2))) : 1
    readonly property int qrPixels: moduleSize * (matrixSize + quietZone * 2)

    visible: root.open
    z: 300

    // Full-screen scrim. Joins QrCodeModal to the modal family for now
    // (full panel chrome — title, padding, close affordance — is a
    // future round). The MouseArea below eats clicks/hover so the
    // dimmed screens beneath don't track focus under the modal.
    Rectangle {
        anchors.fill: parent
        color: Theme.scrim

        MouseArea {
            anchors.fill: parent
            hoverEnabled: true
            acceptedButtons: Qt.AllButtons
        }
    }

    Rectangle {
        x: Sizing.center(parent.width, width)
        y: Sizing.center(parent.height, height)
        width: root.qrPixels
        height: root.qrPixels
        color: "white"
        border.width: Sizing.stroke(root.moduleSize * 0.18)
        border.color: Theme.borderSubtle

        Item {
            id: matrix

            x: Sizing.center(parent.width, width)
            y: Sizing.center(parent.height, height)
            width: root.moduleSize * root.matrixSize
            height: root.moduleSize * root.matrixSize
            visible: root.matrixSize > 0

            Repeater {
                model: root.matrixSize

                delegate: Item {
                    id: rowDelegate

                    required property int index

                    readonly property int row: index
                    readonly property string bits: Browse.QrCode.row_at(row)

                    x: 0
                    y: row * root.moduleSize
                    width: matrix.width
                    height: root.moduleSize

                    Repeater {
                        model: root.matrixSize

                        delegate: Rectangle {
                            required property int index

                            x: index * root.moduleSize
                            y: 0
                            width: root.moduleSize
                            height: root.moduleSize
                            color: "black"
                            visible: rowDelegate.bits.charAt(index) === "1"
                        }
                    }
                }
            }
        }
    }
}
