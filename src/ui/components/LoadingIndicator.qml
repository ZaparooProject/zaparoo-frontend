// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
// The single loading indicator used everywhere in the app: the global
// forward-transition overlay, the in-screen Loading state of
// ScreenStateOverlay, and the in-flight pagination cue on GamesScreen.
// Anything that visually says "loading" goes through this component so
// the icon, baseline, font, and colour stay in lockstep across screens.
// Layout: a Row with an Image and a Text, both vertical-centred against
// an explicit `height` so the icon caps and the text cap-height share
// a baseline. Without the explicit row height, Text would size itself
// to the font metrics (taller than the icon) and the pair would sit a
// few px out of alignment.

import QtQuick
import Zaparoo.Theme

// Software-rendering safe: only Item, Row, Image, Text. No transforms,
// no shaders, no animations.
Row {
    id: indicator

    // Visible label. Defaults to "Loading…" but callers can override
    // for context-specific cues (e.g. "Loading more…" while a paged
    // fetch is in flight).
    property string text: qsTr("Loading…")
    // Glyph height = text cap height. Use Sizing.fontSize(3) by
    // default so the row reads at the same scale as the page-counter
    // and total-files badges in TopStatusStrip.
    property real glyphSize: Sizing.fontSize(3)

    width: Sizing.px(implicitWidth)
    height: indicator.glyphSize
    spacing: Sizing.pctW(0.6)

    Image {
        anchors.verticalCenter: parent.verticalCenter
        height: parent.height
        width: height
        sourceSize.height: Sizing.px(height)
        sourceSize.width: Sizing.px(width)
        source: Resources.iconUrl("Loading")
        fillMode: Image.PreserveAspectFit
        smooth: true
    }

    Text {
        anchors.verticalCenter: parent.verticalCenter
        height: parent.height
        verticalAlignment: Text.AlignVCenter
        text: indicator.text
        font.family: Theme.fontUi
        font.pixelSize: indicator.glyphSize
        color: Theme.textPrimary
        renderType: Text.NativeRendering
    }
}
