// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

import QtQuick

// LoadingIndicator with anti-flicker timing. Fast operations complete
// without showing anything; once the cue appears it remains up briefly
// so it does not flash for a single frame.
Item {
    id: root

    property bool active: false
    property int delayMs: 300
    property int minimumVisibleMs: 200
    property alias text: indicator.text
    property alias glyphSize: indicator.glyphSize
    readonly property bool showing: root._showing

    property bool _showing: false
    property double _shownAtMs: 0

    width: indicator.width
    height: indicator.height
    visible: root._showing

    onActiveChanged: root._sync()
    onDelayMsChanged: root._sync()
    onMinimumVisibleMsChanged: root._sync()
    Component.onCompleted: root._sync()

    function _show(): void {
        if (!root.active)
            return;
        hideTimer.stop();
        if (!root._showing) {
            root._shownAtMs = Date.now();
            root._showing = true;
        }
    }

    function _hide(): void {
        root._showing = false;
        root._shownAtMs = 0;
        hideTimer.stop();
    }

    function _hideWhenAllowed(): void {
        delayTimer.stop();
        if (!root._showing) {
            hideTimer.stop();
            return;
        }
        const elapsed = Math.max(0, Date.now() - root._shownAtMs);
        const remaining = Math.max(0, root.minimumVisibleMs - elapsed);
        if (remaining <= 0) {
            root._hide();
            return;
        }
        hideTimer.interval = remaining;
        hideTimer.restart();
    }

    function _sync(): void {
        if (root.active) {
            hideTimer.stop();
            if (root._showing)
                return;
            delayTimer.stop();
            if (root.delayMs <= 0) {
                root._show();
                return;
            }
            delayTimer.interval = root.delayMs;
            delayTimer.restart();
            return;
        }
        root._hideWhenAllowed();
    }

    Timer {
        id: delayTimer
        repeat: false
        onTriggered: root._show()
    }

    Timer {
        id: hideTimer
        repeat: false
        onTriggered: {
            if (!root.active)
                root._hide();
        }
    }

    LoadingIndicator {
        id: indicator
    }
}
