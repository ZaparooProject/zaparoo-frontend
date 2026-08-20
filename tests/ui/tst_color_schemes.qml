// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

import QtQuick
import QtTest
import Zaparoo.Theme

TestCase {
    name: "ColorSchemes"
    when: windowShown

    readonly property var requiredRoles: ["bgDeep", "bgPanel", "bgBar", "surfaceCard", "tileEdge", "controlEdge", "scrim", "borderSubtle", "borderMid", "textPrimary", "textLabel", "textVariant", "accent", "onAccent", "onAccentMuted", "logoPrimary", "logoSecondary", "logoShadow", "logoFocusPrimary", "logoFocusSecondary", "logoFocusShadow", "marker", "markerOutline", "errorHex"]

    function _linearChannel(value: real): real {
        return value <= 0.04045 ? value / 12.92 : Math.pow((value + 0.055) / 1.055, 2.4);
    }

    function _relativeLuminance(value: color): real {
        return 0.2126 * _linearChannel(value.r) + 0.7152 * _linearChannel(value.g) + 0.0722 * _linearChannel(value.b);
    }

    function _contrastRatio(first: color, second: color): real {
        const firstLuminance = _relativeLuminance(first);
        const secondLuminance = _relativeLuminance(second);
        return (Math.max(firstLuminance, secondLuminance) + 0.05) / (Math.min(firstLuminance, secondLuminance) + 0.05);
    }

    function _saturation(value: color): real {
        const maximum = Math.max(value.r, value.g, value.b);
        const minimum = Math.min(value.r, value.g, value.b);
        return maximum === 0 ? 0 : (maximum - minimum) / maximum;
    }

    function cleanup(): void {
        Theme.colorSchemeId = ColorSchemes.defaultId;
    }

    function test_catalog_ids_and_fallback(): void {
        compare(ColorSchemes.defaultId, "zaparoo-black");
        compare(ColorSchemes.ids.length, 24);
        verify(ColorSchemes.isKnown("zaparoo-black"));
        verify(ColorSchemes.isKnown("midnight-amber"));
        verify(ColorSchemes.isKnown("zaparoo-white"));
        verify(ColorSchemes.isKnown("catppuccin-mocha"));
        compare(ColorSchemes.effectiveId("missing"), ColorSchemes.defaultId);
    }

    // Asset-selection flag for HeaderBar's logo PNG ladder (item 3b) — not a
    // color role, so it is deliberately absent from requiredRoles. Must
    // agree with each preset's own primary/text luma ordering, independent
    // of palette()'s internal `up` flag.
    function test_is_light_surface_matches_preset_luma(): void {
        verify(!ColorSchemes.isLightSurface("zaparoo-black"));
        verify(!ColorSchemes.isLightSurface("midnight-amber"));
        verify(ColorSchemes.isLightSurface("zaparoo-white"));
        compare(ColorSchemes.isLightSurface("missing"), ColorSchemes.isLightSurface(ColorSchemes.defaultId));
    }

    // The whole point of the catalog is that a preset is three colors. If a
    // fourth ever creeps back in, every derived role stops being predictable
    // from the preset and user-authored themes become impossible to specify.
    function test_presets_author_exactly_three_colors(): void {
        for (let i = 0; i < ColorSchemes.ids.length; i++) {
            const id = ColorSchemes.ids[i];
            const source = ColorSchemes._sources[id];
            verify(source !== undefined, id + " missing source");
            const keys = Object.keys(source).sort();
            compare(keys, ["accent", "primary", "text"], id + " must author exactly primary/accent/text");
        }
    }

    function test_every_preset_defines_complete_opaque_roles(): void {
        for (let i = 0; i < ColorSchemes.ids.length; i++) {
            const palette = ColorSchemes.palette(ColorSchemes.ids[i]);
            for (let role = 0; role < requiredRoles.length; role++) {
                const name = requiredRoles[role];
                verify(palette[name] !== undefined, ColorSchemes.ids[i] + " missing " + name);
                if (name === "scrim")
                    continue;
                // Derived roles are colors, not hex strings; errorHex stays a
                // string because Theme.errorHex and UpdateTheme forward it as one.
                if (name === "errorHex")
                    verify(/^#[0-9a-fA-F]{6}$/.test(palette[name]), ColorSchemes.ids[i] + " errorHex must be an opaque hex string");
                else
                    compare(Qt.color(palette[name]).a, 1, ColorSchemes.ids[i] + " " + name + " must be opaque");
            }
        }
    }

    // `textPrimary`/`bgDeep` keeps the AAA 7.0:1 floor -- the primary
    // background is the highest-traffic surface. `textPrimary`/`surfaceCard`
    // relaxes to AA 4.5:1 (round 5): several popular, real palettes
    // (Dracula, Tokyo Night, Catppuccin Frappé, Everforest Dark, Gruvbox
    // Dark, Synthwave '84) sit in the 4.5-7.0 band on their own card surface
    // by design and would otherwise require altering their authored hex to
    // ship. See docs/style.md -> "Preset catalog".
    function test_presets_keep_content_and_focus_legible(): void {
        for (let i = 0; i < ColorSchemes.ids.length; i++) {
            const id = ColorSchemes.ids[i];
            const palette = ColorSchemes.palette(id);
            verify(_contrastRatio(palette.textPrimary, palette.bgDeep) >= 7.0, id + " primary text/background");
            verify(_contrastRatio(palette.textPrimary, palette.surfaceCard) >= 4.5, id + " primary text/card");
            verify(_contrastRatio(palette.accent, palette.bgDeep) >= 3.0, id + " accent/background");
            verify(_contrastRatio(palette.textLabel, palette.bgDeep) >= 3.0, id + " label/background");
        }
    }

    // Part of what reads as a "shiny" 3D surface is that the pressable front
    // edge is chromatically separated from the face along the accent ramp, not
    // merely a darker version of it. A derivation that flattens the edge into
    // the card loses the gloss cue even though nothing looks obviously broken.
    function test_edge_reads_as_a_lit_bevel(): void {
        for (let i = 0; i < ColorSchemes.ids.length; i++) {
            const id = ColorSchemes.ids[i];
            const palette = ColorSchemes.palette(id);
            verify(_contrastRatio(palette.tileEdge, palette.surfaceCard) >= 1.5, id + " tile edge/card separation");
            verify(_saturation(palette.tileEdge) > _saturation(palette.surfaceCard), id + " tile edge must be more saturated than the card");
        }
    }

    // The focused logo ramp is a gradient map, so its span is what turns an
    // antialiased glyph boundary into a rim light. Collapse the span and tinted
    // artwork goes flat. Measured in OKLCh L rather than Rec.709 relative
    // luminance -- the perceptual dimension the ramp is actually built in
    // (see ColorSchemes.qml) -- so the floor doesn't have to vary by preset
    // the way a luminance-space floor would (light presets structurally have
    // less lightness headroom above their own accent; `zaparoo-white`'s span
    // in relative-luminance terms is 0.274, well under a luminance-space 0.45
    // floor, even though its OKLCh L span is comparable to the other two).
    function test_focus_ramp_spans_deep_accent_to_near_text(): void {
        for (let i = 0; i < ColorSchemes.ids.length; i++) {
            const id = ColorSchemes.ids[i];
            const palette = ColorSchemes.palette(id);
            compare(Qt.color(palette.logoFocusSecondary), Qt.color(palette.accent), id + " focus midtone is the accent");
            const shadowL = ColorSchemes._toLch(ColorSchemes._srgbToOklab(palette.logoFocusShadow)).L;
            const primaryL = ColorSchemes._toLch(ColorSchemes._srgbToOklab(palette.logoFocusPrimary)).L;
            verify(primaryL - shadowL >= 0.25, id + " focus ramp OKLCh L span " + (primaryL - shadowL));
        }
    }

    // The whole fix for item 3 ("reads more brown than orange"): mixing
    // toward a lightness extreme in OKLCh cannot desaturate a color the way
    // the old per-channel sRGB lerp did. Both focus rungs must retain a
    // meaningful fraction of the accent's own chroma -- the light-end
    // (`logoFocusPrimary`) floor is lower than the shadow-end floor because
    // sRGB's own gamut holds less chroma at high lightness regardless of
    // color space, not because of any deliberate desaturation here.
    // Shadow-rung floor stays 55%. Primary-rung floor relaxes 45% -> 33%
    // (round 5) -- sRGB's own gamut holds less chroma at high lightness
    // regardless of color space, and several real, popular accents
    // (Dracula's purple, Gruvbox's orange, Tokyo Night's blue) land in the
    // 33-45% band at the light end without any hex change.
    function test_focus_ramp_retains_accent_chroma(): void {
        for (let i = 0; i < ColorSchemes.ids.length; i++) {
            const id = ColorSchemes.ids[i];
            const palette = ColorSchemes.palette(id);
            const accentC = ColorSchemes._toLch(ColorSchemes._srgbToOklab(palette.accent)).C;
            const shadowC = ColorSchemes._toLch(ColorSchemes._srgbToOklab(palette.logoFocusShadow)).C;
            const primaryC = ColorSchemes._toLch(ColorSchemes._srgbToOklab(palette.logoFocusPrimary)).C;
            verify(shadowC >= 0.55 * accentC, id + " focus shadow chroma " + shadowC + " below 55% of accent chroma " + accentC);
            verify(primaryC >= 0.33 * accentC, id + " focus primary chroma " + primaryC + " below 33% of accent chroma " + accentC);
        }
    }

    // Semantic tier (items 2, 4, 5, 6): `onAccent` is the body-text/glyph
    // role for anything sitting on a solid `accent` fill (the inverted
    // selection bar, the favorite heart on a selected row); `onAccentMuted`
    // is its subordinate variant (tag suffixes, the "off" toggle track on a
    // selected row); `marker` is the fixed-hue state-marker role (favorite
    // heart, Hidden badge) kept independent of `accent` so it can't blend
    // into the focus ring.
    function test_on_accent_clears_body_text_contrast(): void {
        for (let i = 0; i < ColorSchemes.ids.length; i++) {
            const id = ColorSchemes.ids[i];
            const palette = ColorSchemes.palette(id);
            verify(_contrastRatio(palette.onAccent, palette.accent) >= 4.5, id + " onAccent/accent must clear body-text contrast");
        }
    }

    function test_on_accent_muted_lands_in_subordinate_band(): void {
        for (let i = 0; i < ColorSchemes.ids.length; i++) {
            const id = ColorSchemes.ids[i];
            const palette = ColorSchemes.palette(id);
            const ratio = _contrastRatio(palette.onAccentMuted, palette.accent);
            verify(ratio >= 3.0 && ratio < 4.5, id + " onAccentMuted/accent " + ratio + " must land in the 3.0-4.5 band");
        }
    }

    function test_marker_is_legible_and_distinct_from_accent(): void {
        for (let i = 0; i < ColorSchemes.ids.length; i++) {
            const id = ColorSchemes.ids[i];
            const palette = ColorSchemes.palette(id);
            verify(_contrastRatio(palette.marker, palette.surfaceCard) >= 3.0, id + " marker/surfaceCard must clear large-graphic contrast");
            const markerHue = ColorSchemes._hueDegrees(ColorSchemes._toLch(ColorSchemes._srgbToOklab(palette.marker)).h);
            const accentHue = ColorSchemes._hueDegrees(ColorSchemes._toLch(ColorSchemes._srgbToOklab(palette.accent)).h);
            verify(ColorSchemes._hueDelta(markerHue, accentHue) >= 40, id + " marker hue too close to accent's own hue");
        }
    }

    // The resting ramp has to read as clearly dimmer than the focus ramp on
    // every rung, not just on average — that is the whole fix for "focus
    // reads as ring only" (see docs/style.md → "Darker unfocused artwork").
    function test_resting_logo_ramp_is_dimmer_than_focus_ramp(): void {
        const rungs = [["logoPrimary", "logoFocusPrimary"], ["logoSecondary", "logoFocusSecondary"], ["logoShadow", "logoFocusShadow"]];
        for (let i = 0; i < ColorSchemes.ids.length; i++) {
            const id = ColorSchemes.ids[i];
            const palette = ColorSchemes.palette(id);
            for (let r = 0; r < rungs.length; r++) {
                const [restingName, focusName] = rungs[r];
                verify(_relativeLuminance(palette[restingName]) < _relativeLuminance(palette[focusName]), id + " " + restingName + " must be dimmer than " + focusName);
            }
        }
    }

    // A selected browse/settings row is inverse video: body text painted in
    // `bgDeep` on a solid `accent` fill (see SelectionBar.qml). Normal-size
    // text needs the stricter 4.5:1 floor, not the 3:1 large-text minimum
    // `test_presets_keep_content_and_focus_legible` checks for the accent
    // chip itself.
    function test_accent_against_bg_deep_clears_body_text_contrast(): void {
        for (let i = 0; i < ColorSchemes.ids.length; i++) {
            const id = ColorSchemes.ids[i];
            const palette = ColorSchemes.palette(id);
            verify(_contrastRatio(palette.accent, palette.bgDeep) >= 4.5, id + " accent/bgDeep must clear body-text contrast for the inverted selection bar");
        }
    }

    function test_theme_switches_live_and_unknown_falls_back(): void {
        Theme.colorSchemeId = "midnight-amber";
        compare(Theme.effectiveColorSchemeId, "midnight-amber");
        compare(Theme.accent, "#ffb347");

        Theme.colorSchemeId = "zaparoo-white";
        compare(Theme.effectiveColorSchemeId, "zaparoo-white");
        compare(Theme.accent, "#0a63c9");
        compare(Theme.bgDeep, "#f2f3f5");

        Theme.colorSchemeId = "zaparoo-black";
        compare(Theme.effectiveColorSchemeId, "zaparoo-black");
        compare(Theme.accent, "#168bff");
        compare(Theme.bgDeep, "#050608");

        Theme.colorSchemeId = "removed-preset";
        compare(Theme.effectiveColorSchemeId, ColorSchemes.defaultId);
        compare(Theme.accent, "#168bff");
    }
}
