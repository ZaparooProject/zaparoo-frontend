// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import Zaparoo.Theme

Item {
    id: root

    required property var model
    property int currentIndex: 0
    property string currentName: ""
    property string currentCoverKey: ""
    property int totalItemsOverride: -1
    property int targetVisibleRowCount: 0
    property bool showFileStem: false
    property bool showChrome: true
    readonly property int itemCount: listView.count
    readonly property int totalItems: totalItemsOverride >= 0 ? totalItemsOverride : itemCount
    readonly property int cardPaddingX: Sizing.pctW(2)
    readonly property int cardPaddingY: Sizing.pctH(2)
    readonly property int rowSpacing: Sizing.pctH(0.7)
    readonly property int contentHeight: Math.max(0, height - (2 * cardPaddingY))
    readonly property int rowHeight: targetVisibleRowCount > 0 ? Math.max(Sizing.pctH(3), Math.floor((contentHeight - (rowSpacing * (targetVisibleRowCount - 1))) / targetVisibleRowCount)) : Sizing.pctH(6)
    readonly property int rowStride: rowHeight + rowSpacing
    readonly property int visibleRowCount: targetVisibleRowCount > 0 ? targetVisibleRowCount : Math.max(1, Math.floor((contentHeight + rowSpacing) / rowStride))
    readonly property int _centerSlot: Math.max(0, Math.floor((visibleRowCount - 1) / 2))
    readonly property int _maxViewTopIndex: Math.max(0, itemCount - visibleRowCount)
    readonly property int _viewTopIndex: Math.max(0, Math.min(_maxViewTopIndex, currentIndex - _centerSlot))
    readonly property int _targetContentY: _viewTopIndex * rowStride
    readonly property int _maxScrollTopIndex: Math.max(0, totalItems - visibleRowCount)
    readonly property int _gutterWidth: Sizing.pctW(3)
    readonly property int _gutterGap: Sizing.pctW(1.5)

    signal itemHovered(int index)
    signal itemClicked(int index)
    signal itemRightClicked(int index)
    signal emptyRightClicked
    signal pageWheelRequested(int delta)

    function _handleWheel(wheel: WheelEvent): void {
        const amount = wheel.angleDelta.y !== 0 ? wheel.angleDelta.y : wheel.pixelDelta.y;
        if (amount === 0)
            return;
        root.pageWheelRequested(amount < 0 ? 1 : -1);
        wheel.accepted = true;
    }

    function currentCellRectIn(target: Item): rect {
        if (root.itemCount <= 0)
            return Qt.rect(0, 0, 0, 0);
        const item = listView.currentItem;
        if (item === null)
            return Qt.rect(0, 0, 0, 0);
        const p = listView.mapToItem(target, 0, item.y - listView.contentY);
        return Qt.rect(p.x, p.y, listView.width, root.rowHeight);
    }

    clip: true

    Rectangle {
        anchors.fill: parent
        color: Theme.surfaceCard
        border.width: Sizing.stroke(1)
        border.color: Theme.borderMid
        radius: Sizing.cornerRadius
        visible: root.showChrome
    }

    onItemCountChanged: {
        if (root.itemCount === 0) {
            root.currentName = "";
            root.currentCoverKey = "";
        }
    }

    MouseArea {
        anchors.fill: parent
        acceptedButtons: Qt.RightButton
        onClicked: root.emptyRightClicked()
        onWheel: wheel => root._handleWheel(wheel)
    }

    ListView {
        id: listView

        anchors.left: parent.left
        anchors.leftMargin: root.cardPaddingX
        anchors.top: parent.top
        anchors.topMargin: root.cardPaddingY
        anchors.bottom: parent.bottom
        anchors.bottomMargin: root.cardPaddingY
        anchors.right: parent.right
        anchors.rightMargin: root.totalItems > root.visibleRowCount ? root._gutterWidth + root._gutterGap + root.cardPaddingX : root.cardPaddingX
        model: root.model
        currentIndex: root.currentIndex
        contentY: Math.min(root._targetContentY, Math.max(0, contentHeight - height))
        boundsBehavior: Flickable.StopAtBounds
        interactive: false
        spacing: root.rowSpacing
        highlightFollowsCurrentItem: false

        delegate: Item {
            id: row

            required property int index
            required property string name
            required property string fileStem
            required property string coverKey
            required property int favorite

            width: listView.width
            height: root.rowHeight

            readonly property bool selected: row.index === root.currentIndex

            Binding {
                target: root
                property: "currentName"
                when: row.selected
                value: row.name
                restoreMode: Binding.RestoreNone
            }

            Binding {
                target: root
                property: "currentCoverKey"
                when: row.selected
                value: row.coverKey
                restoreMode: Binding.RestoreNone
            }

            Item {
                anchors.fill: parent
                visible: row.selected

                Rectangle {
                    anchors.fill: parent
                    color: Theme.selectionSurface
                    radius: Sizing.cornerRadius
                }

                Rectangle {
                    anchors.left: parent.left
                    anchors.top: parent.top
                    anchors.bottom: parent.bottom
                    width: Sizing.cornerRadius
                    color: Theme.selectionSurface
                }
            }

            Rectangle {
                anchors.left: parent.left
                anchors.top: parent.top
                anchors.bottom: parent.bottom
                width: Sizing.pctW(0.45)
                color: Theme.accent
                visible: row.selected
                radius: Math.max(0, Sizing.px(width / 3))
            }

            Text {
                anchors.left: parent.left
                anchors.leftMargin: Sizing.pctW(1.6)
                anchors.right: parent.right
                anchors.rightMargin: row.favorite !== 0 ? Sizing.pctW(5.2) : Sizing.pctW(1.6)
                anchors.verticalCenter: parent.verticalCenter
                text: root.showFileStem && row.fileStem !== "" ? row.fileStem : row.name
                color: row.selected ? Theme.textPrimary : Theme.textLabel
                font.family: Theme.fontUi
                font.pixelSize: Sizing.fontSize(2.9)
                elide: Text.ElideRight
                verticalAlignment: Text.AlignVCenter
                renderType: Text.NativeRendering
            }

            Image {
                anchors.right: parent.right
                anchors.rightMargin: Sizing.pctW(1.6)
                anchors.verticalCenter: parent.verticalCenter
                width: Sizing.pctH(3.2)
                height: width
                source: Resources.iconUrl("Heart")
                sourceSize.width: Sizing.px(width)
                sourceSize.height: Sizing.px(height)
                fillMode: Image.PreserveAspectFit
                smooth: true
                asynchronous: false
                visible: row.favorite !== 0
            }

            MouseArea {
                anchors.fill: parent
                hoverEnabled: true
                acceptedButtons: Qt.LeftButton | Qt.RightButton
                cursorShape: Qt.PointingHandCursor

                onEntered: root.itemHovered(row.index)
                onClicked: mouse => {
                    if (mouse.button === Qt.RightButton)
                        root.itemRightClicked(row.index);
                    else
                        root.itemClicked(row.index);
                }
                onWheel: wheel => root._handleWheel(wheel)
            }
        }
    }

    // ── Left-half scroll indicator ────────────────────────────────────────
    Item {
        id: scrollGutter

        anchors.right: parent.right
        anchors.rightMargin: root.cardPaddingX
        anchors.top: parent.top
        anchors.topMargin: root.cardPaddingY
        anchors.bottom: parent.bottom
        anchors.bottomMargin: root.cardPaddingY
        width: root._gutterWidth
        visible: root.totalItems > root.visibleRowCount

        readonly property int arrowSize: Math.min(width, Sizing.pctH(4))

        Image {
            id: upArrow
            source: Resources.iconUrl("ScrollUp")
            width: scrollGutter.arrowSize
            height: scrollGutter.arrowSize
            anchors.top: parent.top
            anchors.horizontalCenter: parent.horizontalCenter
            fillMode: Image.PreserveAspectFit
            smooth: true
            visible: root.currentIndex > 0

            MouseArea {
                anchors.fill: parent
                acceptedButtons: Qt.LeftButton
                cursorShape: Qt.PointingHandCursor
                enabled: upArrow.visible
                onClicked: root.pageWheelRequested(-1)
            }
        }

        Image {
            id: downArrow
            source: Resources.iconUrl("ScrollDown")
            width: scrollGutter.arrowSize
            height: scrollGutter.arrowSize
            anchors.bottom: parent.bottom
            anchors.horizontalCenter: parent.horizontalCenter
            fillMode: Image.PreserveAspectFit
            smooth: true
            visible: root.currentIndex < root.totalItems - 1

            MouseArea {
                anchors.fill: parent
                acceptedButtons: Qt.LeftButton
                cursorShape: Qt.PointingHandCursor
                enabled: downArrow.visible
                onClicked: root.pageWheelRequested(1)
            }
        }

        Item {
            id: scrollRegion
            anchors.top: parent.top
            anchors.topMargin: scrollGutter.arrowSize + Sizing.pctH(1)
            anchors.bottom: parent.bottom
            anchors.bottomMargin: scrollGutter.arrowSize + Sizing.pctH(1)
            anchors.horizontalCenter: parent.horizontalCenter

            readonly property int _minThumbHeight: Sizing.pctH(4)
            readonly property int _thumbHeight: root.totalItems <= 0 ? 0 : Math.min(scrollRegion.height, Math.max(_minThumbHeight, Math.round(scrollRegion.height * root.visibleRowCount / root.totalItems)))
            readonly property int _thumbY: root._maxScrollTopIndex <= 0 ? 0 : Sizing.px((root._viewTopIndex / root._maxScrollTopIndex) * (scrollRegion.height - _thumbHeight))

            Rectangle {
                id: scrollThumb
                width: Sizing.pctW(1.2)
                height: scrollRegion._thumbHeight
                anchors.horizontalCenter: parent.horizontalCenter
                y: scrollRegion._thumbY
                color: Theme.textPrimary
                radius: Sizing.half(width)
            }
        }
    }
}
