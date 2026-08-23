// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

import QtQuick
import QtTest
import Zaparoo.Theme

// Motion singleton contract: dur() returns the raw ms when enabled and 0
// when disabled. Token values must match the design-spec constants.
TestCase {
    name: "Motion"
    when: windowShown

    function init(): void {
        Motion.enabled = true;
    }

    function cleanup(): void {
        Motion.enabled = true;
    }

    function test_dur_returns_ms_when_enabled(): void {
        Motion.enabled = true;
        compare(Motion.dur(Motion.pressMs), Motion.pressMs, "dur(pressMs) must equal pressMs when enabled");
        compare(Motion.dur(Motion.settleMs), Motion.settleMs, "dur(settleMs) must equal settleMs when enabled");
        compare(Motion.dur(140), 140, "dur(140) must equal 140 when enabled");
    }

    function test_dur_returns_zero_when_disabled(): void {
        Motion.enabled = false;
        compare(Motion.dur(Motion.pressMs), 0, "dur(pressMs) must be 0 when disabled");
        compare(Motion.dur(Motion.settleMs), 0, "dur(settleMs) must be 0 when disabled");
        compare(Motion.dur(140), 0, "dur(140) must be 0 when disabled");
    }

    function test_token_sanity(): void {
        // Tokens must be in the expected design ranges. `pressMs` doubles as
        // `DeferredAction`'s hold time (see Motion.qml's comment) — its floor
        // is one rendered frame at MiSTer's ~30fps (33.3ms), not a
        // perceptual-smoothness floor like `settleMs`'s, so its valid range
        // is deliberately much tighter and much lower.
        verify(Motion.pressMs >= 34 && Motion.pressMs <= 50, "pressMs should be 34-50ms");
        verify(Motion.settleMs >= 100 && Motion.settleMs <= 200, "settleMs should be 100-200ms");
    }

    function test_enabled_toggle_round_trip(): void {
        Motion.enabled = true;
        verify(Motion.enabled, "enabled should be true after setting true");
        Motion.enabled = false;
        verify(!Motion.enabled, "enabled should be false after setting false");
        Motion.enabled = true;
        verify(Motion.enabled, "enabled should be true after toggling back");
    }
}
