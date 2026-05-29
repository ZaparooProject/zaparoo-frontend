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
    property var layoutProfile: null

    readonly property var _detail: root.layoutProfile && root.layoutProfile.detail ? root.layoutProfile.detail : null
    readonly property var _surface: root.layoutProfile && root.layoutProfile.surface ? root.layoutProfile.surface : null
    readonly property int _panePaddingLeft: root._detail ? root._detail.panePaddingLeft : Sizing.pctW(2)
    readonly property int _panePaddingRight: root._detail ? root._detail.panePaddingRight : Sizing.pctW(2)
    readonly property int _panePaddingTop: root._detail ? root._detail.panePaddingTop : Sizing.pctH(2)
    readonly property int _panePaddingBottom: root._detail ? root._detail.panePaddingBottom : Sizing.pctH(2)
    readonly property int _imagePaddingLeft: root._detail ? root._detail.imagePaddingLeft : 0
    readonly property int _imagePaddingRight: root._detail ? root._detail.imagePaddingRight : 0
    readonly property int _imagePaddingTop: root._detail ? root._detail.imagePaddingTop : 0
    readonly property int _imagePaddingBottom: root._detail ? root._detail.imagePaddingBottom : 0
    readonly property int _metadataPaddingLeft: root._detail ? root._detail.metadataPaddingLeft : 0
    readonly property int _metadataPaddingRight: root._detail ? root._detail.metadataPaddingRight : 0
    readonly property int _metadataPaddingTop: root._detail ? root._detail.metadataPaddingTop : 0
    readonly property int _metadataPaddingBottom: root._detail ? root._detail.metadataPaddingBottom : 0
    readonly property int _metadataTopMargin: root._detail && root._detail.metadataTopMargin !== undefined ? root._detail.metadataTopMargin : 0
    readonly property int _metadataLeftMargin: root._detail && root._detail.metadataLeftMargin !== undefined ? root._detail.metadataLeftMargin : 0
    readonly property int _metadataRightMargin: root._detail && root._detail.metadataRightMargin !== undefined ? root._detail.metadataRightMargin : 0
    readonly property int _metadataHeightAdjustment: root._detail && root._detail.metadataHeightAdjustment !== undefined ? root._detail.metadataHeightAdjustment : 0
    readonly property int _sectionGap: root._detail ? root._detail.sectionGap : Sizing.pctH(2)
    readonly property bool _horizontalSections: root._detail && root._detail.contentAxis === "horizontal"
    readonly property real _imageShare: root._detail && root._detail.imageShare !== undefined ? root._detail.imageShare : 1
    readonly property real _metadataShare: root._detail && root._detail.metadataShare !== undefined ? root._detail.metadataShare : 1
    readonly property real _shareTotal: Math.max(1, root._imageShare + root._metadataShare)
    readonly property int _tagRowHeight: root._detail ? root._detail.tagRowHeight : Sizing.pctH(3)
    readonly property int _tagRowSpacing: root._detail ? root._detail.tagRowSpacing : Sizing.pctH(0.55)
    readonly property bool _metadataBottomAligned: root._detail && root._detail.metadataBottomAligned === true
    readonly property int _titleBottomMargin: root._detail ? root._detail.titleBottomMargin : Sizing.pctH(2)
    readonly property real _imageHeightRatioWithTitle: root._detail && root._detail.imageHeightRatioWithTitle !== undefined ? root._detail.imageHeightRatioWithTitle : 48
    readonly property int _imageReservedWidth: root._detail && root._detail.imageReservedWidth !== undefined ? root._detail.imageReservedWidth : 0
    readonly property int _imageReservedHeight: root._detail && root._detail.imageReservedHeight !== undefined ? root._detail.imageReservedHeight : 0
    readonly property int _imageBottomMargin: root._detail && root._detail.imageBottomMargin !== undefined ? root._detail.imageBottomMargin : 0
    readonly property int _cardRadius: root._surface ? root._surface.cornerRadius : Sizing.cornerRadius
    readonly property int _carouselGutter: (canPreviousImage || canNextImage) ? Sizing.pctW(4) : 0
    readonly property bool _coverPending: coverKey === "icons/Loading"
    readonly property url _coverSource: _coverPending ? "" : Resources.coverUrl(coverKey)
    readonly property bool _paneLoading: root.loading
    readonly property bool _detailVisible: !root._paneLoading && !root.detailSuppressed
    readonly property bool _suppressedPlaceholderCover: root.detailSuppressed && coverKey.startsWith("icons/") && root._coverSource !== ""
    readonly property var _detailRows: _parseDetailTags(detailTags)
    readonly property int _tagRowCount: _detailRows.length
    readonly property int _tagTextSize: Sizing.fontSize(2.2)
    readonly property int _tagLabelGap: Sizing.pctW(1.4)
    readonly property int _metadataNaturalHeight: _tagRowCount <= 0 ? 0 : (_tagRowCount * _tagRowHeight) + ((_tagRowCount - 1) * _tagRowSpacing)
    readonly property int _compactMetadataHeight: Math.min(Sizing.px(content.height * 0.38), _metadataNaturalHeight)

    property int _labelColumnWidth: 0

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
        radius: root._cardRadius
        visible: root.showChrome
    }

    Item {
        id: content

        anchors.fill: parent
        anchors.leftMargin: root._panePaddingLeft
        anchors.rightMargin: root._panePaddingRight
        anchors.topMargin: root._panePaddingTop
        anchors.bottomMargin: root._panePaddingBottom
        clip: true

        readonly property int primarySpan: root._horizontalSections ? Math.floor((width - root._sectionGap) * root._imageShare / root._shareTotal) : Math.floor((height - root._sectionGap) * root._imageShare / root._shareTotal)
        readonly property int secondarySpan: root._horizontalSections ? Math.max(0, width - primarySpan - root._sectionGap) : Math.max(0, height - imageSlotHeight - root._sectionGap)
        readonly property int imageSlotX: root._horizontalSections ? root._carouselGutter : root._imagePaddingLeft + root._carouselGutter
        readonly property int imageSlotY: 0
        readonly property int imageSlotWidth: {
            if (root._horizontalSections)
                return Math.max(0, primarySpan - (2 * root._carouselGutter));
            const availableWidth = Math.max(0, width - (2 * root._carouselGutter) - root._imagePaddingLeft - root._imagePaddingRight);
            const maxWidth = Math.max(0, width - root._imagePaddingLeft - root._imagePaddingRight);
            return Math.max(0, Math.min(maxWidth, availableWidth + root._imageReservedWidth));
        }
        readonly property int imageSlotHeight: {
            if (root._horizontalSections)
                return height;
            const availableWidth = Math.max(0, width - (2 * root._carouselGutter) - root._imagePaddingLeft - root._imagePaddingRight);
            const imageLimit = titleText.visible ? Math.floor((height * root._imageHeightRatioWithTitle) / 100) : Math.max(0, height - root._compactMetadataHeight - root._imageBottomMargin);
            return Math.max(0, Math.min(height, Math.min(availableWidth, imageLimit) + root._imageReservedHeight));
        }
        readonly property int metadataX: root._horizontalSections ? primarySpan + root._sectionGap : 0
        readonly property int metadataY: root._horizontalSections ? 0 : imageSlotHeight + root._sectionGap
        readonly property int metadataWidth: root._horizontalSections ? secondarySpan : width
        readonly property int metadataHeight: root._horizontalSections ? height : secondarySpan

        Item {
            id: imageSlot

            x: content.imageSlotX
            y: content.imageSlotY
            width: content.imageSlotWidth
            height: content.imageSlotHeight
            clip: true

            Item {
                anchors.fill: parent
                anchors.leftMargin: root._horizontalSections ? root._imagePaddingLeft : 0
                anchors.rightMargin: root._horizontalSections ? root._imagePaddingRight : 0
                anchors.topMargin: root._imagePaddingTop
                anchors.bottomMargin: root._imagePaddingBottom

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

        Item {
            id: metadataSlot

            x: content.metadataX
            y: content.metadataY
            width: content.metadataWidth
            height: content.metadataHeight
            clip: true

            Item {
                id: metadataInner

                anchors.fill: parent
                anchors.leftMargin: root._horizontalSections ? root._metadataPaddingLeft : root._metadataLeftMargin
                anchors.rightMargin: root._horizontalSections ? root._metadataPaddingRight : root._metadataRightMargin
                anchors.topMargin: root._horizontalSections ? root._metadataPaddingTop : 0
                anchors.bottomMargin: root._horizontalSections ? root._metadataPaddingBottom : 0
                clip: true

                Text {
                    id: titleText

                    anchors.left: parent.left
                    anchors.right: parent.right
                    anchors.top: parent.top
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

                    x: 0
                    y: titleText.visible ? titleText.y + titleText.height + root._titleBottomMargin + root._metadataTopMargin : (root._horizontalSections ? 0 : root._metadataTopMargin)
                    width: parent.width
                    height: {
                        if (root._horizontalSections)
                            return Math.max(0, parent.height - y);
                        if (titleText.visible)
                            return Math.max(0, parent.height - y + root._metadataHeightAdjustment);
                        return root._compactMetadataHeight;
                    }
                    clip: true

                    Column {
                        id: tagTable

                        anchors.left: parent.left
                        anchors.right: parent.right
                        anchors.top: root._metadataBottomAligned && !titleText.visible ? undefined : parent.top
                        anchors.bottom: root._metadataBottomAligned && !titleText.visible ? parent.bottom : undefined
                        spacing: root._tagRowSpacing
                        clip: true
                        visible: root._detailVisible && root._detailRows.length > 0

                        Repeater {
                            model: root._detailRows

                            delegate: Item {
                                id: tagRow

                                required property var modelData

                                width: tagTable.width
                                height: root._tagRowHeight

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
