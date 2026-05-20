// Zaparoo Launcher
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import Zaparoo.Theme

Item {
    id: root

    property string title: ""
    property string coverKey: ""
    property string description: ""
    property bool showDescription: true
    property bool showTitle: true
    property string detailTags: ""
    property bool canPreviousImage: false
    property bool canNextImage: false
    property bool loading: false
    property bool showChrome: true
    property string loadingText: qsTr("Loading…")

    readonly property int _cardPaddingX: Sizing.pctW(2)
    readonly property int _cardPaddingY: Sizing.pctH(2)
    readonly property int _carouselGutter: (canPreviousImage || canNextImage) ? Sizing.pctW(4) : 0
    readonly property int _labelColumnWidth: Sizing.pctW(8)
    readonly property int _tagLabelGap: Sizing.pctW(1.4)
    readonly property int _tagRowCount: detailTags === "" ? 0 : detailTags.split("\n").length
    readonly property int _tagRowSpacing: Sizing.pctH(0.8)
    readonly property int _metadataNaturalHeight: _tagRowCount <= 0 ? 0 : (_tagRowCount * Sizing.pctH(3)) + ((_tagRowCount - 1) * _tagRowSpacing)
    readonly property int _compactDetailHeight: Math.min(Sizing.px(content.height * 0.45), Math.max(Sizing.pctH(12), _metadataNaturalHeight))
    readonly property bool _coverPending: coverKey === "icons/Loading"
    readonly property url _coverSource: _coverPending ? "" : Resources.coverUrl(coverKey)
    readonly property bool _paneLoading: root.loading

    Rectangle {
        anchors.fill: parent
        color: Theme.surfaceCard
        border.width: Sizing.stroke(1)
        border.color: Theme.borderMid
        radius: Sizing.cornerRadius
        visible: root.showChrome
    }

    Item {
        id: content

        anchors.fill: parent
        anchors.leftMargin: root._cardPaddingX
        anchors.rightMargin: root._cardPaddingX
        anchors.topMargin: root._cardPaddingY
        anchors.bottomMargin: root._cardPaddingY
        clip: true

        Item {
            id: imageSlot

            anchors.left: parent.left
            anchors.leftMargin: root._carouselGutter
            anchors.right: parent.right
            anchors.rightMargin: root._carouselGutter
            anchors.top: parent.top
            height: root.showTitle ? Sizing.px(parent.height * 0.48) : Math.max(0, detailBody.y - Sizing.pctH(2))

            Image {
                id: cover
                anchors.fill: parent
                source: root._coverSource
                fillMode: Image.PreserveAspectFit
                sourceSize.width: 512
                smooth: true
                asynchronous: true
                visible: !root._paneLoading && root._coverSource !== "" && status === Image.Ready
            }

            Image {
                x: Sizing.center(parent.width, width)
                y: Sizing.center(parent.height, height)
                width: Math.min(Sizing.pctH(10), parent.width, parent.height)
                height: width
                source: Resources.iconUrl(root._coverPending || cover.status === Image.Loading ? "Loading" : "File")
                sourceSize.width: Sizing.px(width)
                sourceSize.height: Sizing.px(height)
                fillMode: Image.PreserveAspectFit
                smooth: true
                asynchronous: false
                visible: !root._paneLoading && (root._coverPending || cover.status === Image.Loading || root._coverSource === "" || cover.status === Image.Error)
            }
        }

        Image {
            source: Resources.iconUrl("NavLeft")
            width: Sizing.pctH(4)
            height: width
            anchors.left: parent.left
            anchors.verticalCenter: imageSlot.verticalCenter
            fillMode: Image.PreserveAspectFit
            smooth: true
            visible: !root._paneLoading && root.canPreviousImage
        }

        Image {
            source: Resources.iconUrl("NavRight")
            width: Sizing.pctH(4)
            height: width
            anchors.right: parent.right
            anchors.verticalCenter: imageSlot.verticalCenter
            fillMode: Image.PreserveAspectFit
            smooth: true
            visible: !root._paneLoading && root.canNextImage
        }

        Text {
            id: titleText

            anchors.left: parent.left
            anchors.right: parent.right
            anchors.top: imageSlot.bottom
            anchors.topMargin: Sizing.pctH(2)
            text: root.title
            color: Theme.textPrimary
            font.family: Theme.fontUi
            font.pixelSize: Sizing.fontSize(3.2)
            wrapMode: Text.Wrap
            maximumLineCount: 3
            elide: Text.ElideRight
            horizontalAlignment: Text.AlignLeft
            renderType: Text.NativeRendering
            visible: !root._paneLoading && root.showTitle && root.title !== ""
        }

        Item {
            id: detailBody

            x: 0
            y: root.showTitle ? (titleText.visible ? titleText.y + titleText.height : imageSlot.y + imageSlot.height) + Sizing.pctH(2) : parent.height - height
            width: parent.width
            height: root.showTitle ? Math.max(0, parent.height - y) : root._compactDetailHeight
            clip: true

            Column {
                id: tagTable

                visible: !root._paneLoading && root.detailTags !== ""
                anchors.fill: parent
                spacing: root._tagRowSpacing
                clip: true

                Repeater {
                    model: root.detailTags === "" ? [] : root.detailTags.split("\n")

                    delegate: Item {
                        id: tagRow

                        required property string modelData

                        width: tagTable.width
                        height: Math.max(Sizing.pctH(3), tagValue.paintedHeight)

                        readonly property list<string> parts: modelData.split("\t")
                        readonly property string label: parts.length > 0 ? parts[0] : ""
                        readonly property string value: parts.length > 1 ? parts[1] : ""
                        readonly property bool isFilename: label === "Filename"

                        Text {
                            id: tagType

                            anchors.left: parent.left
                            anchors.top: parent.top
                            width: root._labelColumnWidth
                            text: tagRow.label
                            color: Theme.textLabel
                            font.family: Theme.fontUi
                            font.pixelSize: Sizing.fontSize(2.2)
                            elide: Text.ElideRight
                            horizontalAlignment: Text.AlignLeft
                            renderType: Text.NativeRendering
                        }

                        Text {
                            id: tagValue

                            anchors.left: parent.left
                            anchors.leftMargin: root._labelColumnWidth + root._tagLabelGap
                            anchors.right: parent.right
                            anchors.top: parent.top
                            text: tagRow.value
                            color: Theme.textPrimary
                            font.family: Theme.fontUi
                            font.pixelSize: Sizing.fontSize(2.2)
                            wrapMode: Text.Wrap
                            maximumLineCount: tagRow.isFilename ? 8 : 2
                            elide: tagRow.isFilename ? Text.ElideNone : Text.ElideRight
                            horizontalAlignment: Text.AlignLeft
                            renderType: Text.NativeRendering
                        }
                    }
                }
            }
        }

        LoadingIndicator {
            visible: root._paneLoading
            x: Sizing.center(parent.width, width)
            y: Sizing.center(parent.height, height)
            text: root.loadingText
        }
    }
}
