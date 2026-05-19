// Zaparoo Launcher
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import Zaparoo.Browse as Browse
import Zaparoo.Theme

Item {
    id: root

    property bool open: false
    property int _labelColumnWidth: 0

    signal closeRequested

    visible: open
    enabled: visible
    anchors.fill: parent
    z: 300

    onOpenChanged: root._labelColumnWidth = 0

    function handleAction(action: string): void {
        if (action === "cancel" || action === "accept")
            root.closeRequested();
        else if (action === "left")
            Browse.GameInfo.cycle_image(-1);
        else if (action === "right")
            Browse.GameInfo.cycle_image(1);
    }

    Rectangle {
        anchors.fill: parent
        color: Theme.scrim

        MouseArea {
            anchors.fill: parent
            hoverEnabled: true
            acceptedButtons: Qt.AllButtons
            onClicked: root.closeRequested()
        }

        Rectangle {
            id: panel

            x: Sizing.center(parent.width, width)
            y: Sizing.center(parent.height, height)
            width: Sizing.px(Math.min(parent.width * 0.84, Sizing.pctH(118)))
            height: Sizing.px(Math.min(parent.height - Sizing.pctH(14), Sizing.pctH(78)))
            color: Theme.bgPanel
            radius: Sizing.cornerRadius

            MouseArea {
                anchors.fill: parent
                hoverEnabled: true
                acceptedButtons: Qt.AllButtons
            }

            Text {
                id: titleText

                anchors.left: parent.left
                anchors.leftMargin: Sizing.pctW(4)
                anchors.right: closeText.left
                anchors.rightMargin: Sizing.pctW(2)
                anchors.top: parent.top
                anchors.topMargin: Sizing.pctH(4)
                text: Browse.GameInfo.title
                color: Theme.textPrimary
                font.family: Theme.fontUi
                font.pixelSize: Sizing.fontSize(3.2)
                font.weight: Font.Medium
                elide: Text.ElideRight
                horizontalAlignment: Text.AlignLeft
                renderType: Text.NativeRendering
            }

            Text {
                id: closeText

                anchors.right: parent.right
                anchors.rightMargin: Sizing.pctW(4)
                anchors.verticalCenter: titleText.verticalCenter
                text: qsTr("Back to close")
                color: Theme.textLabel
                font.family: Theme.fontUi
                font.pixelSize: Sizing.fontSize(2.2)
                horizontalAlignment: Text.AlignRight
                renderType: Text.NativeRendering
            }

            Rectangle {
                id: contentCard

                anchors.left: parent.left
                anchors.leftMargin: Sizing.pctW(4)
                anchors.right: parent.right
                anchors.rightMargin: Sizing.pctW(4)
                anchors.top: titleText.bottom
                anchors.topMargin: Sizing.pctH(3)
                anchors.bottom: parent.bottom
                anchors.bottomMargin: Sizing.pctH(4)
                color: Theme.surfaceCard
                border.width: Sizing.stroke(1)
                border.color: Theme.borderMid
                radius: Sizing.cornerRadius

                LoadingIndicator {
                    visible: Browse.GameInfo.loading
                    x: Sizing.center(parent.width, width)
                    y: Sizing.center(parent.height, height)
                    text: qsTr("Loading details…")
                }

                Text {
                    visible: !Browse.GameInfo.loading && Browse.GameInfo.error_message !== ""
                    anchors.left: parent.left
                    anchors.leftMargin: Sizing.pctW(3)
                    anchors.right: parent.right
                    anchors.rightMargin: Sizing.pctW(3)
                    anchors.verticalCenter: parent.verticalCenter
                    text: Browse.GameInfo.error_message
                    color: Theme.textPrimary
                    font.family: Theme.fontUi
                    font.pixelSize: Sizing.fontSize(2.6)
                    wrapMode: Text.WordWrap
                    horizontalAlignment: Text.AlignHCenter
                    renderType: Text.NativeRendering
                }

                Flickable {
                    id: flick

                    visible: !Browse.GameInfo.loading && Browse.GameInfo.error_message === ""
                    anchors.fill: parent
                    anchors.leftMargin: Sizing.pctW(3)
                    anchors.rightMargin: Sizing.pctW(3)
                    anchors.topMargin: Sizing.pctH(3)
                    anchors.bottomMargin: Sizing.pctH(3)
                    contentWidth: width
                    contentHeight: contentColumn.height
                    boundsBehavior: Flickable.StopAtBounds
                    clip: true

                    Column {
                        id: contentColumn

                        width: flick.width
                        spacing: Sizing.pctH(2.2)

                        Item {
                            width: parent.width
                            height: Browse.GameInfo.image_key !== "" ? Sizing.pctH(26) : 0
                            visible: height > 0

                            Image {
                                anchors.fill: parent
                                source: Resources.coverUrl(Browse.GameInfo.image_key)
                                sourceSize.width: 512
                                fillMode: Image.PreserveAspectFit
                                smooth: true
                                asynchronous: true
                            }

                            Image {
                                source: Resources.iconUrl("NavLeft")
                                width: Sizing.pctH(4)
                                height: width
                                anchors.left: parent.left
                                anchors.verticalCenter: parent.verticalCenter
                                fillMode: Image.PreserveAspectFit
                                smooth: true
                                visible: Browse.GameInfo.image_can_prev
                            }

                            Image {
                                source: Resources.iconUrl("NavRight")
                                width: Sizing.pctH(4)
                                height: width
                                anchors.right: parent.right
                                anchors.verticalCenter: parent.verticalCenter
                                fillMode: Image.PreserveAspectFit
                                smooth: true
                                visible: Browse.GameInfo.image_can_next
                            }
                        }

                        Column {
                            id: tagTable

                            width: parent.width
                            spacing: Sizing.pctH(0.8)
                            visible: Browse.GameInfo.detail_tags !== ""

                            Repeater {
                                model: Browse.GameInfo.detail_tags === "" ? [] : Browse.GameInfo.detail_tags.split("\n")

                                delegate: Item {
                                    id: tagRow

                                    required property string modelData

                                    width: tagTable.width
                                    height: Math.max(Sizing.pctH(3), tagValue.paintedHeight)

                                    readonly property list<string> parts: modelData.split("\t")
                                    readonly property string label: parts.length > 0 ? parts[0] : ""
                                    readonly property string value: parts.length > 1 ? parts[1] : ""

                                    TextMetrics {
                                        id: labelMetrics
                                        text: tagRow.label
                                        font.family: Theme.fontUi
                                        font.pixelSize: Sizing.fontSize(2.4)
                                        onAdvanceWidthChanged: root._labelColumnWidth = Math.max(root._labelColumnWidth, Math.ceil(advanceWidth))
                                    }

                                    Component.onCompleted: root._labelColumnWidth = Math.max(root._labelColumnWidth, Math.ceil(labelMetrics.advanceWidth))

                                    Text {
                                        anchors.left: parent.left
                                        anchors.top: parent.top
                                        width: root._labelColumnWidth
                                        text: tagRow.label
                                        color: Theme.textLabel
                                        font.family: Theme.fontUi
                                        font.pixelSize: Sizing.fontSize(2.4)
                                        elide: Text.ElideRight
                                        horizontalAlignment: Text.AlignLeft
                                        renderType: Text.NativeRendering
                                    }

                                    Text {
                                        id: tagValue

                                        anchors.left: parent.left
                                        anchors.leftMargin: root._labelColumnWidth + Sizing.pctW(1.4)
                                        anchors.right: parent.right
                                        anchors.top: parent.top
                                        text: tagRow.value
                                        color: Theme.textPrimary
                                        font.family: Theme.fontUi
                                        font.pixelSize: Sizing.fontSize(2.4)
                                        wrapMode: Text.Wrap
                                        horizontalAlignment: Text.AlignLeft
                                        renderType: Text.NativeRendering
                                    }
                                }
                            }
                        }

                        Text {
                            width: parent.width
                            visible: Browse.GameInfo.description !== ""
                            text: Browse.GameInfo.description
                            color: Theme.textPrimary
                            font.family: Theme.fontUi
                            font.pixelSize: Sizing.fontSize(2.6)
                            wrapMode: Text.WordWrap
                            horizontalAlignment: Text.AlignLeft
                            renderType: Text.NativeRendering
                        }
                    }
                }
            }
        }
    }
}
