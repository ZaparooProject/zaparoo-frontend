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

    // CRT native path. Bound from MainLayout (like Theme/Sizing) so this
    // singleton stays dependency-free (no Theme import). At 240p the default
    // press scales move an edge by under a pixel, so the push-in cue is
    // deepened on this path to read as a tactile snap.
    property bool crtNativePath: false

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

    // Scale target for the one-shot push-in cue on squarish surfaces — tiles
    // and dialog buttons. Deeper on the CRT path so a sub-pixel HD nudge
    // becomes a visible snap at 240p.
    readonly property real pressScale: crtNativePath ? 0.90 : 0.96
    // Gentler target for wide, short rows (list-detail rows, settings fields,
    // menu/picker rows). The same scale factor moves a full-width row's edges
    // far more than a squarish tile's, so a wider row needs a value closer to
    // 1.0 to read as the same subtle press. Same CRT deepening as above.
    readonly property real rowPressScale: crtNativePath ? 0.95 : 0.985

    // Collapse all durations to 0 under reduce-motion so Behaviors that
    // use dur() resolve instantly without per-call branching.
    function dur(ms: int): int {
        return enabled ? ms : 0;
    }
}
