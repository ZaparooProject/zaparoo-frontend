// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
// cxx-qt 0.8 singleton methods aren't marked final so Browse.* calls trip
// "Member can be shadowed"; structural, suppress compiler.
// qmllint disable compiler

import QtQuick
import QtTest
import Zaparoo.Browse as Browse
import Zaparoo.Screens
import Zaparoo.Theme

TestCase {
    id: testCase

    name: "UiMediaListPaging"
    when: windowShown
    width: 1280
    height: 720
    visible: true

    property string _originalBrowseLayout: "grid"

    Component.onCompleted: {
        _originalBrowseLayout = Browse.Settings.current_games_browse_layout;
        Sizing.screenWidth = testCase.width;
        Sizing.screenHeight = testCase.height;
    }

    ListModel {
        id: mediaModel
    }

    SignalSpy {
        id: pageMenuSpy

        target: screen
        signalName: "requestPageMenu"
    }

    MediaListScreen {
        id: screen

        anchors.fill: parent
        mediaModel: mediaModel
        emptyText: qsTr("No entries")
        loadingText: qsTr("Loading entries")
        showTopStrip: false
        detailShowTitle: false
        suppressSelectionPersist: true
        gridColumnsOverride: 2
        gridRowsOverride: 2
        totalItemsOverride: mediaModel.count
    }

    function init(): void {
        Sizing.screenWidth = testCase.width;
        Sizing.screenHeight = testCase.height;
        Browse.Settings.current_games_browse_layout = "grid";
        screen.optimisticLoading = false;
        mediaModel.clear();
        // Enough rows that a single page step in either direction stays
        // clear of the linear-move wrap and of the fetch_more calls near
        // the slice tail (this plain ListModel has no fetch_more).
        for (let i = 0; i < 30; i++) {
            mediaModel.append({
                "name": "Game " + i,
                "fileStem": "Game " + i,
                "coverKey": "",
                "favorite": 0,
                "hidden": false,
                "disambiguatingTags": "",
                "entryType": "media",
                "fileCount": 0,
                "disabled": false,
                "isEmpty": false
            });
        }
        tryCompare(screen.mediaGrid, "itemCount", mediaModel.count);
        screen.mediaGrid.setCurrentIndexImmediate(0);
    }

    function cleanup(): void {
        Browse.Settings.current_games_browse_layout = _originalBrowseLayout;
        screen.optimisticLoading = false;
        Sizing.screenWidth = testCase.width;
        Sizing.screenHeight = testCase.height;
    }

    // Left/Right in list layout must land exactly where page_prev /
    // page_next land: a whole page per press, like MiSTer's own menu.
    // Asserting equivalence (not a hardcoded page size) keeps the test
    // honest if the page metric ever changes.
    function test_list_left_right_page_like_page_prev_next(): void {
        Browse.Settings.current_games_browse_layout = "list";
        screen.mediaGrid.setCurrentIndexImmediate(0);

        screen.handleAction("page_next");
        const pagedIndex = screen.mediaGrid.currentIndex;
        verify(pagedIndex > 0, "page_next should advance from row 0");
        screen.handleAction("page_prev");
        compare(screen.mediaGrid.currentIndex, 0);

        screen.handleAction("right");
        compare(screen.mediaGrid.currentIndex, pagedIndex);
        screen.handleAction("left");
        compare(screen.mediaGrid.currentIndex, 0);
    }

    // Scope-changing View menu must remain reachable when active scope is
    // empty, or user cannot clear it. Off by default for screens without such
    // controls.
    function test_page_menu_reachable_when_empty_only_if_opted_in(): void {
        const spy = pageMenuSpy;
        mediaModel.clear();
        tryCompare(screen.mediaGrid, "itemCount", 0);

        screen.pageMenuEnabledWhenEmpty = false;
        spy.clear();
        screen.handleAction("page_menu");
        compare(spy.count, 0, "empty list stays gated by default");

        screen.pageMenuEnabledWhenEmpty = true;
        screen.handleAction("page_menu");
        compare(spy.count, 1, "opted-in screen can still open its View menu");
        screen.pageMenuEnabledWhenEmpty = false;
    }

    // Round 11 regression: list-layout paging used to move by
    // `mediaGrid.pageSize` (this fixture's grid columns x rows = 2x2 = 4),
    // not by however many rows the list actually shows on screen -- a
    // mismatch that either paged farther than a screenful or (as here,
    // where the visible row count is well above 4) left several
    // screenfuls un-skippable in one press. `_listVisiblePageSize` is
    // what both the page cue's displayed page number and the actual
    // press now agree on.
    function test_list_left_right_page_by_the_lists_own_row_count_not_the_grids(): void {
        Browse.Settings.current_games_browse_layout = "list";
        mediaModel.clear();
        for (let i = 0; i < 60; i++) {
            mediaModel.append({
                "name": "Game " + i,
                "fileStem": "Game " + i,
                "coverKey": "",
                "favorite": 0,
                "hidden": false,
                "disambiguatingTags": "",
                "entryType": "media",
                "fileCount": 0,
                "disabled": false,
                "isEmpty": false
            });
        }
        tryCompare(screen.mediaGrid, "itemCount", mediaModel.count);
        screen.mediaGrid.setCurrentIndexImmediate(0);

        const listPageSize = screen._listVisiblePageSize;
        verify(listPageSize !== screen.mediaGrid.pageSize, "fixture must exercise a list page size different from the grid's, or this test proves nothing");
        verify(listPageSize < mediaModel.count, "fixture needs more rows than one list page holds, or paging wraps trivially");

        screen.handleAction("right");
        compare(screen.mediaGrid.currentIndex, listPageSize, "one page-right press must land exactly _listVisiblePageSize rows forward");
    }

    // The page-cue state (what a mounted PageIndicator would show) must
    // track the same page size paging actually uses.
    function test_list_page_state_tracks_the_lists_own_page_size(): void {
        Browse.Settings.current_games_browse_layout = "list";
        screen.mediaGrid.setCurrentIndexImmediate(0);

        compare(screen._listCurrentPage, 0);
        compare(screen._listHasPagesAbove, false);
        verify(screen._listTotalPageCount >= 1);

        screen.handleAction("right");
        compare(screen._listCurrentPage, Math.floor(screen.mediaGrid.currentIndex / screen._listVisiblePageSize));
        compare(screen._listHasPagesAbove, screen._listCurrentPage > 0);
    }

    function test_list_chrome_shows_item_position_without_left_total(): void {
        Browse.Settings.current_games_browse_layout = "list";
        screen.mediaGrid.setCurrentIndexImmediate(5);

        compare(screen.topStrip.totalText, "", "detailed list must not duplicate total count on left");
        compare(screen.topStrip.itemPositionMode, true);
        compare(screen.topStrip.currentItem, 5);
        compare(screen.topStrip.totalItems, mediaModel.count);
        const indicator = findChild(screen.topStrip, "topStatusPageIndicator");
        verify(indicator !== null);
        compare(indicator.pageText, "6/30");
    }

    function test_240p_list_footer_hides_left_total_and_shows_item_position(): void {
        Sizing.screenWidth = 640;
        Sizing.screenHeight = 240;
        Browse.Settings.current_games_browse_layout = "list";
        screen.bottomStatusLeftText = "30 games";
        screen.mediaGrid.setCurrentIndexImmediate(5);

        const count = findChild(screen, "mediaListFooterCount");
        const indicator = findChild(screen, "mediaListFooterPageIndicator");
        verify(count !== null);
        verify(indicator !== null);
        compare(count.visible, false);
        compare(indicator.itemPositionMode, true);
        compare(indicator.pageText, "6/30");

        screen.bottomStatusLeftText = "";
    }

    // Grid layout must keep Left/Right as one-column selection moves;
    // the list paging fallback is gated on _listLayout.
    function test_grid_left_right_still_move_one_column(): void {
        Browse.Settings.current_games_browse_layout = "grid";
        screen.mediaGrid.setCurrentIndexImmediate(0);

        screen.handleAction("right");
        compare(screen.mediaGrid.currentIndex, 1);
        screen.handleAction("left");
        compare(screen.mediaGrid.currentIndex, 0);
    }

    // The paging fallback shares page_prev / page_next's ready-state
    // guard: an empty list must not page.
    function test_list_left_right_do_nothing_when_empty(): void {
        Browse.Settings.current_games_browse_layout = "list";
        mediaModel.clear();
        tryCompare(screen.mediaGrid, "itemCount", 0);
        const before = screen.mediaGrid.currentIndex;

        screen.handleAction("right");
        compare(screen.mediaGrid.currentIndex, before);
        screen.handleAction("left");
        compare(screen.mediaGrid.currentIndex, before);
    }

    // A loading list must not page either; page_next / page_prev are
    // held to the same guard so the paths cannot drift apart. Start from
    // a paged-forward index so a wrap back to row 0 cannot fake a pass.
    function test_list_left_right_do_nothing_while_loading(): void {
        Browse.Settings.current_games_browse_layout = "list";
        screen.mediaGrid.setCurrentIndexImmediate(0);
        screen.handleAction("page_next");
        const before = screen.mediaGrid.currentIndex;
        verify(before > 0, "page_next should advance before the loading gate");
        screen.optimisticLoading = true;

        screen.handleAction("right");
        compare(screen.mediaGrid.currentIndex, before);
        screen.handleAction("left");
        compare(screen.mediaGrid.currentIndex, before);
        screen.handleAction("page_next");
        compare(screen.mediaGrid.currentIndex, before);
        screen.handleAction("page_prev");
        compare(screen.mediaGrid.currentIndex, before);
    }
}
