// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
// cxx-qt 0.8 singleton methods aren't marked final so every Browse.* call trips
// "Member can be shadowed". Screen properties accessed via Loader QObject are
// QVariant at the qmllint level. Both are structural; suppress compiler category.
// qmllint disable compiler

import QtQuick
import QtTest
import Zaparoo.App
import Zaparoo.Browse as Browse
import Zaparoo.Theme

// Exercises the hub ↔ systems ↔ games navigation state machine defined
// in Main.qml. State is driven either by writing to the activeScreen
// property (the observable contract) or by calling root.handleKey(key)
// directly — the latter proves the Keys.onPressed routing, which we
// can't exercise with keyClick because offscreen ApplicationWindows
// don't receive routed key events reliably.
TestCase {
    id: testCase

    name: "UiNavigation"
    when: windowShown

    // View preferences persist to disk; restore machine's original values.
    property string _originalFavoritesSort: ""
    property string _originalFavoritesGrouping: "none"
    property bool _originalGamesFavoritesFilter: false
    property string _originalHubCategory: ""
    property int _originalHubSelectedRow: 0
    property string _originalHubSelectedAction: ""

    Main {
        id: main
        fullScreen: false
        width: 1280
        height: 720
    }

    Component.onCompleted: {
        testCase._originalFavoritesSort = Browse.FavoritesModel.sort_mode ?? "";
        testCase._originalFavoritesGrouping = Browse.Settings.current_favorites_grouping ?? "none";
        testCase._originalGamesFavoritesFilter = Browse.GamesState.favorites_filter === true;
        testCase._originalHubCategory = Browse.HubState.category ?? "";
        testCase._originalHubSelectedRow = Browse.HubState.selected_row ?? 0;
        testCase._originalHubSelectedAction = Browse.HubState.selected_action ?? "";
    }

    function init(): void {
        // Disable motion so DeferredAction.arm() runs dispatch synchronously.
        // Navigation tests assert state immediately after handleKey/handleAction;
        // a 90 ms async lead would cause every accept test to fail.
        Motion.enabled = false;
        // The cold-launch BootOverlay normally hides every screen until
        // Core's catalog reaches READY. Tests don't run a real Core, so
        // we mark the boot complete up-front; otherwise every visibility
        // assertion below would fail against the boot curtain.
        main.bootComplete = true;
        // Warm-resume restore is asynchronous. Clear its complete callback
        // graph so a stale readiness edge cannot route after test setup.
        main._startupRestorePending = false;
        main._startupRestoreStarted = false;
        main._startupRestoreScreen = "";
        main._screenReadyCallbacks = ({});
        main._categoryReadyCallback = null;
        main._systemReadyCallback = null;
        main._favoritesReadyCallback = null;
        main._favoriteSystemsReadyCallback = null;
        main._recentsReadyCallback = null;
        main.startupRestoreCurtainVisible = false;
        main.systemsScreenRequested = true;
        main.gamesScreenRequested = true;
        main.favoritesScreenRequested = true;
        main.favoriteSystemsScreenRequested = true;
        main.recentsScreenRequested = true;
        main.settingsScreenRequested = true;
        main.activeScreen = main.screenHub;
        main.pendingTransition = "";
        main.systemsCoverRevealReady = true;
        main.gamesCoverRevealReady = true;
        main.gamesNavigationInputAt = 0;
        main.gamesNavigationModelReadyAt = 0;
        main.gamesNavigationAction = "";
        main.gamesScreen.lastNavigationInputAt = 0;
        main._firstRunIndexStarted = false;
        tryCompare(main, "transitionCueVisible", false);
        // Hub focus is two rows now (categories + actions); reset both
        // axes so a prior test's row-jump doesn't leak into the next.
        main.hubScreen._focusArmed = false;
        main.hubScreen.resetFocus();
        // Cancel any in-flight dpad-repeat timer left over from a prior
        // test — handleKey(dpad) arms a 350 ms initial timer and tests
        // run in microseconds, so the pending fire would land on the
        // next test if we didn't reset it here.
        main._stopRepeat();
        main._resetRapidNavigation();
        main._setFavoritesSystem("");
        Browse.Settings.set_favorites_grouping("none");
    }

    function cleanup(): void {
        Motion.enabled = true;
        // A modal left open swallows every routed action, so the next test
        // would fail for a reason that has nothing to do with what it tests.
        if (main.listPickerModalVisible)
            main.closeListPickerModal();
        if (main.randomFailedModalVisible)
            main.closeRandomFailedModal();
        // Restore persistent preferences changed by menu-routing tests.
        Browse.FavoritesModel.set_sort_mode(testCase._originalFavoritesSort);
        Browse.Settings.set_favorites_grouping(testCase._originalFavoritesGrouping);
        main._setFavoritesSystem("");
        Browse.GamesModel.apply_favorites_filter(testCase._originalGamesFavoritesFilter);
        Browse.GamesState.favorites_filter = testCase._originalGamesFavoritesFilter;
        Browse.GamesModel.total_files = 0;
        Browse.HubState.category = testCase._originalHubCategory;
        Browse.HubState.selected_row = testCase._originalHubSelectedRow;
        Browse.HubState.selected_action = testCase._originalHubSelectedAction;
    }

    function test_media_screen_requests_sync_cover_size(): void {
        main.gamesScreenRequested = false;
        main.favoritesScreenRequested = false;
        main.recentsScreenRequested = false;

        Browse.GamesModel.set_cover_max_size(0);
        main._requestScreen(main.screenFavorites);
        compare(Browse.GamesModel.cover_max_size, main._gamesCoverMaxSize);
        verify(Browse.GamesModel.cover_max_size > 0);

        Browse.GamesModel.set_cover_max_size(0);
        main._requestScreen(main.screenRecents);
        compare(Browse.GamesModel.cover_max_size, main._gamesCoverMaxSize);
        verify(Browse.GamesModel.cover_max_size > 0);
    }

    function test_first_run_index_starts_only_from_authoritative_empty_state(): void {
        compare(main._shouldStartFirstRunIndex(2, true, true, 0), true);
        compare(main._shouldStartFirstRunIndex(1, true, true, 0), false);
        compare(main._shouldStartFirstRunIndex(2, false, true, 0), false);
        compare(main._shouldStartFirstRunIndex(2, true, false, 0), false);
        compare(main._shouldStartFirstRunIndex(2, true, true, 1), false);
        main._firstRunIndexStarted = true;
        compare(main._shouldStartFirstRunIndex(2, true, true, 0), false);
    }

    function test_catalog_polling_is_limited_to_system_membership_screens(): void {
        main.activeScreen = main.screenHub;
        compare(main._catalogRefreshScreenActive(), true);
        main.activeScreen = main.screenSystems;
        compare(main._catalogRefreshScreenActive(), true);
        main.activeScreen = main.screenFavoriteSystems;
        compare(main._catalogRefreshScreenActive(), true);
        main.activeScreen = main.screenGames;
        compare(main._catalogRefreshScreenActive(), false);
        main.activeScreen = main.screenFavorites;
        compare(main._catalogRefreshScreenActive(), false);
    }

    function test_initial_state_is_hub(): void {
        compare(main.activeScreen, main.screenHub);
        compare(main.hubScreen.visible, true);
        compare(main.hubScreen.currentRow, 1, "Cold optimistic Hub should start on Resume");
        compare(main.hubScreen.currentIndex, 0, "Resume is the first optimistic action");
        compare(main.systemsScreen.visible, false);
        compare(main.gamesScreen.visible, false);
    }

    // Hard-cut peer screens: only the active screen is visible at any
    // time. `visible` binds directly to `root.activeScreen === ...` in
    // MainLayout, so the swap is synchronous with the assignment.
    function test_activating_systems_screen_makes_systems_visible(): void {
        main.activeScreen = main.screenSystems;
        compare(main.systemsScreen.visible, true);
        compare(main.hubScreen.visible, false);
        compare(main.gamesScreen.visible, false);
    }

    function test_activating_games_screen_makes_games_visible(): void {
        main.activeScreen = main.screenGames;
        compare(main.gamesScreen.visible, true);
        compare(main.hubScreen.visible, false);
        compare(main.systemsScreen.visible, false);
    }

    function test_activating_update_screen_makes_update_visible(): void {
        main.activeScreen = main.screenUpdate;
        compare(main.updateScreen.visible, true);
        compare(main.hubScreen.visible, false);
        compare(main.systemsScreen.visible, false);
    }

    function test_help_bar_stays_stable_during_forward_transition(): void {
        main.activeScreen = main.screenHub;
        const before = JSON.stringify(main.helpEntries);
        verify(before !== "[]");

        main.pendingTransition = "systems";
        compare(JSON.stringify(main.helpEntries), before);
    }

    function test_games_rapid_scroll_snapshot_crops_to_cell_area(): void {
        const snapshot = findChild(main.gamesScreen, "rapidScrollSnapshot");
        const snapshotImage = findChild(main.gamesScreen, "rapidScrollSnapshotImage");
        verify(snapshot !== null);
        verify(snapshotImage !== null);
        compare(snapshot.x, main.gamesScreen.gamesGrid.x + main.gamesScreen.gamesGrid.leftInset);
        compare(snapshot.y, main.gamesScreen.gamesGrid.y + main.gamesScreen.gamesGrid.topInset);
        compare(snapshot.width, main.gamesScreen.gamesGrid._contentWidth);
        compare(snapshot.height, main.gamesScreen.gamesGrid.rows * main.gamesScreen.gamesGrid.cellHeight + Math.max(0, main.gamesScreen.gamesGrid.rows - 1) * main.gamesScreen.gamesGrid.cellSpacingY);
        compare(snapshotImage.sourceClipRect.x, main.gamesScreen.gamesGrid.leftInset);
        compare(snapshotImage.sourceClipRect.y, main.gamesScreen.gamesGrid.topInset);
        compare(snapshotImage.opacity, 0.28);
    }

    function test_games_deep_page_restore_hides_page_one_until_selection_found(): void {
        main.activeScreen = main.screenGames;
        main._pendingGameRestorePath = "/saved/parent/page-three-game.zip";
        compare(main.gamesSelectionRestorePending, true);
        compare(main.gamesScreen.optimisticLoading, true);
        compare(main.gamesScreen._gateHide, true, "first loaded page must not paint during deep-page restore");
        main._pendingGameRestorePath = "";
    }

    function test_game_covers_wait_for_model_frame(): void {
        main.activeScreen = main.screenGames;
        main.gamesCoverRevealReady = false;
        compare(main.gamesScreen.gamesGrid.coverRequestsEnabled, false);

        main.gamesCoverRevealReady = true;
        compare(main.gamesScreen.gamesGrid.coverRequestsEnabled, true);
    }

    function test_games_header_extracts_current_folder_name(): void {
        compare(main.gamesScreen._folderNameForPath("/media/fat/games/SNES/RPGs"), "RPGs");
        compare(main.gamesScreen._folderNameForPath("/media/fat/games/SNES/RPGs/"), "RPGs");
        compare(main.gamesScreen._folderNameForPath("C:\\Games\\SNES\\RPGs"), "RPGs");
        compare(main.gamesScreen._folderNameForPath(""), "");
    }

    function test_folder_navigation_timing_uses_input_timestamp(): void {
        const inputAt = Date.now() - 25;
        main.gamesScreen.lastNavigationInputAt = inputAt;

        main._beginFolderNavigationTiming("back");

        compare(main.gamesNavigationInputAt, inputAt);
        compare(main.gamesNavigationModelReadyAt, 0);
        compare(main.gamesNavigationAction, "back");
        compare(main.gamesScreen.lastNavigationInputAt, 0);
    }

    function test_system_covers_wait_for_destination_frame(): void {
        main.activeScreen = main.screenSystems;
        main.systemsCoverRevealReady = false;
        compare(main.systemsScreen.coverRevealReady, false);
        compare(main.systemsScreen.systemsGrid.coverRequestsEnabled, false);

        main.systemsCoverRevealReady = true;
        compare(main.systemsScreen.coverRevealReady, true);
    }

    function test_system_grid_withholds_covers_during_transition(): void {
        main.activeScreen = main.screenHub;
        main.pendingTransition = "systems";
        compare(main.systemsScreen.systemsGrid.suspendDelegates, false);
        compare(main.systemsScreen.systemsGrid.coverRequestsEnabled, false);
        compare(main.systemsScreen.systemsGrid.coverLookaheadPages, 0);
        compare(main.systemsScreen.systemsGrid.eagerFocusedCovers, false);

        main.pendingTransition = "";
        main.activeScreen = main.screenSystems;
        compare(main.systemsScreen.preparingTransition, false);
    }

    function test_transition_timing_closes_on_presented_frame(): void {
        main._beginTransitionTiming("accept");
        main._markTransitionRouted(main.screenSystems);
        compare(main._transitionAction, "accept");
        compare(main._transitionFromScreen, main.screenHub);
        compare(main._transitionToScreen, main.screenSystems);
        verify(main._transitionRouteAt > 0);

        main._finishTransitionTiming();
        compare(main._transitionInputStartedAt, 0);
        compare(main._transitionRouteAt, 0);
        compare(main._transitionToScreen, "");
    }

    // Enter on an optimistic placeholder category starts the normal
    // systems loading transition and preserves the visible category
    // name instead of treating the row as empty.
    function test_enter_on_optimistic_hub_category_starts_systems_transition(): void {
        main.hubScreen.currentRow = 0;
        main.hubScreen.currentIndex = 0;
        main.handleKey(Qt.Key_Return);
        compare(main.pendingTransition, "systems");
        compare(Browse.HubState.category, "Arcade");
        compare(main.activeScreen, main.screenHub, "Optimistic route stays under the loading cue until catalog readiness is authoritative");
    }

    // Down on hub moves focus between the categories row and the
    // actions row (Favorites / Recently Played / optional Update /
    // Settings); it must never flip off-screen to systems. Accept is
    // the only path that drills into another screen.
    function test_down_on_hub_does_not_route_to_systems(): void {
        main.handleKey(Qt.Key_Down);
        compare(main.activeScreen, main.screenHub, "Down on hub must not flip to systems — only Accept drills");
    }

    function test_enter_on_bottom_landing_routes_to_expected_action(): void {
        // qmllint disable compiler
        // Focus Update only when the build and current network state expose
        // it; otherwise Settings is the production empty-catalog fallback.
        const expectedAction = main.hubScreen._emptyCatalogFallbackAction;
        main.hubScreen._focusAction(main.hubScreen._actionIndexForId(expectedAction));
        compare(main.hubScreen.actionEntries[main.hubScreen.currentIndex].id, expectedAction);
        // qmllint enable compiler
        main.handleKey(Qt.Key_Return);
        compare(main.activeScreen, expectedAction === "update" ? main.screenUpdate : main.screenSettings);
    }

    function test_favorite_systems_grid_matches_system_tile_layout(): void {
        compare(main.favoriteSystemsScreen.gridShowCaption, false);
        compare(main.favoriteSystemsScreen.gridColumnsOverride, main.systemsScreen.systemsGrid.columns);
        compare(main.favoriteSystemsScreen.gridRowsOverride, main.systemsScreen.systemsGrid.rows);
    }

    function test_flat_favorites_uses_unbounded_page_chrome(): void {
        compare(main.favoritesScreen.paginationTotalKnown, false);
        compare(main.favoritesScreen.favoritesGrid.paginationTotalKnown, false);
        verify(main.favoritesScreen.topStrip.pageText.indexOf("/") < 0);
    }

    function test_recents_uses_unbounded_page_chrome(): void {
        compare(main.recentsScreen.paginationTotalKnown, false);
        compare(main.recentsScreen.recentsGrid.paginationTotalKnown, false);
        verify(main.recentsScreen.topStrip.pageText.indexOf("/") < 0);
    }

    function test_hub_favorites_action_uses_favorite_systems_mode(): void {
        Browse.Settings.set_favorites_grouping("system");
        main.hubScreen.currentRow = 1;
        main.hubScreen.currentIndex = main.hubScreen._actionIndexForId("favorites");
        main.handleKey(Qt.Key_Return);
        compare(main.pendingTransition, "favorite_systems");
    }

    function test_hub_favorites_action_uses_all_favorites_mode(): void {
        Browse.Settings.set_favorites_grouping("none");
        main.hubScreen.currentRow = 1;
        main.hubScreen.currentIndex = main.hubScreen._actionIndexForId("favorites");
        main.handleKey(Qt.Key_Return);
        compare(main.pendingTransition, "favorites");
    }

    function test_scoped_favorites_back_routes_through_favorite_systems(): void {
        Browse.Settings.set_favorites_grouping("system");
        main._setFavoritesSystem("SNES");
        main.activeScreen = main.screenFavorites;
        main.favoritesScreen.handleAction("cancel");
        verify(main.activeScreen === main.screenFavoriteSystems || (main.pendingTransition === "back" && main._backTransitionTarget === main.screenFavoriteSystems), "Scoped Back targets Favorite Systems even while its model is loading");

        main.pendingTransition = "";
        main._backTransitionTarget = "";
        main.activeScreen = main.screenFavoriteSystems;
        main.favoriteSystemsScreen.handleAction("cancel");
        compare(main.activeScreen, main.screenHub);
    }

    function test_flat_favorites_back_routes_to_hub(): void {
        Browse.Settings.set_favorites_grouping("none");
        main._setFavoritesSystem("");
        main.activeScreen = main.screenFavorites;
        main.handleKey(Qt.Key_Escape);
        compare(main.activeScreen, main.screenHub);
    }

    // Enter on an empty systems screen retries the current load (the
    // help bar's [OK] RETRY contract); it must not flip to games. The
    // test harness has no live catalog, so Systems is always Empty
    // here — the Ready-state drill into games is exercised live.
    function test_enter_on_empty_systems_does_not_flip_to_games(): void {
        main.activeScreen = main.screenSystems;
        main.handleKey(Qt.Key_Return);
        compare(main.activeScreen, main.screenSystems, "Enter on an empty systems screen must retry, not flip to games");
    }

    // Escape on games goes straight back to systems when the destination
    // screen is already mounted and its model is idle.
    function test_escape_on_games_returns_to_systems(): void {
        main.activeScreen = main.screenGames;
        main.handleKey(Qt.Key_Escape);
        compare(main.pendingTransition, "");
        compare(main.activeScreen, main.screenSystems);
        tryCompare(main, "transitionCueVisible", false);
    }

    // Escape on systems goes straight back to Hub; Hub has no model fill to wait on.
    function test_escape_on_systems_returns_to_hub(): void {
        main.activeScreen = main.screenSystems;
        main.handleKey(Qt.Key_Escape);
        compare(main.pendingTransition, "");
        compare(main.activeScreen, main.screenHub);
        tryCompare(main, "transitionCueVisible", false);
    }

    function test_escape_on_update_returns_to_hub(): void {
        main.activeScreen = main.screenUpdate;
        main.handleKey(Qt.Key_Escape);
        compare(main.activeScreen, main.screenHub);
    }

    function test_hub_screen_allows_screensaver(): void {
        compare(main._allowsScreensaver(main.screenHub), true);
    }

    // Up on systems is a grid-internal move; at the top row (or on an
    // empty grid in the test harness) it no-ops rather than flipping
    // back to hub. Escape is the only back path.
    function test_up_on_empty_systems_does_not_return_to_hub(): void {
        main.activeScreen = main.screenSystems;
        main.handleKey(Qt.Key_Up);
        compare(main.activeScreen, main.screenSystems, "Up on systems must not flip to hub — Escape is the back path");
    }

    // Backspace is aliased to Escape in every branch.
    function test_backspace_behaves_like_escape_on_games(): void {
        main.activeScreen = main.screenGames;
        main.handleKey(Qt.Key_Backspace);
        compare(main.pendingTransition, "");
        compare(main.activeScreen, main.screenSystems);
        tryCompare(main, "transitionCueVisible", false);
    }

    function test_settings_root_grid_opens_category_and_returns(): void {
        main.activeScreen = main.screenSettings;
        main.settingsScreen.optimisticLoading = false;
        main.settingsScreen.currentPage = main.settingsScreen.pageRoot;
        compare(main.settingsScreen.rootGridColumns, 3);
        compare(main.settingsScreen.rootGridRows, 2);
        main.settingsScreen.currentIndex = 0;
        // 3x2 grid of six category tiles: down moves a full row, up returns.
        main.handleAction("down");
        compare(main.settingsScreen.currentIndex, main.settingsScreen.rootGridColumns);
        main.handleAction("up");
        compare(main.settingsScreen.currentIndex, 0);
        main.handleAction("right");
        compare(main.settingsScreen.currentIndex, 1);
        main.handleAction("accept");
        compare(main.settingsScreen.currentPage, main.settingsScreen.pageBrowsing);
        main.handleAction("cancel");
        compare(main.settingsScreen.currentPage, main.settingsScreen.pageRoot);
        main.handleAction("cancel");
        compare(main.activeScreen, main.screenHub);
    }

    // Cross-row mapping. The test harness has no live CategoriesModel
    // so we can't drive the full handleAction("down") flow with real
    // categories — instead we unit-test the pure arithmetic helper
    // that owns the math. The shape verifies centered row mapping and
    // a couple of degenerate cases.
    function test_cross_row_4_over_2_down(): void {
        const map = main.hubScreen._mapCrossRow;
        compare(map(0, 4, 2), 0, "Down from top[0] (a) → bottom[0] (e)");
        compare(map(1, 4, 2), 0, "Down from top[1] (b) → bottom[0] (e)");
        compare(map(2, 4, 2), 1, "Down from top[2] (c) → bottom[1] (f)");
        compare(map(3, 4, 2), 1, "Down from top[3] (d) → bottom[1] (f)");
    }

    function test_cross_row_4_over_2_up(): void {
        const map = main.hubScreen._mapCrossRow;
        compare(map(0, 2, 4), 1, "Up from bottom[0] (e) → top[1] (b)");
        compare(map(1, 2, 4), 2, "Up from bottom[1] (f) → top[2] (c)");
    }

    // 4-over-3 — the offset is 0.5,
    // so Math.round's half-toward-+∞ rounds the boundary cells right.
    function test_cross_row_4_over_3(): void {
        const map = main.hubScreen._mapCrossRow;
        compare(map(0, 4, 3), 0);
        compare(map(1, 4, 3), 1);
        compare(map(2, 4, 3), 2);
        compare(map(3, 4, 3), 2, "Rightmost top clamps onto rightmost bottom");
    }

    function test_cross_row_equal_counts_is_identity(): void {
        const map = main.hubScreen._mapCrossRow;
        compare(map(0, 3, 3), 0);
        compare(map(1, 3, 3), 1);
        compare(map(2, 3, 3), 2);
    }

    function test_cross_row_empty_destination_returns_zero(): void {
        const map = main.hubScreen._mapCrossRow;
        compare(map(2, 4, 0), 0, "Degenerate destCount=0 returns 0 — caller guards the no-op");
    }

    // _preferOverride is the pure half of the Hub override resolution: given
    // the override-lookup result (empty when none) and the bundled fallback,
    // it picks the override when non-empty. The Browse.ImageOverrides lookup
    // itself is exercised by the Rust image_overrides tests, not here.
    function test_prefer_override_uses_override_when_present(): void {
        const pick = main.hubScreen._preferOverride;
        compare(pick("custom-image//media/fat/zaparoo/custom/hub/favorites.png", "icons/HeartOutline"), "custom-image//media/fat/zaparoo/custom/hub/favorites.png");
    }

    function test_prefer_override_falls_back_on_empty(): void {
        const pick = main.hubScreen._preferOverride;
        compare(pick("", "icons/HeartOutline"), "icons/HeartOutline", "Empty override falls back to bundled key");
    }

    // Up on the top row wraps onto the bottom row (the two rows form a
    // closed loop). Test harness has no live categories, so we start
    // at top[0] and just verify currentRow flipped — the destination
    // index is verified by the _mapCrossRow tests above.
    function test_up_on_top_row_wraps_to_bottom_row(): void {
        main.hubScreen.currentRow = 0;
        main.hubScreen.currentIndex = 0;
        main.handleKey(Qt.Key_Up);
        compare(main.hubScreen.currentRow, 1, "Up from top should wrap to bottom row");
    }

    // Bottom row wraps left/right. During optimistic boot the Hub has
    // four placeholder categories and four actions (Resume still
    // visible until history proves otherwise), so Down from top[0]
    // lands at bottom[0].
    function test_bottom_row_right_wraps_to_first(): void {
        main.hubScreen.currentRow = 0;
        main.hubScreen.currentIndex = 0;
        main.handleKey(Qt.Key_Down);
        compare(main.hubScreen.currentRow, 1);
        main.hubScreen.currentIndex = main.hubScreen.actionEntries.length - 1;
        main.handleKey(Qt.Key_Right);
        compare(main.hubScreen.currentIndex, 0, "Right at last bottom-row index wraps to first");
    }

    function test_bottom_row_left_wraps_to_last(): void {
        main.hubScreen.currentRow = 0;
        main.hubScreen.currentIndex = 0;
        main.handleKey(Qt.Key_Down);
        compare(main.hubScreen.currentRow, 1);
        main.hubScreen.currentIndex = 0;
        main.handleKey(Qt.Key_Left);
        compare(main.hubScreen.currentIndex, main.hubScreen.actionEntries.length - 1, "Left at first bottom-row index wraps to last");
    }

    // Cross-row round-trip. With unequal row counts, the centered
    // visual-nearest map can't always return Up/Down to the tile a
    // previous cross originated from. The fix is `_crossSavedIndex`:
    // each cross saves the source-row index, the next cross restores
    // it, any horizontal input on the destination row invalidates it.

    // After Down from top[0], the saved index must hold 0 so the next
    // Up can return there. `_mapCrossRow(0, topCount=0, bottomCount)`
    // chooses the centered action-row landing.
    function test_cross_row_arms_saved_source_index(): void {
        main.hubScreen.currentRow = 0;
        main.hubScreen.currentIndex = 0;
        main.handleKey(Qt.Key_Down);
        compare(main.hubScreen.currentRow, 1);
        compare(main.hubScreen._crossSavedIndex, 0, "Down from top[0] must save 0 for the round-trip back");
    }

    // Horizontal input on the destination row clears the saved index
    // — the user has now committed to navigating within the new row,
    // so the next cross should fall back to the centered visual map.
    function test_cross_row_horizontal_input_clears_saved_index(): void {
        main.hubScreen.currentRow = 0;
        main.hubScreen.currentIndex = 0;
        main.handleKey(Qt.Key_Down);
        compare(main.hubScreen._crossSavedIndex, 0);
        main.handleKey(Qt.Key_Left);
        compare(main.hubScreen._crossSavedIndex, -1, "Left on the destination row must invalidate the round-trip");
    }

    // Mouse focus is a deliberate landing on a specific tile, same
    // intent as a horizontal arrow press — clear the saved index so a
    // later Up doesn't snap back to a row the user already left.
    function test_cross_row_mouse_focus_clears_saved_index(): void {
        main.hubScreen.currentRow = 0;
        main.hubScreen.currentIndex = 0;
        main.handleKey(Qt.Key_Down);
        compare(main.hubScreen._crossSavedIndex, 0);
        main.hubScreen._focusAction(0);
        compare(main.hubScreen._crossSavedIndex, -1, "Mouse focus on an action tile must invalidate the round-trip");
    }

    // Restore path: when `_crossSavedIndex` is armed and within the
    // destination row's bounds, `_crossRow` uses it directly instead
    // of the centered visual map. The test harness has no live
    // categories, so we drive `_crossRow` synthetically with a
    // pretend top index whose visual map would land somewhere
    // unrelated, then verify the restore won.
    function test_cross_row_uses_saved_index_over_visual_map(): void {
        main.hubScreen.currentRow = 0;
        main.hubScreen.currentIndex = 7;
        main.hubScreen._crossSavedIndex = 1;
        const moved = main.hubScreen._crossRow();
        verify(moved, "_crossRow with non-empty destination must move");
        compare(main.hubScreen.currentRow, 1, "Cross flips to the other row");
        compare(main.hubScreen.currentIndex, 1, "Saved index 1 wins over the visual map");
        compare(main.hubScreen._crossSavedIndex, 7, "After the cross, the saved index points back to the source");
    }

    // Saved index that points past the destination row's count is
    // ignored — the destination layout may have changed since we
    // crossed away. Falls back to the visual map.
    function test_cross_row_out_of_range_saved_index_falls_back(): void {
        main.hubScreen.currentRow = 0;
        main.hubScreen.currentIndex = 0;
        main.hubScreen._crossSavedIndex = 99;
        const moved = main.hubScreen._crossRow();
        verify(moved);
        compare(main.hubScreen.currentRow, 1);
        // The out-of-range saved index (99) must be ignored; focus falls
        // back to a valid centered action-row index rather than the bogus
        // value.
        verify(main.hubScreen.currentIndex >= 0 && main.hubScreen.currentIndex < main.hubScreen.actionEntries.length, "Out-of-range saved index falls back to the visual map");
    }

    // resetFocus is the test-harness reset and the cold-launch state.
    // It must clear the round-trip arm so a prior test's saved index
    // can't leak into the next case.
    function test_reset_focus_clears_saved_index(): void {
        main.hubScreen._crossSavedIndex = 2;
        main.hubScreen.resetFocus();
        compare(main.hubScreen.currentRow, 1);
        compare(main.hubScreen.currentIndex, 0);
        compare(main.hubScreen._crossSavedIndex, -1);
    }

    // Hold-to-repeat (dpad). The repeat state machine is driven by
    // `_armRepeat` (called from handleKey on a dpad press) and
    // unwound by `_stopRepeat` and `handleKeyRelease`. These tests
    // drive the helpers directly to keep the assertion surface
    // narrow — handleKey's outer "fire handleAction then _armRepeat"
    // shape is one trivial line and doesn't need a per-action test
    // that drags real screen logic into the harness.

    function test_is_repeatable_action_accepts_dpad_directions(): void {
        compare(main._isRepeatableAction("up"), true);
        compare(main._isRepeatableAction("down"), true);
        compare(main._isRepeatableAction("left"), true);
        compare(main._isRepeatableAction("right"), true);
        compare(main._isRepeatableAction("page_prev"), true);
        compare(main._isRepeatableAction("page_next"), true);
    }

    function test_is_repeatable_action_rejects_other_actions(): void {
        compare(main._isRepeatableAction("accept"), false);
        compare(main._isRepeatableAction("cancel"), false);
        compare(main._isRepeatableAction("context_menu"), false);
        compare(main._isRepeatableAction(""), false);
    }

    function test_arm_repeat_records_held_and_starts_initial(): void {
        main._armRepeat("down", Qt.Key_Down);
        compare(main._heldAction, "down");
        compare(main._heldKey, Qt.Key_Down);
        compare(main._repeatPending, true, "Initial-delay timer must be running after _armRepeat");
        compare(main._repeatTicking, false, "Steady tick must not start before the initial delay");
    }

    function test_arm_repeat_with_non_repeatable_action_is_noop(): void {
        main._armRepeat("accept", Qt.Key_Return);
        compare(main._heldAction, "");
        compare(main._heldKey, 0);
        compare(main._repeatPending, false);
        compare(main._repeatTicking, false);
    }

    function test_stop_repeat_clears_state(): void {
        main._armRepeat("down", Qt.Key_Down);
        main._stopRepeat();
        compare(main._heldAction, "");
        compare(main._heldKey, 0);
        compare(main._repeatPending, false);
        compare(main._repeatTicking, false);
    }

    function test_release_of_held_key_clears_state(): void {
        main._armRepeat("down", Qt.Key_Down);
        main.handleKeyRelease(Qt.Key_Down);
        compare(main._heldAction, "");
        compare(main._heldKey, 0);
        compare(main._repeatPending, false);
    }

    // A release of a key that didn't start the repeat (a chord, a
    // stray press mid-hold) must not cancel the active repeat. Only
    // the originating key's release stops it.
    function test_release_of_unrelated_key_keeps_state(): void {
        main._armRepeat("down", Qt.Key_Down);
        main.handleKeyRelease(Qt.Key_Right);
        compare(main._heldAction, "down", "Release of an unrelated key must leave the held repeat alone");
        compare(main._heldKey, Qt.Key_Down);
        compare(main._repeatPending, true);
    }

    // Re-arming with a different direction replaces the held key —
    // a fresh dpad press is intent to change direction, not a chord.
    function test_arm_repeat_replaces_held_action(): void {
        main._armRepeat("down", Qt.Key_Down);
        main._armRepeat("right", Qt.Key_Right);
        compare(main._heldAction, "right");
        compare(main._heldKey, Qt.Key_Right);
        compare(main._repeatPending, true, "Re-arm restarts the initial-delay timer");
    }

    function test_rapid_navigation_taps_require_sustained_same_direction(): void {
        for (let i = 1; i < main._rapidNavigationTapThreshold; ++i) {
            main._noteRapidNavigationAction("down", false);
            compare(main.rapidNavigationActive, false, "ordinary repeated taps stay out of rapid mode");
        }
        main._noteRapidNavigationAction("down", false);
        compare(main.rapidNavigationActive, true, "fourth same-direction tap inside quiet window enters rapid mode");
        compare(main.rapidNavigationIndicatorActive, true);
        wait(main._rapidNavigationQuietMs + 40);
        compare(main.rapidNavigationActive, false, "rapid mode clears after quiet window");
        compare(main.rapidNavigationAction, "", "quiet reset clears rapid action");
    }

    function test_rapid_navigation_alternating_taps_never_activate(): void {
        const actions = ["up", "down", "up", "down", "up"];
        for (let i = 0; i < actions.length; ++i) {
            main._noteRapidNavigationAction(actions[i], false);
            compare(main.rapidNavigationActive, false, "direction changes must reset rapid-mode tap evidence");
            compare(main.rapidNavigationIndicatorActive, false);
        }
    }

    function test_rapid_navigation_ignores_non_rapid_action(): void {
        main._noteRapidNavigationAction("accept", true);
        compare(main.rapidNavigationActive, false);
        compare(main.rapidNavigationAction, "");
    }

    function test_single_page_tap_does_not_show_rapid_indicator(): void {
        main._noteRapidNavigationAction("page_next", false);
        compare(main.rapidNavigationAction, "page_next");
        compare(main.rapidNavigationIndicatorActive, false, "single page tap should not flash rapid indicator");
    }

    function test_repeat_tick_forces_rapid_navigation_active(): void {
        main._armRepeat("page_next", Qt.Key_R);
        main._handleRepeatAction();
        compare(main.rapidNavigationActive, true, "held page action should enter rapid mode on first repeat tick");
        compare(main.rapidNavigationIndicatorActive, true, "held page action should show rapid indicator on first repeat tick");
        main._stopRepeat();
        wait(main._rapidNavigationQuietMs + 40);
        compare(main.rapidNavigationActive, false);
    }

    // Duplicate-input guard. The Keys.onPressed handler collapses a
    // second delivery of the same key while the guard window is open
    // (controller / input-stack double send). The decision is a pure
    // helper so we can assert it without driving real key events or a
    // clock — same key inside the window is a duplicate, a different
    // key or the same key after the window are not.
    function test_duplicate_input_drops_same_key_in_window(): void {
        compare(main._isDuplicateInput(Qt.Key_Down, Qt.Key_Down, true), true, "same key inside the window is a duplicate");
    }

    function test_duplicate_input_passes_same_key_after_window(): void {
        compare(main._isDuplicateInput(Qt.Key_Down, Qt.Key_Down, false), false, "same key after the window closes is a fresh press");
    }

    function test_duplicate_input_passes_different_key_in_window(): void {
        compare(main._isDuplicateInput(Qt.Key_Up, Qt.Key_Down, true), false, "a different key inside the window is never a duplicate");
    }

    // Context-menu builder. Drives the pure helper directly per the QML
    // test isolation rule — no real menu opening, no handleAction.
    // Compares only the entry id sequence; labels are qsTr() and asserted
    // separately so the tests stay translation-friendly.
    function _idsOf(entries: var): var {
        const out = [];
        for (let i = 0; i < entries.length; ++i)
            out.push(entries[i].id);
        return out;
    }

    function test_context_menu_systems_owner_includes_media_actions(): void {
        const entries = main.buildContextMenuEntries("systems", "", false, false, false, "", false);
        compare(_idsOf(entries), ["launch_system", "launch_random_system", "index_system", "scrape_system", "toggle_hide_system"], "Systems context menu includes random and maintenance actions");
        verify(entries[0].label.length > 0, "Launch core label is set (not asserted in English for translation)");
        verify(entries[1].label.length > 0, "Random game label is set");
        verify(entries[2].label.length > 0, "Update media database label is set");
        verify(entries[3].label.length > 0, "Scrape metadata label is set");
        verify(entries[4].label.length > 0, "Hide label is set");
    }

    function test_context_menu_systems_has_nfc_does_not_add_entries(): void {
        const entries = main.buildContextMenuEntries("systems", "", false, true, false, "", false);
        compare(_idsOf(entries), ["launch_system", "launch_random_system", "index_system", "scrape_system", "toggle_hide_system"], "has_nfc must not affect the systems menu");
    }

    // Category index/scrape are gated on the category having at least one
    // indexable (non-launch-only) system. The test Core is empty, so
    // SystemsModel.system_ids_for_category returns nothing and the gate must
    // omit the dead actions, leaving only Hide/Unhide. The positive branch
    // (a mixed or fully-launchable category) is covered at the data layer by
    // the Rust `indexable_system_ids` tests, which the empty test model can't
    // exercise here.
    function test_context_menu_categories_empty_category_omits_index_scrape(): void {
        // Empty category short-circuits the gate (category !== "").
        const entries = main.buildContextMenuEntries("categories", "", false, false, false, "", false, "");
        compare(_idsOf(entries), ["toggle_hide_category"], "Empty category has no indexable systems, so index/scrape are omitted");
    }

    function test_context_menu_categories_no_indexable_systems_omits_index_scrape(): void {
        // Non-empty category whose model yields no indexable systems.
        const entries = main.buildContextMenuEntries("categories", "", false, false, false, "", false, "Other");
        compare(_idsOf(entries), ["toggle_hide_category"], "A category with no indexable systems must not show index/scrape");
    }

    function test_context_menu_categories_hidden_label_toggles(): void {
        const hideEntries = main.buildContextMenuEntries("categories", "", false, false, false, "", false, "Other");
        const unhideEntries = main.buildContextMenuEntries("categories", "", false, false, false, "", true, "Other");
        compare(hideEntries[0].id, "toggle_hide_category");
        compare(unhideEntries[0].id, "toggle_hide_category");
        verify(hideEntries[0].label !== unhideEntries[0].label, "Hide/Unhide label flips on isHidden");
    }

    function test_context_menu_games_directory_returns_empty(): void {
        compare(main.buildContextMenuEntries("games", "directory", false, true, false, ""), [], "Folder tiles have no context menu, even with reader attached");
    }

    function test_context_menu_games_root_returns_empty(): void {
        compare(main.buildContextMenuEntries("games", "root", false, true, false, ""), []);
    }

    function test_context_menu_games_no_reader_omits_write_card(): void {
        const entries = main.buildContextMenuEntries("games", "media", true, false, false, "");
        compare(_idsOf(entries), ["toggle_favorite", "qr_code", "launch_game"], "Write to NFC token must be hidden when no reader is reported");
    }

    function test_context_menu_games_with_reader_includes_write_card(): void {
        const entries = main.buildContextMenuEntries("games", "media", true, true, false, "");
        compare(_idsOf(entries), ["toggle_favorite", "write_card", "qr_code", "launch_game"]);
    }

    function test_context_menu_favorites_matches_games_media_entries(): void {
        const entries = main.buildContextMenuEntries("favorites", "", true, true, true, "", false, "");
        compare(_idsOf(entries), ["toggle_favorite", "write_card", "qr_code", "launch_game"]);
    }

    function test_context_menu_favorites_no_reader_omits_write_card(): void {
        const entries = main.buildContextMenuEntries("favorites", "", true, false, true, "", false, "");
        compare(_idsOf(entries), ["toggle_favorite", "qr_code", "launch_game"]);
    }

    function test_context_menu_favorite_systems_offers_scoped_random(): void {
        const entries = main.buildContextMenuEntries("favorite_systems", "", false, false, false, "SNES", false, "");
        compare(_idsOf(entries), ["launch_random_favorite_system"]);
    }

    function test_context_menu_hub_favorites_offers_random_only(): void {
        const entries = main.buildContextMenuEntries("hub_favorites", "", false, false, false, "");
        compare(_idsOf(entries), ["launch_random_favorite"]);
        verify(entries[0].label.length > 0);
    }

    function test_context_menu_recents_omits_more_info(): void {
        const entries = main.buildContextMenuEntries("recents", "", false, false, false, "", false, "");
        compare(_idsOf(entries), ["launch_game"]);
    }

    function test_context_menu_games_favorite_label_toggles(): void {
        const addEntries = main.buildContextMenuEntries("games", "media", true, false, false, "");
        const removeEntries = main.buildContextMenuEntries("games", "media", true, false, true, "");
        compare(addEntries[0].id, "toggle_favorite");
        compare(removeEntries[0].id, "toggle_favorite");
        verify(addEntries[0].label.length > 0);
        verify(removeEntries[0].label.length > 0);
        verify(addEntries[0].label !== removeEntries[0].label);
    }

    function test_context_menu_unknown_owner_returns_empty(): void {
        compare(main.buildContextMenuEntries("nope", "", false, true, false, ""), [], "Unknown owners get no entries — safe default");
    }

    // QR-code payload wrapper. The web app at zaparoo.app/write reads the
    // zapscript out of the `v=` query param, so the helper must
    // URL-encode reserved characters.
    function test_qr_payload_empty_zapscript(): void {
        compare(main._buildQrPayload(""), "https://zaparoo.app/write?v=");
    }

    function test_qr_payload_plain_ascii(): void {
        compare(main._buildQrPayload("foo"), "https://zaparoo.app/write?v=foo");
    }

    function test_qr_payload_encodes_reserved_chars(): void {
        // encodeURIComponent leaves `* - _ . ! ~ ' ( )` unescaped — only
        // characters that would terminate or restructure the URL get
        // percent-encoded. Real zapscripts look like
        // `**launch.system:foo`; only the `:` needs escaping (it would
        // otherwise be read as a port separator in some parsers).
        const payload = main._buildQrPayload("**launch.system:Atari2600");
        compare(payload, "https://zaparoo.app/write?v=**launch.system%3AAtari2600");
    }

    function test_qr_payload_encodes_url_breakers(): void {
        // Belt-and-braces check that characters that *would* break the URL
        // (space, `&`, `?`) are escaped as expected. None of these appear
        // in current zapscripts but a future zapscript with arguments
        // containing them must still survive a round-trip.
        compare(main._buildQrPayload("a b&c?d"), "https://zaparoo.app/write?v=a%20b%26c%3Fd");
    }

    function test_games_page_menu_offers_core_backed_actions(): void {
        // Nonzero scope exposes recursive Core random action.
        Browse.GamesModel.total_files = 5;
        main.openPageMenu();
        tryCompare(main, "listPickerModalVisible", true);
        const ids = main.listPickerEntries.map(e => e.id);
        verify(ids.indexOf("jump_letter") !== -1, "Go to entry present");
        verify(ids.indexOf("launch_random") !== -1, "Random entry present");
        verify(ids.indexOf("games_filter") !== -1, "Show entry present");
        main.closeListPickerModal();
    }

    function test_page_menu_action_toggles_only_view_pickers(): void {
        const pickers = [
            {
                open: () => main.openPageMenu(),
                fieldId: "page_menu"
            },
            {
                open: () => main.openFavoritesPageMenu(),
                fieldId: "page_menu_favorites"
            },
            {
                open: () => main.openFavoriteSystemsPageMenu(),
                fieldId: "page_menu_favorite_systems"
            }
        ];
        for (let i = 0; i < pickers.length; i++) {
            pickers[i].open();
            tryCompare(main, "listPickerModalVisible", true);
            compare(main.listPickerFieldId, pickers[i].fieldId);
            main.handleAction("page_menu");
            tryCompare(main, "listPickerModalVisible", false);
        }

        main.openListPickerModal("Orientation", [{
            id: "horizontal",
            label: "Horizontal"
        }], "horizontal", "orientation");
        tryCompare(main, "listPickerModalVisible", true);
        main.handleAction("page_menu");
        compare(main.listPickerModalVisible, true, "View must not close an unrelated list picker");
        compare(main.listPickerFieldId, "orientation");
        main.handleAction("cancel");
        tryCompare(main, "listPickerModalVisible", false);
    }

    function test_view_toggle_restores_input_to_underlying_screen(): void {
        main.activeScreen = main.screenGames;
        main.openPageMenu();
        tryCompare(main, "listPickerModalVisible", true);
        main.handleAction("page_menu");
        tryCompare(main, "listPickerModalVisible", false);

        main.handleKey(Qt.Key_Escape);
        compare(main.pendingTransition, "");
        compare(main.activeScreen, main.screenSystems);
        main.activeScreen = main.screenHub;
        Browse.AppState.active_screen = "";
    }

    function test_random_launch_failure_is_reported(): void {
        // Harness has no browse scope, so model rejects before issuing RPC.
        Browse.GamesModel.launch_random();
        tryCompare(main, "randomFailedModalVisible", true);
        verify((Browse.GamesModel.random_error ?? "") !== "", "failure reason recorded");
        main.handleAction("cancel");
        tryCompare(main, "randomFailedModalVisible", false);
        compare(Browse.GamesModel.random_error, "", "dismissal clears reason");
    }

    function test_games_filter_selection_applies_and_persists(): void {
        Browse.GamesModel.apply_favorites_filter(false);
        Browse.GamesState.favorites_filter = false;

        main.openPageMenu();
        main.listPickerAccepted("page_menu", "games_filter");
        tryCompare(main, "listPickerModalVisible", true);
        compare(main.listPickerFieldId, "games_filter_pick");
        const ids = main.listPickerEntries.map(e => e.id);
        verify(ids.indexOf("all") !== -1, "All option present");
        verify(ids.indexOf("favorites") !== -1, "Favorites option present");

        main.listPickerAccepted("games_filter_pick", "favorites");
        tryCompare(main, "listPickerModalVisible", false);
        compare(Browse.GamesModel.favorites_only, true);
        compare(Browse.GamesState.favorites_filter, true);
        compare(main.gamesScreen.pageMenuEnabledWhenEmpty, true, "View remains reachable when filtered folder is empty");
    }

    // Favorites View preserves PR #348 sorting/random and adds entry mode.
    function test_favorites_page_menu_offers_sort_random_and_mode(): void {
        main.openFavoritesPageMenu();
        tryCompare(main, "listPickerModalVisible", true);
        const ids = main.listPickerEntries.map(e => e.id);
        compare(ids, ["favorites_sort", "favorites_mode", "launch_random_favorite"], "View order is Sort, Group by, Random");
        main.closeListPickerModal();
    }

    function test_favorite_systems_page_menu_offers_mode_only(): void {
        main.openFavoriteSystemsPageMenu();
        tryCompare(main, "listPickerModalVisible", true);
        compare(main.listPickerEntries.map(e => e.id), ["favorites_mode"]);
        main.closeListPickerModal();
    }

    function test_favorites_mode_picker_switches_both_directions(): void {
        Browse.Settings.set_favorites_grouping("none");
        main.openFavoritesPageMenu();
        main.listPickerAccepted("page_menu_favorites", "favorites_mode");
        compare(main.listPickerFieldId, "favorites_mode_pick");
        compare(main.listPickerEntries.map(e => e.id), ["none", "system"]);

        main.listPickerAccepted("favorites_mode_pick", "system");
        compare(Browse.Settings.current_favorites_grouping, "system");
        compare(main.pendingTransition, "favorite_systems");

        main.pendingTransition = "";
        main.openFavoriteSystemsPageMenu();
        main.listPickerAccepted("page_menu_favorite_systems", "favorites_mode");
        main.listPickerAccepted("favorites_mode_pick", "none");
        compare(Browse.Settings.current_favorites_grouping, "none");
        compare(main.pendingTransition, "favorites");
        compare(main.favoritesSystemId, "");
    }

    // Choosing Sort must open a second picker on its own field id, and the
    // accepted value must reach the model. A wrong field id would silently
    // route the choice nowhere.
    function test_favorites_sort_selection_applies_to_model(): void {
        main.openFavoritesPageMenu();
        main.listPickerAccepted("page_menu_favorites", "favorites_sort");
        tryCompare(main, "listPickerModalVisible", true);
        compare(main.listPickerFieldId, "favorites_sort_pick");
        const ids = main.listPickerEntries.map(e => e.id);
        verify(ids.indexOf("name") !== -1, "A-Z option present");
        // Default carries a real id, not "" — ListPickerModal never emits an
        // accept for an empty id.
        verify(ids.indexOf(main._favoritesSortDefault) !== -1, "Default option present");

        main.listPickerAccepted("favorites_sort_pick", "name");
        tryCompare(main, "listPickerModalVisible", false);
        compare(Browse.FavoritesModel.sort_mode, "name");

        // Restore the default so the persisted value doesn't leak into other
        // tests or the dev machine's config file.
        main.openFavoritesPageMenu();
        main.listPickerAccepted("page_menu_favorites", "favorites_sort");
        main.listPickerAccepted("favorites_sort_pick", main._favoritesSortDefault);
        compare(Browse.FavoritesModel.sort_mode, "");
    }

    // Blanket guard for the whole class of bug that shipped twice: any menu
    // row carrying an empty id is silently swallowed by ListPickerModal, so
    // no picker this app builds may contain one.
    function test_no_picker_row_uses_the_swallowed_empty_id(): void {
        const pickers = [
            {
                open: () => main.openPageMenu(),
                name: "games View"
            },
            {
                open: () => main.openFavoritesPageMenu(),
                name: "favorites View"
            },
            {
                open: () => main.openFavoritesSortMenu(),
                name: "favorites Sort"
            }
        ];
        for (let i = 0; i < pickers.length; i++) {
            main.closeListPickerModal();
            pickers[i].open();
            tryCompare(main, "listPickerModalVisible", true);
            const ids = main.listPickerEntries.map(e => e.id);
            for (let j = 0; j < ids.length; j++)
                verify(ids[j] !== "", pickers[i].name + " row " + j + " has an empty id, which ListPickerModal never emits");
            main.closeListPickerModal();
        }
    }

    // Regression: the Default row had an empty id, so once A-Z was chosen
    // there was no way back to Core's order from this menu.
    function test_favorites_default_sort_restores_core_order(): void {
        Browse.FavoritesModel.set_sort_mode("name");
        compare(Browse.FavoritesModel.sort_mode, "name");

        main.openFavoritesSortMenu();
        tryCompare(main, "listPickerModalVisible", true);
        // Default is the first row; drive the real accept path.
        main.listPickerModal.currentIndex = 0;
        main.listPickerModal.handleAction("accept");
        compare(Browse.FavoritesModel.sort_mode, "", "Default restores Core order");
    }
}
