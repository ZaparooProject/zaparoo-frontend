// Zaparoo Launcher
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

import QtQuick
import Zaparoo.Theme
import Zaparoo.Ui
import Zaparoo.Browse as Browse

// Shared list/grid screen for flat media-history style datasets such as
// Favorites and Recently Played. The caller supplies the model, persisted
// selection state, and user-facing copy; interaction, focused detail
// policy, and layout stay centralized here.
Item {
    id: root

    property var mediaModel: null
    property var mediaState: null
    property string screenTitle: ""
    property string emptyText: ""
    property string loadingText: ""
    property string detailPlaceholderKey: "icons/File"

    property alias mediaGrid: mediaGrid

    property bool transitioning: false
    property bool gridFocused: true
    property bool detailRapidScrollActive: false
    readonly property bool _listLayout: Browse.Settings.current_browse_layout === "list"
    readonly property int _listOverlayBottomMargin: Sizing.pctH(15)
    readonly property bool _gateHide: root.transitioning || root._loading()

    signal requestHubScreen
    signal requestContextMenu(int index, var anchorRect)

    function _count(): int {
        return root.mediaModel !== null ? root.mediaModel.count : 0;
    }

    function _loading(): bool {
        return root.mediaModel !== null ? root.mediaModel.loading : false;
    }

    function _errorMessage(): string {
        return root.mediaModel !== null ? (root.mediaModel.error_message ?? "") : "";
    }

    function _detailImageKey(): string {
        return root.mediaModel !== null ? (root.mediaModel.current_detail_image_key ?? "") : "";
    }

    function _detailTags(): string {
        return root.mediaModel !== null ? (root.mediaModel.current_detail_tags ?? "") : "";
    }

    function _detailLoading(): bool {
        return root.mediaModel !== null ? root.mediaModel.current_detail_loading : false;
    }

    function restoreSelection(): void {
        if (root._count() <= 0 || root.mediaState === null)
            return;
        const path = root.mediaState.selected_path ?? "";
        if (path === "")
            return;
        const idx = root.mediaModel.index_for_path(path);
        if (idx >= 0 && idx !== mediaGrid.currentIndex)
            mediaGrid.currentIndex = idx;
    }

    function _persistFocus(): void {
        if (root.mediaModel === null || root.mediaState === null)
            return;
        const idx = mediaGrid.currentIndex;
        if (idx < 0)
            return;
        const path = root.mediaModel.path_at(idx);
        if (path === "")
            return;
        root.mediaState.selected_path = path;
    }

    function _focusIndex(index: int): void {
        if (index < 0 || index >= mediaGrid.itemCount)
            return;
        mediaGrid.currentIndex = index;
        root._persistFocus();
    }

    function _performLinearMove(delta: int): void {
        const count = mediaGrid.itemCount;
        if (count <= 0)
            return;
        let next = mediaGrid.currentIndex + delta;
        if (next < 0)
            next = count - 1;
        else if (next >= count)
            next = 0;
        if (next === mediaGrid.currentIndex) {
            if (next >= count - 2)
                root.mediaModel.fetch_more();
            return;
        }
        mediaGrid.currentIndex = next;
        root._persistFocus();
        if (next >= count - 2)
            root.mediaModel.fetch_more();
    }

    function _performPage(delta: int): void {
        if (root._listLayout) {
            root._performLinearMove(delta * mediaGrid.pageSize);
            return;
        }
        mediaGrid.pageBy(delta);
    }

    function _state(): string {
        if (root._loading())
            return "loading";
        if (root._errorMessage() !== "")
            return "error";
        if (root._count() === 0)
            return "empty";
        return "ready";
    }

    function handleAction(action: string): void {
        if (action === "left") {
            if (!root._listLayout)
                mediaGrid.moveSelection(-1, 0);
        } else if (action === "right") {
            if (!root._listLayout)
                mediaGrid.moveSelection(1, 0);
        } else if (action === "up") {
            if (root._listLayout)
                root._performLinearMove(-1);
            else
                mediaGrid.moveSelection(0, -1);
        } else if (action === "down") {
            if (root._listLayout)
                root._performLinearMove(1);
            else
                mediaGrid.moveSelection(0, 1);
        } else if (action === "page_prev") {
            if (root._state() === "ready")
                root._performPage(-1);
        } else if (action === "page_next") {
            if (root._state() === "ready")
                root._performPage(1);
        } else if (action === "accept") {
            const state = root._state();
            if (state === "loading")
                return;
            if (state === "error" || state === "empty") {
                root.mediaModel.fetch_more();
                return;
            }
            root.mediaModel.launch_at(mediaGrid.currentIndex);
        } else if (action === "write_card") {
            if (mediaGrid.itemCount > 0) {
                const idx = mediaGrid.currentIndex;
                root._persistFocus();
                const rect = root._listLayout ? listCard.currentCellRectIn(root) : mediaGrid.currentCellRectIn(root);
                root.requestContextMenu(idx, rect);
            }
        } else if (action === "cancel") {
            root.requestHubScreen();
        }
    }

    FocusedMediaDetailController {
        id: focusedDetail

        enabled: !root._gateHide && root._listLayout
        itemCount: mediaGrid.itemCount
        currentIndex: mediaGrid.currentIndex
        rapidScrollActive: root.detailRapidScrollActive
        mediaModel: root.mediaModel
    }

    TopStatusStrip {
        id: topStrip
        visible: !root._gateHide
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: parent.top
        anchors.topMargin: Sizing.headerBottom + Sizing.pctH(1)
        height: Sizing.pctH(7)
        title: root.screenTitle
        currentPage: mediaGrid.currentPage
        totalPages: Math.max(1, Math.ceil(root._count() / mediaGrid.pageSize))
        totalText: root._listLayout ? "" : (root._count() > 0 ? qsTr("%1 entries").arg(root._count()) : "")
        rightTextOverride: {
            if (!root._listLayout || mediaGrid.itemCount <= 0)
                return "";
            return qsTr("%1 / %2").arg(mediaGrid.currentIndex + 1).arg(Math.max(1, root._count()));
        }
    }

    BrowseListDetailView {
        id: listCard

        visible: !root._gateHide && root._listLayout
        anchors.left: parent.left
        anchors.leftMargin: Sizing.pctW(5)
        anchors.right: parent.right
        anchors.rightMargin: Sizing.pctW(5)
        anchors.top: topStrip.bottom
        anchors.topMargin: Sizing.pctH(2)
        anchors.bottom: parent.bottom
        anchors.bottomMargin: Sizing.pctH(8)
        model: root.mediaModel
        currentIndex: mediaGrid.currentIndex
        detailTitle: listCard.currentName
        detailCoverKey: root.detailRapidScrollActive ? root.detailPlaceholderKey : (root._detailImageKey() !== "" ? root._detailImageKey() : listCard.currentCoverKey)
        detailTags: root._detailTags()
        detailLoading: root._detailLoading()
        detailSuppressed: root.detailRapidScrollActive
        onItemHovered: index => root._focusIndex(index)
        onItemClicked: index => {
            root._focusIndex(index);
            root.handleAction("accept");
        }
        onItemRightClicked: index => {
            root._focusIndex(index);
            root.handleAction("write_card");
        }
        onEmptyRightClicked: root.handleAction("cancel")
        onPageWheelRequested: delta => root.handleAction(delta > 0 ? "page_next" : "page_prev")
    }

    PagedGrid {
        id: mediaGrid

        visible: !root._gateHide && !root._listLayout
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: topStrip.bottom
        anchors.bottom: parent.bottom
        anchors.bottomMargin: Sizing.pctH(15)
        focused: root.gridFocused
        model: root.mediaModel
        delegate: Tile {
            showCaption: true
        }
        columnsOverride: Sizing.gamesGridColumns
        rowsOverride: Sizing.gamesGridRows
        onLoadMoreRequested: root.mediaModel.fetch_more()
        onCurrentIndexChanged: root._persistFocus()
        onItemHovered: index => root._focusIndex(index)
        onItemClicked: index => {
            root._focusIndex(index);
            root.handleAction("accept");
        }
        onItemRightClicked: index => {
            root._focusIndex(index);
            root.handleAction("write_card");
        }
        onEmptyRightClicked: root.handleAction("cancel")
        onPageWheelRequested: delta => root.handleAction(delta > 0 ? "page_next" : "page_prev")
    }

    ActiveLabel {
        id: activeLabel
        visible: !root._gateHide && !root._listLayout
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: mediaGrid.bottom
        height: Sizing.pctH(7)
        text: mediaGrid.itemCount > 0 ? root.mediaModel.name_at(mediaGrid.currentIndex) : ""
    }

    ScreenStateOverlay {
        x: root._listLayout ? 0 : mediaGrid.x
        y: root._listLayout ? listCard.y : mediaGrid.y
        width: root._listLayout ? root.width : mediaGrid.width
        height: root._listLayout ? Math.max(0, root.height - listCard.y - root._listOverlayBottomMargin) : mediaGrid.height
        loading: root._loading()
        errorMessage: root._errorMessage()
        count: root._count()
        emptyText: root.emptyText
        loadingText: root.loadingText
    }
}
