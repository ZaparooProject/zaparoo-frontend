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
        _originalBrowseLayout = Browse.Settings.current_browse_layout;
        Sizing.screenWidth = testCase.width;
        Sizing.screenHeight = testCase.height;
    }

    ListModel {
        id: mediaModel
    }

    MediaListScreen {
        id: screen

        anchors.fill: parent
        mediaModel: mediaModel
        emptyText: "No entries"
        loadingText: "Loading entries"
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
        Browse.Settings.current_browse_layout = "grid";
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
                "disambiguatingTags": ""
            });
        }
        tryCompare(screen.mediaGrid, "itemCount", mediaModel.count);
        screen.mediaGrid.setCurrentIndexImmediate(0);
    }

    function cleanup(): void {
        Browse.Settings.current_browse_layout = _originalBrowseLayout;
        screen.optimisticLoading = false;
    }

    // Left/Right in list layout must land exactly where page_prev /
    // page_next land: a whole page per press, like MiSTer's own menu.
    // Asserting equivalence (not a hardcoded page size) keeps the test
    // honest if the page metric ever changes.
    function test_list_left_right_page_like_page_prev_next(): void {
        Browse.Settings.current_browse_layout = "list";
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

    // Grid layout must keep Left/Right as one-column selection moves;
    // the list paging fallback is gated on _listLayout.
    function test_grid_left_right_still_move_one_column(): void {
        Browse.Settings.current_browse_layout = "grid";
        screen.mediaGrid.setCurrentIndexImmediate(0);

        screen.handleAction("right");
        compare(screen.mediaGrid.currentIndex, 1);
        screen.handleAction("left");
        compare(screen.mediaGrid.currentIndex, 0);
    }

    // The paging fallback shares page_prev / page_next's ready-state
    // guard: an empty list must not page.
    function test_list_left_right_do_nothing_when_empty(): void {
        Browse.Settings.current_browse_layout = "list";
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
        Browse.Settings.current_browse_layout = "list";
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
