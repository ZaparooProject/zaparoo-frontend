// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
// var-typed action/callback properties (acceptAction, cancelAction, gridMoveAction,
// linearMoveAction, etc.) and layout profile bindings on QVariant-typed objects
// cannot be statically typed. cxx-qt 0.8 singleton methods also trip "can be
// shadowed". Both are structural; suppress compiler category file-wide.
// qmllint disable compiler
pragma ComponentBehavior: Bound

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
    property bool detailShowDescription: true
    property bool detailShowTitle: true
    property string detailLoadingText: qsTr("Loading…")
    property bool detailCanPreviousImage: false
    property bool detailCanNextImage: false
    property bool detailReserveImageNav: false
    property var detailIdentityForIndex: null
    property var loadDetailForIndex: null
    // Optional immediate (non-debounced) detail hook. Defaults (when null) to
    // the model's `peek_detail_at`; GamesScreen overrides it with
    // `peek_description_at`. Keeps the detail table identity-correct the instant
    // the focus moves, so it never shows the previous row's metadata.
    property var peekDetailForIndex: null
    property var clearDetailAction: null
    property var retryAction: null
    property var acceptAction: null
    property var cancelAction: null
    property var gridMoveAction: null
    property var linearMoveAction: null
    property var pageAction: null
    property var onListLayoutEntered: null
    // Optional overrides for Left/Right in list layout. When null, the shell
    // pages a whole list page with the same ready-state guard as page_prev /
    // page_next. Grid layout never routes through these.
    property var listLeftAction: null
    property var listRightAction: null
    property var contextMenuEnabledAt: null
    property var restoreSelectionPath: null
    property var persistSelectionPath: null
    // Counterpart to `persistSelectionPath` for screens that debounce their
    // writes: drop anything already scheduled without committing it. Called
    // when a model replacement starts, where the grid's index-0 snap is
    // bookkeeping rather than a real selection -- see `_replacingModel`.
    property var discardSelectionPersist: null
    property var topStripTitleProvider: null
    property var topStripCurrentPageProvider: null
    property var topStripTotalPagesProvider: null
    property var topStripTotalTextProvider: null
    property var topStripRightTextProvider: null
    property var activeLabelTextProvider: null
    property var activeLabelTagsProvider: null
    property var gridCurrentPageChangedAction: null
    property var gridCurrentIndexChangedAction: null
    property var gridLoadMoreAction: null
    // Optional per-row label above cover art. Used only by mixed-system flat
    // views (Favorites/Recents); Games leaves it null.
    property var gridTileTopLabelProvider: null
    // Detail pane's "System" row (list layout) reuses the grid's own
    // per-index system-name lookup -- Recents/Favorites need only wire
    // `gridTileTopLabelProvider` once to get both. A screen wanting a
    // different value here than the grid label can still override it
    // explicitly. Games leaves both null, so it gets no System row --
    // correct, since it's already scoped to one system.
    property var detailSystemNameProvider: root.gridTileTopLabelProvider
    property string gridViewId: "gamesGrid"
    property string listViewId: "gamesList"
    property string tateListViewId: "gamesListTate"

    property alias mediaGrid: mediaGrid
    property alias topStrip: topStrip
    property alias listCard: listCard
    property alias activeLabel: activeLabel

    property bool transitioning: false
    property bool active: true
    // Router can suppress grid Image sources until one model/card frame has
    // painted. Defaults true for Favorites/Recents; Games drives it around
    // screen and folder navigation.
    property bool coverRevealReady: true
    property bool gridFocused: true
    property bool optimisticLoading: false
    // True while a jump-to-letter walk is loading the intervening pages. Folded
    // into `_loading()` so the standard centered loading cue paints over the
    // (stale) source page instead of leaving it frozen. Set/cleared by the
    // consumer that owns the jump (GamesScreen).
    property bool jumpLoading: false
    // False until the user takes control of focus (first input). Combined with
    // `_restoreDone` into `_focusReady`, which gates whether the grid tiles and
    // list rows render selection at all.
    property bool _focusArmed: false
    // Set true once the selection has been finalized from persisted state
    // (restoreSelection for favorites/recents; Main.qml's
    // `_setGamesRestoreIndex` for games). Combined with `_focusArmed` into
    // `_focusReady`, which gates whether the grid tiles and list rows render
    // selection at all - so the default index 0 never paints before restore
    // lands on a cold-start / forward entry.
    property bool _restoreDone: false
    readonly property bool _focusReady: root._focusArmed || root._restoreDone
    property bool detailRapidScrollActive: false
    property bool detailRapidIndicatorActive: detailRapidScrollActive
    property string detailRapidScrollAction: ""
    property bool pauseCoverRequestsDuringRapid: true
    property bool forceListLayout: false
    // Round 10: which of Settings' two independent layout preferences
    // this screen follows. "games" (default) covers Games/Favorites/
    // Recents; FavoriteSystemsScreen sets "systems" -- it's structurally
    // a MediaListScreen but semantically a systems screen (MainLayout.qml
    // groups it with Systems for the same reason). See `_listLayout`
    // below.
    property string layoutScope: "games"
    property bool renderGridLayout: true
    // Opt-in: allow the West-button page menu while the list is empty.
    // Default false so screens whose menu cannot empty the list keep the
    // stricter ready-only gate.
    property bool pageMenuEnabledWhenEmpty: false
    property bool showTopStrip: true
    property bool activeLabelAtBottom: false
    property bool suppressSelectionPersist: false
    // True for the duration of `prepareForModelReplacement()`. That call snaps
    // `currentIndex` to 0 synchronously, which fires `onCurrentIndexChanged` ->
    // `_persistFocus()` while the model still holds the OUTGOING rows -- so the
    // path read back is the previous scope's row 0, aimed at the incoming
    // scope's slot. `suppressSelectionPersist` can't cover this: the router
    // only sets it once rows have landed, several frames later.
    property bool _replacingModel: false
    property int gridBottomMargin: Sizing.pctH(15)
    property int activeLabelBottomMargin: 0
    property int activeLabelHeight: Sizing.pctH(7)
    property int bottomStatusLeftMargin: 0
    property int bottomStatusRightMargin: 0
    readonly property int _gridViewportWidth: Math.max(1, root.width)
    readonly property int _gridViewportHeight: Math.max(1, root.height - (topStrip.y + topStrip.height) - root.gridBottomMargin)
    readonly property var _gridViewportShape: Sizing.gamesGridShape(root._gridViewportWidth, root._gridViewportHeight)
    property int gridColumnsOverride: root._gridViewportShape.columns
    property int gridRowsOverride: root._gridViewportShape.rows
    // Media grids label every cover inside its tile. System grids opt out so
    // curated logos remain image-only and the focused name lives in ActiveLabel,
    // matching SystemsScreen.
    property bool gridShowCaption: true
    property bool pageLoadingVisible: false
    // Footer left-corner count text (e.g. "%1 games"). Empty (the
    // default -- Favorites/Recents leave it unset) simply leaves that
    // slot blank; there is no visibility gate to wire up. The right
    // corner is the built-in PageIndicator below, driven directly off
    // `mediaGrid`'s own page state -- no per-screen text override needed.
    property string bottomStatusLeftText: ""
    property int gridTotalItemsOverride: -1
    property bool gridHasMorePages: false
    property bool gridLoadingMore: false
    // False for cursor-based queries that cannot know their final page until
    // the cursor is exhausted. Hides a growing denominator while retaining
    // current-page text and the chevrons.
    property bool paginationTotalKnown: true
    readonly property bool _listRapidLineMove: root._listLayout && (root.detailRapidScrollAction === "up" || root.detailRapidScrollAction === "down")
    readonly property bool _showRapidScrollIndicator: root.detailRapidIndicatorActive && !root._listRapidLineMove
    readonly property bool _rapidSnapshotVisible: !root._gateHide && root._showRapidScrollIndicator && root._rapidSnapshotReady && mediaGrid.itemCount > 0 && !root._listLayout
    property var _rapidSnapshotResult: null
    property bool _rapidSnapshotReady: false
    property int _rapidSnapshotGeneration: 0
    readonly property bool _listLayout: root.forceListLayout || (root.layoutScope === "systems" ? Browse.Settings.current_systems_browse_layout : Browse.Settings.current_games_browse_layout) === "list"
    readonly property bool _tateListLayout: root._listLayout && Browse.Settings.current_orientation !== "horizontal"
    readonly property string _activeListViewId: root._tateListLayout ? root.tateListViewId : root.listViewId
    readonly property string _browseThemeId: BrowseLayouts.currentThemeId
    readonly property var _gridLayoutProfile: BrowseLayouts.themeProfile(root._browseThemeId, root.gridViewId)
    readonly property var _listLayoutProfile: BrowseLayouts.themeProfile(root._browseThemeId, root._activeListViewId)
    readonly property var _activeViewProfile: root._listLayout ? root._listLayoutProfile : root._gridLayoutProfile
    readonly property var _statusProfile: root._activeViewProfile && root._activeViewProfile.status ? root._activeViewProfile.status : null
    readonly property var _footerProfile: root._gridLayoutProfile && root._gridLayoutProfile.footer ? root._gridLayoutProfile.footer : null
    // CRT keeps the count + page cue in the footer (its top strip is
    // hidden entirely, `status.topStripVisible: false`); every other
    // theme hosts it up on the title line instead, alongside the count
    // badge -- see TopStatusStrip.qml's `pageIndicatorMode` and
    // BrowseLayouts.qml's `footer.pageCueInFooter`.
    readonly property bool _pageCueInFooter: !!(root._footerProfile && root._footerProfile.pageCueInFooter)
    readonly property bool _showGridPageCue: !root._listLayout && root.renderGridLayout && !root._pageCueInFooter
    // Round 11: list layout gets the same interactive chevron+"N / M"
    // PageIndicator grid layout already had, instead of a plain "Page N /
    // M" text or (on Games) an item-position counter with no chevrons at
    // all. Sibling to `_showGridPageCue` rather than folded into it so
    // that property's three existing (grid-only) use sites are untouched.
    readonly property bool _showListPageCue: root._listLayout && !root._pageCueInFooter
    // Whichever layout is active, is its page cue hosted in the top strip.
    readonly property bool _showTopPageCue: root._showGridPageCue || root._showListPageCue
    // List layout's own page size for paging math -- the number of rows
    // actually on screen at once, same value `listCard.visibleRowCount`
    // resolves to (a screen's own `targetVisibleRowCount`, when set, is
    // exactly what that resolves to, so this stays correct for every
    // MediaListScreen subclass without needing a per-screen override).
    readonly property int _listVisiblePageSize: Math.max(1, listCard.visibleRowCount)
    // Mirrors PagedGrid.qml's own `totalItems`/`totalPageCount`/
    // `hasPagesBelow` formulas (see that file's doc comments), substituting
    // the list's own page size and `mediaGrid.itemCount`/`currentIndex` —
    // both tracked identically regardless of which layout is visible (see
    // `listCard.currentIndex: mediaGrid.currentIndex` below).
    readonly property int _listTotalItems: root.paginationTotalKnown && root.gridTotalItemsOverride >= 0 ? root.gridTotalItemsOverride : mediaGrid.itemCount
    readonly property int _listTotalPageCount: Math.max(1, Math.ceil(root._listTotalItems / root._listVisiblePageSize))
    readonly property int _listCurrentPage: Math.floor(mediaGrid.currentIndex / root._listVisiblePageSize)
    readonly property bool _listHasPagesAbove: root._listCurrentPage > 0
    readonly property bool _listHasPagesBelow: root._listCurrentPage < root._listTotalPageCount - 1 || root.gridHasMorePages
    // Round 11: measure the footer's corner occupants (the left count text,
    // the right PageIndicator) instead of reserving a flat third of the
    // screen for each — see SystemsScreen.qml's identical treatment and
    // ActiveLabel.qml's own name/tags measurement for the same idiom.
    readonly property int _bottomStatusLeftTextWidth: Math.ceil(Math.max(bottomTotalTextMetrics.advanceWidth, bottomTotalTextMetrics.boundingRect.width) + (Sizing.tier === "240" ? 0 : Sizing.px(2)))
    readonly property int _footerLeftInset: root._bottomStatusLeftTextWidth + root.bottomStatusLeftMargin
    readonly property int _resolvedBottomStatusRightMargin: root._footerProfile ? root._footerProfile.bottomStatusRightMargin : root.bottomStatusRightMargin
    readonly property int _footerRightInset: footerPageIndicator.width + root._resolvedBottomStatusRightMargin
    readonly property int _listOverlayBottomMargin: root._listLayoutProfile && root._listLayoutProfile.list ? root._listLayoutProfile.list.overlayBottomMargin : Sizing.pctH(15)
    // Hide list/grid content as soon as the model enters Loading, but
    // let ScreenStateOverlay keep delaying the centered cue. Keep the
    // content hidden through the cue's minimum-visible tail so newly
    // loaded rows cannot paint underneath lingering loading text.
    readonly property bool _overlayLoadingVisible: stateOverlay.loadingVisible
    readonly property bool _gateHide: root.transitioning || root._loading() || root._overlayLoadingVisible || root._errorMessage() !== ""

    signal requestHubScreen
    signal requestContextMenu(int index, var anchorRect, int anchorRadius)
    // Page-scoped operations entry point (West button). The router decides
    // what the menu contains; the screen just reports the press when the list
    // is in a usable state.
    signal requestPageMenu

    Connections {
        target: root.mediaModel

        function onLoadingChanged(): void {
            if (!root.mediaModel || !root.mediaModel.loading)
                return;
            // Drop any write already scheduled for the outgoing scope's slot
            // before the index snap can queue another one. Both halves matter:
            // a debounce armed by the user's last move is still in flight on
            // the Accept/Cancel paths that don't flush, and the snap itself
            // would arm a fresh one from stale rows.
            if (typeof root.discardSelectionPersist === "function")
                root.discardSelectionPersist();
            root._replacingModel = true;
            mediaGrid.prepareForModelReplacement();
            root._replacingModel = false;
        }
    }

    on_ListLayoutChanged: {
        if (!root._listLayout)
            return;
        if (typeof root.onListLayoutEntered === "function")
            root.onListLayoutEntered();
        focusedDetail.requestNow();
    }

    // Layout-aware pulse routing. In grid layout this forwards to the
    // PagedGrid tile; in list layout it increments the BrowseListDetailView
    // pulse so the selected row fires its push-in. The same push-in cue
    // serves both forward navigation and game launch.
    // Capture during the d-pad hold delay, before rapid mode suspends live tile
    // delegates. Retaining the grab result keeps its itemgrabber URL alive.
    // Generation check drops a late callback after direction/layout changes.
    function prepareRapidSnapshot(): void {
        root._rapidSnapshotGeneration++;
        const generation = root._rapidSnapshotGeneration;
        root._rapidSnapshotReady = false;
        root._rapidSnapshotResult = null;
        if (root._listLayout || !mediaGrid.visible || mediaGrid.itemCount <= 0)
            return;
        mediaGrid.grabToImage(function (result) {
            if (generation !== root._rapidSnapshotGeneration)
                return;
            root._rapidSnapshotResult = result;
            root._rapidSnapshotReady = true;
        }, Qt.size(mediaGrid.width, mediaGrid.height));
    }

    function pulseActivate(): void {
        if (root._listLayout)
            listCard.activatePulse++;
        else
            mediaGrid.pulseActivate();
    }

    // Settle the push-in cue back to rest after a launch that keeps the
    // frontend on the same screen (a launcher that does not take the FPGA or
    // quit us, e.g. an Audio track). Routed like pulseActivate to whichever
    // layout is live. Not called for forward navigation, which transitions the
    // screen and resets the cue off-screen on its own.
    function releaseActivate(): void {
        if (root._listLayout)
            listCard.releasePulse++;
        else
            mediaGrid.releaseActivate();
    }

    function _count(): int {
        return root.mediaModel !== null ? root.mediaModel.count : 0;
    }

    function _loading(): bool {
        return root.optimisticLoading || root.jumpLoading || (root.mediaModel !== null ? root.mediaModel.loading : false);
    }

    function _errorMessage(): string {
        return root.mediaModel !== null ? (root.mediaModel.error_message ?? "") : "";
    }

    function _detailImageKey(): string {
        return root.mediaModel !== null ? (root.mediaModel.current_detail_image_key ?? "") : "";
    }

    // Default footer-label dim suffix when the screen doesn't supply its
    // own `activeLabelTagsProvider` (Games/Favorites/Recents). Composes
    // the same folder-count suffix the grid tiles and list rows show
    // (Format.rowSuffix) from three index-based lookups, each guarded so
    // a model missing the invokable (a plain ListModel in tests, or a
    // real model that's never had a folder row) just no-ops back to plain
    // disambiguating tags -- see games.rs's `entry_type_at`/
    // `file_count_at`, the only model that currently has folder rows.
    function _defaultActiveLabelTags(): string {
        if (mediaGrid.itemCount <= 0 || root.mediaModel === null)
            return "";
        // Same dependency the text provider needs and for the same reason (see
        // GamesScreen's `activeLabelTextProvider`): all three lookups below are
        // #[qinvokable] methods, so a folder swap that replaces rows in place
        // and lands on the same index with the same count changes nothing this
        // binding tracks. The name half was fixed and this half was not, which
        // left the new folder's name rendering beside the old folder's item
        // count. Guarded because only GamesModel carries a revision counter --
        // Favorites/Recents and the plain ListModels in tests do not.
        if (root.mediaModel.rows_revision !== undefined)
            void root.mediaModel.rows_revision;
        const index = mediaGrid.currentIndex;
        const tags = typeof root.mediaModel.disambiguating_tags_at === "function" ? root.mediaModel.disambiguating_tags_at(index) : "";
        const entryType = typeof root.mediaModel.entry_type_at === "function" ? root.mediaModel.entry_type_at(index) : "media";
        const fileCount = typeof root.mediaModel.file_count_at === "function" ? root.mediaModel.file_count_at(index) : 0;
        return Format.rowSuffix(entryType, tags, fileCount);
    }

    // Prepends a "System" row ahead of the model's own tag rows when a
    // `detailSystemNameProvider` is wired (Recents/Favorites) -- composed
    // here in QML, not in Rust, so it survives states where the model's own
    // `current_detail_tags` blanks out entirely (metadata miss/error): the
    // system name is locally known and doesn't depend on that fetch.
    function _detailTags(): string {
        const base = root.mediaModel !== null ? (root.mediaModel.current_detail_tags ?? "") : "";
        if (typeof root.detailSystemNameProvider !== "function")
            return base;
        const sys = root.detailSystemNameProvider(mediaGrid.currentIndex) ?? "";
        if (sys === "")
            return base;
        const row = "System\t" + sys;
        return base !== "" ? row + "\n" + base : row;
    }

    function _detailLoading(): bool {
        return root.mediaModel !== null ? root.mediaModel.current_detail_loading : false;
    }

    function restoreSelection(): void {
        if (root._count() <= 0)
            return;
        const path = typeof root.restoreSelectionPath === "function" ? (root.restoreSelectionPath() ?? "") : (root.mediaState !== null ? (root.mediaState.selected_path ?? "") : "");
        const idx = path !== "" ? root.mediaModel.index_for_path(path) : -1;
        // A saved path can disappear or belong to an unloaded/changed scope.
        // Never leave its old numeric index behind: seat focus on the first
        // visible item unless the saved item exists in the current model.
        mediaGrid.setCurrentIndexImmediate(idx >= 0 ? idx : 0);
        // Selection is finalized; let tiles/rows render focus only after the
        // valid index above has landed.
        root._restoreDone = true;
    }

    function _persistFocus(): void {
        if (root.suppressSelectionPersist || root._replacingModel || root.mediaModel === null)
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
        root._focusArmed = true;
        mediaGrid.currentIndex = index;
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
        if (next >= count - 2)
            root.mediaModel.fetch_more();
    }

    function _coverRefreshFirstRow(): int {
        if (!root._listLayout)
            return mediaGrid.currentPage * mediaGrid.pageSize;
        return Math.max(0, mediaGrid.currentIndex - listCard.visibleRowCount);
    }

    function _coverRefreshRowCount(): int {
        if (!root._listLayout)
            return mediaGrid.pageSize * 2;
        return Math.max(1, listCard.visibleRowCount * 3);
    }

    function _resumeCoverRequests(): void {
        if (root.mediaModel === null)
            return;
        if (root.pauseCoverRequestsDuringRapid)
            root.mediaModel.cover_requests_paused = false;
        if (typeof root.mediaModel.refresh_cover_keys === "function")
            root.mediaModel.refresh_cover_keys(root._coverRefreshFirstRow(), root._coverRefreshRowCount());
        if (!root._listLayout && typeof root.gridCurrentPageChangedAction === "function")
            root.gridCurrentPageChangedAction();
    }

    function _pauseCoverRequests(): void {
        if (root.mediaModel === null || !root.pauseCoverRequestsDuringRapid)
            return;
        root.mediaModel.cover_requests_paused = true;
        if (typeof root.mediaModel.clear_pending_cover_requests === "function")
            root.mediaModel.clear_pending_cover_requests();
    }

    onDetailRapidScrollActiveChanged: {
        if (root.detailRapidScrollActive)
            root._pauseCoverRequests();
        else
            root._resumeCoverRequests();
    }

    function _performPage(delta: int): void {
        if (typeof root.pageAction === "function") {
            root.pageAction(delta);
            return;
        }
        if (root._listLayout) {
            root._performLinearMove(delta * root._listVisiblePageSize);
            return;
        }
        mediaGrid.pageBy(delta);
    }

    function _state(): string {
        if (root._loading() || root._overlayLoadingVisible)
            return "loading";
        if (root._errorMessage() !== "")
            return "error";
        if (root._count() === 0)
            return "empty";
        return "ready";
    }

    function handleAction(action: string): void {
        if ((action === "left" || action === "right" || action === "up" || action === "down" || action === "context_menu") && root._gateHide)
            return;

        root._focusArmed = true;

        if (action === "left") {
            if (root._listLayout && typeof root.listLeftAction === "function")
                root.listLeftAction();
            else if (root._listLayout && root._state() === "ready")
                root._performPage(-1);
            else if (!root._listLayout && typeof root.gridMoveAction === "function")
                root.gridMoveAction(-1, 0);
            else if (!root._listLayout)
                mediaGrid.moveSelection(-1, 0);
        } else if (action === "right") {
            if (root._listLayout && typeof root.listRightAction === "function")
                root.listRightAction();
            else if (root._listLayout && root._state() === "ready")
                root._performPage(1);
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
        } else if (action === "page_menu") {
            // Screens whose page menu can itself cause the empty state (a
            // filter that matches nothing) must stay reachable while empty,
            // or the user is locked out of the only way to clear it.
            if (root._state() === "ready" || (root.pageMenuEnabledWhenEmpty && root._state() === "empty"))
                root.requestPageMenu();
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
            if (typeof root.acceptAction === "function") {
                root.acceptAction(mediaGrid.currentIndex);
            } else {
                root.pulseActivate();
                defaultLaunchCommit._idx = mediaGrid.currentIndex;
                defaultLaunchCommit.arm();
            }
        } else if (action === "context_menu") {
            if (mediaGrid.itemCount > 0) {
                const idx = mediaGrid.currentIndex;
                if (typeof root.contextMenuEnabledAt === "function" && !root.contextMenuEnabledAt(idx))
                    return;
                root._persistFocus();
                const rect = root._listLayout ? listCard.currentCellRectIn(root) : mediaGrid.currentCellRectIn(root);
                const radius = root._listLayout ? listCard.currentCellRadius : mediaGrid.currentCellRadius;
                root.requestContextMenu(idx, rect, radius);
            }
        } else if (action === "cancel") {
            if (typeof root.cancelAction === "function")
                root.cancelAction();
            else
                root.requestHubScreen();
        }
    }

    // Defers the default launch_at call so the push-in cue (pulseActivate)
    // completes on a static scene before Core takes the FPGA.
    // Only used when no `acceptAction` override is provided (Favorites,
    // Recents). GamesScreen owns its own pressCommit with folder routing.
    DeferredAction {
        id: defaultLaunchCommit
        property int _idx: -1
        onDeferred: {
            const i = _idx;
            _idx = -1;
            if (i >= 0 && root.mediaModel !== null) {
                root.mediaModel.launch_at(i);
                // Settle the push-in back to rest. If the launch takes the FPGA
                // or kills us the release is never seen; if it stays on the page
                // (e.g. an Audio track) the cue does not stick pushed in.
                root.releaseActivate();
            }
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
        peekForIndex: root.peekDetailForIndex
        clearDetail: root.clearDetailAction
        mediaModel: root.mediaModel
    }

    TopStatusStrip {
        id: topStrip
        visible: !root._gateHide && (root._statusProfile ? root._statusProfile.topStripVisible : root.showTopStrip)
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: parent.top
        anchors.topMargin: Sizing.headerBottom + (root._statusProfile ? root._statusProfile.topMargin : Sizing.pctH(1))
        height: root._statusProfile ? root._statusProfile.stripHeight : (root.showTopStrip ? Sizing.pctH(7) : 0)
        slotMargin: root._statusProfile ? root._statusProfile.slotMargin : Sizing.pctW(3)
        title: typeof root.topStripTitleProvider === "function" ? root.topStripTitleProvider() : root.screenTitle
        currentPage: typeof root.topStripCurrentPageProvider === "function" ? root.topStripCurrentPageProvider() : (root._listLayout ? root._listCurrentPage : mediaGrid.currentPage)
        totalPages: typeof root.topStripTotalPagesProvider === "function" ? root.topStripTotalPagesProvider() : (root._listLayout ? root._listTotalPageCount : Math.max(1, Math.ceil(root._count() / mediaGrid.pageSize)))
        pageTotalKnown: root.paginationTotalKnown
        // List layout keeps each screen's own provider (or the generic
        // fallback) exactly as before. Grid layout shows the same count
        // here too now, UNLESS the theme keeps it in the footer instead
        // (`_pageCueInFooter` -- CRT, whose top strip is hidden entirely
        // anyway).
        // Generic fallback must guard on `paginationTotalKnown` same as
        // `rightTextOverride` below does -- `_count()` is rows-loaded-so-far
        // for a cursor-paginated model (Recents/Favorites), not a stable
        // total, and would otherwise grow visibly as the user scrolls.
        // Screens with a real total (Games/Favorite Systems) supply their
        // own `topStripTotalTextProvider` and never reach this branch.
        totalText: (root._listLayout || root._showGridPageCue) ? (typeof root.topStripTotalTextProvider === "function" ? root.topStripTotalTextProvider() : (root.paginationTotalKnown && root._count() > 0 ? qsTr("%1 games").arg(root._count()) : "")) : ""
        // Round 11: list layout now mounts the same interactive
        // PageIndicator grid layout does (`pageIndicatorMode` below), so a
        // per-screen provider returning the old "N / M items" position
        // counter would just never be shown -- those screens dropped it.
        // A provider returning something else (GamesScreen's transient
        // "Loading more…" while a background fetch fills in scrolled-past
        // rows) still needs to be seen, though, so `pageIndicatorMode`
        // yields the slot back to this plain text for as long as it's
        // non-empty rather than only ever showing while grid/list is idle.
        rightTextOverride: typeof root.topStripRightTextProvider === "function" ? root.topStripRightTextProvider() : ""
        showPageCounter: root._listLayout || root._showGridPageCue
        pageIndicatorMode: root._showTopPageCue && topStrip.rightTextOverride === ""
        pageIndicatorChevronSize: root._gridLayoutProfile && root._gridLayoutProfile.grid ? root._gridLayoutProfile.grid.pageChevronSize : Sizing.pctH(4)
        hasPagesAbove: root._listLayout ? root._listHasPagesAbove : mediaGrid.hasPagesAbove
        hasPagesBelow: root._listLayout ? root._listHasPagesBelow : mediaGrid.hasPagesBelow
        onPageRequested: delta => root._performPage(delta)
    }

    BrowseListDetailView {
        id: listCard

        visible: !root._gateHide && root._listLayout
        anchors.left: parent.left
        anchors.leftMargin: root._listLayoutProfile && root._listLayoutProfile.list ? root._listLayoutProfile.list.cardSideMargin : Sizing.pctW(3)
        anchors.right: parent.right
        anchors.rightMargin: root._listLayoutProfile && root._listLayoutProfile.list ? root._listLayoutProfile.list.cardSideMargin : Sizing.pctW(3)
        anchors.top: topStrip.bottom
        anchors.topMargin: root._listLayoutProfile && root._listLayoutProfile.list ? root._listLayoutProfile.list.cardTopMargin : Sizing.pctH(2)
        anchors.bottom: parent.bottom
        anchors.bottomMargin: root._listLayoutProfile && root._listLayoutProfile.list ? root._listLayoutProfile.list.cardBottomMargin : Sizing.pctH(8)
        layoutProfile: root._listLayoutProfile
        model: root.mediaModel
        totalItemsOverride: root.totalItemsOverride
        targetVisibleRowCount: root.targetVisibleRowCount
        currentIndex: mediaGrid.currentIndex
        focusReady: root._focusReady
        detailTitle: listCard.currentName
        detailIdentity: focusedDetail.currentIdentity
        // Detail pane is not painted in grid layout. Withholding its source
        // avoids a hidden width-constrained decode competing with visible
        // height-constrained tile covers.
        detailCoverKey: !root._listLayout ? "" : (root.detailRapidScrollActive ? root.detailPlaceholderKey : (root._detailImageKey() !== "" ? root._detailImageKey() : listCard.currentCoverKey))
        detailShowDescription: root.detailShowDescription
        detailShowTitle: root.detailShowTitle
        detailTags: root._detailTags()
        detailLoading: root._detailLoading()
        detailSuppressed: root.detailRapidScrollActive
        screenSettling: !root.active
        detailLoadingText: root.detailLoadingText
        detailCanPreviousImage: root.detailCanPreviousImage
        detailCanNextImage: root.detailCanNextImage
        detailReserveImageNav: root.detailReserveImageNav
        onItemHovered: index => root._focusIndex(index)
        onItemClicked: index => {
            root._focusIndex(index);
            root.handleAction("accept");
        }
        onItemRightClicked: index => {
            root._focusIndex(index);
            root.handleAction("context_menu");
        }
        onEmptyRightClicked: root.handleAction("cancel")
        onPageWheelRequested: delta => root.handleAction(delta > 0 ? "page_next" : "page_prev")
    }

    PagedGrid {
        id: mediaGrid

        suspendDelegates: root._listLayout
        visible: !root._gateHide && !root._listLayout && root.renderGridLayout
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: topStrip.bottom
        anchors.bottom: parent.bottom
        anchors.bottomMargin: root.gridBottomMargin
        focused: root.gridFocused
        screenSettling: !root.active
        focusReady: root._focusReady
        model: root.mediaModel
        delegate: Tile {
            layoutProfile: root._gridLayoutProfile
            showCaption: root.gridShowCaption
            coverSourceSize: Sizing.gamesGridCoverSourceSize(root._gridViewportWidth, root._gridViewportHeight)
        }
        layoutProfile: root._gridLayoutProfile
        columnsOverride: root.gridColumnsOverride
        rowsOverride: root.gridRowsOverride
        totalItemsOverride: root.gridTotalItemsOverride
        hasMorePages: root.gridHasMorePages
        loadingMore: root.gridLoadingMore
        paginationTotalKnown: root.paginationTotalKnown
        tileTopLabelProvider: root.gridTileTopLabelProvider
        coverRequestsEnabled: root.coverRevealReady
        coverLoadingPaused: root.detailRapidScrollActive
        cellsVisible: !root._rapidSnapshotVisible
        rapidRenderMode: root.detailRapidScrollActive && root._rapidSnapshotReady
        onLoadMoreRequested: urgent => {
            if (typeof root.gridLoadMoreAction === "function")
                root.gridLoadMoreAction(urgent);
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
            root.handleAction("context_menu");
        }
        onEmptyRightClicked: root.handleAction("cancel")
        onPageWheelRequested: delta => root.handleAction(delta > 0 ? "page_next" : "page_prev")
    }

    // Footer row — active item caption. The count badge and page cue sit
    // up on the top strip now instead (`_showGridPageCue` above) except on
    // CRT, whose top strip is hidden entirely -- CRT keeps them down here,
    // `_pageCueInFooter`. Everywhere else this is a title-only row, so
    // `sideInset` reverts to ActiveLabel's own default margin instead of
    // yielding a third of the width to corner slots that are empty here
    // now.
    ActiveLabel {
        id: activeLabel
        visible: !root._gateHide && !root._listLayout && root.renderGridLayout && !root.pageLoadingVisible
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: root.activeLabelAtBottom ? undefined : mediaGrid.bottom
        anchors.bottom: root.activeLabelAtBottom ? parent.bottom : undefined
        anchors.bottomMargin: root.activeLabelAtBottom ? root.activeLabelBottomMargin : 0
        height: root.activeLabelHeight
        sideInset: root._pageCueInFooter ? Math.max(root._footerLeftInset, root._footerRightInset) : Sizing.pctW(3)
        text: typeof root.activeLabelTextProvider === "function" ? root.activeLabelTextProvider() : (mediaGrid.itemCount > 0 ? root.mediaModel.name_at(mediaGrid.currentIndex) : "")
        // Full (untrimmed) disambiguation tokens for the focused item, shown as a
        // dim suffix. Uses an explicit provider when given, else the model's
        // disambiguating_tags_at; guarded so a plain ListModel (tests) is safe.
        tags: typeof root.activeLabelTagsProvider === "function" ? root.activeLabelTagsProvider() : root._defaultActiveLabelTags()
    }

    TextMetrics {
        id: bottomTotalTextMetrics
        text: root.bottomStatusLeftText
        font.family: Theme.fontUi
        font.pixelSize: Sizing.fontSection
    }

    // Round 11: both this and `footerPageIndicator` below used to be
    // grid-only (`!root._listLayout`). On the CRT profile (`_pageCueInFooter`,
    // top strip hidden entirely) that left list layout with no page cue
    // anywhere at all. `bottomStatusLeftText` is already layout-agnostic
    // content (a screen's own total-count text, e.g. GamesScreen's "%1
    // games"), so showing it for list layout too needs no new plumbing.
    Text {
        id: bottomTotalText
        visible: !root._gateHide && root._pageCueInFooter && root.bottomStatusLeftText !== ""
        anchors.left: parent.left
        anchors.leftMargin: root.bottomStatusLeftMargin
        anchors.verticalCenter: activeLabel.verticalCenter
        width: root._bottomStatusLeftTextWidth
        height: Sizing.fontSection
        elide: Text.ElideRight
        horizontalAlignment: Text.AlignLeft
        verticalAlignment: Text.AlignVCenter
        text: root.bottomStatusLeftText
        font.family: Theme.fontUi
        font.pixelSize: Sizing.fontSection
        color: Theme.textPrimary
        renderType: Text.NativeRendering
    }

    PageIndicator {
        id: footerPageIndicator
        objectName: "mediaListFooterPageIndicator"
        visible: !root._gateHide && root._pageCueInFooter && (root._listLayout || root.renderGridLayout)
        anchors.right: parent.right
        anchors.rightMargin: root._resolvedBottomStatusRightMargin
        anchors.verticalCenter: activeLabel.verticalCenter
        chevronSize: root._gridLayoutProfile && root._gridLayoutProfile.grid ? root._gridLayoutProfile.grid.pageChevronSize : Sizing.pctH(4)
        currentPage: root._listLayout ? root._listCurrentPage : mediaGrid.currentPage
        totalPages: root._listLayout ? root._listTotalPageCount : mediaGrid.totalPageCount
        pageTotalKnown: root.paginationTotalKnown
        hasPagesAbove: root._listLayout ? root._listHasPagesAbove : mediaGrid.hasPagesAbove
        hasPagesBelow: root._listLayout ? root._listHasPagesBelow : mediaGrid.hasPagesBelow
        onPageRequested: delta => root._performPage(delta)
    }

    // Pagination loading replaces the focused name instead of painting beside
    // or over it. The wrapper uses the same symmetric safe slot ActiveLabel
    // does, keeping the cue clear of count and page chrome.
    Item {
        visible: !root._gateHide && root.pageLoadingVisible
        anchors.left: parent.left
        anchors.leftMargin: activeLabel.sideInset
        anchors.right: parent.right
        anchors.rightMargin: activeLabel.sideInset
        anchors.verticalCenter: activeLabel.verticalCenter
        height: activeLabel.height
        clip: true

        LoadingIndicator {
            anchors.centerIn: parent
        }
    }

    // Frozen rapid-navigation presentation. The live grid is hidden while this
    // is visible, so the dim Image blends once against an opaque black backing
    // rather than compositing over moving delegates on every repeat tick.
    //
    // Sink is the FULL grid rect, matching `prepareRapidSnapshot()`'s grab
    // (`Qt.size(mediaGrid.width, mediaGrid.height)`) exactly, and
    // `fillMode: Image.Pad` paints the grabbed pixmap at its own natural
    // size with no resampling -- a true 1:1 overlay. Round 8 sized this
    // Item to the smaller *content* rect (`_contentWidth` x the cell
    // block's own height) and relied on `sourceClipRect` to crop the
    // full-rect grab down to it, with `fillMode: Image.Stretch` filling
    // whatever `sourceClipRect` returned into that smaller box.
    // `sourceClipRect` is a `QQuickPixmap` region hint that is not
    // reliably honored for `image://itemgrabber/…` provider URLs -- when
    // dropped, `Stretch` squeezed the entire full-size grab (narrower by
    // `leftInset + rightInset`, shorter by `topInset + bottomInset`) into
    // the smaller box, with `smooth: false` nearest-neighbour on top --
    // exactly the "ugly, scaled smaller" artifact. The crop origin also
    // assumed cells start at `(leftInset, topInset)`, ignoring
    // `_cellBlockOffsetX`/`_cellBlockOffsetY` (PagedGrid.qml), the
    // centering offset applied whenever the cell block is smaller than the
    // available area -- so even a correctly-honored clip would have been
    // shifted. Showing the whole full-rect grab at 1:1 sidesteps both: no
    // scaling path exists to fail, and the offset is already baked into
    // where the grab itself painted the cells.
    Item {
        id: rapidSnapshot
        objectName: "rapidScrollSnapshot"
        visible: root._rapidSnapshotVisible
        x: mediaGrid.x
        y: mediaGrid.y
        width: mediaGrid.width
        height: mediaGrid.height
        clip: true
        z: 19

        Rectangle {
            anchors.fill: parent
            color: Theme.bgBar
        }

        Image {
            objectName: "rapidScrollSnapshotImage"
            anchors.fill: parent
            source: root._rapidSnapshotResult ? root._rapidSnapshotResult.url : ""
            fillMode: Image.Pad
            smooth: false
            opacity: 0.28
        }
    }

    RapidScrollIndicator {
        visible: !root._gateHide && root._showRapidScrollIndicator && mediaGrid.itemCount > 0 && !root._listLayout
        x: Sizing.center(parent.width, width)
        y: Sizing.center(mediaGrid.height, height) + mediaGrid.y
        title: typeof root.activeLabelTextProvider === "function" ? root.activeLabelTextProvider() : (mediaGrid.itemCount > 0 && root.mediaModel !== null ? root.mediaModel.name_at(mediaGrid.currentIndex) : "")
        z: 20
    }

    ScreenStateOverlay {
        id: stateOverlay

        x: root._listLayout ? listCard.x : mediaGrid.x
        y: root._listLayout ? listCard.y : mediaGrid.y
        width: root._listLayout ? listCard.width : mediaGrid.width
        height: root._listLayout ? Math.max(0, root.height - listCard.y - root._listOverlayBottomMargin) : mediaGrid.height
        // Content rect starts below the header, so recenter on the full
        // screen (which matches `scene`, the global transition cue's new
        // parent) rather than this rect's own smaller height — otherwise
        // the loading cue jumps up when the global cue hands off here.
        cueCenterY: root.height / 2 - y
        enabled: true
        loading: root._loading()
        errorMessage: root._errorMessage()
        count: root._count()
        emptyText: root.emptyText
        loadingText: root.loadingText
    }

    // Adjacent-cover preload pool. While the user dwells on a list row
    // these hidden Images decode the next and previous rows' covers into
    // Qt's pixmap cache at the same sourceSize as the visible detail cover
    // (the shared Sizing.detailCoverSourceWidth tier), so the detail cover switch on a d-pad move is a
    // synchronous cache hit rather than an async decode. This is media art
    // from Core, which is a real decode and genuinely needs warming — bundled
    // artwork does not, since it tints synchronously out of the baked atlas.
    // Active only in list layout; in grid layout there is no per-row detail pane.
    // The source guard (`k.startsWith("media-image/")`) keeps the source
    // empty for placeholder keys (`icons/*`) so no decode work is done for
    // folder entries or cold-cache neighbors that haven't resolved yet.
    Image {
        id: prefetchNextCover
        visible: false
        width: 0
        height: 0
        asynchronous: true
        cache: true
        sourceSize.width: Sizing.detailCoverSourceWidth
        source: {
            if (!root._listLayout || root.mediaModel === null)
                return "";
            const k = root.mediaModel.detail_prefetch_key_next ?? "";
            return k.startsWith("media-image/") ? Resources.coverUrl(k, Theme.logoFocusPrimary, Theme.logoFocusSecondary, Theme.logoFocusShadow) : "";
        }
    }

    Image {
        id: prefetchPrevCover
        visible: false
        width: 0
        height: 0
        asynchronous: true
        cache: true
        sourceSize.width: Sizing.detailCoverSourceWidth
        source: {
            if (!root._listLayout || root.mediaModel === null)
                return "";
            const k = root.mediaModel.detail_prefetch_key_prev ?? "";
            return k.startsWith("media-image/") ? Resources.coverUrl(k, Theme.logoFocusPrimary, Theme.logoFocusSecondary, Theme.logoFocusShadow) : "";
        }
    }
}
