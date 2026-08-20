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
            // Resting logo ramp is the neutral ladder. The focused ramp is the
            // accent ramp, and its span is what reads as a specular highlight
            // on tinted artwork: deep accent through accent to near-text.
            "logoShadow": _mix(_mix(primary, text, 0.16), accent, 0.22),
            "logoSecondary": _mix(_mix(primary, text, 0.45), accent, 0.10),
            "logoPrimary": _mix(_mix(primary, text, 0.72), accent, 0.04),
            // Mixing toward black compresses relative luminance far harder than
            // mixing toward white, so a light preset has to travel further to
            // reach the same perceived ramp span.
            "logoFocusShadow": _mix(accent, ink, up ? 0.35 : 0.62),
            "logoFocusSecondary": accent,
            "logoFocusPrimary": _mix(accent, text, 0.92),
            "errorHex": up ? "#ff4f91" : "#c2185b"
        };
    }
}
