// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
pragma Singleton
import QtQuick

// Project-wide color and font constants.
// Never hardcode colors or font families inline — use these instead.
QtObject {
    property bool crtNativePath: false
    property string colorSchemeId: ColorSchemes.defaultId

    readonly property string effectiveColorSchemeId: ColorSchemes.effectiveId(colorSchemeId)
    readonly property var _palette: ColorSchemes.palette(colorSchemeId)

    // Semantic roles resolve through one complete preset. Components never
    // branch on scheme identity, so switching updates surfaces and tinted image
    // URLs through ordinary QML bindings.
    readonly property color bgDeep: _palette.bgDeep
    readonly property color bgPanel: _palette.bgPanel
    readonly property color bgBar: _palette.bgBar
    readonly property color surfaceCard: _palette.surfaceCard
    readonly property color selectionSurface: _palette.selectionSurface
    readonly property color selectionShade: _palette.selectionShade
    readonly property color tileEdge: _palette.tileEdge
    readonly property color controlEdge: _palette.controlEdge
    readonly property color scrim: _palette.scrim
    readonly property color borderSubtle: _palette.borderSubtle
    readonly property color borderMid: _palette.borderMid
    readonly property color textPrimary: _palette.textPrimary
    readonly property color textLabel: _palette.textLabel
    readonly property color textVariant: _palette.textVariant
    readonly property color accent: _palette.accent
    readonly property color logoPrimary: _palette.logoPrimary
    readonly property color logoSecondary: _palette.logoSecondary
    readonly property color logoShadow: _palette.logoShadow
    readonly property color logoFocusPrimary: _palette.logoFocusPrimary
    readonly property color logoFocusSecondary: _palette.logoFocusSecondary
    readonly property color logoFocusShadow: _palette.logoFocusShadow
    readonly property string errorHex: _palette.errorHex
    readonly property color error: errorHex
    // Fonts
    readonly property string fontUi: crtNativePath ? "MxPlus HP 100LX 6x8" : "Noto Sans"
    readonly property string fontMono: crtNativePath ? "MxPlus HP 100LX 6x8" : "monospace"
}
