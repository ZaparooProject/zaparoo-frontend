// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

import QtQuick
import QtTest
import Zaparoo.Theme
import Zaparoo.Ui

// Direct moveSelection coverage. PagedGrid wraps in surprising ways
// (within-row Left/Right wrap, vertical page advance/retreat, partial
// last-page hole clamps), so each branch needs its own explicit case.
//
// Test geometry pinned to 1280×480 with an explicit 4×3 grid
// (pageSize=12). The production browse screens now choose rows/columns
// from viewport-aware sizing, so the test pins its shape directly and
// keeps the navigation assertions stable.
TestCase {
    id: testCase
    name: "UiPagedGrid"
    when: windowShown
    width: 1280
    height: 480
    visible: true

    Component.onCompleted: {
        Sizing.screenWidth = testCase.width;
        Sizing.screenHeight = testCase.height;
    }

    ListModel {
        id: model
    }

    Component {
        id: cellDelegate
        Item {
            property string name: ""
            property string coverKey: ""
            property bool isSelected: false
            property bool isFocused: false
            property int favorite: 0
        }
    }

    PagedGrid {
        id: grid
        anchors.fill: parent
        model: model
        delegate: cellDelegate
        columnsOverride: 4
        rowsOverride: 3
    }

    SignalSpy {
        id: loadMoreSpy
        target: grid
        signalName: "loadMoreRequested"
    }

    property int suspendLiveDelegates: 0

    ListModel {
        id: suspendModel
        ListElement {
            name: "a"
            coverKey: ""
            favorite: 0
            hidden: false
            disambiguatingTags: ""
            entryType: "media"
            fileCount: 0
            disabled: false
            isEmpty: false
        }
        ListElement {
            name: "b"
            coverKey: ""
            favorite: 0
            hidden: false
            disambiguatingTags: ""
            entryType: "media"
            fileCount: 0
            disabled: false
            isEmpty: false
        }
        ListElement {
            name: "c"
            coverKey: ""
            favorite: 0
            hidden: false
            disambiguatingTags: ""
            entryType: "media"
            fileCount: 0
            disabled: false
            isEmpty: false
        }
    }

    ListModel {
        id: suspendReplacementModel
        ListElement {
            name: "replacement"
            coverKey: ""
            favorite: 0
            hidden: false
            disambiguatingTags: ""
            entryType: "media"
            fileCount: 0
            disabled: false
            isEmpty: false
        }
    }

    Component {
        id: suspendDelegate
        Item {
            property string name: ""
            property string coverKey: ""
            property bool isSelected: false
            property bool isFocused: false
            property int favorite: 0
            property bool hidden: false
            property string disambiguatingTags: ""
            property bool isEmpty: false
            Component.onCompleted: testCase.suspendLiveDelegates++
            Component.onDestruction: testCase.suspendLiveDelegates--
        }
    }

    PagedGrid {
        id: suspendProbe
        suspendDelegates: true
        model: suspendModel
        delegate: suspendDelegate
        columnsOverride: 3
        rowsOverride: 3
        width: 300
        height: 300
    }

    ListModel {
        id: coverSyncModel
        ListElement {
            name: "p0a"
            coverKey: ""
            favorite: 0
            hidden: false
            disambiguatingTags: ""
            entryType: "media"
            fileCount: 0
            disabled: false
            isEmpty: false
        }
        ListElement {
            name: "p0b"
            coverKey: ""
            favorite: 0
            hidden: false
            disambiguatingTags: ""
            entryType: "media"
            fileCount: 0
            disabled: false
            isEmpty: false
        }
        ListElement {
            name: "p1a"
            coverKey: ""
            favorite: 0
            hidden: false
            disambiguatingTags: ""
            entryType: "media"
            fileCount: 0
            disabled: false
            isEmpty: false
        }
        ListElement {
            name: "p1b"
            coverKey: ""
            favorite: 0
            hidden: false
            disambiguatingTags: ""
            entryType: "media"
            fileCount: 0
            disabled: false
            isEmpty: false
        }
    }

    // Reads the synchronous-decode flag back off its TileLoader host so the
    // per-cell rule can be asserted without instantiating a real Tile.
    Component {
        id: coverSyncDelegate
        Item {
            property string name: ""
            property string coverKey: ""
            property bool isSelected: false
            property bool isFocused: false
            property int favorite: 0
            // A Loader does not forward its own properties onto the loaded
            // item, so the delegate reads the contract off `parent` — same as
            // Tile.qml does.
            // qmllint disable missing-property
            readonly property bool observedCoverSynchronous: parent.coverSynchronous
            objectName: "coverSyncCell-" + parent.name
            // qmllint enable missing-property
        }
    }

    // One page of two cells, so page 0 holds p0a/p0b and page 1 holds p1a/p1b
    // while both stay inside the retention window.
    PagedGrid {
        id: coverSyncProbe
        model: coverSyncModel
        delegate: coverSyncDelegate
        columnsOverride: 2
        rowsOverride: 1
        width: 300
        height: 150
    }

    // `emptyDelegate` coverage (round 6 follow-up — see PagedGrid.qml's
    // `emptyDelegate`/HubScreen.qml's `_padToPageSize`). Two real rows plus
    // one `isEmpty: true` row on a 3-column/1-row page, so all three are on
    // the same page and reachable without paging. objectName carries the
    // row's `name` (read off `parent`, same pattern as `coverSyncDelegate`
    // above) so a specific row's loaded component is checkable — the
    // delegate Component itself is loaded via `Loader.sourceComponent`,
    // which does not forward `index`, only the properties PagedGrid/
    // TileLoader explicitly set.
    ListModel {
        id: emptyAwareModel
        ListElement {
            name: "real-0"
            coverKey: ""
            favorite: 0
            hidden: false
            disambiguatingTags: ""
            entryType: "media"
            fileCount: 0
            disabled: false
            isEmpty: false
        }
        ListElement {
            name: "real-1"
            coverKey: ""
            favorite: 0
            hidden: false
            disambiguatingTags: ""
            entryType: "media"
            fileCount: 0
            disabled: false
            isEmpty: false
        }
        ListElement {
            name: "pad-2"
            coverKey: ""
            favorite: 0
            hidden: false
            disambiguatingTags: ""
            entryType: "media"
            fileCount: 0
            disabled: false
            isEmpty: true
        }
    }

    Component {
        id: emptyAwareRealDelegate
        Item {
            // qmllint disable missing-property
            objectName: "emptyAwareRealCell-" + parent.name
            // qmllint enable missing-property
        }
    }

    Component {
        id: emptyAwareEmptyDelegate
        Item {
            // qmllint disable missing-property
            objectName: "emptyAwareEmptyCell-" + parent.name
            // qmllint enable missing-property
        }
    }

    PagedGrid {
        id: emptyAwareProbe
        model: emptyAwareModel
        delegate: emptyAwareRealDelegate
        emptyDelegate: emptyAwareEmptyDelegate
        columnsOverride: 3
        rowsOverride: 1
        width: 300
        height: 100
    }

    // Same model and real delegate, but `emptyDelegate` left at its default
    // `null` — every existing PagedGrid caller's shape, must render
    // byte-identically to a grid that has never heard of `isEmpty`.
    PagedGrid {
        id: emptyAwareDefaultProbe
        model: emptyAwareModel
        delegate: emptyAwareRealDelegate
        columnsOverride: 3
        rowsOverride: 1
        width: 300
        height: 100
    }

    // Fixture for `skipEmptyCells` coverage: 3 columns x 2 rows, 2 pages
    // (12 cells). Deliberately laid out so every skip scenario the
    // production Hub relies on has a cell to exercise it against:
    //   page 0        page 1
    //   real-a . real-b   .   real-c   .
    //     .     .    .    real-d  .    .
    // - col 0 has real-a (page0) and real-d (page1): a Down/Up skip must
    //   cross the page boundary to find the other one.
    // - col 2 has ONLY real-b (page0): every other cell in that column,
    //   on both pages, is empty -- Down/Up from real-b must cycle the
    //   whole column, find nothing else, and settle back exactly on
    //   real-b having moved nowhere.
    // - row 0 of page 0 has real-a AND real-b: a Right skip within a
    //   single row, no page crossing.
    ListModel {
        id: skipEmptyModel
        ListElement {
            name: "real-a"
            coverKey: ""
            favorite: 0
            hidden: false
            disambiguatingTags: ""
            entryType: "media"
            fileCount: 0
            disabled: false
            isEmpty: false
        }
        ListElement {
            name: "skip-1"
            coverKey: ""
            favorite: 0
            hidden: false
            disambiguatingTags: ""
            entryType: "media"
            fileCount: 0
            disabled: false
            isEmpty: true
        }
        ListElement {
            name: "real-b"
            coverKey: ""
            favorite: 0
            hidden: false
            disambiguatingTags: ""
            entryType: "media"
            fileCount: 0
            disabled: false
            isEmpty: false
        }
        ListElement {
            name: "skip-3"
            coverKey: ""
            favorite: 0
            hidden: false
            disambiguatingTags: ""
            entryType: "media"
            fileCount: 0
            disabled: false
            isEmpty: true
        }
        ListElement {
            name: "skip-4"
            coverKey: ""
            favorite: 0
            hidden: false
            disambiguatingTags: ""
            entryType: "media"
            fileCount: 0
            disabled: false
            isEmpty: true
        }
        ListElement {
            name: "skip-5"
            coverKey: ""
            favorite: 0
            hidden: false
            disambiguatingTags: ""
            entryType: "media"
            fileCount: 0
            disabled: false
            isEmpty: true
        }
        ListElement {
            name: "skip-6"
            coverKey: ""
            favorite: 0
            hidden: false
            disambiguatingTags: ""
            entryType: "media"
            fileCount: 0
            disabled: false
            isEmpty: true
        }
        ListElement {
            name: "real-c"
            coverKey: ""
            favorite: 0
            hidden: false
            disambiguatingTags: ""
            entryType: "media"
            fileCount: 0
            disabled: false
            isEmpty: false
        }
        ListElement {
            name: "skip-8"
            coverKey: ""
            favorite: 0
            hidden: false
            disambiguatingTags: ""
            entryType: "media"
            fileCount: 0
            disabled: false
            isEmpty: true
        }
        ListElement {
            name: "real-d"
            coverKey: ""
            favorite: 0
            hidden: false
            disambiguatingTags: ""
            entryType: "media"
            fileCount: 0
            disabled: false
            isEmpty: false
        }
        ListElement {
            name: "skip-10"
            coverKey: ""
            favorite: 0
            hidden: false
            disambiguatingTags: ""
            entryType: "media"
            fileCount: 0
            disabled: false
            isEmpty: true
        }
        ListElement {
            name: "skip-11"
            coverKey: ""
            favorite: 0
            hidden: false
            disambiguatingTags: ""
            entryType: "media"
            fileCount: 0
            disabled: false
            isEmpty: true
        }
    }

    PagedGrid {
        id: skipEmptyProbe
        model: skipEmptyModel
        delegate: emptyAwareRealDelegate
        emptyDelegate: emptyAwareEmptyDelegate
        columnsOverride: 3
        rowsOverride: 2
        width: 300
        height: 200
    }

    SignalSpy {
        id: skipEmptyHoveredSpy
        target: skipEmptyProbe
        signalName: "itemHovered"
    }

    // Dedicated fixture for the genuine "nothing else on the whole board"
    // edge case: `_nearestVerticalCandidate` searches every real tile, not
    // just a column, so a single stray real cell one column over is no
    // longer a dead end (see `skipEmptyModel` above) -- this needs a board
    // with truly only one real tile anywhere to exercise the "settle back,
    // report no movement" path.
    ListModel {
        id: singleRealCellModel
        ListElement {
            name: "only-real"
            coverKey: ""
            favorite: 0
            hidden: false
            disambiguatingTags: ""
            entryType: "media"
            fileCount: 0
            disabled: false
            isEmpty: false
        }
        ListElement {
            name: "skip-a"
            coverKey: ""
            favorite: 0
            hidden: false
            disambiguatingTags: ""
            entryType: "media"
            fileCount: 0
            disabled: false
            isEmpty: true
        }
        ListElement {
            name: "skip-b"
            coverKey: ""
            favorite: 0
            hidden: false
            disambiguatingTags: ""
            entryType: "media"
            fileCount: 0
            disabled: false
            isEmpty: true
        }
        ListElement {
            name: "skip-c"
            coverKey: ""
            favorite: 0
            hidden: false
            disambiguatingTags: ""
            entryType: "media"
            fileCount: 0
            disabled: false
            isEmpty: true
        }
    }

    PagedGrid {
        id: singleRealCellProbe
        model: singleRealCellModel
        delegate: emptyAwareRealDelegate
        emptyDelegate: emptyAwareEmptyDelegate
        skipEmptyCells: true
        columnsOverride: 2
        rowsOverride: 2
        width: 200
        height: 200
    }

    PagedGrid {
        id: geometryProbe
        model: model
        delegate: cellDelegate
        columnsOverride: 3
        rowsOverride: 2
        width: 307
        height: 293
        layoutProfile: ({
                "grid": {
                    "leftInset": 4,
                    "rightInset": 0,
                    "topInset": 3,
                    "bottomInset": 4,
                    "columnGap": 5,
                    "rowGap": 7
                },
                "surface": {
                    "cardRadius": 2,
                    "rowRadius": 1
                }
            })
    }

    // `squareCells`/`heightBudget` coverage. Deliberately zeroed insets and
    // gaps for round numbers; `height` is left absurdly large (9999) so a
    // test can prove `heightBudget` — not `height` — is what governs the
    // fit when it is set, which is the whole reason the property exists
    // (see PagedGrid.qml's `heightBudget` doc comment).
    PagedGrid {
        id: squareCellsProbe
        model: model
        delegate: cellDelegate
        columnsOverride: 5
        rowsOverride: 1
        width: 500
        height: 9999
        layoutProfile: ({
                "grid": {
                    "leftInset": 0,
                    "rightInset": 0,
                    "topInset": 0,
                    "bottomInset": 0,
                    "columnGap": 0,
                    "rowGap": 0
                }
            })
    }

    // Pagination-invariant coverage: with no gutter to reserve, cell
    // geometry must be identical whether the dataset is single-page or
    // heavily paginated. `totalItemsOverride` is what a caller like
    // HubScreen/GamesScreen flips to force pagination -- see the
    // `test_cell_geometry_is_identical_regardless_of_page_count` test
    // below, which exercises this probe.
    PagedGrid {
        id: paginationInvariantProbe
        model: model
        delegate: cellDelegate
        // 3 columns deliberately doesn't divide the 200px available width
        // evenly (floor(200/3) = 66, remainder 2) so centering produces a
        // genuinely nonzero offset -- a 4-column fixture here would floor
        // to exactly 0 remainder and prove nothing about the formula.
        columnsOverride: 3
        rowsOverride: 1
        width: 220
        height: 100
        layoutProfile: ({
                "grid": {
                    "leftInset": 10,
                    "rightInset": 10,
                    "topInset": 0,
                    "bottomInset": 0,
                    "columnGap": 0,
                    "rowGap": 0
                }
            })
    }

    function fillModel(count: int): void {
        model.clear();
        for (let i = 0; i < count; i++)
            model.append({
                "name": "item-" + i,
                "coverKey": "",
                "favorite": 0
            });
        // Wait for Repeater itemCount to catch up before any test assertions.
        tryCompare(grid, "itemCount", count);
    }

    function init(): void {
        Sizing.screenWidth = testCase.width;
        Sizing.screenHeight = testCase.height;
        // Reset paginated-state knobs so a leak from a failed test
        // (which skips its cleanup) doesn't poison the next case's
        // pageCount/totalPageCount math.
        grid.hasMorePages = false;
        grid.loadingMore = false;
        grid.paginationTotalKnown = true;
        grid.totalItemsOverride = -1;
        fillModel(0);
        grid.setCurrentIndexImmediate(0);
        loadMoreSpy.clear();
        skipEmptyProbe.skipEmptyCells = false;
        skipEmptyProbe.setCurrentIndexImmediate(0);
        skipEmptyHoveredSpy.clear();
    }

    function test_suspended_delegates_track_model_without_materializing(): void {
        compare(suspendProbe.itemCount, 3);
        compare(testCase.suspendLiveDelegates, 0);

        suspendModel.append({
            "name": "d",
            "coverKey": "",
            "favorite": 0,
            "hidden": false,
            "disambiguatingTags": "",
            "isEmpty": false
        });
        tryCompare(suspendProbe, "itemCount", 4);
        compare(testCase.suspendLiveDelegates, 0);

        suspendProbe.setCurrentIndexImmediate(2);
        suspendProbe.suspendDelegates = false;
        tryCompare(testCase, "suspendLiveDelegates", 4);
        compare(suspendProbe.currentIndex, 2);

        suspendProbe.suspendDelegates = true;
        tryCompare(testCase, "suspendLiveDelegates", 0);
        compare(suspendProbe.itemCount, 4);
        compare(suspendProbe.currentIndex, 2);

        suspendProbe.model = suspendReplacementModel;
        tryCompare(suspendProbe, "itemCount", 1);
        compare(testCase.suspendLiveDelegates, 0);

        suspendProbe.model = suspendModel;
        suspendModel.remove(3);
        tryCompare(suspendProbe, "itemCount", 3);
        suspendProbe.setCurrentIndexImmediate(0);
    }

    function test_geometry_matches_pinned_resolution(): void {
        compare(grid.columns, 4, "expected 4 columns at 480px height");
        compare(grid.rows, 3, "expected 3 rows at 480px height");
        compare(grid.pageSize, 12);
        compare(grid._coverRetentionPages, 2, "tile retention must convert cover count to pages");
    }

    function test_integer_cell_remainders_are_centered(): void {
        fillModel(7);
        geometryProbe.setCurrentIndexImmediate(0);

        const rightRemainder = geometryProbe._availableWidth - geometryProbe._cellBlockOffsetX - geometryProbe._contentWidth;
        const bottomRemainder = geometryProbe._availableHeight - geometryProbe._cellBlockOffsetY - geometryProbe._contentHeight;
        verify(Math.abs(geometryProbe._cellBlockOffsetX - rightRemainder) <= 1);
        verify(Math.abs(geometryProbe._cellBlockOffsetY - bottomRemainder) <= 1);

        const rect = geometryProbe.currentCellRectIn(geometryProbe);
        compare(rect.x, geometryProbe.leftInset + geometryProbe._cellBlockOffsetX);
        compare(rect.y, geometryProbe.topInset + geometryProbe._cellBlockOffsetY);
        compare(rect.width, geometryProbe.cellWidth);
        compare(rect.height, geometryProbe.cellHeight);
    }

    // squareCells clamps BOTH axes to the smaller of the two independent
    // fits, whichever axis is the binding constraint — not just the axis
    // that happens to be tighter in a specific scene.
    function test_square_cells_clamps_to_the_tighter_axis(): void {
        squareCellsProbe.squareCells = true;
        // Width is the tighter axis: 500/5 = 100 per column. heightBudget
        // alone would allow 300 (height, 9999, is deliberately absurd and
        // must be ignored once heightBudget is set).
        squareCellsProbe.heightBudget = 300;
        compare(squareCellsProbe.cellWidth, 100);
        compare(squareCellsProbe.cellHeight, 100, "height must clamp DOWN to match width, not stay at its own looser fit");

        // Height is now the tighter axis: 60/1 row = 60, less than the
        // 100px width fit.
        squareCellsProbe.heightBudget = 60;
        compare(squareCellsProbe.cellWidth, 60, "width must clamp DOWN to match height");
        compare(squareCellsProbe.cellHeight, 60);
    }

    // Both properties at their defaults (squareCells: false, heightBudget:
    // -1) must be byte-identical to the pre-Hub behavior: each axis fits
    // independently, against the item's own height.
    function test_square_cells_and_height_budget_defaults_are_unchanged(): void {
        squareCellsProbe.squareCells = false;
        squareCellsProbe.heightBudget = -1;
        compare(squareCellsProbe.cellWidth, 100);
        compare(squareCellsProbe.cellHeight, 9999, "with both at default, height fits against the item's own (absurd) height, unclamped by width");
    }

    // heightBudget: -1 reproduces the item's own `height` exactly even
    // with squareCells on — the escape hatch only matters once a caller
    // actually sets it.
    function test_height_budget_default_uses_own_height(): void {
        squareCellsProbe.squareCells = true;
        squareCellsProbe.heightBudget = -1;
        compare(squareCellsProbe.cellWidth, 100);
        compare(squareCellsProbe.cellHeight, 100, "with heightBudget unset, the fit uses the item's own (absurd) height, so width is the only real constraint");
    }

    // The cell block always centers against the full inset-to-inset
    // width now -- there is no gutter reservation left to bias it left of
    // true center. width 220, leftInset/rightInset 10 each -> the true
    // center region is x:10..210 (width 200), midpoint x=110.
    function test_cell_block_centers_against_full_inset_to_inset_width(): void {
        compare(paginationInvariantProbe._cellBlockOffsetX, 1);
        const midpoint = paginationInvariantProbe.leftInset + paginationInvariantProbe._cellBlockOffsetX + paginationInvariantProbe._contentWidth / 2;
        compare(midpoint, 110, "cell block must center against the full inset-to-inset width");
    }

    // Regression guard for the round that removed PagedGrid's own gutter
    // reservation: since arming Hub Move always adds a second page (see
    // HubScreen.qml's `_moveArmedTotalPages`), a grid that conditionally
    // reserved space for its own scroll indicator would visibly shift and
    // shrink its cells the instant Move armed. There is no in-grid
    // indicator anymore -- the page cue lives in the host screen's footer
    // (PageIndicator.qml) -- so cell geometry must be identical for a
    // single-page and a many-page dataset.
    function test_cell_geometry_is_identical_regardless_of_page_count(): void {
        compare(paginationInvariantProbe.totalPageCount, 1, "fixture starts single-page, or this test proves nothing");
        const singlePageOffsetX = paginationInvariantProbe._cellBlockOffsetX;
        const singlePageContentWidth = paginationInvariantProbe._contentWidth;

        paginationInvariantProbe.totalItemsOverride = 999;
        compare(paginationInvariantProbe.totalPageCount > 1, true, "fixture must now be multi-page, or this test proves nothing");
        compare(paginationInvariantProbe._cellBlockOffsetX, singlePageOffsetX, "cell block position must not shift once pagination becomes relevant");
        compare(paginationInvariantProbe._contentWidth, singlePageContentWidth, "cell size must not shrink either");

        paginationInvariantProbe.totalItemsOverride = -1;
    }

    function test_empty_model_refuses_movement(): void {
        compare(grid.itemCount, 0);
        compare(grid.moveSelection(1, 0), false);
        compare(grid.moveSelection(0, 1), false);
        compare(grid.currentIndex, 0);
    }

    function test_prepare_for_model_replacement_clears_pending_target(): void {
        fillModel(20);
        grid.totalItemsOverride = 100;
        grid.hasMorePages = true;
        grid.setCurrentIndexImmediate(13);
        compare(grid.jumpToIndex(50), false);
        compare(grid.hasPendingTarget, true);

        grid.prepareForModelReplacement();

        compare(grid.hasPendingTarget, false);
        compare(grid.currentIndex, 0);
    }

    function test_within_page_step_right(): void {
        fillModel(20);
        compare(grid.currentIndex, 0);
        compare(grid.moveSelection(1, 0), true);
        compare(grid.currentIndex, 1);
    }

    function test_within_page_step_down(): void {
        fillModel(20);
        compare(grid.moveSelection(0, 1), true);
        // (row 0, col 0) → (row 1, col 0) → index 4
        compare(grid.currentIndex, 4);
    }

    // ── Vertical paging (Up/Down crosses page boundaries) ───────────────

    function test_down_at_bottom_row_advances_to_next_page(): void {
        // 24 items, two full pages. From (page 0, row 2, col 0) = 8,
        // Down advances to (page 1, row 0, col 0) = 12.
        fillModel(24);
        grid.setCurrentIndexImmediate(8);
        compare(grid.moveSelection(0, 1), true);
        compare(grid.currentIndex, 12);
    }

    function test_up_at_top_row_retreats_to_previous_page(): void {
        // 24 items. From (page 1, row 0, col 0) = 12, Up retreats to
        // (page 0, row 2, col 0) = 8.
        fillModel(24);
        grid.setCurrentIndexImmediate(12);
        compare(grid.moveSelection(0, -1), true);
        compare(grid.currentIndex, 8);
    }

    function test_down_at_last_page_last_row_wraps_to_page_zero(): void {
        // 24 items. From (page 1, row 2, col 0) = 20, Down wraps to
        // (page 0, row 0, col 0) = 0.
        fillModel(24);
        grid.setCurrentIndexImmediate(20);
        compare(grid.moveSelection(0, 1), true);
        compare(grid.currentIndex, 0);
    }

    function test_up_at_page_zero_first_row_wraps_to_last_page(): void {
        // 24 items. From (page 0, row 0, col 0) = 0, Up wraps to
        // (page 1, row 2, col 0) = 20.
        fillModel(24);
        compare(grid.moveSelection(0, -1), true);
        compare(grid.currentIndex, 20);
    }

    function test_up_at_page_zero_wraps_to_partial_last_page_clamped(): void {
        // 20 items: page 1 has rows 0..1 (12..19). Up from index 0
        // would land on (page 1, row 2, col 0) = 20 — a hole. Clamp
        // to the last item on the partial last page (19).
        fillModel(20);
        compare(grid.moveSelection(0, -1), true);
        compare(grid.currentIndex, 19);
    }

    function test_down_overshoot_to_partial_page_clamps_to_last_existing(): void {
        // 13 items: page 1 has only index 12 (row 0, col 0). From
        // (page 0, row 2, col 3) = 11, Down would land on (page 1,
        // row 0, col 3) = 15 — a hole. Clamp to last item on the
        // partial page (12).
        fillModel(13);
        grid.setCurrentIndexImmediate(11);
        compare(grid.moveSelection(0, 1), true);
        compare(grid.currentIndex, 12);
    }

    function test_down_below_last_filled_row_on_partial_page_wraps_to_page_zero(): void {
        // 14 items: standing at (page 1, row 0, col 1) = 13 (the last
        // item; row 1 of this page is empty). Down advances off the
        // last filled row — same as overflowing the grid, so on the
        // last page it wraps to (page 0, row 0, same col) = 1.
        fillModel(14);
        grid.setCurrentIndexImmediate(13);
        compare(grid.moveSelection(0, 1), true);
        compare(grid.currentIndex, 1);
    }

    function test_up_from_partial_page_retreats_to_previous_page(): void {
        // 14 items: page 1 has indices 12, 13. From (page 1, row 0,
        // col 1) = 13, Up retreats to (page 0, row 2, col 1) = 9
        // (a real cell on the full prev page).
        fillModel(14);
        grid.setCurrentIndexImmediate(13);
        compare(grid.moveSelection(0, -1), true);
        compare(grid.currentIndex, 9);
    }

    // ── Single-page Up/Down (wraps within the page) ─────────────────────

    function test_single_page_up_wraps_to_last_row_same_page(): void {
        // 12 items, single full page. From (row 0, col 0) = 0,
        // Up wraps to (row 2, col 0) = 8.
        fillModel(12);
        compare(grid.pageCount, 1);
        compare(grid.moveSelection(0, -1), true);
        compare(grid.currentIndex, 8);
    }

    function test_single_page_down_at_partial_last_row_wraps_to_top(): void {
        // 6 items, single partial page. From (row 1, col 1) = 5, Down
        // steps below the last filled row, which on the only (=last)
        // page wraps to (row 0, same col) = 1. Mirrors the full-page
        // single-page Down-wrap so partial pages aren't a special case.
        fillModel(6);
        grid.setCurrentIndexImmediate(5);
        compare(grid.pageCount, 1);
        compare(grid.moveSelection(0, 1), true);
        compare(grid.currentIndex, 1);
    }

    // ── Horizontal within-row wrap (Left/Right never changes page) ──────

    function test_right_at_last_col_wraps_within_row(): void {
        // 24 items. From (page 0, row 0, col 3) = 3, Right wraps to
        // (page 0, row 0, col 0) = 0. No page change.
        fillModel(24);
        grid.setCurrentIndexImmediate(3);
        compare(grid.moveSelection(1, 0), true);
        compare(grid.currentIndex, 0);
    }

    function test_left_at_first_col_wraps_within_row(): void {
        // 24 items. From (page 0, row 0, col 0) = 0, Left wraps to
        // (page 0, row 0, col 3) = 3. No page change.
        fillModel(24);
        compare(grid.moveSelection(-1, 0), true);
        compare(grid.currentIndex, 3);
    }

    function test_right_on_partial_row_wraps_within_filled(): void {
        // 14 items: page 1 row 0 has (12, 13). From idx 13, Right
        // wraps within the row to col 0 = 12.
        fillModel(14);
        grid.setCurrentIndexImmediate(13);
        compare(grid.moveSelection(1, 0), true);
        compare(grid.currentIndex, 12);
    }

    function test_left_on_partial_row_wraps_within_filled(): void {
        // 14 items. From idx 12 (page 1, row 0, col 0), Left wraps
        // to last filled col on the row = idx 13.
        fillModel(14);
        grid.setCurrentIndexImmediate(12);
        compare(grid.moveSelection(-1, 0), true);
        compare(grid.currentIndex, 13);
    }

    function test_single_page_left_wrap_to_last_col_when_row_full(): void {
        // 6 items at 4-cols: row 0 is full (0..3), row 1 partial (4..5).
        // From idx 0, Left wraps to last col on full row 0 = idx 3.
        fillModel(6);
        compare(grid.pageCount, 1);
        compare(grid.moveSelection(-1, 0), true);
        compare(grid.currentIndex, 3);
    }

    function test_single_page_left_wrap_partial_row_clamps_to_last_item(): void {
        // 2 items at 4-cols: row 0 partial (0..1). From idx 0, Left
        // wraps to last filled col on this row (col 1) = idx 1.
        fillModel(2);
        compare(grid.pageCount, 1);
        compare(grid.moveSelection(-1, 0), true);
        compare(grid.currentIndex, 1);
    }

    function test_single_page_right_at_last_filled_wraps_to_row_start(): void {
        // 6 items. From (row 1, col 1) = 5, Right wraps within the
        // partial row to col 0 = idx 4.
        fillModel(6);
        grid.setCurrentIndexImmediate(5);
        compare(grid.moveSelection(1, 0), true);
        compare(grid.currentIndex, 4);
    }

    function test_no_movement_returns_false(): void {
        fillModel(20);
        compare(grid.moveSelection(0, 0), false);
        compare(grid.currentIndex, 0);
    }

    function test_item_count_reset_falls_back_to_first_item(): void {
        // Shrink the model directly (without an intermediate clear) so the
        // stale index path is exercised. A populated replacement always seats
        // focus on its first valid item rather than retaining an unrelated tail.
        fillModel(20);
        grid.setCurrentIndexImmediate(19);
        model.remove(10, 10);
        tryCompare(grid, "itemCount", 10);
        compare(grid.currentIndex, 0);
    }

    // ── pageBy (L/R shoulder shortcut, unchanged) ────────────────────────

    function test_pageBy_advances_one_page(): void {
        fillModel(24);
        grid.setCurrentIndexImmediate(2); // (row 0, col 2)
        compare(grid.pageBy(1), true);
        compare(grid.currentPage, 1);
        // Preserves (row, col): (page 1, row 0, col 2) = 14.
        compare(grid.currentIndex, 14);
    }

    function test_pageBy_wraps_negative(): void {
        fillModel(24);
        compare(grid.pageBy(-1), true);
        compare(grid.currentPage, 1);
    }

    function test_pageBy_single_page_returns_false(): void {
        fillModel(6);
        compare(grid.pageCount, 1);
        compare(grid.pageBy(1), false);
        compare(grid.pageBy(-1), false);
    }

    function test_pageBy_partial_target_clamps_to_last_item(): void {
        // 14 items, currentIndex 5 (row 1, col 1) on page 0. pageBy(1)
        // targets (page 1, row 1, col 1) = 17 — a hole. Clamps to
        // last on page 1 (13).
        fillModel(14);
        grid.setCurrentIndexImmediate(5);
        compare(grid.pageBy(1), true);
        compare(grid.currentIndex, 13);
    }

    // ── Page-stack flags (footer PageIndicator derivations) ──────────────

    function test_hasPages_flags_track_currentPage(): void {
        fillModel(36); // 3 pages
        grid.setCurrentIndexImmediate(0);
        compare(grid.hasPagesAbove, false);
        compare(grid.hasPagesBelow, true);
        grid.setCurrentIndexImmediate(12); // page 1
        compare(grid.hasPagesAbove, true);
        compare(grid.hasPagesBelow, true);
        grid.setCurrentIndexImmediate(24); // page 2
        compare(grid.hasPagesAbove, true);
        compare(grid.hasPagesBelow, false);
    }

    function test_hasPages_flags_single_page_dataset(): void {
        fillModel(6);
        compare(grid.pageCount, 1);
        compare(grid.hasPagesAbove, false);
        compare(grid.hasPagesBelow, false);
    }

    // Round 9: a *known*-total paginated model (Games) fetches exactly one
    // page's worth of rows on its first load -- `itemCount === pageSize`,
    // `paginationTotalKnown: true`, `hasMorePages: true`. The old formula's
    // known-total branch never looked at `hasMorePages` at all
    // (`currentPage(0) < pageCount(1) - 1` is false, and the OR term was
    // gated `!paginationTotalKnown &&`), so `hasPagesBelow` read false and
    // the footer's chevrons/"N / M" readout (PageIndicator._hasNavigationRange)
    // stayed hidden on entry despite the model plainly having more rows --
    // only the first d-pad move (which grows `itemCount` past a second
    // page) made them appear. A model reporting more rows must always
    // register as "a page below," known total or not.
    function test_has_pages_below_true_for_known_total_on_first_full_page(): void {
        fillModel(grid.pageSize);
        grid.hasMorePages = true;
        grid.paginationTotalKnown = true;
        compare(grid.pageCount, 1, "exactly one page's worth loaded so far");
        compare(grid.currentPage, 0);
        compare(grid.hasPagesBelow, true, "hasMorePages must register even on a known-total model's first page");
    }

    function test_unbounded_pages_keep_down_arrow_at_loaded_edge(): void {
        fillModel(24);
        grid.paginationTotalKnown = false;
        grid.hasMorePages = true;
        grid.setCurrentIndexImmediate(12);
        compare(grid.currentPage, grid.pageCount - 1);
        compare(grid.hasPagesAbove, true);
        compare(grid.hasPagesBelow, true);
    }

    function test_unbounded_page_next_fetches_instead_of_wrapping(): void {
        fillModel(24);
        grid.paginationTotalKnown = false;
        grid.hasMorePages = true;
        grid.setCurrentIndexImmediate(12);
        loadMoreSpy.clear();

        compare(grid.pageBy(1), false);
        compare(grid.currentIndex, 12);
        compare(grid._pendingTargetPage, 2);
        verify(loadMoreSpy.count >= 1);
    }

    function test_unbounded_page_zero_does_not_wrap_backward(): void {
        fillModel(24);
        grid.paginationTotalKnown = false;
        grid.hasMorePages = true;
        compare(grid.pageBy(-1), false);
        compare(grid.moveSelection(0, -1), false);
        compare(grid.currentIndex, 0);
        compare(grid.hasPendingTarget, false);
    }

    function test_unbounded_list_ignores_stale_total_hint(): void {
        fillModel(24);
        grid.totalItemsOverride = 5;
        grid.paginationTotalKnown = false;
        compare(grid.totalItems, 24);
        compare(grid.totalPageCount, 2);
    }

    function test_unbounded_pending_page_commits_after_append(): void {
        fillModel(24);
        // Real Core's deprecated media.search total equals current page size.
        // It must not clamp an unknown-length cursor chain.
        grid.totalItemsOverride = 5;
        grid.paginationTotalKnown = false;
        grid.hasMorePages = true;
        grid.setCurrentIndexImmediate(12);
        compare(grid.pageBy(1), false);
        compare(grid._pendingTargetPage, 2);

        for (let i = 24; i < 36; i++)
            model.append({
                "name": "item-" + i,
                "coverKey": "",
                "favorite": 0
            });
        tryCompare(grid, "itemCount", 36);
        compare(grid.currentIndex, 24);
        compare(grid._pendingTargetPage, -1);
    }

    function test_failed_unbounded_page_does_not_auto_retry(): void {
        fillModel(24);
        grid.paginationTotalKnown = false;
        grid.hasMorePages = true;
        grid.loadingMore = true;
        grid.setCurrentIndexImmediate(12);
        compare(grid.pageBy(1), false);
        compare(grid._pendingTargetPage, 2);

        // Models publish hasMorePages=false before loadingMore=false on a
        // failed cursor request. The first edge must settle pending navigation
        // so the completion edge cannot request the same cursor again.
        grid.hasMorePages = false;
        compare(grid._pendingTargetPage, -1);
        loadMoreSpy.clear();
        grid.loadingMore = false;
        compare(loadMoreSpy.count, 0);
    }

    // ── Footer PageIndicator's totalPageCount source (totalItemsOverride) ─

    function test_totalPageCount_uses_override(): void {
        // 24 items loaded — 2 pages on the 4×3 grid. With an override
        // saying total is 60 (5 pages), totalPageCount must reflect 5
        // so the footer's PageIndicator sizes from the dataset's true total
        // rather than the loaded slice.
        fillModel(24);
        compare(grid.pageCount, 2);
        grid.totalItemsOverride = 60;
        compare(grid.totalPageCount, 5);
        grid.totalItemsOverride = -1;
        compare(grid.totalPageCount, 2);
    }

    // ── Pending wrap-target (paginated dataset, partial load) ───────────
    //
    // GamesScreen sets `totalItemsOverride` to the dataset's true entry
    // count and `hasMorePages` to `GamesModel.has_next_page`. When the
    // user wraps onto an unloaded page, PagedGrid must:
    //   - stash the (page, row, col) target,
    //   - fire `loadMoreRequested` (the screen wires this to fetch_more),
    //   - leave `currentIndex` untouched,
    //   - commit the jump on the next `itemCount` growth that covers
    //     the target,
    //   - drop the pending intent on a sideways move,
    //   - settle on the loaded last item if `hasMorePages` flips false
    //     before the target is reached.
    //
    // Tests below set `totalItemsOverride = 60` (5 pages) but seed the
    // model with 24 items (2 pages loaded) to mimic the partial-load
    // state the user repro'd on Genesis "1 US - A-F".

    function _setupPartialLoad(loaded: int, total: int): void {
        fillModel(loaded);
        grid.totalItemsOverride = total;
        grid.hasMorePages = true;
        grid.setCurrentIndexImmediate(0);
        compare(grid.pageCount, Math.ceil(loaded / grid.pageSize));
        compare(grid.totalPageCount, Math.ceil(total / grid.pageSize));
    }

    function _resetPartialLoadState(): void {
        grid.totalItemsOverride = -1;
        grid.hasMorePages = false;
    }

    function test_up_at_page_zero_unloaded_target_stashes_pending(): void {
        // 24/60 — Up at index 0 targets the dataset's last page (page 4),
        // which isn't loaded. Selection must not move; pending state set.
        _setupPartialLoad(24, 60);
        loadMoreSpy.clear();
        compare(grid.moveSelection(0, -1), false);
        compare(grid.currentIndex, 0, "selection must not move while waiting for fetch");
        compare(grid._pendingTargetPage, 4, "pending target page should be the dataset's last");
        compare(grid._pendingTargetRow, grid.rows - 1);
        compare(grid._pendingTargetCol, 0);
        verify(loadMoreSpy.count >= 1, "expected loadMoreRequested to fire at least once");
        _resetPartialLoadState();
    }

    function test_pending_target_waits_for_active_append_before_next_fetch(): void {
        _setupPartialLoad(24, 60);
        compare(grid.moveSelection(0, -1), false);
        compare(grid._pendingTargetPage, 4);

        grid.loadingMore = true;
        loadMoreSpy.clear();
        for (let i = 24; i < 36; i++)
            model.append({
                "name": "item-" + i,
                "coverKey": "",
                "favorite": 0
            });
        tryCompare(grid, "itemCount", 36);
        compare(loadMoreSpy.count, 0, "active append tail must suppress next cursor request");

        grid.loadingMore = false;
        tryCompare(loadMoreSpy, "count", 1);
        compare(grid._pendingTargetPage, 4);
        _resetPartialLoadState();
    }

    function test_pending_target_commits_when_pages_load(): void {
        // Set up the partial-load wrap, then grow the model to cover
        // the target page. The itemCount-change handler must commit
        // the jump.
        _setupPartialLoad(24, 60);
        compare(grid.moveSelection(0, -1), false);
        compare(grid._pendingTargetPage, 4);
        // Append the rest of the dataset in one go — pageCount jumps
        // to 5, target page 4 is now loaded, jump commits.
        for (let i = 24; i < 60; i++)
            model.append({
                "name": "item-" + i,
                "coverKey": "",
                "favorite": 0
            });
        tryCompare(grid, "itemCount", 60);
        // Pending target row was rows-1 (=2), col 0; target index is
        // page 4 base (48) + row 2 * 4 + col 0 = 56.
        compare(grid.currentIndex, 56);
        compare(grid._pendingTargetPage, -1, "pending should clear after commit");
        _resetPartialLoadState();
    }

    function test_pending_target_clamps_when_target_partial_page(): void {
        // 24/50 — totalPageCount is 5 (last page partial: indices 48,49).
        // Up wraps to (page 4, row 2, col 0) = 56, which doesn't exist.
        // Selection waits while data loads; the model declares no more
        // pages once total is in, and the watchdog settles us on the
        // partial page's last existing item (49).
        fillModel(24);
        grid.totalItemsOverride = 50;
        grid.hasMorePages = true;
        grid.setCurrentIndexImmediate(0);
        compare(grid.totalPageCount, 5);
        compare(grid.moveSelection(0, -1), false);
        compare(grid._pendingTargetPage, 4);
        for (let i = 24; i < 50; i++)
            model.append({
                "name": "item-" + i,
                "coverKey": "",
                "favorite": 0
            });
        tryCompare(grid, "itemCount", 50);
        // Target slot (idx 56) doesn't exist on the partial last page,
        // so selection is still parked while we wait for "more". Real
        // models flip `has_next_page` false at the end of pagination;
        // the watchdog inside _commitPendingTarget then clamps to the
        // last loaded item on the target page.
        compare(grid.currentIndex, 0, "still pending — target slot 56 doesn't exist yet");
        grid.hasMorePages = false;
        compare(grid.currentIndex, 49, "clamp to last existing item on the partial page");
        _resetPartialLoadState();
    }

    function test_pending_target_chains_fetch_when_still_short(): void {
        // 24/60 — Up at index 0 stashes target page 4. Append only one
        // more page (12 items → 36 loaded, pageCount=3). Target still
        // unreached; the handler must fire another loadMoreRequested
        // and leave currentIndex untouched.
        _setupPartialLoad(24, 60);
        compare(grid.moveSelection(0, -1), false);
        loadMoreSpy.clear();
        for (let i = 24; i < 36; i++)
            model.append({
                "name": "item-" + i,
                "coverKey": "",
                "favorite": 0
            });
        tryCompare(grid, "itemCount", 36);
        compare(grid.currentIndex, 0, "still waiting on target page, selection unchanged");
        compare(grid._pendingTargetPage, 4);
        verify(loadMoreSpy.count >= 1, "expected another loadMoreRequested after partial append");
        _resetPartialLoadState();
    }

    function test_pending_target_settles_when_hasMorePages_clears(): void {
        // 24/60 — but the model later reports no more pages without
        // ever reaching pageCount=5. The watchdog branch in
        // _commitPendingTarget must settle on the loaded last item
        // (idx 23) so the user isn't stuck on the source cell.
        _setupPartialLoad(24, 60);
        compare(grid.moveSelection(0, -1), false);
        compare(grid._pendingTargetPage, 4);
        grid.hasMorePages = false;
        // No itemCount change is needed — the hasMorePages watcher
        // fires _commitPendingTarget itself.
        compare(grid._pendingTargetPage, -1, "pending should clear once hasMorePages goes false");
        compare(grid.currentIndex, grid.itemCount - 1, "settle on the loaded last item");
        _resetPartialLoadState();
    }

    function test_pending_target_cancels_on_horizontal_move(): void {
        // After stashing a pending wrap target, a sideways move means
        // the user changed intent — drop the pending jump.
        _setupPartialLoad(24, 60);
        grid.setCurrentIndexImmediate(1);
        compare(grid.moveSelection(0, -1), false);
        compare(grid._pendingTargetPage, 4);
        // Right step within row should clear pending.
        compare(grid.moveSelection(1, 0), true);
        compare(grid._pendingTargetPage, -1);
        _resetPartialLoadState();
    }

    function test_pending_target_clears_on_model_shrink(): void {
        // A model reset (system change, path change) shrinks itemCount.
        // The pending intent belongs to the previous dataset — drop it.
        _setupPartialLoad(24, 60);
        compare(grid.moveSelection(0, -1), false);
        compare(grid._pendingTargetPage, 4);
        model.clear();
        tryCompare(grid, "itemCount", 0);
        compare(grid._pendingTargetPage, -1);
        _resetPartialLoadState();
    }

    function test_down_past_last_loaded_stashes_pending(): void {
        // 24/60 — sit on the last filled row of the loaded slice
        // (page 1 row 2 col 0 = idx 20). Down would advance to page 2,
        // which isn't loaded. Stash, don't move.
        _setupPartialLoad(24, 60);
        grid.setCurrentIndexImmediate(20);
        loadMoreSpy.clear();
        compare(grid.moveSelection(0, 1), false);
        compare(grid.currentIndex, 20);
        compare(grid._pendingTargetPage, 2);
        compare(grid._pendingTargetRow, 0);
        compare(grid._pendingTargetCol, 0);
        verify(loadMoreSpy.count >= 1);
        _resetPartialLoadState();
    }

    function test_pageBy_past_loaded_stashes_pending(): void {
        // 24/60 — pageBy(2) targets page 2, unloaded. Stash, don't move.
        _setupPartialLoad(24, 60);
        grid.setCurrentIndexImmediate(2);
        compare(grid.pageBy(2), false);
        compare(grid.currentIndex, 2, "pageBy past loaded must not move synchronously");
        compare(grid._pendingTargetPage, 2);
        compare(grid._pendingTargetRow, 0);
        compare(grid._pendingTargetCol, 2);
        _resetPartialLoadState();
    }

    function test_loadAheadPages_default_is_two(): void {
        // The default `loadAheadPages` keeps two pages of buffer ahead
        // of the user before firing the prefetch. GamesScreen relies
        // on this default and overrides it only if the trade-off
        // changes.
        compare(grid.loadAheadPages, 2);
    }

    function test_loadAheadPages_two_fires_at_pageCount_minus_three(): void {
        // 60 items at pageSize 12 = 5 pages loaded. With
        // loadAheadPages=2, the trigger fires when `currentPage >=
        // pageCount - loadAheadPages - 1` = page 2. Start at idx 20
        // (page 1, row 2, col 0) and step Down: the move advances to
        // (page 2, row 0, col 0) = idx 24, which sits exactly on the
        // threshold — loadMoreRequested must fire so the next chunk is
        // in flight before the user reaches the loaded edge.
        fillModel(60);
        grid.hasMorePages = true;
        grid.setCurrentIndexImmediate(20);
        loadMoreSpy.clear();
        compare(grid.moveSelection(0, 1), true);
        compare(grid.currentPage, 2);
        verify(loadMoreSpy.count >= 1, "loadAheadPages=2 must fire prefetch on entering pageCount-3");
        grid.hasMorePages = false;
    }

    function test_loadAheadPages_two_does_not_fire_below_threshold(): void {
        // Same 60-item / 5-page setup. Step from idx 8 (page 0, row 2,
        // col 0) Down to idx 12 (page 1, row 0, col 0). currentPage=1
        // sits below the threshold (pageCount-3 = 2), so no prefetch
        // should fire yet.
        fillModel(60);
        grid.hasMorePages = true;
        grid.setCurrentIndexImmediate(8);
        loadMoreSpy.clear();
        compare(grid.moveSelection(0, 1), true);
        compare(grid.currentPage, 1);
        compare(loadMoreSpy.count, 0, "loadAheadPages=2 must not fire below pageCount-3 threshold");
        grid.hasMorePages = false;
    }

    function test_hasPendingTarget_tracks_pending_state(): void {
        // GamesScreen gates the "Loading more..." indicator on this
        // property so background prefetches stay silent. It must
        // mirror `_pendingTargetPage >= 0` exactly: false at rest,
        // true while a wrap/shoulder/hold-Down move is parked, false
        // once the target commits or the user changes intent.
        _setupPartialLoad(24, 60);
        compare(grid.hasPendingTarget, false, "no pending target at rest");
        compare(grid.moveSelection(0, -1), false);
        compare(grid._pendingTargetPage, 4);
        compare(grid.hasPendingTarget, true, "Up wrap to unloaded page must arm hasPendingTarget");
        for (let i = 24; i < 60; i++)
            model.append({
                "name": "item-" + i,
                "coverKey": "",
                "favorite": 0
            });
        tryCompare(grid, "itemCount", 60);
        compare(grid._pendingTargetPage, -1);
        compare(grid.hasPendingTarget, false, "commit must clear hasPendingTarget");
        _resetPartialLoadState();
    }

    // ── jumpToIndex (jump-to-letter position jump) ──────────────────────

    function test_jumpToIndex_loaded_target_lands_immediately(): void {
        // Fully loaded — a jump to any in-range index lands at once (the
        // backward-jump-to-already-loaded-letter case).
        fillModel(60);
        grid.setCurrentIndexImmediate(0);
        compare(grid.jumpToIndex(37), true);
        compare(grid.currentIndex, 37);
    }

    function test_jumpToIndex_clamps_to_last_item(): void {
        fillModel(20);
        compare(grid.jumpToIndex(999), true);
        compare(grid.currentIndex, 19);
    }

    function test_jumpToIndex_unloaded_stashes_absolute_index(): void {
        // 24/60 loaded. Jump to index 50 (unloaded): stash the ABSOLUTE
        // target (not a page/row/col decomposition), fire loadMore, leave
        // selection put. The page-wrap channel must stay clear so the two
        // can't be confused at commit time.
        _setupPartialLoad(24, 60);
        loadMoreSpy.clear();
        compare(grid.jumpToIndex(50), false);
        compare(grid.currentIndex, 0, "must not move while loading");
        compare(grid._pendingTargetIndex, 50, "stash the exact absolute target");
        compare(grid._pendingTargetPage, -1, "page-wrap channel stays clear for a jump");
        compare(grid.hasPendingJump, true);
        verify(loadMoreSpy.count >= 1, "expected loadMoreRequested to fire");
        _resetPartialLoadState();
    }

    function test_jumpToIndex_commits_exact_index_when_loaded(): void {
        // The pending jump commits on the exact absolute target index once
        // the data has loaded that far — never a page-aligned slot, never a
        // page early.
        _setupPartialLoad(24, 60);
        compare(grid.jumpToIndex(50), false);
        compare(grid._pendingTargetIndex, 50);
        for (let i = 24; i < 60; i++)
            model.append({
                "name": "item-" + i,
                "coverKey": "",
                "favorite": 0
            });
        tryCompare(grid, "itemCount", 60);
        compare(grid.currentIndex, 50, "lands on the exact target, mid-page");
        compare(grid._pendingTargetIndex, -1, "pending jump clears after commit");
        compare(grid.hasPendingJump, false);
        _resetPartialLoadState();
    }

    function test_jumpToIndex_commits_on_first_crossing_not_a_page_early(): void {
        // Grow the model one short of the target, then exactly across it.
        // The commit must wait until itemCount actually passes the target and
        // then land on the target itself — not settle a page (or any amount)
        // early while the intervening rows trickle in.
        _setupPartialLoad(24, 60);
        compare(grid.jumpToIndex(50), false);
        // Up to 50 rows loaded => target index 50 still not present
        // (indices 0..49). Must remain pending, selection unmoved.
        for (let i = 24; i < 50; i++)
            model.append({
                "name": "item-" + i,
                "coverKey": "",
                "favorite": 0
            });
        tryCompare(grid, "itemCount", 50);
        compare(grid.currentIndex, 0, "target index 50 not loaded yet — stay parked");
        compare(grid._pendingTargetIndex, 50);
        // One more row => index 50 exists; commit lands exactly there.
        model.append({
            "name": "item-50",
            "coverKey": "",
            "favorite": 0
        });
        tryCompare(grid, "itemCount", 51);
        compare(grid.currentIndex, 50);
        compare(grid._pendingTargetIndex, -1);
        _resetPartialLoadState();
    }

    function test_jumpToIndex_truncated_dataset_lands_on_nearest_loaded(): void {
        // Jump to 50, but the dataset turns out shorter: only 45 rows ever
        // arrive and the model declares no more pages. Settle on the nearest
        // loaded item (44), never a full page back to a page boundary.
        _setupPartialLoad(24, 60);
        compare(grid.jumpToIndex(50), false);
        compare(grid._pendingTargetIndex, 50);
        for (let i = 24; i < 45; i++)
            model.append({
                "name": "item-" + i,
                "coverKey": "",
                "favorite": 0
            });
        tryCompare(grid, "itemCount", 45);
        compare(grid.currentIndex, 0, "still pending — target 50 not loaded");
        grid.hasMorePages = false;
        compare(grid.currentIndex, 44, "settle on nearest loaded item, not a page early");
        compare(grid._pendingTargetIndex, -1);
        _resetPartialLoadState();
    }

    function test_jumpToIndex_pending_cleared_by_directional_move(): void {
        // A pending jump is the user's last expressed intent only until they
        // press a direction; a successful move must drop it.
        fillModel(60);
        grid.hasMorePages = true;
        grid.totalItemsOverride = 120;
        grid.setCurrentIndexImmediate(0);
        compare(grid.jumpToIndex(100), false);
        compare(grid.hasPendingJump, true);
        compare(grid.moveSelection(1, 0), true, "right step within row succeeds");
        compare(grid.hasPendingJump, false, "directional move clears the pending jump");
        compare(grid._pendingTargetIndex, -1);
        _resetPartialLoadState();
    }

    function test_hasPendingJump_false_for_page_wrap_target(): void {
        // A page-wrap (pageBy / vertical wrap) onto an unloaded page arms
        // hasPendingTarget but NOT hasPendingJump — only true letter jumps
        // bulk-load.
        _setupPartialLoad(24, 60);
        compare(grid.moveSelection(0, -1), false);
        compare(grid.hasPendingTarget, true, "page-wrap arms the generic pending flag");
        compare(grid.hasPendingJump, false, "page-wrap is not a jump");
        _resetPartialLoadState();
    }

    // Synchronous cover decoding is a GUI-thread cost, so PagedGrid spends it
    // only where it buys a visibly complete page. These three rules are what
    // keep it from turning into a stall; see Tile.qml's `delegateCoverSynchronous`.
    function test_cover_synchronous_only_on_the_current_page(): void {
        coverSyncProbe.setCurrentIndexImmediate(0);
        tryCompare(coverSyncProbe, "currentPage", 0);
        const onPage = findChild(coverSyncProbe, "coverSyncCell-p0a");
        const offPage = findChild(coverSyncProbe, "coverSyncCell-p1a");
        verify(onPage !== null);
        verify(offPage !== null, "the next page must stay materialized inside the retention window");
        compare(onPage.observedCoverSynchronous, true);
        compare(offPage.observedCoverSynchronous, false, "retention-edge warmup must not block the GUI thread");
    }

    function test_cover_synchronous_follows_the_current_page(): void {
        coverSyncProbe.setCurrentIndexImmediate(2);
        tryCompare(coverSyncProbe, "currentPage", 1);
        compare(findChild(coverSyncProbe, "coverSyncCell-p1a").observedCoverSynchronous, true);
        compare(findChild(coverSyncProbe, "coverSyncCell-p0a").observedCoverSynchronous, false);
        coverSyncProbe.setCurrentIndexImmediate(0);
    }

    function test_rapid_render_mode_never_decodes_synchronously(): void {
        coverSyncProbe.setCurrentIndexImmediate(0);
        coverSyncProbe.rapidRenderMode = true;
        // rapidRenderMode also deactivates the TileLoaders, so the assertion is
        // that nothing survives to request a synchronous decode.
        tryCompare(coverSyncProbe, "rapidRenderMode", true);
        compare(findChild(coverSyncProbe, "coverSyncCell-p0a"), null, "held d-pad must not keep delegates alive");
        coverSyncProbe.rapidRenderMode = false;
        tryCompare(coverSyncProbe, "rapidRenderMode", false);
    }

    function test_cover_synchronous_can_be_disabled_wholesale(): void {
        coverSyncProbe.setCurrentIndexImmediate(0);
        coverSyncProbe.coverSynchronous = false;
        compare(findChild(coverSyncProbe, "coverSyncCell-p0a").observedCoverSynchronous, false);
        coverSyncProbe.coverSynchronous = true;
        compare(findChild(coverSyncProbe, "coverSyncCell-p0a").observedCoverSynchronous, true);
    }

    function test_empty_delegate_renders_for_isEmpty_rows_only(): void {
        verify(findChild(emptyAwareProbe, "emptyAwareRealCell-real-0") !== null, "row 0 must render through the real delegate");
        verify(findChild(emptyAwareProbe, "emptyAwareRealCell-real-1") !== null, "row 1 must render through the real delegate");
        verify(findChild(emptyAwareProbe, "emptyAwareEmptyCell-pad-2") !== null, "the isEmpty row must render through emptyDelegate");
        compare(findChild(emptyAwareProbe, "emptyAwareEmptyCell-real-0"), null, "a real row must never render through emptyDelegate");
        compare(findChild(emptyAwareProbe, "emptyAwareRealCell-pad-2"), null, "the isEmpty row must not also render through the real delegate");
    }

    function test_empty_delegate_null_is_unchanged_behavior(): void {
        // Every existing PagedGrid caller leaves emptyDelegate at its
        // default null — the isEmpty row must still render through the
        // ordinary delegate exactly like a real row.
        verify(findChild(emptyAwareDefaultProbe, "emptyAwareRealCell-real-0") !== null);
        verify(findChild(emptyAwareDefaultProbe, "emptyAwareRealCell-real-1") !== null);
        verify(findChild(emptyAwareDefaultProbe, "emptyAwareRealCell-pad-2") !== null, "isEmpty must fall through to the ordinary delegate when emptyDelegate is null");
    }

    // Same shape as `emptyAwareProbe`, but created inside the test so the
    // warning filter below is armed before the delegates instantiate; the
    // grids declared at TestCase level are built before any test runs.
    Component {
        id: facelessDelegateProbe
        PagedGrid {
            model: emptyAwareModel
            delegate: emptyAwareRealDelegate
            emptyDelegate: emptyAwareEmptyDelegate
            columnsOverride: 3
            rowsOverride: 1
            width: 300
            height: 100
        }
    }

    // Regression: the skeleton Loader mirrors `cardPressed` off the loaded
    // delegate, and only Tile.qml declares that property. `EmptySlot` (and
    // the bare Items above) don't, so the read yielded `undefined`, which
    // QML refused to assign into `property bool cardPressed` -- one
    // "Unable to assign [undefined] to bool" per blank cell on every boot
    // (11 for the Hub's padded bootstrap page in a beta tester's log).
    function test_skeleton_tolerates_a_delegate_without_cardPressed(): void {
        failOnWarning(/Unable to assign \[undefined\] to bool/);
        const probe = createTemporaryObject(facelessDelegateProbe, testCase);
        verify(probe !== null);
        waitForRendering(probe);
        verify(findChild(probe, "emptyAwareEmptyCell-pad-2") !== null, "the isEmpty row must still render through emptyDelegate");
        // `cardPressed` lives on the skeleton's Loader, the loaded card's
        // parent (a Loader never forwards its properties onto its item).
        compare(findChild(probe, "pagedGridPlaceholderCard-0").parent.cardPressed, false, "a delegate with no press state reads as unpressed");
        compare(findChild(probe, "pagedGridPlaceholderCard-2").parent.cardPressed, false, "the blank cell reads as unpressed too");
    }

    // Regression: the always-on card-shaped skeleton PagedGrid paints behind
    // every cell (PagedGrid.qml's `placeholderCard`) assumes the loaded
    // delegate ends up fully opaque on top of it. `EmptySlot` deliberately
    // isn't — a genuinely blank slot, not a card with nothing on it — so
    // without an explicit exception the skeleton stays visible underneath
    // it forever, silently defeating "genuinely blank" while the delegate
    // itself (checked above) is still correctly EmptySlot.
    function test_placeholder_skeleton_is_skipped_for_empty_rows(): void {
        verify(findChild(emptyAwareProbe, "pagedGridPlaceholderCard-0").visible, "a real row keeps its skeleton");
        compare(findChild(emptyAwareProbe, "pagedGridPlaceholderCard-2").visible, false, "an isEmpty row must not paint the opaque skeleton behind EmptySlot");
    }

    // Round 3 regression: the skeleton's "paints opaque on top of me"
    // contract (see the comment above) only holds while the loaded
    // delegate actually STAYS opaque. Tile.qml's held blink (Hub Options
    // -> Move) drops the whole tile's own opacity to 0 and back; without
    // this the skeleton — a blank card with no art or name — showed
    // through underneath every time the tile blinked off, so only the
    // tile's CONTENT appeared to blink while a generic card shape stayed
    // put. The skeleton now tracks the loaded delegate's `opacity`
    // directly, the same way it already tracks `cardPressed`.
    function test_placeholder_skeleton_tracks_the_loaded_delegates_opacity(): void {
        const cell = findChild(emptyAwareProbe, "emptyAwareRealCell-real-0");
        const placeholder = findChild(emptyAwareProbe, "pagedGridPlaceholderCard-0");
        verify(cell !== null);
        verify(placeholder !== null);
        compare(placeholder.opacity, 1);

        cell.opacity = 0;
        compare(placeholder.opacity, 0, "the skeleton must vanish along with the delegate it's meant to hide behind");

        cell.opacity = 1;
        compare(placeholder.opacity, 1);
    }

    function test_isEmpty_row_remains_reachable_by_navigation(): void {
        // `skipEmptyCells` defaults false -- every caller but the Hub, and
        // the Hub itself during a Move session. With it off, isEmpty is a
        // delegate-selection signal only: PagedGrid's own navigation math
        // has no concept of it, so the padded row at index 2 is an
        // ordinary, fully reachable cell.
        compare(emptyAwareProbe.skipEmptyCells, false);
        emptyAwareProbe.setCurrentIndexImmediate(1);
        verify(emptyAwareProbe.moveSelection(1, 0));
        compare(emptyAwareProbe.currentIndex, 2);
    }

    // Right from real-a (row 0, col 0) must skip the empty cell at col 1
    // and land on real-b at col 2 -- a single-row skip, no page crossing.
    function test_skip_empty_cells_skips_within_a_row(): void {
        skipEmptyProbe.skipEmptyCells = true;
        verify(skipEmptyProbe.moveSelection(1, 0));
        compare(skipEmptyProbe.currentIndex, 2);
    }

    // Down from real-a (vr0, col0) must cross the page boundary and land
    // on real-c (vr2, col1) -- NOT real-d (vr3, col0), even though real-d
    // sits in the exact same column. This is the actual bug fix: real-c is
    // 2 virtual rows away but 1 column off (score 13*2²+1²=53); real-d is
    // perfectly aligned but 3 rows away (score 13*3²+0²=117). A search
    // that only ever walked straight down column 0 would tunnel past
    // real-c entirely and land on the farther, merely-aligned real-d --
    // exactly the "cursor sails past something nearby" bug this fixes.
    function test_skip_empty_cells_prefers_the_nearer_off_column_tile(): void {
        skipEmptyProbe.skipEmptyCells = true;
        verify(skipEmptyProbe.moveSelection(0, 1));
        compare(skipEmptyProbe.currentIndex, 7);
    }

    // `pageBy` has its own skip pass (land on the target page's preserved
    // slot, then scan forward for the first real cell on that page) --
    // distinct code path from moveSelection's nearest-candidate search,
    // so it needs its own coverage. Page-forward from real-a (page 0)
    // lands on page 1's slot 0 (skip-6, empty) and must advance to real-c.
    function test_skip_empty_cells_pageBy_lands_on_first_real_cell(): void {
        skipEmptyProbe.skipEmptyCells = true;
        verify(skipEmptyProbe.pageBy(1));
        compare(skipEmptyProbe.currentIndex, 7);
    }

    // Down from real-d (vr3, col0), the bottommost real tile on the whole
    // board, has nothing below it at all -- the wrap pass takes over,
    // scoring every remaining real tile as if the press had continued past
    // the bottom edge and come back around to the top. real-a (vr0, col0)
    // and real-b (vr0, col2) both wrap to the same distance (1), but
    // real-a's column alignment wins the tie: 13*1²+0² = 13 vs
    // 13*1²+2² = 17.
    function test_skip_empty_cells_wraps_to_the_nearest_candidate_at_the_far_edge(): void {
        skipEmptyProbe.skipEmptyCells = true;
        skipEmptyProbe.setCurrentIndexImmediate(9);
        verify(skipEmptyProbe.moveSelection(0, 1));
        compare(skipEmptyProbe.currentIndex, 0);
    }

    // A single real tile anywhere on the whole board (not just a single
    // column) has nothing to search for in either the direct or the wrap
    // pass, so Down must settle back exactly where it started and report
    // no movement.
    function test_skip_empty_cells_with_no_reachable_cell_does_not_move(): void {
        singleRealCellProbe.setCurrentIndexImmediate(0);
        verify(!singleRealCellProbe.moveSelection(0, 1));
        compare(singleRealCellProbe.currentIndex, 0);
        verify(!singleRealCellProbe.moveSelection(0, -1));
        compare(singleRealCellProbe.currentIndex, 0);
    }

    // Mirrors the directional skip: with `skipEmptyCells` set, the mouse
    // must not be able to park the cursor on a blank either (PagedGrid's
    // per-cell MouseArea has no isEmpty guard at all on the non-skipping
    // path, so hover/click always worked there before this).
    function test_skip_empty_cells_ignores_hover_and_click_on_empty_cells(): void {
        skipEmptyProbe.skipEmptyCells = true;
        // Read the empty cell's on-screen rect by landing on it directly
        // (bypassing moveSelection's own skip), then return to real-a
        // before simulating the pointer.
        skipEmptyProbe.setCurrentIndexImmediate(1);
        const emptyRect = skipEmptyProbe.currentCellRectIn(skipEmptyProbe);
        skipEmptyProbe.setCurrentIndexImmediate(0);
        const cx = emptyRect.x + emptyRect.width / 2;
        const cy = emptyRect.y + emptyRect.height / 2;

        mouseMove(skipEmptyProbe, cx, cy);
        compare(skipEmptyProbe.currentIndex, 0, "hovering an empty cell must not move the cursor when skipEmptyCells is set");
        compare(skipEmptyHoveredSpy.count, 0);

        mouseClick(skipEmptyProbe, cx, cy);
        compare(skipEmptyProbe.currentIndex, 0, "clicking an empty cell must not move the cursor either");
    }
}
