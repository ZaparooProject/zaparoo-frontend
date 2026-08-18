// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

import QtQuick
import QtTest
import Zaparoo.Browse as Browse
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
        id: statusPillComponent

        CoreStatusPill {}
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
        Resources.systemLogoStyle = "tinted";
        const url = String(Resources.coverUrl("systems/Apogee", "#ffffff", "#888888", "#000000"));
        verify(url.startsWith("image://tinted-svg/"), "missing system artwork must still be attempted");

        const host = createTemporaryObject(missingSystemTile, testCase);
        verify(host !== null);
        const fallback = findChild(host, "tileFallbackText");
        verify(fallback !== null);
        compare(fallback.text, "Apogee");
        tryCompare(fallback, "opacity", 1.0, 500);
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

    function test_status_pill_uses_available_header_width_cap(): void {
        const pill = createTemporaryObject(statusPillComponent, testCase, {
            "maximumWidth": 180
        });
        verify(pill !== null);
        compare(pill._boundedWidth(250), 180);
        pill.maximumWidth = 500;
        compare(pill._boundedWidth(250), 250);
        pill.maximumWidth = 0;
        compare(pill._boundedWidth(250), 250);
    }

    function test_status_pill_reserves_readable_width_data(): list<var> {
        return [
            {
                "tag": "crt-240p",
                "width": 352,
                "height": 240,
                "crt": true
            },
            {
                "tag": "480p",
                "width": 640,
                "height": 480,
                "crt": false
            },
            {
                "tag": "540p",
                "width": 960,
                "height": 540,
                "crt": false
            },
            {
                "tag": "720p",
                "width": 1280,
                "height": 720,
                "crt": false
            },
            {
                "tag": "1080p",
                "width": 1920,
                "height": 1080,
                "crt": false
            }
        ];
    }

    function test_status_pill_reserves_readable_width(data: var): void {
        const originalWidth = Sizing.screenWidth;
        const originalHeight = Sizing.screenHeight;
        const originalSizingCrt = Sizing.crtNativePath;
        const originalThemeCrt = Theme.crtNativePath;
        const originalLinkState = Browse.AppStatus.link_state;
        const originalConnectionState = Browse.AppStatus.connection_state;
        try {
            Sizing.screenWidth = data.width;
            Sizing.screenHeight = data.height;
            Sizing.crtNativePath = data.crt;
            Theme.crtNativePath = data.crt;
            Browse.AppStatus.link_state = 1;
            Browse.AppStatus.connection_state = 1;

            const pill = createTemporaryObject(statusPillComponent, testCase, {
                "maximumWidth": data.width
            });
            verify(pill !== null);
            compare(pill._connectionLabel, "Connecting…");
            compare(pill._isMediaActivity, false);
            compare(pill._desiredWidth, Math.max(pill._minimumWidth, pill._naturalWidth));
            verify(pill._desiredWidth >= pill._minimumWidth, "connection and media states need the same stable status-bar footprint: desired=" + pill._desiredWidth + " minimum=" + pill._minimumWidth);
            verify(pill._naturalWidth - 2 * pill._textMargin - pill._spinnerReservedWidth >= pill._labelNaturalWidth + pill._textMeasureSlack, "content width must include measured glyphs and native-rendering slack");
        } finally {
            Sizing.screenWidth = originalWidth;
            Sizing.screenHeight = originalHeight;
            Sizing.crtNativePath = originalSizingCrt;
            Theme.crtNativePath = originalThemeCrt;
            Browse.AppStatus.link_state = originalLinkState;
            Browse.AppStatus.connection_state = originalConnectionState;
        }
    }

    function test_header_status_slot_fits_minimum_at_540p(): void {
        const originalWidth = Sizing.screenWidth;
        const originalHeight = Sizing.screenHeight;
        const originalSizingCrt = Sizing.crtNativePath;
        const originalThemeCrt = Theme.crtNativePath;
        try {
            Sizing.screenWidth = 960;
            Sizing.screenHeight = 540;
            Sizing.crtNativePath = false;
            Theme.crtNativePath = false;

            const header = createTemporaryObject(headerBarComponent, testCase, {
                "width": 960
            });
            verify(header !== null);
            const pill = findChild(header, "coreStatusPill");
            verify(pill !== null);
            tryVerify(function () {
                return pill.maximumWidth > 0;
            });
            verify(pill.maximumWidth >= pill._minimumWidth, "header status slot must not cap the responsive minimum");
        } finally {
            Sizing.screenWidth = originalWidth;
            Sizing.screenHeight = originalHeight;
            Sizing.crtNativePath = originalSizingCrt;
            Theme.crtNativePath = originalThemeCrt;
        }
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

    function test_tile_caption_strengthens_only_for_progressive_unsmoothed_text(): void {
        const originalHeight = Sizing.screenHeight;
        const originalSizingCrt = Sizing.crtNativePath;
        const originalThemeCrt = Theme.crtNativePath;
        const originalUnsmoothed = Theme.unsmoothedText;
        try {
            Sizing.screenHeight = 540;
            Sizing.crtNativePath = false;
            Theme.crtNativePath = false;
            Theme.unsmoothedText = true;

            const host = createTemporaryObject(missingSystemTile, testCase);
            verify(host !== null);
            const caption = findChild(host, "tileCaption");
            verify(caption !== null);
            compare(caption.fontPixelSize, Sizing.fontSize(2.4));
            compare(caption.fontWeight, Font.Medium);

            Theme.unsmoothedText = false;
            compare(caption.fontPixelSize, Sizing.fontSize(2.2));
            compare(caption.fontWeight, Font.Normal);
        } finally {
            Sizing.screenHeight = originalHeight;
            Sizing.crtNativePath = originalSizingCrt;
            Theme.crtNativePath = originalThemeCrt;
            Theme.unsmoothedText = originalUnsmoothed;
        }
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
}
