// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

import QtQuick
import QtTest
import Zaparoo.Theme
import Zaparoo.Ui

// DeferredAction contract:
// - With motion disabled, arm() emits deferred synchronously (no event-loop hop).
// - With motion enabled, arm() emits deferred after Motion.pressMs.
// - Re-arming while the timer is running is a no-op (deferred fires exactly once).
TestCase {
    name: "DeferredAction"
    when: windowShown

    DeferredAction {
        id: subject
        property int deferredCount: 0
        onDeferred: deferredCount++
    }

    function init(): void {
        subject.deferredCount = 0;
        Motion.enabled = true;
    }

    function cleanup(): void {
        Motion.enabled = true;
    }

    function test_instant_when_motion_disabled(): void {
        Motion.enabled = false;
        subject.arm();
        compare(subject.deferredCount, 1, "arm() must emit deferred synchronously when motion is disabled");
    }

    function test_deferred_when_motion_enabled(): void {
        Motion.enabled = true;
        subject.arm();
        compare(subject.deferredCount, 0, "arm() must not emit deferred synchronously when motion is enabled");
        wait(Motion.pressMs + 50);
        compare(subject.deferredCount, 1, "arm() must have emitted deferred after pressMs elapsed");
    }

    function test_rearm_while_running_is_dropped(): void {
        Motion.enabled = true;
        subject.arm();
        subject.arm();
        wait(Motion.pressMs + 50);
        compare(subject.deferredCount, 1, "deferred must fire exactly once when re-armed while running");
    }
}
