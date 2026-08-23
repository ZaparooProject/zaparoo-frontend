// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import Zaparoo.Theme
import Zaparoo.Browse as Browse

// cxx-qt 0.8 patches `isFinal: true` on singleton properties but the
// qmltypes schema has no `isFinal` slot, so every read trips qmllint's
// "Member can be shadowed" check. Until the schema grows the slot,
// suppress the compiler category file-wide.
// qmllint disable compiler

// Round 10 — scrape setup modal. Opened from the Settings "Scrape
// metadata" row when idle (replacing a direct, hardcoded-scraper start).
// Scoped down from the Zaparoo TUI's `showScrapeSetup` equivalent
// (`pkg/ui/tui/generatedb.go`): scraper choice + re-scrape toggle +
// Start, dropping the TUI's per-system multi-select — the frontend
// already has that scoping via the existing per-system/per-category
// `scrape_system`/`scrape_category` context-menu entries, which stay on
// the hardcoded "gamelist.xml" scraper untouched by this modal.
//
// Three keyboard-navigable rows (`currentIndex` 0-2): Scraper (opens a
// nested ListPickerModal via Main.qml — see `requestScraperPicker`),
// Re-scrape existing (inline toggle), Start scrape (action). Chrome
// comes from the shared `Modal` shell, same as LogUploadModal.
Item {
    id: modal

    property bool open: false
    // Written back directly by Main.qml's list-picker accept handler
    // (`fieldId === "scraperChoice"`) rather than a Browse.Settings
    // setter — the choice isn't persisted until Start is pressed.
    property string selectedScraperId: ""
    property bool rescrapeExisting: false
    property int currentIndex: 0
    property bool _pressed: false

    readonly property int _rowScraper: 0
    readonly property int _rowToggle: 1
    readonly property int _rowStart: 2

    readonly property string _selectedScraperName: {
        const ids = Browse.MediaStatus.scraper_ids;
        const names = Browse.MediaStatus.scraper_names;
        for (let i = 0; i < ids.length; i++) {
            if (ids[i] === modal.selectedScraperId)
                return names[i] !== undefined && names[i] !== "" ? names[i] : ids[i];
        }
        return modal.selectedScraperId;
    }

    signal closeRequested
    // Bubbled to Main.qml (see MainLayout.qml's Loader wiring) — this
    // modal can't instantiate a second top-level modal itself; the
    // router stacks the shared ListPickerModal on top instead.
    signal requestScraperPicker

    visible: modal.open
    anchors.fill: parent
    z: 300

    onOpenChanged: {
        if (!modal.open) {
            startCommit.stop();
            modal._pressed = false;
            return;
        }
        modal.currentIndex = modal._rowScraper;
        modal.rescrapeExisting = false;
        modal._pressed = false;
    }

    // Seed the selection once the scraper list lands, if nothing is
    // chosen yet (first open) or the previous choice fell out of the
    // list (a stale id from a prior session/Core reconfiguration).
    Connections {
        target: Browse.MediaStatus
        function onScrapers_loadingChanged(): void {
            if (Browse.MediaStatus.scrapers_loading)
                return;
            const ids = Browse.MediaStatus.scraper_ids;
            if (ids.length === 0)
                return;
            if (modal.selectedScraperId === "" || ids.indexOf(modal.selectedScraperId) < 0)
                modal.selectedScraperId = ids[0];
        }
    }

    function handleAction(action: string): void {
        if (action === "cancel")
            modal.closeRequested();
        else if (action === "up")
            modal.currentIndex = Math.max(modal._rowScraper, modal.currentIndex - 1);
        else if (action === "down")
            modal.currentIndex = Math.min(modal._rowStart, modal.currentIndex + 1);
        else if (action === "left" || action === "right") {
            if (modal.currentIndex === modal._rowToggle)
                modal.rescrapeExisting = !modal.rescrapeExisting;
        } else if (action === "accept") {
            if (modal.currentIndex === modal._rowScraper)
                modal.requestScraperPicker();
            else if (modal.currentIndex === modal._rowToggle)
                modal.rescrapeExisting = !modal.rescrapeExisting;
            else if (modal.currentIndex === modal._rowStart)
                modal._startScrape();
        }
    }

    function _startScrape(): void {
        if (modal.selectedScraperId === "" || Browse.MediaStatus.scrapers_loading)
            return;
        modal._pressed = true;
        startCommit.arm();
    }

    // Deferred like SettingsScreen's own field-commit: the row's own
    // push-in cue plays on the still-visible modal before the scrape
    // actually starts and the modal closes.
    DeferredAction {
        id: startCommit
        onDeferred: {
            modal._pressed = false;
            Browse.MediaStatus.start_scrape_with_scraper(modal.selectedScraperId, modal.rescrapeExisting);
            modal.closeRequested();
        }
    }

    Modal {
        id: shell

        open: modal.open
        kind: "shell"
        title: qsTr("Scrape metadata")
        panelMaxWidth: Sizing.pctH(110)

        Column {
            width: parent.width
            spacing: Sizing.pctH(1.5)

            Item {
                width: parent.width
                height: Sizing.pctH(6)
                visible: Browse.MediaStatus.scrapers_loading

                LoadingIndicator {
                    anchors.centerIn: parent
                    text: qsTr("Loading scrapers…")
                }
            }

            Column {
                width: parent.width
                spacing: Sizing.pctH(1.5)
                visible: !Browse.MediaStatus.scrapers_loading

                SettingsField {
                    width: parent.width
                    label: qsTr("Scraper")
                    value: modal._selectedScraperName
                    control: "picker"
                    isFocused: modal.currentIndex === modal._rowScraper
                    onHovered: modal.currentIndex = modal._rowScraper
                    onAccepted: {
                        modal.currentIndex = modal._rowScraper;
                        modal.requestScraperPicker();
                    }
                }

                SettingsField {
                    width: parent.width
                    label: qsTr("Re-scrape existing")
                    value: ""
                    control: "toggle"
                    checked: modal.rescrapeExisting
                    isFocused: modal.currentIndex === modal._rowToggle
                    onHovered: modal.currentIndex = modal._rowToggle
                    onClicked: {
                        modal.currentIndex = modal._rowToggle;
                        modal.rescrapeExisting = !modal.rescrapeExisting;
                    }
                }

                SettingsField {
                    width: parent.width
                    label: qsTr("Start scrape")
                    value: ""
                    control: "action"
                    isFocused: modal.currentIndex === modal._rowStart
                    onHovered: modal.currentIndex = modal._rowStart
                    onAccepted: {
                        modal.currentIndex = modal._rowStart;
                        modal._startScrape();
                    }
                }
            }
        }
    }
}
