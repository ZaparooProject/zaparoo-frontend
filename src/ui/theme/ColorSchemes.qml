// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
pragma Singleton
import QtQuick

// Color presets. A preset authors exactly three colors and this file derives
// every remaining semantic role from them:
//
//   primary  the page background, and the base every surface is mixed from
//   accent   focus, and every highlight in the UI
//   text     primary content color
//
// Components consume Theme roles rather than indexing these objects directly;
// this catalog owns preset identity, fallback, and the derivation ladder. Two
// roles deliberately do not derive. `scrim` is always a dark veil because its
// job is to separate a panel from what sits behind it in either direction, and
// `errorHex` is a semantic constant — deriving it from the accent would make an
// amber preset signal danger in amber.
QtObject {
    readonly property string defaultId: "zaparoo-black"
    readonly property var ids: ["zaparoo-black", "midnight-amber", "zaparoo-white"]

    // `zaparoo-white`'s accent is a darkened brand blue: #168bff only reaches
    // 2.97:1 against a near-white page, just under the 3.0 floor the preset
    // guardrails enforce.
    readonly property var _sources: ({
            "zaparoo-black": {
                "primary": "#050608",
                "accent": "#168bff",
                "text": "#f7f7f5"
            },
            "midnight-amber": {
                "primary": "#0f0f23",
                "accent": "#ffb347",
                "text": "#ffffff"
            },
            "zaparoo-white": {
                "primary": "#f2f3f5",
                "accent": "#0a63c9",
                "text": "#101418"
            }
        })

    // Qt.lighter()/Qt.darker() are HSV value multiplies, so they are no-ops on
    // pure black and cannot express the ladder below. Mix channels explicitly.
    function _luma(value: color): real {
        return 0.2126 * value.r + 0.7152 * value.g + 0.0722 * value.b;
    }

    function _mix(from: color, to: color, amount: real): color {
        return Qt.rgba(from.r + (to.r - from.r) * amount, from.g + (to.g - from.g) * amount, from.b + (to.b - from.b) * amount, 1);
    }

    function isKnown(id: string): bool {
        return ids.indexOf(id) !== -1;
    }

    function effectiveId(id: string): string {
        return isKnown(id) ? id : defaultId;
    }

    // Whether a preset's background reads as the light end of its own
    // primary/text pair — i.e. `up` (see palette()) is false. Exists for
    // asset selection that a color role cannot express, such as picking
    // between a light-wordmark and dark-wordmark header logo PNG. Computed
    // independently of palette() so it stays out of the derived-role object
    // and tst_color_schemes.qml's requiredRoles opacity loop.
    function isLightSurface(id: string): bool {
        const source = _sources[effectiveId(id)];
        return _luma(Qt.color(source.primary)) > _luma(Qt.color(source.text));
    }

    function palette(id: string): var {
        const source = _sources[effectiveId(id)];
        const primary = Qt.color(source.primary);
        const accent = Qt.color(source.accent);
        const text = Qt.color(source.text);

        // `up` is true when text is lighter than the background, i.e. a dark
        // preset. `ink` is the pole *away* from the text, so mixing toward it
        // deepens a surface whichever direction the preset runs.
        const up = _luma(text) > _luma(primary);
        const ink = Qt.color(up ? "#000000" : "#ffffff");
        // The two logo ramps and the marker outline anchor on these instead
        // of on `primary`/`text` directly, so they order by luma rather than
        // by authoring role and cannot invert when a preset swaps which of
        // the two is the lighter one.
        const lightPole = up ? text : primary;
        const darkPole = up ? primary : text;

        // Neutral ladder: step the background toward the text, then give each
        // rung a slight accent cast so a warm accent yields warm surfaces
        // without the preset having to say so.
        const panel = _mix(_mix(primary, text, 0.05), accent, 0.04);
        const card = _mix(_mix(primary, text, 0.08), accent, 0.05);
        const subtle = _mix(_mix(primary, text, 0.14), accent, 0.03);
        const mid = _mix(_mix(primary, text, 0.32), accent, 0.05);
        const variant = _mix(_mix(primary, text, 0.58), accent, 0.14);

        // Selection and the pressable front edge ride the accent ramp. They
        // start one rung up the neutral ladder so they keep a little body on a
        // near-black primary instead of collapsing into the background.
        const edgeBase = _mix(primary, text, 0.06);
        const selection = _mix(edgeBase, accent, 0.22);

        return {
            "bgDeep": primary,
            "bgPanel": panel,
            "bgBar": _mix(primary, ink, 0.35),
            "surfaceCard": card,
            "selectionSurface": selection,
            // Shading is physical, so the recessed row's shaded wall darkens in
            // both directions. The amount differs because equal channel mixes
            // are not equal perceived steps at opposite ends of the sRGB curve.
            "selectionShade": _mix(selection, Qt.color("#000000"), up ? 0.40 : 0.12),
            "tileEdge": _mix(edgeBase, accent, 0.44),
            "controlEdge": _mix(edgeBase, accent, 0.54),
            "scrim": "#cc000000",
            "borderSubtle": subtle,
            "borderMid": mid,
            "textPrimary": text,
            "textLabel": _mix(primary, text, 0.62),
            "textVariant": variant,
            "accent": accent,
            // Both logo ramps run on the accent hue rather than the neutral
            // primary/text axis. `lightPole`/`darkPole` order the two preset
            // colors by luma rather than by authoring role, so `logoShadow`
            // is always the darker end even on a light preset (where `text`
            // is the dark color and `primary` is the light one) — the ramp
            // can no longer run backwards the way a `primary`-anchored mix
            // did. Resting keeps a strong accent weight throughout so an
            // unfocused tile reads as tinted, not desaturated; the focused
            // ramp's wider span is what turns an antialiased glyph boundary
            // into a rim light. Every rung of the focus ramp is lighter than
            // its resting counterpart — including the shadow rung — so a
            // focused tile reads brighter as a whole, not just at its rim.
            "logoShadow": _mix(accent, darkPole, 0.55),
            "logoSecondary": _mix(accent, darkPole, 0.18),
            "logoPrimary": _mix(accent, lightPole, 0.28),
            "logoFocusShadow": _mix(accent, darkPole, 0.30),
            "logoFocusSecondary": accent,
            "logoFocusPrimary": _mix(accent, lightPole, 0.85),
            // Marker keyline (e.g. the favorite heart's outline): the pole
            // opposite the accent's own luma, nudged toward the accent
            // itself so it reads as "this marker's rim," not a flat
            // black/white sticker outline, and never collapses onto
            // `primary` the way nudging toward `primary` would on a preset
            // where the chosen pole equals `primary` (light presets: `up`
            // is false, so `lightPole` is exactly `primary`).
            "markerOutline": _mix(_luma(accent) > 0.5 ? darkPole : lightPole, accent, 0.12),
            "errorHex": up ? "#ff4f91" : "#c2185b"
        };
    }
}
