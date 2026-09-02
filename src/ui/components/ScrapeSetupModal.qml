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

// "Update metadata" setup modal. Every entry point routes here — the
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
// system" list `IndexSetupModal` uses.
//
// Naming: entry points say "Update metadata" because the mechanism is a
// property of the chosen source, not of the action. Inside, once a
// source is picked, the button can be accurate: today every built-in
// source imports local files, so it reads "Start import". When real
// network scrapers land the verb becomes conditional on the source,
// resolved on the frontend as a presentation concern; Core needs no
// kind field on `ScraperInfo` for that.
//
// The entry-point verb has moved twice. "Scrape" named a mechanism that
// is wrong for every source shipped so far (they import scrapes that
// already exist on disk). "Get" fixed that but reads wrong for a re-run,
// which is what most runs are: re-pointing an already-populated library
// at a different source is not "getting" anything it lacks. "Update" is
// mechanism-neutral like "Get", accurate for the repeat case, and still
// reads correctly when there is nothing there yet. One label at every
// entry point, per docs/content-style.md's one-term-per-concept rule.
//
// Four keyboard-navigable rows (`currentIndex` 0-3): Source and Systems
// (each opens a picker *page* of this same panel — see `page`), Replace
// existing (inline toggle), Start import (action). Chrome comes from the
// shared `Modal` shell, same as LogUploadModal.
Item {
    id: modal

    property bool open: false
    // Written back by the Source page's accept rather than a Browse.Settings
    // setter — the choice isn't persisted until Start is pressed, at which
    // point `start_scrape_with_scraper` records it (see media_status.rs's
    // `persist_selected_scraper_id`). Seeded from that persisted value below
    // rather than defaulting to the first scraper Core lists, so reopening
    // this modal shows the scraper actually in force.
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
    // Flat "All systems / All <Category> systems / one system" list for the
    // Systems page, handed over by Main.qml on open (`_buildSystemScopeEntries`)
    // so the sentinel scheme lives in one place.
    property var systemScopeEntries: []
    property bool rescrapeExisting: false
    property int currentIndex: 0
    property bool _pressed: false

    // Which face the panel shows. "form" is the four rows; "source" and
    // "systems" swap the panel's content to that row's option list and
    // back — the same panel, never a second modal on top of this one (see
    // docs/style.md -> "Modal depth"). Back on a page returns to the form
    // with `currentIndex` untouched, so focus lands on the row that opened
    // it. `onOpenChanged` resets to "form".
    property string page: "form"
    readonly property bool onPickerPage: modal.page !== "form"

    readonly property int _rowScraper: 0
    readonly property int _rowSystems: 1
    readonly property int _rowToggle: 2
    readonly property int _rowStart: 3

    // Accept-button verb for the help bar, mirroring SettingsScreen's
    // `focusedActionLabel`. The bar describes the press, not the feature,
    // so it names what A does to the focused row rather than repeating
    // the modal's title. On a picker page A picks the focused entry.
    readonly property string focusedActionLabel: modal.onPickerPage ? qsTr("Select") : (modal.currentIndex === modal._rowStart ? qsTr("Start") : (modal.currentIndex === modal._rowToggle ? qsTr("Toggle") : qsTr("Change")))

    // `{ id, label }` rows for the Source page, straight from the list Core
    // reports; a source with no display name shows its id.
    readonly property var scraperEntries: {
        const ids = Browse.MediaStatus.scraper_ids;
        const names = Browse.MediaStatus.scraper_names;
        const entries = [];
        for (let i = 0; i < ids.length; i++)
            entries.push({
                id: ids[i],
                label: names[i] !== undefined && names[i] !== "" ? names[i] : ids[i]
            });
        return entries;
    }

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

    visible: modal.open
    anchors.fill: parent
    z: 300

    onOpenChanged: {
        modal.page = "form";
        if (!modal.open) {
            startCommit.stop();
            modal._pressed = false;
            return;
        }
        modal.currentIndex = modal._rowScraper;
        modal.selectedSystemScope = modal.initialSystemScope !== "" ? modal.initialSystemScope : "*";
        modal.rescrapeExisting = false;
        modal._pressed = false;
        // Re-seed from the scraper actually in force on every open, so a pick
        // made and then abandoned (changed the row, then backed out without
        // pressing Start) doesn't persist visually as though it had applied.
        modal._reseedScraperSelection(true);
    }

    // Resolve `selectedScraperId` against the list Core currently reports.
    //
    // `force` is for the open path, which discards an abandoned pick outright.
    // Otherwise a selection that is still offered is left alone, so a refresh
    // that returns the same list doesn't disturb the user mid-edit.
    //
    // An empty list clears the selection. `_startScrape` only guards against
    // an empty id, so leaving a stale one there would let Start submit a
    // scraper Core no longer offers.
    //
    // This is also the only place a stale persisted id gets reconciled:
    // `normalize_metadata_scraper` deliberately can't check membership,
    // because Core reports its scrapers at runtime and this list is the only
    // view of them.
    function _reseedScraperSelection(force: bool): void {
        const ids = Browse.MediaStatus.scraper_ids;
        if (ids.length === 0) {
            modal.selectedScraperId = "";
            return;
        }
        if (!force && modal.selectedScraperId !== "" && ids.indexOf(modal.selectedScraperId) >= 0)
            return;
        const persisted = Browse.Settings.current_metadata_scraper;
        modal.selectedScraperId = ids.indexOf(persisted) >= 0 ? persisted : ids[0];
    }

    // Reconcile on the LIST changing, not on the loading flag. `refresh_scrapers`
    // clears `scrapers_loading` before it writes `scraper_ids`
    // (media_status.rs), so a handler hung off the flag alone reads the
    // previous list -- it would validate the selection against stale data on
    // every refresh, and would miss an empty result entirely because that
    // write arrives after the flag has already settled.
    Connections {
        target: Browse.MediaStatus
        function onScraper_idsChanged(): void {
            modal._reseedScraperSelection(false);
            modal._refreshSourcePage();
        }
        // Names land in a separate write after the ids; refresh the page's
        // labels once they do.
        function onScraper_namesChanged(): void {
            modal._refreshSourcePage();
        }
        // The error path never assigns `scraper_ids` at all, so the signal
        // above never fires there. Catch the settle so a failed refresh still
        // reconciles against whatever list is currently held.
        function onScrapers_loadingChanged(): void {
            if (!Browse.MediaStatus.scrapers_loading)
                modal._reseedScraperSelection(false);
        }
    }

    function handleAction(action: string): void {
        if (modal.onPickerPage) {
            // Back leaves the page, not the modal; up/down/accept drive the
            // list. Left/right have nothing to do on a list.
            if (action === "cancel")
                modal.page = "form";
            else if (action === "up" || action === "down" || action === "accept")
                pickerList.handleAction(action);
            return;
        }
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
                modal._openPage("source");
            else if (modal.currentIndex === modal._rowSystems)
                modal._openPage("systems");
            else if (modal.currentIndex === modal._rowToggle)
                modal.rescrapeExisting = !modal.rescrapeExisting;
            else if (modal.currentIndex === modal._rowStart)
                modal._startScrape();
        }
    }

    // Entries and the initial focus are assigned, not bound, so both are in
    // place before `active` flips and the list re-applies them: the order
    // three bindings re-evaluate on one `page` change is not something to
    // lean on.
    function _openPage(target: string): void {
        // Source refresh is intentionally silent: keep the stable form on
        // screen, but do not open an empty picker during a first-load stall.
        if (target === "source" && modal.scraperEntries.length === 0)
            return;
        pickerList.entries = target === "source" ? modal.scraperEntries : modal.systemScopeEntries;
        pickerList.initialId = target === "source" ? modal.selectedScraperId : modal.selectedSystemScope;
        modal.page = target;
    }

    // The Source page copies the list when it opens, and `refresh_scrapers`
    // (fired by the router on open) can land after that: a page opened
    // early shows the previous list, and a scraper Core dropped would still
    // be pickable. Re-seed the open page from the reported list, focus on
    // the reconciled selection; no sources left means no page to be on.
    function _refreshSourcePage(): void {
        if (modal.page !== "source")
            return;
        const entries = modal.scraperEntries;
        if (entries.length === 0) {
            modal.page = "form";
            return;
        }
        pickerList.initialId = modal.selectedScraperId;
        pickerList.entries = entries;
    }

    function _pickFromPage(id: string): void {
        if (modal.page === "source")
            modal.selectedScraperId = id;
        else if (modal.page === "systems")
            modal.selectedSystemScope = id;
        modal.page = "form";
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
        title: qsTr("Update metadata")
        panelMaxWidth: Sizing.pctH(110)

        Column {
            width: parent.width
            spacing: Sizing.pctH(1.5)

            // Picker page: the row's own name as a section heading over the
            // list, under the unchanged modal title, so the dialog reads as
            // the same place with one row opened up rather than a new one.
            SectionHeader {
                objectName: "setupPickerHeader"
                width: parent.width
                visible: modal.onPickerPage
                label: modal.page === "source" ? qsTr("Source") : qsTr("Systems")
            }

            PickerList {
                id: pickerList

                objectName: "setupPickerList"
                width: parent.width
                visible: modal.onPickerPage
                active: modal.onPickerPage
                onAccepted: id => modal._pickFromPage(id)
                onCloseRequested: modal.page = "form"
            }

            // Labeled pointer rather than a paragraph explaining what the
            // sources do: the panel already carries four rows, and at 240p
            // every line above them is a line the Start row can't have. The
            // label is what makes a bare URL readable as a destination.
            // Text rather than a QR because a code large enough to scan
            // from a couch would not fit here; Settings > About carries the
            // scannable one.
            Text {
                width: parent.width
                visible: !modal.onPickerPage
                text: qsTr("Documentation: %1").arg("zaparoo.org/docs/frontend/scraping")
                font.family: Theme.fontUi
                font.pixelSize: Sizing.fontCaption
                color: Theme.textLabel
                wrapMode: Text.WrapAnywhere
                horizontalAlignment: Text.AlignHCenter
                renderType: Text.NativeRendering
            }

            // Source discovery normally completes within one frame and can
            // reuse the last reported list on later opens. Keep this form
            // mounted while refresh runs instead of replacing it with a
            // full-panel loading state that only flashes.
            Column {
                objectName: "setupForm"
                width: parent.width
                spacing: Sizing.pctH(1.5)
                visible: !modal.onPickerPage

                SettingsField {
                    width: parent.width
                    label: qsTr("Source")
                    value: modal._selectedScraperName
                    control: "picker"
                    isFocused: modal.currentIndex === modal._rowScraper
                    onHovered: modal.currentIndex = modal._rowScraper
                    onAccepted: {
                        modal.currentIndex = modal._rowScraper;
                        modal._openPage("source");
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
                        modal._openPage("systems");
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
