// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

import QtQuick
import QtTest
import Zaparoo.App
import Zaparoo.Browse as Browse
import Zaparoo.Theme

// Verifies that browse screens route geometry through the shared
// BrowseLayouts profiles instead of inlining CRT-specific numbers in
// each screen/component. The goal is not to snapshot every pixel, just
// to prove the live tree picks the intended profile in the key modes.
TestCase {
    name: "UiBrowseLayoutProfiles"
    when: windowShown

    Main {
        id: main
        fullScreen: false
        width: 1280
        height: 720
    }

    property string _originalBrowseLayout: "grid"

    Component.onCompleted: {
        _originalBrowseLayout = Browse.Settings.current_browse_layout;
    }

    function init(): void {
        main.bootComplete = true;
        main.activeScreen = main.screenSystems;
        main.crtNativePath = false;
        Browse.Settings.current_browse_layout = "grid";
    }

    function cleanup(): void {
        main.crtNativePath = false;
        Browse.Settings.current_browse_layout = _originalBrowseLayout;
    }

    function test_crt_grid_uses_crt_tile_profile(): void {
        main.crtNativePath = true;
        Browse.Settings.current_browse_layout = "grid";

        compare(main.headerBar.layoutProfile.header.titleInHeader, true);
        compare(main.systemsScreen.systemsGrid.layoutProfile.surface.cornerRadius, 4);
        compare(main.systemsScreen.systemsGrid.leftInset, 4);
        compare(main.systemsScreen.systemsGrid.gutterWidth, 8);
        compare(main.systemsScreen.systemsGrid.scrollArrowSize, 8);
    }

    function test_crt_list_uses_crt_header_and_profile(): void {
        main.crtNativePath = true;
        Browse.Settings.current_browse_layout = "list";

        compare(main.headerBar.layoutProfile.header.titleInHeader, true);
        compare(main.systemsScreen.listCard.layoutProfile.surface.cornerRadius, 4);
        compare(main.systemsScreen.listCard.layoutProfile.list.rowHeight, 12);
        compare(main.systemsScreen.listCard.layoutProfile.list.scrollbarGap, 2);
    }
}
