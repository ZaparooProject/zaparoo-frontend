// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
// cxx-qt 0.8 patches `isFinal: true` on singleton properties but the
// qmltypes schema has no `isFinal` slot, so every read trips qmllint's
// "Member can be shadowed" check. Until the schema grows the slot,
// suppress the compiler category file-wide.
// qmllint disable compiler
// Single mutually-exclusive status pill anchored in the top-right HUD.
// Surfaces — in priority order, only one ever paints — Core link state
// (disconnected / connecting / error), media-database build progress
// (indexing / optimizing / paused), and scraper progress.
// When no condition is interesting the pill is `visible: false` and
// `width: 0`, so the surrounding Row collapses around it and the rest
// of the HUD slides flush to the right edge.

import QtQuick
import Zaparoo.Browse as Browse
import Zaparoo.Theme

// Software-renderer safe: a single Rectangle with two Text children,
// no animations, no shaders. Counter and step text update via plain
// property bindings so the dirty rect is bounded to the pill.
Item {
    id: pill

    // Link-state constants mirror rust/frontend/src/models/app_status.rs:
    //   0 DISCONNECTED · 1 CONNECTING · 2 CONNECTED · 3 RECONNECTING · 4 UNREACHABLE.
    // Catalog connection_state constants:
    //   0 DISCONNECTED · 1 CONNECTING · 2 READY · 3 ERROR.
    readonly property int _linkDisconnected: 0
    readonly property int _linkConnecting: 1
    readonly property int _linkConnected: 2
    readonly property int _linkReconnecting: 3
    readonly property int _linkUnreachable: 4
    readonly property int _connError: 3
    // Resolved (priority, label) for the current state. Empty `text`
    // means "nothing interesting is happening" and the pill hides.
    readonly property string _label: {
        const link = Browse.AppStatus.link_state ?? pill._linkDisconnected;
        const conn = Browse.AppStatus.connection_state ?? pill._linkDisconnected;
        // Priority 1 — Core link state.
        if (link === pill._linkUnreachable)
            return qsTr("Disconnected");

        if (link === pill._linkReconnecting)
            return qsTr("Reconnecting…");

        if (link === pill._linkDisconnected)
            return qsTr("Disconnected");

        if (link === pill._linkConnecting)
            return qsTr("Connecting…");

        if (conn === pill._connError)
            return qsTr("Core error");

        // Priority 2 — media database.
        if (Browse.MediaStatus.optimizing)
            return qsTr("Optimizing database");

        if (Browse.MediaStatus.indexing) {
            const cur = Browse.MediaStatus.current_step;
            const tot = Browse.MediaStatus.total_steps;
            const display = Browse.MediaStatus.current_step_display;
            if (Browse.MediaStatus.paused)
                return qsTr("Indexing paused");

            if (tot > 0 && display !== "")
                return qsTr("Indexing %1/%2 - %3").arg(cur).arg(tot).arg(display);

            if (tot > 0)
                return qsTr("Indexing %1/%2").arg(cur).arg(tot);

            return qsTr("Indexing…");
        }
        // Priority 3 — scraper.
        if (Browse.MediaStatus.scraping) {
            const cur = Browse.MediaStatus.scrape_current_step;
            const tot = Browse.MediaStatus.scrape_total_steps;
            const display = Browse.MediaStatus.scrape_current_step_display;
            if (Browse.MediaStatus.scrape_paused)
                return qsTr("Scrape paused");

            if (tot > 0 && display !== "")
                return qsTr("Scraping %1/%2 - %3").arg(cur).arg(tot).arg(display);

            if (tot > 0)
                return qsTr("Scraping %1/%2").arg(cur).arg(tot);

            return qsTr("Scraping…");
        }
        return "";
    }
    // Border colour leans on the same convention as the old connection
    // strip: warmer accent for error-class link states, muted otherwise.
    readonly property bool _isError: Browse.AppStatus.link_state === pill._linkUnreachable || Browse.AppStatus.connection_state === pill._connError

    visible: pill._label !== ""
    height: pill.visible ? Sizing.fontSize(3.4) : 0
    width: pill.visible ? Sizing.px(labelText.implicitWidth + Sizing.pctW(2.4)) : 0

    Rectangle {
        anchors.fill: parent
        color: Theme.surfaceCard
        radius: Sizing.half(pill.height)
        border.width: Sizing.stroke(1)
        border.color: pill._isError ? Theme.accent : Theme.borderMid

        Text {
            id: labelText

            anchors.verticalCenter: parent.verticalCenter
            anchors.left: parent.left
            anchors.leftMargin: Sizing.pctW(1.2)
            anchors.right: parent.right
            anchors.rightMargin: Sizing.pctW(1.2)
            elide: Text.ElideRight
            horizontalAlignment: Text.AlignHCenter
            text: pill._label
            font.family: Theme.fontUi
            font.pixelSize: Sizing.fontSize(2.2)
            color: Theme.textPrimary
            renderType: Text.NativeRendering
        }
    }
}
