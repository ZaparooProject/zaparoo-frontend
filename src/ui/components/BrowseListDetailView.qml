// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

import QtQuick
import Zaparoo.Theme

Item {
    id: root

    property alias model: browseList.model
    property alias currentIndex: browseList.currentIndex
    property alias totalItemsOverride: browseList.totalItemsOverride
    property alias targetVisibleRowCount: browseList.targetVisibleRowCount
    property alias showFileStem: browseList.showFileStem
    property alias currentName: browseList.currentName
    property alias currentCoverKey: browseList.currentCoverKey
    property alias itemCount: browseList.itemCount
    property alias visibleRowCount: browseList.visibleRowCount
    property var layoutProfile: null

    property alias detailTitle: detailPane.title
    property alias detailCoverKey: detailPane.coverKey
    property alias detailDescription: detailPane.description
    property alias detailShowDescription: detailPane.showDescription
    property alias detailShowTitle: detailPane.showTitle
    property alias detailTags: detailPane.detailTags
    property alias detailLoading: detailPane.loading
    property alias detailLoadingText: detailPane.loadingText
    property alias detailSuppressed: detailPane.detailSuppressed
    property alias detailCanPreviousImage: detailPane.canPreviousImage
    property alias detailCanNextImage: detailPane.canNextImage
    readonly property int _cardRadius: root.layoutProfile ? root.layoutProfile.tileCornerRadius : Sizing.cornerRadius
    readonly property int _dividerOffsetX: root.layoutProfile && root.layoutProfile.listDividerOffsetX !== undefined ? root.layoutProfile.listDividerOffsetX : 0

    signal itemHovered(int index)
    signal itemClicked(int index)
    signal itemRightClicked(int index)
    signal emptyRightClicked
    signal pageWheelRequested(int delta)

    function currentCellRectIn(target: Item): rect {
        return browseList.currentCellRectIn(target);
    }

    Rectangle {
        anchors.fill: parent
        color: Theme.surfaceCard
        border.width: Sizing.stroke(1)
        border.color: Theme.borderMid
        radius: root._cardRadius
    }

    CardDivider {
        id: listDivider

        x: Sizing.px(parent.width * 2 / 3) + root._dividerOffsetX
        anchors.top: parent.top
        anchors.bottom: parent.bottom
    }

    BrowseList {
        id: browseList

        anchors.left: parent.left
        anchors.right: listDivider.left
        anchors.top: parent.top
        anchors.bottom: parent.bottom
        layoutProfile: root.layoutProfile
        showChrome: false
        onItemHovered: index => root.itemHovered(index)
        onItemClicked: index => root.itemClicked(index)
        onItemRightClicked: index => root.itemRightClicked(index)
        onEmptyRightClicked: root.emptyRightClicked()
        onPageWheelRequested: delta => root.pageWheelRequested(delta)
    }

    BrowseDetailPane {
        id: detailPane

        anchors.left: listDivider.right
        anchors.right: parent.right
        anchors.top: parent.top
        anchors.bottom: parent.bottom
        layoutProfile: root.layoutProfile
        showChrome: false
    }
}
