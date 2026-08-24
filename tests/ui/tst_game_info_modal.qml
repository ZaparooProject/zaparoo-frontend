// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

import QtQuick
import QtTest
import Zaparoo.Theme
import Zaparoo.Ui

// Round 10 coverage: the fixed tag-label column (replacing the old
// accumulate-forever `_labelColumnWidth`) and the scroll-chevron
// hide-when-not-scrollable rule (matching PageIndicator.qml's own
// single-page hide rule). Both are pure computed properties that don't
// depend on Browse.GameInfo's live RPC data, so they're testable without
// driving that singleton's `load()`.
TestCase {
    id: testCase
    name: "UiGameInfoModal"
    when: windowShown
    width: 640
    height: 480
    visible: true

    GameInfoModal {
        id: modal
        anchors.fill: parent
        open: true
    }

    function initTestCase(): void {
        Sizing.screenWidth = testCase.width;
        Sizing.screenHeight = testCase.height;
    }

    // Fixed against the known ordered-tag vocabulary in
    // game_info.rs's `detail_tags_from_meta`/`display_label` (System,
    // Platform, Year, Release date, Genre, Players, Play mode,
    // Cooperative, Developer, Publisher, Rating, Filename) — a pure
    // function of that fixed list and the caption font metrics, not of
    // whatever tags the currently-loaded game happens to carry.
    function test_label_column_width_is_positive_and_fixed(): void {
        verify(modal._labelColumnWidth > 0);
        const before = modal._labelColumnWidth;
        // Re-reading must be stable — nothing about opening/closing or
        // the absence of live tag data should perturb a value derived
        // entirely from the fixed label set and the font.
        modal.open = false;
        modal.open = true;
        compare(modal._labelColumnWidth, before, "label column width must not depend on modal open/close state");
    }

    // Before any game is loaded, Browse.GameInfo starts idle (not
    // loading, no error, no tags, no cover) — the flickable has nothing
    // to scroll, so both round-10 chevrons must stay hidden entirely
    // rather than showing two permanently-dim arrows pointing at nothing.
    function test_scroll_chevrons_hidden_when_content_does_not_overflow(): void {
        compare(modal._scrollable, false, "an idle GameInfo with no tags/description/cover has nothing to scroll");
        const up = findChild(modal, "gameInfoScrollUp");
        const down = findChild(modal, "gameInfoScrollDown");
        verify(up !== null);
        verify(down !== null);
        compare(up.visible, false);
        compare(down.visible, false);
    }
}
