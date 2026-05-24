// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
pragma Singleton
import QtQuick

// Project-wide color and font constants.
// Never hardcode colors or font families inline — use these instead.
QtObject {
    property bool crtNativePath: false

    // Backgrounds
    readonly property color bgDeep: "#0f0f23"
    readonly property color bgPanel: "#1a1a35"
    readonly property color bgBar: "#0a0a15"
    // Card surface used for tile bodies in rows/grids. Sits a step
    // above bgPanel so a solid white icon+label silhouette has clear
    // contrast — the page bg pattern stays visible in the gaps between
    // tiles, and each tile reads as a self-contained chip.
    readonly property color surfaceCard: "#2a2a45"
    // Selected row fill. Cooler and darker than the amber accent so
    // text stays high-contrast while the accent bar remains the focus
    // cue layered on top.
    readonly property color selectionSurface: "#3a3a66"
    // Modal scrim — translucent black so the screen behind a modal
    // dims uniformly without a blur or shader pass.
    readonly property color scrim: "#cc000000"
    // Borders
    readonly property color borderSubtle: "#1a1a2e"
    readonly property color borderMid: "#404060"

    // Text
    readonly property color textPrimary: "#ffffff"
    readonly property color textLabel: "#888888"
    // Accent — static warm amber used for selection highlights.
    readonly property color accent: "#FFB347"
    // Fonts
    readonly property string fontUi: crtNativePath ? "MxPlus HP 100LX 6x8" : "Noto Sans"
    readonly property string fontMono: crtNativePath ? "MxPlus HP 100LX 6x8" : "monospace"
}
