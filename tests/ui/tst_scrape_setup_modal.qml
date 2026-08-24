// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

import QtQuick
import QtTest
import Zaparoo.Theme
import Zaparoo.Ui

// Round 10/11 coverage for the scrape setup modal's own state machine:
// row navigation, the inline re-scrape toggle, and the dispatch signals
// (requestScraperPicker, requestSystemScopePicker, closeRequested).
// Doesn't touch Browse.MediaStatus's live scraper data -- the modal never
// calls refresh_scrapers() itself (Main.qml's openScrapeSetupModal does
// that), so scraper_ids/scraper_names stay empty here and every assertion
// below is about the row/toggle state machine, not scraper content.
TestCase {
    id: testCase
    name: "UiScrapeSetupModal"
    when: windowShown
    width: 640
    height: 480
    visible: true

    ScrapeSetupModal {
        id: modal
        anchors.fill: parent
        open: true
    }

    SignalSpy {
        id: pickerSpy
        target: modal
        signalName: "requestScraperPicker"
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
        pickerSpy.clear();
        systemScopePickerSpy.clear();
        closeSpy.clear();
    }

    function test_starts_on_the_scraper_row(): void {
        compare(modal.currentIndex, modal._rowScraper);
        compare(modal.selectedSystemScope, "*");
        compare(modal.rescrapeExisting, false);
    }

    function test_down_advances_through_all_four_rows_and_clamps(): void {
        modal.handleAction("down");
        compare(modal.currentIndex, modal._rowSystems);
        modal.handleAction("down");
        compare(modal.currentIndex, modal._rowToggle);
        modal.handleAction("down");
        compare(modal.currentIndex, modal._rowStart);
        modal.handleAction("down");
        compare(modal.currentIndex, modal._rowStart, "must clamp at the last row, not wrap or overshoot");
    }

    function test_up_retreats_through_all_four_rows_and_clamps(): void {
        modal.currentIndex = modal._rowStart;
        modal.handleAction("up");
        compare(modal.currentIndex, modal._rowToggle);
        modal.handleAction("up");
        compare(modal.currentIndex, modal._rowSystems);
        modal.handleAction("up");
        compare(modal.currentIndex, modal._rowScraper);
        modal.handleAction("up");
        compare(modal.currentIndex, modal._rowScraper, "must clamp at the first row, not go negative");
    }

    function test_left_right_only_affects_the_toggle_row(): void {
        modal.currentIndex = modal._rowScraper;
        modal.handleAction("left");
        modal.handleAction("right");
        compare(modal.rescrapeExisting, false, "left/right on the scraper row must not touch the toggle");

        modal.currentIndex = modal._rowToggle;
        modal.handleAction("right");
        compare(modal.rescrapeExisting, true);
        modal.handleAction("left");
        compare(modal.rescrapeExisting, false);
    }

    function test_accept_on_scraper_row_requests_the_picker(): void {
        modal.currentIndex = modal._rowScraper;
        modal.handleAction("accept");
        compare(pickerSpy.count, 1);
    }

    function test_accept_on_systems_row_requests_the_system_scope_picker(): void {
        modal.currentIndex = modal._rowSystems;
        modal.handleAction("accept");
        compare(systemScopePickerSpy.count, 1);
        compare(pickerSpy.count, 0, "must not also fire the scraper-picker signal");
    }

    function test_accept_on_toggle_row_flips_rescrape_existing(): void {
        modal.currentIndex = modal._rowToggle;
        modal.handleAction("accept");
        compare(modal.rescrapeExisting, true);
        modal.handleAction("accept");
        compare(modal.rescrapeExisting, false);
    }

    function test_cancel_emits_close_requested(): void {
        modal.handleAction("cancel");
        compare(closeSpy.count, 1);
    }

    function test_reopening_resets_row_and_toggle_state(): void {
        modal.currentIndex = modal._rowStart;
        modal.selectedSystemScope = "SNES";
        modal.rescrapeExisting = true;
        modal.open = false;
        modal.open = true;
        compare(modal.currentIndex, modal._rowScraper);
        compare(modal.selectedSystemScope, "*");
        compare(modal.rescrapeExisting, false);
    }

    // Round 11: the display property backing the Systems row's own
    // value text -- covers the sentinel scheme without touching
    // Browse.SystemsModel (which has no catalog loaded in this harness,
    // so a real system id would resolve to itself via the empty-name
    // fallback -- exercised separately, this only pins the two sentinel
    // branches that don't depend on the catalog).
    function test_selected_system_scope_name_covers_the_sentinel_branches(): void {
        modal.selectedSystemScope = "*";
        compare(modal._selectedSystemScopeName, qsTr("All systems"));
        modal.selectedSystemScope = "cat:Console";
        compare(modal._selectedSystemScopeName, qsTr("All %1 systems").arg("Console"));
    }
}
