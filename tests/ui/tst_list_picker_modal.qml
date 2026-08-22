// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

import QtQuick
import QtTest
import Zaparoo.Theme
import Zaparoo.Ui

// `_entries` returns a plain JS array of `{ id, label }` objects. The
// AOT compiler can't infer the shape, so leaving it untyped trips the
// compiler category for that one helper. Same pattern the production
// `entries`-consuming files use (ContextMenu.qml, ListPickerModal.qml).
// qmllint disable compiler

// Direct ListPickerModal coverage. We exercise the navigation, signal,
// and initial-selection behavior end-to-end on the component itself —
// no screens involved, so this stays inside the "test reusable
// components, not screens" rule.
TestCase {
    id: testCase
    name: "UiListPickerModal"
    when: windowShown
    width: 640
    height: 480
    visible: true

    Component.onCompleted: {
        Sizing.screenWidth = testCase.width;
        Sizing.screenHeight = testCase.height;
    }

    ListPickerModal {
        id: picker
        anchors.fill: parent
        title: "Pick one"
    }

    // Standalone probe matching production's `_rowLabelMetrics` font exactly
    // — `_rowLabelMetrics` itself is a bare `id`, lexically scoped to
    // ListPickerModal.qml and not reachable as `picker._rowLabelMetrics`
    // from this file.
    FontMetrics {
        id: probeMetrics
        font.family: Theme.fontUi
        font.pixelSize: Sizing.fontBody
    }

    SignalSpy {
        id: acceptedSpy
        target: picker
        signalName: "accepted"
    }

    SignalSpy {
        id: closeSpy
        target: picker
        signalName: "closeRequested"
    }

    function init(): void {
        Sizing.screenWidth = testCase.width;
        Sizing.screenHeight = testCase.height;
        picker.open = false;
        picker.entries = [];
        picker.initialId = "";
        picker.currentIndex = 0;
        acceptedSpy.clear();
        closeSpy.clear();
    }

    function _entries(count) {
        const list = [];
        for (let i = 0; i < count; ++i)
            list.push({
                id: "id-" + i,
                label: "Item " + i
            });
        return list;
    }

    // Swatch-bearing entries — the color-scheme picker's shape. Every entry
    // carries a 3-color `swatch` array; `_hasSwatchPreview` only checks the
    // first entry, so the picker is assumed homogeneous.
    function _swatchEntries(count) {
        const list = [];
        for (let i = 0; i < count; ++i)
            list.push({
                id: "id-" + i,
                label: "Item " + i,
                swatch: [Qt.rgba(0.1 * i, 0, 0, 1), Qt.rgba(0, 0.1 * i, 0, 1), Qt.rgba(0, 0, 0.1 * i, 1)]
            });
        return list;
    }

    function test_open_with_no_initial_id_starts_at_zero(): void {
        picker.entries = _entries(3);
        picker.open = true;
        compare(picker.currentIndex, 0);
    }

    function test_open_with_matching_initial_id_selects_entry(): void {
        picker.entries = _entries(4);
        picker.initialId = "id-2";
        picker.open = true;
        compare(picker.currentIndex, 2);
    }

    function test_open_with_unknown_initial_id_falls_back_to_zero(): void {
        picker.entries = _entries(3);
        picker.initialId = "id-missing";
        picker.open = true;
        compare(picker.currentIndex, 0);
    }

    function test_move_advances_and_wraps_forward(): void {
        picker.entries = _entries(3);
        picker.open = true;
        picker.move(1);
        compare(picker.currentIndex, 1);
        picker.move(1);
        compare(picker.currentIndex, 2);
        picker.move(1);
        compare(picker.currentIndex, 0);
    }

    function test_move_retreats_and_wraps_backward(): void {
        picker.entries = _entries(3);
        picker.open = true;
        picker.move(-1);
        compare(picker.currentIndex, 2);
        picker.move(-1);
        compare(picker.currentIndex, 1);
    }

    function test_move_with_empty_entries_is_noop(): void {
        picker.entries = [];
        picker.open = true;
        picker.currentIndex = 0;
        picker.move(1);
        compare(picker.currentIndex, 0);
        picker.move(-1);
        compare(picker.currentIndex, 0);
    }

    function test_handle_action_up_down_drives_navigation(): void {
        picker.entries = _entries(3);
        picker.open = true;
        picker.handleAction("down");
        compare(picker.currentIndex, 1);
        picker.handleAction("down");
        compare(picker.currentIndex, 2);
        picker.handleAction("up");
        compare(picker.currentIndex, 1);
    }

    function test_handle_action_accept_emits_accepted_with_current_id(): void {
        picker.entries = _entries(3);
        picker.open = true;
        picker.currentIndex = 2;
        picker.handleAction("accept");
        // accepted() is deferred via DeferredAction so the push-in
        // animation completes first. tryCompare polls until it fires.
        tryCompare(acceptedSpy, "count", 1);
        compare(acceptedSpy.signalArguments[0][0], "id-2");
    }

    function test_handle_action_accept_with_empty_id_emits_accepted(): void {
        picker.entries = [
            {
                id: "",
                label: "Automatic"
            }
        ];
        picker.open = true;
        picker.handleAction("accept");
        tryCompare(acceptedSpy, "count", 1);
        compare(acceptedSpy.signalArguments[0][0], "");
    }

    function test_handle_action_accept_with_empty_entries_no_signal(): void {
        picker.entries = [];
        picker.open = true;
        picker.currentIndex = 0;
        picker.handleAction("accept");
        compare(acceptedSpy.count, 0);
    }

    function test_handle_action_cancel_emits_close_requested(): void {
        picker.entries = _entries(3);
        picker.open = true;
        picker.handleAction("cancel");
        compare(closeSpy.count, 1);
    }

    function test_handle_action_page_menu_is_ignored(): void {
        picker.entries = _entries(3);
        picker.open = true;
        picker.handleAction("page_menu");
        compare(closeSpy.count, 0);
        compare(picker.open, true);
    }

    function test_reopen_recomputes_initial_index(): void {
        // First open lands on a match.
        picker.entries = _entries(4);
        picker.initialId = "id-3";
        picker.open = true;
        compare(picker.currentIndex, 3);
        // Close, swap entries + initialId, re-open. The next open must
        // re-resolve from the new initialId, not carry the prior index.
        picker.open = false;
        picker.entries = _entries(2);
        picker.initialId = "id-1";
        picker.open = true;
        compare(picker.currentIndex, 1);
    }

    function test_long_list_caps_visible_rows(): void {
        // _viewportHeight is bounded by _maxViewportHeight, which is
        // a Sizing.pctH(...) value — visible row count falls out of
        // that. For a list longer than the cap, the viewport must
        // reflect the cap (not full content) so the Flickable
        // scrolls; _contentHeight stays sized to the full list.
        const cap = picker._maxViewportHeight;
        const stride = picker._rowHeight + picker._rowSpacing;
        const fits = Math.floor((cap + picker._rowSpacing) / stride);
        // Use enough entries to exceed the cap on any plausible
        // screen size so the cap is exercised.
        const total = fits + 4;
        picker.entries = _entries(total);
        picker.open = true;
        compare(picker._visibleRows, fits);
        verify(picker._viewportHeight <= cap);
        compare(picker._viewportHeight, fits * picker._rowHeight + Math.max(0, fits - 1) * picker._rowSpacing);
        compare(picker._contentHeight, total * picker._rowHeight + (total - 1) * picker._rowSpacing);
    }

    // Callers swap `entries` on an open modal (the launcher flow replaces a
    // one-row "saving" list with a three-row error list). The focus must
    // re-resolve to `initialId` in the NEW list; leaving a stale index past
    // the end renders no focused row and makes Accept a dead key.
    function test_entries_swap_while_open_reapplies_initial_id(): void {
        picker.entries = _entries(1);
        picker.initialId = "";
        picker.open = true;
        compare(picker.currentIndex, 0);

        picker.initialId = "id-1";
        picker.entries = _entries(3);
        compare(picker.currentIndex, 1);
        compare(picker.entries[picker.currentIndex].id, "id-1");
    }

    // A swap that shrinks the list must not leave currentIndex out of range.
    function test_entries_swap_shrinking_list_clamps_focus(): void {
        picker.entries = _entries(4);
        picker.open = true;
        picker.handleAction("down");
        picker.handleAction("down");
        compare(picker.currentIndex, 2);

        picker.initialId = "";
        picker.entries = _entries(1);
        verify(picker.currentIndex < picker.entries.length, "focus stays inside the new list");
        compare(picker.currentIndex, 0);
    }

    function test_short_list_does_not_pad_viewport(): void {
        // For a list that fits inside the cap, the viewport should
        // match the content exactly so the modal doesn't reserve dead
        // space below the entries.
        picker.entries = _entries(2);
        picker.open = true;
        compare(picker._visibleRows, 2);
        compare(picker._viewportHeight, picker._contentHeight);
    }

    // Regression coverage for the color-scheme picker's swatch preview
    // (round 5). Plain `{id, label}` entries — every picker except color
    // scheme — must render pixel-for-pixel as before: centered label
    // visible, swatch elements absent and reserving no panel width.
    function test_entries_without_swatch_use_plain_centered_row(): void {
        picker.entries = _entries(1);
        picker.open = true;
        verify(!picker._hasSwatchPreview);
        compare(picker._swatchBandWidth, 0);

        const row = findChild(picker, "listPickerRow-0");
        verify(row !== null);
        const centered = findChild(row, "listPickerRowLabelCentered");
        const swatchLabel = findChild(row, "listPickerRowLabelSwatch");
        const swatches = findChild(row, "listPickerRowSwatches");
        verify(centered !== null);
        verify(centered.visible);
        verify(swatchLabel !== null);
        verify(!swatchLabel.visible);
        verify(swatches !== null);
        verify(!swatches.visible);
    }

    // Entries carrying a `swatch` array — the color-scheme picker's shape —
    // switch the row to a left-aligned label plus three color boxes on the
    // right, and the panel reserves width for the swatch band.
    function test_entries_with_swatch_render_label_left_and_three_boxes(): void {
        picker.entries = _swatchEntries(2);
        picker.open = true;
        verify(picker._hasSwatchPreview);
        verify(picker._swatchBandWidth > 0);

        const row = findChild(picker, "listPickerRow-1");
        verify(row !== null);
        const centered = findChild(row, "listPickerRowLabelCentered");
        const swatchLabel = findChild(row, "listPickerRowLabelSwatch");
        const swatches = findChild(row, "listPickerRowSwatches");
        verify(centered !== null);
        verify(!centered.visible);
        verify(swatchLabel !== null);
        verify(swatchLabel.visible);
        verify(swatches !== null);
        verify(swatches.visible);

        const expected = picker.entries[1].swatch;
        for (let i = 0; i < 3; ++i) {
            const box = findChild(row, "listPickerRowSwatch-" + i);
            verify(box !== null, "swatch box " + i + " must exist");
            compare(box.color.toString(), expected[i].toString());
            // Round 6, item 1: a near-black or near-white swatch can sit at
            // the same contrast as the row's own surfaceCard face and
            // disappear into it. Every box carries a textLabel border.
            compare(box.border.color.toString(), Theme.textLabel.toString());
            verify(box.border.width > 0);
        }
    }

    // Round 6, item 2: the panel/row width formula must carry deliberate
    // slack over the raw measured advance width. `Text.NativeRendering`
    // lays out on integer, hinted per-glyph advances, which can paint a
    // few px wider than `FontMetrics.advanceWidth()`'s fractional,
    // unhinted total; a zero-slack fit then elided text that should have
    // fit. Verified structurally (the formula includes the slack term)
    // rather than by forcing an actual hinting mismatch, which isn't
    // reproducible deterministically in a headless test environment.
    function test_measure_label_width_carries_hinting_slack(): void {
        const label = "A moderately long label for measuring";
        const raw = Math.ceil(Math.max(probeMetrics.advanceWidth(label), probeMetrics.boundingRect(label).width));
        const measured = picker._measureLabelWidth(probeMetrics, label, Sizing.fontBody, Theme.fontUi);
        compare(measured, raw + Sizing.stroke(2));
    }

    // The panel sizes around the widest entry via the same slack-carrying
    // measurement — a panel sized to exactly the raw advance width was the
    // other half of the zero-slack bug (item 2): even with per-row slack,
    // a panel with none of its own would still clip the widest label.
    function test_widest_entry_label_width_uses_the_same_slack(): void {
        picker.entries = [
            {
                id: "id-0",
                label: "Short"
            },
            {
                id: "id-1",
                label: "A rather longer picker entry label"
            }
        ];
        const expected = picker._measureLabelWidth(probeMetrics, "A rather longer picker entry label", Sizing.fontBody, Theme.fontUi);
        compare(picker._widestEntryLabelWidth, expected);
    }

    // Round 6 follow-up: the item-2 measurement fix wasn't sufficient on
    // its own. Modal.qml's shell-mode branch applies its own 78%-of-viewport
    // breathing-room ceiling on top of whatever `panelMaxWidth` a caller
    // supplies — fine for the QR/legal shells whose content Modal can't
    // measure, but it was still clipping the picker's own precisely
    // measured width whenever the swatch band pushed that past 78% of a
    // small screen. `contentSized: true` swaps that ceiling for the same
    // 92% the four prebaked kinds use.
    function test_content_sized_panel_is_not_clamped_by_shell_breathing_room(): void {
        const labels = ["Zaparoo Dark", "Zaparoo Light", "Classic Purple", "Dracula", "Nord", "Synthwave '84", "Amber Phosphor", "Green Phosphor", "Neo Geo", "NES", "Virtual Boy"];
        picker.entries = labels.map((label, i) => ({
                    id: "id-" + i,
                    label: label,
                    swatch: [Qt.rgba(0.1, 0, 0, 1), Qt.rgba(0, 0.1, 0, 1), Qt.rgba(0, 0, 0.1, 1)]
                }));
        picker.open = true;

        const expectedWidth = Sizing.px(Math.min(testCase.width * 0.92, Math.max(picker._minPanelWidth, picker._desiredPanelWidth)));
        const panel = findChild(picker, "modalPanel");
        verify(panel !== null);
        compare(panel.width, expectedWidth);

        for (let i = 0; i < labels.length; ++i) {
            const row = findChild(picker, "listPickerRow-" + i);
            verify(row !== null, "row " + i + " must exist");
            const swatchLabel = findChild(row, "listPickerRowLabelSwatch");
            verify(swatchLabel !== null);
            verify(!swatchLabel.truncated, "label '" + labels[i] + "' must not elide when the panel is content-sized");
        }
    }
}
