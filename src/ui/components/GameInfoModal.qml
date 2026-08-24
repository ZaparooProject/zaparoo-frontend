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
    readonly property bool _hasContentAbove: flick.contentY > 1
    readonly property bool _hasContentBelow: flick.contentY + flick.height < flick.contentHeight - 1
    // Round 10: scroll chevrons only paint when there's genuinely more
    // than one screenful -- see `_hasContentAbove`/`_hasContentBelow`
    // below, which mirror the same "hide when there's nothing to page/
    // scroll to" rule PageIndicator.qml applies to grid paging.
    readonly property bool _scrollable: flick.contentHeight > flick.height
    // Round 10: fixed label-column width, sized once against the widest
    // of game_info.rs's known ordered tag labels (display_label()'s
    // fixed vocabulary), not a live per-open accumulator. The old
    // `_labelColumnWidth` grew across the whole modal's lifetime
    // (Component.onCompleted + onAdvanceWidthChanged per row) and never
    // shrank even after switching to a game with shorter tags, so the
    // column could stay needlessly wide. A handful of un-ordered
    // passthrough DB tags fall outside this known set and simply elide
    // (`Text.ElideRight` on the label below) rather than growing the
    // column further.
    readonly property var _knownTagLabels: ["System", "Platform", "Year", "Release date", "Genre", "Players", "Play mode", "Cooperative", "Developer", "Publisher", "Rating", "Filename"]
    readonly property int _labelColumnWidth: Sizing.px(root._knownTagLabels.reduce((widest, label) => Math.max(widest, tagLabelMetrics.advanceWidth(label)), 0) + Sizing.stroke(2))

    signal closeRequested

    visible: open
    enabled: visible
    anchors.fill: parent
    z: 300

    onOpenChanged: {
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

    // Fixed-weight measurement of the known tag-label vocabulary above --
    // FontMetrics + an invokable `.advanceWidth(text)` call is safe here
    // (unlike a per-row *live* weight, the round-8/9 pitfall documented in
    // ContextMenu.qml/ListPickerModal.qml) because every string in
    // `_knownTagLabels` is a fixed JS literal, not a property whose
    // changes need tracking -- there is nothing for the binding to miss.
    FontMetrics {
        id: tagLabelMetrics
        font.family: Theme.fontUi
        font.pixelSize: Sizing.fontCaption
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
            radius: Sizing.radiusMd

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

            // Header divider — matches SettingsSectionHeader's "a color
            // step survives every render tier" reasoning (docs/style.md
            // -> "Settings section headers"): the title now reads as a
            // proper header band boundary instead of floating text ahead
            // of a bare gap.
            Rectangle {
                id: titleDivider
                anchors.left: parent.left
                anchors.leftMargin: Sizing.pctW(4)
                anchors.right: parent.right
                anchors.rightMargin: Sizing.pctW(4)
                anchors.top: titleText.bottom
                anchors.topMargin: Sizing.pctH(1.5)
                height: Sizing.stroke(2)
                color: Theme.borderMid
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
                anchors.top: titleDivider.bottom
                anchors.topMargin: Sizing.pctH(4)
                text: qsTr("Could not load details. Check Zaparoo Core and try again.")
                color: Theme.textPrimary
                font.family: Theme.fontUi
                font.pixelSize: Sizing.fontBody
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
                anchors.top: titleDivider.bottom
                anchors.topMargin: Sizing.pctH(2)
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

                    // Cover/screenshot card — inset in a Theme.surfaceCard
                    // rounded rect matching the Tile/PagedGrid card
                    // language, instead of the art floating directly on
                    // the bare panel background.
                    Rectangle {
                        id: coverCard

                        width: parent.width
                        height: Browse.GameInfo.image_count > 0 ? Sizing.pctH(32) : 0
                        visible: height > 0
                        color: Theme.surfaceCard
                        radius: Sizing.radiusMd
                        border.color: Theme.borderMid
                        border.width: Sizing.cardBorderWidth

                        Item {
                            id: coverInner
                            anchors.fill: parent
                            anchors.margins: Sizing.pctH(1)

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
                                glyphSize: Sizing.fontCaption
                            }

                            Image {
                                source: Resources.iconUrl("NavLeft", Theme.textPrimary)
                                width: Sizing.pctH(4)
                                height: width
                                sourceSize.width: Sizing.px(width)
                                sourceSize.height: Sizing.px(height)
                                anchors.left: parent.left
                                anchors.verticalCenter: parent.verticalCenter
                                fillMode: Image.PreserveAspectFit
                                smooth: true
                                visible: Browse.GameInfo.image_count > 1 && Browse.GameInfo.image_can_prev
                            }

                            Image {
                                source: Resources.iconUrl("NavRight", Theme.textPrimary)
                                width: Sizing.pctH(4)
                                height: width
                                sourceSize.width: Sizing.px(width)
                                sourceSize.height: Sizing.px(height)
                                anchors.right: parent.right
                                anchors.verticalCenter: parent.verticalCenter
                                fillMode: Image.PreserveAspectFit
                                smooth: true
                                visible: Browse.GameInfo.image_count > 1 && Browse.GameInfo.image_can_next
                            }
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
                                required property int index

                                width: tagTable.width
                                height: Math.max(Sizing.pctH(3), tagValue.paintedHeight) + (tagRow.index > 0 ? Sizing.pctH(0.8) : 0)

                                readonly property list<string> parts: modelData.split("\t")
                                readonly property string label: parts.length > 0 ? parts[0] : ""
                                readonly property string value: parts.length > 1 ? parts[1] : ""

                                // Hairline row divider — every row but the
                                // first, so adjacent tags read as
                                // scannable rows instead of one undivided
                                // block.
                                Rectangle {
                                    visible: tagRow.index > 0
                                    anchors.left: parent.left
                                    anchors.right: parent.right
                                    anchors.top: parent.top
                                    height: Sizing.stroke(1)
                                    color: Theme.borderSubtle
                                }

                                readonly property int _rowTop: tagRow.index > 0 ? Sizing.pctH(0.8) : 0

                                Text {
                                    anchors.left: parent.left
                                    anchors.top: parent.top
                                    anchors.topMargin: tagRow._rowTop
                                    width: root._labelColumnWidth
                                    text: tagRow.label
                                    color: Theme.textLabel
                                    font.family: Theme.fontUi
                                    font.pixelSize: Sizing.fontCaption
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
                                    anchors.topMargin: tagRow._rowTop
                                    text: tagRow.value
                                    color: Theme.textPrimary
                                    font.family: Theme.fontUi
                                    font.pixelSize: Sizing.fontCaption
                                    wrapMode: Text.Wrap
                                    horizontalAlignment: Text.AlignLeft
                                    renderType: Text.NativeRendering
                                }
                            }
                        }
                    }

                    // Description band — its own section header (matching
                    // SettingsSectionHeader's band idiom, same module,
                    // no import needed) rather than just extra Column
                    // spacing, so it reads as a distinct block from the
                    // tag table above it.
                    SettingsSectionHeader {
                        width: parent.width
                        visible: Browse.GameInfo.description !== ""
                        label: qsTr("Description")
                    }

                    Text {
                        width: parent.width
                        visible: Browse.GameInfo.description !== ""
                        text: Browse.GameInfo.description
                        color: Theme.textPrimary
                        font.family: Theme.fontUi
                        font.pixelSize: Sizing.fontBody
                        wrapMode: Text.WordWrap
                        horizontalAlignment: Text.AlignLeft
                        renderType: Text.NativeRendering
                    }
                }
            }

            // Round 9: dims (Theme.textLabel) rather than hides when the
            // panel doesn't overflow in that direction, matching
            // PageIndicator.qml's treatment. Round 10: both now hide
            // entirely (not just their dim state) when `flick`'s content
            // doesn't overflow at all -- a description-only game with no
            // tag table might not fill the flickable, and two permanently
            // dim arrows pointing at nothing to scroll to said nothing
            // useful.
            Image {
                objectName: "gameInfoScrollUp"
                source: Resources.iconUrl("ScrollUp", root._hasContentAbove ? Theme.textPrimary : Theme.textLabel)
                width: Sizing.pctH(3)
                height: width
                sourceSize.width: Sizing.px(width)
                sourceSize.height: Sizing.px(height)
                anchors.bottom: flick.top
                anchors.bottomMargin: Sizing.pctH(0.5)
                anchors.horizontalCenter: flick.horizontalCenter
                fillMode: Image.PreserveAspectFit
                smooth: true
                visible: flick.visible && root._scrollable
            }

            Image {
                objectName: "gameInfoScrollDown"
                source: Resources.iconUrl("ScrollDown", root._hasContentBelow ? Theme.textPrimary : Theme.textLabel)
                width: Sizing.pctH(3)
                height: width
                sourceSize.width: Sizing.px(width)
                sourceSize.height: Sizing.px(height)
                anchors.top: flick.bottom
                anchors.topMargin: Sizing.pctH(0.5)
                anchors.horizontalCenter: flick.horizontalCenter
                fillMode: Image.PreserveAspectFit
                smooth: true
                visible: flick.visible && root._scrollable
            }
        }
    }
}
