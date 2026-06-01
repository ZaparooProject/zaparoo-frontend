// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

import QtQuick
import QtTest
import Zaparoo.App

// Exercises the hub ↔ systems ↔ games navigation state machine defined
// in Main.qml. State is driven either by writing to the activeScreen
// property (the observable contract) or by calling root.handleKey(key)
// directly — the latter proves the Keys.onPressed routing, which we
// can't exercise with keyClick because offscreen ApplicationWindows
// don't receive routed key events reliably.
TestCase {
    name: "UiNavigation"
    when: windowShown

    Main {
        id: main
        fullScreen: false
        width: 1280
        height: 720
    }

    function init(): void {
        // The cold-launch BootOverlay normally hides every screen until
        // Core's catalog reaches READY. Tests don't run a real Core, so
        // we mark the boot complete up-front; otherwise every visibility
        // assertion below would fail against the boot curtain.
        main.bootComplete = true;
        main.activeScreen = main.screenHub;
        // Hub focus is two rows now (categories + actions); reset both
        // axes so a prior test's row-jump doesn't leak into the next.
        // qmllint disable compiler
        main.hubScreen.resetFocus();
        // qmllint enable compiler
        // Cancel any in-flight dpad-repeat timer left over from a prior
        // test — handleKey(dpad) arms a 350 ms initial timer and tests
        // run in microseconds, so the pending fire would land on the
        // next test if we didn't reset it here.
        main._stopRepeat();
    }

    function test_initial_state_is_hub(): void {
        compare(main.activeScreen, main.screenHub);
        compare(main.hubScreen.visible, true);
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

    // Enter on hub categories drills into systems screen.
    function test_enter_on_hub_routes_to_systems(): void {
        main.handleKey(Qt.Key_Return);
        compare(main.activeScreen, main.screenSystems);
    }

    // Down on hub moves focus between the categories row and the
    // actions row (Favorites / Recently Played / Settings); it must
    // never flip off-screen to systems. Accept is the only path that
    // drills into another screen.
    function test_down_on_hub_does_not_route_to_systems(): void {
        main.handleKey(Qt.Key_Down);
        compare(main.activeScreen, main.screenHub, "Down on hub must not flip to systems — only Accept drills");
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

    // Escape on games goes back to systems (one peer up the stack).
    function test_escape_on_games_returns_to_systems(): void {
        main.activeScreen = main.screenGames;
        main.handleKey(Qt.Key_Escape);
        compare(main.activeScreen, main.screenSystems);
    }

    // Escape on systems goes back to hub.
    function test_escape_on_systems_returns_to_hub(): void {
        main.activeScreen = main.screenSystems;
        main.handleKey(Qt.Key_Escape);
        compare(main.activeScreen, main.screenHub);
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
        compare(main.activeScreen, main.screenSystems);
    }

    // Cross-row mapping. The test harness has no live CategoriesModel
    // so we can't drive the full handleAction("down") flow with real
    // categories — instead we unit-test the pure arithmetic helper
    // that owns the math. The shape verifies centered row mapping and
    // a couple of degenerate cases.
    // qmllint disable compiler
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

    // 4-over-3 (the previous Favorites layout) — the offset is 0.5,
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

    // Up on the top row wraps onto the bottom row (the two rows form a
    // closed loop). Test harness has no live categories, so we start
    // at top[0] and just verify currentRow flipped — the destination
    // index is verified by the _mapCrossRow tests above.
    function test_up_on_top_row_wraps_to_bottom_row(): void {
        // resetFocus() in init() leaves us on top[0].
        main.handleKey(Qt.Key_Up);
        compare(main.hubScreen.currentRow, 1, "Up from top should wrap to bottom row");
    }

    // Bottom row wraps left/right. Use Down from top[0] to drop into
    // the bottom row first; bottomCount=3 so a single Right at the
    // last index must wrap to 0.
    function test_bottom_row_right_wraps_to_first(): void {
        main.handleKey(Qt.Key_Down);
        // _mapCrossRow(0, topCount=0, 3) lands us at bottom[2].
        compare(main.hubScreen.currentRow, 1);
        compare(main.hubScreen.currentIndex, 2, "Centered map of top[0] with empty top lands at bottom[2]");
        main.handleKey(Qt.Key_Right);
        compare(main.hubScreen.currentIndex, 0, "Right at last bottom-row index wraps to first");
    }

    function test_bottom_row_left_wraps_to_last(): void {
        main.handleKey(Qt.Key_Down);
        compare(main.hubScreen.currentRow, 1);
        // Drive Left three times:
        // bottom[2] → bottom[1] → bottom[0] → wrap to bottom[2].
        main.handleKey(Qt.Key_Left);
        compare(main.hubScreen.currentIndex, 1);
        main.handleKey(Qt.Key_Left);
        compare(main.hubScreen.currentIndex, 0);
        main.handleKey(Qt.Key_Left);
        compare(main.hubScreen.currentIndex, 2, "Left at first bottom-row index wraps to last");
    }

    // Cross-row round-trip. With 4 categories on top vs 3 actions on
    // bottom, the centered visual-nearest map can't return Up to the
    // tile a previous Down originated from — every Down→Up shifts
    // right by one cell. The fix is `_crossSavedIndex`: each cross
    // saves the source-row index, the next cross restores it, any
    // horizontal input on the destination row invalidates it.

    // After Down from top[0], the saved index must hold 0 so the next
    // Up can return there. `_mapCrossRow(0, topCount=0, 3)` puts us at
    // bottom[2] regardless — that part is unchanged.
    function test_cross_row_arms_saved_source_index(): void {
        main.handleKey(Qt.Key_Down);
        compare(main.hubScreen.currentRow, 1);
        compare(main.hubScreen._crossSavedIndex, 0, "Down from top[0] must save 0 for the round-trip back");
    }

    // Horizontal input on the destination row clears the saved index
    // — the user has now committed to navigating within the new row,
    // so the next cross should fall back to the centered visual map.
    function test_cross_row_horizontal_input_clears_saved_index(): void {
        main.handleKey(Qt.Key_Down);
        compare(main.hubScreen._crossSavedIndex, 0);
        main.handleKey(Qt.Key_Left);
        compare(main.hubScreen._crossSavedIndex, -1, "Left on the destination row must invalidate the round-trip");
    }

    // Mouse focus is a deliberate landing on a specific tile, same
    // intent as a horizontal arrow press — clear the saved index so a
    // later Up doesn't snap back to a row the user already left.
    function test_cross_row_mouse_focus_clears_saved_index(): void {
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
        // _mapCrossRow(0, topCount=0, 3) lands at bottom[2].
        compare(main.hubScreen.currentIndex, 2, "Out-of-range saved index falls back to the visual map");
    }

    // resetFocus is the test-harness reset and the cold-launch state.
    // It must clear the round-trip arm so a prior test's saved index
    // can't leak into the next case.
    function test_reset_focus_clears_saved_index(): void {
        main.hubScreen._crossSavedIndex = 2;
        main.hubScreen.resetFocus();
        compare(main.hubScreen._crossSavedIndex, -1);
    }
    // qmllint enable compiler

    // Hold-to-repeat (dpad). The repeat state machine is driven by
    // `_armRepeat` (called from handleKey on a dpad press) and
    // unwound by `_stopRepeat` and `handleKeyRelease`. These tests
    // drive the helpers directly to keep the assertion surface
    // narrow — handleKey's outer "fire handleAction then _armRepeat"
    // shape is one trivial line and doesn't need a per-action test
    // that drags real screen logic into the harness.

    function test_is_repeatable_action_accepts_dpad_directions(): void {
        // qmllint disable compiler
        compare(main._isRepeatableAction("up"), true);
        compare(main._isRepeatableAction("down"), true);
        compare(main._isRepeatableAction("left"), true);
        compare(main._isRepeatableAction("right"), true);
    // qmllint enable compiler
    }

    function test_is_repeatable_action_rejects_other_actions(): void {
        // qmllint disable compiler
        compare(main._isRepeatableAction("accept"), false);
        compare(main._isRepeatableAction("cancel"), false);
        compare(main._isRepeatableAction("page_prev"), false);
        compare(main._isRepeatableAction("page_next"), false);
        compare(main._isRepeatableAction("write_card"), false);
        compare(main._isRepeatableAction(""), false);
    // qmllint enable compiler
    }

    function test_arm_repeat_records_held_and_starts_initial(): void {
        main._armRepeat("down", Qt.Key_Down);
        compare(main._heldAction, "down");
        compare(main._heldKey, Qt.Key_Down);
        compare(main._repeatPending, true, "Initial-delay timer must be running after _armRepeat");
        compare(main._repeatTicking, false, "Steady tick must not start before the initial delay");
    }

    function test_arm_repeat_with_non_dpad_action_is_noop(): void {
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

    // Context-menu builder. Drives the pure helper directly per the QML
    // test isolation rule — no real menu opening, no handleAction.
    // Compares only the entry id sequence; labels are qsTr() and asserted
    // separately so the tests stay translation-friendly.
    // qmllint disable compiler
    function _idsOf(entries: var): var {
        const out = [];
        for (let i = 0; i < entries.length; ++i)
            out.push(entries[i].id);
        return out;
    }
    // qmllint enable compiler

    function test_context_menu_systems_owner_is_single_launch_core(): void {
        // qmllint disable compiler
        const entries = main.buildContextMenuEntries("systems", "", false, false, false, "");
        compare(_idsOf(entries), ["launch_system"], "Systems context menu is just Launch core regardless of has_nfc");
        verify(entries[0].label.length > 0, "Launch core label is set (not asserted in English for translation)");
    // qmllint enable compiler
    }

    function test_context_menu_systems_has_nfc_does_not_add_entries(): void {
        // qmllint disable compiler
        const entries = main.buildContextMenuEntries("systems", "", false, true, false, "");
        compare(_idsOf(entries), ["launch_system"], "has_nfc must not affect the systems menu");
    // qmllint enable compiler
    }

    function test_context_menu_games_directory_returns_empty(): void {
        // qmllint disable compiler
        compare(main.buildContextMenuEntries("games", "directory", false, true, false, ""), [], "Folder tiles have no context menu, even with reader attached");
    // qmllint enable compiler
    }

    function test_context_menu_games_root_returns_empty(): void {
        // qmllint disable compiler
        compare(main.buildContextMenuEntries("games", "root", false, true, false, ""), []);
    // qmllint enable compiler
    }

    function test_context_menu_games_no_reader_omits_write_card(): void {
        // qmllint disable compiler
        const entries = main.buildContextMenuEntries("games", "media", true, false, false, "");
        compare(_idsOf(entries), ["toggle_favorite", "qr_code", "launch_game"], "Write to NFC token must be hidden when no reader is reported");
    // qmllint enable compiler
    }

    function test_context_menu_games_with_reader_includes_write_card(): void {
        // qmllint disable compiler
        const entries = main.buildContextMenuEntries("games", "media", true, true, false, "");
        compare(_idsOf(entries), ["toggle_favorite", "write_card", "qr_code", "launch_game"]);
    // qmllint enable compiler
    }

    function test_context_menu_favorites_matches_games_media_entries(): void {
        // qmllint disable compiler
        const entries = main.buildContextMenuEntries("favorites", "", true, true, true, "");
        compare(_idsOf(entries), ["toggle_favorite", "write_card", "qr_code", "launch_game"]);
    // qmllint enable compiler
    }

    function test_context_menu_favorites_no_reader_omits_write_card(): void {
        // qmllint disable compiler
        const entries = main.buildContextMenuEntries("favorites", "", true, false, true, "");
        compare(_idsOf(entries), ["toggle_favorite", "qr_code", "launch_game"]);
    // qmllint enable compiler
    }

    function test_context_menu_recents_omits_more_info(): void {
        // qmllint disable compiler
        const entries = main.buildContextMenuEntries("recents", "", false, false, false, "");
        compare(_idsOf(entries), ["launch_game"]);
    // qmllint enable compiler
    }

    function test_context_menu_games_favorite_label_toggles(): void {
        // qmllint disable compiler
        const addEntries = main.buildContextMenuEntries("games", "media", true, false, false, "");
        const removeEntries = main.buildContextMenuEntries("games", "media", true, false, true, "");
        compare(addEntries[0].id, "toggle_favorite");
        compare(removeEntries[0].id, "toggle_favorite");
        verify(addEntries[0].label.length > 0);
        verify(removeEntries[0].label.length > 0);
        verify(addEntries[0].label !== removeEntries[0].label);
    // qmllint enable compiler
    }

    function test_context_menu_unknown_owner_returns_empty(): void {
        // qmllint disable compiler
        compare(main.buildContextMenuEntries("nope", "", false, true, false, ""), [], "Unknown owners get no entries — safe default");
    // qmllint enable compiler
    }

    // QR-code payload wrapper. The web app at zaparoo.app/write reads the
    // zapscript out of the `v=` query param, so the helper must
    // URL-encode reserved characters.
    function test_qr_payload_empty_zapscript(): void {
        // qmllint disable compiler
        compare(main._buildQrPayload(""), "https://zaparoo.app/write?v=");
    // qmllint enable compiler
    }

    function test_qr_payload_plain_ascii(): void {
        // qmllint disable compiler
        compare(main._buildQrPayload("foo"), "https://zaparoo.app/write?v=foo");
    // qmllint enable compiler
    }

    function test_qr_payload_encodes_reserved_chars(): void {
        // qmllint disable compiler
        // encodeURIComponent leaves `* - _ . ! ~ ' ( )` unescaped — only
        // characters that would terminate or restructure the URL get
        // percent-encoded. Real zapscripts look like
        // `**launch.system:foo`; only the `:` needs escaping (it would
        // otherwise be read as a port separator in some parsers).
        const payload = main._buildQrPayload("**launch.system:Atari2600");
        compare(payload, "https://zaparoo.app/write?v=**launch.system%3AAtari2600");
    // qmllint enable compiler
    }

    function test_qr_payload_encodes_url_breakers(): void {
        // qmllint disable compiler
        // Belt-and-braces check that characters that *would* break the URL
        // (space, `&`, `?`) are escaped as expected. None of these appear
        // in current zapscripts but a future zapscript with arguments
        // containing them must still survive a round-trip.
        compare(main._buildQrPayload("a b&c?d"), "https://zaparoo.app/write?v=a%20b%26c%3Fd");
    // qmllint enable compiler
    }
}
