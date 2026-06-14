// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
// cxx-qt 0.8 Browse singleton methods lack isFinal in the qmltypes schema so
// every access trips "Member can be shadowed". Structural; suppress compiler.
// qmllint disable compiler
pragma ComponentBehavior: Bound

import QtQuick
import Zaparoo.Browse as Browse
import Zaparoo.Theme

Item {
    id: root

    property bool open: false
    property int _labelColumnWidth: 0
    readonly property bool _hasContentAbove: flick.contentY > 1
    readonly property bool _hasContentBelow: flick.contentY + flick.height < flick.contentHeight - 1

    signal closeRequested

    visible: open
    enabled: visible
    anchors.fill: parent
    z: 300

    onOpenChanged: {
        root._labelColumnWidth = 0;
        if (root.open)
            flick.contentY = 0;
    }

    function _scrollBody(delta: int): void {
        if (!flick.visible)
            return;
        const maxY = Math.max(0, flick.contentHeight - flick.height);
        flick.contentY = Math.max(0, Math.min(maxY, flick.contentY + delta));
    }

    function handleAction(action: string): void {
        if (action === "cancel" || action === "accept")
            root.closeRequested();
        else if (action === "left" && Browse.GameInfo.image_count > 1)
            Browse.GameInfo.cycle_image(-1);
        else if (action === "right" && Browse.GameInfo.image_count > 1)
            Browse.GameInfo.cycle_image(1);
        else if (action === "up")
            root._scrollBody(-Sizing.pctH(8));
        else if (action === "down")
            root._scrollBody(Sizing.pctH(8));
        else if (action === "page_prev")
            root._scrollBody(-Math.max(Sizing.pctH(12), flick.height - Sizing.pctH(8)));
        else if (action === "page_next")
            root._scrollBody(Math.max(Sizing.pctH(12), flick.height - Sizing.pctH(8)));
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
            width: Sizing.px(Math.min(parent.width - Sizing.pctW(6), Sizing.pctH(150)))
            height: Sizing.px(parent.height - Sizing.pctH(16))
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
                anchors.right: parent.right
                anchors.rightMargin: Sizing.pctW(4)
                anchors.top: parent.top
                anchors.topMargin: Sizing.pctH(4)
                text: Browse.GameInfo.title
                color: Theme.textPrimary
                font.family: Theme.fontUi
                font.pixelSize: Sizing.fontSize(3.4)
                font.weight: Font.Medium
                elide: Text.ElideRight
                maximumLineCount: 1
                horizontalAlignment: Text.AlignLeft
                renderType: Text.NativeRendering
            }

            LoadingIndicator {
                visible: Browse.GameInfo.loading
                x: Sizing.center(parent.width, width)
                y: Sizing.center(parent.height, height)
                text: qsTr("Loading details…")
            }

            Text {
                visible: !Browse.GameInfo.loading && Browse.GameInfo.error_message !== ""
                anchors.left: parent.left
                anchors.leftMargin: Sizing.pctW(4)
                anchors.right: parent.right
                anchors.rightMargin: Sizing.pctW(4)
                anchors.top: titleText.bottom
                anchors.topMargin: Sizing.pctH(4)
                text: Browse.GameInfo.error_message
                color: Theme.textPrimary
                font.family: Theme.fontUi
                font.pixelSize: Sizing.fontSize(2.6)
                wrapMode: Text.WordWrap
                horizontalAlignment: Text.AlignLeft
                renderType: Text.NativeRendering
            }

            Flickable {
                id: flick

                visible: !Browse.GameInfo.loading && Browse.GameInfo.error_message === ""
                anchors.left: parent.left
                anchors.leftMargin: Sizing.pctW(4)
                anchors.right: parent.right
                anchors.rightMargin: Sizing.pctW(4)
                anchors.top: titleText.bottom
                anchors.topMargin: Sizing.pctH(3)
                anchors.bottom: parent.bottom
                anchors.bottomMargin: Sizing.pctH(4)
                contentWidth: width
                contentHeight: contentColumn.height
                boundsBehavior: Flickable.StopAtBounds
                clip: true

                Column {
                    id: contentColumn

                    width: flick.width
                    spacing: Sizing.pctH(2.4)

                    Item {
                        width: parent.width
                        height: Browse.GameInfo.image_count > 0 ? Sizing.pctH(32) : 0
                        visible: height > 0

                        Image {
                            anchors.fill: parent
                            source: Browse.GameInfo.image_key !== "" ? Resources.coverUrl(Browse.GameInfo.image_key, Theme.textPrimary, Theme.surfaceCard) : ""
                            sourceSize.width: Sizing.px(parent.width)
                            fillMode: Image.PreserveAspectFit
                            asynchronous: true
                        }

                        LoadingIndicator {
                            visible: Browse.GameInfo.image_key === ""
                            x: Sizing.center(parent.width, width)
                            y: Sizing.center(parent.height, height)
                            text: qsTr("Loading image…")
                            glyphSize: Sizing.fontSize(2.4)
                        }

                        Image {
                            source: Resources.iconUrl("NavLeft")
                            width: Sizing.pctH(4)
                            height: width
                            anchors.left: parent.left
                            anchors.verticalCenter: parent.verticalCenter
                            fillMode: Image.PreserveAspectFit
                            smooth: true
                            visible: Browse.GameInfo.image_count > 1 && Browse.GameInfo.image_can_prev
                        }

                        Image {
                            source: Resources.iconUrl("NavRight")
                            width: Sizing.pctH(4)
                            height: width
                            anchors.right: parent.right
                            anchors.verticalCenter: parent.verticalCenter
                            fillMode: Image.PreserveAspectFit
                            smooth: true
                            visible: Browse.GameInfo.image_count > 1 && Browse.GameInfo.image_can_next
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

            Image {
                source: Resources.iconUrl("ScrollUp")
                width: Sizing.pctH(3)
                height: width
                anchors.bottom: flick.top
                anchors.bottomMargin: Sizing.pctH(0.5)
                anchors.horizontalCenter: flick.horizontalCenter
                fillMode: Image.PreserveAspectFit
                smooth: true
                visible: flick.visible && root._hasContentAbove
            }

            Image {
                source: Resources.iconUrl("ScrollDown")
                width: Sizing.pctH(3)
                height: width
                anchors.top: flick.bottom
                anchors.topMargin: Sizing.pctH(0.5)
                anchors.horizontalCenter: flick.horizontalCenter
                fillMode: Image.PreserveAspectFit
                smooth: true
                visible: flick.visible && root._hasContentBelow
            }
        }
    }
}
