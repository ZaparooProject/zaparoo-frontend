// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
// cxx-qt 0.8 patches `isFinal: true` on singleton properties but the
// qmltypes schema has no `isFinal` slot, so every read of a Browse
// singleton trips qmllint's "Member can be shadowed" check. Suppress
// the compiler category file-wide until the schema grows the slot.
// qmllint disable compiler
pragma ComponentBehavior: Bound

import QtQuick
import Zaparoo.Browse as Browse
import Zaparoo.Theme

// Header status line — replaces CoreStatusPill. Surfaces, in
// priority order, only one of: Core link state, an active background
// task (indexing / optimizing / scraping, including why it's paused), a
// terminal message held briefly after a task ends, a transient event
// (card scan / playtime warning / inbox message), or nothing. See
// docs/style.md -> "Header status line" for the full rationale and
// docs/qml-gotchas.md for why the leading-cell pulse is the one
// exception to no-persistent-motion.
//
// No card fill, no border, no radius: this is plain text (plus the
// track) directly on `Theme.bgDeep`, the same "no surface" treatment
// docs/style.md already sanctions for the global Loading cue and
// TopStatusStrip titles.
//
// Content is a right-aligned pair -- label, then track -- hugging the
// header's own right margin, not stretched across the full row the way
// the old pill's replacement first was. The track is the rightmost
// element; there is no trailing count next to it. That was tried (a
// step ratio, then an absolute file/record count) and cut both times:
// the bar already conveys progress on its own, same as the mobile app,
// and a separate number cell kept adding layout complexity -- a
// reserved-width void when the number was absent, a shifting anchor
// point when it wasn't -- for information nobody needed to read twice.
// A short message just sits closer to the right edge; the label
// shrink-wraps to its own content and only elides once it would run
// past the logo, so idle space moves to the left of the pair instead of
// sitting in the middle of the message, and long system names still get
// real room instead of forcing CRT-only abbreviations ("Idx…", "Scr…").
Item {
    id: root

    objectName: "statusLine"
    property bool mediaActivityEnabled: false

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

    // ---- Tier 1: Core connection ----
    readonly property string _connectionLabel: {
        const link = Browse.AppStatus.link_state ?? root._linkDisconnected;
        const conn = Browse.AppStatus.connection_state ?? root._linkDisconnected;

        if (link === root._linkUnreachable)
            return qsTr("Disconnected");
        if (link === root._linkReconnecting)
            return qsTr("Reconnecting…");
        if (link === root._linkDisconnected)
            return qsTr("Disconnected");
        if (link === root._linkConnecting)
            return qsTr("Connecting…");
        if (conn === root._connError) {
            return Browse.AppStatus.last_error !== "" ? qsTr("Core error — %1").arg(Browse.AppStatus.last_error) : qsTr("Core error");
        }
        return "";
    }
    readonly property bool _isError: Browse.AppStatus.link_state === root._linkUnreachable || Browse.AppStatus.connection_state === root._connError

    // ---- Tier 2: active background task ----
    readonly property bool _taskActive: root.mediaActivityEnabled && root._connectionLabel === "" && (Browse.MediaStatus.indexing || Browse.MediaStatus.optimizing || Browse.MediaStatus.scraping)

    readonly property string _taskLabel: {
        if (!root._taskActive)
            return "";
        if (Browse.MediaStatus.indexing) {
            if (Browse.MediaStatus.paused)
                return qsTr("Indexing paused — game running");
            // Core doesn't populate current_step_display until it has
            // actually picked a system/step to report -- most visible
            // right at the start of a run. A bare "Indexing: " with
            // nothing after the colon reads as broken, so fall back to
            // an ellipsis instead of assuming the detail is always there.
            return Browse.MediaStatus.current_step_display !== "" ? qsTr("Indexing: %1").arg(Browse.MediaStatus.current_step_display) : qsTr("Indexing…");
        }
        if (Browse.MediaStatus.optimizing)
            return Browse.MediaStatus.current_step_display !== "" ? qsTr("Optimizing database: %1").arg(Browse.MediaStatus.current_step_display) : qsTr("Optimizing database…");
        // Scraping is the only remaining `_taskActive` branch.
        if (Browse.MediaStatus.scrape_paused)
            return qsTr("Scraping paused — game running");
        return Browse.MediaStatus.scrape_current_step_display !== "" ? qsTr("Scraping: %1").arg(Browse.MediaStatus.scrape_current_step_display) : qsTr("Scraping…");
    }
    readonly property bool _taskPaused: Browse.MediaStatus.indexing ? Browse.MediaStatus.paused : Browse.MediaStatus.scrape_paused
    // Optimize/vacuum has no step count Core reports -- one cell marches
    // instead of a fraction filling. Indexing and scraping always carry
    // a step count once running (`totalSteps` guards the zero case too).
    readonly property bool _taskTotalKnown: !Browse.MediaStatus.optimizing
    readonly property int _taskCurrentStep: Browse.MediaStatus.indexing ? Browse.MediaStatus.current_step : Browse.MediaStatus.scrape_current_step
    readonly property int _taskTotalSteps: Browse.MediaStatus.indexing ? Browse.MediaStatus.total_steps : Browse.MediaStatus.scrape_total_steps

    // ---- Tier 3: terminal message, held briefly after a task ends ----
    // A plain computed binding can't hold a message past the moment its
    // own condition goes false, so this tier is the one piece of real
    // state here: latch a message on the busy→idle edge, clear it after
    // `_terminalDwellMs`. Scrape failure lands here too, not as its own
    // ladder tier -- Core's docs put `state: "failed"` only on the same
    // terminal (non-running) frame as a normal completion, so by the
    // time it's visible `scraping` has already gone false; there is no
    // separate "still running but failed" moment to give a tier of its
    // own.
    // Not `readonly` so tests can shrink the dwell and observe the real
    // Timer clear the message, rather than faking the edge by hand.
    property int _terminalDwellMs: 6000
    readonly property bool _indexBusy: Browse.MediaStatus.indexing || Browse.MediaStatus.optimizing
    readonly property bool _scrapeBusy: Browse.MediaStatus.scraping
    property bool _indexWasBusy: false
    property bool _scrapeWasBusy: false
    property string _terminalMessage: ""

    onMediaActivityEnabledChanged: {
        // Seed the "was busy" edges once activity is enabled (see
        // HeaderBar's own comment on the same startup-cost guard) so the
        // very first change afterward can't be misread as a fresh edge.
        root._indexWasBusy = root._indexBusy;
        root._scrapeWasBusy = root._scrapeBusy;
    }

    on_IndexBusyChanged: {
        if (!root._indexWasBusy && root._indexBusy) {
            root._terminalMessage = "";
            terminalMessageTimer.stop();
        } else if (root._indexWasBusy && !root._indexBusy) {
            root._terminalMessage = Browse.MediaStatus.total_files > 0 ? qsTr("Indexed %1 files").arg(Browse.MediaStatus.total_files) : qsTr("Indexing complete");
            terminalMessageTimer.restart();
        }
        root._indexWasBusy = root._indexBusy;
    }

    on_ScrapeBusyChanged: {
        if (!root._scrapeWasBusy && root._scrapeBusy) {
            root._terminalMessage = "";
            terminalMessageTimer.stop();
        } else if (root._scrapeWasBusy && !root._scrapeBusy) {
            root._terminalMessage = Browse.MediaStatus.scrape_state === "failed" ? qsTr("Scrape failed — %1").arg(Browse.MediaStatus.scrape_error) : qsTr("Scraped %1 of %2").arg(Browse.MediaStatus.scrape_matched).arg(Browse.MediaStatus.scrape_total);
            terminalMessageTimer.restart();
        }
        root._scrapeWasBusy = root._scrapeBusy;
    }

    Timer {
        id: terminalMessageTimer
        interval: root._terminalDwellMs
        onTriggered: root._terminalMessage = ""
    }

    // ---- Tier 4: transient event (card scan / playtime warning / inbox) ----
    property int _eventDwellMs: 5000
    property string _eventMessage: ""

    Connections {
        target: Browse.StatusEvents
        function onSequenceChanged() {
            // Dropped, not queued, while a higher tier owns the line --
            // these are informational and Core retains the underlying
            // record, so missing one here loses nothing the user can't
            // see again in Settings/Inbox.
            if (root._connectionLabel !== "" || root._taskActive || root._terminalMessage !== "")
                return;
            const detail = Browse.StatusEvents.detail;
            switch (Browse.StatusEvents.kind) {
            case "token_scanned":
                root._eventMessage = detail !== "" ? qsTr("Card scanned — %1").arg(detail) : qsTr("Card scanned");
                break;
            case "playtime_warning":
                root._eventMessage = qsTr("Playtime limit in %1").arg(detail);
                break;
            case "inbox_message":
                root._eventMessage = detail;
                break;
            default:
                return;
            }
            eventMessageTimer.restart();
        }
    }

    Timer {
        id: eventMessageTimer
        interval: root._eventDwellMs
        onTriggered: root._eventMessage = ""
    }

    // ---- Ladder resolution ----
    readonly property string _label: {
        if (root._connectionLabel !== "")
            return root._connectionLabel;
        if (root._taskActive)
            return root._taskLabel;
        if (root._terminalMessage !== "")
            return root._terminalMessage;
        return root._eventMessage;
    }
    readonly property bool _showTrack: root._taskActive

    visible: root._label !== ""
    height: root.visible ? Sizing.headerRowHeight : 0

    readonly property int _cellsSpacing: Sizing.pctW(1)
    // Space the track (plus its one flanking gap) claims from the label's
    // budget, when shown. `track.width` is a fixed internal size
    // regardless of `visible`, so this has to collapse to 0 here rather
    // than by relying on the track's own geometry shrinking -- the label
    // targets `parent.right` directly below for the same reason: chaining
    // its anchor through `track.left` would leave it stranded
    // `track.width` short of the right edge even while the track is
    // hidden.
    readonly property int _trackReserve: root._showTrack ? track.width + root._cellsSpacing : 0
    readonly property int _labelNaturalWidth: Math.ceil(Math.max(labelMetrics.advanceWidth, labelMetrics.boundingRect.width))
    readonly property int _labelWidth: Math.min(root._labelNaturalWidth, Math.max(0, root.width - root._trackReserve))

    TextMetrics {
        id: labelMetrics
        text: root._label
        font.family: Theme.fontUi
        font.pixelSize: Sizing.fontSmall
    }

    ProgressTrack {
        id: track
        objectName: "statusLineTrack"
        anchors.right: parent.right
        anchors.verticalCenter: parent.verticalCenter
        visible: root._showTrack
        active: root._showTrack
        paused: root._taskPaused
        totalKnown: root._taskTotalKnown
        currentStep: root._taskCurrentStep
        totalSteps: root._taskTotalSteps
    }

    // Shrink-wraps to its own content (`width` is the natural measured
    // width, not the full available span) so it hugs the track instead of
    // leaving a gap -- any slack space lands to the left of this item
    // instead, between the pair and the logo. Only caps down to
    // `_labelWidth`'s budget, and only then elides, once the pair as a
    // whole would otherwise run past the logo.
    Text {
        objectName: "statusLineLabel"
        anchors.right: parent.right
        anchors.rightMargin: root._trackReserve
        anchors.verticalCenter: parent.verticalCenter
        width: root._labelWidth
        elide: Text.ElideRight
        text: root._label
        font.family: Theme.fontUi
        font.pixelSize: Sizing.fontSmall
        color: root._isError ? Theme.error : Theme.textPrimary
        renderType: Text.NativeRendering
    }
}
