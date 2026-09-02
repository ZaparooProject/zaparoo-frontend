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
// Games/Favorites also pass `tags`: disambiguation tokens rendered as a
// dim suffix after the name. Name gets first claim on available width; tags
// use the remainder and elide. With no tags layout is unchanged.

import QtQuick
import Zaparoo.Theme

// Software-rendering safe: only Item + Text, no transforms, no
// shaders, no opacity tweens.
Item {
    id: root

    property string text: ""
    property string tags: ""
    // Horizontal margin reserved on each side before the name/tags block.
    // Defaults to the same pctW(3) every caller used before this was a
    // property, so plain usage is byte-identical. Footer callers that sit
    // this alongside reserved corner slots (a count badge on the left, a
    // PageIndicator on the right -- see HubScreen/SystemsScreen/
    // MediaListScreen) bind this to the slot width instead, so a long
    // name elides before it reaches either corner rather than running
    // underneath it.
    property int sideInset: Sizing.pctW(3)

    readonly property int _slack: Theme.crtNativePath ? 0 : Sizing.px(2)
    readonly property int _fontSize: Sizing.fontHero
    readonly property int _maxWidth: Math.max(0, root.width - 2 * root.sideInset)
    readonly property bool _hasTags: root.tags !== ""
    readonly property int _requestedGapW: root._hasTags ? Sizing.pctW(1.5) : 0
    readonly property int _nameMeasured: Math.ceil(Math.max(nameMetrics.advanceWidth, nameMetrics.boundingRect.width) + root._slack)
    readonly property int _tagsMeasured: root._hasTags ? Math.ceil(Math.max(tagsMetrics.advanceWidth, tagsMetrics.boundingRect.width) + root._slack) : 0
    // Name is primary context. Tags fit only in space left after the name;
    // both widths remain inside _maxWidth so footer corner chrome cannot be
    // overpainted by a long title or suffix.
    readonly property int _nameWidth: Math.min(root._nameMeasured, root._maxWidth)
    readonly property int _tagsWidth: Math.min(root._tagsMeasured, Math.max(0, root._maxWidth - root._nameWidth - root._requestedGapW))
    readonly property int _gapW: root._tagsWidth > 0 ? root._requestedGapW : 0
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
