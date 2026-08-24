// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import Zaparoo.Theme
import Zaparoo.Ui
import Zaparoo.Browse as Browse

// cxx-qt 0.8 patches `isFinal: true` on singleton properties but the
// qmltypes schema has no `isFinal` slot for Method, so every qinvokable
// call on a Zaparoo.Browse singleton (system_id_at, set_system, etc.)
// still trips qmllint's "Member can be shadowed" check. Until the
// schema grows method-level finality, suppress the compiler category
// file-wide.
// qmllint disable compiler

// Systems screen — paged grid driven by `Browse.SystemsModel`. Pure
// input dispatcher: emits `requestAccept(systemId)` on Accept (with
// "" payload to signal Empty/Error retry intent),
// `requestContextMenu(index, anchorRect)` on the context-menu action, and
// `requestHubScreen()` on Escape. Cross-screen orchestration (model
// fills, transition overlay, screen flip) lives in Main.qml;
// `transitioning` is written by the router so the grid hides during
// the loading wait.
Item {
    id: systems

    property alias systemsGrid: systemsGrid
    property alias listCard: listCard
    property alias activeLabel: activeLabel
    property alias topStrip: topStrip
    property bool transitioning: false
    // True while Hub→Systems routing is preparing this destination, before
    // the delayed loading cue becomes visible. Used only to suspend hidden
    // delegates; source-screen hiding still follows `transitioning`.
    property bool preparingTransition: false
    // Set false by MainLayout when this screen is not the active screen.
    // Forwarded to systemsGrid.screenSettling so tile delegates reset
    // their held activation cue off-screen.
    property bool active: true
    // Router-driven flag: `MainLayout` writes this to
    // `!ScreenManager.hasModal` so the focused tile's accent ring
    // hides while a modal (the context menu) is on top of the stack.
    // Two competing focus rings — one on the menu's selected entry
    // and one on the anchored tile — read as ambiguous; suppressing
    // the tile ring keeps a single visible focus indicator at all
    // times. The ring restores automatically when the modal pops.
    property bool gridFocused: true
    property bool optimisticLoading: false
    // False until the user takes control of focus (first input). Combined
    // with `_restoreDone` into `_focusReady`, which gates whether the grid
    // tiles render focus at all.
    property bool _focusArmed: false
    // Set true once the load-time system restore has run (see Main.qml's
    // `_restoreSystemsScreenSelection`). Combined with `_focusArmed` into
    // `_focusReady`, which gates whether the grid tiles render focus at all —
    // so the grid's default tile 0 never paints a ring during the window
    // before restore points `currentIndex` at the saved system on a cold start.
    property bool _restoreDone: false
    readonly property bool _focusReady: systems._focusArmed || systems._restoreDone
    readonly property bool _listLayout: Browse.Settings.current_systems_browse_layout === "list"
    readonly property bool _crtGridLayout: Theme.crtNativePath && !systems._listLayout
    readonly property bool _crtListStrip: Theme.crtNativePath && systems._listLayout
    readonly property bool _tateListLayout: systems._listLayout && Browse.Settings.current_orientation !== "horizontal"
    readonly property string _viewId: systems._listLayout ? (systems._tateListLayout ? "systemsListTate" : "systemsList") : "systemsGrid"
    readonly property string _browseThemeId: BrowseLayouts.currentThemeId
    readonly property var _gridProfile: BrowseLayouts.themeProfile(systems._browseThemeId, "systemsGrid")
    readonly property var _viewProfile: BrowseLayouts.themeProfile(systems._browseThemeId, systems._viewId)
    readonly property var _statusProfile: systems._viewProfile && systems._viewProfile.status ? systems._viewProfile.status : null
    readonly property var _footerProfile: systems._gridProfile && systems._gridProfile.footer ? systems._gridProfile.footer : null
    // CRT keeps the count + page cue in the footer (its top strip is
    // hidden entirely, `status.topStripVisible: false`); every other
    // theme hosts it up on the title line instead, alongside the count
    // badge -- see TopStatusStrip.qml's `pageIndicatorMode` and
    // BrowseLayouts.qml's `footer.pageCueInFooter`.
    readonly property bool _pageCueInFooter: !!(systems._footerProfile && systems._footerProfile.pageCueInFooter)
    readonly property bool _showGridPageCue: !systems._listLayout && !systems._pageCueInFooter
    // Round 11: list layout gets the same interactive chevron+"N / M"
    // PageIndicator grid layout already had. Sibling to `_showGridPageCue`
    // rather than folded into it so that property's existing grid-only
    // meaning stays intact.
    readonly property bool _showListPageCue: systems._listLayout && !systems._pageCueInFooter
    readonly property bool _showTopPageCue: systems._showGridPageCue || systems._showListPageCue
    // List layout's own page size for paging math -- the number of rows
    // actually on screen at once. Mirrors PagedGrid.qml's own
    // `currentPage`/`totalPageCount`/`hasPagesBelow` formulas, substituting
    // the list's own page size; the catalog has no incremental-fetch
    // concept (`Browse.SystemsModel.count` is always the true total), so
    // there's no `hasMorePages`-style term to add on top.
    readonly property int _listVisiblePageSize: Math.max(1, listCard.visibleRowCount)
    readonly property int _listTotalPageCount: Math.max(1, Math.ceil(Browse.SystemsModel.count / systems._listVisiblePageSize))
    readonly property int _listCurrentPage: Math.floor(systemsGrid.currentIndex / systems._listVisiblePageSize)
    readonly property bool _listHasPagesAbove: systems._listCurrentPage > 0
    readonly property bool _listHasPagesBelow: systems._listCurrentPage < systems._listTotalPageCount - 1
    // Round 11: the footer label's sideInset used to reserve a flat third
    // of the screen on each side to clear the count text (left) and the
    // PageIndicator (right) — regardless of how wide either one actually
    // is. Measure them instead, same idiom ActiveLabel itself uses for its
    // own name/tags block (TextMetrics + a couple px of slack).
    readonly property string _footerCountText: qsTr("%1 systems").arg(Browse.SystemsModel.count)
    readonly property int _footerCountTextWidth: Math.ceil(Math.max(footerCountMetrics.advanceWidth, footerCountMetrics.boundingRect.width) + (Theme.crtNativePath ? 0 : Sizing.px(2)))
    readonly property int _footerLeftInset: systems._footerCountTextWidth + (systems._footerProfile ? systems._footerProfile.bottomStatusLeftMargin : 0)
    readonly property int _footerRightInset: footerPageIndicator.width + (systems._footerProfile ? systems._footerProfile.bottomStatusRightMargin : 0)
    readonly property var _listProfile: systems._viewProfile && systems._viewProfile.list ? systems._viewProfile.list : null
    readonly property int _listOverlayBottomMargin: systems._listProfile ? systems._listProfile.overlayBottomMargin : Sizing.pctH(15)
    readonly property var _gridShape: Sizing.systemsGridShape(Sizing.screenWidth, Sizing.screenHeight)
    readonly property bool _loading: Browse.SystemsModel.loading || systems.optimisticLoading
    readonly property bool _overlayLoadingVisible: stateOverlay.loadingVisible
    readonly property bool _gateHide: systems.transitioning || systems._loading || systems._overlayLoadingVisible || (Browse.SystemsModel.error_message ?? "") !== ""

    signal requestAccept(systemId: string)
    signal requestHubScreen
    signal requestContextMenu(int index, var anchorRect, int anchorRadius)

    // Move selection by (dx, dy) and commit the new system id on
    // success. Returns the moveSelection result; row/column moves wrap
    // within the grid (no row-edge escape), so callers don't need to
    // act on the false branch — Esc is the only back path.
    function _performMove(dx: int, dy: int): bool {
        if (systems._listLayout) {
            if (dy === 0)
                return false;
            return systems._performLinearMove(dy);
        }
        if (systems.systemsGrid.moveSelection(dx, dy)) {
            Browse.SystemsState.system_id = Browse.SystemsModel.system_id_at(systems.systemsGrid.currentIndex);
            return true;
        }
        return false;
    }

    function _performLinearMove(delta: int): bool {
        const count = systems.systemsGrid.itemCount;
        if (count <= 0)
            return false;
        let next = systems.systemsGrid.currentIndex + delta;
        if (next < 0)
            next = count - 1;
        else if (next >= count)
            next = 0;
        if (next === systems.systemsGrid.currentIndex)
            return false;
        systems.systemsGrid.currentIndex = next;
        Browse.SystemsState.system_id = Browse.SystemsModel.system_id_at(systems.systemsGrid.currentIndex);
        return true;
    }

    // Page jump (L/R shoulder buttons). Wraps in both directions; same
    // post-move state-commit path as _performMove so the saved system
    // tracks whichever entry the user lands on.
    function _performPage(delta: int): bool {
        // Round 11: list layout used to fall straight through to the grid's
        // own pageBy() unconditionally -- which pages by the GRID's
        // columns x rows, not by however many rows are actually visible in
        // the list, so a page turn skipped past (or short of) what the
        // chevron/counter display implied.
        if (systems._listLayout)
            return systems._performLinearMove(delta * systems._listVisiblePageSize);
        if (systems.systemsGrid.pageBy(delta)) {
            Browse.SystemsState.system_id = Browse.SystemsModel.system_id_at(systems.systemsGrid.currentIndex);
            return true;
        }
        return false;
    }

    function _focusIndex(index: int): void {
        if (index < 0 || index >= systems.systemsGrid.itemCount)
            return;
        systems._focusArmed = true;
        systems.systemsGrid.currentIndex = index;
        Browse.SystemsState.system_id = Browse.SystemsModel.system_id_at(systems.systemsGrid.currentIndex);
    }

    // Mirrors ScreenStateOverlay's `state` ternary so accept routing and
    // the in-screen overlay agree on which state we're in.
    function _state(): string {
        if (systems._loading || systems._overlayLoadingVisible)
            return "loading";
        if ((Browse.SystemsModel.error_message ?? "") !== "")
            return "error";
        if (Browse.SystemsModel.count === 0)
            return "empty";
        return "ready";
    }

    function handleAction(action: string): void {
        if ((action === "left" || action === "right" || action === "up" || action === "down" || action === "page_prev" || action === "page_next") && systems._overlayLoadingVisible)
            return;
        if (action === "context_menu" && systems._gateHide)
            return;
        systems._focusArmed = true;

        if (action === "left") {
            systems._performMove(-1, 0);
        } else if (action === "right") {
            systems._performMove(1, 0);
        } else if (action === "down") {
            systems._performMove(0, 1);
        } else if (action === "up") {
            // Up inside the grid moves a row; at the top row it wraps
            // to the bottom row of the same page. Use Escape to back
            // out to the hub.
            systems._performMove(0, -1);
        } else if (action === "page_prev") {
            // L shoulder. Ignored on non-Ready states — there's no
            // data to page through.
            if (systems._state() === "ready")
                systems._performPage(-1);
        } else if (action === "page_next") {
            // R shoulder.
            if (systems._state() === "ready")
                systems._performPage(1);
        } else if (action === "accept") {
            // Accept routing depends on the screen's data state, matching
            // the help bar vocabulary in MainLayout.qml. Loading swallows
            // the press at the screen layer (no signal emitted).
            // Empty/Error emit `requestAccept("")` to signal the router
            // to retry the current load (the [OK] RETRY contract).
            // Ready emits `requestAccept(systemId)` to drill into Games.
            const state = systems._state();
            if (state === "loading")
                return;
            if (state === "error" || state === "empty") {
                systems.requestAccept("");
                return;
            }
            const chosen = Browse.SystemsModel.system_id_at(systems.systemsGrid.currentIndex);
            // Route the push-in cue to the visible layout. In list mode the
            // grid is hidden and the BrowseList is shown, so pulsing the grid
            // would animate nothing; mirror the layout-aware routing
            // MediaListScreen.pulseActivate() uses.
            if (systems._listLayout)
                listCard.activatePulse++;
            else
                systems.systemsGrid.pulseActivate();
            pressCommit._systemId = chosen;
            pressCommit.arm();
        } else if (action === "context_menu") {
            if (systems.systemsGrid.itemCount > 0) {
                const idx = systems.systemsGrid.currentIndex;
                Browse.SystemsState.system_id = Browse.SystemsModel.system_id_at(idx);
                systems.requestContextMenu(idx, systems._listLayout ? listCard.currentCellRectIn(systems) : systems.systemsGrid.currentCellRectIn(systems), systems._listLayout ? listCard.currentCellRadius : systems.systemsGrid.currentCellRadius);
            }
        } else if (action === "cancel") {
            // Disarm a pending accept so a press-then-back inside the deferred
            // window cannot drill into a system after the user has backed out.
            pressCommit.stop();
            systems.requestHubScreen();
        }
    }

    // ── Visual tree ───────────────────────────────────────────────────────────

    DeferredAction {
        id: pressCommit
        property string _systemId: ""
        onDeferred: {
            const id = _systemId;
            _systemId = "";
            systems.requestAccept(id);
        }
    }

    // Top status strip — category title (center), count badge (left), and
    // chevron page cue (right, `pageIndicatorMode`) for both grid and list
    // layout (round 11 gave list layout the same treatment grid already
    // had, replacing a plain item-position counter with no chevrons at
    // all) -- UNLESS the theme keeps them in the footer instead
    // (`_pageCueInFooter` -- CRT, whose top strip is hidden entirely
    // anyway).
    //
    // The screen Item fills the whole window, so the strip clears the
    // MainLayout HeaderBar (Sizing.headerBottom) with a small gap.
    TopStatusStrip {
        id: topStrip
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: parent.top
        anchors.topMargin: Sizing.headerBottom + (systems._statusProfile ? systems._statusProfile.topMargin : Sizing.pctH(1))
        height: systems._statusProfile ? systems._statusProfile.stripHeight : Sizing.pctH(7)
        slotMargin: systems._statusProfile ? systems._statusProfile.slotMargin : Sizing.pctW(3)
        title: CategoryIds.displayName(Browse.SystemsModel.current_category)
        currentPage: systems._listLayout ? systems._listCurrentPage : systemsGrid.currentPage
        totalPages: systems._listLayout ? systems._listTotalPageCount : Math.max(1, Math.ceil(Browse.SystemsModel.count / systemsGrid.pageSize))
        totalText: {
            if (systems._listLayout)
                return !Theme.crtNativePath && Browse.SystemsModel.count > 0 ? qsTr("%1 systems").arg(Browse.SystemsModel.count) : "";
            return systems._showGridPageCue && Browse.SystemsModel.count > 0 ? qsTr("%1 systems").arg(Browse.SystemsModel.count) : "";
        }
        // Round 11: list layout now mounts the same interactive
        // PageIndicator grid layout has (`pageIndicatorMode` below) instead
        // of an item-position "N / M" counter -- the catalog loads fully
        // upfront (no background-fetch signal worth surfacing here the way
        // GamesScreen's "Loading more…" is), so this is always empty now.
        rightTextOverride: ""
        showPageCounter: systems._listLayout || systems._showGridPageCue
        pageIndicatorMode: systems._showTopPageCue
        pageIndicatorChevronSize: systems._gridProfile && systems._gridProfile.grid ? systems._gridProfile.grid.pageChevronSize : Sizing.pctH(4)
        hasPagesAbove: systems._listLayout ? systems._listHasPagesAbove : systemsGrid.hasPagesAbove
        hasPagesBelow: systems._listLayout ? systems._listHasPagesBelow : systemsGrid.hasPagesBelow
        onPageRequested: delta => systems._performPage(delta)
        visible: !systems._gateHide && (!systems._statusProfile || systems._statusProfile.topStripVisible)
    }

    BrowseListDetailView {
        id: listCard

        visible: !systems._gateHide && systems._listLayout
        anchors.left: parent.left
        anchors.leftMargin: systems._listProfile ? systems._listProfile.cardSideMargin : Sizing.pctW(3)
        anchors.right: parent.right
        anchors.rightMargin: systems._listProfile ? systems._listProfile.cardSideMargin : Sizing.pctW(3)
        anchors.top: topStrip.bottom
        anchors.topMargin: systems._listProfile ? systems._listProfile.cardTopMargin : Sizing.pctH(2)
        anchors.bottom: parent.bottom
        anchors.bottomMargin: systems._listProfile ? systems._listProfile.cardBottomMargin : Sizing.pctH(8)
        model: Browse.SystemsModel
        currentIndex: systemsGrid.currentIndex
        focusReady: systems._focusReady
        screenSettling: !systems.active
        layoutProfile: systems._viewProfile
        detailTitle: listCard.currentName
        detailCoverKey: listCard.currentCoverKey
        detailTags: Browse.SystemsModel.count > 0 ? Browse.SystemsModel.detail_tags_at(systemsGrid.currentIndex) : ""
        onItemHovered: index => systems._focusIndex(index)
        onItemClicked: index => {
            systems._focusIndex(index);
            systems.handleAction("accept");
        }
        onItemRightClicked: index => {
            systems._focusIndex(index);
            systems.handleAction("context_menu");
        }
        onEmptyRightClicked: systems.handleAction("cancel")
        onPageWheelRequested: delta => systems.handleAction(delta > 0 ? "page_next" : "page_prev")
    }

    // Grid fills the safe zone between the top strip and the active
    // label. The bottom margin reserves the label's own bottom margin
    // plus its height; the global help bar is handled separately.
    PagedGrid {
        id: systemsGrid

        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: topStrip.bottom
        anchors.bottom: parent.bottom
        anchors.bottomMargin: systems._footerProfile ? systems._footerProfile.gridBottomMargin : (Sizing.pctH(8) + Sizing.pctH(7))
        focused: systems.gridFocused
        screenSettling: !systems.active
        focusReady: systems._focusReady
        // Keep the lightweight delegate/cursor structure during category
        // replacement, but withhold Image sources while hidden. Fully removing
        // Repeater's model tears down the prior category synchronously and made
        // the transition itself wait on that cleanup.
        suspendDelegates: systems._listLayout
        coverRequestsEnabled: systems.active && !systems.preparingTransition && !systems._gateHide
        // Router already warms visible page. Do not simultaneously rasterize
        // hidden next-page logos or focused variants for every system.
        coverLookaheadPages: 0
        eagerFocusedCovers: false
        model: Browse.SystemsModel
        layoutProfile: systems._viewProfile
        columnsOverride: systems._gridShape.columns
        rowsOverride: systems._gridShape.rows
        delegate: Tile {
            layoutProfile: systems._viewProfile
        }
        onItemHovered: index => systems._focusIndex(index)
        onItemClicked: index => {
            systems._focusIndex(index);
            systems.handleAction("accept");
        }
        onItemRightClicked: index => {
            systems._focusIndex(index);
            systems.handleAction("context_menu");
        }
        onEmptyRightClicked: systems.handleAction("cancel")
        onPageWheelRequested: delta => systems.handleAction(delta > 0 ? "page_next" : "page_prev")

        // Hide tiles as soon as the model enters Loading, while the
        // centered cue below can still use its anti-flicker delay.
        // Otherwise the cleared/reseeded model can flash loading tiles.
        visible: !systems._gateHide && !systems._listLayout
    }

    // Footer row — active system caption, same typography as the top
    // strip's title slot so the two big captions read as a matched pair.
    // The count badge and page cue sit up on the top strip now instead
    // (`_showGridPageCue` above) except on CRT, whose top strip is hidden
    // entirely -- CRT keeps them down here, `_pageCueInFooter`. Everywhere
    // else this is a title-only row, so `sideInset` reverts to
    // ActiveLabel's own default margin instead of yielding a third of the
    // width to corner slots that are empty here now.
    ActiveLabel {
        id: activeLabel
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.bottom: parent.bottom
        anchors.bottomMargin: systems._footerProfile ? systems._footerProfile.activeLabelBottomMargin : Sizing.pctH(8)
        height: systems._footerProfile ? systems._footerProfile.activeLabelHeight : Sizing.pctH(7)
        sideInset: systems._pageCueInFooter ? Math.max(systems._footerLeftInset, systems._footerRightInset) : Sizing.pctW(3)
        text: systemsGrid.itemCount > 0 ? Browse.SystemsModel.system_name_at(systemsGrid.currentIndex) : ""
        // Worded reason for the muted front edge (Tile.qml's `edgeColor`)
        // on a hidden system tile -- Systems tiles carry no per-tile
        // caption to fold this into (showCaption: false), so it surfaces
        // here instead, only while that tile is focused. Mirrors
        // HubScreen.qml's equivalent `tags` binding for disabled tiles.
        tags: systemsGrid.itemCount > 0 && Browse.SystemsModel.is_hidden_at(systemsGrid.currentIndex) ? qsTr("Hidden") : ""
        visible: !systems._gateHide && !systems._listLayout
    }

    // Measures `_footerCountText` so the reserved footer inset (above) and
    // this Text's own width track the count string's actual size instead
    // of a flat third of the screen.
    TextMetrics {
        id: footerCountMetrics
        text: systems._footerCountText
        font.family: Theme.fontUi
        font.pixelSize: Sizing.fontSection
    }

    Text {
        id: footerCount
        objectName: "systemsFooterCount"
        // Round 11: the count text is layout-agnostic (just "%1 systems"),
        // so it now shows in the footer for list layout on CRT too --
        // previously grid-only, leaving list layout with no page cue
        // anywhere on that profile.
        visible: !systems._gateHide && systems._pageCueInFooter && Browse.SystemsModel.count > 0
        anchors.left: parent.left
        anchors.leftMargin: systems._footerProfile ? systems._footerProfile.bottomStatusLeftMargin : 0
        anchors.verticalCenter: activeLabel.verticalCenter
        width: systems._footerCountTextWidth
        height: Sizing.fontSection
        elide: Text.ElideRight
        horizontalAlignment: Text.AlignLeft
        verticalAlignment: Text.AlignVCenter
        text: systems._footerCountText
        font.family: Theme.fontUi
        font.pixelSize: Sizing.fontSection
        color: Theme.textPrimary
        renderType: Text.NativeRendering
    }

    PageIndicator {
        id: footerPageIndicator
        objectName: "systemsFooterPageIndicator"
        visible: !systems._gateHide && systems._pageCueInFooter
        anchors.right: parent.right
        anchors.rightMargin: systems._footerProfile ? systems._footerProfile.bottomStatusRightMargin : 0
        anchors.verticalCenter: activeLabel.verticalCenter
        chevronSize: systems._gridProfile && systems._gridProfile.grid ? systems._gridProfile.grid.pageChevronSize : Sizing.pctH(4)
        currentPage: systems._listLayout ? systems._listCurrentPage : systemsGrid.currentPage
        totalPages: systems._listLayout ? systems._listTotalPageCount : Math.max(1, Math.ceil(Browse.SystemsModel.count / systemsGrid.pageSize))
        hasPagesAbove: systems._listLayout ? systems._listHasPagesAbove : systemsGrid.hasPagesAbove
        hasPagesBelow: systems._listLayout ? systems._listHasPagesBelow : systemsGrid.hasPagesBelow
        onPageRequested: delta => systems._performPage(delta)
    }

    ScreenStateOverlay {
        id: stateOverlay

        x: systems._listLayout ? 0 : systemsGrid.x
        y: systems._listLayout ? listCard.y : systemsGrid.y
        width: systems._listLayout ? systems.width : systemsGrid.width
        height: systems._listLayout ? Math.max(0, systems.height - listCard.y - systems._listOverlayBottomMargin) : systemsGrid.height
        // Content rect starts below the header, so recenter on the full
        // screen (which matches `scene`, the global transition cue's new
        // parent) rather than this rect's own smaller height — otherwise
        // the loading cue jumps up when the global cue hands off here.
        cueCenterY: systems.height / 2 - y
        enabled: true
        loading: systems._loading
        errorMessage: Browse.SystemsModel.error_message ?? ""
        count: Browse.SystemsModel.count
        emptyText: qsTr("No systems in this category")
        loadingText: qsTr("Loading systems…")
    }
}
