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

// Software-renderer safe: a single Text in an Item, no shaders, no
// transforms, no animations.
Item {
    id: root

    required property string label

    implicitHeight: Sizing.pctH(5.5)

    Text {
        anchors.left: parent.left
        // Same left inset as `SettingsField` labels so headers and
        // field labels share a vertical baseline.
        anchors.leftMargin: Sizing.pctW(2)
        anchors.bottom: parent.bottom
        anchors.bottomMargin: Sizing.pctH(0.5)
        // Sentence-case + DemiBold reads as a header without needing
        // a separator rule. Sized one step above the field label
        // (which is 2.6) and painted in `textPrimary` so the group
        // break asserts itself across the form.
        text: root.label
        color: Theme.textPrimary
        font.family: Theme.fontUi
        font.pixelSize: Sizing.fontSize(2.9)
        font.weight: Font.DemiBold
        renderType: Text.NativeRendering
    }
}
