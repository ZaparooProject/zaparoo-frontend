// Zaparoo Launcher
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

import QtQuick
import Zaparoo.Theme
import Zaparoo.Ui
import Zaparoo.Browse as Browse

// cxx-qt 0.8 patches `isFinal: true` on singleton properties but the
// qmltypes schema has no `isFinal` slot for Method, so every qinvokable
// call on a Zaparoo.Browse singleton (launch_at, name_at, etc.) still
// trips qmllint's "Member can be shadowed" check. Until the schema
// grows method-level finality, suppress the compiler category file-wide.
// qmllint disable compiler

// Favorites screen — flat paged grid driven by
// `Browse.FavoritesModel`. Pure input dispatcher: emits
// `requestHubScreen()` on Escape and launches the highlighted entry on
// Accept by calling the model's `launch_at` (which fans out to Core's
// `run` endpoint).
//
// Favorites is a flat list — no folder navigation, no card-write flow —
// so this screen is much simpler than `GamesScreen.qml`.
Item {
    id: favorites

    property alias favoritesGrid: favoritesGrid

    // Bound by MainLayout to `root.pendingTransition !== ""`. Favorites is
    // a destination, never a source, so this is currently always false
    // when the screen is visible — kept for parity with the other
    // screens so the convention holds when a future routing change adds
    // a Favorites-as-source path.
    property bool transitioning: false
    // Router-driven flag: `MainLayout` writes this to
    // `!ScreenManager.hasModal` so the focused tile's accent ring
    // hides while a modal (the context menu) is on top of the stack.
    property bool gridFocused: true
    readonly property bool _listLayout: Browse.Settings.current_browse_layout === "list"
    readonly property int _listOverlayBottomMargin: Sizing.pctH(15)

    // True while either the cross-screen router is mid-flip
    // (`transitioning`) or the in-screen cover gate is holding
    // `FavoritesModel.loading`. The grid + active-label hide on this so
    // the centred `ScreenStateOverlay` paints alone on a cleared band
    // during cold-launch / model-reset, matching `GamesScreen.qml`.
    // Pagination uses a separate `loading_more` flag and is unaffected.
    readonly property bool _gateHide: favorites.transitioning || Browse.FavoritesModel.loading
    property string _detailRequestKey: ""

    signal requestHubScreen
    signal requestContextMenu(int index, var anchorRect)

    // Restore the previously focused entry when the model is Ready.
    // Called by the router after the Hub→Favorites transition lands;
    // also runs whenever the model count changes so tag changes keep
    // the user's previously highlighted row if it's still in the page.
    function restoreSelection(): void {
        if (Browse.FavoritesModel.count <= 0)
            return;
        const path = Browse.FavoritesState.selected_path;
        if (path === "")
            return;
        const idx = Browse.FavoritesModel.index_for_path(path);
        if (idx >= 0 && idx !== favoritesGrid.currentIndex)
            favoritesGrid.currentIndex = idx;
    }

    // Persist the focused entry's path on every focus move so a
    // kill-resume puts the highlight back. `path_at` returns "" for
    // out-of-range indices; skip writes on those so PagedGrid's
    // shrinkage clamp (currentIndex → 0 when itemCount drops to 0)
    // doesn't clobber the saved path with the empty fallback.
    function _persistFocus(): void {
        const idx = favoritesGrid.currentIndex;
        if (idx < 0)
            return;
        const path = Browse.FavoritesModel.path_at(idx);
        if (path === "")
            return;
        Browse.FavoritesState.selected_path = path;
    }

    function _focusIndex(index: int): void {
        if (index < 0 || index >= favorites.favoritesGrid.itemCount)
            return;
        favorites.favoritesGrid.currentIndex = index;
        favorites._persistFocus();
    }

    function _selectedDetailKey(): string {
        if (favorites.favoritesGrid.itemCount <= 0)
            return "";
        const idx = favoritesGrid.currentIndex;
        const systemId = Browse.FavoritesModel.system_id_at(idx);
        const path = Browse.FavoritesModel.path_at(idx);
        return systemId !== "" && path !== "" ? systemId + "\n" + path : "";
    }

    function _scheduleDetailLoad(): void {
        if (!favorites._listLayout)
            return;
        const key = favorites._selectedDetailKey();
        if (key === "" || key === favorites._detailRequestKey)
            return;
        favorites._detailRequestKey = key;
        detailLoadDebounce.restart();
    }

    function _loadSelectedDetail(): void {
        if (!favorites._listLayout || favorites.favoritesGrid.itemCount <= 0)
            return;
        Browse.FavoritesModel.load_detail_at(favoritesGrid.currentIndex);
    }

    function _performLinearMove(delta: int): void {
        const count = favorites.favoritesGrid.itemCount;
        if (count <= 0)
            return;
        let next = favorites.favoritesGrid.currentIndex + delta;
        if (next < 0)
            next = count - 1;
        else if (next >= count)
            next = 0;
        if (next === favorites.favoritesGrid.currentIndex) {
            if (next >= count - 2)
                Browse.FavoritesModel.fetch_more();
            return;
        }
        favorites.favoritesGrid.currentIndex = next;
        favorites._persistFocus();
        if (next >= count - 2)
            Browse.FavoritesModel.fetch_more();
    }

    function _state(): string {
        if (Browse.FavoritesModel.loading)
            return "loading";
        if ((Browse.FavoritesModel.error_message ?? "") !== "")
            return "error";
        if (Browse.FavoritesModel.count === 0)
            return "empty";
        return "ready";
    }

    function handleAction(action: string): void {
        if (action === "left") {
            if (!favorites._listLayout)
                favorites.favoritesGrid.moveSelection(-1, 0);
        } else if (action === "right") {
            if (!favorites._listLayout)
                favorites.favoritesGrid.moveSelection(1, 0);
        } else if (action === "up") {
            if (favorites._listLayout)
                favorites._performLinearMove(-1);
            else
                favorites.favoritesGrid.moveSelection(0, -1);
        } else if (action === "down") {
            if (favorites._listLayout)
                favorites._performLinearMove(1);
            else
                favorites.favoritesGrid.moveSelection(0, 1);
        } else if (action === "page_prev") {
            if (favorites._state() === "ready")
                favorites.favoritesGrid.pageBy(-1);
        } else if (action === "page_next") {
            if (favorites._state() === "ready")
                favorites.favoritesGrid.pageBy(1);
        } else if (action === "accept") {
            // Loading swallows the press at the screen layer; Empty/Error
            // re-fires the current load by calling `fetch_more` (a stale
            // cursor still triggers the fetch — the model's seq guard
            // discards a result that no longer matches the chain).
            const state = favorites._state();
            if (state === "loading")
                return;
            if (state === "error" || state === "empty") {
                Browse.FavoritesModel.fetch_more();
                return;
            }
            Browse.FavoritesModel.launch_at(favorites.favoritesGrid.currentIndex);
        } else if (action === "write_card") {
            if (favorites.favoritesGrid.itemCount > 0) {
                const idx = favorites.favoritesGrid.currentIndex;
                favorites._persistFocus();
                const rect = favorites._listLayout ? listCard.currentCellRectIn(favorites) : favorites.favoritesGrid.currentCellRectIn(favorites);
                favorites.requestContextMenu(idx, rect);
            }
        } else if (action === "cancel") {
            favorites.requestHubScreen();
        }
    }

    // ── Visual tree ───────────────────────────────────────────────────────────

    Timer {
        id: detailLoadDebounce
        interval: 220
        repeat: false
        onTriggered: favorites._loadSelectedDetail()
    }

    Connections {
        target: Browse.FavoritesModel
        function onCountChanged(): void {
            if (Browse.FavoritesModel.current_detail_tags === "")
                favorites._detailRequestKey = "";
            favorites._scheduleDetailLoad();
        }
    }

    // Top status strip — page counter, screen title, total entries.
    // The total badge reads `count` directly: favorites is a flat list,
    // so the rendered count tracks the loaded slice rather than a
    // server-side total. Good enough until Core surfaces a total.
    TopStatusStrip {
        id: topStrip
        visible: !favorites._gateHide
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: parent.top
        anchors.topMargin: Sizing.headerBottom + Sizing.pctH(1)
        height: Sizing.pctH(7)
        title: qsTr("Favorites")
        currentPage: favoritesGrid.currentPage
        totalPages: Math.max(1, Math.ceil(Browse.FavoritesModel.count / favoritesGrid.pageSize))
        totalText: favorites._listLayout ? "" : (Browse.FavoritesModel.count > 0 ? qsTr("%1 entries").arg(Browse.FavoritesModel.count) : "")
        rightTextOverride: {
            if (!favorites._listLayout || favoritesGrid.itemCount <= 0)
                return "";
            return qsTr("%1 / %2").arg(favoritesGrid.currentIndex + 1).arg(Math.max(1, Browse.FavoritesModel.count));
        }
    }

    BrowseListDetailView {
        id: listCard

        visible: !favorites._gateHide && favorites._listLayout
        anchors.left: parent.left
        anchors.leftMargin: Sizing.pctW(5)
        anchors.right: parent.right
        anchors.rightMargin: Sizing.pctW(5)
        anchors.top: topStrip.bottom
        anchors.topMargin: Sizing.pctH(2)
        anchors.bottom: parent.bottom
        anchors.bottomMargin: Sizing.pctH(8)
        model: Browse.FavoritesModel
        currentIndex: favoritesGrid.currentIndex
        detailTitle: listCard.currentName
        detailCoverKey: Browse.FavoritesModel.current_detail_image_key !== "" ? Browse.FavoritesModel.current_detail_image_key : listCard.currentCoverKey
        detailTags: Browse.FavoritesModel.current_detail_tags
        onItemHovered: index => favorites._focusIndex(index)
        onItemClicked: index => {
            favorites._focusIndex(index);
            favorites.handleAction("accept");
        }
        onItemRightClicked: index => {
            favorites._focusIndex(index);
            favorites.handleAction("write_card");
        }
        onEmptyRightClicked: favorites.handleAction("cancel")
        onPageWheelRequested: delta => favorites.handleAction(delta > 0 ? "page_next" : "page_prev")
        onVisibleChanged: {
            if (visible)
                favorites._scheduleDetailLoad();
            else {
                favorites._detailRequestKey = "";
                Browse.FavoritesModel.clear_current_detail();
            }
        }
    }

    PagedGrid {
        id: favoritesGrid

        visible: !favorites._gateHide && !favorites._listLayout
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: topStrip.bottom
        anchors.bottom: parent.bottom
        // pctH(15) clears the focused-title row (pctH(7)) plus the
        // pctH(2) gap and the pctH(6) instructions bar — same recipe
        // GamesScreen uses, so the bottom band reads consistently.
        anchors.bottomMargin: Sizing.pctH(15)
        focused: favorites.gridFocused
        model: Browse.FavoritesModel
        delegate: Tile {
            showCaption: true
        }
        // Match games-grid layout (taller cover-art tiles); the systems
        // grid's 5x3 starves vertical space on these covers.
        columnsOverride: Sizing.gamesGridColumns
        rowsOverride: Sizing.gamesGridRows
        onLoadMoreRequested: Browse.FavoritesModel.fetch_more()
        onCurrentIndexChanged: {
            favorites._persistFocus();
            favorites._scheduleDetailLoad();
        }
        onItemHovered: index => favorites._focusIndex(index)
        onItemClicked: index => {
            favorites._focusIndex(index);
            favorites.handleAction("accept");
        }
        onItemRightClicked: index => {
            favorites._focusIndex(index);
            favorites.handleAction("write_card");
        }
        onEmptyRightClicked: favorites.handleAction("cancel")
    }

    // Focused-tile caption — single big line just under the grid.
    // Same typography / placement as GamesScreen so the screens read
    // as a matched pair (top strip = section context, bottom row =
    // focused-tile selection).
    ActiveLabel {
        id: activeLabel
        visible: !favorites._gateHide && !favorites._listLayout
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: favoritesGrid.bottom
        height: Sizing.pctH(7)
        text: favoritesGrid.itemCount > 0 ? Browse.FavoritesModel.name_at(favoritesGrid.currentIndex) : ""
    }

    ScreenStateOverlay {
        x: favorites._listLayout ? 0 : favoritesGrid.x
        y: favorites._listLayout ? listCard.y : favoritesGrid.y
        width: favorites._listLayout ? favorites.width : favoritesGrid.width
        height: favorites._listLayout ? Math.max(0, favorites.height - listCard.y - favorites._listOverlayBottomMargin) : favoritesGrid.height
        loading: Browse.FavoritesModel.loading
        errorMessage: Browse.FavoritesModel.error_message ?? ""
        count: Browse.FavoritesModel.count
        emptyText: qsTr("No favorites yet")
        loadingText: qsTr("Loading favorites…")
    }
}
