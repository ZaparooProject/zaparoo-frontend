// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

import QtQuick
import QtTest
import Zaparoo.Theme
import Zaparoo.Ui

TestCase {
    id: testCase
    name: "UiScreenStateOverlay"
    when: windowShown
    width: 640
    height: 480
    visible: true

    function initTestCase(): void {
        Sizing.screenWidth = testCase.width;
        Sizing.screenHeight = testCase.height;
    }

    // Stand-in for `scene`: the one coordinate space the global transition
    // cue (Main.qml) and every screen's ScreenStateOverlay must agree on.
    Item {
        id: windowSpace
        anchors.fill: parent

        // Stand-in for a screen whose content rect (e.g. a media grid)
        // starts below a header bar, exactly like MediaListScreen /
        // SystemsScreen. The overlay below is parented here, not directly
        // under `windowSpace`, so its own (0, 0) is offset from window
        // space by `headerHeight`.
        Item {
            id: screenContent
            readonly property int headerHeight: 64
            x: 0
            y: headerHeight
            width: windowSpace.width
            height: windowSpace.height - headerHeight
        }
    }

    ScreenStateOverlay {
        id: overlay
        parent: screenContent
        anchors.fill: parent
        loading: true
        loadingDelayMs: 0
        minimumLoadingVisibleMs: 0
    }

    function _cueCenterInWindowSpace(): real {
        const cue = findChild(overlay, "screenStateLoadingCue");
        verify(cue !== null);
        const mapped = cue.mapToItem(windowSpace, 0, 0);
        return mapped.y + cue.height / 2;
    }

    function init(): void {
        overlay.cueCenterY = overlay.height / 2;
    }

    // One pixel of slack: `y: Sizing.px(cueCenterY - height / 2)` snaps to
    // the nearest integer, so when the cue's own height is odd the
    // recovered center is off by half a pixel from the real-valued target
    // -- expected px-grid rounding, not a defect in the centering itself.
    function _verifyNear(actual: real, expected: real, what: string): void {
        verify(Math.abs(actual - expected) <= 1, what + ": expected ~" + expected + ", got " + actual);
    }

    // With no override, the cue centers on the overlay's own rect -- the
    // pre-existing behavior, still correct for a caller whose content rect
    // is not offset from its container (e.g. SettingsScreen).
    function test_default_cue_centers_on_overlay_rect(): void {
        _verifyNear(_cueCenterInWindowSpace(), screenContent.y + overlay.height / 2, "default center");
    }

    // `cueCenterY` is overlay-local: setting it directly positions the
    // cue's vertical center at that offset from the overlay's own top.
    function test_cue_center_y_positions_cue_at_explicit_offset(): void {
        overlay.cueCenterY = 40;
        _verifyNear(_cueCenterInWindowSpace(), screenContent.y + 40, "explicit offset");
    }

    // The actual regression this exists to catch: a screen whose content
    // rect is inset below a header (MediaListScreen/SystemsScreen's grid)
    // must wire `cueCenterY: windowHeight / 2 - y` so its own loading cue
    // lands at the exact window-space y the global transition cue (which
    // always centers on the full window/scene rect) centers on. Without
    // the override the cue would center on the smaller inset rect instead
    // and visibly jump when the global cue hands off to this one.
    function test_offset_content_rect_cue_matches_full_window_center(): void {
        overlay.cueCenterY = windowSpace.height / 2 - screenContent.y;
        _verifyNear(_cueCenterInWindowSpace(), windowSpace.height / 2, "window-space match");
    }
}
