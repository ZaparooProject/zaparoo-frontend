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

    // Both chevrons hang OUTSIDE the flickable — the up one above
    // `flick.top`, the down one below `flick.bottom` — so each needs its own
    // height plus its margin of clear space on that side. The up chevron's gap
    // was `pctH(2)` against a `pctH(3.5)` requirement, so it drew through the
    // title divider whenever the body scrolled. Anchors resolve regardless of
    // `visible`, so this is checkable without driving Browse.GameInfo's live
    // data, matching how the rest of this file works.
    function test_scroll_chevrons_do_not_overlap_their_neighbours(): void {
        const up = findChild(modal, "gameInfoScrollUp");
        const down = findChild(modal, "gameInfoScrollDown");
        const divider = findChild(modal, "gameInfoTitleDivider");
        verify(up !== null);
        verify(down !== null);
        verify(divider !== null, "title divider needs an objectName for this assertion");
        verify(up.height > 0, "chevron must have resolved geometry");
        verify(up.y >= divider.y + divider.height, "up chevron (" + up.y + ") must clear the title divider (" + (divider.y + divider.height) + ")");
        // The down chevron has the room already; assert it so a future change
        // to the bottom margin cannot quietly take it away.
        verify(down.y + down.height <= modal.height, "down chevron must stay inside the card");
    }
}
