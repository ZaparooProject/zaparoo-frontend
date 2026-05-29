// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
pragma Singleton
import QtQuick

// Resolution-agnostic sizing helpers.
// All UI elements must use these functions rather than hardcoded pixel values.
// The UI must run correctly from 240p (CRT) through 1080p.
QtObject {
    id: root

    // Reference window dimensions — updated by Main.qml on start and resize.
    property real screenWidth: 640
    property real screenHeight: 480
    property bool crtNativePath: false
    property bool swapPercentageAxes: false

    // Visible tile-row covers: fewer at very low resolution to avoid crowding.
    readonly property int visibleCovers: screenHeight < 300 ? 3 : 5
    // Paged grid shape: chosen by screen height so the same grid reads
    // sensibly from MiSTer 240p through 1080p. Width × height of one
    // page in tiles; product is the page size used by `PagedGrid`.
    readonly property int gridColumns: screenHeight < 300 ? 3 : screenHeight < 600 ? 4 : 5
    readonly property int gridRows: screenHeight < 300 ? 2 : 3
    // Games grid shape — taller per-tile cover art than systems logos
    // means a 5x3 layout starves vertical space, so games use 5x2 on
    // desktop. The 240p MiSTer branch keeps a 3x2 layout for parity
    // with the other rows on a small screen.
    readonly property int gamesGridColumns: screenHeight < 300 ? 3 : screenHeight < 600 ? 4 : 5
    readonly property int gamesGridRows: 2
    // Standard corner radius for rounded surfaces — tile cards, focus
    // rings (computed as `cornerRadius - outlineGap`), settings rows.
    // Pill controls (toggle track/thumb) use `height/2` instead and
    // are intentionally a different shape. See docs/style.md.
    readonly property int cornerRadius: pctH(3.5)
    // ── Top header (logo + status row + status pill) ──────────────────
    // Single source of truth for the header bar that sits at the top of
    // every screen. The logo's height is locked to the stacked-row
    // total so the brand mark sits flush with the top of the status
    // row and the bottom of the pill row, even when the pill is idle
    // (its space is reserved). Screen content clears `headerBottom`.
    readonly property int headerRowHeight: fontSize(3.4)
    readonly property int headerStackGap: pctH(0.8)
    readonly property int headerTopMargin: pctH(2)
    readonly property int headerSideMargin: pctW(2)
    readonly property int headerHeight: 2 * headerRowHeight + headerStackGap
    readonly property int headerBottom: headerTopMargin + headerHeight

    function pctH(percent: real): int {
        return Math.round((swapPercentageAxes ? screenWidth : screenHeight) * percent / 100);
    }

    function pctW(percent: real): int {
        return Math.round((swapPercentageAxes ? screenHeight : screenWidth) * percent / 100);
    }

    function px(value: real): int {
        return Math.round(value);
    }

    function stroke(value: real): int {
        return Math.max(1, px(value));
    }

    function center(parentSize: real, childSize: real): int {
        return px((parentSize - childSize) / 2);
    }

    function half(value: real): int {
        return px(value / 2);
    }

    // Minimum 8px to remain legible on CRT 240p displays.
    function fontSize(percent: real): int {
        const size = Math.max(8, pctH(percent));
        if (!crtNativePath)
            return size;
        return size < 12 ? 8 : 16;
    }
}
