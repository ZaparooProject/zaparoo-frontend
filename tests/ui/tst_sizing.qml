// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

import QtQuick
import QtTest
import Zaparoo.Theme
import Zaparoo.App

// Resolution-agnostic sizing contract: pctH/pctW/fontSize must scale with the
// Main window's screenWidth/screenHeight, and visibleCovers must honour the
// 240p special-case (3 covers instead of 5).
TestCase {
    name: "UiSizing"
    when: windowShown

    Main {
        id: main
        fullScreen: false
        width: 1280
        height: 720
    }

    function setResolution(w: int, h: int): void {
        main.width = w;
        main.height = h;
        // Main.qml's onWidthChanged/onHeightChanged propagate to Sizing.
        tryCompare(Sizing, "screenWidth", w);
        tryCompare(Sizing, "screenHeight", h);
    }

    function test_pct_helpers_scale_with_window_size(): void {
        setResolution(1920, 1080);
        compare(Sizing.pctH(10), 108);
        compare(Sizing.pctW(50), 960);
        compare(Sizing.pctH(100), 1080);

        setResolution(1280, 720);
        compare(Sizing.pctH(10), 72);
        compare(Sizing.pctW(50), 640);

        setResolution(320, 240);
        compare(Sizing.pctH(10), 24);
        compare(Sizing.pctW(50), 160);
    }

    function test_font_size_respects_minimum_for_240p(): void {
        setResolution(320, 240);
        // pctH(2) would be 5 at 240p, but fontSize clamps to 8.
        verify(Sizing.fontSize(2) >= 8, "fontSize must never fall below 8px for CRT legibility");
        // A larger percent still scales above the floor.
        compare(Sizing.fontSize(10), 24);
    }

    function test_visible_covers_drops_to_three_on_240p(): void {
        setResolution(320, 240);
        compare(Sizing.visibleCovers, 3);

        setResolution(1280, 720);
        compare(Sizing.visibleCovers, 5);

        setResolution(1920, 1080);
        compare(Sizing.visibleCovers, 5);
    }

    function test_sizing_updates_propagate_proportionally(): void {
        setResolution(1280, 720);
        var baseline = Sizing.pctH(10);
        compare(baseline, 72);

        setResolution(1920, 1080);
        var scaled = Sizing.pctH(10);
        // 1080/720 = 1.5 → 72 * 1.5 = 108. Allow ±1px for rounding.
        verify(Math.abs(scaled - baseline * 1.5) <= 1, "pctH scaling should track screen height proportionally");
    }
}
