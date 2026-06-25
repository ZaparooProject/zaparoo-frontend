// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

import QtQuick
import Zaparoo.Update

Item {
    id: updateEntry

    property bool transitioning: false
    readonly property bool allowsScreensaver: updateScreen.allowsScreensaver
    readonly property var helpEntries: updateScreen.helpEntries

    signal requestHubScreen

    function handleAction(action: string): void {
        updateScreen.handleAction(action);
    }

    UpdateScreen {
        id: updateScreen

        anchors.fill: parent
        transitioning: updateEntry.transitioning
        onRequestHubScreen: updateEntry.requestHubScreen()
    }
}
