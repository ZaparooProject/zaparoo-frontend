// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

import QtQuick
import QtTest
import Zaparoo.Theme
import Zaparoo.Ui

// Round 11 coverage for the index setup modal's own state machine: row
// navigation, the in-panel Systems page (docs/style.md -> "Modal depth"),
// closeRequested, and the reopen-resets-state contract. Mirrors
// tst_scrape_setup_modal.qml's shape at the two-row scale.
TestCase {
    id: testCase
    name: "UiIndexSetupModal"
    when: windowShown
    width: 640
    height: 480
    visible: true

    IndexSetupModal {
        id: modal
        anchors.fill: parent
        open: true
    }

    SignalSpy {
        id: closeSpy
        target: modal
        signalName: "closeRequested"
    }

    readonly property var scopeEntries: [
        {
            id: "*",
            label: "All systems"
        },
        {
            id: "SNES",
            label: "SNES"
        }
    ]

    function initTestCase(): void {
        Sizing.screenWidth = testCase.width;
        Sizing.screenHeight = testCase.height;
    }

    function init(): void {
        modal.open = false;
        modal.systemScopeEntries = testCase.scopeEntries;
        modal.open = true;
        closeSpy.clear();
    }

    function test_starts_on_the_systems_row(): void {
        compare(modal.currentIndex, modal._rowSystems);
        compare(modal.page, "form");
        compare(modal.selectedSystemScope, "*");
    }

    function test_down_advances_through_both_rows_and_clamps(): void {
        modal.handleAction("down");
        compare(modal.currentIndex, modal._rowStart);
        modal.handleAction("down");
        compare(modal.currentIndex, modal._rowStart, "must clamp at the last row, not wrap or overshoot");
    }

    function test_up_retreats_through_both_rows_and_clamps(): void {
        modal.currentIndex = modal._rowStart;
        modal.handleAction("up");
        compare(modal.currentIndex, modal._rowSystems);
        modal.handleAction("up");
        compare(modal.currentIndex, modal._rowSystems, "must clamp at the first row, not go negative");
    }

    // Accept on Systems opens the Systems page of this same panel; accept
    // on an entry applies it and returns to the form with focus where it
    // was. Nothing is emitted for a router to stack a picker with.
    function test_accept_on_systems_row_opens_the_page_and_a_pick_returns(): void {
        modal.currentIndex = modal._rowSystems;
        modal.handleAction("accept");
        compare(modal.page, "systems");
        compare(modal.focusedActionLabel, qsTr("Select"));
        const list = findChild(modal, "setupPickerList");
        verify(list !== null);
        compare(list.entries.length, 2);
        compare(list.currentIndex, 0, "the page opens on the current scope");
        modal.handleAction("down");
        compare(modal.currentIndex, modal._rowSystems, "the form row must not move while the page is open");
        modal.handleAction("accept");
        tryCompare(modal, "page", "form");
        compare(modal.selectedSystemScope, "SNES");
        compare(modal.currentIndex, modal._rowSystems);
        compare(closeSpy.count, 0, "picking a scope must not close the modal");
    }

    function test_cancel_on_the_page_returns_to_the_form_unchanged(): void {
        modal.currentIndex = modal._rowSystems;
        modal.handleAction("accept");
        modal.handleAction("down");
        modal.handleAction("cancel");
        compare(modal.page, "form");
        compare(modal.selectedSystemScope, "*");
        compare(closeSpy.count, 0, "Back on the page must not close the modal");
    }

    function test_cancel_emits_close_requested(): void {
        modal.handleAction("cancel");
        compare(closeSpy.count, 1);
    }

    function test_reopening_resets_row_scope_and_page_state(): void {
        modal.currentIndex = modal._rowStart;
        modal.selectedSystemScope = "SNES";
        modal.page = "systems";
        modal.open = false;
        modal.open = true;
        compare(modal.currentIndex, modal._rowSystems);
        compare(modal.selectedSystemScope, "*");
        compare(modal.page, "form", "a page left open must not survive a reopen");
    }

    function test_selected_system_scope_name_covers_the_sentinel_branches(): void {
        modal.selectedSystemScope = "*";
        compare(modal._selectedSystemScopeName, qsTr("All systems"));
        modal.selectedSystemScope = "cat:Console";
        compare(modal._selectedSystemScopeName, qsTr("All %1 systems").arg("Console"));
    }

    // The help bar describes the press, not the feature. Before this
    // existed neither setup modal had a helpEntries branch at all, so the
    // bar advertised the Settings screen's rows while a modal owned input.
    function test_focused_action_label_tracks_the_focused_row(): void {
        modal.currentIndex = modal._rowSystems;
        compare(modal.focusedActionLabel, qsTr("Change"));
        modal.currentIndex = modal._rowStart;
        compare(modal.focusedActionLabel, qsTr("Start"));
    }
}
