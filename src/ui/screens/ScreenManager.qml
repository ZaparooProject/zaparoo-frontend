// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

pragma Singleton

import QtQuick

// App-wide screen + modal routing. Screens are identified by plain strings
// so a new screen can be added without a central enum; see HubScreen.qml
// and GamesScreen.qml for the current registrants. Modal overlays push
// onto the stack; the top is the one currently receiving input.
QtObject {
    id: manager

    // Screen name constants. Re-exported by MainLayout for test back-compat.
    readonly property string screenHub: "hub"
    readonly property string screenSystems: "systems"
    readonly property string screenGames: "games"
    readonly property string screenFavorites: "favorites"
    readonly property string screenFavoriteSystems: "favoritesystems"
    readonly property string screenRecents: "recents"
    readonly property string screenUpdate: "update"
    readonly property string screenSettings: "settings"
    readonly property string screenAbout: "about"

    // Currently-focused root screen. Persistence lives in
    // Browse.AppState — write there via Main.qml's orchestration, not
    // here, so we stay decoupled from the persistence layer.
    property string activeScreen: manager.screenHub

    // Modal stack — transient. Top of stack receives input; empty stack
    // means the active root screen handles input.
    property list<string> modalStack: []

    readonly property int modalCount: manager.modalStack.length
    readonly property bool hasModal: manager.modalStack.length > 0
    readonly property string topModal: manager.modalStack.length > 0 ? manager.modalStack[manager.modalStack.length - 1] : ""

    function go(screen: string): void {
        manager.activeScreen = screen;
    }

    // One modal at a time. The only thing allowed above an open modal is an
    // `action_error` alert (docs/style.md -> "Modal depth"); a modal that
    // needs a choice made swaps its own panel to the list instead. Anything
    // else stacking here is a bug, so say so where the frontend log shows
    // it and where a test's `failOnWarning` can catch it.
    function pushModal(name: string): void {
        if (manager.modalStack.length > 0 && name !== "action_error")
            console.warn("ScreenManager: modal '" + name + "' pushed over '" + manager.topModal + "' (see docs/style.md, Modal depth)");
        manager.modalStack = manager.modalStack.concat([name]);
    }

    function popModal(): void {
        if (manager.modalStack.length > 0)
            manager.modalStack = manager.modalStack.slice(0, manager.modalStack.length - 1);
    }
}
