// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

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
    property bool transitioning: false
    // Router-driven flag: `MainLayout` writes this to
    // `!ScreenManager.hasModal` so the focused tile's accent ring
    // hides while a modal (the context menu) is on top of the stack.
    // Two competing focus rings — one on the menu's selected entry
    // and one on the anchored tile — read as ambiguous; suppressing
    // the tile ring keeps a single visible focus indicator at all
    // times. The ring restores automatically when the modal pops.
    property bool gridFocused: true
    property bool optimisticLoading: false
    readonly property bool _listLayout: Browse.Settings.current_browse_layout === "list"
    readonly property bool _crtGridLayout: Theme.crtNativePath && !systems._listLayout
    readonly property bool _crtListStrip: Theme.crtNativePath && systems._listLayout
    readonly property bool _tateListLayout: systems._listLayout && Browse.Settings.current_orientation !== "horizontal"
    readonly property string _viewId: systems._listLayout ? (systems._tateListLayout ? "systemsListTate" : "systemsList") : "systemsGrid"
    readonly property string _browseThemeId: BrowseLayouts.currentThemeId
    readonly property var _gridProfile: BrowseLayouts.themeProfile(systems._browseThemeId, "systemsGrid")
    readonly property var _viewProfile: BrowseLayouts.themeProfile(systems._browseThemeId, systems._viewId)
    readonly property var _statusProfile: systems._viewProfile && systems._viewProfile.status ? systems._viewProfile.status : null
    readonly property var _footerProfile: systems._gridProfile && systems._gridProfile.footer ? systems._gridProfile.footer : null
    readonly property var _listProfile: systems._viewProfile && systems._viewProfile.list ? systems._viewProfile.list : null
    readonly property int _listOverlayBottomMargin: systems._listProfile ? systems._listProfile.overlayBottomMargin : Sizing.pctH(15)
    readonly property var _gridShape: Sizing.systemsGridShape(Sizing.screenWidth, Sizing.screenHeight)
    readonly property bool _loading: Browse.SystemsModel.loading || systems.optimisticLoading
    readonly property bool _gateHide: systems.transitioning || systems._loading

    signal requestAccept(systemId: string)
    signal requestHubScreen
    signal requestContextMenu(int index, var anchorRect)

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
        if (systems.systemsGrid.pageBy(delta)) {
            Browse.SystemsState.system_id = Browse.SystemsModel.system_id_at(systems.systemsGrid.currentIndex);
            return true;
        }
        return false;
    }

    function _focusIndex(index: int): void {
        if (index < 0 || index >= systems.systemsGrid.itemCount)
            return;
        systems.systemsGrid.currentIndex = index;
        Browse.SystemsState.system_id = Browse.SystemsModel.system_id_at(systems.systemsGrid.currentIndex);
    }

    // Mirrors ScreenStateOverlay's `state` ternary so accept routing and
    // the in-screen overlay agree on which state we're in.
    function _state(): string {
        if (systems._loading)
            return "loading";
        if ((Browse.SystemsModel.error_message ?? "") !== "")
            return "error";
        if (Browse.SystemsModel.count === 0)
            return "empty";
        return "ready";
    }

    function handleAction(action: string): void {
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
            systems.requestAccept(chosen);
        } else if (action === "write_card") {
            if (systems.systemsGrid.itemCount > 0) {
                const idx = systems.systemsGrid.currentIndex;
                Browse.SystemsState.system_id = Browse.SystemsModel.system_id_at(idx);
                systems.requestContextMenu(idx, systems._listLayout ? listCard.currentCellRectIn(systems) : systems.systemsGrid.currentCellRectIn(systems));
            }
        } else if (action === "cancel") {
            systems.requestHubScreen();
        }
    }

    // ── Visual tree ───────────────────────────────────────────────────────────

    // Top status strip — page counter (left), category title (center),
    // total-systems badge (right). Replaces the standalone top label
    // and the old bottom-of-grid PaginationStatus band so the screen's
    // "where am I" context all sits at the top in one row.
    //
    // The screen Item fills the whole window, so the strip clears the
    // MainLayout HeaderBar (Sizing.headerBottom) with a small gap.
    //
    // SystemsModel is non-paginated (every row loads eagerly on
    // category switch) — the page counter still reads off
    // systemsGrid.currentPage / pageCount because PagedGrid pages
    // through whatever count it sees. The "%1 systems" badge is the
    // filter-applied count for the current category, not the catalog
    // total.
    TopStatusStrip {
        id: topStrip
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.top: parent.top
        anchors.topMargin: Sizing.headerBottom + (systems._statusProfile ? systems._statusProfile.topMargin : Sizing.pctH(1))
        height: systems._statusProfile ? systems._statusProfile.stripHeight : Sizing.pctH(7)
        slotMargin: systems._statusProfile ? systems._statusProfile.slotMargin : Sizing.pctW(5)
        title: Browse.SystemsModel.current_category
        currentPage: systemsGrid.currentPage
        totalPages: systems._footerProfile && systems._footerProfile.bottomStatusVisible ? 1 : Math.max(1, Math.ceil(Browse.SystemsModel.count / systemsGrid.pageSize))
        totalText: Theme.crtNativePath ? "" : (Browse.SystemsModel.count > 0 ? qsTr("%1 systems").arg(Browse.SystemsModel.count) : "")
        rightTextOverride: !systems._listLayout || systemsGrid.itemCount <= 0 ? "" : qsTr("%1 / %2").arg(systemsGrid.currentIndex + 1).arg(Math.max(1, Browse.SystemsModel.count))
        visible: !systems._gateHide && (!systems._statusProfile || systems._statusProfile.topStripVisible)
    }

    BrowseListDetailView {
        id: listCard

        visible: !systems._gateHide && systems._listLayout
        anchors.left: parent.left
        anchors.leftMargin: systems._listProfile ? systems._listProfile.cardSideMargin : Sizing.pctW(5)
        anchors.right: parent.right
        anchors.rightMargin: systems._listProfile ? systems._listProfile.cardSideMargin : Sizing.pctW(5)
        anchors.top: topStrip.bottom
        anchors.topMargin: systems._listProfile ? systems._listProfile.cardTopMargin : Sizing.pctH(2)
        anchors.bottom: parent.bottom
        anchors.bottomMargin: systems._listProfile ? systems._listProfile.cardBottomMargin : Sizing.pctH(8)
        model: Browse.SystemsModel
        currentIndex: systemsGrid.currentIndex
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
            systems.handleAction("write_card");
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
            systems.handleAction("write_card");
        }
        onEmptyRightClicked: systems.handleAction("cancel")
        onPageWheelRequested: delta => systems.handleAction(delta > 0 ? "page_next" : "page_prev")

        // Hide tiles as soon as the model enters Loading, while the
        // centered cue below can still use its anti-flicker delay.
        // Otherwise the cleared/reseeded model can flash loading tiles.
        visible: !systems._gateHide && !systems._listLayout
    }

    // Active system caption — single big line just under the grid.
    // Same typography as the top strip's title slot so the two big
    // captions read as a matched pair (top = category context, bottom
    // = focused-tile selection).
    ActiveLabel {
        id: activeLabel
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.bottom: parent.bottom
        anchors.bottomMargin: systems._footerProfile ? systems._footerProfile.activeLabelBottomMargin : Sizing.pctH(8)
        height: systems._footerProfile ? systems._footerProfile.activeLabelHeight : Sizing.pctH(7)
        text: systemsGrid.itemCount > 0 ? Browse.SystemsModel.system_name_at(systemsGrid.currentIndex) : ""
        visible: !systems._gateHide && !systems._listLayout
    }

    Text {
        visible: systems._footerProfile && systems._footerProfile.bottomStatusVisible && !systems._gateHide && !systems._listLayout && Browse.SystemsModel.count > 0
        anchors.left: parent.left
        anchors.leftMargin: systems._footerProfile ? systems._footerProfile.bottomStatusLeftMargin : 0
        anchors.verticalCenter: activeLabel.verticalCenter
        width: Sizing.px(parent.width / 3) - (systems._footerProfile ? systems._footerProfile.bottomStatusLeftMargin : 0)
        height: Sizing.fontSize(2.9)
        elide: Text.ElideRight
        horizontalAlignment: Text.AlignLeft
        verticalAlignment: Text.AlignVCenter
        text: qsTr("%1 systems").arg(Browse.SystemsModel.count)
        font.family: Theme.fontUi
        font.pixelSize: Sizing.fontSize(2.9)
        color: Theme.textPrimary
        renderType: Text.NativeRendering
    }

    Text {
        visible: systems._footerProfile && systems._footerProfile.bottomStatusVisible && !systems._gateHide && !systems._listLayout && Math.ceil(Browse.SystemsModel.count / systemsGrid.pageSize) > 1
        anchors.right: parent.right
        anchors.rightMargin: systems._footerProfile ? systems._footerProfile.bottomStatusRightMargin : 0
        anchors.verticalCenter: activeLabel.verticalCenter
        width: Sizing.px(parent.width / 3) - (systems._footerProfile ? systems._footerProfile.bottomStatusRightMargin : 0)
        height: Sizing.fontSize(2.9)
        elide: Text.ElideRight
        horizontalAlignment: Text.AlignRight
        verticalAlignment: Text.AlignVCenter
        text: qsTr("%1 / %2").arg(systemsGrid.currentPage + 1).arg(Math.max(1, Math.ceil(Browse.SystemsModel.count / systemsGrid.pageSize)))
        font.family: Theme.fontUi
        font.pixelSize: Sizing.fontSize(2.9)
        color: Theme.textPrimary
        renderType: Text.NativeRendering
    }

    ScreenStateOverlay {
        x: systems._listLayout ? 0 : systemsGrid.x
        y: systems._listLayout ? listCard.y : systemsGrid.y
        width: systems._listLayout ? systems.width : systemsGrid.width
        height: systems._listLayout ? Math.max(0, systems.height - listCard.y - systems._listOverlayBottomMargin) : systemsGrid.height
        enabled: true
        loading: systems._loading
        errorMessage: Browse.SystemsModel.error_message ?? ""
        count: Browse.SystemsModel.count
        emptyText: qsTr("No systems in this category")
        loadingText: qsTr("Loading systems…")
    }
}
