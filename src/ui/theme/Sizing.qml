// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
pragma Singleton
import QtQuick

// Resolution-agnostic sizing helpers.
// All UI elements must use these functions rather than hardcoded pixel values.
// The UI must run correctly from 240p (CRT) through 1080p.
QtObject {
    id: root

    // Reference window dimensions — updated by Main.qml on start and resize.
    property real screenWidth: 640
    property real screenHeight: 480
    property bool crtNativePath: false
    property bool bitmapType: false
    property bool swapPercentageAxes: false
    property string interfaceProfile: "standard"
    readonly property bool handheldProfile: interfaceProfile === "handheld"

    // Shape and type hierarchy use discrete logical-resolution tiers. In TATE
    // the scene dimensions are swapped, so read the original framebuffer's
    // height axis rather than promoting 720p portrait to the 1080 tier.
    readonly property int effectiveHeight: swapPercentageAxes ? screenWidth : screenHeight
    // CRT scenes expose the action-safe canvas after 5% has been removed
    // from each edge. Recover the framebuffer's resolution axis before tier
    // selection so rendering path cannot demote future 540p/720p CRT modes.
    readonly property int resolutionHeight: crtNativePath ? Math.round(effectiveHeight / 0.9) : effectiveHeight
    // Resolution tier is independent of output/rendering path. CRT-specific
    // safe areas, bitmap type, and browse profiles key off crtNativePath or
    // bitmapType directly.
    readonly property string tier: resolutionHeight >= 900 ? "1080" : resolutionHeight >= 660 ? "720" : resolutionHeight >= 520 ? "540" : resolutionHeight >= 400 ? "480" : "240"

    // Discrete shape tokens. Rounded-square geometry is deliberately not a
    // percentage: resolution changes thickness, not the design's silhouette.
    readonly property int radiusMd: tier === "1080" ? 8 : tier === "720" ? 6 : tier === "540" ? 4 : tier === "480" ? 3 : 2
    readonly property int radiusSm: Math.max(1, half(radiusMd))
    // At 240p these radii represent a deliberate one-pixel corner cut.
    // Coverage antialiasing softens adjacent pixels without adding useful
    // shape information, so rounded-square surfaces use crisp raster edges.
    readonly property bool cornerAntialiasing: tier !== "240"

    // Six semantic text roles. Bitmap type (CRT, or embedded 240p without
    // --crt) keeps the bitmap face's existing 8/16px quantization by
    // resolving through fontSize(); other paths use a tiered ladder so
    // adjacent roles never collapse at 240p/480p/540p.
    readonly property int fontHero: bitmapType ? fontSize(4.0) : _fontForTier(0)
    readonly property int fontTitle: bitmapType ? fontSize(3.2) : _fontForTier(1)
    readonly property int fontSection: bitmapType ? fontSize(2.9) : _fontForTier(2)
    readonly property int fontBody: bitmapType ? fontSize(2.6) : _fontForTier(3)
    readonly property int fontCaption: bitmapType ? fontSize(2.4) : _fontForTier(4)
    readonly property int fontSmall: bitmapType ? fontSize(2.2) : _fontForTier(5)

    // Semantic thickness ladder. Every helper returns an integer and clamps to
    // at least one physical pixel through stroke().
    readonly property int cardBorderWidth: stroke(pctH(0.2))
    readonly property int focusBorderWidth: stroke(pctH(0.4))
    readonly property int focusRingWidth: stroke(pctH(0.6))
    readonly property int pressEdgeHeight: stroke(pctH(0.8))

    // Compact screens may need two atomic icon+label help groups. Reserve two
    // rows at 240p; larger tiers keep the single-line footer.
    readonly property int helpBarHeight: tier === "240" ? pctH(10) : pctH(6)
    readonly property int helpBarClearance: helpBarHeight + pctH(2)

    // Visible tile-row covers: fewer at very low resolution to avoid crowding.
    readonly property int visibleCovers: effectiveHeight < 300 ? 3 : 5

    // Hub grid shape — a FIXED table keyed only on `tier`, deliberately NOT
    // run through `_selectGridShape`'s adaptive viewport scorer the way
    // Systems/Games are. See docs/plans/ui-geometry-refresh.md -> section 9
    // ("Forward compatibility: Hub as a Wii/3DS-style menu"): a user-arranged
    // Hub layout must not reflow when the window/display changes, only when
    // the discrete `tier` itself changes — an adaptive fit would silently
    // scramble hand-placed tiles on a display or CRT switch.
    //
    // `rows` (round 6 follow-up): the original "2 at every tier" carried
    // over the old two-row design's *visual shape* unexamined, not what
    // actually fits — checked against real content (5 categories + up to 5
    // actions = 10 items today), `1080`/`720`/`540` at 5x2 already have zero
    // headroom for one more item, and `480`/`240` already overflow a single
    // page. Hub tiles carry no embedded caption text (HubScreen's delegate
    // is a bare `Tile{}`; the selected item's name only ever shows in
    // `ActiveLabel` below the grid), so they don't need to hold
    // Systems/Games' 160px minimum cell width — closer to the 72px floor
    // Systems/Games themselves accept on low-resolution CRT layouts. 3 rows
    // clears that relaxed floor comfortably at `1080`/`720`/`540`
    // (224/150/112px); `480`/`240` stay at 2 rows (a 3-row 240p cell would
    // be 49px, under even the CRT floor). Pagination (PagedGrid already pages
    // for free) is the intended overflow path at every tier, not a
    // fallback — see HubScreen.qml's `items` construction.
    //
    // Column count at `540`/`720`/`1080` (round follow-up): at 3 rows,
    // `squareCells` clamps every tier's cell to the *height* fit, not the
    // width fit — checked against each tier's canonical scene, 3 rows
    // already bind the cell before columns even enter it (~106/143/212px at
    // 540/720/1080), leaving the width fit well above that at 5 columns,
    // i.e. real unused horizontal slack at every one of these three tiers,
    // not just 540. 7 columns is the largest count whose width fit still
    // clears the height-bound cell at all three (no shrink, or a ~2px
    // rounding-noise shrink on a busy 720p page — nothing perceptible); 8
    // columns drops the width fit under the height-bound cell at every
    // tier (shrinks it), so 7 is the ceiling, not a per-tier judgment call.
    // Rows stay at 3: unlike columns, rows have zero slack by construction
    // — `heightFit(rows)` already divides the full vertical budget, so
    // going to 4 rows shrinks the cell directly, which is exactly the
    // "scaling them" this table exists to avoid. `480`/`240` are unrelated
    // tiers (2 rows, tighter budget) and keep their own counts. Both NTSC
    // 240p and PAL 288p resolve to the `240` tier and have room for 4x2.
    // This keys page geometry to resolution, not CRT mode, so future
    // higher-resolution CRT modes scale normally.
    // Handheld changes page density, not persisted order: the same linear Hub
    // slots reflow into fewer columns without resetting user arrangements.
    // Low-resolution tiers already use the compact 4x2 shape.
    readonly property var _hubGridBaseShape: root.handheldProfile && tier !== "240" && tier !== "480" ? root._gridShape(4, 3) : root._gridShape(tier === "240" || tier === "480" ? 4 : 7, tier === "240" || tier === "480" ? 2 : 3)
    readonly property var hubGridShape: swapPercentageAxes ? root._gridShape(_hubGridBaseShape.rows, _hubGridBaseShape.columns) : _hubGridBaseShape
    readonly property int hubGridColumns: hubGridShape.columns
    readonly property int hubGridRows: hubGridShape.rows

    // Hub tile dimensions resolved from its column/row fits and reserved
    // vertical band (header + label strip + minimum gaps). Pure functions of
    // screenWidth/screenHeight/tier let Settings reuse them without a live
    // HubScreen, keeping both grids visually matched. Keep these formulas in
    // sync with PagedGrid's per-axis fits and HubScreen's band math.
    readonly property int _hubActiveLabelHeight: tier === "240" ? 8 : pctH(7)
    readonly property int _hubGridTopMargin: tier === "240" ? pctH(1) : 0
    readonly property int _hubGridHeightBudget: Math.max(0, tier === "240" ? screenHeight - headerBottom - helpBarHeight - _hubActiveLabelHeight - _hubGridTopMargin : screenHeight - headerBottom - helpBarHeight - _hubActiveLabelHeight - 3 * pctH(2))
    readonly property int _hubGridSideInset: tier === "240" ? headerSideMargin : pctW(3)
    readonly property int _hubGridColumnGap: tier === "240" ? 4 : pctW(2)
    readonly property int _hubGridTopInset: tier === "240" ? 2 : pctH(2)
    readonly property int _hubGridBottomInset: tier === "240" ? 4 : pctH(2)
    readonly property int _hubGridRowGap: tier === "240" ? 4 : pctH(4)
    readonly property int _hubGridWidthFit: Math.max(0, Math.floor((screenWidth - 2 * _hubGridSideInset - (hubGridColumns - 1) * _hubGridColumnGap) / hubGridColumns))
    readonly property int _hubGridHeightFit: Math.max(0, Math.floor((_hubGridHeightBudget - _hubGridTopInset - _hubGridBottomInset - (hubGridRows - 1) * _hubGridRowGap) / hubGridRows))
    readonly property int hubTileSize: Math.min(_hubGridWidthFit, _hubGridHeightFit)
    // Four-column low-resolution Hub pages are width-bound. Let tile height
    // use its independent fit so top/inter-row/bottom gaps stay balanced;
    // Settings consumes these same resolved dimensions below. Larger tiers
    // keep the established square tile.
    readonly property int hubTileWidth: tier === "240" ? _hubGridWidthFit : hubTileSize
    readonly property int hubTileHeight: tier === "240" ? _hubGridHeightFit : hubTileSize

    // Shared browse-grid bounds. Systems and games both solve the same
    // viewport-fit problem now, so the common limits live here and the
    // per-surface configs only override what is materially different.
    readonly property var _browseGridBaseConfig: _browseGridBaseConfigForTheme()
    // Systems grid uses the same viewport-driven shape selection as
    // games so both browse screens present a similar amount of content.
    // Systems tiles are squarer than box-art tiles, so they target a
    // slightly wider aspect while keeping the same preferred page size.
    readonly property var _systemsGridConfig: _gridConfig(_browseGridBaseConfig, {
        "minCellHeight": crtNativePath ? 72 : 140,
        "preferredPageSize": crtNativePath ? 9 : 12,
        "targetAspect": 1.25
    })
    readonly property var _systemsGridShape: systemsGridShape(screenWidth, screenHeight)
    // qmllint disable compiler
    readonly property int systemsGridColumns: _systemsGridShape.columns
    readonly property int systemsGridRows: _systemsGridShape.rows
    // qmllint enable compiler
    // Games grid shape comes from the logical viewport, not from
    // screen-height-only breakpoints. The selector preserves a stable
    // tile aspect while respecting a minimum readable tile size, so
    // rotating the scene changes how many tiles fit without stretching
    // the cards into a different shape.
    readonly property var _gamesGridConfig: _gridConfig(_browseGridBaseConfig, {
        // A 1080p MiSTer output renders through a 960x540 framebuffer. At that
        // logical height, 31.5% resolves to 170px and preserves the normal
        // five-column, two-row page instead of falling back to 2x2.
        "minCellHeight": crtNativePath ? 96 : pctH(31.5),
        "targetAspect": crtNativePath ? 0.78 : 0.71
    })
    readonly property var _gamesGridShape: gamesGridShape(screenWidth, screenHeight)
    // qmllint disable compiler
    readonly property int gamesGridColumns: _gamesGridShape.columns
    readonly property int gamesGridRows: _gamesGridShape.rows
    // qmllint enable compiler
    // ── Top header (logo + status row + status pill) ──────────────────
    // Single source of truth for the header bar that sits at the top of
    // every screen. The logo's height is locked to the stacked-row
    // total so the brand mark sits flush with the top of the status
    // row and the bottom of the pill row, even when the pill is idle
    // (its space is reserved). Screen content clears `headerBottom`.
    readonly property int headerRowHeight: fontSize(3.4)
    readonly property int headerStackGap: pctH(0.8)
    readonly property int headerTopMargin: pctH(2)
    readonly property int headerSideMargin: pctW(2)
    readonly property int headerHeight: 2 * headerRowHeight + headerStackGap
    readonly property int headerBottom: headerTopMargin + headerHeight

    function _browseGridBaseConfigForTheme(): var {
        return {
            "minCellWidth": crtNativePath ? 72 : 160,
            "preferredPageSize": crtNativePath ? 6 : 10,
            "minColumns": 2,
            "maxColumns": crtNativePath ? 3 : 5,
            "minRows": 2,
            "maxRows": crtNativePath ? 3 : 5
        };
    }

    function pctH(percent: real): int {
        return Math.round((swapPercentageAxes ? screenWidth : screenHeight) * percent / 100);
    }

    function pctW(percent: real): int {
        return Math.round((swapPercentageAxes ? screenHeight : screenWidth) * percent / 100);
    }

    function px(value: real): int {
        return Math.round(value);
    }

    function stroke(value: real): int {
        return Math.max(1, px(value));
    }

    function center(parentSize: real, childSize: real): int {
        return px((parentSize - childSize) / 2);
    }

    function half(value: real): int {
        return px(value / 2);
    }

    function gamesGridShape(viewportWidth: int, viewportHeight: int): var {
        const declared = root._declaredGridShape("games");
        return declared === null ? root._selectGridShape(viewportWidth, viewportHeight, root._gamesGridConfig) : declared;
    }

    // Cover decode tiers, mirroring Core's resize ladder. A cover's source
    // size must be one of these so the QML-decoded texture matches the WebP
    // Core delivers (no resample) and small resolution wobble does not move the
    // tier. Snaps up to the smallest tier that covers `px`.
    function snapCoverTier(px: real): int {
        if (px <= 128)
            return 128;
        if (px <= 256)
            return 256;
        if (px <= 512)
            return 512;
        return 768;
    }

    // Header-logo asset ladder (resources/images/logo/logo-<variant>-<w>.png).
    // Pre-sized rungs so HeaderBar decodes close to its painted width instead
    // of bilinearly downscaling the 600px master at paint time. Snaps up to
    // the smallest rung that covers `px`, mirroring snapCoverTier.
    function snapLogoWidth(px: real): int {
        if (px <= 96)
            return 96;
        if (px <= 144)
            return 144;
        if (px <= 192)
            return 192;
        if (px <= 256)
            return 256;
        if (px <= 384)
            return 384;
        return 600;
    }

    // Raw painted height (px) of a games-grid cover in caption mode: raised
    // face height minus top padding and bottom caption band. Box art is
    // portrait, so this height is the bounding side. Pure function of the
    // resolution — the same value for every grid tile — so callers use it as a
    // stable Image.sourceSize input rather than the live painted height, which
    // fluctuates per layout/recycle and would make Qt reload the Image on every
    // change. Percentage bands track Tile.qml's front edge, _padding,
    // _captionHeight, and _captionGap; exact agreement is not required since
    // the result is snapped.
    // qmllint disable compiler
    function _gamesGridCoverBox(viewportWidth: int, viewportHeight: int): int {
        const shape = gamesGridShape(viewportWidth, viewportHeight);
        const tileHeight = Math.ceil(Math.max(1, viewportHeight) / Math.max(1, shape.rows));
        const coverBox = tileHeight - pressEdgeHeight - pctH(2) - (pctH(5.5) + pctH(0.4));
        return Math.max(1, coverBox);
    }
    // qmllint enable compiler

    // Stable per-view cover decode size (px), snapped to a Core tier. One source
    // of truth for both the Core fetch request (Main.qml) and the grid tile's
    // Image.sourceSize (Tile.qml), so request size == decode size.
    function gamesGridCoverSourceSize(viewportWidth: int, viewportHeight: int): int {
        return snapCoverTier(_gamesGridCoverBox(viewportWidth, viewportHeight));
    }

    // Detail-pane cover decode size: ~2x the grid cover, snapped to its own
    // tier. The detail view paints one large cover, so it lands a tier above the
    // grid (e.g. 768 vs 512 at 1080p), keeping the two views distinct. A future
    // metadata modal adds its own ...CoverSourceSize here.
    //
    // Capped at the largest tier the viewport can actually express: on a
    // CRT-native scene (~316 px wide after safe-area insets) the doubled
    // tier used to request 512-wide decodes that the framebuffer can never
    // display. Each such decode costs a resample step and a ~1.1 MB
    // decoded-cache entry, so the 64 MB cache held only ~57 covers and
    // list scrolling re-decoded on every pass. The cap keys off the live
    // viewport, so HDMI and desktop scenes (>=768 px wide) are unchanged.
    function detailCoverSourceSize(viewportWidth: int, viewportHeight: int): int {
        const doubled = snapCoverTier(_gamesGridCoverBox(viewportWidth, viewportHeight) * 2);
        return Math.min(doubled, maxExpressibleCoverTier(viewportWidth));
    }

    // Viewport the detail-cover tier is computed from. Defaults to the
    // full scene; Main.qml binds these to the games-grid viewport (the
    // scene minus header strips and margins) so the Core fetch request
    // and the QML decode tier consume identical dimensions — if they
    // ever diverged, a fetched tier could mismatch the decode tier and
    // miss the pixmap cache.
    property real detailCoverViewportWidth: screenWidth
    property real detailCoverViewportHeight: screenHeight

    // Live detail-cover decode width for Image.sourceSize consumers
    // (detail panes and their neighbour-prefetch pools). One binding so
    // every consumer decodes at the same width — a prefetch at a
    // different sourceSize would populate a different pixmap-cache
    // entry and never be hit by the visible cover.
    readonly property int detailCoverSourceWidth: detailCoverSourceSize(detailCoverViewportWidth, detailCoverViewportHeight)

    // Largest cover tier that is not wider than the viewport itself: a
    // decode wider than the scene can only ever be shown downscaled, so
    // requesting it wastes resample time and decoded-cache bytes.
    function maxExpressibleCoverTier(viewportWidth: int): int {
        if (viewportWidth >= 768)
            return 768;
        if (viewportWidth >= 512)
            return 512;
        if (viewportWidth >= 256)
            return 256;
        return 128;
    }

    function systemsGridShape(viewportWidth: int, viewportHeight: int): var {
        // System logos need more vertical capacity than the compact 240p
        // landscape grid provides. Both rotated layouts use a fixed 2x3 page.
        if (root.swapPercentageAxes)
            return root._gridShape(2, 3);
        const declared = root._declaredGridShape("systems");
        return declared === null ? root._selectGridShape(viewportWidth, viewportHeight, root._systemsGridConfig) : declared;
    }

    // Common framebuffer sizes have declared page geometry. Rotated games
    // transpose the horizontal shape; systems use the explicit portrait shape
    // above.
    function _declaredGridShape(kind: string): var {
        if (root.crtNativePath) {
            if (kind === "systems")
                return root._orientedGridShape(3, 3);
            return root._orientedGridShape(3, 2);
        }

        const common = root._commonDigitalScene();
        if (common === "")
            return null;
        if (kind === "systems") {
            if (common === "240")
                return root._orientedGridShape(2, 2);
            if (common === "480")
                return root._orientedGridShape(3, 3);
            return root._orientedGridShape(4, 3);
        }
        if (common === "240")
            return root._orientedGridShape(3, 2);
        if (common === "480")
            return root._orientedGridShape(4, 2);
        return root._orientedGridShape(5, 2);
    }

    function _orientedGridShape(columns: int, rows: int): var {
        return root.swapPercentageAxes ? root._gridShape(rows, columns) : root._gridShape(columns, rows);
    }

    function _gridShape(columns: int, rows: int): var {
        return {
            "columns": columns,
            "rows": rows
        };
    }

    function _commonDigitalScene(): string {
        const close = (actual, expected) => Math.abs(actual - expected) <= 2;
        const width = root.swapPercentageAxes ? root.screenHeight : root.screenWidth;
        const height = root.swapPercentageAxes ? root.screenWidth : root.screenHeight;
        if (close(width, 320) && close(height, 240))
            return "240";
        if (close(width, 640) && close(height, 480))
            return "480";
        if (close(width, 960) && close(height, 540))
            return "540";
        if (close(width, 1280) && close(height, 720))
            return "720";
        if (close(width, 1366) && close(height, 768))
            return "720";
        if (close(width, 1920) && close(height, 1080))
            return "1080";
        return "";
    }

    function _gridConfig(base: var, overrides: var): var {
        const merged = {};
        for (const key in base)
            merged[key] = base[key];
        for (const key in overrides)
            merged[key] = overrides[key];
        return merged;
    }

    // qmllint disable compiler
    function _selectGridShape(viewportWidth: int, viewportHeight: int, options: var): var {
        const safeWidth = Math.max(1, viewportWidth);
        const safeHeight = Math.max(1, viewportHeight);
        let bestColumns = options.minColumns;
        let bestRows = options.minRows;
        let bestScore = Number.MAX_VALUE;

        for (let columns = options.minColumns; columns <= options.maxColumns; columns++) {
            const cellWidth = safeWidth / columns;
            if (cellWidth < options.minCellWidth)
                continue;
            for (let rows = options.minRows; rows <= options.maxRows; rows++) {
                const cellHeight = safeHeight / rows;
                if (cellHeight < options.minCellHeight)
                    continue;
                const aspect = cellWidth / cellHeight;
                const aspectError = Math.abs(Math.log(aspect / options.targetAspect));
                const pagePenalty = Math.abs((columns * rows) - options.preferredPageSize) * 0.04;
                const score = aspectError + pagePenalty;
                if (score < bestScore) {
                    bestScore = score;
                    bestColumns = columns;
                    bestRows = rows;
                }
            }
        }

        return {
            "columns": bestColumns,
            "rows": bestRows
        };
    }
    // qmllint enable compiler

    function _fontForTier(role: int): int {
        const table = tier === "1080" ? [43, 35, 31, 28, 26, 24] : tier === "720" ? [29, 23, 21, 19, 17, 16] : tier === "540" ? [24, 20, 18, 17, 15, 14] : tier === "480" ? [22, 18, 17, 16, 14, 13] : [14, 12, 11, 10, 9, 8];
        return table[Math.max(0, Math.min(table.length - 1, role))];
    }

    // Minimum 8px to remain legible on 240p displays. Kept for specialist
    // text and geometry; ordinary text chooses one of the six roles above.
    function fontSize(percent: real): int {
        const size = Math.max(8, pctH(percent));
        if (!bitmapType)
            return size;
        return size < 12 ? 8 : 16;
    }
}
