// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

import QtQuick
import QtTest
import Zaparoo.Theme

// Resources.coverUrl is the single source of truth for turning a model
// cover key into an image:// URL. These tests lock the routing contract,
// especially that user `custom-image/` overrides bypass the tint pipeline
// and are served exactly as-is.
TestCase {
    name: "UiResources"

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
        const cat = String(Resources.coverUrl("categories/Arcade", "#ffffff", "#888888", "#000000"));
        verify(cat.startsWith("image://tinted-svg/"));
        const sys = String(Resources.coverUrl("systems/SNES", "#ffffff", "#888888", "#000000"));
        verify(sys.startsWith("image://tinted-svg/"));
    }

    function test_empty_key_returns_empty(): void {
        compare(String(Resources.coverUrl("", "#ffffff", "#888888", "#000000")), "");
    }
}
