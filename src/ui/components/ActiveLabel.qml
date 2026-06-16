// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
// Big single-line caption for the focused-tile name. Mounted under the
// grid on Systems and Games, and directly under the categories row on
// the Hub. Same typography as the screen-title slot in TopStatusStrip
// so the two big captions read as a matched pair.
// Single line with `elide: ElideRight` — long names are cut, never
// wrapped. Two-line wrap would shift the help-bar baseline by a row
// every time the focus crossed between a short and long entry, which
// reads as visible chop on a busy directional-input session.
// Games/Favorites also pass `tags`: the full (untrimmed) disambiguation
// tokens for the focused item, rendered as a dim suffix after the name.
// The name elides to keep the suffix visible; with no tags the layout is
// identical to before (Systems/Hub unaffected).

import QtQuick
import Zaparoo.Theme

// Software-rendering safe: only Item + Text, no transforms, no
// shaders, no opacity tweens.
Item {
    id: root

    property string text: ""
    property string tags: ""

    readonly property int _slack: Theme.crtNativePath ? 0 : Sizing.px(2)
    readonly property int _fontSize: Sizing.fontSize(4)
    readonly property int _maxWidth: Math.max(0, root.width - 2 * Sizing.pctW(5))
    readonly property bool _hasTags: root.tags !== ""
    readonly property int _gapW: root._hasTags ? Sizing.pctW(1.5) : 0
    readonly property int _tagsWidth: root._hasTags ? Math.ceil(Math.max(tagsMetrics.advanceWidth, tagsMetrics.boundingRect.width) + root._slack) : 0
    readonly property int _nameMeasured: Math.ceil(Math.max(nameMetrics.advanceWidth, nameMetrics.boundingRect.width) + root._slack)
    // Name keeps whatever the suffix leaves; the suffix is always shown.
    readonly property int _nameWidth: Math.min(root._nameMeasured, Math.max(0, root._maxWidth - root._gapW - root._tagsWidth))
    readonly property int _blockWidth: root._nameWidth + root._gapW + root._tagsWidth
    readonly property int _blockX: Sizing.center(root.width, root._blockWidth)

    TextMetrics {
        id: nameMetrics

        text: root.text
        font.family: Theme.fontUi
        font.pixelSize: root._fontSize
        font.weight: Font.Medium
    }

    TextMetrics {
        id: tagsMetrics

        text: root.tags
        font.family: Theme.fontUi
        font.pixelSize: root._fontSize
        font.weight: Font.Medium
    }

    Text {
        id: nameLabel

        x: root._blockX
        y: Sizing.center(parent.height, height)
        width: root._nameWidth
        height: root._fontSize
        text: root.text
        font.family: Theme.fontUi
        font.pixelSize: root._fontSize
        font.weight: Font.Medium
        color: Theme.textPrimary
        horizontalAlignment: Text.AlignLeft
        verticalAlignment: Text.AlignVCenter
        elide: Text.ElideRight
        renderType: Text.NativeRendering
    }

    Text {
        id: tagsLabel

        x: root._blockX + root._nameWidth + root._gapW
        y: nameLabel.y
        width: root._tagsWidth
        height: root._fontSize
        visible: root._hasTags
        text: root.tags
        font.family: Theme.fontUi
        font.pixelSize: root._fontSize
        font.weight: Font.Medium
        color: Theme.textVariant
        horizontalAlignment: Text.AlignLeft
        verticalAlignment: Text.AlignVCenter
        elide: Text.ElideRight
        renderType: Text.NativeRendering
    }
}
