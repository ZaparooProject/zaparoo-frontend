// Zaparoo Frontend
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
    property bool detailSuppressed: false
    property bool showChrome: true
    property string loadingText: qsTr("Loading…")

    readonly property int _cardPaddingX: Sizing.pctW(2)
    readonly property int _cardPaddingY: Sizing.pctH(2)
    readonly property int _carouselGutter: (canPreviousImage || canNextImage) ? Sizing.pctW(4) : 0
    property int _labelColumnWidth: 0
    readonly property int _tagTextSize: Sizing.fontSize(2.2)
    readonly property int _tagLabelGap: Sizing.pctW(1.4)
    readonly property var _detailRows: _parseDetailTags(detailTags)
    readonly property int _tagRowCount: _detailRows.length
    readonly property int _tagRowHeight: Sizing.pctH(3)
    readonly property int _tagRowSpacing: Sizing.pctH(0.55)
    readonly property int _metadataNaturalHeight: _tagRowCount <= 0 ? 0 : (_tagRowCount * _tagRowHeight) + ((_tagRowCount - 1) * _tagRowSpacing)
    readonly property int _compactDetailHeight: Math.min(Sizing.px(content.height * 0.38), _metadataNaturalHeight)
    readonly property bool _coverPending: coverKey === "icons/Loading"
    readonly property url _coverSource: _coverPending ? "" : Resources.coverUrl(coverKey)
    readonly property bool _paneLoading: root.loading
    readonly property bool _detailVisible: !root._paneLoading && !root.detailSuppressed
    readonly property bool _suppressedPlaceholderCover: root.detailSuppressed && coverKey.startsWith("icons/") && root._coverSource !== ""

    onDetailTagsChanged: root._labelColumnWidth = 0

    function _localizedTagLabel(label: string): string {
        if (label === "Year")
            return qsTr("Year");
        if (label === "Genre")
            return qsTr("Genre");
        if (label === "Players")
            return qsTr("Players");
        if (label === "Developer")
            return qsTr("Developer");
        if (label === "Publisher")
            return qsTr("Publisher");
        if (label === "Rating")
            return qsTr("Rating");
        if (label === "Category")
            return qsTr("Category");
        if (label === "Release date")
            return qsTr("Release date");
        if (label === "Manufacturer")
            return qsTr("Manufacturer");
        return label;
    }

    function _parseDetailTags(tags: string): var {
        if (tags === "")
            return [];
        return tags.split("\n").map(row => {
            const parts = row.split("\t");
            const rawLabel = parts.length > 0 ? parts[0] : "";
            return {
                "rawLabel": rawLabel,
                "label": root._localizedTagLabel(rawLabel),
                "value": parts.length > 1 ? parts[1] : ""
            };
        });
    }

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

            readonly property int availableWidth: Math.max(0, parent.width - (2 * root._carouselGutter))
            readonly property int availableHeight: Math.max(0, root.showTitle ? Sizing.px(parent.height * 0.48) : detailBody.y - Sizing.pctH(1))
            readonly property int slotSize: Math.min(availableWidth, availableHeight)

            x: root._carouselGutter + Sizing.center(availableWidth, width)
            anchors.top: parent.top
            width: slotSize
            height: slotSize

            Image {
                id: cover
                anchors.fill: parent
                source: root._coverSource
                fillMode: Image.PreserveAspectFit
                sourceSize.width: 512
                smooth: true
                asynchronous: true
                visible: !root._paneLoading && root._coverSource !== "" && status === Image.Ready && (!root.detailSuppressed || root._suppressedPlaceholderCover)
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
                visible: !root._paneLoading && !root._suppressedPlaceholderCover && (root.detailSuppressed || root._coverPending || cover.status === Image.Loading || root._coverSource === "" || cover.status === Image.Error)
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
            visible: root._detailVisible && root.canPreviousImage
        }

        Image {
            source: Resources.iconUrl("NavRight")
            width: Sizing.pctH(4)
            height: width
            anchors.right: parent.right
            anchors.verticalCenter: imageSlot.verticalCenter
            fillMode: Image.PreserveAspectFit
            smooth: true
            visible: root._detailVisible && root.canNextImage
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
            visible: root._detailVisible && root.showTitle && root.title !== ""
        }

        Item {
            id: detailBody

            readonly property int _bodyY: Math.round(root.showTitle ? (titleText.visible ? titleText.y + titleText.height : imageSlot.y + imageSlot.height) + Sizing.pctH(2) : parent.height - height)

            x: 0
            y: _bodyY
            width: parent.width
            height: root.showTitle ? Math.round(Math.max(0, parent.height - _bodyY)) : root._compactDetailHeight
            clip: true

            Column {
                id: tagTable

                visible: root._detailVisible && root._detailRows.length > 0
                anchors.fill: parent
                spacing: root._tagRowSpacing
                clip: true

                Repeater {
                    model: root._detailRows

                    delegate: Item {
                        id: tagRow

                        required property var modelData

                        width: tagTable.width
                        height: root._tagRowHeight

                        readonly property string rawLabel: modelData.rawLabel ?? ""
                        readonly property string label: modelData.label ?? ""
                        readonly property string value: modelData.value ?? ""

                        TextMetrics {
                            id: labelMetrics

                            text: tagRow.label
                            font.family: Theme.fontUi
                            font.pixelSize: root._tagTextSize
                            onAdvanceWidthChanged: root._labelColumnWidth = Math.max(root._labelColumnWidth, Math.ceil(advanceWidth))
                        }

                        Component.onCompleted: root._labelColumnWidth = Math.max(root._labelColumnWidth, Math.ceil(labelMetrics.advanceWidth))

                        Text {
                            id: tagType

                            anchors.left: parent.left
                            anchors.top: parent.top
                            width: root._labelColumnWidth
                            text: tagRow.label
                            color: Theme.textLabel
                            font.family: Theme.fontUi
                            font.pixelSize: root._tagTextSize
                            elide: Text.ElideRight
                            horizontalAlignment: Text.AlignRight
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
                            font.pixelSize: root._tagTextSize
                            wrapMode: Text.NoWrap
                            maximumLineCount: 1
                            elide: Text.ElideRight
                            horizontalAlignment: Text.AlignLeft
                            renderType: Text.NativeRendering
                        }
                    }
                }
            }
        }

        LoadingIndicator {
            visible: root._paneLoading && !root.detailSuppressed
            x: Sizing.center(parent.width, width)
            y: Sizing.center(parent.height, height)
            text: root.loadingText
        }
    }
}
