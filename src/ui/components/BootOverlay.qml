// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

import QtQuick
import Zaparoo.Browse as Browse
import Zaparoo.Theme

// cxx-qt 0.8 patches `isFinal: true` on singleton properties but the
// qmltypes schema has no `isFinal` slot, so every read trips qmllint's
// "Member can be shadowed" check. Until the schema grows the slot,
// suppress the compiler category file-wide.
// qmllint disable compiler

// Cold-launch curtain. The frontend process starts before Core finishes
// listening, and the very first frames of the Hub paint with a still-
// loading catalog (categories empty, status icons stale). Rather than
// expose that mid-construction frame to the user we keep the screens
// hidden until the catalog is Ready and let this component own the
// window: the global background (`bgDeep` + circuit-trace tile) that
// `MainLayout` already paints provides the surface, and the overlay
// simply adds a single centred `LoadingIndicator` on top — same icon,
// font, and baseline as every other loading cue in the app.
//
// Per `feedback_never_paint_background`, the overlay does **not** add a
// new full-screen fill. The existing `MainLayout` background paints
// through. Only the host screens are hidden (their `visible` flips off
// via `root.bootComplete`).
//
// Dismissal is one-shot — `MainLayout` removes the overlay from the
// scene graph the moment `connection_state` first reaches READY, so a
// subsequent disconnect surfaces only via the status pill and never
// re-asserts itself over the user's now-loaded catalog.
Item {
    id: overlay

    // Link-state constants mirror rust/frontend/src/models/app_status.rs:
    //   0 DISCONNECTED · 1 CONNECTING · 2 CONNECTED · 3 RECONNECTING · 4 UNREACHABLE.
    readonly property int _linkDisconnected: 0
    readonly property int _linkConnecting: 1
    readonly property int _linkConnected: 2
    readonly property int _linkReconnecting: 3
    readonly property int _linkUnreachable: 4

    // Tracks elapsed time on UNREACHABLE so the message escalates after
    // a few seconds instead of immediately. A flicker on first connect
    // — TCP open then a transient probe failure — wouldn't otherwise be
    // worth scaring the user about.
    property bool _unreachableLong: false

    Connections {
        target: Browse.AppStatus
        function onLink_stateChanged(): void {
            if (Browse.AppStatus.link_state === overlay._linkUnreachable) {
                unreachableEscalateTimer.restart();
            } else {
                unreachableEscalateTimer.stop();
                overlay._unreachableLong = false;
            }
        }
    }

    Timer {
        id: unreachableEscalateTimer
        interval: 5000
        repeat: false
        onTriggered: overlay._unreachableLong = true
    }

    LoadingIndicator {
        x: Sizing.center(parent.width, width)
        y: Sizing.center(parent.height, height)
        text: {
            const link = Browse.AppStatus.link_state ?? overlay._linkDisconnected;
            if (link === overlay._linkUnreachable && overlay._unreachableLong)
                return qsTr("Can't reach Zaparoo Core. Check your connection.");
            if (link === overlay._linkReconnecting)
                return qsTr("Reconnecting…");
            if (link === overlay._linkConnected)
                return qsTr("Loading library…");
            // DISCONNECTED, CONNECTING, and the first few seconds of
            // UNREACHABLE all read the same — the user pressed launch,
            // we're trying to reach Core. Don't blame the network until
            // we're sure.
            return qsTr("Connecting to Zaparoo Core…");
        }
    }
}
