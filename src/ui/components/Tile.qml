// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
// Unified grid tile. Solid card with a centered icon area filling the
// card body, plus an accent-coloured outline ring around the card when
// this tile is the focused selection. Used by every tile surface in the frontend
// — hub categories row, systems grid, games grid, recents grid — so
// the vocabulary is identical across screens.
// Two layout modes, gated by `showCaption`:
//   - off (default): full-bleed icon, no in-tile label. Used by Hub
//     and Systems where a curated logo already carries identity.
//   - on: cover slot shrinks vertically to free a thin band along the
//     bottom edge for a one-line elided name caption. Used by Games
//     and Recents because a long shelf of similar boxart needs
//     per-tile labelling — the focused-tile caption below the grid
//     (ActiveLabel) only identifies one cell at a time.
// In caption mode the loading-state fallback is an hourglass glyph,
// not the wrapping-name text used in non-caption mode — the bottom
// caption already shows the name, so the centred-text fallback would
// just read it twice.
// Parent contract — Tile must be loaded inside a host that exposes:
//   - isSelected: bool   — true when this tile is the focused selection
//   - isFocused:  bool   — true when the section owning this tile has user focus
//   - name:       string — model display name (used by the procedural
//                          fallback while the cover PNG decodes)
//   - coverKey:   string — relative path under resources/images/ (no extension)
//   - favorite:   int    — optional 0/1; shows a small heart badge when 1

import QtQuick
import Zaparoo.Theme

// PagedGrid.qml and HubScreen's static category row both wrap their
// Tile delegate in a TileLoader that defines the required properties
// above; QML's late-binding model means a caller that forgets one
// fails silently at runtime rather than at build time, so the
// Component.onCompleted check below converts that footgun into a
// loud warning.
Item {
    // Do NOT add `layer.enabled` here. On Qt's software adaptation
    // it allocates a per-item QImage backing store, blits it into
    // the parent on every paint (extra memcpy, not the cached-blit
    // win the docs imply for hardware rendering), and its
    // compositing path with translucent siblings/parents differs
    // from the direct-paint path — visible as flicker on focus
    // moves and lost transparency around the focus ring.
    // `layer.enabled` is documented for scene graph (GPU) rendering;
    // on the MiSTer software target it is a regression, not an
    // optimization.
    // qmllint enable missing-property compiler
    // No focus scale bump. The earlier 1.06 scale on the focused tile
    // forced a bilinear resample of the cover pixmap on every focus
    // move and overflowed the cell by ~3% on each side, dirtying
    // strips of up to four neighbours per press — under software
    // rendering on MiSTer that read as choppy d-pad navigation on
    // covered grids. The focus outline ring + caption + active-label
    // already mark the selection clearly, so the size cue isn't worth
    // the per-press repaint cost. See `docs/qml-gotchas.md` →
    // "Software-renderer animation costs".
    // Bottom caption strip (caption mode only). Single line, ellipsised
    // when long. Tints to `textPrimary` on the focused tile so the
    // selection reads at a glance even when the focus outline ring is
    // outside the eye's centre — matches the procedural fallback's
    // focus tint above.

    id: root

    // qmllint disable missing-property compiler
    readonly property bool delegateIsSelected: parent.isSelected
    readonly property bool delegateIsFocused: parent.isFocused
    readonly property string delegateName: parent.name
    readonly property string delegateCoverKey: parent.coverKey
    readonly property bool delegateFavorite: parent.favorite !== 0
    property var layoutProfile: null
    // Opt-in per-tile name caption. Off by default so Hub and Systems
    // keep their full-bleed logo layout. Cover-art screens (Games,
    // Recents) flip this on at the delegate template.
    property bool showCaption: false
    // Equal cover padding on top, left, and right — the bottom is
    // owned by the caption strip in caption mode and matches `_padding`
    // visually in non-caption mode. pctH(2) is enough to read as
    // deliberate breathing room without giving back much cover area.
    // Below the cover sits the caption flush against the card's
    // bottom edge, separated from the cover by `_captionGap`.
    readonly property int _padding: Sizing.pctH(2)
    readonly property int _outlineGap: Sizing.pctH(0.4)
    readonly property int _outlineWidth: Sizing.stroke(Sizing.pctH(0.6))
    readonly property int _captionHeight: Sizing.pctH(5.5)
    readonly property int _captionGap: Sizing.pctH(0.4)
    readonly property int _captionTextSize: Sizing.fontSize(2.2)
    readonly property int _tileCornerRadius: root.layoutProfile ? root.layoutProfile.tileCornerRadius : Sizing.cornerRadius
    readonly property int _captionTextMaxWidth: Math.max(0, root.width - 2 * root._tileCornerRadius)
    readonly property int _textMeasureSlack: Theme.crtNativePath ? 0 : 2
    readonly property int _captionMeasuredWidth: Math.ceil(Math.max(captionMetrics.advanceWidth, captionMetrics.boundingRect.width) + root._textMeasureSlack)
    readonly property int _captionTextWidth: Math.min(root._captionTextMaxWidth, root._captionMeasuredWidth)

    readonly property bool _focusedSelection: root.delegateIsSelected && root.delegateIsFocused
    // `coverKey` is the relative path under `resources/images/` without
    // extension — `systems/snes`, `categories/Consoles`, etc. The model
    // chooses the subdirectory; Tile is agnostic. Resources.coverUrl is
    // the single source of truth for the qrc layout — see Resources.qml.
    //
    // The model's `icons/Loading` sentinel is a special case: it means
    // "cover fetch is in flight". Routing it through the full-bleed
    // cover slot would rasterise the SVG at the entire icon area; the
    // existing `loadingGlyph` overlay below already defines the
    // standard centred hourglass size, so swallow the source here and
    // let `loadingGlyph` own the painting.
    readonly property bool _coverPending: root.delegateCoverKey === "icons/Loading"
    readonly property url _coverSource: root._coverPending ? "" : Resources.coverUrl(root.delegateCoverKey)
    readonly property bool _hasCover: cover.status === Image.Ready

    anchors.fill: parent
    Component.onCompleted: {
        // Self-check the parent contract. Logs once at construction so
        // a future caller that drops Tile into a non-conforming wrapper
        // sees the failure mode immediately instead of debugging
        // mysteriously empty tiles.
        // qmllint disable missing-property compiler
        if (typeof parent.isSelected !== "boolean" || typeof parent.isFocused !== "boolean" || typeof parent.name !== "string" || typeof parent.coverKey !== "string")
            console.warn("Tile: parent does not satisfy the delegate contract " + "(expected isSelected:bool, isFocused:bool, " + "name:string, coverKey:string)");
    }

    // Tile body. Solid card so the white icon has a high-contrast
    // surface. Always visible — no opacity gating — which is the
    // unified-Tile contract: every grid renders the same shape.
    // Static 1px borderMid edge gives every tile a card edge whether
    // focused or not — same depth cue settings rows carry. The accent
    // focus ring still paints on top when this tile is the focused
    // selection.
    Rectangle {
        anchors.fill: parent
        radius: root._tileCornerRadius
        color: Theme.surfaceCard
        border.color: Theme.borderMid
        border.width: Sizing.stroke(1)
    }

    // Focus outline ring. Drawn *inside* the card edge so the ring
    // never bleeds past the cell bounds — that's the project standard:
    // borders/outlines stay within their parent rather than overflowing
    // it. Keeps the ring out of PagedGrid's clip rect at the row edges
    // and means callers don't have to reserve bleed room for it. Gated
    // on `_focusedSelection` so only the focused tile in the focused
    // section lights up — keeps multiple tile sections on screen from
    // competing for the eye. Drawn after the card so the border sits on
    // top; the icon padding (`_padding = pctH(3)`) is far larger than
    // the inset (`_outlineGap = pctH(0.4)`), so the ring never overlaps
    // content.
    // Focus ring drawn as two stacked *filled* rounded rectangles — an
    // outer accent pill and an inner surfaceCard mask that punches the
    // centre back, leaving a uniform outline. Equivalent to the older
    // single-Rectangle `border.color` + `border.width` approach but
    // significantly smoother on the corners under Qt's software
    // adaptation: filled rounded rects honour the AA path, while thin
    // rounded *borders* are tessellated without subpixel coverage and
    // step visibly at the corners (see QTBUG-123210). Both rectangles
    // are still inside the card edge by `_outlineGap`, so the ring
    // never bleeds past the cell bounds.
    Rectangle {
        id: focusRingOuter

        anchors.fill: parent
        anchors.margins: root._outlineGap
        color: Theme.accent
        radius: Math.max(0, root._tileCornerRadius - root._outlineGap)
        antialiasing: true
        visible: root._focusedSelection
    }

    Rectangle {
        anchors.fill: focusRingOuter
        anchors.margins: root._outlineWidth
        color: Theme.surfaceCard
        // Inner radius shrinks to keep the visible ring's outer edge
        // and inner edge concentric with the card corners. Floor at 0
        // so very small tiles (where _outlineWidth approaches the
        // outer radius) collapse to a sharp inner mask rather than
        // negative-radius garbage.
        radius: Math.max(0, focusRingOuter.radius - root._outlineWidth)
        antialiasing: true
        visible: root._focusedSelection
    }

    // Icon area. Fills the card minus padding on every side, centered
    // horizontally. PreserveAspectFit lets curated logos render at
    // their native aspect inside whichever dimension is the tighter
    // constraint.
    Image {
        id: cover

        width: parent.width - 2 * root._padding
        source: root._coverSource
        // Pin to the system PNGs' native width (256). A size-dependent
        // binding here would force a re-decode every frame the cell
        // animates — a constant value means QPixmapCache hits once per
        // logo and reuses the decoded pixmap across each layout
        // change. Combined with `smooth: true`, downscaling to the
        // actual cell width is bilinear-filtered on draw.
        sourceSize.width: 256
        fillMode: Image.PreserveAspectFit
        smooth: true
        asynchronous: true
        opacity: root._hasCover ? 1 : 0

        anchors {
            top: parent.top
            topMargin: root._padding
            bottom: parent.bottom
            // In caption mode the cover sits above the bottom caption
            // strip with only `_captionGap` of breathing room. The
            // caption is flush against the card's bottom edge, so the
            // cover's lower bound is just (caption height + gap) —
            // there is no second layer of card padding below.
            bottomMargin: root.showCaption ? root._captionHeight + root._captionGap : root._padding
            horizontalCenter: parent.horizontalCenter
        }
    }

    // Caption-mode loading cue. Centred hourglass glyph that paints
    // only during the Image.Loading window — once the cover lands the
    // glyph hides and the cover paints in. Error/Null cover state
    // also hides the glyph (a stuck hourglass on a permanently failed
    // cover would mislead) and the bottom caption still identifies
    // the tile. Bundled qrc asset, decode is cheap, no animation.
    Image {
        id: loadingGlyph

        x: cover.x + Sizing.center(cover.width, width)
        y: cover.y + Sizing.center(cover.height, height)
        width: Sizing.pctH(10)
        height: Sizing.pctH(10)
        source: Resources.iconUrl("Loading")
        // Loading.svg has a 24×24 native viewBox; without sourceSize
        // Qt rasterises at that intrinsic size and bilinear-upscales
        // to the rendered box, which reads as soft on every screen
        // taller than ~240 px. Pinning sourceSize to the rendered
        // dimensions makes the SVG renderer rasterise at target size
        // — same pattern StatusIcon.qml and LoadingIndicator.qml use.
        sourceSize.width: Sizing.px(width)
        sourceSize.height: Sizing.px(height)
        fillMode: Image.PreserveAspectFit
        smooth: true
        asynchronous: false
        visible: root.showCaption && (root._coverPending || cover.status === Image.Loading)
    }

    Image {
        id: favoriteGlyph

        anchors.left: parent.left
        anchors.top: parent.top
        anchors.leftMargin: Sizing.px(parent.width / 12)
        anchors.topMargin: Sizing.px(parent.width / 12)
        width: Sizing.px(parent.width / 6)
        height: width
        source: Resources.iconUrl("Heart")
        sourceSize.width: Sizing.px(width)
        sourceSize.height: Sizing.px(height)
        fillMode: Image.PreserveAspectFit
        smooth: true
        asynchronous: false
        visible: root.delegateFavorite
    }

    // Non-caption procedural fallback. Sits at the same geometry as
    // the cover and snaps to the cover the moment Image.status hits
    // Ready; the brief Loading window shows the fallback text rather
    // than crossfading. Cache hits skip Loading entirely and snap
    // directly. In caption mode this is suppressed — the bottom
    // caption already shows the name and the hourglass above signals
    // load progress, so a wrapping copy of the name in this slot is
    // redundant.
    Text {
        anchors.fill: cover
        text: root.delegateName
        font.family: Theme.fontUi
        font.pixelSize: Sizing.fontSize(2.4)
        color: root._focusedSelection ? Theme.textPrimary : Theme.textLabel
        // Wrap (not WordWrap): an unbreakable identifier like
        // `_LongCollectionName_Definitive_Cut.smc` would otherwise
        // render past `width` and bleed out of the tile.
        wrapMode: Text.Wrap
        horizontalAlignment: Text.AlignHCenter
        verticalAlignment: Text.AlignVCenter
        renderType: Text.NativeRendering
        opacity: (!root.showCaption && !root._hasCover) ? 1 : 0
        clip: true
    }

    // Bottom caption strip (caption mode only). Single line, ellipsised
    // when long. The Text item itself is centered on an integer x and
    // the glyph run is left-aligned inside it; `Text.AlignHCenter` can
    // place bitmap glyphs on a half-pixel when the tile width and text
    // width have opposite parity, which softens Bongo in CRT mode.
    // Tints to `textPrimary` on the focused tile so the selection reads
    // at a glance even when the focus outline ring is outside the eye's
    // centre — matches the procedural fallback's focus tint above.
    //
    // The strip sits flush at the card's bottom edge so the title
    // visually owns the bottom of the tile rather than hovering above
    // a band of card padding. Horizontal margins clear `cornerRadius`
    // so glyph descenders never enter the rounded-corner region (the
    // card's surfaceCard fill curves away there, so a glyph past the
    // inset would paint against whatever sits behind the tile).
    // Vertically, the centred glyph lands well inside the focus
    // ring's inner mask zone (which extends `_outlineGap +
    // _outlineWidth` from the bottom edge), so the text background
    // remains surfaceCard even on a focused tile.
    TextMetrics {
        id: captionMetrics

        text: root.delegateName
        font.family: Theme.fontUi
        font.pixelSize: root._captionTextSize
    }

    Text {
        id: caption

        x: Sizing.center(parent.width, width)
        y: parent.height - root._captionHeight + Sizing.center(root._captionHeight, height)
        width: root._captionTextWidth
        height: root._captionTextSize
        visible: root.showCaption
        text: root.delegateName
        font.family: Theme.fontUi
        font.pixelSize: root._captionTextSize
        color: root._focusedSelection ? Theme.textPrimary : Theme.textLabel
        elide: Text.ElideRight
        horizontalAlignment: Text.AlignLeft
        verticalAlignment: Text.AlignVCenter
        renderType: Text.NativeRendering
    }
}
