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
    // Gates whether the selected row paints its highlight (selection surface +
    // accent bar + bright text). The host leaves it false until the screen's
    // selection is finalized (restore or first input) so the default row 0
    // never lights up during the window before restore points currentIndex at
    // the saved item on a cold start. Default true so unwired hosts highlight
    // the selection normally.
    property bool focusReady: true
    property string currentName: ""
    property string currentCoverKey: ""
    property int totalItemsOverride: -1
    property int targetVisibleRowCount: 0
    property bool showChrome: true
    property var layoutProfile: null
    // layoutProfile and its sub-objects (_list, _grid, _surface) are JS-object
    // vars; the QML compiler cannot statically type their properties. Suppress
    // the compiler category for these bindings only.
    // qmllint disable compiler
    readonly property var _list: root.layoutProfile && root.layoutProfile.list ? root.layoutProfile.list : null
    readonly property var _grid: root.layoutProfile && root.layoutProfile.grid ? root.layoutProfile.grid : null
    readonly property var _surface: root.layoutProfile && root.layoutProfile.surface ? root.layoutProfile.surface : null
    readonly property int itemCount: listView.count
    readonly property int totalItems: totalItemsOverride >= 0 ? totalItemsOverride : itemCount
    readonly property bool _portraitNonCrt: !Theme.crtNativePath && Sizing.screenWidth < Sizing.screenHeight
    readonly property int _cardRadius: root._surface ? root._surface.cardRadius : Sizing.radiusMd
    readonly property int _selectionRadius: root._surface ? root._surface.rowRadius : Sizing.radiusSm
    readonly property int cardPaddingLeft: root._list ? root._list.cardPaddingLeft : Sizing.pctW(2)
    readonly property int cardPaddingRight: root._list ? root._list.cardPaddingRight : Sizing.pctW(2)
    readonly property int cardPaddingTop: root._list ? root._list.cardPaddingTop : Sizing.pctH(2)
    readonly property int cardPaddingBottom: root._list ? root._list.cardPaddingBottom : Sizing.pctH(2)
    readonly property int rowSpacing: root._list ? root._list.rowSpacing : (root._portraitNonCrt ? Sizing.pctH(0.3) : Sizing.pctH(0.7))
    readonly property int contentHeight: Math.max(0, height - cardPaddingTop - cardPaddingBottom)
    readonly property int rowHeight: root._list && root._list.rowHeight > 0 ? root._list.rowHeight : (targetVisibleRowCount > 0 ? Math.max(Sizing.pctH(3), Math.floor((contentHeight - (rowSpacing * (targetVisibleRowCount - 1))) / targetVisibleRowCount)) : Sizing.pctH(6))
    readonly property int rowStride: rowHeight + rowSpacing
    readonly property int visibleRowCount: targetVisibleRowCount > 0 ? targetVisibleRowCount : Math.max(1, Math.floor((contentHeight + rowSpacing) / rowStride))
    readonly property int _centerSlot: root._list && root._list.centerSlot >= 0 ? Math.max(0, Math.min(visibleRowCount - 1, root._list.centerSlot)) : Math.max(0, Math.floor((visibleRowCount - 1) / 2))
    readonly property int _maxViewTopIndex: Math.max(0, itemCount - visibleRowCount)
    readonly property int _viewTopIndex: Math.max(0, Math.min(_maxViewTopIndex, currentIndex - _centerSlot))
    readonly property int _targetContentY: _viewTopIndex * rowStride
    readonly property int _rowTextLeftPadding: root._list ? root._list.rowTextLeftPadding : Sizing.pctW(1.6)
    readonly property int _rowTextRightPadding: root._list ? root._list.rowTextRightPadding : Sizing.pctW(1.6)
    readonly property int _favoriteRightPadding: root._list ? root._list.favoriteRightPadding : Sizing.pctW(1.6)
    // qmllint enable compiler

    // Pulse counter for the selected row's inverse-video flash. Callers
    // increment via activatePulse; only the selected row's bar/content colors
    // swap. Forward navigation and game launch share this single cue.
    property int activatePulse: 0
    // Cuts a held flash short. Incremented by the host after a launch that
    // keeps the frontend on the same screen. Forward navigation never
    // increments it; screenSettling cuts the flash off-screen.
    property int releasePulse: 0
    // When true, cuts a held flash short so it does not persist when the
    // screen is shown again. Set by the host to !active while the screen is
    // off-screen.
    property bool screenSettling: false

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

    // Corner radius of the rect `currentCellRectIn()` returns, for
    // ContextMenu's rounded scrim hole (Part 5).
    readonly property int currentCellRadius: root._selectionRadius

    function currentCellRectIn(target: Item): rect {
        if (root.itemCount <= 0)
            return Qt.rect(0, 0, 0, 0);
        const item = listView.currentItem;
        if (item === null)
            return Qt.rect(0, 0, 0, 0);
        const p = listView.mapToItem(target, 0, item.y - listView.contentY);
        return Qt.rect(p.x, p.y, listView.width, root.rowHeight);
    }

    function _syncContentY(): void {
        const maxY = Math.max(0, listView.contentHeight - listView.height);
        const targetY = Math.min(root._targetContentY, maxY);
        if (listView.contentY !== targetY)
            listView.contentY = targetY;
    }

    clip: true

    Rectangle {
        anchors.fill: parent
        color: Theme.surfaceCard
        border.width: Sizing.cardBorderWidth
        border.color: Theme.borderMid
        radius: root._cardRadius
        visible: root.showChrome
    }

    onItemCountChanged: {
        if (root.itemCount === 0) {
            root.currentName = "";
            root.currentCoverKey = "";
        }
        root._syncContentY();
    }
    on_TargetContentYChanged: root._syncContentY()

    MouseArea {
        anchors.fill: parent
        acceptedButtons: Qt.RightButton
        onClicked: root.emptyRightClicked()
        onWheel: wheel => root._handleWheel(wheel)
    }

    ListView {
        id: listView

        anchors.left: parent.left
        anchors.leftMargin: root.cardPaddingLeft
        anchors.top: parent.top
        anchors.topMargin: root.cardPaddingTop
        anchors.bottom: parent.bottom
        anchors.bottomMargin: root.cardPaddingBottom
        anchors.right: parent.right
        anchors.rightMargin: root.cardPaddingRight
        model: root.model
        currentIndex: root.currentIndex
        boundsBehavior: Flickable.StopAtBounds
        interactive: false
        spacing: root.rowSpacing
        highlightFollowsCurrentItem: false
        Component.onCompleted: root._syncContentY()
        onContentHeightChanged: root._syncContentY()
        onHeightChanged: root._syncContentY()

        delegate: Item {
            id: row

            objectName: "browseListRow-" + row.index
            required property int index
            required property string name
            required property string fileStem
            required property string coverKey
            required property int favorite
            // Newline-joined disambiguating-tag tokens (empty for models
            // without variants). Every Browse model exposes this role. For
            // a folder/root row this instead carries the round-11 roots-
            // screen distinguisher (see games.rs's `root_distinguishers`),
            // if any -- `_tagsSuffix` below folds it together with
            // `fileCount` via `Format.rowSuffix`.
            required property string disambiguatingTags
            // Round 11. Required, same as `disambiguatingTags` above -- a
            // plain non-required property does NOT automatically bind to a
            // matching model role (Qt only wires that up for `required
            // property`), so every model reaching this delegate sets both
            // explicitly.
            required property string entryType
            required property int fileCount

            width: listView.width
            height: root.rowHeight

            readonly property bool selected: row.index === root.currentIndex
            // Visual highlight is withheld until the host marks focus ready, so
            // the default row 0 never paints the accent before restore lands.
            // `selected` itself stays ungated so the detail-pane bindings below
            // still track content during the pre-restore window.
            readonly property bool _highlightVisible: row.selected && root.focusReady
            readonly property string _baseTitle: row.name !== "" ? row.name : row.fileStem
            readonly property string _tagsSuffix: Format.rowSuffix(row.entryType, row.disambiguatingTags, row.fileCount)
            // Horizontal space reserved on the right for the favorite heart.
            readonly property int _favoriteSlot: row.favorite !== 0 ? root._favoriteRightPadding + Sizing.pctH(3.2) : 0

            Binding {
                target: root
                property: "currentName"
                when: row.selected
                value: row._baseTitle
                restoreMode: Binding.RestoreNone
            }

            Binding {
                target: root
                property: "currentCoverKey"
                when: row.selected
                value: row.coverKey
                restoreMode: Binding.RestoreNone
            }

            // Inverse-video highlight. See SelectionBar.qml -- selection is a
            // line of text swapping foreground and background, not an object
            // lifting off the page.
            SelectionBar {
                id: bar
                objectName: "selectionBar"
                anchors.fill: parent
                active: row._highlightVisible
                activatePulse: root.activatePulse
                releasePulse: root.releasePulse
                screenSettling: root.screenSettling
                radius: root._selectionRadius
            }

            // Row title carrying the inline dim token suffix. ScrollingCaption
            // left-aligns and elides it, pins the top token after the name
            // elides, and marquees the full string while this row is the
            // selection (reduce-motion falls back to a static elide). The right
            // margin reserves the favorite-heart slot.
            ScrollingCaption {
                anchors.left: parent.left
                anchors.leftMargin: root._rowTextLeftPadding
                anchors.right: parent.right
                anchors.rightMargin: row._favoriteSlot + root._rowTextRightPadding
                anchors.verticalCenter: parent.verticalCenter
                height: parent.height
                name: row._baseTitle
                tags: row._tagsSuffix
                focused: row._highlightVisible
                centerContent: false
                fontPixelSize: Sizing.fontSection
                fontWeight: bar.contentWeight
                nameColor: row._highlightVisible ? bar.contentColor : Theme.textLabel
                variantColor: row._highlightVisible ? Theme.onAccentMuted : Theme.textVariant
            }

            Image {
                anchors.right: parent.right
                anchors.rightMargin: root._favoriteRightPadding
                anchors.verticalCenter: parent.verticalCenter
                width: Sizing.pctH(3.2)
                height: width
                source: row._highlightVisible ? Resources.coverUrl("icons/Heart", Theme.onAccent, Theme.onAccent, Theme.selectionFill) : Resources.coverUrl("icons/Heart", Theme.marker, Theme.marker, Theme.markerOutline)
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
}
