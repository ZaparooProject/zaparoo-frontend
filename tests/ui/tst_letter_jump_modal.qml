// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

import QtQuick
import QtTest
import Zaparoo.Theme
import Zaparoo.Ui

// `_entries` returns a plain JS array of `{ key, label, count, cursor }`
// objects. The AOT compiler can't infer the shape, so leaving it untyped trips
// the compiler category for that one helper. Same pattern the production
// `entries`-consuming files use (LetterJumpModal.qml, ListPickerModal.qml).
// qmllint disable compiler

// Direct LetterJumpModal coverage. `nextIndex` is a pure 2D-grid move, so we
// exercise it (and the accept/close dispatch) on the component itself - no
// screens involved, staying inside the "test reusable components, not screens"
// rule.
TestCase {
    id: testCase
    name: "UiLetterJumpModal"
    when: windowShown
    width: 640
    height: 480
    visible: true

    Component.onCompleted: {
        Sizing.screenWidth = testCase.width;
        Sizing.screenHeight = testCase.height;
    }

    LetterJumpModal {
        id: grid
        anchors.fill: parent
    }

    SignalSpy {
        id: acceptedSpy
        target: grid
        signalName: "accepted"
    }

    SignalSpy {
        id: closeSpy
        target: grid
        signalName: "closeRequested"
    }

    function init(): void {
        Sizing.screenWidth = testCase.width;
        Sizing.screenHeight = testCase.height;
        grid.open = false;
        grid.entries = [];
        grid.currentIndex = 0;
        grid.loading = false;
        acceptedSpy.clear();
        closeSpy.clear();
    }

    function _entries(count) {
        const list = [];
        for (let i = 0; i < count; ++i)
            list.push({
                key: "K" + i,
                label: "L" + i,
                count: i,
                cursor: "c" + i,
                // Distinct from count so the test proves the modal emits the
                // server-supplied offset, not a client-side count sum.
                offset: i * 100
            });
        return list;
    }

    // ── nextIndex: pure 2D grid move ──────────────────────────────────────────

    function test_next_index_left_right_clamps_within_bounds(): void {
        // 8 cells, 4 columns. right advances; right at the end stays.
        compare(grid.nextIndex("right", 0, 8, 4), 1);
        compare(grid.nextIndex("right", 7, 8, 4), 7);
        compare(grid.nextIndex("left", 3, 8, 4), 2);
        compare(grid.nextIndex("left", 0, 8, 4), 0);
    }

    function test_next_index_up_down_step_by_columns(): void {
        // 8 cells, 4 columns → two full rows.
        compare(grid.nextIndex("down", 1, 8, 4), 5);
        compare(grid.nextIndex("up", 5, 8, 4), 1);
        // up from the top row stays put.
        compare(grid.nextIndex("up", 2, 8, 4), 2);
    }

    function test_next_index_down_lands_on_last_cell_from_partial_row(): void {
        // 6 cells, 4 columns → row 0 = 0..3, row 1 = 4..5 (partial).
        // down from index 2 would be 6 (out of range) → clamp to last cell 5.
        compare(grid.nextIndex("down", 2, 6, 4), 5);
        // down from the last row stays put.
        compare(grid.nextIndex("down", 5, 6, 4), 5);
    }

    function test_next_index_empty_returns_zero(): void {
        compare(grid.nextIndex("down", 0, 0, 4), 0);
        compare(grid.nextIndex("right", 0, 0, 4), 0);
    }

    // ── _fitColumns: area-bounded column packing ──────────────────────────────

    function test_fit_columns_zero_returns_one(): void {
        compare(grid._fitColumns(0, 1000, 500, 10), 1);
    }

    function test_fit_columns_few_buckets_single_row(): void {
        // A handful of buckets in a wide, short area pack into one row (more
        // rows would only shrink the cells), so columns == bucket count.
        const cols = grid._fitColumns(5, 1200, 120, 10);
        compare(cols, 5);
        compare(Math.ceil(5 / cols), 1);
    }

    function test_fit_columns_prefers_larger_cells_in_tall_area(): void {
        // In a tall area, packing into multiple rows yields larger square cells
        // than a single wide row, so the helper picks fewer columns.
        const cols = grid._fitColumns(5, 1000, 500, 10);
        verify(cols < 5);
        verify(Math.ceil(5 / cols) > 1);
    }

    function test_fit_columns_full_alphabet_fits_height(): void {
        // 28 buckets (#, 0-9, A-Z) in a typical area must not pick so few
        // columns that the rows overflow the height budget. Verify the chosen
        // layout's grid height fits within availH, and it isn't a single row.
        const count = 28;
        const availW = 1000;
        const availH = 500;
        const gap = 10;
        const cols = grid._fitColumns(count, availW, availH, gap);
        verify(cols > 1);
        const rows = Math.ceil(count / cols);
        const cellH = (availH - (rows - 1) * gap) / rows;
        const cellW = (availW - (cols - 1) * gap) / cols;
        const cell = Math.min(cellW, cellH);
        // The packed grid (square cells) fits inside both dimensions.
        verify(rows * cell + (rows - 1) * gap <= availH + 1);
        verify(cols * cell + (cols - 1) * gap <= availW + 1);
    }

    // ── handleAction dispatch ────────────────────────────────────────────────

    function test_handle_action_accept_emits_item_offset(): void {
        // _entries(n) sets each bucket's offset to index*100. The modal emits
        // the selected bucket's offset verbatim, so picking index 3 emits 300.
        grid.entries = _entries(6);
        grid.open = true;
        grid.currentIndex = 3;
        grid.handleAction("accept");
        // accepted() is deferred via DeferredAction so the push-in animation
        // completes first. tryCompare polls until it fires.
        tryCompare(acceptedSpy, "count", 1);
        compare(acceptedSpy.signalArguments[0][0], 300);
    }

    function test_offset_for_index_reads_server_offset(): void {
        // Offsets are index*100; the helper returns the entry's offset field
        // directly (no client-side summing), and 0 for out-of-range.
        grid.entries = _entries(5);
        compare(grid._offsetForIndex(0), 0);
        compare(grid._offsetForIndex(3), 300);
        compare(grid._offsetForIndex(4), 400);
        compare(grid._offsetForIndex(5), 0);
    }

    function test_handle_action_accept_with_empty_entries_no_signal(): void {
        grid.entries = [];
        grid.open = true;
        grid.currentIndex = 0;
        grid.handleAction("accept");
        compare(acceptedSpy.count, 0);
    }

    function test_handle_action_cancel_emits_close(): void {
        grid.entries = _entries(4);
        grid.open = true;
        grid.handleAction("cancel");
        compare(closeSpy.count, 1);
    }

    function test_handle_action_page_menu_emits_close(): void {
        // West again while the grid is open closes it (page-scoped toggle).
        grid.entries = _entries(4);
        grid.open = true;
        grid.handleAction("page_menu");
        compare(closeSpy.count, 1);
    }

    function test_open_resets_index_to_zero(): void {
        grid.entries = _entries(6);
        grid.currentIndex = 4;
        grid.open = true;
        compare(grid.currentIndex, 0);
    }
}
