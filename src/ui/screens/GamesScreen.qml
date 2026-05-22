// Zaparoo Launcher
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

// Games screen — paged grid driven by `Browse.GamesModel`. Owns the
// action dispatch for the games subset; emits `requestSystemsScreen`
// on Escape so Main.qml can drive the cross-screen back-jump.
Item {
    id: games

    property alias gamesGrid: gamesGrid

    // Router-driven flag: `MainLayout.qml` writes this to
    // `root.pendingTransition !== ""` for every peer screen, including
    // this one. The screen body hides while it's true so the global
    // "Loading…" cue paints alone on a cleared band rather than over
    // a populated grid.
    property bool transitioning: false
    // Router-driven flag: `MainLayout` writes this to
    // `!ScreenManager.hasModal` so the focused tile's accent ring
    // hides while a modal (the context menu) is on top of the stack.
    // Avoids the two-focus-ring read where the menu's selected entry
    // and the anchored tile both light up.
    property bool gridFocused: true
    readonly property bool _listLayout: Browse.Settings.current_browse_layout === "list"
    readonly property int _listPageSize: 10
    readonly property int _browsePageSize: games._listLayout ? Math.max(1, listCard.visibleRowCount) : gamesGrid.pageSize
    readonly property bool _crtGridLayout: Theme.crtNativePath && !games._listLayout
    readonly property var _tileLayout: games._crtGridLayout ? BrowseLayouts.crtTile : BrowseLayouts.defaultTile
    property bool _currentMoveIsRepeat: false

    // Cover-gate flag: true while `GamesModel` is holding `loading`
    // for the initial-page paint. Pagination uses a separate
    // `loading_more` flag, so PgDn doesn't trip this. The body hides
    // on either flag (see `visible:` bindings below) so the centred
    // `ScreenStateOverlay` paints alone in both cases.
    readonly property bool coverGateLoading: Browse.GamesModel.loading

    Binding {
        target: Browse.GamesModel
        property: "cover_key_roles_enabled"
        value: !games._listLayout
    }

    on_ListLayoutChanged: {
        if (games._listLayout) {
            games._fillListPage();
            games._scheduleMetadataLoad(false);
        }
    }

    // Emitted when the user presses Escape — Main.qml flips the
    // active screen back to SystemsScreen (one peer up the back-stack;
    // a second Escape from there pops to Hub).
    signal requestSystemsScreen
    signal requestContextMenu(int index, var anchorRect)

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
    function _performMove(dx: int, dy: int, isRepeat): void {
        if (games._listLayout) {
            if (dy !== 0)
                games._performLinearMove(dy, isRepeat === true);
            return;
        }
        if (games.gamesGrid.moveSelection(dx, dy))
            games._scheduleSelectedPersist(Browse.GamesModel.path_at(games.gamesGrid.currentIndex));
    }

    function _performLinearMove(delta: int, isRepeat: bool): void {
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
        games._currentMoveIsRepeat = isRepeat;
        games.gamesGrid.currentIndex = next;
        games._currentMoveIsRepeat = false;
        games._scheduleSelectedPersist(Browse.GamesModel.path_at(games.gamesGrid.currentIndex));
        if (next >= count - 2 && Browse.GamesModel.has_next_page)
            Browse.GamesModel.fetch_more();
        games._prefetchListTail(next);
    }

    function _scheduleMetadataLoad(isRepeat): void {
        if (!games._listLayout)
            return;
        if (isRepeat === true)
            Browse.GamesModel.clear_current_detail();
        metadataLoadDebounce.restart();
    }

    function _loadSelectedMetadata(): void {
        if (!games._listLayout || games.gamesGrid.itemCount <= 0)
            return;
        Browse.GamesModel.load_description_at(gamesGrid.currentIndex);
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
    // post-move state-commit path as _performMove so the saved entry
    // tracks whichever item the user lands on.
    function _performPage(delta: int): void {
        if (games._listLayout) {
            games._performLinearMove(delta * games._browsePageSize, false);
            return;
        }
        if (games.gamesGrid.pageBy(delta))
            games._scheduleSelectedPersist(Browse.GamesModel.path_at(games.gamesGrid.currentIndex));
    }

    function _focusIndex(index: int): void {
        if (index < 0 || index >= games.gamesGrid.itemCount)
            return;
        games._currentMoveIsRepeat = false;
        games.gamesGrid.currentIndex = index;
        games._currentMoveIsRepeat = false;
        games._scheduleSelectedPersist(Browse.GamesModel.path_at(games.gamesGrid.currentIndex));
    }

    // Mirrors ScreenStateOverlay's `state` ternary so accept routing and
    // the in-screen overlay agree on which state we're in.
    function _state(): string {
        if (Browse.GamesModel.loading)
            return "loading";
        if ((Browse.GamesModel.error_message ?? "") !== "")
            return "error";
        if (Browse.GamesModel.count === 0)
            return "empty";
        return "ready";
    }

    // True when we're inside a navigated folder (path_stack length > 1).
    // Drives folder-aware cancel routing.
    function _atFolderLevel(): bool {
        return Browse.GamesState.path_stack.length > 1;
    }

    function handleAction(action: string, isRepeat): void {
        if (games._listLayout && isRepeat === true && (action === "up" || action === "down")) {
            Browse.GamesModel.clear_current_detail();
            metadataLoadDebounce.restart();
        }
        if (action === "left") {
            if (games._listLayout)
                Browse.GamesModel.cycle_detail_image(-1);
            else
                games._performMove(-1, 0, isRepeat);
        } else if (action === "right") {
            if (games._listLayout)
                Browse.GamesModel.cycle_detail_image(1);
            else
                games._performMove(1, 0, isRepeat);
        } else if (action === "up") {
            games._performMove(0, -1, isRepeat);
        } else if (action === "down") {
            games._performMove(0, 1, isRepeat);
        } else if (action === "page_prev") {
            // L shoulder. Ignored on non-Ready states — there's no
            // data to page through.
            if (games._state() === "ready")
                games._performPage(-1);
        } else if (action === "page_next") {
            // R shoulder.
            if (games._state() === "ready")
                games._performPage(1);
        } else if (action === "accept") {
            // Accept routing depends on the screen's data state, matching
            // the help bar vocabulary in MainLayout.qml. Loading swallows
            // the press (load is in flight); Error/Empty re-fires the
            // current load (the [OK] RETRY behavior the help bar
            // promises); Ready launches the highlighted game OR drills
            // into a directory/root entry. The retry path picks
            // set_path vs set_system based on whether we're at a deeper
            // level, so retrying inside a folder doesn't kick the user
            // back to the system root.
            const state = games._state();
            if (state === "loading")
                return;
            if (state === "error" || state === "empty") {
                if (games._atFolderLevel()) {
                    const stack = Browse.GamesState.path_stack;
                    const top = stack[stack.length - 1];
                    Browse.GamesModel.set_path(top);
                } else {
                    const sid = Browse.GamesModel.current_system_id;
                    if (sid !== "")
                        Browse.GamesModel.set_system(sid);
                }
                return;
            }
            const idx = games.gamesGrid.currentIndex;
            const entryType = Browse.GamesModel.entry_type_at(idx);
            if (entryType === "directory" || entryType === "root") {
                // Flush before push_level. The debounced timer writes to
                // selected_at_level.last(); push_level appends a new ""
                // entry for the child level, so a late flush would land
                // the parent's selection on the just-pushed child level.
                // Same handoff pattern as launch_at and cancel below.
                games.flushSelectedPersist();
                games.requestNavigateIntoFolder(Browse.GamesModel.path_at(idx));
                return;
            }
            // Persist before handing control away. Directional moves
            // already update the saved selection on every step, but the
            // user may press Accept on the first highlighted entry
            // without navigating, leaving the saved selection stale
            // from a prior system. Writing here makes the commit
            // explicit so a kill during launch resumes on the correct
            // entry. Schedule + flush so the debounced timer doesn't
            // strand a later move past launch handoff.
            games._scheduleSelectedPersist(Browse.GamesModel.path_at(idx));
            games.flushSelectedPersist();
            Browse.GamesModel.launch_at(idx);
        } else if (action === "write_card") {
            if (games.gamesGrid.itemCount > 0) {
                const idx = games.gamesGrid.currentIndex;
                // Folders/roots have no menu — open=Accept, X is a no-op.
                const entryType = Browse.GamesModel.entry_type_at(idx);
                if (entryType === "directory" || entryType === "root")
                    return;
                games._scheduleSelectedPersist(Browse.GamesModel.path_at(idx));
                games.flushSelectedPersist();
                const rect = games._listLayout ? listCard.currentCellRectIn(games) : games.gamesGrid.currentCellRectIn(games);
                games.requestContextMenu(idx, rect);
            }
        } else if (action === "cancel") {
            games.flushSelectedPersist();
            if (games._atFolderLevel())
                games.requestNavigateOutOfFolder();
            else
                games.requestSystemsScreen();
        }
    }

    // ── Visual tree ───────────────────────────────────────────────────────────

    Timer {
        id: metadataLoadDebounce
        interval: 220
        repeat: false
        onTriggered: games._loadSelectedMetadata()
    }

    // Top status strip — page counter (left), system title (center),
    // total-files badge (right). System title is composed via
    // SystemsModel because GamesModel only carries `current_system_id`,
    // not the human name. The id-fallback covers the brief navigate
    // window before SystemsModel sees the new id and the test harness
    // case where SystemsModel is empty; the user sees the id rather
    // than nothing.
    //
    // The screen Item fills the whole window, so the strip clears the
    // MainLayout HeaderBar (Sizing.headerBottom) with a small gap.
    // Total is exact: Core's media.browse returns directories only on
    // page 1 and always before files, so dir_count + total_files is
    // the precise entry count for the path.
    TopStatusStrip {
        id: topStrip
        visible: !games.transitioning && !games.coverGateLoading && games._tileLayout.showTopStrip
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: parent.top
        anchors.topMargin: Sizing.headerBottom + Sizing.pctH(1)
        height: games._tileLayout.showTopStrip ? Sizing.pctH(7) : 0
        title: {
            if (games._tileLayout.showHeaderTitleInHeader)
                return "";
            const sid = Browse.GamesModel.current_system_id;
            if (sid === "")
                return "";
            const idx = Browse.SystemsModel.index_for_system_id(sid);
            return idx >= 0 ? Browse.SystemsModel.system_name_at(idx) : sid;
        }
        currentPage: Math.floor(gamesGrid.currentIndex / games._browsePageSize)
        totalPages: games._tileLayout.showBottomStatusRow ? 1 : Math.max(1, Math.ceil((Browse.GamesModel.dir_count + Browse.GamesModel.total_files) / games._browsePageSize))
        totalText: games._listLayout || games._tileLayout.showBottomStatusRow ? "" : (Browse.GamesModel.total_files > 0 ? qsTr("%1 files").arg(Browse.GamesModel.total_files) : "")
        rightTextOverride: {
            if (!games._listLayout)
                return "";
            if (Browse.GamesModel.loading_more)
                return qsTr("Loading more…");
            if (gamesGrid.itemCount <= 0)
                return "";
            const total = Math.max(1, Browse.GamesModel.dir_count + Browse.GamesModel.total_files);
            return qsTr("%1 / %2").arg(gamesGrid.currentIndex + 1).arg(total);
        }
    }

    BrowseListDetailView {
        id: listCard

        visible: !games.transitioning && !games.coverGateLoading && games._listLayout
        anchors.left: parent.left
        anchors.leftMargin: Sizing.pctW(5)
        anchors.right: parent.right
        anchors.rightMargin: Sizing.pctW(5)
        anchors.top: topStrip.bottom
        anchors.topMargin: Sizing.pctH(2)
        anchors.bottom: parent.bottom
        anchors.bottomMargin: Sizing.pctH(8)
        model: Browse.GamesModel
        totalItemsOverride: Browse.GamesModel.dir_count + Browse.GamesModel.total_files
        targetVisibleRowCount: games._listPageSize
        showFileStem: true
        currentIndex: gamesGrid.currentIndex
        detailTitle: listCard.currentName
        detailCoverKey: Browse.GamesModel.current_detail_image_key !== "" ? Browse.GamesModel.current_detail_image_key : listCard.currentCoverKey
        detailShowDescription: false
        detailShowTitle: false
        detailTags: Browse.GamesModel.current_detail_tags
        detailLoading: Browse.GamesModel.current_detail_loading
        detailLoadingText: qsTr("Loading game…")
        detailCanPreviousImage: Browse.GamesModel.current_detail_image_can_prev
        detailCanNextImage: Browse.GamesModel.current_detail_image_can_next
        onItemHovered: index => games._focusIndex(index)
        onItemClicked: index => {
            games._focusIndex(index);
            games.handleAction("accept");
        }
        onItemRightClicked: index => {
            games._focusIndex(index);
            games.handleAction("write_card");
        }
        onEmptyRightClicked: games.handleAction("cancel")
        onPageWheelRequested: delta => games.handleAction(delta > 0 ? "page_next" : "page_prev")
        onVisibleChanged: {
            if (visible)
                Browse.GamesModel.load_description_at(gamesGrid.currentIndex);
        }
    }

    // Grid fills the safe zone between the top strip and the active
    // label. bottomMargin = MainLayout's instructionsBar height
    // (pctH(6)) + pctH(2) gap + the active label's pctH(7). If you
    // change the help-bar height or the label height, update this too.
    PagedGrid {
        id: gamesGrid

        visible: !games.transitioning && !games.coverGateLoading && !games._listLayout
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: topStrip.bottom
        anchors.bottom: parent.bottom
        anchors.bottomMargin: Sizing.pctH(6) + games._tileLayout.activeLabelBottomMargin + games._tileLayout.activeLabelHeight
        focused: games.gridFocused
        model: Browse.GamesModel
        layoutProfile: games._tileLayout
        delegate: Tile {
            layoutProfile: games._tileLayout
            showCaption: true
        }
        // Cover-art tiles run taller than systems logos, so a 5x3
        // layout starves vertical space. Games gets its own
        // gamesGridColumns/Rows in Sizing — 5x2 on desktop, narrower
        // branches at low resolutions match the systems grid logic.
        columnsOverride: Sizing.gamesGridColumns
        rowsOverride: Sizing.gamesGridRows
        // Stable scroll-thumb sizing while `fetch_more` grows the loaded
        // slice: feed PagedGrid the dataset's true total entry count
        // (same expression the top strip uses for `totalPages`).
        totalItemsOverride: Browse.GamesModel.dir_count + Browse.GamesModel.total_files
        // Drives the pending-target watchdog inside PagedGrid: while
        // this is true, an Up-at-page-1 (or Down-past-last-loaded)
        // wrap-target chains additional `loadMoreRequested` emissions
        // until the destination page lands. When it flips false with
        // a target still ahead of the loaded slice, the grid settles
        // on the loaded last item rather than spinning forever.
        hasMorePages: Browse.GamesModel.has_next_page
        onLoadMoreRequested: Browse.GamesModel.fetch_more()
        onCurrentIndexChanged: {
            if (games._listLayout)
                games._scheduleMetadataLoad(games._currentMoveIsRepeat);
        }
        // Cover prefetch is driven by what the user is looking at,
        // not by what metadata page Core happened to send back. Each
        // page turn re-anchors the queue so the visible page wins
        // LIFO drain order, with the next page warming behind it.
        onCurrentPageChanged: {
            const first = currentPage * pageSize;
            Browse.GamesModel.visible_first_row = first;
            if (!games._listLayout)
                Browse.GamesModel.prefetch_around(first);
        }
        onItemHovered: index => games._focusIndex(index)
        onItemClicked: index => {
            games._focusIndex(index);
            games.handleAction("accept");
        }
        onItemRightClicked: index => {
            games._focusIndex(index);
            games.handleAction("write_card");
        }
        onEmptyRightClicked: games.handleAction("cancel")
        onPageWheelRequested: delta => games.handleAction(delta > 0 ? "page_next" : "page_prev")
    }

    // Active game caption — single big line just under the grid. Same
    // typography as the top strip's title slot so the two big captions
    // read as a matched pair (top = system context, bottom = focused-
    // tile selection).
    ActiveLabel {
        id: activeLabel
        visible: !games.transitioning && !games.coverGateLoading && !games._listLayout
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.bottom: parent.bottom
        anchors.bottomMargin: games._tileLayout.activeLabelBottomMargin
        height: games._tileLayout.activeLabelHeight
        text: gamesGrid.itemCount > 0 ? Browse.GamesModel.name_at(gamesGrid.currentIndex) : ""
    }

    Text {
        id: bottomTotalText
        visible: games._tileLayout.showBottomStatusRow && !games.transitioning && !games.coverGateLoading && Browse.GamesModel.total_files > 0
        anchors.left: parent.left
        anchors.leftMargin: games._tileLayout.bottomStatusLeftMargin
        anchors.verticalCenter: activeLabel.verticalCenter
        width: Sizing.px(parent.width / 3) - games._tileLayout.bottomStatusLeftMargin
        height: Sizing.fontSize(2.9)
        elide: Text.ElideRight
        horizontalAlignment: Text.AlignLeft
        verticalAlignment: Text.AlignVCenter
        text: qsTr("%1 files").arg(Browse.GamesModel.total_files)
        font.family: Theme.fontUi
        font.pixelSize: Sizing.fontSize(2.9)
        color: Theme.textPrimary
        renderType: Text.NativeRendering
    }

    Text {
        visible: games._tileLayout.showBottomStatusRow && !games.transitioning && !games.coverGateLoading && Math.ceil((Browse.GamesModel.dir_count + Browse.GamesModel.total_files) / games._browsePageSize) > 1
        anchors.right: parent.right
        anchors.rightMargin: games._tileLayout.bottomStatusRightMargin
        anchors.verticalCenter: activeLabel.verticalCenter
        width: Sizing.px(parent.width / 3) - games._tileLayout.bottomStatusRightMargin
        height: Sizing.fontSize(2.9)
        elide: Text.ElideRight
        horizontalAlignment: Text.AlignRight
        verticalAlignment: Text.AlignVCenter
        text: qsTr("%1 / %2").arg(Math.floor(gamesGrid.currentIndex / games._browsePageSize) + 1).arg(Math.max(1, Math.ceil((Browse.GamesModel.dir_count + Browse.GamesModel.total_files) / games._browsePageSize)))
        font.family: Theme.fontUi
        font.pixelSize: Sizing.fontSize(2.9)
        color: Theme.textPrimary
        renderType: Text.NativeRendering
    }

    ScreenStateOverlay {
        x: games._listLayout ? listCard.x : gamesGrid.x
        y: games._listLayout ? listCard.y : gamesGrid.y
        width: games._listLayout ? listCard.width : gamesGrid.width
        height: games._listLayout ? listCard.height : gamesGrid.height
        loading: Browse.GamesModel.loading
        errorMessage: Browse.GamesModel.error_message ?? ""
        count: Browse.GamesModel.count
        emptyText: qsTr("No games in this system")
        loadingText: qsTr("Loading games…")
    }

    // In-flight pagination cue. Sits on the ActiveLabel row at the
    // same horizontal position as the grid's left content edge, so it
    // never overlaps the bottom row of tiles and reads as part of the
    // status band underneath the grid. Visible only when the user is
    // genuinely waiting on a fetch they triggered: a wrap-to-last,
    // shoulder-jump, or hold-Down past the loaded edge stashes a
    // pending target on PagedGrid, and that pending target is the
    // signal that "the press did something, we're working on it".
    // Background prefetches (look-ahead chunks the user hasn't
    // bumped into yet) keep `loading_more` true but leave
    // `hasPendingTarget` false, so the cue stays silent. Hidden
    // during transition or initial cover-gate so it never paints
    // over the global "Loading..." overlay.
    LoadingIndicator {
        id: pageLoadingCue
        visible: !games.transitioning && !games.coverGateLoading && !games._listLayout && Browse.GamesModel.loading_more && gamesGrid.hasPendingTarget
        anchors.left: activeLabel.left
        anchors.leftMargin: games._tileLayout.showBottomStatusRow && bottomTotalText.visible ? bottomTotalText.x + bottomTotalText.width + games._tileLayout.bottomStatusLeftMargin : gamesGrid.leftInset
        anchors.verticalCenter: activeLabel.verticalCenter
        text: qsTr("Loading more…")
    }
}
