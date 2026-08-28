// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

import QtQuick
import QtTest
import Zaparoo.Theme
import Zaparoo.Ui

// Round 11 coverage for the index setup modal's own state machine: row
// navigation, the two dispatch signals (requestSystemScopePicker,
// closeRequested), and the reopen-resets-state contract. Mirrors
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
        id: systemScopePickerSpy
        target: modal
        signalName: "requestSystemScopePicker"
    }

    SignalSpy {
        id: closeSpy
        target: modal
        signalName: "closeRequested"
    }

    function initTestCase(): void {
        Sizing.screenWidth = testCase.width;
        Sizing.screenHeight = testCase.height;
    }

    function init(): void {
        modal.open = false;
        modal.open = true;
        systemScopePickerSpy.clear();
        closeSpy.clear();
    }

    function test_starts_on_the_systems_row(): void {
        compare(modal.currentIndex, modal._rowSystems);
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

    function test_accept_on_systems_row_requests_the_system_scope_picker(): void {
        modal.currentIndex = modal._rowSystems;
        modal.handleAction("accept");
        compare(systemScopePickerSpy.count, 1);
    }

    function test_cancel_emits_close_requested(): void {
        modal.handleAction("cancel");
        compare(closeSpy.count, 1);
    }

    function test_reopening_resets_row_and_scope_state(): void {
        modal.currentIndex = modal._rowStart;
        modal.selectedSystemScope = "SNES";
        modal.open = false;
        modal.open = true;
        compare(modal.currentIndex, modal._rowSystems);
        compare(modal.selectedSystemScope, "*");
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
