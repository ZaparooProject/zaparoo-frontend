// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

import QtQuick
import Zaparoo.Theme
import Zaparoo.Ui
import Zaparoo.Browse as Browse

// cxx-qt 0.8 patches `isFinal: true` on singleton properties but the
// qmltypes schema has no `isFinal` slot for Method, so every qinvokable
// call on a Zaparoo.Browse singleton (path_at, set_system, etc.) still
// trips qmllint's "Member can be shadowed" check. Until the schema grows
// method-level finality, suppress the compiler category file-wide.
// qmllint disable compiler

// Games screen — paged grid driven by `Browse.GamesModel`. Games keeps
// the folder-navigation, back-stack, and paging semantics that differ
// from the flat history lists, but it now reuses the shared
// `MediaListScreen` shell for the common list/detail rendering,
// focused-detail policy, and input plumbing.
MediaListScreen {
    id: games

    property alias gamesGrid: games.mediaGrid

    readonly property bool _portraitNonCrtList: !Theme.crtNativePath && Browse.Settings.current_orientation !== "horizontal"
    readonly property int _listPageSize: games._portraitNonCrtList ? 16 : 10
    readonly property int _browsePageSize: games._listLayout ? Math.max(1, games.listCard.visibleRowCount) : games.gamesGrid.pageSize
    readonly property bool _crtGridLayout: Theme.crtNativePath && !games._listLayout
    readonly property bool _tateListLayout: Browse.Settings.current_orientation !== "horizontal"
    readonly property string _gridViewId: "gamesGrid"
    readonly property string _listViewId: "gamesList"
    readonly property string _tateListViewId: "gamesListTate"
    readonly property string _activeViewId: games._listLayout ? (games._tateListLayout ? games._tateListViewId : games._listViewId) : games._gridViewId
    readonly property string _browseThemeId: BrowseLayouts.currentThemeId
    readonly property var _gridProfile: BrowseLayouts.themeProfile(games._browseThemeId, games._gridViewId)
    readonly property var _viewProfile: BrowseLayouts.themeProfile(games._browseThemeId, games._activeViewId)
    readonly property var _headerProfile: games._viewProfile && games._viewProfile.header ? games._viewProfile.header : null
    readonly property var _statusProfile: games._viewProfile && games._viewProfile.status ? games._viewProfile.status : null
    readonly property var _footerProfile: games._gridProfile && games._gridProfile.footer ? games._gridProfile.footer : null

    mediaModel: Browse.GamesModel
    emptyText: qsTr("No games in this system")
    loadingText: qsTr("Loading games…")
    totalItemsOverride: Browse.GamesModel.dir_count + Browse.GamesModel.total_files
    targetVisibleRowCount: games._listPageSize
    showFileStem: true
    detailShowDescription: false
    detailShowTitle: false
    detailLoadingText: qsTr("Loading game…")
    detailCanPreviousImage: Browse.GamesModel.current_detail_image_can_prev
    detailCanNextImage: Browse.GamesModel.current_detail_image_can_next
    detailIdentityForIndex: function (index) {
        const entryType = Browse.GamesModel.entry_type_at(index);
        if (entryType === "directory" || entryType === "root")
            return "";
        const systemId = Browse.GamesModel.system_id_at(index);
        const path = Browse.GamesModel.path_at(index);
        return systemId !== "" && path !== "" ? systemId + "\n" + path : "";
    }
    loadDetailForIndex: index => Browse.GamesModel.load_description_at(index)
    clearDetailAction: () => Browse.GamesModel.clear_current_detail()
    restoreSelectionPath: () => {
        const selected = Browse.GamesState.selected_at_level;
        return selected.length > 0 ? selected[selected.length - 1] : "";
    }
    persistSelectionPath: path => games._scheduleSelectedPersist(path)
    gridMoveAction: (dx, dy) => games._performGridMove(dx, dy)
    linearMoveAction: delta => games._performLinearMove(delta)
    pageAction: delta => games._performPage(delta)
    onListLayoutEntered: () => games._fillListPage()
    gridViewId: games._gridViewId
    listViewId: games._listViewId
    tateListViewId: games._tateListViewId
    listLeftAction: () => Browse.GamesModel.cycle_detail_image(-1)
    listRightAction: () => Browse.GamesModel.cycle_detail_image(1)
    contextMenuEnabledAt: index => {
        const entryType = Browse.GamesModel.entry_type_at(index);
        return entryType !== "directory" && entryType !== "root";
    }
    retryAction: () => {
        if (games._atFolderLevel()) {
            const stack = Browse.GamesState.path_stack;
            const top = stack[stack.length - 1];
            Browse.GamesModel.set_path(top);
            return;
        }
        const sid = Browse.GamesModel.current_system_id;
        if (sid !== "")
            Browse.GamesModel.set_system(sid);
    }
    acceptAction: index => {
        const entryType = Browse.GamesModel.entry_type_at(index);
        if (entryType === "directory" || entryType === "root") {
            games.flushSelectedPersist();
            games.requestNavigateIntoFolder(Browse.GamesModel.path_at(index));
            return;
        }
        games._scheduleSelectedPersist(Browse.GamesModel.path_at(index));
        games.flushSelectedPersist();
        Browse.GamesModel.launch_at(index);
    }
    cancelAction: () => {
        games.flushSelectedPersist();
        if (games._atFolderLevel())
            games.requestNavigateOutOfFolder();
        else
            games.requestSystemsScreen();
    }
    showTopStrip: games._statusProfile ? games._statusProfile.topStripVisible : true
    topStripTitleProvider: () => {
        const sid = Browse.GamesModel.current_system_id;
        if (sid === "")
            return "";
        const idx = Browse.SystemsModel.index_for_system_id(sid);
        return idx >= 0 ? Browse.SystemsModel.system_name_at(idx) : sid;
    }
    topStripCurrentPageProvider: () => Math.floor(games.gamesGrid.currentIndex / games._browsePageSize)
    topStripTotalPagesProvider: () => games._footerProfile && games._footerProfile.bottomStatusVisible ? 1 : Math.max(1, Math.ceil((Browse.GamesModel.dir_count + Browse.GamesModel.total_files) / games._browsePageSize))
    topStripTotalTextProvider: () => games._listLayout || (games._footerProfile && games._footerProfile.bottomStatusVisible) ? "" : (Browse.GamesModel.total_files > 0 ? qsTr("%1 files").arg(Browse.GamesModel.total_files) : "")
    topStripRightTextProvider: () => {
        if (!games._listLayout)
            return "";
        if (Browse.GamesModel.loading_more)
            return qsTr("Loading more…");
        if (games.gamesGrid.itemCount <= 0)
            return "";
        const total = Math.max(1, Browse.GamesModel.dir_count + Browse.GamesModel.total_files);
        return qsTr("%1 / %2").arg(games.gamesGrid.currentIndex + 1).arg(total);
    }
    gridBottomMargin: games._footerProfile ? games._footerProfile.gridBottomMargin : (Sizing.pctH(6) + Sizing.pctH(8) + Sizing.pctH(7))
    gridTotalItemsOverride: Browse.GamesModel.dir_count + Browse.GamesModel.total_files
    gridHasMorePages: Browse.GamesModel.has_next_page
    gridLoadMoreAction: () => Browse.GamesModel.fetch_more()
    gridCurrentPageChangedAction: () => {
        const first = games.gamesGrid.currentPage * games.gamesGrid.pageSize;
        Browse.GamesModel.visible_first_row = first;
        if (!games._listLayout)
            Browse.GamesModel.prefetch_around(first);
    }
    activeLabelTextProvider: () => games.gamesGrid.itemCount > 0 ? Browse.GamesModel.name_at(games.gamesGrid.currentIndex) : ""
    activeLabelAtBottom: true
    activeLabelBottomMargin: games._footerProfile ? games._footerProfile.activeLabelBottomMargin : Sizing.pctH(8)
    activeLabelHeight: games._footerProfile ? games._footerProfile.activeLabelHeight : Sizing.pctH(7)
    showBottomStatusRow: games._footerProfile ? games._footerProfile.bottomStatusVisible : false
    bottomStatusLeftMargin: games._footerProfile ? games._footerProfile.bottomStatusLeftMargin : 0
    bottomStatusRightMargin: games._footerProfile ? games._footerProfile.bottomStatusRightMargin : 0
    bottomStatusLeftText: games._footerProfile && games._footerProfile.bottomStatusVisible && Browse.GamesModel.total_files > 0 ? qsTr("%1 files").arg(Browse.GamesModel.total_files) : ""
    bottomStatusRightText: games._footerProfile && games._footerProfile.bottomStatusVisible && Math.ceil((Browse.GamesModel.dir_count + Browse.GamesModel.total_files) / games._browsePageSize) > 1 ? qsTr("%1 / %2").arg(Math.floor(games.gamesGrid.currentIndex / games._browsePageSize) + 1).arg(Math.max(1, Math.ceil((Browse.GamesModel.dir_count + Browse.GamesModel.total_files) / games._browsePageSize))) : ""
    pageLoadingVisible: !games._listLayout && Browse.GamesModel.loading_more && games.gamesGrid.hasPendingTarget
    pageLoadingLeftMargin: games._footerProfile && games._footerProfile.bottomStatusVisible && games.bottomStatusLeftText !== "" ? Sizing.px(games.width / 3) : games.gamesGrid.leftInset

    Binding {
        target: Browse.GamesModel
        property: "cover_key_roles_enabled"
        value: !games._listLayout
    }

    // Emitted when the user presses Escape — Main.qml flips the
    // active screen back to SystemsScreen (one peer up the back-stack;
    // a second Escape from there pops to Hub).
    signal requestSystemsScreen

    // Emitted when the user accepts a directory or root entry — Main.qml
    // pushes the level onto GamesState and drives the model into the new
    // path. Stays inside the games screen (no peer flip).
    signal requestNavigateIntoFolder(string path)

    // Emitted when the user cancels from a deeper folder level — Main.qml
    // pops one level off the stack and rebrowses the parent.
    signal requestNavigateOutOfFolder

    // Persist debounce. Writing `state.toml` is an atomic
    // write+sync_all+rename (`rust/zaparoo-core/src/persist.rs`); on
    // MiSTer's SD card that's a real disk hit, and the hold-repeat tick
    // in `Main.qml` (`_repeatTickMs = 90`) means a long Down-hold would
    // fire ~11 of these per second. We coalesce them: each move stamps
    // `_pendingSelectedPath` and restarts the timer; the final flush
    // lands on hold release / Accept / Cancel via the helpers below
    // (`Main.qml`'s `_stopRepeat` calls `flushSelectedPersist`). The
    // 250 ms interval is shorter than a deliberate single tap → hold
    // gap, so isolated presses still persist quickly.
    property string _pendingSelectedPath: ""

    Timer {
        id: persistDebounce
        interval: 250
        repeat: false
        onTriggered: {
            if (games._pendingSelectedPath !== "")
                Browse.GamesState.set_selected_at_top(games._pendingSelectedPath);
            games._pendingSelectedPath = "";
        }
    }

    function _scheduleSelectedPersist(path: string): void {
        games._pendingSelectedPath = path;
        persistDebounce.restart();
    }

    // Force any pending persist to land synchronously. Called from the
    // Accept path (we hand control off to launch and the kill-resume
    // guarantee depends on the latest selection being on disk), from
    // Cancel/Escape (about to flip screens), and from `Main.qml`'s
    // `_stopRepeat` so dpad release commits the latest cell.
    function flushSelectedPersist(): void {
        if (!persistDebounce.running && games._pendingSelectedPath === "")
            return;
        persistDebounce.stop();
        if (games._pendingSelectedPath !== "")
            Browse.GamesState.set_selected_at_top(games._pendingSelectedPath);
        games._pendingSelectedPath = "";
    }

    // Move selection by (dx, dy) and commit the new selection on
    // success. Unlike HubScreen's _handleSystems, none of the games-grid
    // directions have a row-edge escape branch, so all four cardinal
    // actions share this exact body.
    function _performGridMove(dx: int, dy: int): void {
        if (games.gamesGrid.moveSelection(dx, dy))
            games._scheduleSelectedPersist(Browse.GamesModel.path_at(games.gamesGrid.currentIndex));
    }

    function _performLinearMove(delta: int): void {
        const count = games.gamesGrid.itemCount;
        if (count <= 0)
            return;
        let next = games.gamesGrid.currentIndex + delta;
        if (next < 0)
            next = count - 1;
        else if (next >= count) {
            const knownTotal = Browse.GamesModel.dir_count + Browse.GamesModel.total_files;
            if (games._listHasMore(count, knownTotal)) {
                Browse.GamesModel.fetch_more();
                return;
            }
            next = 0;
        }
        if (next === games.gamesGrid.currentIndex) {
            if (next >= count - 2)
                Browse.GamesModel.fetch_more();
            return;
        }
        games.gamesGrid.currentIndex = next;
        games._scheduleSelectedPersist(Browse.GamesModel.path_at(games.gamesGrid.currentIndex));
        if (next >= count - 2 && Browse.GamesModel.has_next_page)
            Browse.GamesModel.fetch_more();
        games._prefetchListTail(next);
    }

    function _fillListPage(): void {
        if (!games._listLayout)
            return;
        const count = games.gamesGrid.itemCount;
        if (count > 0 && count <= games._listPageSize && Browse.GamesModel.has_next_page)
            Browse.GamesModel.fetch_more();
    }

    function _listHasMore(count: int, knownTotal: int): bool {
        return Browse.GamesModel.has_next_page || knownTotal > count;
    }

    function _prefetchListTail(index: int): void {
        if (!games._listLayout || Browse.GamesModel.loading_more)
            return;
        const count = games.gamesGrid.itemCount;
        const knownTotal = Browse.GamesModel.dir_count + Browse.GamesModel.total_files;
        if (index >= count - games._listPageSize && games._listHasMore(count, knownTotal))
            Browse.GamesModel.fetch_more();
    }

    // Page jump (L/R shoulder buttons). Wraps in both directions; same
    // post-move state-commit path as _performGridMove so the saved entry
    // tracks whichever item the user lands on.
    function _performPage(delta: int): void {
        if (games._listLayout) {
            games._performLinearMove(delta * games._browsePageSize);
            return;
        }
        if (games.gamesGrid.pageBy(delta))
            games._scheduleSelectedPersist(Browse.GamesModel.path_at(games.gamesGrid.currentIndex));
    }

    // True when we're inside a navigated folder (path_stack length > 1).
    // Drives folder-aware cancel routing.
    function _atFolderLevel(): bool {
        return Browse.GamesState.path_stack.length > 1;
    }
}
