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

// Round 11 — update-media-database setup modal. Opened from the Settings
// "Update media database" row when idle (replacing a direct, always-every-
// system index start), mirroring the treatment `runScraper` got in round
// 10. Structurally a trimmed `ScrapeSetupModal`: just a Systems scope
// picker and Start, since a full index has no scraper choice or re-scrape
// toggle to make.
//
// Two keyboard-navigable rows (`currentIndex` 0-1): Systems (opens a
// picker *page* of this same panel — see `page`), Start update (action).
// Chrome comes from the shared `Modal` shell, same as
// ScrapeSetupModal/LogUploadModal.
Item {
    id: modal

    property bool open: false
    // "*" (all systems), "cat:<Category>", or a single system id. Same
    // sentinel convention as ScrapeSetupModal — see Main.qml's
    // `_systemScopeAll`/`_buildSystemScopeEntries`.
    property string selectedSystemScope: "*"
    // Flat "All systems / All <Category> systems / one system" list for the
    // Systems page, handed over by Main.qml on open (`_buildSystemScopeEntries`)
    // so the sentinel scheme lives in one place.
    property var systemScopeEntries: []
    property int currentIndex: 0
    property bool _pressed: false

    // "form" or "systems": the Systems row swaps this panel's content to
    // its option list and back rather than opening a second modal (see
    // docs/style.md -> "Modal depth"). Back on the page returns to the form
    // with `currentIndex` untouched. `onOpenChanged` resets to "form".
    property string page: "form"
    readonly property bool onPickerPage: modal.page !== "form"

    readonly property int _rowSystems: 0
    readonly property int _rowStart: 1

    // Accept-button verb for the help bar, mirroring SettingsScreen's
    // `focusedActionLabel`. The bar describes the press, not the feature,
    // so it names what A does to the focused row rather than repeating
    // the modal's title. On the picker page A picks the focused entry.
    readonly property string focusedActionLabel: modal.onPickerPage ? qsTr("Select") : (modal.currentIndex === modal._rowStart ? qsTr("Start") : qsTr("Change"))

    // Same display convention as ScrapeSetupModal's identical property —
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
        modal.currentIndex = modal._rowSystems;
        modal.selectedSystemScope = "*";
        modal._pressed = false;
    }

    function handleAction(action: string): void {
        if (modal.onPickerPage) {
            // Back leaves the page, not the modal; up/down/accept drive the
            // list.
            if (action === "cancel")
                modal.page = "form";
            else if (action === "up" || action === "down" || action === "accept")
                pickerList.handleAction(action);
            return;
        }
        if (action === "cancel")
            modal.closeRequested();
        else if (action === "up")
            modal.currentIndex = Math.max(modal._rowSystems, modal.currentIndex - 1);
        else if (action === "down")
            modal.currentIndex = Math.min(modal._rowStart, modal.currentIndex + 1);
        else if (action === "accept") {
            if (modal.currentIndex === modal._rowSystems)
                modal._openPage("systems");
            else if (modal.currentIndex === modal._rowStart)
                modal._startIndex();
        }
    }

    // Entries and the initial focus are assigned, not bound, so both are in
    // place before `active` flips and the list re-applies them (see
    // ScrapeSetupModal's identical helper).
    function _openPage(target: string): void {
        pickerList.entries = modal.systemScopeEntries;
        pickerList.initialId = modal.selectedSystemScope;
        modal.page = target;
    }

    function _pickFromPage(id: string): void {
        modal.selectedSystemScope = id;
        modal.page = "form";
    }

    function _startIndex(): void {
        modal._pressed = true;
        startCommit.arm();
    }

    // Deferred like SettingsScreen's own field-commit: the row's own
    // push-in cue plays on the still-visible modal before the update
    // actually starts and the modal closes.
    DeferredAction {
        id: startCommit
        onDeferred: {
            modal._pressed = false;
            if (modal.selectedSystemScope === "" || modal.selectedSystemScope === "*")
                Browse.MediaStatus.start_index();
            else if (modal.selectedSystemScope.startsWith("cat:"))
                Browse.MediaStatus.start_index_for_systems(Browse.SystemsModel.system_ids_for_category(modal.selectedSystemScope.slice(4)));
            else
                Browse.MediaStatus.start_index_for_systems([modal.selectedSystemScope]);
            modal.closeRequested();
        }
    }

    Modal {
        id: shell

        open: modal.open
        kind: "shell"
        title: qsTr("Update media database")
        panelMaxWidth: Sizing.pctH(110)

        Column {
            width: parent.width
            spacing: Sizing.pctH(1.5)

            // Picker page: the row's own name as a section heading over the
            // list, under the unchanged modal title.
            SectionHeader {
                objectName: "setupPickerHeader"
                width: parent.width
                visible: modal.onPickerPage
                label: qsTr("Systems")
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

            SettingsField {
                width: parent.width
                visible: !modal.onPickerPage
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
                visible: !modal.onPickerPage
                label: qsTr("Start update")
                value: ""
                control: "action"
                isFocused: modal.currentIndex === modal._rowStart
                onHovered: modal.currentIndex = modal._rowStart
                onAccepted: {
                    modal.currentIndex = modal._rowStart;
                    modal._startIndex();
                }
            }
        }
    }
}
