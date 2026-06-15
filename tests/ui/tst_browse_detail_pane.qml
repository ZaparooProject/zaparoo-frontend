// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
// tryVerify() lambdas, Qt.createQmlObject() return type, try/finally blocks,
// and var-typed property accesses are all structural to QuickTest patterns and
// cannot be statically typed. Suppress the compiler category file-wide.
// qmllint disable compiler

import QtQuick
import QtTest
import Zaparoo.Theme
import Zaparoo.Ui

TestCase {
    id: testCase
    name: "BrowseDetailPane"
    when: windowShown
    width: 320
    height: 240
    visible: true

    Component.onCompleted: {
        Sizing.screenWidth = testCase.width;
        Sizing.screenHeight = testCase.height;
    }

    BrowseDetailPane {
        id: pane

        width: 320
        height: 240
        loadingDelayMs: 150
        showTitle: true
    }

    // Auxiliary panes created per-test to exercise non-default property
    // combinations. Stored here so cleanup() can destroy them even when a
    // compare() or verify() aborts the test function early.
    property var _helperPane: null

    Component {
        id: noTitlePaneComp
        BrowseDetailPane { width: 320; height: 240; showTitle: false }
    }

    Component {
        id: reserveNavPaneComp
        BrowseDetailPane { width: 320; height: 240; showTitle: false; reserveImageNav: true }
    }

    function resetPane(): void {
        pane.loading = false;
        pane.detailSuppressed = false;
        pane.loadingDelayMs = 150;
        pane.title = "";
        pane.detailTags = "";
        pane.coverKey = "";
        pane._lastGoodCoverSource = "";
        pane._heldDetailRows = [];
        wait(1);
    }

    function init(): void {
        resetPane();
    }

    function cleanup(): void {
        if (testCase._helperPane !== null) {
            testCase._helperPane.destroy();
            testCase._helperPane = null;
        }
        resetPane();
    }

    function test_metadata_stays_visible_while_loading(): void {
        pane.title = "Selected Game";
        pane.detailTags = "Year\t1990\nGenre\tAction";
        pane.coverKey = "icons/Loading";
        pane.loading = true;
        wait(1);

        // Title and tags are visible immediately.
        verify(findChild(pane, "detailTitleText").visible);
        verify(findChild(pane, "detailTagTable").visible);
        // The loading placeholder is gated behind the grace delay so it does
        // not flash on fast warm navigations. Wait for the timer to fire.
        tryVerify(() => findChild(pane, "detailPlaceholderIcon").visible, pane.loadingDelayMs + 50);
        verify(!findChild(pane, "detailLoadingIndicator").visible);
    }

    function test_loading_icon_survives_media_image_handoff(): void {
        // The icons/Loading placeholder is gated behind the grace delay; wait
        // for it rather than checking immediately after the key flip.
        pane.coverKey = "icons/Loading";
        tryVerify(() => findChild(pane, "detailPlaceholderIcon").visible, pane.loadingDelayMs + 50);

        // Transitioning to a media-image key also gates the placeholder behind
        // the grace delay (every busy signal is now debounced uniformly).
        pane.coverKey = "media-image/not-ready";
        tryVerify(() => findChild(pane, "detailPlaceholderIcon").visible, pane.loadingDelayMs + 50);
    }

    // Regression: when showTitle is false (Games / Recents / Favorites), the
    // image slot must be the same height whether detailTags is empty (on
    // arrival) or fully populated (after metadata loads ~220 ms later). The
    // old metadata-driven height caused a fill-then-shrink reflow on every
    // d-pad move.
    function test_image_slot_stable_without_title(): void {
        // Need a layout profile with imageShare so primarySpan is usable.
        // Without a layoutProfile the content.height == pane.height and
        // primarySpan defaults to full height (shareTotal = 1, imageShare =
        // 1 -> primarySpan = height). That's deterministic, so the equality
        // check is still valid: both with and without tags the slot is the
        // same value.
        const paneNoTitle = noTitlePaneComp.createObject(testCase);
        testCase._helperPane = paneNoTitle;
        paneNoTitle.coverKey = "";
        paneNoTitle.detailTags = "";
        wait(1);
        const slot = findChild(paneNoTitle, "detailCoverImage");
        // Measure the cover image slot indirectly via detailCoverImage parent.
        const slotEmpty = slot !== null ? slot.parent.height : -1;

        paneNoTitle.detailTags = "Year\t1990\nGenre\tAction";
        wait(1);
        const slotFull = slot !== null ? slot.parent.height : -2;

        compare(slotEmpty, slotFull, "imageSlot height must not change when tags load (showTitle:false)");
        testCase._helperPane = null;
        paneNoTitle.destroy();
    }

    // Regression: when reserveImageNav is true (GamesScreen) the cover footprint
    // must not change when canNextImage flips from false to true. The gutter is
    // reserved up front so no reflow occurs when carousel metadata loads async.
    function test_image_slot_stable_with_reserve_nav(): void {
        const paneNav = reserveNavPaneComp.createObject(testCase);
        testCase._helperPane = paneNav;
        paneNav.canNextImage = false;
        wait(1);
        const slot = findChild(paneNav, "detailCoverImage");
        const widthBefore = slot !== null ? slot.parent.width : -1;

        paneNav.canNextImage = true;
        wait(1);
        const widthAfter = slot !== null ? slot.parent.width : -2;

        compare(widthBefore, widthAfter, "imageSlot width must not change when canNextImage flips (reserveImageNav:true)");
        testCase._helperPane = null;
        paneNav.destroy();
    }

    // Regression: when meta is still loading (loading: true) and the live tags
    // are value-less, the table must display the last-good (held) rows, not the
    // blank ones. Once loading settles (loading: false), the live rows win even
    // if they are empty, so a genuinely metadata-less item shows blank.
    function test_metadata_hold_while_loading(): void {
        // Prime the hold with real values.
        pane.loading = false;
        pane.detailTags = "Year\t1990\nGenre\tAction";
        wait(1);
        // Move to a new item: live tags are the blank synchronous set; loading is true.
        pane.loading = true;
        pane.detailTags = "Year\t\nGenre\t\nPlayers\t\nDeveloper\t\nPublisher\t\nRating\t";
        wait(1);
        const table = findChild(pane, "detailTagTable");
        verify(table.visible, "tag table must remain visible while held rows exist and loading");
        // The held rows carry Year=1990; confirm the held data is being used.
        // We access _displayRows via the model that drives the Repeater.
        verify(pane._displayRows.length > 0, "displayRows must be the held rows, not empty");
        verify((pane._displayRows[0].value ?? "") !== "", "held row value must be non-empty while loading with blank live tags");

        // Once loading settles, the live (blank) rows replace the held ones.
        pane.loading = false;
        wait(1);
        verify(pane._displayRows[0].value === "" || pane._displayRows[0].value === undefined, "after loading, live (blank) rows must be shown");
    }

    function test_cover_hold_hidden_with_no_prior_cover(): void {
        // coverHold must exist in the tree for the hold mechanic to work.
        const hold = findChild(pane, "detailCoverHold");
        verify(hold !== null, "detailCoverHold child must exist");
        // With no prior decoded cover (_lastGoodCoverSource == ""), the hold
        // stays hidden so the slot does not show a stale image on first load.
        // resetPane() already cleared _lastGoodCoverSource via init().
        verify(!hold.visible, "coverHold must be hidden when _lastGoodCoverSource is empty");
    }

    function test_suppressed_detail_still_hides_metadata(): void {
        pane.title = "Selected Game";
        pane.detailTags = "Year\t1990";
        pane.detailSuppressed = true;
        wait(1);

        verify(!findChild(pane, "detailTitleText").visible);
        verify(!findChild(pane, "detailTagTable").visible);
    }

    // Regression: during fast scroll (detailSuppressed=true) no placeholder chip
    // should appear in the cover slot. The sidebar must be fully blank so only
    // the card's own surfaceCard background shows through.
    function test_suppressed_hides_cover_chip(): void {
        pane.coverKey = "icons/File";
        pane.loadingDelayMs = 0;
        pane.detailSuppressed = true;
        wait(1);

        verify(!findChild(pane, "detailPlaceholderIcon").visible, "chip must be hidden during suppression");
        verify(!findChild(pane, "detailCoverImage").visible, "cover image must be hidden during suppression");
    }

    // When a system key has no matching SVG (the tinted-svg provider returns
    // an error), the wordmark should show and the generic File chip should not.
    function test_wordmark_shown_for_system_without_logo(): void {
        pane.title = "Foo";
        pane.coverKey = "systems/__no_logo__";
        // Wait for the async load to settle to Error; 500 ms is generous.
        tryVerify(() => findChild(pane, "detailLogoWordmark").visible, 500);
        verify(!findChild(pane, "detailPlaceholderIcon").visible);
    }

    // Regression: the cover Image must request the correct URL from the
    // provider as soon as coverKey changes. The QuickTest harness does not
    // register the live image providers, so Image.status never reaches Ready
    // here — this test only asserts the source binding is wired, not the
    // painted result. The manual check (user-driven) is the real gate for
    // actual paint.
    function test_cover_image_source_tracks_cover_key(): void {
        pane.coverKey = "media-image/SNES/some-game";
        wait(1);
        const img = findChild(pane, "detailCoverImage");
        verify(img !== null, "detailCoverImage child must exist");
        verify(img.source.toString().indexOf("image://media-image/") >= 0, "source should be a media-image provider URL, got: " + img.source);

        // Switching to a chip key should immediately clear the media-image URL.
        pane.coverKey = "icons/File";
        wait(1);
        verify(img.source.toString().indexOf("image://media-image/") < 0, "source should no longer be a media-image URL after switching to chip key");
    }
}
