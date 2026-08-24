// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
// cxx-qt 0.8 singleton methods aren't marked final so Browse.* calls trip
// "Member can be shadowed". findChild() returns QVariant so property accesses
// on the result can't be statically typed. Both are structural; suppress compiler.
// qmllint disable compiler

import QtQuick
import QtTest
import Zaparoo.Browse as Browse
import Zaparoo.Screens
import Zaparoo.Theme
import Zaparoo.Ui

TestCase {
    id: testCase

    name: "UiMediaListTitles"
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
        activeLabelTextProvider: () => testCase.displayTitleAt(screen.mediaGrid.currentIndex)
    }

    function init(): void {
        Sizing.screenWidth = testCase.width;
        Sizing.screenHeight = testCase.height;
        Browse.Settings.current_games_browse_layout = "grid";
        mediaModel.clear();
        mediaModel.append({
            "name": "D (Disc 1)",
            "fileStem": "D",
            "coverKey": "",
            "favorite": 0,
            "hidden": false,
            "disambiguatingTags": "",
            "isEmpty": false,
            "entryType": "media",
            "fileCount": 0,
            "disabled": false
        });
        mediaModel.append({
            "name": "D (Disc 2)",
            "fileStem": "D",
            "coverKey": "",
            "favorite": 0,
            "hidden": false,
            "disambiguatingTags": "",
            "isEmpty": false,
            "entryType": "media",
            "fileCount": 0,
            "disabled": false
        });
        mediaModel.append({
            "name": "Friendly Alias",
            "fileStem": "InternalContainer",
            "coverKey": "",
            "favorite": 0,
            "hidden": false,
            "disambiguatingTags": "",
            "isEmpty": false,
            "entryType": "media",
            "fileCount": 0,
            "disabled": false
        });
        tryCompare(screen.mediaGrid, "itemCount", mediaModel.count);
        screen.mediaGrid.setCurrentIndexImmediate(0);
    }

    function cleanup(): void {
        Browse.Settings.current_games_browse_layout = _originalBrowseLayout;
    }

    function displayTitleAt(index: int): string {
        if (index < 0 || index >= mediaModel.count)
            return "";
        const row = mediaModel.get(index);
        return row.name !== "" ? row.name : row.fileStem;
    }

    function hasVisibleText(item: var, expected: string): bool {
        if (item === null || item.visible === false || item.opacity === 0)
            return false;
        if (typeof item.text === "string" && item.text === expected && item.width > 0 && item.height > 0)
            return true;
        const children = item.children;
        for (let i = 0; i < children.length; i++) {
            if (hasVisibleText(children[i], expected))
                return true;
        }
        return false;
    }

    function assertGridAndListTitle(index: int, expected: string): void {
        Browse.Settings.current_games_browse_layout = "grid";
        screen.mediaGrid.setCurrentIndexImmediate(index);
        tryCompare(screen.activeLabel, "text", expected);
        tryVerify(() => hasVisibleText(screen.mediaGrid, expected), 1000, "grid title should render " + expected);

        Browse.Settings.current_games_browse_layout = "list";
        screen.mediaGrid.setCurrentIndexImmediate(index);
        tryCompare(screen.listCard, "visible", true);
        tryVerify(() => hasVisibleText(screen.listCard, expected), 1000, "list title should render " + expected);
    }

    function test_multi_disc_titles_match_between_grid_and_list(): void {
        assertGridAndListTitle(0, "D (Disc 1)");
        assertGridAndListTitle(1, "D (Disc 2)");
    }

    function test_singleton_directory_alias_title_matches_between_grid_and_list(): void {
        assertGridAndListTitle(2, "Friendly Alias");
    }

    // A disambiguating-tag token renders inline after the name, in both layouts,
    // without being focused (the always-visible top token is what lets users
    // tell same-named variants apart at a glance across a grid).
    function test_disambiguating_token_renders_inline_in_grid_and_list(): void {
        mediaModel.append({
            "name": "Sonic CD",
            "fileStem": "Sonic CD",
            "coverKey": "",
            "favorite": 0,
            "hidden": false,
            "disambiguatingTags": "US",
            "isEmpty": false
        });
        const idx = mediaModel.count - 1;
        tryCompare(screen.mediaGrid, "itemCount", mediaModel.count);

        Browse.Settings.current_games_browse_layout = "grid";
        screen.mediaGrid.setCurrentIndexImmediate(idx);
        tryVerify(() => hasVisibleText(screen.mediaGrid, "Sonic CD"), 1000, "grid name should render");
        tryVerify(() => hasVisibleText(screen.mediaGrid, "US"), 1000, "grid token should render inline");

        Browse.Settings.current_games_browse_layout = "list";
        screen.mediaGrid.setCurrentIndexImmediate(idx);
        tryCompare(screen.listCard, "visible", true);
        tryVerify(() => hasVisibleText(screen.listCard, "Sonic CD"), 1000, "list name should render");
        tryVerify(() => hasVisibleText(screen.listCard, "US"), 1000, "list token should render inline");
    }

    // Round 11: a folder/root row's dim suffix is the item count, not
    // disambiguating tags -- composed by Format.rowSuffix and threaded
    // through the same `entryType`/`fileCount` roles in both layouts (see
    // BrowseList.qml/PagedGrid.qml/Tile.qml).
    function test_folder_item_count_renders_inline_in_grid_and_list(): void {
        mediaModel.append({
            "name": "RPGs",
            "fileStem": "RPGs",
            "coverKey": "",
            "favorite": 0,
            "hidden": false,
            "disambiguatingTags": "",
            "isEmpty": false,
            "entryType": "directory",
            "fileCount": 5,
            "disabled": false
        });
        const idx = mediaModel.count - 1;
        const expected = Format.count(5);
        tryCompare(screen.mediaGrid, "itemCount", mediaModel.count);

        Browse.Settings.current_games_browse_layout = "grid";
        screen.mediaGrid.setCurrentIndexImmediate(idx);
        tryVerify(() => hasVisibleText(screen.mediaGrid, "RPGs"), 1000, "grid name should render");
        tryVerify(() => hasVisibleText(screen.mediaGrid, expected), 1000, "grid item count should render inline");

        Browse.Settings.current_games_browse_layout = "list";
        screen.mediaGrid.setCurrentIndexImmediate(idx);
        tryCompare(screen.listCard, "visible", true);
        tryVerify(() => hasVisibleText(screen.listCard, "RPGs"), 1000, "list name should render");
        tryVerify(() => hasVisibleText(screen.listCard, expected), 1000, "list item count should render inline");
    }

    // A media row's own entryType ("media", the default) must not pick up
    // a folder-style item count from a stray fileCount value.
    function test_media_row_ignores_file_count(): void {
        mediaModel.append({
            "name": "Some Game",
            "fileStem": "Some Game",
            "coverKey": "",
            "favorite": 0,
            "hidden": false,
            "disambiguatingTags": "",
            "isEmpty": false,
            "entryType": "media",
            "fileCount": 5,
            "disabled": false
        });
        const idx = mediaModel.count - 1;
        const suffix = Format.count(5);
        tryCompare(screen.mediaGrid, "itemCount", mediaModel.count);

        Browse.Settings.current_games_browse_layout = "grid";
        screen.mediaGrid.setCurrentIndexImmediate(idx);
        tryVerify(() => hasVisibleText(screen.mediaGrid, "Some Game"), 1000, "grid name should render");
        verify(!hasVisibleText(screen.mediaGrid, suffix), "a media row must not show a folder item count");
    }
}
