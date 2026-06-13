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
    readonly property color surfaceCard: "#22223a"
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
    // System logo tint tokens. Source logo tones pass through a
    // monotonic neutral grade: lifted shadow, near-white midtone,
    // and crisp white highlight. Single-tone logos render as white.
    readonly property color logoPrimary: textPrimary
    readonly property color logoShadow: Qt.rgba(surfaceCard.r * 0.38 + textPrimary.r * 0.62, surfaceCard.g * 0.38 + textPrimary.g * 0.62, surfaceCard.b * 0.38 + textPrimary.b * 0.62, 1)
    readonly property color logoSecondary: Qt.rgba(logoShadow.r * 0.18 + textPrimary.r * 0.82, logoShadow.g * 0.18 + textPrimary.g * 0.82, logoShadow.b * 0.18 + textPrimary.b * 0.82, 1)
    // Fonts
    readonly property string fontUi: crtNativePath ? "MxPlus HP 100LX 6x8" : "Noto Sans"
    readonly property string fontMono: crtNativePath ? "MxPlus HP 100LX 6x8" : "monospace"
}
