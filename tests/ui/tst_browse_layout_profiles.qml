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
        _originalBrowseLayout = Browse.Settings.current_systems_browse_layout;
    }

    function init(): void {
        main.width = 1280;
        main.height = 720;
        Sizing.screenWidth = 1280;
        Sizing.screenHeight = 720;
        main.bootComplete = true;
        main.systemsScreenRequested = true;
        main.activeScreen = main.screenSystems;
        main.crtNativePath = false;
        Browse.Settings.current_systems_browse_layout = "grid";
    }

    function cleanup(): void {
        main.width = 1280;
        main.height = 720;
        Sizing.screenWidth = 1280;
        Sizing.screenHeight = 720;
        main.crtNativePath = false;
        Browse.Settings.current_systems_browse_layout = _originalBrowseLayout;
    }

    function _use240p(): void {
        main.width = 640;
        main.height = 240;
        Sizing.screenWidth = 640;
        Sizing.screenHeight = 240;
    }

    function test_compact_profile_follows_resolution_not_crt_flag(): void {
        compare(BrowseLayouts.currentThemeId, "default");
        main.crtNativePath = true;
        compare(BrowseLayouts.currentThemeId, "default");
        main.crtNativePath = false;
        _use240p();
        compare(BrowseLayouts.currentThemeId, "crt");
    }

    function test_240p_grid_uses_compact_tile_profile(): void {
        _use240p();
        Browse.Settings.current_systems_browse_layout = "grid";

        compare(main.headerBar.layoutProfile.header.titleInHeader, true);
        compare(main.systemsScreen.systemsGrid.layoutProfile.surface.cardRadius, Sizing.radiusMd);
        compare(main.systemsScreen.systemsGrid.layoutProfile.surface.rowRadius, Sizing.radiusSm);
        compare(main.systemsScreen.systemsGrid.leftInset, Sizing.headerSideMargin);
        compare(main.systemsScreen.systemsGrid.rightInset, Sizing.headerSideMargin);
        compare(main.systemsScreen.systemsGrid.layoutProfile.grid.pageChevronSize, 8);
    }

    // Larger layouts host count and page cue on top strip. Compact 240p
    // layout keeps one cue in footer and removes top-strip duplicate.
    function test_240p_keeps_only_footer_page_cue(): void {
        compare(main.systemsScreen._pageCueInFooter, false);
        compare(main.systemsScreen.topStrip.pageIndicatorMode, true);
        const footerCount = findChild(main.systemsScreen, "systemsFooterCount");
        const footerIndicator = findChild(main.systemsScreen, "systemsFooterPageIndicator");
        verify(footerCount !== null);
        verify(footerIndicator !== null);

        _use240p();
        Browse.Settings.current_systems_browse_layout = "list";
        compare(main.systemsScreen._pageCueInFooter, true);
        compare(main.systemsScreen.topStrip.visible, false);
        compare(main.systemsScreen.topStrip.pageIndicatorMode, false);
    }

    function test_240p_list_aligns_with_grid_rails(): void {
        _use240p();
        Browse.Settings.current_systems_browse_layout = "list";

        const profile = main.systemsScreen.listCard.layoutProfile;
        compare(main.headerBar.layoutProfile.header.titleInHeader, true);
        compare(profile.surface.cardRadius, Sizing.radiusMd);
        compare(profile.surface.rowRadius, Sizing.radiusSm);
        compare(profile.list.rowHeight, 12);
        compare(profile.list.cardSideMargin, Sizing.headerSideMargin);
        compare(profile.list.cardTopMargin, 2);
        compare(profile.list.cardBottomMargin, Sizing.pctH(6) + 12);
        compare(profile.status.topStripVisible, false);

        const gamesProfile = BrowseLayouts.themeProfile(BrowseLayouts.currentThemeId, "gamesList");
        compare(gamesProfile.list.cardSideMargin, Sizing.headerSideMargin);
        compare(gamesProfile.list.cardTopMargin, 2);
        compare(gamesProfile.list.cardBottomMargin, Sizing.pctH(6) + 12);
        compare(gamesProfile.status.topStripVisible, false);
    }
}
