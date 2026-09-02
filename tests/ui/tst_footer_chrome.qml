// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

import QtQuick
import QtTest
import Zaparoo.Theme
import Zaparoo.Ui

// Covers the two small building blocks the "kill the scrollbars" round
// introduced: PageIndicator (the footer's up/down + "N/M" page cue that
// replaced PagedGrid's right-gutter scroll indicator) and ActiveLabel's
// new `sideInset`, which keeps a long focused-item name from running
// under the footer's reserved corner slots. See PagedGrid.qml's
// `tst_paged_grid.qml` for the pagination-geometry-invariant coverage
// (cell block position/size never moving when pages become relevant) —
// this file is just the two new components in isolation.
TestCase {
    id: testCase
    name: "UiFooterChrome"
    when: windowShown
    width: 400
    height: 100
    visible: true

    Component.onCompleted: {
        Sizing.screenWidth = testCase.width;
        Sizing.screenHeight = testCase.height;
    }

    PageIndicator {
        id: indicator
        anchors.right: parent.right
        currentPage: 0
        totalPages: 1
        hasPagesAbove: false
        hasPagesBelow: false
    }

    SignalSpy {
        id: pageRequestedSpy
        target: indicator
        signalName: "pageRequested"
    }

    function init(): void {
        indicator.currentPage = 0;
        indicator.totalPages = 1;
        indicator.pageTotalKnown = true;
        indicator.hasPagesAbove = false;
        indicator.hasPagesBelow = false;
        pageRequestedSpy.clear();
    }

    // The whole point of moving the page cue out of PagedGrid's gutter:
    // a chevron's own visibility must never change the indicator's
    // overall width. If it did, the footer's reserved slot (and anything
    // anchored against it, like ActiveLabel's elide point) would still
    // shift the instant a grid crossed from one page to two — the exact
    // bug this component exists to fix, just at a smaller scale.
    function test_width_is_unaffected_by_chevron_visibility(): void {
        indicator.hasPagesAbove = false;
        indicator.hasPagesBelow = false;
        const bothHidden = indicator.width;

        indicator.hasPagesAbove = true;
        indicator.hasPagesBelow = true;
        compare(indicator.width, bothHidden, "showing both chevrons must not change the reserved width");

        indicator.hasPagesAbove = true;
        indicator.hasPagesBelow = false;
        compare(indicator.width, bothHidden, "showing only one chevron must not change the reserved width either");
    }

    // Round 10: the chevrons themselves must hide entirely (not just dim)
    // on a single page -- two permanently-dim arrows pointing at nothing
    // to page to said nothing useful. Same `_hasMultiplePages` gate the
    // page-count text already used before this round.
    function test_chevrons_hide_entirely_on_a_single_page(): void {
        indicator.pageTotalKnown = true;
        indicator.totalPages = 1;
        indicator.hasPagesAbove = false;
        indicator.hasPagesBelow = false;
        // Neither chevron accepts a click when hidden -- mouseClick at
        // the up-chevron's position must not request a page.
        mouseClick(indicator, Sizing.half(indicator.chevronSize), Sizing.half(indicator.chevronSize), Qt.LeftButton);
        compare(pageRequestedSpy.count, 0, "a hidden, single-page chevron must not be clickable");

        indicator.hasPagesBelow = true;
        mouseClick(indicator, Sizing.half(indicator.chevronSize), Sizing.half(indicator.chevronSize), Qt.LeftButton);
        compare(pageRequestedSpy.count, 0, "the up chevron must stay hidden and unclickable when only \"below\" has more pages");

        const downX = indicator.chevronSize + indicator.chevronSpacing + Sizing.half(indicator.chevronSize);
        mouseClick(indicator, downX, Sizing.half(indicator.chevronSize), Qt.LeftButton);
        compare(pageRequestedSpy.count, 1, "the down chevron must become visible and clickable once there is another page");
    }

    // A "1/1" or bare "1" readout next to two chevrons that are also
    // both hidden says nothing a user needs -- see PageIndicator.qml's
    // `_hasMultiplePages` doc comment. Covers both the known-total case
    // (totalPages stays 1) and the unknown-total case (a cursor list with
    // nothing above or below to fetch), since both mean "only one page."
    function test_page_text_hides_when_there_is_only_one_page(): void {
        const text = findChild(indicator, "pageIndicatorText");
        verify(text !== null);

        indicator.pageTotalKnown = true;
        indicator.totalPages = 1;
        indicator.hasPagesAbove = false;
        indicator.hasPagesBelow = false;
        compare(text.visible, false, "known single-page total must hide the readout");

        indicator.pageTotalKnown = false;
        indicator.hasPagesAbove = false;
        indicator.hasPagesBelow = false;
        compare(text.visible, false, "an unknown total with nothing to page to must also hide the readout");

        indicator.hasPagesBelow = true;
        compare(text.visible, true, "the moment there's another page to reach, the readout must reappear");
    }

    function test_known_total_shows_denominator(): void {
        indicator.pageTotalKnown = true;
        indicator.currentPage = 1;
        indicator.totalPages = 5;
        compare(indicator.pageText, "2/5");
    }

    // Cursor-paginated lists (Favorites/Recents/Games) cannot know their
    // final page until the cursor is exhausted — a denominator here would
    // grow as more rows arrive. Mirrors TopStatusStrip's own known/unknown
    // split, just without the "Page" word (see PageIndicator.qml's doc
    // comment on `pageTotalKnown`).
    function test_unknown_total_shows_bare_page_number(): void {
        indicator.pageTotalKnown = false;
        indicator.currentPage = 2;
        indicator.totalPages = 1;
        compare(indicator.pageText, "3");
        verify(indicator.pageText.indexOf("/") < 0);
    }

    function test_up_chevron_click_requests_previous_page(): void {
        indicator.hasPagesAbove = true;
        mouseClick(indicator, Sizing.half(indicator.chevronSize), Sizing.half(indicator.chevronSize), Qt.LeftButton);
        compare(pageRequestedSpy.count, 1);
        compare(pageRequestedSpy.signalArguments[0][0], -1);
    }

    function test_down_chevron_click_requests_next_page(): void {
        indicator.hasPagesAbove = true;
        indicator.hasPagesBelow = true;
        const downX = indicator.chevronSize + indicator.chevronSpacing + Sizing.half(indicator.chevronSize);
        mouseClick(indicator, downX, Sizing.half(indicator.chevronSize), Qt.LeftButton);
        compare(pageRequestedSpy.count, 1);
        compare(pageRequestedSpy.signalArguments[0][0], 1);
    }

    // The chevron pair must read as one control: the anchor gap between
    // them is strictly tighter than the gap the pair keeps from the
    // trailing "N/M" text. See PageIndicator.qml's doc comment on
    // `chevronSpacing` for the glyph-bearing measurement behind this.
    function test_chevron_pair_spacing_is_tighter_than_text_spacing(): void {
        verify(indicator.chevronSpacing < indicator.itemSpacing);
    }

    ActiveLabel {
        id: activeLabel
        width: 300
        height: 40
        sideInset: 100
        text: "A Very Long Focused Item Name That Would Otherwise Run On"
    }

    // Mirrors the reserved one-third footer slot every host screen now
    // sets `sideInset` to (see HubScreen/SystemsScreen/MediaListScreen) —
    // the name must elide before it reaches that reserved width rather
    // than running underneath the count badge or PageIndicator.
    function test_active_label_side_inset_limits_the_name_block(): void {
        compare(activeLabel._maxWidth, activeLabel.width - 2 * activeLabel.sideInset);
        verify(activeLabel._nameWidth <= activeLabel._maxWidth);
    }

    // Default sideInset reproduces every pre-existing caller's margin
    // exactly (pctW(3) each side) — this property must be a pure opt-in.
    function test_active_label_default_side_inset_is_unchanged(): void {
        compare(activeLabel.sideInset, 100);
        const defaultLabel = defaultInsetComponent.createObject(testCase, {
            width: 300,
            height: 40
        });
        compare(defaultLabel.sideInset, Sizing.pctW(3));
        defaultLabel.destroy();
    }

    Component {
        id: defaultInsetComponent
        ActiveLabel {}
    }
}
