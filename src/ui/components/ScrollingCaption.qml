// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
pragma ComponentBehavior: Bound

import QtQuick
import Zaparoo.Theme

// One-line title caption with an inline dim suffix for a media item's
// disambiguating-tag tokens (already sibling-diffed by the model), shared by the
// games grid tile caption and the browse list row. Three states, picked
// automatically:
//
//   - Fits: the full name + tokens render statically (centered when
//     `centerContent`, else left-aligned).
//   - Overflows, unfocused: the name is guaranteed a minimum share of the width
//     and the token suffix takes the rest, each eliding as needed — so a long
//     tag can never crush the title to nothing, and the tokens stay visible for
//     at-a-glance comparison across a grid.
//   - Overflows, focused: an integer-pixel marquee scroll-reveals the full
//     name + tokens. Gated on `Motion.enabled`, so reduce-motion falls back to
//     the static elided form.
//
// Pure Item + Text, no shaders/effects. The marquee steps whole pixels via a
// Timer (not a fractional NumberAnimation) so the MiSTer bitmap font stays
// crisp, and only the focused caption ever animates, so the dirty rect is one
// line on one tile.
Item {
    id: root

    // Display name (already resolved for the original-filename toggle by the
    // model). Required.
    property string name: ""
    // Inline token suffix from the model's `disambiguatingTags` role: the final,
    // space-joined, sibling-diffed display string. "" when the item has no
    // variants (the common case — this then behaves like a plain elided caption).
    property string tags: ""
    // True when this caption's tile/row is the focused selection: enables the
    // overflow marquee.
    property bool focused: false
    // Center the static block (grid tiles) vs. left-align it (list rows).
    property bool centerContent: false
    property int fontPixelSize: Sizing.fontSize(2.2)
    property string fontFamily: Theme.fontUi
    property color nameColor: Theme.textLabel
    property color variantColor: Theme.textVariant
    // Smallest fraction of the caption width the name is allowed to shrink to
    // when a long token suffix competes for space. Keeps the title legible.
    property real minNameFraction: 0.58

    readonly property bool _hasTags: root.tags !== ""
    readonly property int _gapW: root._hasTags ? Sizing.pctW(1.2) : 0

    readonly property int _avail: Math.max(0, root.width)
    readonly property int _nameFullW: Math.ceil(nameMetrics.advanceWidth)
    readonly property int _tagsFullW: root._hasTags ? Math.ceil(tagsMetrics.advanceWidth) : 0

    readonly property int _blockW: root._nameFullW + root._gapW + root._tagsFullW
    readonly property int _scrollDist: Math.max(0, root._blockW - root._avail)
    readonly property bool _fits: root._scrollDist === 0
    readonly property bool _marquee: root.focused && !root._fits && Motion.enabled && root._scrollDist > 0
    readonly property bool _staticOverflow: !root._fits && !root._marquee

    // Width policy for the static-overflow state. The name takes its natural
    // width (never reserving more than it needs, so a short name leaves no gap
    // before the suffix) but is protected by `minNameFraction` so a long suffix
    // can't crush it. The suffix then takes whatever the name leaves — including
    // the slack a short name gives back — so the block fills the width edge to
    // edge instead of sitting in over-wide centered margins.
    readonly property int _nameMinW: Math.round(root._avail * root.minNameFraction)
    readonly property int _nameStaticW: Math.min(root._nameFullW, Math.max(root._nameMinW, root._avail - root._gapW - root._tagsFullW))
    readonly property int _tagsStaticW: Math.min(root._tagsFullW, Math.max(0, root._avail - root._gapW - root._nameStaticW))

    readonly property int _nameRenderW: root._staticOverflow ? root._nameStaticW : root._nameFullW
    readonly property int _tagsRenderW: root._staticOverflow ? root._tagsStaticW : root._tagsFullW
    readonly property int _staticBlockW: root._nameRenderW + root._gapW + root._tagsRenderW
    readonly property int _fitsOffset: root.centerContent ? Math.round((root._avail - (root._fits ? root._blockW : root._staticBlockW)) / 2) : 0

    // Marquee scroll position (0 .. -_scrollDist), driven in whole pixels.
    property int _scrollX: 0
    property int _scrollDir: -1
    property int _pauseTicks: 0

    on_MarqueeChanged: {
        if (!root._marquee) {
            root._scrollX = 0;
            root._scrollDir = -1;
            root._pauseTicks = 0;
        }
    }

    clip: true

    TextMetrics {
        id: nameMetrics

        text: root.name
        font.family: root.fontFamily
        font.pixelSize: root.fontPixelSize
    }

    TextMetrics {
        id: tagsMetrics

        text: root.tags
        font.family: root.fontFamily
        font.pixelSize: root.fontPixelSize
    }

    // Steps the marquee one pixel per tick with a dwell at each end. Runs only
    // while focused + overflowing + motion enabled; otherwise the caption is
    // static and this is stopped.
    Timer {
        interval: 40
        repeat: true
        running: root._marquee

        onTriggered: {
            if (root._pauseTicks > 0) {
                root._pauseTicks -= 1;
                return;
            }
            const next = root._scrollX + root._scrollDir;
            if (next <= -root._scrollDist) {
                root._scrollX = -root._scrollDist;
                root._scrollDir = 1;
                root._pauseTicks = 20;
            } else if (next >= 0) {
                root._scrollX = 0;
                root._scrollDir = -1;
                root._pauseTicks = 20;
            } else {
                root._scrollX = next;
            }
        }
    }

    Item {
        id: content

        y: 0
        width: root._nameRenderW + root._gapW + root._tagsRenderW
        height: parent.height
        // Static: centered (grid) or left-aligned (list) offset; the block
        // fills the width when overflowing, so `_fitsOffset` collapses to 0.
        // Marquee: integer scroll position.
        x: root._marquee ? root._scrollX : root._fitsOffset

        Text {
            id: nameText

            x: 0
            width: root._nameRenderW
            height: parent.height
            text: root.name
            color: root.nameColor
            font.family: root.fontFamily
            font.pixelSize: root.fontPixelSize
            elide: (!root._marquee && root._nameRenderW < root._nameFullW) ? Text.ElideRight : Text.ElideNone
            horizontalAlignment: Text.AlignLeft
            verticalAlignment: Text.AlignVCenter
            renderType: Text.NativeRendering
        }

        Text {
            id: suffixText

            x: root._nameRenderW + root._gapW
            width: root._tagsRenderW
            height: parent.height
            text: root.tags
            visible: root._hasTags
            color: root.variantColor
            font.family: root.fontFamily
            font.pixelSize: root.fontPixelSize
            // Elide from the LEFT so the specific, most-distinguishing end of a
            // long token suffix (`...lightgun`, `...system-1`) stays visible.
            elide: (!root._marquee && root._tagsRenderW < root._tagsFullW) ? Text.ElideLeft : Text.ElideNone
            horizontalAlignment: Text.AlignLeft
            verticalAlignment: Text.AlignVCenter
            renderType: Text.NativeRendering
        }
    }
}
