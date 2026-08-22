// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

import QtQuick
import QtTest
import Zaparoo.Theme
import Zaparoo.App

// Resolution-agnostic sizing contract: pctH/pctW/fontSize must scale with the
// Main window's screenWidth/screenHeight, and visibleCovers must honour the
// 240p special-case (3 covers instead of 5).
TestCase {
    name: "UiSizing"
    when: windowShown

    Main {
        id: main
        fullScreen: false
        width: 1280
        height: 720
    }

    function cleanup(): void {
        main.debugCrtSafeAreaOverlay = false;
        main.crtNativePath = false;
        main.bitmapType = false;
        setResolution(1280, 720);
    }

    function setResolution(w: int, h: int): void {
        setResolutionExpect(w, h, w, h);
    }

    function setResolutionExpect(w: int, h: int, expectedW: int, expectedH: int): void {
        main.width = w;
        main.height = h;
        // Main.qml's onWidthChanged/onHeightChanged propagate to Sizing.
        tryCompare(Sizing, "screenWidth", expectedW);
        tryCompare(Sizing, "screenHeight", expectedH);
    }

    function crtSafeWidth(w: int): int {
        return w - 2 * Math.round(w * 0.05);
    }

    function crtSafeHeight(h: int): int {
        return h - 2 * Math.round(h * 0.05);
    }

    function test_pct_helpers_scale_with_window_size(): void {
        setResolution(1920, 1080);
        compare(Sizing.pctH(10), 108);
        compare(Sizing.pctW(50), 960);
        compare(Sizing.pctH(100), 1080);

        setResolution(1280, 720);
        compare(Sizing.pctH(10), 72);
        compare(Sizing.pctW(50), 640);

        setResolution(320, 240);
        compare(Sizing.pctH(10), 24);
        compare(Sizing.pctW(50), 160);
    }

    function test_font_size_respects_minimum_for_240p(): void {
        setResolution(320, 240);
        // pctH(2) would be 5 at 240p, but fontSize clamps to 8.
        verify(Sizing.fontSize(2) >= 8, "fontSize must never fall below 8px for CRT legibility");
        // A larger percent still scales above the floor.
        compare(Sizing.fontSize(10), 24);
    }

    function test_visible_covers_drops_to_three_on_240p(): void {
        setResolution(320, 240);
        compare(Sizing.visibleCovers, 3);

        setResolution(1280, 720);
        compare(Sizing.visibleCovers, 5);

        setResolution(1920, 1080);
        compare(Sizing.visibleCovers, 5);
    }

    function test_debug_crt_safe_area_guide_visibility(): void {
        main.debugCrtSafeAreaOverlay = false;
        main.crtNativePath = true;
        setResolutionExpect(320, 240, crtSafeWidth(320), crtSafeHeight(240));
        compare(main._debugCrtSafeAreaGuideVisible, false);

        main.debugCrtSafeAreaOverlay = true;
        compare(main._debugCrtSafeAreaGuideVisible, true);

        main.crtNativePath = false;
        compare(main._debugCrtSafeAreaGuideVisible, false);

        main.crtNativePath = true;
        setResolutionExpect(640, 480, crtSafeWidth(640), crtSafeHeight(480));
        compare(main._debugCrtSafeAreaGuideVisible, false);

        setResolutionExpect(640, 288, crtSafeWidth(640), crtSafeHeight(288));
        compare(main._debugCrtSafeAreaGuideVisible, true);

        main.debugCrtSafeAreaOverlay = false;
        main.crtNativePath = false;
    }

    function test_sizing_updates_propagate_proportionally(): void {
        setResolution(1280, 720);
        var baseline = Sizing.pctH(10);
        compare(baseline, 72);

        setResolution(1920, 1080);
        var scaled = Sizing.pctH(10);
        // 1080/720 = 1.5 → 72 * 1.5 = 108. Allow ±1px for rounding.
        verify(Math.abs(scaled - baseline * 1.5) <= 1, "pctH scaling should track screen height proportionally");
    }

    // Pure function of its argument -- no resolution harness needed. Mirrors
    // snapCoverTier's "snap up to the smallest rung that covers px" contract
    // for the header-logo PNG ladder (resources/images/logo/).
    function test_snap_logo_width_rounds_up_to_ladder_rung(): void {
        compare(Sizing.snapLogoWidth(1), 96);
        compare(Sizing.snapLogoWidth(96), 96);
        compare(Sizing.snapLogoWidth(97), 144);
        compare(Sizing.snapLogoWidth(192), 192);
        compare(Sizing.snapLogoWidth(193), 256);
        compare(Sizing.snapLogoWidth(384), 384);
        compare(Sizing.snapLogoWidth(385), 600);
        compare(Sizing.snapLogoWidth(600), 600);
        compare(Sizing.snapLogoWidth(9999), 600);
    }

    function test_detail_cover_tier_capped_by_viewport_width(): void {
        // Pin the resolution through the Main harness first: the cover-box
        // math reads the singleton's live screen size for its paddings, so
        // asserting against detached argument pairs would depend on test
        // order.
        //
        // CRT-native scene: 352x240 minus the 5% safe-area insets. The
        // doubled detail tier must not exceed what the framebuffer can
        // express -- a 512-wide decode can never be displayed at 512 on a
        // 352-wide mode and only wastes resample time and decoded-cache
        // bytes.
        main.crtNativePath = true;
        setResolutionExpect(352, 240, crtSafeWidth(352), crtSafeHeight(240));
        verify(Sizing.detailCoverSourceSize(Sizing.screenWidth, Sizing.screenHeight) <= 256, "CRT detail tier must not exceed the viewport-expressible tier");
        compare(Sizing.detailCoverSourceWidth, Sizing.detailCoverSourceSize(Sizing.detailCoverViewportWidth, Sizing.detailCoverViewportHeight), "decode width must equal the tier for its bound viewport");

        // Wider scenes keep the historical behavior: at 1080p the doubled
        // grid tier lands on the top tier, uncapped.
        main.crtNativePath = false;
        setResolution(1920, 1080);
        compare(Sizing.detailCoverSourceSize(1920, 1080), 768, "1080p detail tier must stay the top tier");

        // Restore the harness default so later tests stay order-independent.
        setResolution(1280, 720);
    }

    function test_half_size_1080p_games_grid_keeps_normal_page_density(): void {
        const shape = Sizing.gamesGridShape(960, 365);
        compare(shape.columns, 5);
        compare(shape.rows, 2);
    }

    function test_semantic_tiers_radii_fonts_and_strokes(): void {
        const cases = [
            {
                "w": 320,
                "h": 240,
                "tier": "240",
                "radiusMd": 2,
                "radiusSm": 1,
                "fonts": [14, 12, 11, 10, 9, 8],
                "strokes": [1, 1, 1, 2]
            },
            {
                "w": 640,
                "h": 480,
                "tier": "480",
                "radiusMd": 3,
                "radiusSm": 2,
                "fonts": [22, 18, 17, 16, 14, 13],
                "strokes": [1, 2, 3, 4]
            },
            {
                "w": 960,
                "h": 540,
                "tier": "540",
                "radiusMd": 4,
                "radiusSm": 2,
                "fonts": [24, 20, 18, 17, 15, 14],
                "strokes": [1, 2, 3, 4]
            },
            {
                "w": 1280,
                "h": 720,
                "tier": "720",
                "radiusMd": 6,
                "radiusSm": 3,
                "fonts": [29, 23, 21, 19, 17, 16],
                "strokes": [1, 3, 4, 6]
            },
            {
                "w": 1366,
                "h": 768,
                "tier": "720",
                "radiusMd": 6,
                "radiusSm": 3,
                "fonts": [29, 23, 21, 19, 17, 16],
                "strokes": [2, 3, 5, 6]
            },
            {
                "w": 1920,
                "h": 1080,
                "tier": "1080",
                "radiusMd": 8,
                "radiusSm": 4,
                "fonts": [43, 35, 31, 28, 26, 24],
                "strokes": [2, 4, 6, 9]
            }
        ];
        const fontNames = ["fontHero", "fontTitle", "fontSection", "fontBody", "fontCaption", "fontSmall"];
        const strokeNames = ["cardBorderWidth", "focusBorderWidth", "focusRingWidth", "pressEdgeHeight"];
        for (const entry of cases) {
            setResolution(entry.w, entry.h);
            compare(Sizing.tier, entry.tier);
            compare(Sizing.radiusMd, entry.radiusMd);
            compare(Sizing.radiusSm, entry.radiusSm);
            verify(Sizing.radiusSm <= Sizing.radiusMd);
            for (let i = 0; i < fontNames.length; ++i) {
                compare(Sizing[fontNames[i]], entry.fonts[i], fontNames[i] + " at " + entry.w + "x" + entry.h);
                if (i > 0)
                    verify(Sizing[fontNames[i - 1]] > Sizing[fontNames[i]], "font roles must strictly descend");
            }
            for (let i = 0; i < strokeNames.length; ++i)
                compare(Sizing[strokeNames[i]], entry.strokes[i], strokeNames[i] + " at " + entry.w + "x" + entry.h);
        }
    }

    // ContextMenu's corner masks (Part 5) are only baked for radii 1..16;
    // Resources.cornerCutUrl() silently returns "" outside that band, so a
    // ladder change that pushes radiusMd/radiusSm past 16 would quietly
    // stop rounding the scrim hole instead of failing loudly. Covers every
    // resolution tier, including "crt" which the semantic-tier table above
    // doesn't exercise.
    function test_radius_ladder_stays_within_baked_corner_mask_range(): void {
        const resolutions = [[320, 240], [640, 480], [960, 540], [1280, 720], [1920, 1080]];
        for (const [w, h] of resolutions) {
            setResolution(w, h);
            verify(Sizing.radiusMd >= 1 && Sizing.radiusMd <= 16, "radiusMd out of baked corner mask range at " + w + "x" + h);
            verify(Sizing.radiusSm >= 1 && Sizing.radiusSm <= 16, "radiusSm out of baked corner mask range at " + w + "x" + h);
        }

        main.crtNativePath = true;
        setResolutionExpect(352, 240, crtSafeWidth(352), crtSafeHeight(240));
        compare(Sizing.tier, "crt");
        verify(Sizing.radiusMd >= 1 && Sizing.radiusMd <= 16, "radiusMd out of baked corner mask range on crt tier");
        verify(Sizing.radiusSm >= 1 && Sizing.radiusSm <= 16, "radiusSm out of baked corner mask range on crt tier");
        main.crtNativePath = false;
    }

    function test_common_digital_grid_shapes_are_declared(): void {
        const cases = [
            {
                "w": 320,
                "h": 240,
                "systems": [2, 2],
                "games": [2, 2]
            },
            {
                "w": 640,
                "h": 480,
                "systems": [3, 3],
                "games": [4, 2]
            },
            {
                "w": 960,
                "h": 540,
                "systems": [4, 3],
                "games": [5, 2]
            },
            {
                "w": 1280,
                "h": 720,
                "systems": [4, 3],
                "games": [5, 2]
            },
            {
                "w": 1366,
                "h": 768,
                "systems": [4, 3],
                "games": [5, 2]
            },
            {
                "w": 1920,
                "h": 1080,
                "systems": [4, 3],
                "games": [5, 2]
            }
        ];
        for (const entry of cases) {
            setResolution(entry.w, entry.h);
            const systems = Sizing.systemsGridShape(entry.w, entry.h);
            const games = Sizing.gamesGridShape(entry.w, Math.max(1, entry.h * 0.68));
            compare(systems.columns, entry.systems[0]);
            compare(systems.rows, entry.systems[1]);
            compare(games.columns, entry.games[0]);
            compare(games.rows, entry.games[1]);
        }
    }

    // Hub grid shape (round 6, item 7; row count corrected in the round 6
    // follow-up — see Sizing.qml's `hubGridShape` comment) — a fixed
    // per-tier table, unlike systemsGridShape/gamesGridShape above.
    // Verifies both the table's values AND the property that actually
    // matters for a hand-arranged Hub layout: it must NOT move with
    // viewport width the way the adaptive browse-grid shapes do.
    function test_hub_grid_shape_is_fixed_per_tier_not_viewport(): void {
        const cases = [
            {
                "w": 320,
                "h": 240,
                "columns": 3,
                "rows": 2
            },
            {
                "w": 640,
                "h": 480,
                "columns": 4,
                "rows": 2
            },
            {
                "w": 960,
                "h": 540,
                "columns": 7,
                "rows": 3
            },
            {
                "w": 1280,
                "h": 720,
                "columns": 7,
                "rows": 3
            },
            {
                "w": 1920,
                "h": 1080,
                "columns": 7,
                "rows": 3
            }
        ];
        for (const entry of cases) {
            setResolution(entry.w, entry.h);
            compare(Sizing.hubGridColumns, entry.columns, entry.w + "x" + entry.h);
            compare(Sizing.hubGridRows, entry.rows, entry.w + "x" + entry.h);
        }

        // Same tier ("720"), two very different widths -- columns must not
        // move. systemsGridColumns/gamesGridColumns are explicitly allowed
        // to differ here; hubGridColumns must not, by design.
        setResolution(1280, 720);
        const narrowColumns = Sizing.hubGridColumns;
        setResolution(3000, 720);
        compare(Sizing.hubGridColumns, narrowColumns, "hub grid shape must not fit to viewport width");
        compare(Sizing.tier, "720");
    }

    function test_nonstandard_scene_uses_adaptive_grid_scorer(): void {
        setResolution(1000, 600);
        const shape = Sizing.gamesGridShape(1000, 405);
        const scored = Sizing._selectGridShape(1000, 405, Sizing._gamesGridConfig);
        compare(shape.columns, scored.columns);
        compare(shape.rows, scored.rows);
    }

    function test_crt_fonts_and_declared_grid_shapes(): void {
        // Real --crt launches always carry both flags (main.cpp's
        // bitmapTypeEnabled formula is crtNativePathEnabled || ...), so the
        // harness sets them together to mirror production wiring.
        main.crtNativePath = true;
        main.bitmapType = true;
        setResolutionExpect(352, 240, crtSafeWidth(352), crtSafeHeight(240));
        compare(Sizing.tier, "crt");
        compare(Sizing.fontHero, Sizing.fontSize(4.0));
        compare(Sizing.fontTitle, Sizing.fontSize(3.2));
        compare(Sizing.fontSection, Sizing.fontSize(2.9));
        compare(Sizing.fontBody, Sizing.fontSize(2.6));
        compare(Sizing.fontCaption, Sizing.fontSize(2.4));
        compare(Sizing.fontSmall, Sizing.fontSize(2.2));
        compare(Sizing.systemsGridColumns, 3);
        compare(Sizing.systemsGridRows, 3);
        compare(Sizing.hubGridColumns, 3);
        compare(Sizing.hubGridRows, 2);
        compare(Sizing.swapPercentageAxes, false);
        compare(Sizing.screenHeight, crtSafeHeight(240));
        const declaredGames = Sizing._declaredGridShape("games");
        compare(declaredGames.columns, 2);
        let games = Sizing.gamesGridShape(Sizing.screenWidth, Sizing.screenHeight);
        compare(games.columns, 2);
        compare(games.rows, 2);

        setResolutionExpect(352, 288, crtSafeWidth(352), crtSafeHeight(288));
        compare(Sizing.systemsGridColumns, 3);
        compare(Sizing.systemsGridRows, 3);
        games = Sizing.gamesGridShape(Sizing.screenWidth, Sizing.screenHeight);
        compare(games.columns, 3);
        compare(games.rows, 2);
    }
}
