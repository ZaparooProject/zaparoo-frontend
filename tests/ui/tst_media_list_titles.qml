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

TestCase {
    id: testCase

    name: "UiMediaListTitles"
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
        activeLabelTextProvider: () => testCase.displayTitleAt(screen.mediaGrid.currentIndex)
    }

    function init(): void {
        Sizing.screenWidth = testCase.width;
        Sizing.screenHeight = testCase.height;
        Browse.Settings.current_browse_layout = "grid";
        mediaModel.clear();
        mediaModel.append({
            "name": "D (Disc 1)",
            "fileStem": "D",
            "coverKey": "",
            "favorite": 0,
            "hidden": false
        });
        mediaModel.append({
            "name": "D (Disc 2)",
            "fileStem": "D",
            "coverKey": "",
            "favorite": 0,
            "hidden": false
        });
        mediaModel.append({
            "name": "Friendly Alias",
            "fileStem": "InternalContainer",
            "coverKey": "",
            "favorite": 0,
            "hidden": false
        });
        tryCompare(screen.mediaGrid, "itemCount", mediaModel.count);
        screen.mediaGrid.setCurrentIndexImmediate(0);
    }

    function cleanup(): void {
        Browse.Settings.current_browse_layout = _originalBrowseLayout;
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
        Browse.Settings.current_browse_layout = "grid";
        screen.mediaGrid.setCurrentIndexImmediate(index);
        tryCompare(screen.activeLabel, "text", expected);
        tryVerify(() => hasVisibleText(screen.mediaGrid, expected), 1000, "grid title should render " + expected);

        Browse.Settings.current_browse_layout = "list";
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
}
