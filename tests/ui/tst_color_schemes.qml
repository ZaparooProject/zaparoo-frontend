// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

import QtQuick
import QtTest
import Zaparoo.Theme

TestCase {
    name: "ColorSchemes"
    when: windowShown

    readonly property var requiredRoles: ["bgDeep", "bgPanel", "bgBar", "surfaceCard", "selectionSurface", "selectionShade", "tileEdge", "controlEdge", "scrim", "borderSubtle", "borderMid", "textPrimary", "textLabel", "textVariant", "accent", "logoPrimary", "logoSecondary", "logoShadow", "logoFocusPrimary", "logoFocusSecondary", "logoFocusShadow", "errorHex"]

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
        compare(ColorSchemes.ids.length, 3);
        verify(ColorSchemes.isKnown("zaparoo-black"));
        verify(ColorSchemes.isKnown("midnight-amber"));
        verify(ColorSchemes.isKnown("zaparoo-white"));
        compare(ColorSchemes.effectiveId("missing"), ColorSchemes.defaultId);
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

    function test_presets_keep_content_and_focus_legible(): void {
        for (let i = 0; i < ColorSchemes.ids.length; i++) {
            const id = ColorSchemes.ids[i];
            const palette = ColorSchemes.palette(id);
            verify(_contrastRatio(palette.textPrimary, palette.bgDeep) >= 7.0, id + " primary text/background");
            verify(_contrastRatio(palette.textPrimary, palette.surfaceCard) >= 7.0, id + " primary text/card");
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
    // artwork goes flat.
    function test_focus_ramp_spans_deep_accent_to_near_text(): void {
        for (let i = 0; i < ColorSchemes.ids.length; i++) {
            const id = ColorSchemes.ids[i];
            const palette = ColorSchemes.palette(id);
            compare(Qt.color(palette.logoFocusSecondary), Qt.color(palette.accent), id + " focus midtone is the accent");
            const span = Math.abs(_relativeLuminance(palette.logoFocusPrimary) - _relativeLuminance(palette.logoFocusShadow));
            verify(span >= 0.45, id + " focus ramp span " + span);
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
