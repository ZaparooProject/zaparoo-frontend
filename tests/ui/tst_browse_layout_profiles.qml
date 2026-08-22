// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

// cxx-qt 0.8 exposes singletons without `isFinal` on method entries, so every
// Browse.Settings write trips "Member can be shadowed". The profile sub-property
// accesses (layoutProfile.surface.cardRadius etc.) are on JS-object vars and
// cannot be statically typed. Both are structural; suppress the compiler category.
// qmllint disable compiler
import QtQuick
import QtTest
import Zaparoo.App
import Zaparoo.Browse as Browse

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
        main.systemsScreenRequested = true;
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
        compare(main.systemsScreen.systemsGrid.layoutProfile.surface.cardRadius, 2);
        compare(main.systemsScreen.systemsGrid.layoutProfile.surface.rowRadius, 1);
        compare(main.systemsScreen.systemsGrid.leftInset, 4);
        compare(main.systemsScreen.systemsGrid.rightInset, 4);
        compare(main.systemsScreen.systemsGrid.layoutProfile.grid.pageChevronSize, 8);
    }

    // Non-CRT hosts the count badge and page cue on the top strip's title
    // line (`pageIndicatorMode`); CRT's top strip is hidden entirely
    // (`status.topStripVisible: false`), so it keeps them in the footer
    // instead, unchanged from before this round --
    // `footer.pageCueInFooter` is the profile flag both screens key off
    // (BrowseLayouts.qml). Asserts the profile-derived flags directly
    // rather than final `.visible` (which also folds in this harness's
    // own loading-gate state, unrelated to what's under test here).
    function test_default_grid_shows_page_cue_at_top_crt_shows_it_in_footer(): void {
        compare(main.systemsScreen._pageCueInFooter, false);
        compare(main.systemsScreen.topStrip.pageIndicatorMode, true);
        const footerCount = findChild(main.systemsScreen, "systemsFooterCount");
        const footerIndicator = findChild(main.systemsScreen, "systemsFooterPageIndicator");
        verify(footerCount !== null);
        verify(footerIndicator !== null);

        main.crtNativePath = true;
        compare(main.systemsScreen._pageCueInFooter, true);
        compare(main.systemsScreen.topStrip.pageIndicatorMode, false);
    }

    function test_crt_list_uses_crt_header_and_profile(): void {
        main.crtNativePath = true;
        Browse.Settings.current_browse_layout = "list";

        compare(main.headerBar.layoutProfile.header.titleInHeader, true);
        compare(main.systemsScreen.listCard.layoutProfile.surface.cardRadius, 2);
        compare(main.systemsScreen.listCard.layoutProfile.surface.rowRadius, 1);
        compare(main.systemsScreen.listCard.layoutProfile.list.rowHeight, 12);
    }
}
