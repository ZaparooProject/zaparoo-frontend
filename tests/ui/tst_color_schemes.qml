// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

import QtQuick
import QtTest
import Zaparoo.Theme

TestCase {
    name: "ColorSchemes"
    when: windowShown

    readonly property var requiredRoles: ["bgDeep", "bgPanel", "bgBar", "surfaceCard", "tileEdge", "controlEdge", "scrim", "borderSubtle", "borderMid", "textPrimary", "textLabel", "textVariant", "accent", "selectionFill", "onAccent", "onAccentMuted", "logoPrimary", "logoSecondary", "logoShadow", "logoFocusPrimary", "logoFocusSecondary", "logoFocusShadow", "marker", "markerOutline", "errorHex", "qrLight", "qrDark"]

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

    function _cbrt(value: real): real {
        return value < 0 ? -Math.pow(-value, 1 / 3) : Math.pow(value, 1 / 3);
    }

    function _srgbToLinearChannel(value: real): real {
        return value <= 0.04045 ? value / 12.92 : Math.pow((value + 0.055) / 1.055, 2.4);
    }

    // OKLCh chroma (Bjorn Ottosson's OKLab) -- independently reimplemented
    // from ColorSchemes.qml's own `_srgbToOklab`/`_toLch` (same numbers,
    // verified separately here rather than calling into the production
    // helpers, matching this file's existing `_relativeLuminance`/
    // `_contrastRatio` pattern) so a regression in that derivation itself
    // gets caught, not just a regression in how the palette consumes it.
    // `_saturation` above is HSV, which orders by the color's own r/g/b
    // spread rather than perceptual colorfulness -- it disagrees with
    // chroma exactly on presets whose *background* is itself a saturated
    // color (`solarized-dark`, `everforest`, `nord`, `green-phosphor`,
    // `virtual-boy`), which is what this role's own guardrail needs to
    // measure against.
    function _chroma(value: color): real {
        const r = _srgbToLinearChannel(value.r);
        const g = _srgbToLinearChannel(value.g);
        const b = _srgbToLinearChannel(value.b);
        const l = 0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b;
        const m = 0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b;
        const s = 0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b;
        const l_ = _cbrt(l);
        const m_ = _cbrt(m);
        const s_ = _cbrt(s);
        const a = 1.9779984951 * l_ - 2.4285922050 * m_ + 0.4505937099 * s_;
        const bLab = 0.0259040371 * l_ + 0.7827717662 * m_ - 0.8086757660 * s_;
        return Math.sqrt(a * a + bLab * bLab);
    }

    function cleanup(): void {
        Theme.colorSchemeId = ColorSchemes.defaultId;
    }

    function test_catalog_ids_and_fallback(): void {
        compare(ColorSchemes.defaultId, "zaparoo-dark");
        compare(ColorSchemes.ids.length, 19);
        verify(ColorSchemes.isKnown("zaparoo-dark"));
        verify(ColorSchemes.isKnown("classic-purple"));
        verify(ColorSchemes.isKnown("zaparoo-light"));
        verify(ColorSchemes.isKnown("dracula"));
        // Round-5 ids pruned in round 6 must no longer resolve as known —
        // this is the regression a stale isKnown() cache would miss.
        verify(!ColorSchemes.isKnown("zaparoo-black"));
        verify(!ColorSchemes.isKnown("midnight-amber"));
        verify(!ColorSchemes.isKnown("catppuccin-mocha"));
        compare(ColorSchemes.effectiveId("missing"), ColorSchemes.defaultId);
    }

    // Asset-selection flag for HeaderBar's logo PNG ladder (item 3b) — not a
    // color role, so it is deliberately absent from requiredRoles. Must
    // agree with each preset's own primary/text luma ordering, independent
    // of palette()'s internal `up` flag.
    function test_is_light_surface_matches_preset_luma(): void {
        verify(!ColorSchemes.isLightSurface("zaparoo-dark"));
        verify(!ColorSchemes.isLightSurface("classic-purple"));
        verify(ColorSchemes.isLightSurface("zaparoo-light"));
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
    // relaxes to AA 4.5:1 (round 5): several real palettes admitted that
    // round sat in the 4.5-7.0 band on their own card surface by design and
    // would otherwise have required altering their authored hex to ship.
    // Round 6 pruned every preset that actually needed the relaxed floor
    // (every kept preset clears 7.3:1+) but the floor stays at 4.5 rather
    // than reverting to 7.0 -- it is a legitimate AA guarantee on its own,
    // and reverting would just have to relax again for the next real
    // palette added. See docs/style.md -> "Preset catalog".
    function test_presets_keep_content_and_focus_legible(): void {
        for (let i = 0; i < ColorSchemes.ids.length; i++) {
            const id = ColorSchemes.ids[i];
            const palette = ColorSchemes.palette(id);
            verify(_contrastRatio(palette.textPrimary, palette.bgDeep) >= 7.0, id + " primary text/background");
            verify(_contrastRatio(palette.textPrimary, palette.surfaceCard) >= 4.5, id + " primary text/card");
            verify(_contrastRatio(palette.accent, palette.bgDeep) >= 3.0, id + " accent/background");
            verify(_contrastRatio(palette.textLabel, palette.bgDeep) >= 3.0, id + " label/background");
            // Round N: `textLabel` was only floored against `bgDeep` above,
            // not `surfaceCard`, where most of it actually renders (menu/
            // settings rows, tile captions); `textVariant` had no floor at
            // all, even though it's the dimmest text role in the palette.
            // Both matter more at 540p, where a TV's own scaling/sharpening
            // is the first thing to make marginal contrast go mushy.
            verify(_contrastRatio(palette.textLabel, palette.surfaceCard) >= 3.0, id + " label/card");
            verify(_contrastRatio(palette.textVariant, palette.surfaceCard) >= 3.0, id + " variant/card");
        }
    }

    // Part of what reads as a "shiny" 3D surface is that the pressable front
    // edge is chromatically separated from the face along the accent ramp, not
    // merely a darker version of it. A derivation that flattens the edge into
    // the card loses the gloss cue even though nothing looks obviously broken.
    //
    // Separation is measured in OKLCh chroma, not HSV saturation: HSV
    // saturation orders by a color's own r/g/b spread, which is high on a
    // preset whose *background* is itself a saturated hue
    // (`solarized-dark`, `everforest`, `nord`, `green-phosphor`,
    // `virtual-boy`) even when the edge's perceptual chroma is
    // deliberately low there -- chroma is what these roles are actually
    // tuned in (see `ColorSchemes.qml`'s `_edgeFor`), so it's what the
    // guardrail needs to measure. `controlEdge` gets the same floor as
    // `tileEdge` (previously untested) since it carries the "this is a
    // button" cue on every menu/settings row the same way `tileEdge` does
    // on tiles.
    function test_edge_reads_as_a_lit_bevel(): void {
        for (let i = 0; i < ColorSchemes.ids.length; i++) {
            const id = ColorSchemes.ids[i];
            const palette = ColorSchemes.palette(id);
            verify(_contrastRatio(palette.tileEdge, palette.surfaceCard) >= 1.5, id + " tile edge/card separation");
            verify(_contrastRatio(palette.controlEdge, palette.surfaceCard) >= 1.5, id + " control edge/card separation");
            verify(_chroma(palette.tileEdge) > _chroma(palette.surfaceCard), id + " tile edge must be more chromatic than the card");
            verify(_chroma(palette.controlEdge) > _chroma(palette.surfaceCard), id + " control edge must be more chromatic than the card");
        }
    }

    // "Color intensity" ships defaulted to Subtle precisely so that adding
    // the setting changes nothing for an install that never touches it. That
    // promise is only worth anything if it's checked: every role, every
    // preset, byte for byte against the no-argument call every other test
    // here (and Theme, before the setting existed) makes.
    function test_subtle_intensity_reproduces_the_shipped_palette(): void {
        for (let i = 0; i < ColorSchemes.ids.length; i++) {
            const id = ColorSchemes.ids[i];
            const shipped = ColorSchemes.palette(id);
            const subtle = ColorSchemes.palette(id, "subtle");
            for (let r = 0; r < requiredRoles.length; r++) {
                const role = requiredRoles[r];
                compare(Qt.color(subtle[role]), Qt.color(shipped[role]), id + " " + role + " must be unchanged at Subtle");
            }
        }
    }

    // An unknown or empty value must land on Subtle, not on a broken palette
    // — a state.toml hand-edited to a typo, or written by a newer build with
    // a third intensity, still has to render.
    function test_unknown_intensity_falls_back_to_subtle(): void {
        const subtle = ColorSchemes.palette("nes", "subtle");
        compare(Qt.color(ColorSchemes.palette("nes", "not-an-intensity").tileEdge), Qt.color(subtle.tileEdge));
        compare(Qt.color(ColorSchemes.palette("nes", "").tileEdge), Qt.color(subtle.tileEdge));
        compare(ColorSchemes.defaultIntensity, "subtle");
    }

    // The whole point of Vivid is that the accent actually reaches the
    // resting chrome, so assert it moved rather than only that it stayed
    // legal. Measured on the high-chroma presets, which are the ones the
    // chroma cap binds hardest and the ones testers picked expecting vivid
    // color ("I liked the red underlines on the NES theme").
    function test_vivid_intensity_raises_ambient_accent(): void {
        const presets = ["nes", "virtual-boy", "game-boy", "synthwave-84"];
        for (let i = 0; i < presets.length; i++) {
            const id = presets[i];
            const subtle = ColorSchemes.palette(id, "subtle");
            const vivid = ColorSchemes.palette(id, "vivid");
            verify(_chroma(vivid.tileEdge) > _chroma(subtle.tileEdge), id + " vivid tile edge must be more chromatic than subtle");
            verify(_chroma(vivid.controlEdge) > _chroma(subtle.controlEdge), id + " vivid control edge must be more chromatic than subtle");
            // Surfaces are NOT part of the setting -- see `_intensities`.
            // Asserted rather than left implicit so a future attempt to
            // scale the ladder cast has to come back through that comment.
            compare(Qt.color(vivid.surfaceCard), Qt.color(subtle.surfaceCard), id + " card must not change with intensity");
            compare(Qt.color(vivid.textVariant), Qt.color(subtle.textVariant), id + " variant text must not change with intensity");
            compare(Qt.color(vivid.accent), Qt.color(subtle.accent), id + " accent must not change with intensity");
            compare(Qt.color(vivid.marker), Qt.color(subtle.marker), id + " marker must not change with intensity");
        }
    }

    // Vivid scales the inputs to the same solvers, so every floor the shipped
    // palette clears must still clear. Mirrors the assertions in
    // `test_edge_reads_as_a_lit_bevel` and
    // `test_presets_keep_content_and_focus_legible`, run against the other
    // intensity — a scale factor tuned too far up is exactly the regression
    // this catches, and it would otherwise ship invisible because every other
    // test in this file only ever exercises Subtle.
    function test_vivid_intensity_holds_every_contrast_floor(): void {
        for (let i = 0; i < ColorSchemes.ids.length; i++) {
            const id = ColorSchemes.ids[i];
            const palette = ColorSchemes.palette(id, "vivid");
            verify(_contrastRatio(palette.tileEdge, palette.surfaceCard) >= 1.5, id + " vivid tile edge/card separation");
            verify(_contrastRatio(palette.controlEdge, palette.surfaceCard) >= 1.5, id + " vivid control edge/card separation");
            verify(_chroma(palette.tileEdge) > _chroma(palette.surfaceCard), id + " vivid tile edge must be more chromatic than the card");
            verify(_chroma(palette.controlEdge) > _chroma(palette.surfaceCard), id + " vivid control edge must be more chromatic than the card");
            // Surfaces do not scale, so these should be untouched — kept
            // as a guardrail against a change that starts moving them.
            verify(_contrastRatio(palette.textPrimary, palette.surfaceCard) >= 4.5, id + " vivid primary text/card");
            verify(_contrastRatio(palette.textLabel, palette.surfaceCard) >= 3.0, id + " vivid label/card");
            verify(_contrastRatio(palette.textVariant, palette.surfaceCard) >= 3.0, id + " vivid variant/card");
            verify(_contrastRatio(palette.textPrimary, palette.bgPanel) >= 4.5, id + " vivid primary text/panel");
            verify(_contrastRatio(palette.borderMid, palette.surfaceCard) >= 1.2, id + " vivid borderMid/card");
        }
    }

    // sRGB lerp, the same arithmetic as ColorSchemes.qml's `_mix`,
    // reimplemented here for the same reason `_chroma` is.
    function _mix(from: color, to: color, amount: real): color {
        return Qt.rgba(from.r + (to.r - from.r) * amount, from.g + (to.g - from.g) * amount, from.b + (to.b - from.b) * amount, 1);
    }

    // Vivid exists because testers missed the pre-`_edgeFor` bevel: a plain
    // 44% step from the neutral ladder toward the accent,
    // `_mix(_mix(primary, text, 0.06), accent, 0.44)`, which shipped through
    // e8ff996. That bevel was both more chromatic AND lighter than what the
    // contrast-floor solver produces, so a Vivid that only raised the
    // chroma cap still read flatter than the original ("less vivid than it
    // was, even with Vivid selected"). Both axes are asserted: at least the
    // original's chroma, and a higher edge/card contrast than Subtle.
    // Measured on the high-chroma presets, where the cap binds and the
    // difference is what testers see.
    function test_vivid_intensity_restores_the_original_bevel(): void {
        const presets = ["nes", "virtual-boy", "game-boy", "synthwave-84"];
        for (let i = 0; i < presets.length; i++) {
            const id = presets[i];
            const subtle = ColorSchemes.palette(id, "subtle");
            const vivid = ColorSchemes.palette(id, "vivid");
            const original = _mix(_mix(Qt.color(vivid.bgDeep), Qt.color(vivid.textPrimary), 0.06), Qt.color(vivid.accent), 0.44);
            verify(_chroma(vivid.tileEdge) >= _chroma(original), id + " vivid tile edge chroma " + _chroma(vivid.tileEdge).toFixed(3) + " must reach the original bevel's " + _chroma(original).toFixed(3));
            verify(_contrastRatio(vivid.tileEdge, vivid.surfaceCard) > _contrastRatio(subtle.tileEdge, subtle.surfaceCard), id + " vivid tile edge must sit lighter off the card than subtle");
            verify(_contrastRatio(vivid.controlEdge, vivid.surfaceCard) > _contrastRatio(subtle.controlEdge, subtle.surfaceCard), id + " vivid control edge must sit lighter off the card than subtle");
        }
    }

    // The focused logo ramp is a gradient map, so its span is what turns an
    // antialiased glyph boundary into a rim light. Collapse the span and tinted
    // artwork goes flat. Measured in OKLCh L rather than Rec.709 relative
    // luminance -- the perceptual dimension the ramp is actually built in
    // (see ColorSchemes.qml) -- so the floor doesn't have to vary by preset
    // the way a luminance-space floor would (light presets structurally have
    // less lightness headroom above their own accent; `zaparoo-light`'s span
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
    // regardless of color space, and two of the round-6 catalog's own kept
    // accents still land in that band at the light end without any hex
    // change: Dracula's purple (~39%) and Synthwave '84's pink (~38%).
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
    // Measured against `selectionFill`, not `accent`: the fill is the ground
    // every piece of on-accent content actually sits on (SelectionBar's text,
    // the settings toggle knob border, the favorite heart on a highlighted
    // row). Focus rings use the raw accent but never render text on it.
    // Checked at both intensities, since the fill's chroma is what the
    // Color intensity setting scales.
    function test_on_accent_clears_body_text_contrast(): void {
        const intensities = ["subtle", "vivid"];
        for (let n = 0; n < intensities.length; n++) {
            for (let i = 0; i < ColorSchemes.ids.length; i++) {
                const id = ColorSchemes.ids[i];
                const palette = ColorSchemes.palette(id, intensities[n]);
                verify(_contrastRatio(palette.onAccent, palette.selectionFill) >= 4.5, id + " " + intensities[n] + " onAccent/selectionFill must clear body-text contrast");
            }
        }
    }

    // The selection fill keeps the accent's hue and lightness and only drops
    // chroma, so a selected row still reads as the accent colour rather than
    // as grey, and its lightness (which is what carries the contrast) does not
    // drift. Subtle pulls harder than Vivid -- that is the whole point of
    // tying it to the setting.
    function test_selection_fill_desaturates_without_shifting_lightness(): void {
        for (let i = 0; i < ColorSchemes.ids.length; i++) {
            const id = ColorSchemes.ids[i];
            const subtle = ColorSchemes.palette(id, "subtle");
            const vivid = ColorSchemes.palette(id, "vivid");
            const accentL = ColorSchemes._toLch(ColorSchemes._srgbToOklab(subtle.accent)).L;
            const fillL = ColorSchemes._toLch(ColorSchemes._srgbToOklab(subtle.selectionFill)).L;
            verify(Math.abs(fillL - accentL) < 0.02, id + " selection fill must keep the accent's lightness");
            verify(_chroma(subtle.selectionFill) < _chroma(subtle.accent), id + " selection fill must be less chromatic than the accent");
            verify(_chroma(subtle.selectionFill) < _chroma(vivid.selectionFill), id + " Subtle must desaturate the fill further than Vivid");
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

    // ContextMenu and ListPickerModal rows moved from PressableSurface to the
    // same inverse-video SelectionBar as browse/settings rows (round 7 --
    // see docs/style.md -> "Two registers"), painting a resting row's label
    // in `textPrimary` directly against `bgPanel` rather than a nested
    // `surfaceCard`. Nothing previously guarded `bgPanel` contrast at all --
    // it only existed as a background fill, never a surface body text sat
    // on directly. `bgPanel` is derived the same way `bgDeep` is (barely
    // mixed away from the raw authored `primary` -- see the `panel`/`bgDeep`
    // derivations above), so it's held to the same 7.0 AAA floor rather than
    // `surfaceCard`'s lighter-touch 4.5, matching how close a background it
    // actually is. A selected row's `onAccent`-on-`accent` pairing is already
    // covered by the guardrail below, independent of whatever sits behind
    // the panel.
    function test_text_primary_against_bg_panel_clears_body_text_contrast(): void {
        for (let i = 0; i < ColorSchemes.ids.length; i++) {
            const id = ColorSchemes.ids[i];
            const palette = ColorSchemes.palette(id);
            verify(_contrastRatio(palette.textPrimary, palette.bgPanel) >= 7.0, id + " textPrimary/bgPanel must clear body-text contrast for menu/picker rows at rest");
        }
    }

    function test_theme_switches_live_and_unknown_falls_back(): void {
        Theme.colorSchemeId = "classic-purple";
        compare(Theme.effectiveColorSchemeId, "classic-purple");
        compare(Theme.accent, "#ffbc4d");

        Theme.colorSchemeId = "zaparoo-light";
        compare(Theme.effectiveColorSchemeId, "zaparoo-light");
        compare(Theme.accent, "#0a63c9");
        compare(Theme.bgDeep, "#f2f3f5");

        Theme.colorSchemeId = "zaparoo-dark";
        compare(Theme.effectiveColorSchemeId, "zaparoo-dark");
        compare(Theme.accent, "#168bff");
        compare(Theme.bgDeep, "#050608");

        // A round-5 id that used to be valid must fall back like any other
        // unknown value now that round 6 pruned it.
        Theme.colorSchemeId = "midnight-amber";
        compare(Theme.effectiveColorSchemeId, ColorSchemes.defaultId);
        compare(Theme.accent, "#168bff");

        Theme.colorSchemeId = "removed-preset";
        compare(Theme.effectiveColorSchemeId, ColorSchemes.defaultId);
        compare(Theme.accent, "#168bff");
    }

    // Item 4 (round 6): QR quiet-zone/module rungs never invert regardless
    // of whether the preset itself is dark or light, and stay scannable.
    // 6.0 floor is comfortably under the measured 6.37-7.43:1 range across
    // the round-6 catalog -- see docs/style.md -> "Themed QR codes".
    function test_qr_rungs_stay_scannable(): void {
        for (let i = 0; i < ColorSchemes.ids.length; i++) {
            const id = ColorSchemes.ids[i];
            const palette = ColorSchemes.palette(id);
            verify(_relativeLuminance(palette.qrLight) > _relativeLuminance(palette.qrDark), id + " qrLight must be the lighter rung");
            verify(_contrastRatio(palette.qrLight, palette.qrDark) >= 6.0, id + " QR rung contrast " + _contrastRatio(palette.qrLight, palette.qrDark));
        }
    }
}
