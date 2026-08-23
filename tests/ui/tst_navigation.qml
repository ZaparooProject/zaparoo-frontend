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
import Zaparoo.Screens
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
    property string _originalHubSelectedItem: ""
    property int _actionErrorCallbackCount: 0
    property int _actionErrorDeliveredCount: 0

    Main {
        id: main
        fullScreen: false
        width: 1280
        height: 720
    }

    SignalSpy {
        id: actionErrorBatchSpy

        target: Browse.ActionError
        signalName: "sequenceChanged"
    }

    Connections {
        target: Browse.ActionError
        function onSequenceChanged(): void {
            testCase._actionErrorDeliveredCount += Browse.ActionError.event_sequences.length;
        }
    }

    Component.onCompleted: {
        testCase._originalFavoritesSort = Browse.FavoritesModel.sort_mode ?? "";
        testCase._originalFavoritesGrouping = Browse.Settings.current_favorites_grouping ?? "none";
        testCase._originalGamesFavoritesFilter = Browse.GamesState.favorites_filter === true;
        testCase._originalHubCategory = Browse.HubState.category ?? "";
        testCase._originalHubSelectedRow = Browse.HubState.selected_row ?? 0;
        testCase._originalHubSelectedAction = Browse.HubState.selected_action ?? "";
        testCase._originalHubSelectedItem = Browse.HubState.selected_item ?? "";
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
        main.gamesCoverRevealReady = true;
        main.gamesNavigationInputAt = 0;
        main.gamesNavigationModelReadyAt = 0;
        main.gamesNavigationAction = "";
        main.gamesScreen.lastNavigationInputAt = 0;
        // A Hub `system`/`folder` shortcut test sets this and doesn't
        // press Back to clear it, so a later test asserting the ordinary
        // Games -> Systems back-route would otherwise inherit a leaked
        // `true` and route to Hub instead.
        Browse.GamesState.entered_from_hub = false;
        main._firstRunIndexStarted = false;
        tryCompare(main, "transitionCueVisible", false);
        // Hub focus is one flat grid now (categories then actions, no gap);
        // reset it so a prior test's cursor jump doesn't leak into the next.
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
        testCase._actionErrorCallbackCount = 0;
        testCase._actionErrorDeliveredCount = 0;
    }

    function cleanup(): void {
        Motion.enabled = true;
        // A modal left open swallows every routed action, so the next test
        // would fail for a reason that has nothing to do with what it tests.
        main._actionErrorQueue = [];
        if (main.actionErrorModalVisible)
            main.closeActionErrorModal(false);
        if (main.listPickerModalVisible)
            main.closeListPickerModal();
        compare(ScreenManager.modalCount, 0, "test leaked modal stack ownership");
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
        Browse.HubState.selected_item = testCase._originalHubSelectedItem;
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
        const entry = main.hubScreen.items[main.hubScreen.currentIndex];
        verify(entry !== undefined, "Cold optimistic Hub must land on a valid selection");
        compare(entry.kind, "action");
        compare(entry.id, "resume", "Cold optimistic Hub should start on Resume");
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

    // Round 9: the snapshot Item now covers the FULL grid rect (matching
    // `prepareRapidSnapshot()`'s own full-rect `grabToImage` call) instead
    // of the smaller content rect it used to crop down to via
    // `sourceClipRect` -- a `QQuickPixmap` region hint that is not
    // reliably honored for `image://itemgrabber/…` provider URLs, and
    // whose absence let `fillMode: Image.Stretch` visibly downscale the
    // whole grab into the smaller box. `fillMode: Image.Pad` now paints
    // the grabbed pixmap at 1:1 with no scaling path at all, so there is
    // no `sourceClipRect` left to assert on.
    function test_games_rapid_scroll_snapshot_covers_full_grid_at_1to1(): void {
        const snapshot = findChild(main.gamesScreen, "rapidScrollSnapshot");
        const snapshotImage = findChild(main.gamesScreen, "rapidScrollSnapshotImage");
        verify(snapshot !== null);
        verify(snapshotImage !== null);
        compare(snapshot.x, main.gamesScreen.gamesGrid.x);
        compare(snapshot.y, main.gamesScreen.gamesGrid.y);
        compare(snapshot.width, main.gamesScreen.gamesGrid.width);
        compare(snapshot.height, main.gamesScreen.gamesGrid.height);
        compare(snapshotImage.fillMode, Image.Pad);
        compare(snapshotImage.smooth, false);
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

    // The Systems grid used to withhold covers for one extra frame after the
    // transition landed: `_completeTransition(screenSystems)` cleared a router
    // flag and only `onFramePresented` set it again. That existed because an
    // async tint could outlast the transition, so revealing early meant a grid
    // of empty tiles. Tints come synchronously out of the baked atlas now, so
    // the flag is gone and the destination grid must ask for its logos in the
    // same binding pass that completes the transition -- the first frame the
    // user sees is the frame with artwork in it.
    function test_system_covers_are_requested_as_soon_as_the_transition_completes(): void {
        const originalLoading = Browse.SystemsModel.loading;
        const originalError = Browse.SystemsModel.error_message;
        const originalCatalogLoaded = Browse.CategoriesModel.loaded;
        // A booting catalog puts the screen in `optimisticLoading`, which hides
        // the whole grid. Settle it so the only thing left that could hold
        // covers back is the router.
        Browse.CategoriesModel.loaded = true;
        Browse.SystemsModel.loading = false;
        Browse.SystemsModel.error_message = "";

        main.activeScreen = main.screenHub;
        main.pendingTransition = "systems";
        compare(main.systemsScreen.systemsGrid.coverRequestsEnabled, false, "an in-flight transition gates covers");

        main._completeTransition(main.screenSystems);
        compare(main.pendingTransition, "");
        // No presented frame in between -- this is the assertion the deleted
        // reveal flag would have failed.
        compare(main.systemsScreen._gateHide, false);
        compare(main.systemsScreen.systemsGrid.coverRequestsEnabled, true);

        Browse.SystemsModel.error_message = originalError;
        Browse.SystemsModel.loading = originalLoading;
        Browse.CategoriesModel.loaded = originalCatalogLoaded;
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
        // Bootstrap order seeds Resume first (index 0), then the
        // placeholder categories — see HubScreen.qml's `items`.
        main.hubScreen.currentIndex = main.hubScreen._itemIndexForId("category", "Arcade");
        main.handleKey(Qt.Key_Return);
        compare(main.pendingTransition, "systems");
        compare(Browse.HubState.category, "Arcade");
        compare(main.activeScreen, main.screenHub, "Optimistic route stays under the loading cue until catalog readiness is authoritative");
    }

    // A Hub `system` shortcut's Accept path (Main.qml's onRequestAccept ->
    // _navigateFromSystems) -- was a deliberate no-op before this round;
    // now it must establish both state singletons and start the Games
    // transition, same as accepting the system from the Systems screen
    // itself would. Only the SYNCHRONOUS part of the transition is
    // observable here: _ensureSystem's deferred completion needs Core to
    // resolve GamesModel.loading, which this harness (no live Core) never
    // does — the same reason no existing test drives a Systems->Games
    // transition to completion either.
    //
    // `add_target_item` mutates the real (test-isolated) HubLayout
    // singleton, which flips `is_unseeded()` false for the rest of the
    // process the moment `items` goes non-empty — every other test relies
    // on the bootstrap-placeholder branch staying active, so the
    // try/finally here is load-bearing, not defensive style: a skipped
    // cleanup (e.g. a failed assertion aborting the function) corrupts
    // every later test's Hub content, not just this one.
    function test_hub_system_shortcut_accept_navigates_to_games(): void {
        verify(Browse.HubLayout.add_target_item("system", "NES", "", "", "", "", ""));
        const idx = main.hubScreen._itemIndexForId("system", "NES");
        const hubIndex = idx >= 0 ? main.hubScreen.items[idx].hubIndex : -1;
        try {
            verify(idx >= 0, "the shortcut must resolve into the Hub's items list");
            main.hubScreen.currentIndex = idx;
            main.handleKey(Qt.Key_Return);
            compare(Browse.SystemsState.system_id, "NES");
            compare(Browse.GamesState.system_id, "NES");
            compare(main.pendingTransition, "games");
            compare(Browse.GamesState.entered_from_hub, true, "a Hub shortcut must persist the back-to-Hub breadcrumb");
        } finally {
            if (hubIndex >= 0)
                Browse.HubLayout.remove_item(hubIndex);
        }
    }

    // A Hub `folder` shortcut's Accept path (_navigateFromHubFolder) --
    // must establish the shortcut's owning system BEFORE pushing the
    // folder's own path, since GamesState.push_level appends onto
    // whatever level system_id already established. The push itself runs
    // inside _ensureSystem's deferred callback (see the previous test's
    // comment for why this harness can't observe it completing), so only
    // the synchronous state-establishment half is asserted here.
    function test_hub_folder_shortcut_accept_navigates_to_games(): void {
        const path = "/media/fat/games/SNES/Homebrew";
        verify(Browse.HubLayout.add_target_item("folder", "", path, "", "", "", "SNES"));
        const idx = main.hubScreen._itemIndexForId("folder", path);
        const hubIndex = idx >= 0 ? main.hubScreen.items[idx].hubIndex : -1;
        try {
            verify(idx >= 0, "the shortcut must resolve into the Hub's items list");
            compare(main.hubScreen.items[idx].system, "SNES", "the resolver must carry the system hint through to Accept");
            main.hubScreen.currentIndex = idx;
            main.handleKey(Qt.Key_Return);
            compare(Browse.SystemsState.system_id, "SNES");
            compare(Browse.GamesState.system_id, "SNES");
            compare(main.pendingTransition, "games");
            compare(Browse.GamesState.entered_from_hub, true, "a Hub shortcut must persist the back-to-Hub breadcrumb");
        } finally {
            if (hubIndex >= 0)
                Browse.HubLayout.remove_item(hubIndex);
        }
    }

    // A folder shortcut with no stored system (a malformed/pre-feature
    // layout entry) must not attempt to load an empty system id.
    function test_hub_folder_shortcut_with_no_system_does_not_navigate(): void {
        const path = "/media/fat/games/SNES/Homebrew";
        verify(Browse.HubLayout.add_target_item("folder", "", path, "", "", "", ""));
        const idx = main.hubScreen._itemIndexForId("folder", path);
        const hubIndex = idx >= 0 ? main.hubScreen.items[idx].hubIndex : -1;
        try {
            verify(idx >= 0);
            main.hubScreen.currentIndex = idx;
            main.handleKey(Qt.Key_Return);
            compare(main.pendingTransition, "", "no system to establish means no navigation");
            compare(main.activeScreen, main.screenHub);
        } finally {
            if (hubIndex >= 0)
                Browse.HubLayout.remove_item(hubIndex);
        }
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
        // This test exercises action routing, not optimistic catalog startup.
        // Make catalog readiness authoritative so Settings does not correctly
        // wait behind the startup transition cue on slower test runs.
        const originalCatalogLoaded = Browse.CategoriesModel.loaded;
        Browse.CategoriesModel.loaded = true;
        // Focus Update only when the build and current network state expose
        // it; otherwise Settings is the production empty-catalog fallback.
        const expectedAction = main.hubScreen._emptyCatalogFallbackAction;
        main.hubScreen.currentIndex = main.hubScreen._actionIndexForId(expectedAction);
        compare(main.hubScreen.items[main.hubScreen.currentIndex].id, expectedAction);
        main.handleKey(Qt.Key_Return);
        compare(main.activeScreen, expectedAction === "update" ? main.screenUpdate : main.screenSettings);
        Browse.CategoriesModel.loaded = originalCatalogLoaded;
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
        // Grid layout's actual page cue is the top strip's PageIndicator
        // (`pageIndicatorMode`), not the plain-text `pageText` above --
        // that only backs list layout. Confirm the cue really is up here
        // on the default (non-CRT) theme this harness runs under.
        verify(main.favoritesScreen.topStrip.pageIndicatorMode);
        compare(main.favoritesScreen.topStrip.pageTotalKnown, false);
    }

    function test_recents_uses_unbounded_page_chrome(): void {
        compare(main.recentsScreen.paginationTotalKnown, false);
        compare(main.recentsScreen.recentsGrid.paginationTotalKnown, false);
        verify(main.recentsScreen.topStrip.pageText.indexOf("/") < 0);
        verify(main.recentsScreen.topStrip.pageIndicatorMode);
        compare(main.recentsScreen.topStrip.pageTotalKnown, false);
    }

    function test_hub_favorites_action_uses_favorite_systems_mode(): void {
        Browse.Settings.set_favorites_grouping("system");
        main.hubScreen.currentIndex = main.hubScreen._actionIndexForId("favorites");
        main.handleKey(Qt.Key_Return);
        compare(main.pendingTransition, "favorite_systems");
    }

    function test_hub_favorites_action_uses_all_favorites_mode(): void {
        Browse.Settings.set_favorites_grouping("none");
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

    // A Games screen entered via a Hub `system`/`folder` shortcut (see
    // test_hub_system_shortcut_accept_navigates_to_games above) sets
    // GamesState.entered_from_hub, which onRequestSystemsScreen must
    // honour: Back returns to Hub, not Systems — a screen the user never
    // visited on this path — and the breadcrumb clears once consumed, so
    // a later unrelated system entry (see the previous test) starts
    // clean.
    function test_escape_on_hub_entered_games_returns_to_hub(): void {
        main.activeScreen = main.screenGames;
        Browse.GamesState.entered_from_hub = true;
        main.handleKey(Qt.Key_Escape);
        compare(main.pendingTransition, "");
        compare(main.activeScreen, main.screenHub);
        compare(Browse.GamesState.entered_from_hub, false, "the breadcrumb must clear once consumed by a Back navigation");
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
        compare(main.settingsScreen.currentPage, main.settingsScreen.pageLibraryData);
        main.handleAction("cancel");
        compare(main.settingsScreen.currentPage, main.settingsScreen.pageRoot);
        main.handleAction("cancel");
        compare(main.activeScreen, main.screenHub);
    }

    function test_settings_resolution_labels_explain_automatic_and_motion(): void {
        compare(main.settingsScreen._resolutionDisplay(""), "Automatic");
        compare(main.settingsScreen._resolutionDisplay("1280x720"), "1280 × 720");
        compare(main.settingsScreen._resolutionPickerDisplay(""), "Automatic (Recommended)");
        compare(main.settingsScreen._resolutionPickerDisplay("1920x1080"), "1920 × 1080 (Animations off)");
    }

    function test_settings_resolution_can_stage_automatic_restart(): void {
        main.stageSettingRestart("resolution", "");
        compare(main._resolutionRestartPending, true);
        compare(main._pendingResolutionSelection, "");
        compare(main.settingNeedsRestartModalVisible, true);
        main.cancelPendingRestart();
        compare(main._resolutionRestartPending, false);
        compare(main.settingNeedsRestartModalVisible, false);
    }

    // Tail-of-list padding (round 6 follow-up) replaces round <=5's
    // hand-rolled "visually nearest cell" cross-row mapping (_mapCrossRow)
    // AND round 6's original per-block row-padding (_padToColumns, which
    // padded categories to a row boundary so actions always started
    // fresh) — see HubScreen.qml's header comment for why grouping-padding
    // was dropped. The flat grid's Up/Down is column-preserving regardless
    // (see PagedGrid.moveSelection). _padToPageSize is the pure half of
    // what fills out the *last page*: given an arbitrary list and a page
    // size, it pads with isEmpty:true entries up to the next full page, so
    // a page's trailing remainder renders as deliberate empty slots (see
    // EmptySlot.qml) instead of a wasted mid-page gap or blank background.
    function test_pad_to_page_size_fills_partial_page_with_empty_entries(): void {
        const pad = main.hubScreen._padToPageSize;
        const list = [
            {
                kind: "category",
                id: "a"
            },
            {
                kind: "category",
                id: "b"
            },
            {
                kind: "category",
                id: "c"
            }
        ];
        const padded = pad(list, 5, 0);
        compare(padded.length, 5, "3 real entries pad to a full page of 5");
        compare(padded[3].kind, "empty");
        compare(padded[3].id, "");
        compare(padded[3].isEmpty, true);
        compare(padded[4].kind, "empty");
        compare(padded[4].isEmpty, true);
    }

    function test_pad_to_page_size_is_noop_on_exact_multiple(): void {
        const pad = main.hubScreen._padToPageSize;
        const list = [
            {
                kind: "category",
                id: "a"
            },
            {
                kind: "category",
                id: "b"
            }
        ];
        const padded = pad(list, 2, 0);
        compare(padded, list, "an already-full page must not be padded");
    }

    // Round 2: an empty list now pads up to one full page instead of
    // staying empty -- a seeded layout the user has emptied out entirely
    // (every tile removed, trailing blanks trimmed by Rust) is a real,
    // navigable empty Hub, not a broken zero-height grid; the padded page
    // is what lets the cursor land somewhere and reach View -> Add item….
    function test_pad_to_page_size_pads_an_empty_list_to_one_full_page(): void {
        const pad = main.hubScreen._padToPageSize;
        const padded = pad([], 5, 0);
        compare(padded.length, 5);
        compare(padded[0].isEmpty, true);
        compare(padded[4].isEmpty, true);
    }

    // `minPages` is a FLOOR on the total page count, not an increment on
    // top of the natural page count (round 3 fix -- the original increment
    // design kept receding a page further every time real content grew to
    // reach a new Move target, producing unbounded page growth; see
    // HubScreen.qml's `beginMove` doc comment). A floor already satisfied
    // by the natural page count must be a strict no-op.
    function test_pad_to_page_size_min_pages_is_a_floor_not_an_increment(): void {
        const pad = main.hubScreen._padToPageSize;
        const list = [
            {
                kind: "category",
                id: "a"
            },
            {
                kind: "category",
                id: "b"
            }
        ];
        // Already fills exactly 1 page at this size -- a floor of 1 must
        // change nothing.
        compare(pad(list, 2, 1), list, "a floor already satisfied must not pad further");
        // A floor of 2/3 pages pads up to (and only to) that many pages.
        compare(pad(list, 2, 2).length, 4);
        compare(pad(list, 2, 3).length, 6);
        // The floor applies to an empty list too, winning over the natural
        // single-page default.
        compare(pad([], 5, 2).length, 10);
    }

    // Round 3 regression: `_syncGridModel` used to call `hubGridModel.
    // clear()` on any length change, which drops the model to ZERO rows
    // for an instant before repopulating it -- PagedGrid's
    // `onItemCountChanged` reads that as "model shed rows" and clamps
    // `currentIndex` to 0. Since arming Move itself changes `items`'
    // padding target, this fired on every single `beginMove` call, before
    // the first press even landed, resetting the held tracking to
    // whichever tile happened to be at slot 0 regardless of what was
    // actually focused. `beginMove` itself needs a real
    // `Browse.HubLayout` entry (`hubIndex >= 0`) this suite deliberately
    // never seeds -- seeding it here would permanently flip
    // `is_unseeded()` to false for the rest of this test binary's run
    // (there is no un-seed invokable), silently breaking every other
    // bootstrap-branch test depending on file/function execution order.
    // Toggling `moveArmed`/`_moveArmedTotalPages` directly reproduces the
    // exact same length-change path `items` takes during a real session
    // without needing that seed.
    function test_grid_model_sync_survives_a_length_change_without_resetting_current_index(): void {
        const hub = main.hubScreen;
        // Comfortably away from slot 0 -- the bootstrap window always has
        // at least 5 placeholder categories, so this is a real category
        // tile regardless of live Resume/Update visibility state.
        hub.currentIndex = 3;
        hub.moveArmed = true;
        hub._moveArmedTotalPages = 2;
        compare(hub.currentIndex, 3, "arming (a length change) must not reset the cursor to slot 0");
        hub.moveArmed = false;
        hub._moveArmedTotalPages = 0;
        compare(hub.currentIndex, 3, "disarming (a shrink) must not move an in-bounds cursor");
    }

    // Round 3: a held tile must never wrap around the grid's edges (Up at
    // page 0's top row, Down at the last page's bottom row) even though
    // ordinary navigation's closed loop does exactly that on purpose --
    // see HubScreen.qml's `_wouldWrapVertically`/`_wouldWrapPage` doc
    // comments. Pure geometry, so exercised directly without needing a
    // seeded Browse.HubLayout (see the note above).
    function test_would_wrap_vertically_blocks_only_the_true_top_and_bottom_edges(): void {
        const hub = main.hubScreen;
        hub.moveArmed = true;
        hub._moveArmedTotalPages = 2;
        const columns = Sizing.hubGridColumns;
        const rows = Sizing.hubGridRows;
        const pageSize = columns * rows;

        hub.currentIndex = 0;
        compare(hub._wouldWrapVertically(-1), true, "Up at page 0 row 0 must be blocked");
        if (rows > 1)
            compare(hub._wouldWrapVertically(1), false, "Down off row 0 must not be blocked when there's another row below");

        // Page 0's last row is NOT the true bottom edge -- there's a
        // second (reserve) page below it, so Down must cross onto it.
        hub.currentIndex = (rows - 1) * columns;
        compare(hub._wouldWrapVertically(1), false, "Down at the bottom of a non-last page must cross to the next page");

        // Page 1 (the last page)'s last row IS the true bottom edge.
        hub.currentIndex = pageSize + (rows - 1) * columns;
        compare(hub._wouldWrapVertically(1), true, "Down at the last page's bottom row must be blocked");
        compare(hub._wouldWrapVertically(-1), false, "Up off the last row must not be blocked when it isn't page 0's top row");

        hub.moveArmed = false;
        hub._moveArmedTotalPages = 0;
    }

    // Round 4: same guard, horizontal axis -- Left at column 0 (or Right at
    // the last column) must not wrap within the row onto a synthetic
    // padding cell and drag the held tile to the opposite edge. See
    // HubScreen.qml's `_wouldWrapColumn` doc comment.
    function test_would_wrap_column_blocks_only_the_true_left_and_right_edges(): void {
        const hub = main.hubScreen;
        hub.moveArmed = true;
        hub._moveArmedTotalPages = 2;
        const columns = Sizing.hubGridColumns;

        hub.currentIndex = 0;
        compare(hub._wouldWrapColumn(-1), true, "Left at column 0 must be blocked");
        if (columns > 1)
            compare(hub._wouldWrapColumn(1), false, "Right off column 0 must not be blocked when there's another column beyond it");

        hub.currentIndex = columns - 1;
        compare(hub._wouldWrapColumn(1), true, "Right at the last column must be blocked");
        compare(hub._wouldWrapColumn(-1), false, "Left off the last column must not be blocked when it isn't column 0");

        hub.moveArmed = false;
        hub._moveArmedTotalPages = 0;
    }

    function test_would_wrap_page_blocks_only_the_first_and_last_page(): void {
        const hub = main.hubScreen;
        hub.moveArmed = true;
        hub._moveArmedTotalPages = 2;
        const pageSize = Sizing.hubGridColumns * Sizing.hubGridRows;

        hub.currentIndex = 0;
        compare(hub._wouldWrapPage(-1), true, "page_prev on the first page must be blocked");
        compare(hub._wouldWrapPage(1), false, "page_next off the first page must not be blocked");

        hub.currentIndex = pageSize;
        compare(hub._wouldWrapPage(1), true, "page_next on the last page must be blocked");
        compare(hub._wouldWrapPage(-1), false, "page_prev off the last page must not be blocked");

        hub.moveArmed = false;
        hub._moveArmedTotalPages = 0;
    }

    // `armMoveForHubIndex` is the seek-then-arm helper View -> Add item…
    // uses (Main.qml's `hub_add_pick` handler) so a freshly placed tile
    // (which the cursor is never already resting on) can be positioned
    // immediately rather than left wherever add_item's append/target rule
    // happened to put it: find the flat (padded) index for a given
    // hubIndex, seat the cursor there, then hand off to `beginMove`.
    //
    // This harness never calls `Browse.HubLayout.reconcile(...)`, so the
    // Hub stays in its bootstrap-placeholder window for the whole suite
    // (every entry's `hubIndex` is -1 — see `items`' bootstrap branch
    // above) and there is no real hubIndex>=0 entry available here to
    // exercise the successful seek. Only the rejection paths are covered
    // at this level; the successful seek + arm is exercised end to end by
    // `just run-dev` (View -> Add item…, see docs/plans -- the plan this
    // round shipped under).
    function test_arm_move_for_hub_index_ignores_an_unresolvable_index(): void {
        const hub = main.hubScreen;
        hub.currentIndex = 0;
        hub.armMoveForHubIndex(999999);
        compare(hub.currentIndex, 0, "an unresolvable hubIndex must not move the cursor at all");
        compare(hub.moveArmed, false);

        hub.currentIndex = 0;
        hub.armMoveForHubIndex(-1);
        compare(hub.currentIndex, 0, "a negative hubIndex must be rejected the same way beginMove already rejects it");
    }

    // Replaces round <=5's saved-index round-trip tests (_crossSavedIndex,
    // _crossRow) — that machinery existed only because the two rows had
    // different lengths and a centered visual map couldn't always return
    // Up/Down to the originating tile. `_nearestVerticalCandidate`
    // (PagedGrid.qml) doesn't structurally guarantee a round trip the way
    // a strict same-column walk would -- it picks whichever real tile
    // scores best from wherever the cursor actually lands, which need not
    // be the tile that sent it there -- but for this harness's densely
    // packed row-major content the nearest tile below is directly below,
    // so the round trip holds in practice. Kept as a regression pin on
    // that property for the harness's actual layout, not a claim that the
    // algorithm guarantees it in general.
    function test_up_down_round_trip_preserves_column(): void {
        main.hubScreen.currentIndex = 2;
        main.handleKey(Qt.Key_Down);
        const afterDown = main.hubScreen.currentIndex;
        verify(afterDown !== 2, "Down must actually move focus onto the other row");
        main.handleKey(Qt.Key_Up);
        compare(main.hubScreen.currentIndex, 2, "Down then Up must return to the exact same tile");
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

    // _hubCoverKey used to return "" until the `custom/hub/` scan reported in,
    // which meant every Hub tile painted its first frame with no source at all
    // and then swapped once a filesystem walk finished. That is a blank frame
    // charged to every user to spare the few with overrides a one-frame swap.
    // The bundled key is always known up front, so the resolver must never
    // return empty for a non-empty fallback — whatever hub_loaded currently is.
    function test_hub_cover_key_never_withholds_the_bundled_key(): void {
        const resolve = main.hubScreen._hubCoverKey;
        const cases = [[CategoryIds.arcadeId, CategoryIds.coverKey(CategoryIds.arcadeId)], ["favorites", "icons/HeartOutline"], ["settings", "icons/Tools"]];
        for (let i = 0; i < cases.length; ++i) {
            const fallback = cases[i][1];
            verify(fallback !== "", "test bug: the fallback itself must be non-empty");
            compare(resolve(cases[i][0], fallback), fallback, "no override is configured in the test environment, so the bundled key must come straight back");
        }
    }

    // _resolveZapScriptEntry's cover-key priority: an explicit icon
    // override always wins, even over a linked game's own art -- the user
    // asked for that icon specifically.
    function test_resolve_zapscript_entry_prefers_icon_override(): void {
        const entry = main.hubScreen._resolveZapScriptEntry("**launch.random:NES", "", "Dice", "NES", "/media/fat/games/NES/game.nes");
        compare(entry.coverKey, main.hubScreen._hubCoverKey("Dice", "icons/File"));
    }

    // system+path together identify a LINKED GAME (the "add this game to
    // the Hub" case) -- cover art must resolve through the same Core
    // media lookup a Games-grid row uses, not a generic icon. The test
    // path is deliberately never cached, so the real lookup's first-call
    // contract (kick a fetch, answer "icons/Loading" in the meantime) is
    // what proves the REAL lookup fired rather than falling through to
    // the system-logo-only branch.
    function test_resolve_zapscript_entry_looks_up_real_cover_for_a_linked_game(): void {
        const entry = main.hubScreen._resolveZapScriptEntry("/media/fat/games/NES/_test_probe_never_cached.nes", "", "", "NES", "/media/fat/games/NES/_test_probe_never_cached.nes");
        compare(entry.coverKey, "icons/Loading", "an uncached linked-game path must kick a real Core lookup, not fall back to a generic/system icon");
    }

    // `system` alone (no `path`) is a softer hint for a script with no
    // single game to fetch art for (e.g. "launch a random game in this
    // system") -- falls back to that system's logo, no Core lookup.
    function test_resolve_zapscript_entry_falls_back_to_system_logo_without_a_path(): void {
        const entry = main.hubScreen._resolveZapScriptEntry("**launch.random:NES", "", "", "NES", "");
        compare(entry.coverKey, Browse.HubLayout.resolve_system_cover_key("NES"));
    }

    function test_resolve_zapscript_entry_generic_icon_with_no_hints(): void {
        const entry = main.hubScreen._resolveZapScriptEntry("**launch.random:NES", "", "", "", "");
        compare(entry.coverKey, "icons/File");
    }

    function test_resolve_zapscript_entry_name_falls_back_to_script_text(): void {
        const entry = main.hubScreen._resolveZapScriptEntry("**launch.random:NES", "", "", "", "");
        compare(entry.name, "**launch.random:NES");
        const named = main.hubScreen._resolveZapScriptEntry("**launch.random:NES", "Random NES", "", "", "");
        compare(named.name, "Random NES");
    }

    function test_resolve_zapscript_entry_empty_script_is_skipped(): void {
        verify(main.hubScreen._resolveZapScriptEntry("", "", "", "", "") === null);
    }

    // system-kind tiles resolve name/cover through Browse.HubLayout's
    // id-only lookups, independent of any live category row -- see that
    // singleton's resolve_system_name/resolve_system_cover_key.
    function test_resolve_system_entry_uses_overrides_when_set(): void {
        const overridden = main.hubScreen._resolveSystemEntry("NES", "My NES", "Cartridge");
        compare(overridden.name, "My NES");
        compare(overridden.coverKey, main.hubScreen._hubCoverKey("Cartridge", "icons/File"));
        const bare = main.hubScreen._resolveSystemEntry("NES", "", "");
        compare(bare.name, Browse.HubLayout.resolve_system_name("NES"));
        compare(bare.coverKey, Browse.HubLayout.resolve_system_cover_key("NES"));
    }

    function test_resolve_system_entry_empty_id_is_skipped(): void {
        verify(main.hubScreen._resolveSystemEntry("", "", "") === null);
    }

    // Folders are addressed by path (GamesState.path_stack's own scheme);
    // the display name falls back to the path's final segment.
    function test_resolve_folder_entry_name_falls_back_to_final_path_segment(): void {
        const entry = main.hubScreen._resolveFolderEntry("/media/fat/games/SNES/Homebrew", "", "");
        compare(entry.name, "Homebrew");
        compare(entry.path, "/media/fat/games/SNES/Homebrew");
        const named = main.hubScreen._resolveFolderEntry("/media/fat/games/SNES/Homebrew", "My Folder", "");
        compare(named.name, "My Folder");
    }

    function test_resolve_folder_entry_empty_path_is_skipped(): void {
        verify(main.hubScreen._resolveFolderEntry("", "", "") === null);
    }

    // The same invariant one level up, on what the Hub actually renders. A
    // resolver regression that only bites a subset of ids would slip past the
    // direct test above; an empty coverKey here is a guaranteed blank tile.
    // Robust to whichever state Browse.HubLayout happens to be in (bootstrap
    // placeholders or a real seeded layout) -- every real (non-"empty",
    // i.e. non-padding) entry must carry a cover key either way.
    function test_hub_entries_all_carry_a_cover_key(): void {
        const entries = main.hubScreen.items.filter(entry => entry.kind !== "empty");
        verify(entries.length > 0, "the Hub always has at least categories and actions");
        for (let i = 0; i < entries.length; ++i)
            verify(entries[i].coverKey !== "", "empty coverKey on Hub entry " + entries[i].id);
    }

    // Mirrors PagedGrid._nearestVerticalCandidate exactly (`skipEmptyCells`,
    // which the Hub arms outside a Move session): a whole-board
    // nearest-candidate search, not a same-column walk -- see
    // PagedGrid.qml's own doc comment for the Android FocusFinder-derived
    // 13:1 weighting this reproduces. Two passes: real tiles strictly in
    // `dRow`'s direction first; if none, every real tile scored as if the
    // press wrapped around the far edge. Returns `startIndex` only when it
    // is the sole real tile on the whole board.
    function _expectedNearestVerticalCandidate(items: var, fromIndex: int, columns: int, rows: int, dRow: int): int {
        const pageSize = columns * rows;
        const totalRows = Math.ceil(items.length / pageSize) * rows;
        const srcLocal = fromIndex % pageSize;
        const srcRow = Math.floor(srcLocal / columns);
        const srcCol = srcLocal % columns;
        const srcVirtualRow = Math.floor(fromIndex / pageSize) * rows + srcRow;

        function bestOf(wrap: bool): int {
            let best = -1;
            let bestScore = Infinity;
            for (let idx = 0; idx < items.length; idx++) {
                if (idx === fromIndex || !items[idx] || items[idx].kind === "empty")
                    continue;
                const local = idx % pageSize;
                const row = Math.floor(local / columns);
                const col = local % columns;
                const virtualRow = Math.floor(idx / pageSize) * rows + row;
                let major;
                if (!wrap) {
                    if (dRow > 0 ? virtualRow <= srcVirtualRow : virtualRow >= srcVirtualRow)
                        continue;
                    major = dRow > 0 ? virtualRow - srcVirtualRow : srcVirtualRow - virtualRow;
                } else {
                    major = dRow > 0 ? virtualRow + (totalRows - srcVirtualRow) : srcVirtualRow + (totalRows - virtualRow);
                }
                const minor = Math.abs(col - srcCol);
                const score = 13 * major * major + minor * minor;
                if (score < bestScore) {
                    bestScore = score;
                    best = idx;
                }
            }
            return best;
        }
        const direct = bestOf(false);
        if (direct >= 0)
            return direct;
        const wrapped = bestOf(true);
        return wrapped >= 0 ? wrapped : fromIndex;
    }

    // Up from the top-left cell has nothing above it, so this always
    // exercises the wrap pass — the closed-loop board wraps around to
    // whichever real tile is nearest the bottom edge, weighted toward
    // staying column-aligned rather than necessarily landing in column 0.
    // Asserts against that same whole-board search instead of assuming
    // content shape.
    function test_up_on_top_row_wraps_to_bottom_row(): void {
        main.hubScreen.currentIndex = 0;
        const expected = testCase._expectedNearestVerticalCandidate(main.hubScreen.items, 0, Sizing.hubGridColumns, Sizing.hubGridRows, -1);
        main.handleKey(Qt.Key_Up);
        compare(main.hubScreen.currentIndex, expected, "Up from the top-left cell must wrap to the nearest real tile at the far edge");
        verify(main.hubScreen.items[main.hubScreen.currentIndex].kind !== "empty", "landed cell must be a real tile, not a blank");
    }

    // Right/Left wrap within whatever row the first action tile sits on —
    // true regardless of where that is now that the layout can freely
    // interleave any kind (no more fixed category/action block boundary;
    // `_firstItemIndexOfKind` searches for wherever it actually landed —
    // the bootstrap order seeds Resume before every placeholder category,
    // so that's index 0 in this harness, but the test doesn't assume that).
    // The row's last slot may be a real tile or a trailing `_padToPageSize`
    // empty slot depending on how many entries are visible; either way it's
    // `rowStart + columns - 1` (clamped to `items.length - 1`), same math
    // PagedGrid.moveSelection itself uses for a within-row wrap.
    function test_bottom_row_right_wraps_to_first(): void {
        const firstActionIndex = main.hubScreen._firstItemIndexOfKind("action");
        verify(firstActionIndex >= 0, "test bug: the Hub always has at least one action tile");
        const columns = Sizing.hubGridColumns;
        const rowStart = Math.floor(firstActionIndex / columns) * columns;
        const rowEnd = Math.min(main.hubScreen.items.length - 1, rowStart + columns - 1);
        main.hubScreen.currentIndex = rowEnd;
        main.handleKey(Qt.Key_Right);
        compare(main.hubScreen.currentIndex, rowStart, "Right at the actions row's last column wraps to its first");
    }

    function test_bottom_row_left_wraps_to_last(): void {
        const firstActionIndex = main.hubScreen._firstItemIndexOfKind("action");
        verify(firstActionIndex >= 0, "test bug: the Hub always has at least one action tile");
        const columns = Sizing.hubGridColumns;
        const rowStart = Math.floor(firstActionIndex / columns) * columns;
        const rowEnd = Math.min(main.hubScreen.items.length - 1, rowStart + columns - 1);
        main.hubScreen.currentIndex = rowStart;
        main.handleKey(Qt.Key_Left);
        compare(main.hubScreen.currentIndex, rowEnd, "Left at the actions row's first column wraps to its last");
    }

    // Resume (and every action/category) used to be null-returned from
    // `hub.items` while unavailable, which shifted every later tile's flat
    // index and could strand the restored cursor — see this round's plan
    // ("Tile state consolidation"). It's now always present; only its
    // `disabled` flag changes. This harness has no live Core connection, so
    // `resumeKnownUnavailable` can't be driven true here (it requires
    // `connection_state === 2`) — this test locks in the structural half:
    // Resume is never absent, and always carries a boolean `disabled`.
    function test_resume_tile_is_always_present_never_null_returned(): void {
        const items = main.hubScreen.items;
        const resumeIdx = items.findIndex(e => e.kind === "action" && e.id === "resume");
        verify(resumeIdx >= 0, "Resume must always be present in hub.items, never null-returned");
        compare(typeof items[resumeIdx].disabled, "boolean", "Resume entry must carry a disabled flag");
        compare(typeof items[resumeIdx].stateReason, "string", "Resume entry must carry a stateReason string");
    }

    // Round 8: the Resume tile prefers Browse.RecentsModel.resume_cover_key
    // (a real resolved cover -- see recents.rs's resume_cover_key_for) once
    // the model is live, falling back to the static glyph before first
    // frame -- mirrors _resolveActionEntry's own gating rather than
    // asserting one hardcoded resumeModelEnabled state, since exactly when
    // that flips true isn't what this test is checking.
    function test_resume_tile_cover_key_prefers_resolved_cover_once_live(): void {
        const items = main.hubScreen.items;
        const resumeIdx = items.findIndex(e => e.kind === "action" && e.id === "resume");
        verify(resumeIdx >= 0);
        const expected = main.hubScreen.resumeModelEnabled ? Browse.RecentsModel.resume_cover_key : "icons/PlayOutline";
        compare(items[resumeIdx].coverKey, expected);
    }

    // page_prev/page_next (L/R shoulder) were entirely unhandled on the Hub
    // before this round — every other paged screen supports them, and
    // Hub paging is now the normal case once grouping-padding is gone (see
    // HubScreen.qml's header comment). This harness's real content (5
    // placeholder categories + built-in actions) fits in a single page at
    // the 720 tier, so pageBy() itself returns false (exhaustively covered
    // for every wrap/clamp case in tst_paged_grid.qml already) — this test
    // locks in that the new branches exist and are a safe no-op rather
    // than an unhandled action or a crash.
    function test_page_prev_and_page_next_are_wired_and_safe_on_a_single_page(): void {
        const hub = main.hubScreen;
        hub.currentIndex = 2;
        const before = Browse.HubState.selected_item;
        main.handleKey(Qt.Key_PageUp);
        compare(hub.currentIndex, 2, "page_prev on a single-page Hub must not move the cursor");
        compare(Browse.HubState.selected_item, before, "a no-op page turn must not commit new HubState");
        main.handleKey(Qt.Key_PageDown);
        compare(hub.currentIndex, 2, "page_next on a single-page Hub must not move the cursor");
        compare(Browse.HubState.selected_item, before, "a no-op page turn must not commit new HubState");
    }

    // Round 6 follow-up: header->grid, grid->activeLabel, and
    // activeLabel->help-bar must all be the same gap. Previously the grid
    // and activeLabel were centered as one block with a fixed pctH(3) gap
    // between them, which only made the two OUTER gaps equal to each other
    // (a side effect of centering) — never to the fixed inner one.
    function test_hub_vertical_gaps_are_equal(): void {
        const hub = main.hubScreen;
        const headerToGrid = hub._blockY - Sizing.headerBottom;
        const gridToLabel = hub._verticalGap;
        const labelBottom = hub._blockY + hub._gridHeight + hub._verticalGap + hub._activeLabelHeight;
        const helpBarTop = hub.height - Sizing.pctH(6);
        const labelToHelpBar = helpBarTop - labelBottom;
        // Allow a 2px rounding tolerance -- the gap is one rounded
        // division across the band, not three independently distributed
        // pixel remainders (docs/style.md's "Grid cell rounding" accepts
        // the same class of small asymmetry elsewhere).
        verify(Math.abs(headerToGrid - gridToLabel) <= 2, "header->grid (" + headerToGrid + ") and grid->label (" + gridToLabel + ") gaps must match");
        verify(Math.abs(gridToLabel - labelToHelpBar) <= 2, "grid->label (" + gridToLabel + ") and label->help-bar (" + labelToHelpBar + ") gaps must match");
    }

    // resetFocus is the test-harness reset and the cold-launch state — it
    // must always land on Resume regardless of where focus was before.
    function test_reset_focus_seats_on_resume(): void {
        main.hubScreen.currentIndex = 2;
        main.hubScreen.resetFocus();
        const entry = main.hubScreen.items[main.hubScreen.currentIndex];
        compare(entry.kind, "action");
        compare(entry.id, "resume");
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

    // Round 10: a held direction consumed by a modal (e.g. GameInfoModal's
    // own scroll) must NOT arm rapid-nav tracking on the screen behind it
    // -- that state drives the Games/Favorites/Recents rapid-scroll
    // ghost-snapshot regardless of modal state, so arming it while a
    // modal owns input popped the background grid's freeze-frame in and
    // out on every repeat tick. See `_handleRepeatAction`'s doc comment.
    function test_repeat_tick_does_not_arm_rapid_navigation_while_modal_owns_input(): void {
        ScreenManager.pushModal("test_modal");
        try {
            main._armRepeat("down", Qt.Key_Down);
            main._handleRepeatAction();
            compare(main.rapidNavigationActive, false, "a modal-owned held direction must not arm the background screen's rapid-scroll state");
            compare(main.rapidNavigationIndicatorActive, false);
        } finally {
            main._stopRepeat();
            ScreenManager.popModal();
        }
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
        compare(_idsOf(entries), ["launch_system", "launch_random_system", "add_to_hub", "toggle_hide_system", "index_system", "scrape_system"], "Systems context menu is ordered primary/frequent/organizational/maintenance per docs/content-style.md");
        verify(entries[0].label.length > 0, "Launch system label is set (not asserted in English for translation)");
        verify(entries[1].label.length > 0, "Random game label is set");
        verify(entries[2].label.length > 0, "Add to Hub label is set");
        verify(entries[3].label.length > 0, "Hide label is set");
        verify(entries[4].label.length > 0, "Update media database label is set");
        verify(entries[5].label.length > 0, "Scrape metadata label is set");
    }

    function test_context_menu_systems_has_nfc_does_not_add_entries(): void {
        const entries = main.buildContextMenuEntries("systems", "", false, true, false, "", false);
        compare(_idsOf(entries), ["launch_system", "launch_random_system", "add_to_hub", "toggle_hide_system", "index_system", "scrape_system"], "has_nfc must not affect the systems menu");
    }

    // Category index/scrape are gated on the category having at least one
    // indexable (non-launch-only) system. The test Core is empty, so
    // SystemsModel.system_ids_for_category returns nothing and the gate must
    // omit the dead actions — categories have no Hide/Unhide entry any more
    // (the Hub is a persisted layout now, Browse.HubLayout; removing a
    // category tile is a layout edit — see docs/plans/ui-geometry-refresh.md's
    // Hub roadmap), so a category with nothing indexable has NO entries at
    // all. The positive branch (a mixed or fully-launchable category) is
    // covered at the data layer by the Rust `indexable_system_ids` tests,
    // which the empty test model can't exercise here.
    function test_context_menu_categories_empty_category_omits_index_scrape(): void {
        // Empty category short-circuits the gate (category !== "").
        const entries = main.buildContextMenuEntries("categories", "", false, false, false, "", false, "");
        compare(_idsOf(entries), [], "Empty category has no indexable systems and no hide entry, so the menu is empty");
    }

    function test_context_menu_categories_no_indexable_systems_omits_index_scrape(): void {
        // Non-empty category whose model yields no indexable systems.
        const entries = main.buildContextMenuEntries("categories", "", false, false, false, "", false, "Other");
        compare(_idsOf(entries), [], "A category with no indexable systems and no hide entry has an empty menu");
    }

    // A plain (non-media-capable) folder now offers a menu with exactly
    // one entry -- "Add to Hub" -- instead of no menu at all. Every
    // media-scoped action (favorite/NFC/QR/launch) stays gated on
    // mediaCapable exactly as before.
    function test_context_menu_games_directory_offers_add_to_hub_only(): void {
        const entries = main.buildContextMenuEntries("games", "directory", false, true, false, "");
        compare(_idsOf(entries), ["add_to_hub"], "A plain folder's only entry is the Hub shortcut action");
    }

    // Favorites never gets a folder-shortcut entry — a favorite is already
    // a saved shortcut of its own kind.
    function test_context_menu_favorites_directory_returns_empty(): void {
        compare(main.buildContextMenuEntries("favorites", "directory", false, true, false, ""), [], "Favorites rows never offer Add to Hub");
    }

    function test_context_menu_games_root_returns_empty(): void {
        compare(main.buildContextMenuEntries("games", "root", false, true, false, ""), []);
    }

    function test_context_menu_games_no_reader_omits_write_card(): void {
        const entries = main.buildContextMenuEntries("games", "media", true, false, false, "");
        compare(_idsOf(entries), ["launch_game", "more_info", "toggle_favorite", "qr_code", "add_to_hub"], "Write to NFC token must be hidden when no reader is reported");
    }

    function test_context_menu_games_with_reader_includes_write_card(): void {
        const entries = main.buildContextMenuEntries("games", "media", true, true, false, "");
        compare(_idsOf(entries), ["launch_game", "more_info", "toggle_favorite", "write_card", "qr_code", "add_to_hub"]);
    }

    function test_context_menu_favorites_matches_games_media_entries(): void {
        const entries = main.buildContextMenuEntries("favorites", "", true, true, true, "", false, "");
        compare(_idsOf(entries), ["launch_game", "more_info", "toggle_favorite", "write_card", "qr_code"]);
    }

    function test_context_menu_favorites_no_reader_omits_write_card(): void {
        const entries = main.buildContextMenuEntries("favorites", "", true, false, true, "", false, "");
        compare(_idsOf(entries), ["launch_game", "more_info", "toggle_favorite", "qr_code"]);
    }

    // Round 10: "Discover alt. versions" only appears when the row's own
    // system is the literal MiSTer Arcade/MRA system -- the backend
    // (alternate_versions.rs) matches `system_id == "Arcade"` exactly, not
    // Core's broader 32-system "Arcade" category, so showing the entry on
    // e.g. a CPS2 or NAOMI game would always return "No alternates found."
    function test_context_menu_games_discover_only_on_arcade_system(): void {
        const arcade = main.buildContextMenuEntries("games", "media", true, false, false, "Arcade");
        verify(_idsOf(arcade).includes("discover"), "Arcade games must offer Discover alt. versions");

        const nonArcade = main.buildContextMenuEntries("games", "media", true, false, false, "CPS2");
        verify(!_idsOf(nonArcade).includes("discover"), "A CPS2 game (Arcade-category, not the Arcade system) must not offer Discover alt. versions");
    }

    function test_context_menu_favorites_discover_only_on_arcade_system(): void {
        const arcade = main.buildContextMenuEntries("favorites", "", true, false, true, "Arcade", false, "");
        verify(_idsOf(arcade).includes("discover"));

        const nonArcade = main.buildContextMenuEntries("favorites", "", true, false, true, "SNES", false, "");
        verify(!_idsOf(nonArcade).includes("discover"));
    }

    function test_context_menu_recents_discover_only_on_arcade_system(): void {
        const arcade = main.buildContextMenuEntries("recents", "", false, false, false, "Arcade", false, "");
        verify(_idsOf(arcade).includes("discover"));

        const nonArcade = main.buildContextMenuEntries("recents", "", false, false, false, "NES", false, "");
        verify(!_idsOf(nonArcade).includes("discover"));
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

    // Recents offers Details, write-token/QR, and Add to Hub like Games —
    // but never a favorite-toggle, since Core's media.history carries no
    // tags (see the doc comment on RecentsModel in recents.rs).
    function test_context_menu_recents_includes_details_and_hub_shortcut(): void {
        const entries = main.buildContextMenuEntries("recents", "", false, false, false, "", false, "");
        compare(_idsOf(entries), ["launch_game", "more_info", "qr_code", "add_to_hub"]);
    }

    function test_context_menu_recents_with_reader_includes_write_card(): void {
        const entries = main.buildContextMenuEntries("recents", "", false, true, false, "", false, "");
        compare(_idsOf(entries), ["launch_game", "more_info", "write_card", "qr_code", "add_to_hub"]);
    }

    function test_context_menu_games_favorite_label_toggles(): void {
        const addEntries = main.buildContextMenuEntries("games", "media", true, false, false, "");
        const removeEntries = main.buildContextMenuEntries("games", "media", true, false, true, "");
        compare(addEntries[2].id, "toggle_favorite");
        compare(removeEntries[2].id, "toggle_favorite");
        verify(addEntries[2].label.length > 0);
        verify(removeEntries[2].label.length > 0);
        verify(addEntries[2].label !== removeEntries[2].label);
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
        verify(ids.indexOf("back_to_hub") !== -1, "Back to Hub entry present");
        main.closeListPickerModal();
    }

    // "Back to Hub" is unconditional -- present and functional on the
    // Games View menu regardless of how the screen was reached, not just
    // for shortcut-entered sessions.
    function test_games_page_menu_back_to_hub_navigates_to_hub(): void {
        main.activeScreen = main.screenGames;
        main.openPageMenu();
        main.listPickerAccepted("page_menu", "back_to_hub");
        tryCompare(main, "listPickerModalVisible", false);
        compare(main.activeScreen, main.screenHub);
    }

    function test_favorites_page_menu_back_to_hub_navigates_to_hub(): void {
        main.activeScreen = main.screenFavorites;
        main.openFavoritesPageMenu();
        main.listPickerAccepted("page_menu_favorites", "back_to_hub");
        tryCompare(main, "listPickerModalVisible", false);
        compare(main.activeScreen, main.screenHub);
    }

    // Quit moved off B/Cancel onto the Hub's View menu, alongside a
    // shortcut straight to Settings for users who don't want it on the
    // tile grid — see this round's plan.
    function test_hub_page_menu_settings_entry_navigates_to_settings(): void {
        const originalCatalogLoaded = Browse.CategoriesModel.loaded;
        Browse.CategoriesModel.loaded = true;
        main.activeScreen = main.screenHub;
        main.openHubPageMenu();
        main.listPickerAccepted("page_menu_hub", "hub_settings");
        tryCompare(main, "listPickerModalVisible", false);
        compare(main.activeScreen, main.screenSettings);
        Browse.CategoriesModel.loaded = originalCatalogLoaded;
        main._goto(main.screenHub);
    }

    function test_hub_page_menu_quit_entry_opens_quit_confirm_modal(): void {
        main.activeScreen = main.screenHub;
        compare(main.quitConfirmModalVisible, false);
        main.openHubPageMenu();
        main.listPickerAccepted("page_menu_hub", "hub_quit");
        tryCompare(main, "listPickerModalVisible", false);
        compare(main.quitConfirmModalVisible, true);
        main.closeQuitConfirmModal();
    }

    // Cancel/B is unbound on the Hub root now — Quit lives in the View menu
    // instead of on Cancel, so a stray Escape/B press must not do anything.
    function test_hub_cancel_is_a_no_op_when_not_moving(): void {
        main.activeScreen = main.screenHub;
        compare(main.quitConfirmModalVisible, false);
        main.hubScreen.handleAction("cancel");
        compare(main.quitConfirmModalVisible, false, "cancel must not open the quit-confirm modal");
        compare(main.activeScreen, main.screenHub, "cancel must not navigate away from the Hub root");
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

        main.openListPickerModal("Orientation", [
            {
                id: "horizontal",
                label: "Horizontal"
            }
        ], "horizontal", "orientation");
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
        compare(main.actionErrorModalVisible, true);
        verify((Browse.GamesModel.random_error ?? "") !== "", "failure reason recorded");
        main.handleAction("cancel");
        tryCompare(main, "randomFailedModalVisible", false);
        compare(main.actionErrorModalVisible, false);
        compare(Browse.GamesModel.random_error, "", "dismissal clears reason");
    }

    function test_action_error_owns_input_and_dismisses(): void {
        main.activeScreen = main.screenHub;
        const startIndex = main.hubScreen.currentIndex;
        main.presentActionError("launch:test", "Launch failed", "Safe explanation", "OK", null);
        compare(main.actionErrorModalVisible, true);

        main.handleAction("right");
        compare(main.hubScreen.currentIndex, startIndex, "modal must own directional input");
        main.handleAction("cancel");
        compare(main.actionErrorModalVisible, false);
    }

    function test_action_error_deduplicates_and_queues(): void {
        main.presentActionError("same", "First", "First body", "OK", null);
        main.presentActionError("same", "Duplicate", "Duplicate body", "OK", null);
        compare(main.actionErrorTitle, "First");
        compare(main._actionErrorQueue.length, 0, "same visible failure is deduplicated");

        main.presentActionError("next", "Second", "Second body", "OK", null);
        compare(main._actionErrorQueue.length, 1, "different failure waits behind current modal");
        main.handleAction("accept");
        main.presentActionError("last", "Third", "Third body", "OK", null);
        compare(main.actionErrorModalVisible, false, "new failure must wait during queued-modal handoff");
        compare(main._actionErrorQueue.length, 2, "handoff preserves FIFO order");
        tryCompare(main, "actionErrorTitle", "Second");
        compare(main._actionErrorQueue.length, 1);
        main.handleAction("cancel");
        tryCompare(main, "actionErrorTitle", "Third");
        compare(main._actionErrorQueue.length, 0);
        main.handleAction("cancel");
    }

    function test_action_error_accept_dispatches_retry_once(): void {
        main.presentActionError("retry", "Retry failed action", "Safe explanation", "Retry", function () {
            testCase._actionErrorCallbackCount++;
        });
        main.handleAction("accept");
        compare(testCase._actionErrorCallbackCount, 1);
        compare(main.actionErrorModalVisible, false);
        main.handleAction("accept");
        compare(testCase._actionErrorCallbackCount, 1, "dismissed modal cannot repeat callback");
    }

    function test_action_error_rust_bridge_delivers_all_events(): void {
        actionErrorBatchSpy.clear();
        testCase._actionErrorDeliveredCount = 0;
        const oversizedPayload = "x".repeat(5000);
        Browse.QrCode.generate(oversizedPayload);
        compare(Browse.QrCode.size, 0, "first oversized payload must fail generation");
        Browse.QrCode.generate(oversizedPayload + "y");
        compare(Browse.QrCode.size, 0, "second oversized payload must fail generation");

        tryCompare(testCase, "_actionErrorDeliveredCount", 2, 1000, "Rust queue must retain both events across batches");
        verify(actionErrorBatchSpy.count >= 1, "Rust bridge publishes at least one observable batch");
        tryCompare(main, "actionErrorModalVisible", true);
        compare(main.actionErrorTitle, "QR code failed");
        compare(main.actionErrorBody, "Could not create the QR code for this item.");
        compare(main._actionErrorQueue.length, 0, "same failure kind is deduplicated after bridge delivery");
        main.handleAction("cancel");
    }

    function test_systems_error_retry_refetches_catalog(): void {
        main.activeScreen = main.screenSystems;
        const originalCategory = Browse.SystemsModel.current_category;
        const originalError = Browse.SystemsModel.error_message;
        const originalLoading = Browse.SystemsModel.loading;
        Browse.SystemsModel.current_category = "Consoles";
        Browse.SystemsModel.loading = false;
        main.systemsScreen.optimisticLoading = false;
        tryCompare(main.systemsScreen, "_overlayLoadingVisible", false);
        Browse.SystemsModel.error_message = "catalog failed";
        compare(Browse.SystemsModel.current_category, "Consoles");
        compare(main.systemsScreen._state(), "error");

        main.systemsScreen.handleAction("accept");
        compare(Browse.SystemsModel.loading, true, "Retry starts a fresh catalog load");
        compare(Browse.SystemsModel.error_message, "", "Retry clears terminal error while loading");

        Browse.SystemsModel.current_category = originalCategory;
        Browse.SystemsModel.error_message = originalError;
        Browse.SystemsModel.loading = originalLoading;
    }

    function test_media_error_hides_live_content(): void {
        main.activeScreen = main.screenGames;
        const originalError = Browse.GamesModel.error_message;
        Browse.GamesModel.error_message = "raw rpc detail";
        compare(main.gamesScreen._gateHide, true);
        compare(main.gamesScreen.gamesGrid.visible, false);
        compare(main.gamesScreen.listCard.visible, false);
        compare(main.gamesScreen.activeLabel.visible, false);
        Browse.GamesModel.error_message = originalError;

        main.activeScreen = main.screenSystems;
        const originalSystemsError = Browse.SystemsModel.error_message;
        Browse.SystemsModel.error_message = "raw catalog detail";
        compare(main.systemsScreen._gateHide, true);
        compare(main.systemsScreen.systemsGrid.visible, false);
        compare(main.systemsScreen.listCard.visible, false);
        compare(main.systemsScreen.activeLabel.visible, false);
        Browse.SystemsModel.error_message = originalSystemsError;
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
        compare(ids, ["favorites_sort", "favorites_mode", "launch_random_favorite", "back_to_hub"], "View order is Sort, Group by, Random, Back to Hub");
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
