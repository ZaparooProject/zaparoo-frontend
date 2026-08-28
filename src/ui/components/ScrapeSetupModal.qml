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

// "Get metadata" setup modal. Every entry point routes here — the
// Settings > Library row, and the system/category/game context-menu
// entries, which previously bypassed this modal and started a scrape
// with a hardcoded "gamelist.xml" scraper and `force: false`. Callers
// pre-scope via `initialSystemScope` before opening, so a game-level
// entry that currently scrapes the containing system shows that scope
// in the Systems row rather than quietly doing more than it claims.
//
// Scoped down from the Zaparoo TUI's `showScrapeSetup` equivalent
// (`pkg/ui/tui/generatedb.go`): source choice + system scope + replace
// toggle + Start, dropping the TUI's per-system multi-select widget in
// favor of the same flat "All systems / All <Category> systems / one
// system" picker `IndexSetupModal` uses.
//
// Naming: entry points say "Get metadata" because the mechanism is a
// property of the chosen source, not of the action. Inside, once a
// source is picked, the button can be accurate — today every built-in
// source imports local files, so it reads "Start import". When real
// network scrapers land the verb becomes conditional on the source,
// resolved on the frontend as a presentation concern; Core needs no
// kind field on `ScraperInfo` for that.
//
// Four keyboard-navigable rows (`currentIndex` 0-3): Source (opens a
// nested ListPickerModal via Main.qml — see `requestScraperPicker`),
// Systems (opens the same nested picker via `requestSystemScopePicker`),
// Replace existing (inline toggle), Start import (action). Chrome comes
// from the shared `Modal` shell, same as LogUploadModal.
Item {
    id: modal

    property bool open: false
    // Written back directly by Main.qml's list-picker accept handler
    // (`fieldId === "scraperChoice"`) rather than a Browse.Settings
    // setter — the choice isn't persisted until Start is pressed.
    property string selectedScraperId: ""
    // "*" (all systems), "cat:<Category>", or a single system id. Same
    // sentinel convention as IndexSetupModal — see Main.qml's
    // `_systemScopeAll`/`_buildSystemScopeEntries`.
    property string selectedSystemScope: "*"
    // Scope the modal opens with. Main.qml sets this before opening so a
    // context-menu entry lands pre-scoped to the system or category it
    // was invoked from; the Settings row leaves it at "*". Read once in
    // `onOpenChanged` so the user can still widen or narrow it afterwards.
    property string initialSystemScope: "*"
    property bool rescrapeExisting: false
    property int currentIndex: 0
    property bool _pressed: false

    readonly property int _rowScraper: 0
    readonly property int _rowSystems: 1
    readonly property int _rowToggle: 2
    readonly property int _rowStart: 3

    // Accept-button verb for the help bar, mirroring SettingsScreen's
    // `focusedActionLabel`. The bar describes the press, not the feature,
    // so it names what A does to the focused row rather than repeating
    // the modal's title.
    readonly property string focusedActionLabel: modal.currentIndex === modal._rowStart ? qsTr("Start") : (modal.currentIndex === modal._rowToggle ? qsTr("Toggle") : qsTr("Change"))

    readonly property string _selectedScraperName: {
        const ids = Browse.MediaStatus.scraper_ids;
        const names = Browse.MediaStatus.scraper_names;
        for (let i = 0; i < ids.length; i++) {
            if (ids[i] === modal.selectedScraperId)
                return names[i] !== undefined && names[i] !== "" ? names[i] : ids[i];
        }
        return modal.selectedScraperId;
    }

    // Same display convention as IndexSetupModal's identical property —
    // see Main.qml's `_buildSystemScopeEntries` for the sentinel scheme.
    readonly property string _selectedSystemScopeName: {
        if (modal.selectedSystemScope === "" || modal.selectedSystemScope === "*")
            return qsTr("All systems");
        if (modal.selectedSystemScope.startsWith("cat:"))
            return qsTr("All %1 systems").arg(modal.selectedSystemScope.slice(4));
        const name = Browse.SystemsModel.system_name_for_id(modal.selectedSystemScope);
        return name !== "" ? name : modal.selectedSystemScope;
    }

    signal closeRequested
    // Bubbled to Main.qml (see MainLayout.qml's Loader wiring) — this
    // modal can't instantiate a second top-level modal itself; the
    // router stacks the shared ListPickerModal on top instead.
    signal requestScraperPicker
    signal requestSystemScopePicker

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
        modal.selectedSystemScope = modal.initialSystemScope !== "" ? modal.initialSystemScope : "*";
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
            else if (modal.currentIndex === modal._rowSystems)
                modal.requestSystemScopePicker();
            else if (modal.currentIndex === modal._rowToggle)
                modal.rescrapeExisting = !modal.rescrapeExisting;
            else if (modal.currentIndex === modal._rowStart)
                modal._startScrape();
        }
    }

    // Resolves `selectedSystemScope`'s "*"/"cat:<Category>"/<system id>
    // sentinel into the concrete QStringList `start_scrape_with_scraper`
    // takes — empty for "all systems", matching that invokable's own
    // "empty means every system the scraper supports" contract.
    function _resolvedSystems(): var {
        if (modal.selectedSystemScope === "" || modal.selectedSystemScope === "*")
            return [];
        if (modal.selectedSystemScope.startsWith("cat:"))
            return Browse.SystemsModel.system_ids_for_category(modal.selectedSystemScope.slice(4));
        return [modal.selectedSystemScope];
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
            Browse.MediaStatus.start_scrape_with_scraper(modal.selectedScraperId, modal._resolvedSystems(), modal.rescrapeExisting);
            modal.closeRequested();
        }
    }

    Modal {
        id: shell

        open: modal.open
        kind: "shell"
        title: qsTr("Get metadata")
        panelMaxWidth: Sizing.pctH(110)

        Column {
            width: parent.width
            spacing: Sizing.pctH(1.5)

            // Labeled pointer rather than a paragraph explaining what the
            // sources do: the panel already carries four rows, and at 240p
            // every line above them is a line the Start row can't have. The
            // label is what makes a bare URL readable as a destination.
            // Text rather than a QR because a code large enough to scan
            // from a couch would not fit here; Settings > About carries the
            // scannable one.
            Text {
                width: parent.width
                text: qsTr("Documentation: %1").arg("zaparoo.org/docs/frontend/scraping")
                font.family: Theme.fontUi
                font.pixelSize: Sizing.fontCaption
                color: Theme.textLabel
                wrapMode: Text.WrapAnywhere
                horizontalAlignment: Text.AlignHCenter
                renderType: Text.NativeRendering
            }

            Item {
                width: parent.width
                height: Sizing.pctH(6)
                visible: Browse.MediaStatus.scrapers_loading

                LoadingIndicator {
                    anchors.centerIn: parent
                    text: qsTr("Loading sources…")
                }
            }

            Column {
                width: parent.width
                spacing: Sizing.pctH(1.5)
                visible: !Browse.MediaStatus.scrapers_loading

                SettingsField {
                    width: parent.width
                    label: qsTr("Source")
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
                    label: qsTr("Systems")
                    value: modal._selectedSystemScopeName
                    control: "picker"
                    isFocused: modal.currentIndex === modal._rowSystems
                    onHovered: modal.currentIndex = modal._rowSystems
                    onAccepted: {
                        modal.currentIndex = modal._rowSystems;
                        modal.requestSystemScopePicker();
                    }
                }

                SettingsField {
                    width: parent.width
                    label: qsTr("Replace existing")
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
                    label: qsTr("Start import")
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
