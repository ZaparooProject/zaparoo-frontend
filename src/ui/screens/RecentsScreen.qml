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

// Recently Played screen — flat paged grid driven by
// `Browse.RecentsModel`. Pure input dispatcher: emits
// `requestHubScreen()` on Escape and launches the highlighted entry on
// Accept by calling the model's `launch_at` (which fans out to Core's
// `run` endpoint).
//
// History is a flat list — no folder navigation, no card-write flow —
// so this screen is much simpler than `GamesScreen.qml`.
Item {
    id: recents

    property alias recentsGrid: recentsGrid

    // Bound by MainLayout to `root.pendingTransition !== ""`. Recents is
    // a destination, never a source, so this is currently always false
    // when the screen is visible — kept for parity with the other
    // screens so the convention holds when a future routing change adds
    // a Recents-as-source path.
    property bool transitioning: false
    // Router-driven flag: `MainLayout` writes this to
    // `!ScreenManager.hasModal` so the focused tile's accent ring
    // hides while a modal (the context menu) is on top of the stack.
    property bool gridFocused: true
    readonly property bool _listLayout: Browse.Settings.current_browse_layout === "list"
    readonly property int _listOverlayBottomMargin: Sizing.pctH(15)

    // True while either the cross-screen router is mid-flip
    // (`transitioning`) or the in-screen cover gate is holding
    // `RecentsModel.loading`. The grid + active-label hide on this so
    // the centred `ScreenStateOverlay` paints alone on a cleared band
    // during cold-launch / model-reset, matching `GamesScreen.qml`.
    // Pagination uses a separate `loading_more` flag and is unaffected.
    readonly property bool _gateHide: recents.transitioning || Browse.RecentsModel.loading
    property string _detailRequestKey: ""

    signal requestHubScreen
    signal requestContextMenu(int index, var anchorRect)

    // Restore the previously focused entry when the model is Ready.
    // Called by the router after the Hub→Recents transition lands;
    // also runs whenever the model count changes so a freshly-played
    // game (which prepends to history and resets the model) keeps the
    // user's previously highlighted row if it's still in the page.
    function restoreSelection(): void {
        if (Browse.RecentsModel.count <= 0)
            return;
        const path = Browse.RecentsState.selected_path;
        if (path === "")
            return;
        const idx = Browse.RecentsModel.index_for_path(path);
        if (idx >= 0 && idx !== recentsGrid.currentIndex)
            recentsGrid.currentIndex = idx;
    }

    // Persist the focused entry's path on every focus move so a
    // kill-resume puts the highlight back. `path_at` returns "" for
    // out-of-range indices; skip writes on those so PagedGrid's
    // shrinkage clamp (currentIndex → 0 when itemCount drops to 0)
    // doesn't clobber the saved path with the empty fallback.
    function _persistFocus(): void {
        const idx = recentsGrid.currentIndex;
        if (idx < 0)
            return;
        const path = Browse.RecentsModel.path_at(idx);
        if (path === "")
            return;
        Browse.RecentsState.selected_path = path;
    }

    function _focusIndex(index: int): void {
        if (index < 0 || index >= recents.recentsGrid.itemCount)
            return;
        recents.recentsGrid.currentIndex = index;
        recents._persistFocus();
    }

    function _selectedDetailKey(): string {
        if (recents.recentsGrid.itemCount <= 0)
            return "";
        const idx = recentsGrid.currentIndex;
        const systemId = Browse.RecentsModel.system_id_at(idx);
        const path = Browse.RecentsModel.path_at(idx);
        return systemId !== "" && path !== "" ? systemId + "\n" + path : "";
    }

    function _scheduleDetailLoad(): void {
        if (!recents._listLayout)
            return;
        const key = recents._selectedDetailKey();
        if (key === "" || key === recents._detailRequestKey)
            return;
        recents._detailRequestKey = key;
        detailLoadDebounce.restart();
    }

    function _loadSelectedDetail(): void {
        if (!recents._listLayout || recents.recentsGrid.itemCount <= 0)
            return;
        Browse.RecentsModel.load_detail_at(recentsGrid.currentIndex);
    }

    function _performLinearMove(delta: int): void {
        const count = recents.recentsGrid.itemCount;
        if (count <= 0)
            return;
        let next = recents.recentsGrid.currentIndex + delta;
        if (next < 0)
            next = count - 1;
        else if (next >= count)
            next = 0;
        if (next === recents.recentsGrid.currentIndex) {
            if (next >= count - 2)
                Browse.RecentsModel.fetch_more();
            return;
        }
        recents.recentsGrid.currentIndex = next;
        recents._persistFocus();
        if (next >= count - 2)
            Browse.RecentsModel.fetch_more();
    }

    function _state(): string {
        if (Browse.RecentsModel.loading)
            return "loading";
        if ((Browse.RecentsModel.error_message ?? "") !== "")
            return "error";
        if (Browse.RecentsModel.count === 0)
            return "empty";
        return "ready";
    }

    function handleAction(action: string): void {
        if (action === "left") {
            if (!recents._listLayout)
                recents.recentsGrid.moveSelection(-1, 0);
        } else if (action === "right") {
            if (!recents._listLayout)
                recents.recentsGrid.moveSelection(1, 0);
        } else if (action === "up") {
            if (recents._listLayout)
                recents._performLinearMove(-1);
            else
                recents.recentsGrid.moveSelection(0, -1);
        } else if (action === "down") {
            if (recents._listLayout)
                recents._performLinearMove(1);
            else
                recents.recentsGrid.moveSelection(0, 1);
        } else if (action === "page_prev") {
            if (recents._state() === "ready")
                recents.recentsGrid.pageBy(-1);
        } else if (action === "page_next") {
            if (recents._state() === "ready")
                recents.recentsGrid.pageBy(1);
        } else if (action === "accept") {
            // Loading swallows the press at the screen layer; Empty/Error
            // re-fires the current load by calling `fetch_more` (a stale
            // cursor still triggers the fetch — the model's seq guard
            // discards a result that no longer matches the chain).
            const state = recents._state();
            if (state === "loading")
                return;
            if (state === "error" || state === "empty") {
                Browse.RecentsModel.fetch_more();
                return;
            }
            Browse.RecentsModel.launch_at(recents.recentsGrid.currentIndex);
        } else if (action === "write_card") {
            if (recents.recentsGrid.itemCount > 0) {
                const idx = recents.recentsGrid.currentIndex;
                recents._persistFocus();
                const rect = recents._listLayout ? listCard.currentCellRectIn(recents) : recents.recentsGrid.currentCellRectIn(recents);
                recents.requestContextMenu(idx, rect);
            }
        } else if (action === "cancel") {
            recents.requestHubScreen();
        }
    }

    // ── Visual tree ───────────────────────────────────────────────────────────

    Timer {
        id: detailLoadDebounce
        interval: 220
        repeat: false
        onTriggered: recents._loadSelectedDetail()
    }

    Connections {
        target: Browse.RecentsModel
        function onCountChanged(): void {
            if (Browse.RecentsModel.current_detail_tags === "")
                recents._detailRequestKey = "";
            recents._scheduleDetailLoad();
        }
    }

    // Top status strip — page counter, screen title, total entries.
    // The total badge reads `count` directly: history is a flat list,
    // so the rendered count tracks the loaded slice rather than a
    // server-side total. Good enough until Core surfaces a total.
    TopStatusStrip {
        id: topStrip
        visible: !recents._gateHide
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: parent.top
        anchors.topMargin: Sizing.headerBottom + Sizing.pctH(1)
        height: Sizing.pctH(7)
        title: qsTr("Recently Played")
        currentPage: recentsGrid.currentPage
        totalPages: Math.max(1, Math.ceil(Browse.RecentsModel.count / recentsGrid.pageSize))
        totalText: recents._listLayout ? "" : (Browse.RecentsModel.count > 0 ? qsTr("%1 entries").arg(Browse.RecentsModel.count) : "")
        rightTextOverride: {
            if (!recents._listLayout || recentsGrid.itemCount <= 0)
                return "";
            return qsTr("%1 / %2").arg(recentsGrid.currentIndex + 1).arg(Math.max(1, Browse.RecentsModel.count));
        }
    }

    BrowseListDetailView {
        id: listCard

        visible: !recents._gateHide && recents._listLayout
        anchors.left: parent.left
        anchors.leftMargin: Sizing.pctW(5)
        anchors.right: parent.right
        anchors.rightMargin: Sizing.pctW(5)
        anchors.top: topStrip.bottom
        anchors.topMargin: Sizing.pctH(2)
        anchors.bottom: parent.bottom
        anchors.bottomMargin: Sizing.pctH(8)
        model: Browse.RecentsModel
        currentIndex: recentsGrid.currentIndex
        detailTitle: listCard.currentName
        detailCoverKey: Browse.RecentsModel.current_detail_image_key !== "" ? Browse.RecentsModel.current_detail_image_key : listCard.currentCoverKey
        detailTags: Browse.RecentsModel.current_detail_tags
        onItemHovered: index => recents._focusIndex(index)
        onItemClicked: index => {
            recents._focusIndex(index);
            recents.handleAction("accept");
        }
        onItemRightClicked: index => {
            recents._focusIndex(index);
            recents.handleAction("write_card");
        }
        onEmptyRightClicked: recents.handleAction("cancel")
        onPageWheelRequested: delta => recents.handleAction(delta > 0 ? "page_next" : "page_prev")
        onVisibleChanged: {
            if (visible)
                recents._scheduleDetailLoad();
            else {
                recents._detailRequestKey = "";
                Browse.RecentsModel.clear_current_detail();
            }
        }
    }

    PagedGrid {
        id: recentsGrid

        visible: !recents._gateHide && !recents._listLayout
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: topStrip.bottom
        anchors.bottom: parent.bottom
        // pctH(15) clears the focused-title row (pctH(7)) plus the
        // pctH(2) gap and the pctH(6) instructions bar — same recipe
        // GamesScreen uses, so the bottom band reads consistently.
        anchors.bottomMargin: Sizing.pctH(15)
        focused: recents.gridFocused
        model: Browse.RecentsModel
        delegate: Tile {
            showCaption: true
        }
        // Match games-grid layout (taller cover-art tiles); the systems
        // grid's 5x3 starves vertical space on these covers.
        columnsOverride: Sizing.gamesGridColumns
        rowsOverride: Sizing.gamesGridRows
        onLoadMoreRequested: Browse.RecentsModel.fetch_more()
        onCurrentIndexChanged: {
            recents._persistFocus();
            recents._scheduleDetailLoad();
        }
        onItemHovered: index => recents._focusIndex(index)
        onItemClicked: index => {
            recents._focusIndex(index);
            recents.handleAction("accept");
        }
        onItemRightClicked: index => {
            recents._focusIndex(index);
            recents.handleAction("write_card");
        }
        onEmptyRightClicked: recents.handleAction("cancel")
        onPageWheelRequested: delta => recents.handleAction(delta > 0 ? "page_next" : "page_prev")
    }

    // Focused-tile caption — single big line just under the grid.
    // Same typography / placement as GamesScreen so the screens read
    // as a matched pair (top strip = section context, bottom row =
    // focused-tile selection).
    ActiveLabel {
        id: activeLabel
        visible: !recents._gateHide && !recents._listLayout
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: recentsGrid.bottom
        height: Sizing.pctH(7)
        text: recentsGrid.itemCount > 0 ? Browse.RecentsModel.name_at(recentsGrid.currentIndex) : ""
    }

    ScreenStateOverlay {
        x: recents._listLayout ? 0 : recentsGrid.x
        y: recents._listLayout ? listCard.y : recentsGrid.y
        width: recents._listLayout ? recents.width : recentsGrid.width
        height: recents._listLayout ? Math.max(0, recents.height - listCard.y - recents._listOverlayBottomMargin) : recentsGrid.height
        loading: Browse.RecentsModel.loading
        errorMessage: Browse.RecentsModel.error_message ?? ""
        count: Browse.RecentsModel.count
        emptyText: qsTr("Nothing played yet")
        loadingText: qsTr("Loading recently played…")
    }
}
