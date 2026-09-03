// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import Zaparoo.Theme

Item {
    id: root

    property var entries: []
    property int sideMargin: Sizing.pctW(2)
    readonly property int _entryHeight: Math.max(Sizing.pctH(4), Sizing.fontBody)
    readonly property int _rowSpacing: Sizing.pctH(1)
    readonly property int _splitIndex: Math.ceil(root.entries.length / 2)
    readonly property int _safeWidth: Math.max(0, width - 2 * sideMargin)
    readonly property bool wrapEntries: Sizing.tier === "240" && singleRow.implicitWidth > root._safeWidth
    readonly property int contentHeight: root.wrapEntries ? wrappedRows.height : singleRow.height

    Row {
        id: singleRow

        visible: !root.wrapEntries
        x: Sizing.center(parent.width, width)
        y: Sizing.center(parent.height, height) + Sizing.pctH(0.2)
        height: root._entryHeight
        spacing: Sizing.pctW(2)

        Repeater {
            model: root.entries

            delegate: HelpBarEntry {
                required property int index
                required property var modelData

                entryIndex: index
                entry: modelData
            }
        }
    }

    Item {
        id: wrappedRows

        objectName: "wrappedHelpRow"
        visible: root.wrapEntries
        anchors.left: parent.left
        anchors.leftMargin: root.sideMargin
        anchors.right: parent.right
        anchors.rightMargin: root.sideMargin
        anchors.verticalCenter: parent.verticalCenter
        height: 2 * root._entryHeight + root._rowSpacing

        Row {
            id: firstRow

            x: Sizing.center(parent.width, width)
            height: root._entryHeight
            spacing: Sizing.pctW(2)

            Repeater {
                model: root.entries.slice(0, root._splitIndex)

                delegate: HelpBarEntry {
                    required property int index
                    required property var modelData

                    entryIndex: index
                    entry: modelData
                }
            }
        }

        Row {
            id: secondRow

            x: Sizing.center(parent.width, width)
            y: root._entryHeight + root._rowSpacing
            height: root._entryHeight
            spacing: Sizing.pctW(2)

            Repeater {
                model: root.entries.slice(root._splitIndex)

                delegate: HelpBarEntry {
                    required property int index
                    required property var modelData

                    entryIndex: root._splitIndex + index
                    entry: modelData
                }
            }
        }
    }
}
