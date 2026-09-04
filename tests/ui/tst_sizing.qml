// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

import QtQuick
import QtTest
import Zaparoo.Theme
import Zaparoo.App
import Zaparoo.Browse as Browse

// Resolution-agnostic sizing contract: pctH/pctW/fontSize must scale with the
// Main window's screenWidth/screenHeight, and visibleCovers must honour the
// 240p special-case (3 covers instead of 5).
TestCase {
    id: testCase
    name: "UiSizing"
    when: windowShown

    Main {
        id: main
        fullScreen: false
        width: 1280
        height: 720
    }

    // Standing guard on the QML-to-Rust split. `Sizing`'s derived values come
    // from `Browse.SizingRules` as NOTIFY properties, and its percentage
    // helpers stay QML JavaScript. Both kinds have to keep bindings alive: a
    // cxx-qt *invokable* registers no dependency, so a derived value exposed
    // that way would silently freeze at its startup value on the next resize
    // or rotation. Nothing else in this suite reads Sizing through a binding,
    // so nothing else would catch it.
    QtObject {
        id: bindingProbe

        // Rust-backed property.
        readonly property int rustProperty: Sizing.hubGridColumns
        // QML helper with a literal argument, the ~510-call-site case.
        readonly property int qmlHelper: Sizing.pctH(10)
        // Rust invokable reached through the facade wrapper, which reads the
        // config inputs first so the dependency is still captured.
        readonly property int rustInvokable: Sizing.gamesGridShape(Sizing.screenWidth, Sizing.screenHeight).columns
        // The hard case: constant arguments, so the only thing that can keep
        // this binding alive is the facade wrapper reading `crtNativePath`
        // before it delegates. Drop that read and this freezes while every
        // other probe here still passes.
        readonly property int constantArgs: Sizing.gamesGridShape(640, 480).columns
    }

    property string _originalOrientation: "horizontal"
    property string _originalInterfaceProfile: "device"

    Component.onCompleted: {
        testCase._originalOrientation = Browse.Settings.current_orientation;
        testCase._originalInterfaceProfile = Browse.Settings.current_interface_profile;
    }

    function init(): void {
        main.defaultInterfaceProfile = "standard";
        Browse.Settings.current_interface_profile = "device";
        Browse.Settings.current_orientation = "horizontal";
        tryCompare(Sizing, "interfaceProfile", "standard");
        tryCompare(Sizing, "swapPercentageAxes", false);
    }

    function cleanup(): void {
        main.debugCrtSafeAreaOverlay = false;
        main.crtNativePath = false;
        main.bitmapType = false;
        main.defaultInterfaceProfile = "standard";
        Browse.Settings.current_interface_profile = "device";
        Browse.Settings.current_orientation = "horizontal";
        tryCompare(Sizing, "interfaceProfile", "standard");
        tryCompare(Sizing, "swapPercentageAxes", false);
        setResolution(1280, 720);
    }

    function cleanupTestCase(): void {
        Browse.Settings.current_interface_profile = testCase._originalInterfaceProfile;
        Browse.Settings.current_orientation = testCase._originalOrientation;
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

    function test_handheld_profile_uses_compiled_default_and_user_override(): void {
        compare(main.effectiveInterfaceProfile, "standard");

        main.defaultInterfaceProfile = "handheld";
        tryCompare(main, "effectiveInterfaceProfile", "handheld");
        tryCompare(Sizing, "interfaceProfile", "handheld");

        Browse.Settings.current_interface_profile = "standard";
        tryCompare(main, "effectiveInterfaceProfile", "standard");
        tryCompare(Sizing, "interfaceProfile", "standard");

        main.defaultInterfaceProfile = "standard";
        Browse.Settings.current_interface_profile = "handheld";
        tryCompare(main, "effectiveInterfaceProfile", "handheld");
        tryCompare(Sizing, "interfaceProfile", "handheld");
    }

    function test_handheld_profile_reduces_700_square_hub_density(): void {
        setResolution(700, 700);
        compare(Sizing.hubGridColumns, 7);
        compare(Sizing.hubGridRows, 3);
        const standardTileWidth = Sizing.hubTileWidth;

        Browse.Settings.current_interface_profile = "handheld";
        tryCompare(Sizing, "interfaceProfile", "handheld");
        compare(Sizing.hubGridColumns, 4);
        compare(Sizing.hubGridRows, 3);
        verify(Sizing.hubTileWidth > standardTileWidth);

        setResolution(320, 240);
        compare(Sizing.hubGridColumns, 4);
        compare(Sizing.hubGridRows, 2);
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
    // resolution tier plus CRT safe-area sizing.
    function test_radius_ladder_stays_within_baked_corner_mask_range(): void {
        const resolutions = [[320, 240], [640, 480], [960, 540], [1280, 720], [1920, 1080]];
        for (const [w, h] of resolutions) {
            setResolution(w, h);
            verify(Sizing.radiusMd >= 1 && Sizing.radiusMd <= 16, "radiusMd out of baked corner mask range at " + w + "x" + h);
            verify(Sizing.radiusSm >= 1 && Sizing.radiusSm <= 16, "radiusSm out of baked corner mask range at " + w + "x" + h);
        }

        main.crtNativePath = true;
        setResolutionExpect(352, 240, crtSafeWidth(352), crtSafeHeight(240));
        compare(Sizing.tier, "240");
        verify(Sizing.radiusMd >= 1 && Sizing.radiusMd <= 16, "radiusMd out of baked corner mask range on 240p CRT");
        verify(Sizing.radiusSm >= 1 && Sizing.radiusSm <= 16, "radiusSm out of baked corner mask range on 240p CRT");
        main.crtNativePath = false;
    }

    function test_common_digital_grid_shapes_are_declared(): void {
        const cases = [
            {
                "w": 320,
                "h": 240,
                "systems": [2, 2],
                "games": [3, 2]
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

    function test_rotated_common_grids_transpose_rows_and_columns(): void {
        const cases = [[320, 240], [640, 480], [960, 540], [1280, 720], [1366, 768], [1920, 1080]];
        for (const [width, height] of cases) {
            Browse.Settings.current_orientation = "horizontal";
            tryCompare(Sizing, "swapPercentageAxes", false);
            setResolution(width, height);
            const hub = [Sizing.hubGridColumns, Sizing.hubGridRows];
            const games = Sizing.gamesGridShape(Sizing.screenWidth, Sizing.screenHeight);

            for (const orientation of ["cw", "ccw"]) {
                Browse.Settings.current_orientation = orientation;
                tryCompare(Sizing, "swapPercentageAxes", true);
                tryCompare(Sizing, "screenWidth", height);
                tryCompare(Sizing, "screenHeight", width);
                compare(Sizing.hubGridColumns, hub[1], orientation + " Hub columns at " + width + "x" + height);
                compare(Sizing.hubGridRows, hub[0], orientation + " Hub rows at " + width + "x" + height);
                const rotatedSystems = Sizing.systemsGridShape(Sizing.screenWidth, Sizing.screenHeight);
                compare(rotatedSystems.columns, 2, orientation + " Systems columns at " + width + "x" + height);
                compare(rotatedSystems.rows, 3, orientation + " Systems rows at " + width + "x" + height);
                const rotatedGames = Sizing.gamesGridShape(Sizing.screenWidth, Sizing.screenHeight);
                compare(rotatedGames.columns, games.rows, orientation + " Games columns at " + width + "x" + height);
                compare(rotatedGames.rows, games.columns, orientation + " Games rows at " + width + "x" + height);

                Browse.Settings.current_orientation = "horizontal";
                tryCompare(Sizing, "swapPercentageAxes", false);
            }
        }
    }

    function test_rotated_crt_grids_transpose_rows_and_columns(): void {
        main.crtNativePath = true;
        main.bitmapType = true;
        setResolutionExpect(352, 240, crtSafeWidth(352), crtSafeHeight(240));

        for (const orientation of ["cw", "ccw"]) {
            Browse.Settings.current_orientation = orientation;
            tryCompare(Sizing, "swapPercentageAxes", true);
            tryCompare(Sizing, "screenWidth", crtSafeHeight(240));
            tryCompare(Sizing, "screenHeight", crtSafeWidth(352));
            compare(Sizing.hubGridColumns, 2);
            compare(Sizing.hubGridRows, 4);
            compare(Sizing.systemsGridColumns, 2);
            compare(Sizing.systemsGridRows, 3);
            compare(Sizing.gamesGridColumns, 2);
            compare(Sizing.gamesGridRows, 3);

            Browse.Settings.current_orientation = "horizontal";
            tryCompare(Sizing, "swapPercentageAxes", false);
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
                "columns": 4,
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

    // Sizing.hubTileSize duplicates HubScreen's own squareCells fit
    // (Sizing.qml's own doc comment) so Settings can read the Hub's
    // resolved tile size without a HubScreen instance. Cross-check it
    // against the real thing at a few tiers so the two formulas can't
    // silently drift apart, plus a hand-computed check at one tier for an
    // independent sanity bound.
    function test_hub_tile_size_matches_real_grid_and_hand_math(): void {
        const cases = [[320, 240], [640, 480], [1280, 720], [1920, 1080]];
        for (const [w, h] of cases) {
            setResolution(w, h);
            compare(Sizing.hubTileSize, main.hubScreen._gridCellWidth, w + "x" + h);
        }

        setResolution(1920, 1080);
        const heightBudget = Sizing.screenHeight - Sizing.headerBottom - Sizing.pctH(6) - Sizing.pctH(7) - 3 * Sizing.pctH(2);
        const widthFit = Math.floor((Sizing.screenWidth - 2 * Sizing.pctW(3) - (Sizing.hubGridColumns - 1) * Sizing.pctW(2)) / Sizing.hubGridColumns);
        const heightFit = Math.floor((heightBudget - 2 * Sizing.pctH(2) - (Sizing.hubGridRows - 1) * Sizing.pctH(4)) / Sizing.hubGridRows);
        compare(Sizing.hubTileSize, Math.min(widthFit, heightFit));
    }

    // A scene that is not one of the common framebuffer sizes has no declared
    // page geometry, so the adaptive scorer resolves it. Previously this
    // compared gamesGridShape against Sizing._selectGridShape called directly;
    // both now come from zaparoo_app::sizing, which would make that comparison
    // tautological. Assert the distinguishing fact instead (no declared shape)
    // and pin the scored result, whose value comes from
    // tests/fixtures/sizing_golden.txt.
    function test_bindings_track_resize_and_rotation(): void {
        setResolution(1920, 1080);
        compare(bindingProbe.rustProperty, 7);
        compare(bindingProbe.qmlHelper, 108);
        compare(bindingProbe.rustInvokable, 5);

        setResolution(320, 240);
        compare(bindingProbe.rustProperty, 4, "Rust-backed property must follow the resize");
        compare(bindingProbe.qmlHelper, 24, "QML helper must follow the resize");
        compare(bindingProbe.rustInvokable, 3, "facade invokable must follow the resize");

        // Rotation moves the grid shapes without moving the resolution, so it
        // exercises a different input than the resize above.
        setResolution(1920, 1080);
        Browse.Settings.current_orientation = "cw";
        tryCompare(Sizing, "swapPercentageAxes", true);
        compare(bindingProbe.rustProperty, 3, "Rust-backed property must follow the rotation");
        compare(bindingProbe.rustInvokable, 2, "facade invokable must follow the rotation");

        Browse.Settings.current_orientation = "horizontal";
        tryCompare(Sizing, "swapPercentageAxes", false);
        compare(bindingProbe.rustProperty, 7);
        compare(bindingProbe.rustInvokable, 5);

        // Constant arguments: the declared page geometry still depends on the
        // rendering path, which only the facade wrapper's read can report.
        compare(bindingProbe.constantArgs, 5);
        main.crtNativePath = true;
        tryCompare(Sizing, "crtNativePath", true);
        compare(bindingProbe.constantArgs, 3, "constant-argument binding must follow the rendering path");
        main.crtNativePath = false;
        tryCompare(Sizing, "crtNativePath", false);
        compare(bindingProbe.constantArgs, 5);
    }

    function test_nonstandard_scene_uses_adaptive_grid_scorer(): void {
        setResolution(1000, 600);
        compare(Sizing._declaredGridShape("games"), null, "1000x600 must not be a declared common scene");
        const shape = Sizing.gamesGridShape(1000, 405);
        compare(shape.columns, 5);
        compare(shape.rows, 2);

        // A common scene at the same tier does have one, so the null above is
        // the scene's doing and not a broken lookup.
        setResolution(960, 540);
        const declared = Sizing._declaredGridShape("games");
        verify(declared !== null, "960x540 is a declared common scene");
        compare(declared.columns, 5);
        compare(declared.rows, 2);
    }

    function test_crt_fonts_and_declared_grid_shapes(): void {
        // Real --crt launches always carry both flags (main.cpp's
        // bitmapTypeEnabled formula is crtNativePathEnabled || ...), so the
        // harness sets them together to mirror production wiring.
        main.crtNativePath = true;
        main.bitmapType = true;
        setResolutionExpect(352, 240, crtSafeWidth(352), crtSafeHeight(240));
        compare(Sizing.tier, "240");
        compare(Sizing.fontHero, Sizing.fontSize(4.0));
        compare(Sizing.fontTitle, Sizing.fontSize(3.2));
        compare(Sizing.fontSection, Sizing.fontSize(2.9));
        compare(Sizing.fontBody, Sizing.fontSize(2.6));
        compare(Sizing.fontCaption, Sizing.fontSize(2.4));
        compare(Sizing.fontSmall, Sizing.fontSize(2.2));
        compare(Sizing.systemsGridColumns, 3);
        compare(Sizing.systemsGridRows, 3);
        compare(Sizing.hubGridColumns, 4);
        compare(Sizing.hubGridRows, 2);
        compare(Sizing.swapPercentageAxes, false);
        compare(Sizing.screenHeight, crtSafeHeight(240));
        const declaredGames = Sizing._declaredGridShape("games");
        compare(declaredGames.columns, 3);
        let games = Sizing.gamesGridShape(Sizing.screenWidth, Sizing.screenHeight);
        compare(games.columns, 3);
        compare(games.rows, 2);

        setResolutionExpect(352, 288, crtSafeWidth(352), crtSafeHeight(288));
        compare(Sizing.tier, "240");
        compare(Sizing.hubGridColumns, 4);
        compare(Sizing.hubGridRows, 2);
        compare(Sizing.systemsGridColumns, 3);
        compare(Sizing.systemsGridRows, 3);
        games = Sizing.gamesGridShape(Sizing.screenWidth, Sizing.screenHeight);
        compare(games.columns, 3);
        compare(games.rows, 2);

        // CRT is a rendering path, not a geometry tier. Safe-area reduction
        // must not demote a future 540-line mode into the 480 tier.
        setResolutionExpect(960, 540, crtSafeWidth(960), crtSafeHeight(540));
        compare(Sizing.resolutionHeight, 540);
        compare(Sizing.tier, "540");
        compare(Sizing.hubGridColumns, 7);
        compare(Sizing.hubGridRows, 3);
    }
}
