// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
pragma Singleton
import QtQuick

// Motion tokens — durations, scale targets, and the global on/off switch
// for all interaction animations.
//
// `enabled` is written by the app layer from the persisted reduce-motion
// setting (see `Main.qml` for the Binding). Keeping this singleton
// dependency-free means `Zaparoo.Theme` does not depend on `Zaparoo.Browse`.
//
// When `enabled` is false, `dur()` returns 0 and every Behavior that reads
// `Motion.enabled` is inert — animations complete in one frame with no
// branching in the consuming code.
QtObject {
    // Master switch. Written from Main.qml via a Binding.
    property bool enabled: true

    // Duration buckets (milliseconds).
    // `pressMs` sits at the ~90ms perceivable-motion floor — feels
    // instant but registers as tactile feedback.
    // `settleMs` covers small-element motion (100-200ms band).
    readonly property int pressMs: 90
    readonly property int settleMs: 140

    // Scale target for the one-shot tile push-in cue, shared by every
    // button-like action (navigation and game launch alike).
    readonly property real pressScale: 0.96

    // Collapse all durations to 0 under reduce-motion so Behaviors that
    // use dur() resolve instantly without per-call branching.
    function dur(ms: int): int {
        return enabled ? ms : 0;
    }
}
