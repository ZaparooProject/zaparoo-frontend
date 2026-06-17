// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

import QtQuick
import Zaparoo.Theme

// Non-visual helper that sequences a press cue ahead of a dispatch.
// The host fires its tile pulse counter (which starts the push-in animation),
// then calls `arm()`. The `deferred` signal fires after `Motion.pressMs` —
// long enough for the tile push-in's downward leg to complete on a static
// scene before the navigation starts. That lead sits inside the existing
// 300 ms loading-overlay grace window (MainLayout.loadingIndicatorDelayMs),
// so it adds no perceptible navigation latency in the common case.
//
// Usage: declare an `onDeferred:` handler on the DeferredAction instance
// (evaluated in the outer screen's scope, so all screen ids are accessible),
// then call `arm()` in the accept path:
//
//   DeferredAction {
//       id: pressCommit
//       onDeferred: hub._emitActivate()
//   }
//   ...
//   hub.activatePulse++;
//   pressCommit.arm();
//
// Re-arming while the timer is running is a no-op — debounces a double-accept
// inside the lead window since the router's input gate only closes after the
// `onDeferred` handler sets `pendingTransition`. Under reduce-motion the
// `deferred` signal fires synchronously with no event-loop hop.
Timer {
    id: ctl

    signal deferred
    interval: Motion.dur(Motion.pressMs)
    repeat: false

    onTriggered: ctl.deferred()

    function arm(): void {
        if (ctl.running)
            return;
        if (!Motion.enabled) {
            ctl.deferred();
            return;
        }
        ctl.restart();
    }
}
