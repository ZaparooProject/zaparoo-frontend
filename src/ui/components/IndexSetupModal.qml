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
// Two keyboard-navigable rows (`currentIndex` 0-1): Systems (opens the
// shared ListPickerModal via Main.qml — see `requestSystemScopePicker`),
// Start update (action). Chrome comes from the shared `Modal` shell, same
// as ScrapeSetupModal/LogUploadModal.
Item {
    id: modal

    property bool open: false
    // "*" (all systems), "cat:<Category>", or a single system id. Same
    // sentinel convention as ScrapeSetupModal — see Main.qml's
    // `_systemScopeAll`/`_buildSystemScopeEntries`.
    property string selectedSystemScope: "*"
    property int currentIndex: 0
    property bool _pressed: false

    readonly property int _rowSystems: 0
    readonly property int _rowStart: 1

    // Accept-button verb for the help bar, mirroring SettingsScreen's
    // `focusedActionLabel`. The bar describes the press, not the feature,
    // so it names what A does to the focused row rather than repeating
    // the modal's title.
    readonly property string focusedActionLabel: modal.currentIndex === modal._rowStart ? qsTr("Start") : qsTr("Change")

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
    // Bubbled to Main.qml (see MainLayout.qml's Loader wiring) — this
    // modal can't instantiate a second top-level modal itself; the
    // router stacks the shared ListPickerModal on top instead.
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
        modal.currentIndex = modal._rowSystems;
        modal.selectedSystemScope = "*";
        modal._pressed = false;
    }

    function handleAction(action: string): void {
        if (action === "cancel")
            modal.closeRequested();
        else if (action === "up")
            modal.currentIndex = Math.max(modal._rowSystems, modal.currentIndex - 1);
        else if (action === "down")
            modal.currentIndex = Math.min(modal._rowStart, modal.currentIndex + 1);
        else if (action === "accept") {
            if (modal.currentIndex === modal._rowSystems)
                modal.requestSystemScopePicker();
            else if (modal.currentIndex === modal._rowStart)
                modal._startIndex();
        }
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
