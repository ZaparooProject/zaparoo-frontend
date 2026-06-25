// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
pragma Singleton

import QtQuick
import Zaparoo.Theme as AppTheme

// Stable host-facing theme tokens for QML packaged by zaparoo-update. The
// frontend may reorganize its internal theme module as long as this facade keeps
// the update package's token names compatible.
//
// Zaparoo.Theme.Sizing is intentionally not wrapped here. Its integer-pixel
// helpers are expected to remain a stable frontend host API, and forwarding
// every sizing call would add avoidable JS work in update detail-list delegates.
QtObject {
    readonly property color accent: AppTheme.Theme.accent
    readonly property color bgBar: AppTheme.Theme.bgBar
    readonly property color bgPanel: AppTheme.Theme.bgPanel
    readonly property color borderMid: AppTheme.Theme.borderMid
    readonly property color borderSubtle: AppTheme.Theme.borderSubtle
    readonly property bool crtNativePath: AppTheme.Theme.crtNativePath
    readonly property color error: AppTheme.Theme.error
    readonly property string errorHex: AppTheme.Theme.errorHex
    readonly property string fontMono: AppTheme.Theme.fontMono
    readonly property string fontUi: AppTheme.Theme.fontUi
    readonly property color selectionSurface: AppTheme.Theme.selectionSurface
    readonly property color surfaceCard: AppTheme.Theme.surfaceCard
    readonly property color textLabel: AppTheme.Theme.textLabel
    readonly property string textLabelHex: String(AppTheme.Theme.textLabel)
    readonly property color textPrimary: AppTheme.Theme.textPrimary
}
