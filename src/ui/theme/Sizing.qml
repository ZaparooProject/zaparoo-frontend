// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
pragma Singleton
import QtQuick
import Zaparoo.Browse as Browse

// Resolution-agnostic sizing helpers.
// All UI elements must use these functions rather than hardcoded pixel values.
// The UI must run correctly from 240p (CRT) through 1080p.
//
// The tables, ladders and the grid-shape solver live in Rust
// (`rust/zaparoo-app/src/sizing.rs`), pinned to this file's former behaviour
// by `tests/fixtures/sizing_golden.txt`. This file is the facade: same public
// API, same call sites, no arithmetic beyond the percentage helpers below.
//
// Those helpers stay here on purpose. A QML binding captures its dependencies
// by watching property reads while it evaluates, so `Sizing.pctH(5.5)` inside
// a binding re-evaluates on resize only because `pctH` is QML JavaScript and
// the engine sees it read `screenHeight`. A cxx-qt invokable registers
// nothing, so moving these would freeze roughly 510 call sites at their
// startup value. Derived values cross as notifying properties instead, which
// bindings do track. Anything below that calls into Rust with arguments reads
// the config inputs first for the same reason.
//
// qmllint disable compiler
// ^ cxx-qt patches `isFinal` onto singleton properties while the qmltypes
//   schema has no slot for it, so every `Browse.SizingRules.*` read trips
//   "Member can be shadowed". Confining the reads to this one file is what
//   keeps that suppression out of the ~49 files that consume Sizing.
QtObject {
    id: root

    // ── Inputs ────────────────────────────────────────────────────────
    // Written by Main.qml (scene dimensions) and MainLayout.qml (rendering
    // path, orientation, interface profile). Kept as QML properties so those
    // Binding blocks, and the tests that assign directly, are untouched; the
    // change handlers below forward them to Rust.
    property real screenWidth: 640
    property real screenHeight: 480
    property bool crtNativePath: false
    property bool bitmapType: false
    property bool swapPercentageAxes: false
    property string interfaceProfile: "standard"

    // Viewport the detail-cover tier is computed from. Defaults to the full
    // scene; Main.qml binds these to the games-grid viewport so the Core
    // fetch request and the QML decode tier consume identical dimensions. If
    // they ever diverged, a fetched tier could mismatch the decode tier and
    // miss the pixmap cache.
    property real detailCoverViewportWidth: screenWidth
    property real detailCoverViewportHeight: screenHeight

    onScreenWidthChanged: Browse.SizingRules.screen_width = root.screenWidth
    onScreenHeightChanged: Browse.SizingRules.screen_height = root.screenHeight
    onCrtNativePathChanged: Browse.SizingRules.crt_native_path = root.crtNativePath
    onBitmapTypeChanged: Browse.SizingRules.bitmap_type = root.bitmapType
    onSwapPercentageAxesChanged: Browse.SizingRules.swap_percentage_axes = root.swapPercentageAxes
    onInterfaceProfileChanged: Browse.SizingRules.interface_profile = root.interfaceProfile

    // Change handlers only fire on change, so seed the initial state once.
    // Rust's defaults already match the property defaults above, which is what
    // keeps the first binding pass consistent if this runs late.
    Component.onCompleted: {
        Browse.SizingRules.screen_width = root.screenWidth;
        Browse.SizingRules.screen_height = root.screenHeight;
        Browse.SizingRules.crt_native_path = root.crtNativePath;
        Browse.SizingRules.bitmap_type = root.bitmapType;
        Browse.SizingRules.swap_percentage_axes = root.swapPercentageAxes;
        Browse.SizingRules.interface_profile = root.interfaceProfile;
    }

    // ── Percentage helpers ────────────────────────────────────────────
    // One line each, no tables, no loops. See the header for why they stay.
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

    // Minimum 8px to remain legible on 240p displays. Kept for specialist
    // text and geometry; ordinary text chooses one of the six roles below.
    // Stays in QML because callers pass literals inside bindings.
    function fontSize(percent: real): int {
        const size = Math.max(8, pctH(percent));
        if (!bitmapType)
            return size;
        return size < 12 ? 8 : 16;
    }

    // ── Derived values ────────────────────────────────────────────────
    readonly property bool handheldProfile: Browse.SizingRules.handheld_profile
    readonly property int effectiveHeight: Browse.SizingRules.effective_height
    readonly property int resolutionHeight: Browse.SizingRules.resolution_height
    readonly property string tier: Browse.SizingRules.tier

    readonly property int radiusMd: Browse.SizingRules.radius_md
    readonly property int radiusSm: Browse.SizingRules.radius_sm
    readonly property bool cornerAntialiasing: Browse.SizingRules.corner_antialiasing

    readonly property int fontHero: Browse.SizingRules.font_hero
    readonly property int fontTitle: Browse.SizingRules.font_title
    readonly property int fontSection: Browse.SizingRules.font_section
    readonly property int fontBody: Browse.SizingRules.font_body
    readonly property int fontCaption: Browse.SizingRules.font_caption
    readonly property int fontSmall: Browse.SizingRules.font_small

    readonly property int cardBorderWidth: Browse.SizingRules.card_border_width
    readonly property int focusBorderWidth: Browse.SizingRules.focus_border_width
    readonly property int focusRingWidth: Browse.SizingRules.focus_ring_width
    readonly property int pressEdgeHeight: Browse.SizingRules.press_edge_height

    readonly property int helpBarHeight: Browse.SizingRules.help_bar_height
    readonly property int helpBarClearance: Browse.SizingRules.help_bar_clearance
    readonly property int visibleCovers: Browse.SizingRules.visible_covers

    readonly property int hubGridColumns: Browse.SizingRules.hub_grid_columns
    readonly property int hubGridRows: Browse.SizingRules.hub_grid_rows
    readonly property int _hubActiveLabelHeight: Browse.SizingRules.hub_active_label_height
    readonly property int _hubGridTopMargin: Browse.SizingRules.hub_grid_top_margin
    readonly property int _hubGridHeightBudget: Browse.SizingRules.hub_grid_height_budget
    readonly property int _hubGridSideInset: Browse.SizingRules.hub_grid_side_inset
    readonly property int _hubGridColumnGap: Browse.SizingRules.hub_grid_column_gap
    readonly property int _hubGridTopInset: Browse.SizingRules.hub_grid_top_inset
    readonly property int _hubGridBottomInset: Browse.SizingRules.hub_grid_bottom_inset
    readonly property int _hubGridRowGap: Browse.SizingRules.hub_grid_row_gap
    readonly property int _hubGridWidthFit: Browse.SizingRules.hub_grid_width_fit
    readonly property int _hubGridHeightFit: Browse.SizingRules.hub_grid_height_fit
    readonly property int hubTileSize: Browse.SizingRules.hub_tile_size
    readonly property int hubTileWidth: Browse.SizingRules.hub_tile_width
    readonly property int hubTileHeight: Browse.SizingRules.hub_tile_height

    readonly property int systemsGridColumns: Browse.SizingRules.systems_grid_columns
    readonly property int systemsGridRows: Browse.SizingRules.systems_grid_rows
    readonly property int gamesGridColumns: Browse.SizingRules.games_grid_columns
    readonly property int gamesGridRows: Browse.SizingRules.games_grid_rows

    readonly property int headerRowHeight: Browse.SizingRules.header_row_height
    readonly property int headerStackGap: Browse.SizingRules.header_stack_gap
    readonly property int headerTopMargin: Browse.SizingRules.header_top_margin
    readonly property int headerSideMargin: Browse.SizingRules.header_side_margin
    readonly property int headerHeight: Browse.SizingRules.header_height
    readonly property int headerBottom: Browse.SizingRules.header_bottom

    // Live detail-cover decode width for Image.sourceSize consumers (detail
    // panes and their neighbour-prefetch pools). One binding so every consumer
    // decodes at the same width: a prefetch at a different sourceSize would
    // populate a different pixmap-cache entry and never be hit by the visible
    // cover.
    // A binding rather than a Rust property: the arguments are QML properties,
    // so the binding tracks them directly, and there is no second copy of the
    // value in Rust that a resize could leave stale.
    readonly property int detailCoverSourceWidth: root.detailCoverSourceSize(root.detailCoverViewportWidth, root.detailCoverViewportHeight)

    // ── Argument-taking helpers ───────────────────────────────────────
    // Pure in their argument, so a binding that computes the argument already
    // carries the dependency and these can go straight to Rust.
    function snapCoverTier(px: real): int {
        return Browse.SizingRules.snap_cover_tier(px);
    }

    function snapLogoWidth(px: real): int {
        return Browse.SizingRules.snap_logo_width(px);
    }

    function maxExpressibleCoverTier(viewportWidth: int): int {
        return Browse.SizingRules.max_expressible_cover_tier(viewportWidth);
    }

    // Everything below passes the scene configuration to Rust explicitly, and
    // that is the whole point rather than an accident of the signature. A QML
    // binding registers a dependency by watching property reads while it
    // evaluates, and a cxx-qt invokable reports nothing. So a binding that
    // calls one of these with constant viewport arguments would freeze on the
    // next resize, rotation or CRT switch unless the call itself reads the
    // properties it depends on. Reading them and throwing the result away does
    // not work: the QML compiler elides that, and `tst_sizing.qml`'s
    // `constantArgs` probe catches it when it does.
    function gamesGridShape(viewportWidth: int, viewportHeight: int): var {
        return {
            "columns": Browse.SizingRules.games_grid_columns_for(viewportWidth, viewportHeight, root.screenWidth, root.screenHeight, root.crtNativePath, root.swapPercentageAxes),
            "rows": Browse.SizingRules.games_grid_rows_for(viewportWidth, viewportHeight, root.screenWidth, root.screenHeight, root.crtNativePath, root.swapPercentageAxes)
        };
    }

    function systemsGridShape(viewportWidth: int, viewportHeight: int): var {
        return {
            "columns": Browse.SizingRules.systems_grid_columns_for(viewportWidth, viewportHeight, root.screenWidth, root.screenHeight, root.crtNativePath, root.swapPercentageAxes),
            "rows": Browse.SizingRules.systems_grid_rows_for(viewportWidth, viewportHeight, root.screenWidth, root.screenHeight, root.crtNativePath, root.swapPercentageAxes)
        };
    }

    // Stable per-view cover decode size, snapped to a Core tier. One source of
    // truth for both the Core fetch request (Main.qml) and the grid tile's
    // Image.sourceSize (Tile.qml), so request size equals decode size.
    function gamesGridCoverSourceSize(viewportWidth: int, viewportHeight: int): int {
        return Browse.SizingRules.games_grid_cover_source_size(viewportWidth, viewportHeight, root.screenWidth, root.screenHeight, root.crtNativePath, root.swapPercentageAxes);
    }

    // Detail-pane cover decode size: about twice the grid cover, snapped to
    // its own tier and capped at the largest tier the viewport can express.
    // Without the cap a CRT-native scene requests decodes the framebuffer can
    // never display, at roughly 1.1 MB of decoded cache each.
    function detailCoverSourceSize(viewportWidth: int, viewportHeight: int): int {
        return Browse.SizingRules.detail_cover_source_size(viewportWidth, viewportHeight, root.screenWidth, root.screenHeight, root.crtNativePath, root.swapPercentageAxes);
    }

    // Hand-declared page geometry for CRT and the common framebuffer sizes;
    // null when the scene has none and the adaptive scorer runs instead.
    function _declaredGridShape(kind: string): var {
        const columns = Browse.SizingRules.declared_grid_columns(kind, root.screenWidth, root.screenHeight, root.crtNativePath, root.swapPercentageAxes);
        if (columns < 0)
            return null;
        return {
            "columns": columns,
            "rows": Browse.SizingRules.declared_grid_rows(kind, root.screenWidth, root.screenHeight, root.crtNativePath, root.swapPercentageAxes)
        };
    }

    function _gamesGridCoverBox(viewportWidth: int, viewportHeight: int): int {
        return Browse.SizingRules.games_grid_cover_box(viewportWidth, viewportHeight, root.screenWidth, root.screenHeight, root.crtNativePath, root.swapPercentageAxes);
    }
}
