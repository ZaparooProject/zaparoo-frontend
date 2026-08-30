// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

import QtQuick
import QtTest
import Zaparoo.Theme
import Zaparoo.Browse as Browse
import Zaparoo.Ui

// Coverage for the scrape setup modal's own state machine: row
// navigation, the inline re-scrape toggle, the in-panel picker pages the
// Source and Systems rows open (docs/style.md -> "Modal depth": a choice
// made inside a modal is a page of that modal, never a second modal), and
// closeRequested. Doesn't drive Browse.MediaStatus's live fetch -- the
// modal never calls refresh_scrapers() itself (Main.qml's
// openScrapeSetupModal does that) -- so scraper_ids/scraper_names are set
// directly where a test needs sources.
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
            id: "cat:Console",
            label: "All Console systems"
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
        // Reset before reopening: `initialSystemScope` seeds
        // `selectedSystemScope` in onOpenChanged, so a test that sets it
        // would otherwise carry its scope into every later test.
        modal.initialSystemScope = "*";
        modal.systemScopeEntries = testCase.scopeEntries;
        Browse.MediaStatus.scraper_ids = ["gamelist.xml", "es-media-folders"];
        Browse.MediaStatus.scraper_names = ["Gamelist XML", "ES media folders"];
        modal.open = true;
        closeSpy.clear();
    }

    function _list(): var {
        return findChild(modal, "setupPickerList");
    }

    function test_starts_on_the_scraper_row(): void {
        compare(modal.currentIndex, modal._rowScraper);
        compare(modal.page, "form");
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

    // Accept on Source opens the Source page: the same panel, the row's
    // entries listed with the current selection focused. Nothing is
    // emitted for a router to stack a picker with.
    function test_accept_on_scraper_row_opens_the_source_page(): void {
        modal.selectedScraperId = "es-media-folders";
        modal.currentIndex = modal._rowScraper;
        modal.handleAction("accept");
        compare(modal.page, "source");
        compare(modal.onPickerPage, true);
        compare(modal.focusedActionLabel, qsTr("Select"));
        const list = _list();
        verify(list !== null);
        compare(list.entries.length, 2);
        compare(list.entries[1].label, "ES media folders");
        compare(list.currentIndex, 1, "the page opens on the current selection");
        compare(findChild(modal, "setupPickerHeader").label, qsTr("Source"));
    }

    // Up/down on a page move the list, not the form; the form's own row
    // is untouched until the page closes.
    function test_page_navigation_moves_the_list_not_the_form(): void {
        modal.selectedScraperId = "gamelist.xml";
        modal.currentIndex = modal._rowScraper;
        modal.handleAction("accept");
        const list = _list();
        compare(list.currentIndex, 0);
        modal.handleAction("down");
        compare(list.currentIndex, 1);
        compare(modal.currentIndex, modal._rowScraper, "the form row must not move while a page is open");
        modal.handleAction("up");
        compare(list.currentIndex, 0);
    }

    // Accept on an entry applies it, returns to the form, and leaves focus
    // on the row that opened the page.
    function test_accepting_a_source_applies_it_and_returns_to_the_form(): void {
        modal.selectedScraperId = "gamelist.xml";
        modal.currentIndex = modal._rowScraper;
        modal.handleAction("accept");
        modal.handleAction("down");
        modal.handleAction("accept");
        // The list's accept is deferred behind the row's push cue.
        tryCompare(modal, "page", "form");
        compare(modal.selectedScraperId, "es-media-folders");
        compare(modal.currentIndex, modal._rowScraper);
        compare(modal.focusedActionLabel, qsTr("Change"));
        compare(closeSpy.count, 0, "picking a source must not close the modal");
    }

    // Back on a page leaves the page, not the modal, and changes nothing.
    function test_cancel_on_a_page_returns_to_the_form_unchanged(): void {
        modal.selectedScraperId = "gamelist.xml";
        modal.currentIndex = modal._rowScraper;
        modal.handleAction("accept");
        modal.handleAction("down");
        modal.handleAction("cancel");
        compare(modal.page, "form");
        compare(modal.selectedScraperId, "gamelist.xml");
        compare(modal.currentIndex, modal._rowScraper);
        compare(closeSpy.count, 0, "Back on a page must not close the modal");
    }

    function test_accept_on_systems_row_opens_the_systems_page(): void {
        modal.selectedSystemScope = "cat:Console";
        modal.currentIndex = modal._rowSystems;
        modal.handleAction("accept");
        compare(modal.page, "systems");
        const list = _list();
        compare(list.entries.length, 3);
        compare(list.currentIndex, 1, "the page opens on the current scope");
        compare(findChild(modal, "setupPickerHeader").label, qsTr("Systems"));
        modal.handleAction("down");
        modal.handleAction("accept");
        tryCompare(modal, "page", "form");
        compare(modal.selectedSystemScope, "SNES");
        compare(modal.currentIndex, modal._rowSystems);
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

    function test_reopening_resets_row_toggle_and_page_state(): void {
        modal.currentIndex = modal._rowStart;
        modal.selectedSystemScope = "SNES";
        modal.rescrapeExisting = true;
        modal.page = "systems";
        modal.open = false;
        modal.open = true;
        compare(modal.currentIndex, modal._rowScraper);
        compare(modal.selectedSystemScope, "*");
        compare(modal.rescrapeExisting, false);
        compare(modal.page, "form", "a page left open must not survive a reopen");
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

    // Context-menu entries route through this modal pre-scoped, so the
    // Systems row shows what is actually about to run rather than the
    // caller quietly scraping wider than the user asked for.
    function test_initial_scope_seeds_the_systems_row_on_open(): void {
        modal.open = false;
        modal.initialSystemScope = "Genesis";
        modal.open = true;
        compare(modal.selectedSystemScope, "Genesis");
    }

    // The Settings > Library entry point passes the all-systems sentinel;
    // an empty string must not strand the modal on a blank scope.
    function test_empty_initial_scope_falls_back_to_all_systems(): void {
        modal.open = false;
        modal.initialSystemScope = "";
        modal.open = true;
        compare(modal.selectedSystemScope, "*");
    }

    // Pre-scoping seeds the row but doesn't lock it: the user can still
    // widen or narrow before starting.
    function test_initial_scope_is_only_a_seed(): void {
        modal.open = false;
        modal.initialSystemScope = "Genesis";
        modal.open = true;
        modal.selectedSystemScope = "*";
        compare(modal.selectedSystemScope, "*");
    }

    // `refresh_scrapers` clears `scrapers_loading` BEFORE it assigns
    // `scraper_ids` (media_status.rs), so reconciliation hangs off the list
    // changing rather than off the flag. These drive `scraper_ids` directly,
    // which is the same signal the real fetch produces.
    function test_scraper_selection_follows_the_reported_list(): void {
        modal.selectedScraperId = "es-media-folders";
        // A still-offered selection is left alone, so a refresh returning the
        // same list does not disturb an edit in progress.
        Browse.MediaStatus.scraper_ids = ["gamelist.xml", "es-media-folders"];
        compare(modal.selectedScraperId, "es-media-folders");

        // Dropped from the list: fall back rather than keep pointing at a
        // scraper Core no longer offers.
        Browse.MediaStatus.scraper_ids = ["gamelist.xml"];
        compare(modal.selectedScraperId, "gamelist.xml");
    }

    // An empty list must clear the selection outright. `_startScrape` only
    // guards against an empty id, so a stale one left here would let Start
    // submit a scraper that does not exist.
    function test_empty_scraper_list_clears_the_selection(): void {
        Browse.MediaStatus.scraper_ids = ["gamelist.xml"];
        compare(modal.selectedScraperId, "gamelist.xml");
        Browse.MediaStatus.scraper_ids = [];
        compare(modal.selectedScraperId, "", "an empty list must leave nothing selected");
    }

    // The help bar describes the press, not the feature, so the accept
    // verb tracks the focused row rather than repeating the title.
    function test_focused_action_label_tracks_the_focused_row(): void {
        modal.currentIndex = modal._rowScraper;
        compare(modal.focusedActionLabel, qsTr("Change"));
        modal.currentIndex = modal._rowSystems;
        compare(modal.focusedActionLabel, qsTr("Change"));
        modal.currentIndex = modal._rowToggle;
        compare(modal.focusedActionLabel, qsTr("Toggle"));
        modal.currentIndex = modal._rowStart;
        compare(modal.focusedActionLabel, qsTr("Start"));
    }
}
