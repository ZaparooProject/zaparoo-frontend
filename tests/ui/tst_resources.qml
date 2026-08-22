// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

import QtQuick
import QtTest
import Zaparoo.Theme
import Zaparoo.Ui

// Resources.coverUrl is the single source of truth for turning a model
// cover key into an image:// URL. These tests lock the routing contract,
// especially that user `custom-image/` overrides bypass the tint pipeline
// and are served exactly as-is.
TestCase {
    id: testCase

    name: "UiResources"

    Component {
        id: headerBarComponent

        HeaderBar {
            width: 960
        }
    }

    Component {
        id: scrollingCaptionComponent

        ScrollingCaption {
            width: 120
            height: 24
            name: "jjjj WAVE"
            tags: "Rev A"
        }
    }

    Component {
        id: missingSystemTile

        Item {
            width: 240
            height: 160
            property bool isSelected: false
            property bool isFocused: false
            property string name: "Apogee"
            property string coverKey: "systems/Apogee"
            property string topLabel: ""
            property int favorite: 0
            property bool hidden: false
            property string disambiguatingTags: ""

            Tile {
                anchors.fill: parent
            }
        }
    }

    function test_custom_image_key_routes_to_custom_provider(): void {
        const url = String(Resources.coverUrl("custom-image//media/fat/zaparoo/custom/systems/SNES.png", "#111111", "#222222", "#333333"));
        compare(url, "image://custom-image//media/fat/zaparoo/custom/systems/SNES.png");
    }

    function test_custom_image_ignores_tint_params(): void {
        // Different tint tokens must produce an identical URL — overrides are
        // never recolored, so the focused and unfocused ramps collapse to one
        // fetch (Main.qml's prefetch relies on this).
        const a = String(Resources.coverUrl("custom-image/foo.png", "#111111", "#222222", "#333333"));
        const b = String(Resources.coverUrl("custom-image/foo.png", "#aaaaaa", "#bbbbbb", "#cccccc"));
        compare(a, b);
        compare(a, "image://custom-image/foo.png");
    }

    function test_bundled_keys_still_route_through_tinted_svg(): void {
        // Contrast case: bundled category/system/icon keys DO go through the
        // tint provider so their color tracks the theme.
        Resources.systemLogoStyle = "tinted";
        const cat = String(Resources.coverUrl("categories/Arcade", "#ffffff", "#888888", "#000000"));
        verify(cat.startsWith("image://tinted-svg/"));
        const sys = String(Resources.coverUrl("systems/SNES", "#ffffff", "#888888", "#000000"));
        verify(sys.startsWith("image://tinted-svg/"));
    }

    function test_color_system_logo_style_uses_png_when_available(): void {
        Resources.systemLogoStyle = "color";
        const sys = String(Resources.coverUrl("systems/SNES", "#ffffff", "#888888", "#000000"));
        compare(sys, "qrc:/qt/qml/Zaparoo/App/resources/images/systems-color/SNES.png");
        const regional = String(Resources.coverUrl("systems/TurboGrafx16CD.jp", "#ffffff", "#888888", "#000000"));
        compare(regional, "qrc:/qt/qml/Zaparoo/App/resources/images/systems-color/TurboGrafx16CD.jp.png");
    }

    function test_color_system_logo_style_falls_back_to_tinted_svg(): void {
        Resources.systemLogoStyle = "color";
        const sys = String(Resources.coverUrl("systems/AliceMC10", "#ffffff", "#888888", "#000000"));
        verify(sys.startsWith("image://tinted-svg/"));
    }

    function test_missing_system_logo_attempts_load_then_shows_text_on_error(): void {
        const originalStyle = Resources.systemLogoStyle;
        try {
            Resources.systemLogoStyle = "tinted";
            const url = String(Resources.coverUrl("systems/Apogee", "#ffffff", "#888888", "#000000"));
            verify(url.startsWith("image://tinted-svg/"), "missing system artwork must still be attempted");

            const host = createTemporaryObject(missingSystemTile, testCase);
            verify(host !== null);
            const fallback = findChild(host, "tileFallbackText");
            verify(fallback !== null);
            compare(fallback.text, "Apogee");
            tryCompare(fallback, "opacity", 1.0, 500);
        } finally {
            Resources.systemLogoStyle = originalStyle;
        }
    }

    function test_non_system_image_error_never_shows_text_fallback(): void {
        const host = createTemporaryObject(missingSystemTile, testCase, {
            "coverKey": "categories/__missing_category__",
            "name": "Missing category"
        });
        verify(host !== null);
        const fallback = findChild(host, "tileFallbackText");
        verify(fallback !== null);
        wait(500);
        compare(fallback.opacity, 0.0);
    }

    function test_header_hides_obviously_invalid_clock_dates(): void {
        const header = createTemporaryObject(headerBarComponent, testCase);
        verify(header !== null);
        compare(header._clockDateValid(new Date(1970, 0, 1)), false);
        compare(header._clockDateValid(new Date(2019, 11, 31)), false);
        compare(header._clockDateValid(new Date(2020, 0, 1)), true);
    }

    function test_scrolling_caption_measures_painted_glyph_bounds(): void {
        const caption = createTemporaryObject(scrollingCaptionComponent, testCase);
        verify(caption !== null);
        const nameMetrics = findChild(caption, "scrollingCaptionNameMetrics");
        const tagsMetrics = findChild(caption, "scrollingCaptionTagsMetrics");
        verify(nameMetrics !== null);
        verify(tagsMetrics !== null);
        const expectedNameWidth = Math.ceil(Math.max(nameMetrics.advanceWidth, nameMetrics.boundingRect.x + nameMetrics.boundingRect.width) - Math.min(0, nameMetrics.boundingRect.x));
        const expectedTagsWidth = Math.ceil(Math.max(tagsMetrics.advanceWidth, tagsMetrics.boundingRect.x + tagsMetrics.boundingRect.width) - Math.min(0, tagsMetrics.boundingRect.x));
        compare(caption._nameFullW, expectedNameWidth);
        compare(caption._tagsFullW, expectedTagsWidth);
    }

    function test_media_cover_uses_short_reveal_without_loading_glyph(): void {
        const host = createTemporaryObject(missingSystemTile, testCase, {
            "coverKey": "media-image/example"
        });
        verify(host !== null);
        const reveal = findChild(host, "tileCoverRevealAnimation");
        verify(reveal !== null);
        compare(reveal.duration, Motion.dur(Motion.pressMs));
        compare(findChild(host, "tileLoadingGlyph"), null);
    }

    function test_tile_top_label_reserves_space_above_cover(): void {
        const host = createTemporaryObject(missingSystemTile, testCase, {
            "coverKey": "media-image/example",
            "topLabel": "Super Nintendo"
        });
        verify(host !== null);
        const label = findChild(host, "tileTopLabel");
        const cover = findChild(host, "tileCoverBase");
        verify(label !== null);
        verify(cover !== null);
        compare(label.text, "Super Nintendo");
        verify(cover.y >= label.y + label.height, "cover must start below system label");
    }

    function test_tile_without_top_label_keeps_cover_at_default_inset(): void {
        const plainHost = createTemporaryObject(missingSystemTile, testCase);
        const labelledHost = createTemporaryObject(missingSystemTile, testCase, {
            "topLabel": "Super Nintendo"
        });
        verify(plainHost !== null);
        verify(labelledHost !== null);
        const plainLabel = findChild(plainHost, "tileTopLabel");
        const plainCover = findChild(plainHost, "tileCoverBase");
        const labelledCover = findChild(labelledHost, "tileCoverBase");
        compare(plainLabel.text, "");
        verify(plainCover.y < labelledCover.y, "empty labels must not reserve top space");
    }

    function test_system_artwork_alias_is_checked_after_remap(): void {
        Resources.systemLogoStyle = "tinted";
        const sys = String(Resources.coverUrl("systems/MacPlus", "#ffffff", "#888888", "#000000"));
        verify(sys.endsWith("/images/systems/MacOS.svg"));
    }

    function test_empty_key_returns_empty(): void {
        compare(String(Resources.coverUrl("", "#ffffff", "#888888", "#000000")), "");
    }

    // logoUrl is item 9d's shared rung/variant derivation for HeaderBar and
    // AboutScreen, both of which sit on the theme's own surface. `data.width`
    // is a painted width; `data.rung` is the pre-sized ladder rung it must
    // snap up to (Sizing.snapLogoWidth), and `data.variant` is the light/dark
    // wordmark picked from Theme.lightSurface (itself derived from
    // Theme.colorSchemeId).
    function test_logo_url_snaps_rung_and_picks_variant_data(): list<var> {
        return [
            {
                tag: "exact rung 96",
                width: 96,
                scheme: "zaparoo-dark",
                rung: 96,
                variant: "on-dark"
            },
            {
                tag: "just over 96 snaps to 144",
                width: 97,
                scheme: "zaparoo-dark",
                rung: 144,
                variant: "on-dark"
            },
            {
                tag: "exact rung 144",
                width: 144,
                scheme: "zaparoo-dark",
                rung: 144,
                variant: "on-dark"
            },
            {
                tag: "just over 144 snaps to 192",
                width: 145,
                scheme: "zaparoo-dark",
                rung: 192,
                variant: "on-dark"
            },
            {
                tag: "exact rung 256",
                width: 256,
                scheme: "zaparoo-dark",
                rung: 256,
                variant: "on-dark"
            },
            {
                tag: "exact rung 384",
                width: 384,
                scheme: "zaparoo-dark",
                rung: 384,
                variant: "on-dark"
            },
            {
                tag: "above largest sub-600 rung clamps to 600",
                width: 500,
                scheme: "zaparoo-dark",
                rung: 600,
                variant: "on-dark"
            },
            {
                tag: "far above 600 still clamps to 600",
                width: 1200,
                scheme: "zaparoo-dark",
                rung: 600,
                variant: "on-dark"
            },
            {
                tag: "light surface picks on-light variant",
                width: 144,
                scheme: "zaparoo-light",
                rung: 144,
                variant: "on-light"
            },
            {
                tag: "dark accent preset still picks on-dark variant",
                width: 144,
                scheme: "classic-purple",
                rung: 144,
                variant: "on-dark"
            }
        ];
    }

    function test_logo_url_snaps_rung_and_picks_variant(data: var): void {
        const originalScheme = Theme.colorSchemeId;
        try {
            Theme.colorSchemeId = data.scheme;
            const url = String(Resources.logoUrl(data.width));
            compare(url, "qrc:/qt/qml/Zaparoo/App/resources/images/logo/logo-" + data.variant + "-" + data.rung + ".png");
        } finally {
            Theme.colorSchemeId = originalScheme;
        }
    }

    // Round 6 follow-up: screensaverLogoUrl() must always resolve the
    // on-dark-surface variant, regardless of the active theme — the
    // screensaver's backstop is unconditionally solid black
    // (ScreensaverOverlay.qml), so following the theme the way logoUrl()
    // does picks an "on-light" logo that disappears against it whenever a
    // light preset is active.
    function test_screensaver_logo_url_ignores_theme_and_always_picks_on_dark_data(): list<var> {
        return [
            {
                tag: "dark preset",
                scheme: "zaparoo-dark"
            },
            {
                tag: "light preset",
                scheme: "zaparoo-light"
            },
            {
                tag: "dark-accent preset",
                scheme: "classic-purple"
            }
        ];
    }

    function test_screensaver_logo_url_ignores_theme_and_always_picks_on_dark(data: var): void {
        const originalScheme = Theme.colorSchemeId;
        try {
            Theme.colorSchemeId = data.scheme;
            const url = String(Resources.screensaverLogoUrl(144));
            compare(url, "qrc:/qt/qml/Zaparoo/App/resources/images/logo/logo-on-dark-144.png");
        } finally {
            Theme.colorSchemeId = originalScheme;
        }
    }

    // isTintedProviderKey is the load-policy half of coverUrl: it answers "is
    // this a sub-millisecond LUT pass over a mapped mask, or a real image
    // decode?". Tile.qml only permits a synchronous, GUI-thread decode for the
    // former, so a wrong answer here is either pop-in (false negative) or a
    // multi-millisecond stall per tile (false positive).
    function test_is_tinted_provider_key_data(): list<var> {
        return [
            {
                tag: "system logo",
                style: "tinted",
                key: "systems/SNES",
                expected: true
            },
            {
                tag: "hub category",
                style: "tinted",
                key: "categories/Arcade",
                expected: true
            },
            {
                tag: "ui glyph",
                style: "tinted",
                key: "icons/Settings",
                expected: true
            },
            {
                tag: "color style system png",
                style: "color",
                key: "systems/SNES",
                expected: false
            },
            {
                tag: "color style aliased system png",
                style: "color",
                key: "systems/SVI328",
                expected: false
            },
            {
                tag: "color style without a png",
                style: "color",
                key: "systems/AliceMC10",
                expected: true
            },
            {
                tag: "color style leaves glyphs tinted",
                style: "color",
                key: "icons/Settings",
                expected: true
            },
            {
                tag: "user override",
                style: "tinted",
                key: "custom-image/foo.png",
                expected: false
            },
            {
                tag: "core media art",
                style: "tinted",
                key: "media-image/SNES/game",
                expected: false
            },
            {
                tag: "empty",
                style: "tinted",
                key: "",
                expected: false
            }
        ];
    }

    function test_is_tinted_provider_key(data: var): void {
        const originalStyle = Resources.systemLogoStyle;
        try {
            Resources.systemLogoStyle = data.style;
            compare(Resources.isTintedProviderKey(data.key), data.expected);
            // The predicate is only useful if it agrees with the URL the same
            // key actually produces, so check both halves against each other.
            const url = String(Resources.coverUrl(data.key, "#ffffff", "#888888", "#000000"));
            compare(url.startsWith("image://tinted-svg/"), data.expected, "predicate disagrees with coverUrl");
        } finally {
            Resources.systemLogoStyle = originalStyle;
        }
    }
}
