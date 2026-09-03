// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

import QtQuick
import Zaparoo.Theme

// Non-focusable group heading for a vertical list: the Settings form's
// bands (General / Library / Advanced), Game info's Description block, and
// the picker page inside the setup modals. A metadata-colored label on a
// hairline rule, inset like the row labels under it, so a group reads the
// same on the settings card, in a modal panel, and on the detail sheet.
// The host's navigation skips it; it never takes focus.
//
// Color and a rule, not size or weight: in bitmap mode (--crt, embedded
// 240p) `Sizing.fontSize()` quantizes fontSection and fontBody to the same
// 8px strike and `Theme.fontUi`'s bitmap face has one weight, so a bigger
// or bolder label is pixel-identical to a row label there. `Theme.textLabel`
// against `textPrimary` rows is the color step the settings hint band
// already leans on for the same reason, and a one-stroke rule is a
// rectangle that renders at any tier. The earlier filled `borderMid` band
// only worked where it met the settings card's own frame edge to edge;
// inside a modal panel it was a gray block with margins. See
// docs/style.md -> "Section headings".
//
// Software-renderer safe: one Text and one Rectangle.
Item {
    id: root

    required property string label

    // Same left inset as `SettingsField` labels so headings and field
    // labels share a vertical baseline; the rule runs the full row width.
    //
    // Settable, because the invariant is "share the baseline with the rows
    // under it", not the literal value: a host whose rows sit flush at
    // x: 0 (GameInfoModal's tag table and description) has to zero this or
    // the heading floats indented relative to everything it heads.
    property int labelInset: Sizing.pctW(2)
    readonly property int _labelInset: root.labelInset
    // Room above the label so a heading separates from the group before it
    // as well as titling the one below.
    readonly property int _topGap: Sizing.pctH(1.5)
    readonly property int _ruleGap: Sizing.pctH(0.8)

    implicitHeight: root._topGap + labelText.implicitHeight + root._ruleGap + rule.height

    Text {
        id: labelText

        objectName: "sectionHeaderLabel"
        anchors.left: parent.left
        anchors.leftMargin: root._labelInset
        anchors.right: parent.right
        anchors.rightMargin: root._labelInset
        anchors.top: parent.top
        anchors.topMargin: root._topGap
        // Sentence case (docs/content-style.md); DemiBold still reads as a
        // heading wherever weight applies, the color carries it where it
        // doesn't.
        text: root.label
        color: Theme.textLabel
        font.family: Theme.fontUi
        font.pixelSize: Sizing.fontBody
        font.weight: Font.DemiBold
        elide: Text.ElideRight
        renderType: Text.NativeRendering
    }

    Rectangle {
        id: rule

        objectName: "sectionHeaderRule"
        anchors.left: parent.left
        anchors.right: parent.right
        anchors.bottom: parent.bottom
        height: Sizing.stroke(1)
        // The card frame's own color: 1.5:1+ off surfaceCard on every
        // preset, so the break is visible at 240p; stays off Theme.accent,
        // which SettingsField reserves for "this row is an action".
        color: Theme.borderMid
    }
}
