// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

import QtQuick
import Zaparoo.Theme
import Zaparoo.Ui
import Zaparoo.Browse as Browse

// Shared media-list screen shell. The caller supplies the model,
// persisted selection state, and user-facing copy; interaction,
// focused-detail policy, and list/detail layout stay centralized here.
//
// Favorites and Recently Played use this directly today. Games can grow
// on top of the same shell by layering in folder navigation, richer
// pagination/status rules, and image-cycling behavior without forking
// the common list/detail mechanics again.
Item {
    id: root

    property var mediaModel: null
    property var mediaState: null
    property string screenTitle: ""
    property string emptyText: ""
    property string loadingText: ""
    property string detailPlaceholderKey: "icons/File"
    property int totalItemsOverride: -1
    property int targetVisibleRowCount: 0
    property bool showFileStem: false
    property bool detailShowDescription: true
    property bool detailShowTitle: true
    property string detailLoadingText: qsTr("Loading…")
    property bool detailCanPreviousImage: false
    property bool detailCanNextImage: false
    property var detailIdentityForIndex: null
    property var loadDetailForIndex: null
    property var clearDetailAction: null
    property var retryAction: null
    property var acceptAction: null
    property var cancelAction: null
    property var gridMoveAction: null
    property var linearMoveAction: null
    property var pageAction: null
    property var onListLayoutEntered: null
    property var listLeftAction: null
    property var listRightAction: null
    property var contextMenuEnabledAt: null
    property var restoreSelectionPath: null
    property var persistSelectionPath: null
    property var topStripTitleProvider: null
    property var topStripCurrentPageProvider: null
    property var topStripTotalPagesProvider: null
    property var topStripTotalTextProvider: null
    property var topStripRightTextProvider: null
    property var activeLabelTextProvider: null
    property var gridCurrentPageChangedAction: null
    property var gridCurrentIndexChangedAction: null
    property var gridLoadMoreAction: null

    property alias mediaGrid: mediaGrid
    property alias topStrip: topStrip
    property alias listCard: listCard
    property alias activeLabel: activeLabel

    property bool transitioning: false
    property bool gridFocused: true
    property bool detailRapidScrollActive: false
    property bool forceListLayout: false
    property bool renderGridLayout: true
    property bool showTopStrip: true
    property bool showBottomStatusRow: false
    property bool showHeaderTitleInHeader: false
    property bool activeLabelAtBottom: false
    property int gridBottomMargin: Sizing.pctH(15)
    property int activeLabelBottomMargin: 0
    property int activeLabelHeight: Sizing.pctH(7)
    property int bottomStatusLeftMargin: 0
    property int bottomStatusRightMargin: 0
    property int pageLoadingLeftMargin: 0
    property bool pageLoadingVisible: false
    property string bottomStatusLeftText: ""
    property string bottomStatusRightText: ""
    property var gridLayoutProfile: null
    property int gridTotalItemsOverride: -1
    property bool gridHasMorePages: false
    readonly property bool _listLayout: root.forceListLayout || Browse.Settings.current_browse_layout === "list"
    readonly property bool _crtListStrip: Theme.crtNativePath && root._listLayout
    readonly property var _listLayoutProfile: Theme.crtNativePath ? BrowseLayouts.crtTile : BrowseLayouts.defaultTile
    readonly property int _listOverlayBottomMargin: Sizing.pctH(15)
    readonly property bool _gateHide: root.transitioning || root._loading()

    signal requestHubScreen
    signal requestContextMenu(int index, var anchorRect)

    on_ListLayoutChanged: {
        if (!root._listLayout)
            return;
        if (typeof root.onListLayoutEntered === "function")
            root.onListLayoutEntered();
        focusedDetail.requestNow();
    }

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
        if (root._count() <= 0)
            return;
        const path = typeof root.restoreSelectionPath === "function" ? (root.restoreSelectionPath() ?? "") : (root.mediaState !== null ? (root.mediaState.selected_path ?? "") : "");
        if (path === "")
            return;
        const idx = root.mediaModel.index_for_path(path);
        if (idx >= 0 && idx !== mediaGrid.currentIndex)
            mediaGrid.currentIndex = idx;
    }

    function _persistFocus(): void {
        if (root.mediaModel === null)
            return;
        const idx = mediaGrid.currentIndex;
        if (idx < 0)
            return;
        const path = root.mediaModel.path_at(idx);
        if (path === "")
            return;
        if (typeof root.persistSelectionPath === "function")
            root.persistSelectionPath(path);
        else if (root.mediaState !== null)
            root.mediaState.selected_path = path;
    }

    function _focusIndex(index: int): void {
        if (index < 0 || index >= mediaGrid.itemCount)
            return;
        mediaGrid.currentIndex = index;
        root._persistFocus();
    }

    function _performLinearMove(delta: int): void {
        if (typeof root.linearMoveAction === "function") {
            root.linearMoveAction(delta);
            return;
        }
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
        if (typeof root.pageAction === "function") {
            root.pageAction(delta);
            return;
        }
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
            if (root._listLayout && typeof root.listLeftAction === "function")
                root.listLeftAction();
            else if (!root._listLayout && typeof root.gridMoveAction === "function")
                root.gridMoveAction(-1, 0);
            else if (!root._listLayout)
                mediaGrid.moveSelection(-1, 0);
        } else if (action === "right") {
            if (root._listLayout && typeof root.listRightAction === "function")
                root.listRightAction();
            else if (!root._listLayout && typeof root.gridMoveAction === "function")
                root.gridMoveAction(1, 0);
            else if (!root._listLayout)
                mediaGrid.moveSelection(1, 0);
        } else if (action === "up") {
            if (root._listLayout)
                root._performLinearMove(-1);
            else if (typeof root.gridMoveAction === "function")
                root.gridMoveAction(0, -1);
            else
                mediaGrid.moveSelection(0, -1);
        } else if (action === "down") {
            if (root._listLayout)
                root._performLinearMove(1);
            else if (typeof root.gridMoveAction === "function")
                root.gridMoveAction(0, 1);
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
                if (typeof root.retryAction === "function")
                    root.retryAction();
                else
                    root.mediaModel.fetch_more();
                return;
            }
            if (typeof root.acceptAction === "function")
                root.acceptAction(mediaGrid.currentIndex);
            else
                root.mediaModel.launch_at(mediaGrid.currentIndex);
        } else if (action === "write_card") {
            if (mediaGrid.itemCount > 0) {
                const idx = mediaGrid.currentIndex;
                if (typeof root.contextMenuEnabledAt === "function" && !root.contextMenuEnabledAt(idx))
                    return;
                root._persistFocus();
                const rect = root._listLayout ? listCard.currentCellRectIn(root) : mediaGrid.currentCellRectIn(root);
                root.requestContextMenu(idx, rect);
            }
        } else if (action === "cancel") {
            if (typeof root.cancelAction === "function")
                root.cancelAction();
            else
                root.requestHubScreen();
        }
    }

    FocusedMediaDetailController {
        id: focusedDetail

        enabled: !root._gateHide && root._listLayout
        itemCount: mediaGrid.itemCount
        currentIndex: mediaGrid.currentIndex
        rapidScrollActive: root.detailRapidScrollActive
        identityForIndex: root.detailIdentityForIndex
        loadForIndex: root.loadDetailForIndex
        clearDetail: root.clearDetailAction
        mediaModel: root.mediaModel
    }

    TopStatusStrip {
        id: topStrip
        visible: !root._gateHide && (root.showTopStrip || root._crtListStrip)
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: parent.top
        anchors.topMargin: Sizing.headerBottom + Sizing.pctH(1)
        height: root._crtListStrip ? root._listLayoutProfile.listStripHeight : (root.showTopStrip ? Sizing.pctH(7) : 0)
        slotMargin: root._crtListStrip ? root._listLayoutProfile.listStripSlotMargin : Sizing.pctW(5)
        title: typeof root.topStripTitleProvider === "function" ? root.topStripTitleProvider() : root.screenTitle
        currentPage: typeof root.topStripCurrentPageProvider === "function" ? root.topStripCurrentPageProvider() : mediaGrid.currentPage
        totalPages: typeof root.topStripTotalPagesProvider === "function" ? root.topStripTotalPagesProvider() : Math.max(1, Math.ceil(root._count() / mediaGrid.pageSize))
        totalText: typeof root.topStripTotalTextProvider === "function" ? root.topStripTotalTextProvider() : (root._listLayout ? "" : (root._count() > 0 ? qsTr("%1 entries").arg(root._count()) : ""))
        rightTextOverride: typeof root.topStripRightTextProvider === "function" ? root.topStripRightTextProvider() : (!root._listLayout || mediaGrid.itemCount <= 0 ? "" : qsTr("%1 / %2").arg(mediaGrid.currentIndex + 1).arg(Math.max(1, root._count())))
    }

    BrowseListDetailView {
        id: listCard

        visible: !root._gateHide && root._listLayout
        anchors.left: parent.left
        anchors.leftMargin: root._listLayoutProfile.listCardSideMargin
        anchors.right: parent.right
        anchors.rightMargin: root._listLayoutProfile.listCardSideMargin
        anchors.top: topStrip.bottom
        anchors.topMargin: Sizing.pctH(2)
        anchors.bottom: parent.bottom
        anchors.bottomMargin: Sizing.pctH(8)
        layoutProfile: root._listLayoutProfile
        model: root.mediaModel
        totalItemsOverride: root.totalItemsOverride
        targetVisibleRowCount: root.targetVisibleRowCount
        showFileStem: root.showFileStem
        currentIndex: mediaGrid.currentIndex
        detailTitle: listCard.currentName
        detailCoverKey: root.detailRapidScrollActive ? root.detailPlaceholderKey : (root._detailImageKey() !== "" ? root._detailImageKey() : listCard.currentCoverKey)
        detailShowDescription: root.detailShowDescription
        detailShowTitle: root.detailShowTitle
        detailTags: root._detailTags()
        detailLoading: root._detailLoading()
        detailSuppressed: root.detailRapidScrollActive
        detailLoadingText: root.detailLoadingText
        detailCanPreviousImage: root.detailCanPreviousImage
        detailCanNextImage: root.detailCanNextImage
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

        visible: !root._gateHide && !root._listLayout && root.renderGridLayout
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: topStrip.bottom
        anchors.bottom: parent.bottom
        anchors.bottomMargin: root.gridBottomMargin
        focused: root.gridFocused
        model: root.mediaModel
        delegate: Tile {
            layoutProfile: root.gridLayoutProfile
            showCaption: true
        }
        layoutProfile: root.gridLayoutProfile
        columnsOverride: Sizing.gamesGridColumns
        rowsOverride: Sizing.gamesGridRows
        totalItemsOverride: root.gridTotalItemsOverride
        hasMorePages: root.gridHasMorePages
        onLoadMoreRequested: {
            if (typeof root.gridLoadMoreAction === "function")
                root.gridLoadMoreAction();
            else
                root.mediaModel.fetch_more();
        }
        onCurrentIndexChanged: {
            root._persistFocus();
            if (typeof root.gridCurrentIndexChangedAction === "function")
                root.gridCurrentIndexChangedAction();
        }
        onCurrentPageChanged: {
            if (typeof root.gridCurrentPageChangedAction === "function")
                root.gridCurrentPageChangedAction();
        }
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
        visible: !root._gateHide && !root._listLayout && root.renderGridLayout
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: root.activeLabelAtBottom ? undefined : mediaGrid.bottom
        anchors.bottom: root.activeLabelAtBottom ? parent.bottom : undefined
        anchors.bottomMargin: root.activeLabelAtBottom ? root.activeLabelBottomMargin : 0
        height: root.activeLabelHeight
        text: typeof root.activeLabelTextProvider === "function" ? root.activeLabelTextProvider() : (mediaGrid.itemCount > 0 ? root.mediaModel.name_at(mediaGrid.currentIndex) : "")
    }

    Text {
        id: bottomTotalText
        visible: root.showBottomStatusRow && !root._gateHide && !root._listLayout && root.bottomStatusLeftText !== ""
        anchors.left: parent.left
        anchors.leftMargin: root.bottomStatusLeftMargin
        anchors.verticalCenter: activeLabel.verticalCenter
        width: Sizing.px(parent.width / 3) - root.bottomStatusLeftMargin
        height: Sizing.fontSize(2.9)
        elide: Text.ElideRight
        horizontalAlignment: Text.AlignLeft
        verticalAlignment: Text.AlignVCenter
        text: root.bottomStatusLeftText
        font.family: Theme.fontUi
        font.pixelSize: Sizing.fontSize(2.9)
        color: Theme.textPrimary
        renderType: Text.NativeRendering
    }

    Text {
        visible: root.showBottomStatusRow && !root._gateHide && !root._listLayout && root.bottomStatusRightText !== ""
        anchors.right: parent.right
        anchors.rightMargin: root.bottomStatusRightMargin
        anchors.verticalCenter: activeLabel.verticalCenter
        width: Sizing.px(parent.width / 3) - root.bottomStatusRightMargin
        height: Sizing.fontSize(2.9)
        elide: Text.ElideRight
        horizontalAlignment: Text.AlignRight
        verticalAlignment: Text.AlignVCenter
        text: root.bottomStatusRightText
        font.family: Theme.fontUi
        font.pixelSize: Sizing.fontSize(2.9)
        color: Theme.textPrimary
        renderType: Text.NativeRendering
    }

    LoadingIndicator {
        visible: !root._gateHide && !root._listLayout && root.pageLoadingVisible
        anchors.left: activeLabel.left
        anchors.leftMargin: root.pageLoadingLeftMargin
        anchors.verticalCenter: activeLabel.verticalCenter
    }

    ScreenStateOverlay {
        x: root._listLayout ? listCard.x : mediaGrid.x
        y: root._listLayout ? listCard.y : mediaGrid.y
        width: root._listLayout ? listCard.width : mediaGrid.width
        height: root._listLayout ? Math.max(0, root.height - listCard.y - root._listOverlayBottomMargin) : mediaGrid.height
        loading: root._loading()
        errorMessage: root._errorMessage()
        count: root._count()
        emptyText: root.emptyText
        loadingText: root.loadingText
    }
}
