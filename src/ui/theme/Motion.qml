// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
pragma Singleton
import QtQuick

// Motion tokens — durations and the global on/off switch
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

    // Duration buckets (milliseconds). The practical floor here is the frame
    // budget, not perception: on MiSTer's software renderer (~30fps) motion the
    // eye tracks needs ~3 frames (~100ms) to read as smooth rather than a
    // two-frame jump. So `settleMs` (tracked motion) stays above that floor,
    // while `pressMs` can sit a little under it because it reads as a punchy
    // tactile snap, not tracked motion. Don't drop these much further or the
    // cues turn choppy on hardware.
    // `pressMs`  — push-in feedback on accept/activate.
    // `settleMs` — settle/release legs and the toggle-knob slide.
    readonly property int pressMs: 80
    readonly property int settleMs: 110
    // How long `ProgressTrack`'s leading cell holds each on/off state of
    // its blink (~2 Hz full cycle). This is a hard cut, not a fade — a
    // Timer flips a bool every `pulseMs` and `color` reads it directly,
    // no ColorAnimation or Behavior involved, the same instant-swap idiom
    // `SelectionBar`'s inverse-video flash already uses. It is the one
    // exception to "no persistent motion": a background task's progress
    // cue in header chrome, never over content, gated on `enabled` and on
    // the task actually running — the same small-dirty-rect page-dot/
    // focus-ring-blink exemption CLAUDE.md already carves out, just
    // continuous instead of one-shot because there is no natural "done"
    // edge mid-task. See ProgressTrack.qml's doc comment and
    // docs/style.md -> "Header status line" for the write-up.
    readonly property int pulseMs: 250

    // Collapse all durations to 0 under reduce-motion so Behaviors that
    // use dur() resolve instantly without per-call branching.
    function dur(ms: int): int {
        return enabled ? ms : 0;
    }
}
