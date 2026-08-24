// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
// Non-focusable group label for the Settings form. Splits the otherwise
// flat list of `SettingsField` rows into bands (General / Library /
// Advanced) so adjacent items are visibly related and rare entries
// don't crowd the commonly-used ones.
// The screen's navigation logic skips entries whose `kind` is `"header"`,
// so this row never receives focus, never paints a border, and has no
// hover/accept handling — it's purely a divider.

import QtQuick
import Zaparoo.Theme

// Software-renderer safe: a Rectangle band plus one Text, no shaders, no
// transforms, no animations.
//
// A filled, full-card-width band rather than a bigger/bolder label (round
// 6) — see docs/style.md -> "Settings section headers": in bitmap mode
// (--crt, or embedded 240p) `Sizing.fontSize()` quantizes `fontSection` and
// `fontBody` to the same 8px, and `Theme.fontUi`'s bitmap face has a single
// weight, so `Font.DemiBold` is a no-op there. Size and weight are simply
// unavailable as signals at 240p; a rectangle and a color step survive it
// where typography can't.
Item {
    id: root

    required property string label
    // The screen mounts this Item without its usual card-padding margin so
    // the band below can span the full card width edge-to-edge; this makes
    // up the difference for the label alone so it still lines up with
    // SettingsField's own (card-padding-inset) labels. See
    // SettingsScreen.qml's mount comment.
    property int contentInset: 0

    implicitHeight: Sizing.pctH(5.5)

    Rectangle {
        objectName: "settingsSectionHeaderBand"
        anchors.fill: parent
        // borderMid measures 1.6-2.3:1 off surfaceCard across the round-6
        // catalog — an unmistakable block on every preset — and stays off
        // Theme.accent, which SettingsField.qml reserves for "this row is
        // an action".
        color: Theme.borderMid
    }

    Text {
        objectName: "settingsSectionHeaderLabel"
        anchors.left: parent.left
        // Same left inset as `SettingsField` labels (plus contentInset,
        // see above) so headers and field labels share a vertical baseline.
        anchors.leftMargin: root.contentInset + Sizing.pctW(2)
        anchors.right: parent.right
        anchors.rightMargin: root.contentInset + Sizing.pctW(2)
        anchors.bottom: parent.bottom
        anchors.bottomMargin: Sizing.pctH(0.5)
        // Sentence-case + DemiBold still reads correctly wherever weight
        // does apply; the band above is what carries the break at 240p.
        text: root.label
        color: Theme.textPrimary
        font.family: Theme.fontUi
        font.pixelSize: Sizing.fontSection
        font.weight: Font.DemiBold
        elide: Text.ElideRight
        renderType: Text.NativeRendering
    }
}
