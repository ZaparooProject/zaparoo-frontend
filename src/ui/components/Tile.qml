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
//   - topLabel:   string — optional compact label above cover art
//   - favorite:   int    — optional 0/1; shows a small heart badge when 1
//   - coverSynchronous: bool — optional; decode bundled artwork inline on the
//                          GUI thread so it paints in the tile's first frame

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
    // No persistent focus scale. The earlier 1.06 scale held on every
    // focused tile forced a bilinear resample of the cover pixmap on
    // every d-pad move and overflowed the cell by ~3% on each side,
    // dirtying strips of up to four neighbours per press. Under Qt's
    // software adaptation on MiSTer that read as choppy navigation on
    // covered grids. That scale was a persistent, per-focus-move cost
    // across the whole visible grid.
    //
    // The activate/launch press below is a different cost class: one tile's
    // face translates by a few integer pixels at activation. The dirty rect
    // stays inside one cell, cover art is never resampled, and neighbours are
    // unaffected. See `docs/qml-gotchas.md` →
    // "Software-renderer animation costs" for the full distinction.
    // Bottom caption strip (caption mode only). Single line, ellipsised
    // when long. Tints to `textPrimary` on the focused tile so the
    // selection reads at a glance even when the focus outline ring is
    // outside the eye's centre — matches the procedural fallback's
    // focus tint above.

    id: root
    objectName: "tile"

    // qmllint disable missing-property compiler
    readonly property bool delegateIsSelected: parent.isSelected
    readonly property bool delegateIsFocused: parent.isFocused
    readonly property string delegateName: parent.name
    readonly property string delegateCoverKey: parent.coverKey
    readonly property string delegateTopLabel: parent.topLabel ?? ""
    readonly property bool delegateFavorite: parent.favorite !== 0
    // qmllint disable missing-property compiler
    readonly property bool delegateHidden: parent.hidden === true
    // qmllint disable missing-property compiler
    // Hub-only today (Games/Systems/Favorites/Recents hosts never set this):
    // true while the tile's live precondition isn't currently met (Resume
    // with no history, Update with no internet, a category Core hasn't
    // confirmed). Never means the tile is absent -- see HubScreen.qml's
    // resolvers -- only that it reads muted and Accept no-ops on it.
    readonly property bool delegateDisabled: parent.disabled === true
    // qmllint disable missing-property compiler
    // Sibling-diffed disambiguating-tag display string (region, disc, rev, ...).
    // Empty for items with no variants. Rendered as a dim inline suffix after
    // the name in the bottom caption (see ScrollingCaption), identically on the
    // default and CRT paths. For a folder/root row this instead carries the
    // round-11 roots-screen distinguisher, if any (games.rs's
    // `root_distinguishers`) -- `delegateTagsSuffix` below folds it
    // together with `delegateFileCount`.
    readonly property string delegateDisambiguatingTags: parent.disambiguatingTags ?? ""
    // Round 11. Defaults keep every model without these roles (Hub's
    // hand-built ListModel) silently at "no folder-count suffix", same
    // reasoning as `delegateDisabled` above.
    readonly property string delegateEntryType: parent.entryType ?? ""
    readonly property int delegateFileCount: parent.fileCount ?? 0
    readonly property string delegateTagsSuffix: Format.rowSuffix(root.delegateEntryType, root.delegateDisambiguatingTags, root.delegateFileCount)
    // Pulse counter forwarded by TileLoader — increment to lower the raised
    // face on the focused tile. Every button-like action (folder
    // drill-in, system select, game launch) shares this single cue, so
    // there is no separate launch animation. Default to 0 so hosts that
    // do not wire it are silently no-ops.
    readonly property int delegateActivatePulse: parent.activatePulse ?? 0
    // Release counter forwarded by TileLoader — increment to raise the face
    // after a launch that keeps the frontend on the same screen (e.g. an Audio
    // track that does not take the FPGA). Forward navigation never fires it;
    // the screen transition + `settling` resets the held press off-screen.
    // Defaults to 0 so hosts that do not wire it are silently no-ops.
    readonly property int delegateReleasePulse: parent.releasePulse ?? 0
    // `settling` is set true by the host screen when the screen becomes
    // inactive (off-screen). Used to raise a held face before the screen is
    // shown again.
    readonly property bool delegateSettling: parent.settling ?? false
    // True while the host has picked this tile up for a reorder (the Hub's
    // Options -> Move). Defaults false for hosts that do not wire it.
    readonly property bool delegateHeld: parent.held ?? false
    // `focusReady` gates whether this tile renders its focused styling at all
    // (ring + focused cover ramp). The host leaves it false until the screen's
    // focus index is finalized
    // (programmatic restore or first input). Before that, a tile that happens
    // to sit at the default index must not paint a ring, or the wrong tile
    // flashes focused for the frames before restore corrects the index.
    // Defaults true for hosts that do not wire it.
    readonly property bool delegateFocusReady: parent.focusReady ?? true
    readonly property bool delegateLoadFocusedCover: parent.loadFocusedCover ?? true
    // Opt-in same-frame cover decode, forwarded by TileLoader/PagedGrid. Only
    // honored for tinted-provider keys, where the request is a sub-millisecond
    // LUT pass over a mapped mask (see BakedIconAtlas). Defaults true so a host
    // that does not wire it still gets instant bundled icons.
    readonly property bool delegateCoverSynchronous: parent.coverSynchronous ?? true
    // qmllint enable missing-property
    property var layoutProfile: null
    readonly property var _surfaceProfile: root.layoutProfile && root.layoutProfile.surface ? root.layoutProfile.surface : null
    // Opt-in per-tile name caption. Off by default so Hub and Systems
    // keep their full-bleed logo layout. Cover-art screens (Games,
    // Recents) flip this on at the delegate template.
    property bool showCaption: false
    // Decode size (px) for real cover art, set by the owning screen to its
    // per-view, resolution-derived tier (see Sizing.…CoverSourceSize). Held
    // stable while browsing so Qt's pixmap cache short-circuits reloads instead
    // of re-decoding; default 256 matches the built-in tinted-asset raster.
    property int coverSourceSize: 256
    // Opt-in tighter inset for the Hub grid and Settings' own root category
    // grid (which deliberately shares the Hub's fixed tile size — see
    // docs/style.md's "Tile aspect and grid blocks"). Both are full-bleed
    // icon/cover tiles with no caption band competing for space, so they
    // can afford to give more of the cell to the art itself than a
    // captioned browse tile can. Off by default — every other caller
    // (Systems, Games, Recents, Favorites) is unaffected.
    property bool compactPadding: false
    // Ring geometry, declared ahead of `_padding` so the padding can be
    // derived from it rather than merely checked against it — an earlier
    // version of this file computed `_padding` independently and only
    // *claimed* in a comment that it cleared the ring; it didn't (see git
    // history around round 8's `compactPadding` and docs/style.md's "Tile
    // focus ring" note). The ring's own inner edge, in the tile's local
    // coordinate space, sits at `_outlineGap + _outlineWidth` in from each
    // side.
    readonly property int _outlineGap: Sizing.pctH(0.4)
    readonly property int _outlineWidth: Sizing.focusRingWidth
    readonly property int _ringInnerEdge: root._outlineGap + root._outlineWidth
    // Equal cover padding on top, left, and right — the bottom is
    // owned by the caption strip in caption mode and matches `_padding`
    // visually in non-caption mode. pctH(2) is enough to read as
    // deliberate breathing room without giving back much cover area.
    // `compactPadding` derives its inset from `_ringInnerEdge` plus a
    // fixed pctH(0.4) clearance, instead of an independent pctH(1) that
    // used to land exactly on the ring's inner edge with a zero-pixel
    // gap at every tier — art touched the ring by construction, not by
    // rounding. Deriving from the ring means a future change to either
    // ring token can't silently close the gap again.
    // Below the cover sits the caption flush against the card's
    // bottom edge, separated from the cover by `_captionGap`.
    readonly property int _padding: root.compactPadding ? root._ringInnerEdge + Sizing.pctH(0.4) : Sizing.pctH(2)
    readonly property int _captionHeight: Sizing.pctH(5.5)
    readonly property int _captionGap: Sizing.pctH(0.4)
    readonly property int _captionTextSize: Sizing.fontSmall
    readonly property int _captionTextWeight: Font.Normal
    readonly property bool _hasTopLabel: root.delegateTopLabel !== ""
    // Matches `_captionHeight`/`_captionGap` exactly -- the top label is a
    // `ScrollingCaption` too (see below), and a reserved band the same size
    // as the bottom caption's is what makes the two read as visually
    // identical rather than just similarly-behaved.
    readonly property int _topLabelHeight: root._captionHeight
    readonly property int _topLabelGap: root._captionGap
    // Same ladder token the bottom caption uses (`_captionTextSize`) so the
    // two labels read as the same visual weight -- this used to be the raw
    // geometry helper `Sizing.fontSize(2)`, an off-ladder "seventh text
    // role" docs/style.md's type-ladder section explicitly warns against.
    readonly property int _topLabelTextSize: Sizing.fontSmall
    readonly property int _topLabelTextWeight: Font.Medium
    readonly property int _tileCornerRadius: root._surfaceProfile ? root._surfaceProfile.cardRadius : Sizing.radiusMd
    // Width available to the bottom caption. Matches the cover image's own
    // left/right padding (`_padding`) rather than a smaller corner-radius
    // inset, which used to sit inside the focus ring's inner edge and let
    // glyphs overlap it. ScrollingCaption does its own measuring/eliding/
    // marquee inside this width.
    readonly property int _captionSideInset: root._padding
    readonly property int _captionTextMaxWidth: Math.max(0, root.width - 2 * root._captionSideInset)

    // Focused styling (ring + focused cover ramp) is withheld until the host
    // marks focus ready via `delegateFocusReady`. This keeps a default-index
    // tile from painting a ring during the window between first paint and the
    // programmatic restore that finalizes the real selection — the source of
    // the load-time "wrong tile flashes focused" bug. The focus ring snaps on
    // and off with selection; the only per-tile motion is the press cue.
    readonly property bool _focusedSelection: root.delegateIsSelected && root.delegateIsFocused && root.delegateFocusReady
    // `coverKey` is the relative path under `resources/images/` without
    // extension — `systems/snes`, `categories/Consoles`, etc. The model
    // chooses the subdirectory; Tile is agnostic. Resources.coverUrl is
    // the single source of truth for the qrc layout — see Resources.qml.
    //
    // The model's `icons/Loading` sentinel means "cover fetch is in flight".
    // Swallow it so media tiles remain blank until real art is ready; painting
    // the sentinel in the cover slot would scale an hourglass across the card.
    readonly property bool _coverPending: root.delegateCoverKey === "icons/Loading"
    readonly property bool _systemCover: root.delegateCoverKey.startsWith("systems/")
    // True for any built-in vector asset: system logos, hub category icons,
    // folder/file/action UI glyphs. False for real art (media-image/,
    // custom-image/) which is never recolored — a user override is shown
    // exactly as it is on disk. This is an identity test, so it stays stable
    // across logo styles; it drives the wordmark treatment, which must read the
    // same for every system tile in a grid even though the color style routes
    // some of them around the tint.
    readonly property bool _isBundledArtwork: root.delegateCoverKey.startsWith("systems/") || root.delegateCoverKey.startsWith("categories/") || root.delegateCoverKey.startsWith("icons/")
    // Narrower: the subset of bundled artwork that routes through the
    // tinted-svg provider *right now*. Under the color logo style a `systems/`
    // key short-circuits to a plain qrc PNG, which has no focused ramp and
    // costs a real 1-3 ms decode on ARM — so it must take neither the
    // focus-ramp path nor the synchronous path below.
    readonly property bool _isTinted: Resources.isTintedProviderKey(root.delegateCoverKey)
    // True for real raster cover art (a fetched media/custom image) as opposed
    // to the built-in tinted vector assets (system logos, category/UI glyphs).
    // Drives the cover decode size below; defined on the art's own identity
    // (its key prefix), not on `_isTinted`, so the decode policy stays correct
    // independently of theme-tint behavior.
    readonly property bool _coverIsRealArt: root.delegateCoverKey.startsWith("media-image/") || root.delegateCoverKey.startsWith("custom-image/")
    // Unfocused ramp — always loaded for tinted keys; also the sole source for
    // real art (media-image/, custom-image/) which is focus-independent.
    readonly property url _coverBaseSrc: root._coverPending ? "" : Resources.coverUrl(root.delegateCoverKey, Theme.logoPrimary, Theme.logoSecondary, Theme.logoShadow)
    // Focused ramp — only loaded for tinted icons; empty string for real art so
    // the Image item never initiates a second fetch for cover/boxart tiles.
    readonly property url _coverFocusSrc: (root.delegateLoadFocusedCover && root._isTinted && !root._coverPending) ? Resources.coverUrl(root.delegateCoverKey, Theme.logoFocusPrimary, Theme.logoFocusSecondary, Theme.logoFocusShadow) : ""
    // True once the focused ramp is decoded and this tile is the focused
    // selection — used to suppress coverBase so the two renders don't stack
    // (which would double the effective opacity on hidden tiles).
    readonly property bool _focusCoverActive: root._focusedSelection && root._isTinted && coverFocus.status === Image.Ready
    // System wordmarks are a terminal-error fallback only. Every logo request
    // must first pass through Image.Loading and the provider; never show text
    // for Null/Loading, and never substitute text for failed category, icon, or
    // media artwork.
    readonly property bool _fallbackVisible: root._systemCover && !root.showCaption && !root._coverPending && coverBase.status === Image.Error
    readonly property int _fallbackTextSize: root._systemCover ? Sizing.fontSize(5.8) : Sizing.fontCaption
    readonly property int _fallbackMinimumTextSize: root._systemCover ? Sizing.fontSize(2.8) : Sizing.fontCaption
    // Every key the tinted-svg provider serves, which is exactly the set this
    // tile can decode in its own first frame. It used to be five hard-coded Hub
    // keys, which made the instrument blind to the Systems grid and the Settings
    // tiles -- the two places remaining pop-in would actually hide.
    readonly property bool _coverTraceResource: root._isTinted
    property double _coverTraceLoadStartedAt: 0
    // The pop-in metric itself. A cover that was never observed in
    // Image.Loading resolved inline during construction, so the tile has
    // painted artwork in the first frame it appears in -- zero blank frames,
    // which is the literal definition of no pop-in. Reset per source so a
    // recycled delegate reports on the cover it is showing now.
    // See docs/architecture.md -> "Measuring cover pop-in".
    property bool _coverEverLoading: false

    anchors.fill: parent
    // Grid tiles use the same raised-face vocabulary as modal and settings
    // controls. Activation lowers the face by the front-edge depth and holds it
    // there while navigation or launch completes; no cover-art resampling is
    // involved.
    property bool _pressed: false
    // Public read for PagedGrid's placeholder surface, which sits behind the
    // asynchronously loaded Tile and must leave the same top gap while
    // pressed. Held does not touch this — see `delegateHeld`'s doc comment;
    // held blinks the whole tile out of existence, not a face/edge change,
    // so it must not claim the pressed face gap.
    readonly property bool cardPressed: root._pressed

    // Held cue (Options -> Move) — and ONLY this: the WHOLE tile blinks
    // completely out of existence and back — no color, no tint, not even
    // the background color; genuinely nothing painted there for that
    // instant, including the focus ring, then the tile returns exactly as
    // it was. Nothing else changes: no lift, no face/edge recolor, no
    // other animation. A hard on/off cut at ~0.77 Hz, well under the 3 Hz
    // WCAG flash-safety threshold, and well within the "small area"
    // exception even covering the whole tile (one Hub tile is a small
    // fraction of the screen) — rate and area are the safety factors, not
    // the transition style.
    //
    // Implemented as `root.opacity` toggling between exactly 0 and exactly
    // 1, never animated (no `Behavior`) — opacity cascades to every child,
    // so this alone takes the cover art, caption, AND focus ring with it,
    // with no second mechanism needed. Deliberately `opacity`, not
    // `visible`: a `visible: <bound expression>` binding on a plain
    // Rectangle did not reliably reflect changes in testing (root cause
    // not identified — smelled like a Qt Quick Compiler quirk specific to
    // `visible` bindings; an otherwise identical `opacity` binding on the
    // same item updated correctly), so `opacity` is used everywhere a hard
    // binary visibility cut is needed here. 0/1 with no interpolation is
    // the same hard cut either way — nothing paints at 0, fully opaque at
    // 1, never anything between.
    //
    // `Motion.enabled: false` shows the tile normally (opacity 1) instead
    // of blinking — freezing on "invisible" would hide the very thing a
    // reduce-motion user needs to see, and there is no static middle state
    // for a disappear/reappear cue the way a color has a restable "on"
    // value. The screen-level move-mode indicator (MainLayout's help bar)
    // and the fact that the cursor tracks the held tile exactly are what
    // carry the meaning for reduce-motion users instead.
    property bool _heldBlinkOn: true
    readonly property real _heldOpacity: {
        if (!root.delegateHeld || !Motion.enabled)
            return 1;
        return root._heldBlinkOn ? 1 : 0;
    }
    opacity: root._heldOpacity
    Timer {
        interval: Motion.dur(650)
        running: root.delegateHeld && Motion.enabled
        repeat: true
        onTriggered: root._heldBlinkOn = !root._heldBlinkOn
    }
    onDelegateHeldChanged: root._heldBlinkOn = true

    // Ignore construction-time pulse resolution. `_mounted` flips one event
    // loop after completion, once delegate bindings have settled, so only a
    // genuine user activation can lower the face.
    property bool _mounted: false
    onDelegateActivatePulseChanged: {
        if (root._mounted && root._focusedSelection)
            root._pressed = true;
    }

    // A launch that remains on this screen releases the tile. Forward
    // navigation leaves it lowered until the screen's settling flag resets it.
    onDelegateReleasePulseChanged: {
        if (root._mounted && root._focusedSelection)
            root._pressed = false;
    }

    // Recycled delegates and off-screen pages must always return at rest.
    onDelegateNameChanged: root._pressed = false
    onDelegateSettlingChanged: {
        if (root.delegateSettling)
            root._pressed = false;
    }

    Component.onCompleted: {
        // Self-check the parent contract. Logs once at construction so
        // a future caller that drops Tile into a non-conforming wrapper
        // sees the failure mode immediately instead of debugging
        // mysteriously empty tiles.
        // qmllint disable missing-property compiler
        if (typeof parent.isSelected !== "boolean" || typeof parent.isFocused !== "boolean" || typeof parent.name !== "string" || typeof parent.coverKey !== "string")
            console.warn("Tile: parent does not satisfy the delegate contract " + "(expected isSelected:bool, isFocused:bool, " + "name:string, coverKey:string)");
        // Defer one event-loop pass so construction-time activate-pulse
        // transients (see onDelegateActivatePulseChanged) do not fire the cue.
        Qt.callLater(() => {
            root._mounted = true;
        });
    }

    // Cover load events go to the nearest ancestor that offers a `_coverTrace`
    // sink (MainLayout), which logs them for as long as debug logging is on.
    // Deliberately not the `_startupTrace` sink: that one goes quiet after the
    // first Hub paint, and pop-in on Systems and Settings happens later.
    // Extra arguments past `details` are forwarded; the sink is variadic.
    function _coverTrace(stage: string, details: string): void {
        if (!root._coverTraceResource)
            return;
        const extra = Array.prototype.slice.call(arguments, 1);
        let node = root.parent;
        while (node) {
            if (typeof node._coverTrace === "function") {
                node._coverTrace.apply(node, [stage, "coverKey=" + root.delegateCoverKey].concat(extra));
                return;
            }
            node = node.parent;
        }
        console.debug([stage, "coverKey=" + root.delegateCoverKey].concat(extra).join(" "));
    }

    // Raised tile surface. PressableSurface reserves its opaque front edge
    // inside the existing cell footprint, preserving grid shape and pagination.
    // All artwork, captions, badges, and the focus ring live on the moving face.
    PressableSurface {
        id: tileSurface

        objectName: "tileSurface"
        anchors.fill: parent
        radius: root._tileCornerRadius
        // A hidden or disabled tile's front edge goes neutral instead of
        // the normal accent-tinted `tileEdge` -- the same "resting card
        // edge" role PressableSurface's own static border already uses, so
        // this needs no new palette engineering. It's the glanceable cue,
        // visible on every affected tile at once regardless of focus; see
        // `caption`'s `tags` binding below and `ActiveLabel` on the owning
        // screen for the worded reason. Deliberately not opacity -- see
        // docs/plans (this round's plan) for the measured contrast numbers
        // that ruled an alpha-based cue out.
        edgeColor: (root.delegateHidden || root.delegateDisabled) ? Theme.borderMid : Theme.tileEdge
        pressed: root._pressed

        // Focus outline ring. Drawn *inside* the card edge so the ring
        // never bleeds past the cell bounds — that's the project standard:
        // borders/outlines stay within their parent rather than overflowing
        // it. Keeps the ring out of PagedGrid's clip rect at the row edges
        // and means callers don't have to reserve bleed room for it. Gated
        // on `_focusedSelection` so only the focused tile in the focused
        // section lights up — keeps multiple tile sections on screen from
        // competing for the eye. Drawn after the card so the border sits on
        // top. `_padding` is derived from the ring's own geometry
        // (`_ringInnerEdge = _outlineGap + _outlineWidth`, plus clearance
        // under `compactPadding`, or a flat pctH(2) otherwise), so content
        // can never overlap the ring regardless of which tokens change.
        // Focus ring drawn as two stacked *filled* rounded rectangles — an
        // outer accent pill and an inner surfaceCard mask that punches the
        // centre back, leaving a uniform outline. Equivalent to the older
        // single-Rectangle `border.color` + `border.width` approach but
        // significantly smoother on the corners under Qt's software
        // adaptation: filled rounded rects honour the AA path, while thin
        // rounded *borders* are tessellated without subpixel coverage and
        // step visibly at the corners (see QTBUG-123210). Both rectangles
        // are still inside the card edge by `_outlineGap`, so the ring
        // never bleeds past the cell bounds. The ring is an accent rect with a
        // surface-colored inner mask punched out of its center; both snap on and
        // off with `_focusedSelection` (no fade).
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

        // Icon area — two stacked Images for the unfocused and focused tint ramps.
        // Both share identical geometry; `coverFocus` sits above `coverBase` (z: 1)
        // and is only loaded for tinted keys (system logos, category icons, UI
        // glyphs). Real art (media-image/, custom-image/) uses only `coverBase`.
        //
        // Focus transitions are an instant visibility swap with zero async work:
        // both ramps are decoded while the tile is idle (coverBase during the
        // prefetch gate; coverFocus as soon as it enters the visible delegate pool),
        // so moving the cursor never re-requests the SVG render or drops to the
        // procedural Text fallback.
        //
        // `_focusCoverActive` suppresses coverBase when the focused ramp is on top,
        // preventing the two opaque layers from stacking their alpha on hidden tiles.
        // Top label (mixed-system media views only -- the system name above
        // the cover). `ScrollingCaption`, identically to the bottom caption
        // below: full-width, union-of-advance-and-bounds measurement (not
        // shrink-to-fit `advanceWidth` alone, which understated overflow
        // for hinted glyphs and elided harder than it needed to), and the
        // same overflow marquee on focus the bottom caption already has --
        // this used to be a bespoke Text that never scrolled and truncated
        // hard on anything longer than a couple of words. y is flush at the
        // top edge, mirroring the bottom caption's flush-bottom placement.
        ScrollingCaption {
            objectName: "tileTopLabel"
            x: root._captionSideInset
            y: Sizing.center(root._topLabelHeight, height)
            width: root._captionTextMaxWidth
            height: root._topLabelHeight
            visible: root._hasTopLabel
            centerContent: true
            focused: root._focusedSelection
            name: root.delegateTopLabel
            fontPixelSize: root._topLabelTextSize
            fontWeight: root._topLabelTextWeight
            nameColor: root._focusedSelection ? Theme.textPrimary : Theme.textLabel
        }

        Image {
            id: coverBase
            objectName: "tileCoverBase"

            width: parent.width - 2 * root._padding
            source: root._coverBaseSrc
            // Cover decode size, split by what the image IS (not whether it is
            // tinted). Real raster art decodes at the per-view `coverSourceSize` the
            // owning screen supplies — box art is portrait, so height is the bounding
            // side. That value is a stable, resolution-derived tier, NOT the live
            // painted height: Qt reloads an Image whenever sourceSize changes ("Avoid
            // changing this property dynamically"), so binding it to the fluctuating
            // box height (per layout pass, recycle, and grid retention revisit) would
            // re-decode the same cover many times over. A constant requestedSize lets
            // every reload short-circuit to the pixmap cache. Built-in tinted vector
            // assets (system logos, category/UI glyphs) instead pin a fixed 256 px
            // raster: the same logo appears at different sizes across screens, and
            // the tinted-svg image provider's cache (4 MiB cap, keyed on the
            // colors plus the resolved output size) would churn if each screen
            // requested its own size — the fixed raster consolidates every tile
            // to one cache entry per logo, and 256 is the baked size, so it is
            // also the only size that tints straight out of mapped memory with
            // no rescale.
            sourceSize.width: root._coverIsRealArt ? 0 : 256
            sourceSize.height: root._coverIsRealArt ? root.coverSourceSize : 0
            fillMode: Image.PreserveAspectFit
            smooth: true
            // Bundled artwork decodes inline on the GUI thread when the host
            // asks for it, so the tile paints its icon in the frame it appears
            // in. Everything else — real cover art and color-style system PNGs
            // — stays on the reader thread.
            asynchronous: !root._isTinted || !root.delegateCoverSynchronous
            // Real media covers get one brief reveal after decode. Tinted system,
            // category, and action artwork remains instant. Keeping this multiplier
            // separate prevents focus-ramp swaps from accidentally becoming
            // opacity animations. Hidden/disabled tiles render art at full
            // opacity -- the front edge (tileSurface.edgeColor) and the
            // caption/ActiveLabel reason carry that signal instead; see
            // this round's plan for why an opacity-based dim was dropped.
            property real revealOpacity: root._coverIsRealArt ? 0 : 1
            opacity: (coverBase.status === Image.Ready && !root._focusCoverActive) ? coverBase.revealOpacity : 0

            NumberAnimation {
                id: coverRevealAnimation
                objectName: "tileCoverRevealAnimation"

                target: coverBase
                property: "revealOpacity"
                from: 0
                to: 1
                duration: Motion.dur(Motion.pressMs)
                easing.type: Easing.OutQuad
            }

            function updateReveal(): void {
                coverRevealAnimation.stop();
                if (coverBase.status === Image.Ready && root._coverIsRealArt) {
                    coverBase.revealOpacity = 0;
                    coverRevealAnimation.restart();
                } else {
                    coverBase.revealOpacity = coverBase.status === Image.Ready ? 1 : 0;
                }
            }

            Component.onCompleted: coverBase.updateReveal()

            anchors {
                top: parent.top
                topMargin: root._hasTopLabel ? root._topLabelHeight + root._topLabelGap : root._padding
                bottom: parent.bottom
                // In caption mode the cover sits above the bottom caption strip with
                // only `_captionGap` of breathing room. The caption is flush against
                // the card's bottom edge, so the cover's lower bound is just
                // (caption height + gap) — no second layer of card padding below.
                bottomMargin: root.showCaption ? root._captionHeight + root._captionGap : root._padding
                horizontalCenter: parent.horizontalCenter
            }

            // A new source restarts the measurement. Qt emits sourceChanged
            // before it kicks off the load, so this always lands ahead of the
            // status edges below.
            onSourceChanged: {
                root._coverEverLoading = false;
                root._coverTraceLoadStartedAt = 0;
            }

            onStatusChanged: {
                coverBase.updateReveal();
                if (status === Image.Loading)
                    root._coverEverLoading = true;
                if (!root._coverTraceResource)
                    return;
                if (status === Image.Loading) {
                    root._coverTraceLoadStartedAt = Date.now();
                    root._coverTrace("startup/qml resource load start", "source=" + source);
                } else if (status === Image.Ready) {
                    const durMs = root._coverTraceLoadStartedAt > 0 ? Math.max(0, Date.now() - root._coverTraceLoadStartedAt) : 0;
                    root._coverTrace("startup/qml resource load ready", "source=" + source, "dur_ms=" + durMs, "everLoading=" + root._coverEverLoading, "paintedWidth=" + width, "paintedHeight=" + height);
                    root._coverTraceLoadStartedAt = 0;
                } else if (status === Image.Error) {
                    const durMs = root._coverTraceLoadStartedAt > 0 ? Math.max(0, Date.now() - root._coverTraceLoadStartedAt) : 0;
                    root._coverTrace("startup/qml resource load error", "source=" + source, "dur_ms=" + durMs);
                    root._coverTraceLoadStartedAt = 0;
                }
            }
        }

        // Focused-ramp variant. Only loaded for tinted keys (_isTinted); source is
        // "" for real art so this Image never initiates a fetch for boxart tiles.
        // Painted on top of coverBase (z: 1); visible only on the focused+selected
        // tile. When not yet decoded (status != Ready) opacity is 0, so coverBase
        // shows through as a fallback unfocused-ramp — no flash to text.
        Image {
            id: coverFocus

            z: 1
            width: coverBase.width
            source: root._coverFocusSrc
            sourceSize.width: 256
            fillMode: Image.PreserveAspectFit
            smooth: true
            asynchronous: !root._isTinted || !root.delegateCoverSynchronous
            visible: root._focusedSelection && root._isTinted
            opacity: coverFocus.status === Image.Ready ? 1.0 : 0

            anchors {
                top: parent.top
                topMargin: root._hasTopLabel ? root._topLabelHeight + root._topLabelGap : root._padding
                bottom: parent.bottom
                bottomMargin: root.showCaption ? root._captionHeight + root._captionGap : root._padding
                horizontalCenter: parent.horizontalCenter
            }
        }

        Image {
            id: favoriteGlyph

            anchors.left: parent.left
            anchors.top: parent.top
            anchors.leftMargin: Sizing.px(parent.width / 12)
            anchors.topMargin: Sizing.px(parent.width / 12)
            width: Sizing.px(parent.width / 6)
            height: width
            // Tinted on the fly from theme tokens (fill -> marker, keyline ->
            // markerOutline rim) via the tinted-svg provider, like every other
            // icon. The source SVG is neutral grayscale; colors live in Theme.
            // `marker` is a fixed hue independent of `accent` so a favorited
            // tile stays distinguishable from the focus ring (item 4).
            source: Resources.coverUrl("icons/Heart", Theme.marker, Theme.marker, Theme.markerOutline)
            sourceSize.width: Sizing.px(width)
            sourceSize.height: Sizing.px(height)
            fillMode: Image.PreserveAspectFit
            smooth: true
            asynchronous: false
            visible: root.delegateFavorite
        }

        // Non-caption procedural fallback. Sits at the same geometry as the
        // cover and appears only when the icon fails to load (Image.Error), not
        // while it is decoding — the slot stays blank until the icon pops in so
        // the name never flashes in first. Missing system logos use a larger
        // fitted wordmark-style treatment so the tile reads as intentional text
        // artwork, not a broken-image placeholder. In caption mode this is
        // suppressed — the bottom caption already shows the name and the
        // hourglass above signals load progress, so a wrapping copy of the name
        // in this slot is redundant.
        Text {
            objectName: "tileFallbackText"
            anchors.fill: coverBase
            anchors.margins: root._systemCover ? Sizing.pctH(1) : 0
            text: root.delegateName
            font.family: Theme.fontUi
            font.pixelSize: root._fallbackTextSize
            fontSizeMode: root._systemCover ? Text.Fit : Text.FixedSize
            minimumPixelSize: root._fallbackMinimumTextSize
            font.weight: root._systemCover ? Font.DemiBold : Font.Normal
            color: root._isBundledArtwork ? (root._focusedSelection ? Theme.logoFocusPrimary : Theme.logoPrimary) : (root._focusedSelection ? Theme.textPrimary : Theme.textLabel)
            // Wrap (not WordWrap): an unbreakable identifier like
            // `_LongCollectionName_Definitive_Cut.smc` would otherwise
            // render past `width` and bleed out of the tile.
            wrapMode: Text.Wrap
            horizontalAlignment: Text.AlignHCenter
            verticalAlignment: Text.AlignVCenter
            renderType: Text.NativeRendering
            opacity: root._fallbackVisible ? 1.0 : 0
            clip: true
        }

        // Bottom caption (caption mode only). Single line carrying the name plus an
        // inline dim suffix of disambiguating tokens; ScrollingCaption centers and
        // elides it, pins the top token after the name elides, and marquees the
        // full string while this tile is the focused selection (reduce-motion falls
        // back to a static elide).
        //
        // The strip sits flush at the card's bottom edge so the title visually owns
        // the bottom of the tile. The width clears `cornerRadius` on both sides so
        // glyphs never enter the rounded-corner region. The text lands well inside
        // the focus ring's inner mask zone, so its background stays surfaceCard even
        // on a focused tile. Tints to `textPrimary` on the focused tile so the
        // selection reads at a glance.
        ScrollingCaption {
            id: caption

            objectName: "tileCaption"
            x: root._captionSideInset
            y: parent.height - root._captionHeight
            width: root._captionTextMaxWidth
            height: root._captionHeight
            visible: root.showCaption
            centerContent: true
            focused: root._focusedSelection
            name: root.delegateName
            // Folds the "Hidden" state into the same dim suffix slot
            // disambiguating tags already use, rather than a separate
            // badge -- see this round's plan ("Tile state consolidation").
            tags: root.delegateHidden ? (root.delegateTagsSuffix !== "" ? root.delegateTagsSuffix + " · " + qsTr("Hidden") : qsTr("Hidden")) : root.delegateTagsSuffix
            fontPixelSize: root._captionTextSize
            fontWeight: root._captionTextWeight
            nameColor: root._focusedSelection ? Theme.textPrimary : Theme.textLabel
        }
    }
}
