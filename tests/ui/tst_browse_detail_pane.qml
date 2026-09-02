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

    // Reference metrics for the tag row height floor test below — same
    // font/size the pane's own internal `tagFontMetrics` measures.
    FontMetrics {
        id: tagRowRefMetrics
        font.family: Theme.fontUi
        font.pixelSize: Sizing.fontSmall
    }

    // Auxiliary panes created per-test to exercise non-default property
    // combinations. Stored here so cleanup() can destroy them even when a
    // compare() or verify() aborts the test function early.
    property var _helperPane: null

    Component {
        id: noTitlePaneComp
        BrowseDetailPane {
            width: 320
            height: 240
            showTitle: false
        }
    }

    Component {
        id: reserveNavPaneComp
        BrowseDetailPane {
            width: 320
            height: 240
            showTitle: false
            reserveImageNav: true
        }
    }

    function resetPane(): void {
        pane.loading = false;
        pane.detailSuppressed = false;
        pane.loadingDelayMs = 150;
        pane.title = "";
        pane.detailTags = "";
        pane.identity = "";
        pane.coverKey = "";
        pane._lastGoodCoverSource = "";
        pane._lastGoodCoverIdentity = "";
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

        // Title and tags are visible immediately, even while the cover and
        // pane are both still resolving.
        verify(findChild(pane, "detailTitleText").visible);
        verify(findChild(pane, "detailTagTable").visible);
        // Round 11: no hourglass overlay -- the cover slot stays blank while
        // pending, matching how the grid tiles handle the same sentinel
        // (Tile.qml's `_coverPending` swallows it to an empty source).
        verify(!findChild(pane, "detailPlaceholderIcon").visible, "no hourglass placeholder while a cover fetch is pending");
        verify(!findChild(pane, "detailLoadingIndicator").visible);
    }

    // Round 11: the `icons/Loading` sentinel must never paint a placeholder
    // -- the slot stays blank (coverHold showing any prior art, or nothing)
    // for as long as the fetch is pending, with no grace-delay flip to an
    // hourglass the way round 9/10 handled it.
    function test_cover_stays_blank_while_pending_no_hourglass(): void {
        pane.coverKey = "icons/Loading";
        wait(pane.loadingDelayMs + 50);
        verify(!findChild(pane, "detailPlaceholderIcon").visible, "no hourglass while pending, even after a delay");
        verify(!findChild(pane, "detailCoverImage").visible, "no cover image paints from an empty pending source");
    }

    function test_held_cover_never_crosses_focused_identity(): void {
        pane.identity = "Arcade\nold-game";
        pane._lastGoodCoverIdentity = pane.identity;
        pane._lastGoodCoverSource = "image://media-image/old-cover";
        pane.coverKey = "icons/Loading";
        compare(String(pane._coverSource), "image://media-image/old-cover");

        pane.identity = "Arcade\nnew-game";
        compare(String(pane._coverSource), "", "new row must not inherit previous row cover");
        verify(!findChild(pane, "detailCoverHold").visible, "previous row cover hold must stop immediately");
        verify(!findChild(pane, "detailPlaceholderIcon").visible, "pending row remains blank rather than flashing stale art or placeholder");
    }

    function test_current_held_cover_is_not_overpainted_by_placeholder(): void {
        pane.identity = "Arcade\nsame-game";
        pane._lastGoodCoverIdentity = pane.identity;
        pane._lastGoodCoverSource = "image://media-image/held-cover";
        pane.coverKey = "";
        verify(!findChild(pane, "detailPlaceholderIcon").visible);
    }

    // A cover key that resolves to a terminal decode error (the harness's
    // media-image provider isn't registered, so any media-image/ key errors
    // out) is a *confirmed* no-cover state, distinct from merely pending --
    // the File chip is correct to show here, same as before round 11.
    function test_confirmed_decode_error_still_shows_the_file_chip(): void {
        pane.coverKey = "media-image/not-ready";
        tryVerify(() => findChild(pane, "detailPlaceholderIcon").visible, 500);
    }

    // Round 11: only fetched raster art (media-image/, custom-image/) is
    // eligible for the reveal fade -- bundled glyphs and system logos show
    // instantly. This is a pure classification check, independent of any
    // Image reaching Ready.
    function test_cover_is_real_art_classifies_media_and_custom_image_keys(): void {
        pane.coverKey = "media-image/SNES/some-game";
        compare(pane._coverIsRealArt, true);
        pane.coverKey = "custom-image/some/path.png";
        compare(pane._coverIsRealArt, true);
        pane.coverKey = "icons/File";
        compare(pane._coverIsRealArt, false);
        pane.coverKey = "systems/NES";
        compare(pane._coverIsRealArt, false);
    }

    // A bundled glyph key resolves through the registered tinted-svg
    // provider (see this file's header comment), so it can reach Ready in
    // this harness -- confirms `updateReveal()` leaves non-real-art at full
    // opacity rather than fading it in.
    function test_bundled_glyph_cover_shows_instantly_without_a_fade(): void {
        pane.coverKey = "icons/File";
        const img = findChild(pane, "detailCoverImage");
        tryVerify(() => img.status === Image.Ready, 500);
        compare(img.revealOpacity, 1, "bundled glyph keys must not fade -- they show at full opacity as soon as ready");
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

    // Regression (stale-metadata flash): the detail table tracks the focused
    // row's live tags directly, never the previous row's values. The model
    // keeps current_detail_tags identity-correct on every move (an immediate
    // peek shows cached/local rows or a clean blank), so the moment the live
    // tags go value-less the table must reflect that — not hold the prior
    // item's metadata.
    function test_metadata_tracks_live_tags(): void {
        // One item with real values.
        pane.loading = false;
        pane.detailTags = "Year\t1990\nGenre\tAction";
        wait(1);
        verify(pane._displayRows.length > 0, "real rows must be shown");
        verify((pane._displayRows[0].value ?? "") !== "", "real row value must be present");

        // Move to a value-less item while a fetch is pending: the table must
        // show the blank live rows immediately, never the previous 1990/Action.
        pane.loading = true;
        pane.detailTags = "Year\t\nGenre\t\nPlayers\t\nDeveloper\t\nPublisher\t\nRating\t";
        wait(1);
        verify(pane._displayRows[0].value === "" || pane._displayRows[0].value === undefined, "blank live rows must be shown at once, never the previous item's values");

        // A genuinely metadata-less item (empty tags) shows no table at all.
        pane.detailTags = "";
        pane.loading = false;
        wait(1);
        verify(!findChild(pane, "detailTagTable").visible, "tag table must be hidden when the item has no metadata");
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

    // Round 11 regression (reported as "the g in Rating gets cut off"): a
    // tag row's height must never be shorter than the label/value text's
    // own real line height, or the last row's descender crosses
    // `detailTagTable`'s clip boundary. The per-theme profile value alone
    // used to win even when it was a few px short of Noto Sans's real
    // ascent+descent at this font size.
    function test_tag_row_height_covers_the_full_font_metrics_line(): void {
        verify(pane._tagRowHeight >= Math.ceil(tagRowRefMetrics.height), "row height (" + pane._tagRowHeight + ") must cover the text's real line height (" + Math.ceil(tagRowRefMetrics.height) + ")");
    }

    // Round 11 regression: `_compactMetadataHeight` used to be a flat 38%
    // of the pane's *total* height, disconnected from how much space the
    // metadata slot actually has below the cover -- letting `detailBody`
    // size itself taller than `metadataSlot` (its own clipping ancestor)
    // and clipping the last row. It must now never exceed the slot's real
    // height, however many rows are asked to fit.
    function test_compact_metadata_height_never_exceeds_the_real_metadata_slot(): void {
        pane.title = "";
        pane.detailTags = "Year\t1990\nGenre\tAction\nPlayers\t2\nDeveloper\tX\nPublisher\tY\nRating\tE";
        wait(1);
        const table = findChild(pane, "detailTagTable");
        verify(table !== null);
        // detailTagTable -> detailBody -> metadataInner -> metadataSlot.
        const metadataSlotHeight = table.parent.parent.parent.height;
        verify(pane._compactMetadataHeight <= metadataSlotHeight, "compact metadata height (" + pane._compactMetadataHeight + ") must not exceed the real metadata slot (" + metadataSlotHeight + ")");
    }

    // The pane and the details modal render the same tag types, so they
    // must resolve to the same strings. The vocabulary used to be a
    // private ladder in this file, which is why the modal shipped its own
    // untranslated labels for the same data.
    function test_tag_labels_come_from_the_shared_vocabulary(): void {
        pane.detailTags = "Year\t1994\nrelease_date\t1994-03-19\ncheevos\tyes";
        const rows = pane._detailRows;
        compare(rows.length, 3);
        compare(rows[0].measureLabel, Format.metadataLabel("year"), "the legacy English label must resolve to the shared label");
        compare(rows[1].measureLabel, Format.metadataLabel("release_date"), "the canonical type must resolve to the same label");
        compare(rows[2].measureLabel, "Cheevos", "an unknown passthrough type still reads as a label");
        pane.detailTags = "";
    }

    // U+009C is Qt's alternative-text separator: an eliding Text renders
    // the short form rather than truncating the long one. Types with no
    // abbreviation must pack nothing so they elide normally.
    function test_short_labels_are_packed_for_narrow_columns(): void {
        pane.detailTags = "Year\t1994\ncheevos\tyes";
        const rows = pane._detailRows;
        compare(rows[0].label, Format.metadataLabel("year") + "\u009C" + Format.metadataShortLabel("year"));
        verify(Format.metadataShortLabel("year").length > 0);
        compare(rows[1].label, "Cheevos", "a type with no short form packs nothing");
        compare(Format.metadataShortLabel("cheevos"), "");
        pane.detailTags = "";
    }

    // The column used to be a session-long `Math.max` with no reset, so
    // browsing a systems table (which carries `Manufacturer`) and then a
    // media table left the narrower table paying for a width no label in
    // it needs. It now tracks the rows actually on screen.
    function test_label_column_shrinks_back_for_a_narrower_table(): void {
        pane.detailTags = "Manufacturer\tNintendo";
        const wide = pane._labelColumnNaturalWidth;
        verify(wide > 0);
        pane.detailTags = "Yr\t1994";
        const narrow = pane._labelColumnNaturalWidth;
        verify(narrow > 0);
        verify(narrow < wide, "a narrower label set (" + narrow + ") must not keep the wider column (" + wide + ")");
        pane.detailTags = "";
        compare(pane._labelColumnNaturalWidth, 0, "no rows means no column");
    }
}
