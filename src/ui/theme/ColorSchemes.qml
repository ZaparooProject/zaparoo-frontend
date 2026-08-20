// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
pragma Singleton
import QtQuick

// Color presets. A preset authors exactly three colors; this file derives a
// semantic tier from them, and every remaining component role from that tier:
//
//   primary  the page background, and the base every surface is mixed from
//   accent   focus, selection, and every highlight in the UI
//   text     primary content color
//
// The semantic tier (`onAccent`, `onAccentMuted`, `marker`) exists because
// state markers used to ride directly on `accent` — a convention that
// assumes a dark accent and breaks on a mid-luma one. See docs/style.md ->
// "Semantic tier".
//
// Components consume Theme roles rather than indexing these objects directly;
// this catalog owns preset identity, fallback, and the derivation ladder. Two
// roles deliberately do not derive. `scrim` is always a dark veil because its
// job is to separate a panel from what sits behind it in either direction, and
// `errorHex` is a semantic constant — deriving it from the accent would make an
// amber preset signal danger in amber.
QtObject {
    readonly property string defaultId: "zaparoo-black"
    readonly property var ids: ["zaparoo-black", "midnight-amber", "zaparoo-white", "catppuccin-mocha", "catppuccin-macchiato", "catppuccin-frappe", "nord", "dracula", "gruvbox-dark", "gruvbox-light", "tokyo-night", "rose-pine", "kanagawa-wave", "ayu-dark", "nightfox", "monokai", "one-dark-pro", "everforest-dark", "synthwave-84", "amber-phosphor", "green-phosphor", "neo-geo", "nes", "virtual-boy"]

    // `zaparoo-white`'s accent is a darkened brand blue: #168bff only reaches
    // 2.97:1 against a near-white page, just under the 3.0 floor the preset
    // guardrails enforce.
    //
    // The presets from `catppuccin-mocha` on are real, unmodified hex triads
    // pulled from popular editor/terminal themes and documented retro/console
    // palettes (round 5) -- see docs/style.md -> "Preset catalog" for sourcing
    // and the two guardrail floors relaxed to admit them without touching a
    // single hex.
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
            },
            "catppuccin-mocha": {
                "primary": "#1e1e2e",
                "accent": "#cba6f7",
                "text": "#cdd6f4"
            },
            "catppuccin-macchiato": {
                "primary": "#24273a",
                "accent": "#c6a0f6",
                "text": "#cad3f5"
            },
            "catppuccin-frappe": {
                "primary": "#303446",
                "accent": "#ca9ee6",
                "text": "#c6d0f5"
            },
            "nord": {
                "primary": "#2e3440",
                "accent": "#88c0d0",
                "text": "#eceff4"
            },
            "dracula": {
                "primary": "#282a36",
                "accent": "#bd93f9",
                "text": "#f8f8f2"
            },
            "gruvbox-dark": {
                "primary": "#282828",
                "accent": "#fe8019",
                "text": "#ebdbb2"
            },
            "gruvbox-light": {
                "primary": "#fbf1c7",
                "accent": "#af3a03",
                "text": "#3c3836"
            },
            "tokyo-night": {
                "primary": "#1a1b26",
                "accent": "#7aa2f7",
                "text": "#c0caf5"
            },
            "rose-pine": {
                "primary": "#191724",
                "accent": "#c4a7e7",
                "text": "#e0def4"
            },
            "kanagawa-wave": {
                "primary": "#1f1f28",
                "accent": "#7e9cd8",
                "text": "#dcd7ba"
            },
            "ayu-dark": {
                "primary": "#0b0e14",
                "accent": "#e6b450",
                "text": "#bfbdb6"
            },
            "nightfox": {
                "primary": "#192330",
                "accent": "#719cd6",
                "text": "#cdcecf"
            },
            "monokai": {
                "primary": "#272822",
                "accent": "#a6e22e",
                "text": "#f8f8f2"
            },
            "one-dark-pro": {
                "primary": "#282c34",
                "accent": "#61afef",
                "text": "#dcdfe4"
            },
            "everforest-dark": {
                "primary": "#2d353b",
                "accent": "#a7c080",
                "text": "#d3c6aa"
            },
            "synthwave-84": {
                "primary": "#262335",
                "accent": "#ff7edb",
                "text": "#ffffff"
            },
            "amber-phosphor": {
                "primary": "#100b00",
                "accent": "#ffb000",
                "text": "#ffcc66"
            },
            "green-phosphor": {
                "primary": "#001b00",
                "accent": "#00cc33",
                "text": "#33ff33"
            },
            "neo-geo": {
                "primary": "#101010",
                "accent": "#ffb300",
                "text": "#f0f0f0"
            },
            "nes": {
                "primary": "#0d0d0d",
                "accent": "#f83800",
                "text": "#fcfcfc"
            },
            "virtual-boy": {
                "primary": "#100000",
                "accent": "#ff2020",
                "text": "#ff8080"
            }
        })

    // Qt.lighter()/Qt.darker() are HSV value multiplies, so they are no-ops on
    // pure black and cannot express the ladder below. Mix channels explicitly.
    // Gamma-space Rec.709 weights — used only to *order* two colors (which one
    // is lighter), not to measure contrast. `_relativeLuminance` below is the
    // sRGB-linearized figure the contrast guardrails actually compare.
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

    // The three authored colors for `id`, in primary/accent/text order — the
    // picker's swatch preview (SettingsScreen.qml -> ListPickerModal.qml),
    // and the one place outside palette() itself that needs the raw triad
    // rather than the derived ladder. Independent of the active
    // Theme.colorSchemeId, so it can preview any preset without switching.
    function previewColors(id: string): var {
        const source = _sources[effectiveId(id)];
        return [Qt.color(source.primary), Qt.color(source.accent), Qt.color(source.text)];
    }

    // ── OKLab / OKLCh (Björn Ottosson) ──────────────────────────────────────
    // sRGB per-channel lerp does not preserve chroma: mixing a saturated
    // accent toward a near-black or near-white pole desaturates it on the
    // way, which is what made a focused amber tile read near-white and a
    // focused orange tile read brown. Every role below that carries the
    // accent's own hue is derived in OKLCh instead, holding chroma and hue
    // fixed while only lightness moves. The neutral surface ladder above
    // stays on `_mix` — it only mixes near-neutrals, where the sRGB error is
    // too small to matter.

    function _cbrt(value: real): real {
        return value < 0 ? -Math.pow(-value, 1 / 3) : Math.pow(value, 1 / 3);
    }

    function _srgbToLinearChannel(value: real): real {
        return value <= 0.04045 ? value / 12.92 : Math.pow((value + 0.055) / 1.055, 2.4);
    }

    function _linearToSrgbChannel(value: real): real {
        const clamped = Math.max(0, Math.min(1, value));
        const encoded = clamped <= 0.0031308 ? clamped * 12.92 : 1.055 * Math.pow(clamped, 1 / 2.4) - 0.055;
        return Math.max(0, Math.min(1, encoded));
    }

    function _srgbToOklab(value: color): var {
        const r = _srgbToLinearChannel(value.r);
        const g = _srgbToLinearChannel(value.g);
        const b = _srgbToLinearChannel(value.b);
        const l = 0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b;
        const m = 0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b;
        const s = 0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b;
        const l_ = _cbrt(l);
        const m_ = _cbrt(m);
        const s_ = _cbrt(s);
        return {
            "L": 0.2104542553 * l_ + 0.7936177850 * m_ - 0.0040720468 * s_,
            "a": 1.9779984951 * l_ - 2.4285922050 * m_ + 0.4505937099 * s_,
            "b": 0.0259040371 * l_ + 0.7827717662 * m_ - 0.8086757660 * s_
        };
    }

    function _oklabToLinearSrgb(L: real, a: real, b: real): var {
        const l_ = L + 0.3963377774 * a + 0.2158037573 * b;
        const m_ = L - 0.1055613458 * a - 0.0638541728 * b;
        const s_ = L - 0.0894841775 * a - 1.2914855480 * b;
        const l = l_ * l_ * l_;
        const m = m_ * m_ * m_;
        const s = s_ * s_ * s_;
        return {
            "r": 4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
            "g": -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
            "b": -0.0041960863 * l - 0.7034186147 * m + 1.7076147010 * s
        };
    }

    function _oklabToSrgb(L: real, a: real, b: real): color {
        const linear = _oklabToLinearSrgb(L, a, b);
        return Qt.rgba(_linearToSrgbChannel(linear.r), _linearToSrgbChannel(linear.g), _linearToSrgbChannel(linear.b), 1);
    }

    function _toLch(lab: var): var {
        return {
            "L": lab.L,
            "C": Math.sqrt(lab.a * lab.a + lab.b * lab.b),
            "h": Math.atan2(lab.b, lab.a)
        };
    }

    function _fromLch(L: real, C: real, h: real): var {
        return {
            "L": L,
            "a": C * Math.cos(h),
            "b": C * Math.sin(h)
        };
    }

    function _oklchInGamut(L: real, C: real, h: real): bool {
        const lab = _fromLch(L, C, h);
        const linear = _oklabToLinearSrgb(lab.L, lab.a, lab.b);
        const eps = 0.00005;
        return linear.r >= -eps && linear.r <= 1 + eps && linear.g >= -eps && linear.g <= 1 + eps && linear.b >= -eps && linear.b <= 1 + eps;
    }

    // Reduce chroma at a fixed L/h until every sRGB channel lands in gamut,
    // then convert. Binary search rather than an analytic gamut boundary —
    // OKLCh's gamut is not a simple shape, and this runs only a handful of
    // times per scheme switch. Hue is never altered, only chroma.
    function _gamutFit(L: real, C: real, h: real): color {
        const clampedL = Math.max(0, Math.min(1, L));
        if (C <= 0)
            return _oklabToSrgb(clampedL, 0, 0);
        if (_oklchInGamut(clampedL, C, h)) {
            const lab = _fromLch(clampedL, C, h);
            return _oklabToSrgb(lab.L, lab.a, lab.b);
        }
        let lo = 0;
        let hi = C;
        for (let i = 0; i < 24; i++) {
            const mid = (lo + hi) / 2;
            if (_oklchInGamut(clampedL, mid, h))
                lo = mid;
            else
                hi = mid;
        }
        const lab = _fromLch(clampedL, lo, h);
        return _oklabToSrgb(lab.L, lab.a, lab.b);
    }

    // A neutral (C = 0) at a given OKLCh lightness — the perceptual
    // replacement for the old gamma-space `accentGrey` channel-average.
    function _greyAt(L: real): color {
        return _oklabToSrgb(Math.max(0, Math.min(1, L)), 0, 0);
    }

    // ── Contrast (WCAG relative luminance) ──────────────────────────────────
    function _relativeChannelLuminance(value: real): real {
        return value <= 0.04045 ? value / 12.92 : Math.pow((value + 0.055) / 1.055, 2.4);
    }

    function _relativeLuminance(value: color): real {
        return 0.2126 * _relativeChannelLuminance(value.r) + 0.7152 * _relativeChannelLuminance(value.g) + 0.0722 * _relativeChannelLuminance(value.b);
    }

    function _contrastRatio(first: color, second: color): real {
        const a = _relativeLuminance(first);
        const b = _relativeLuminance(second);
        return (Math.max(a, b) + 0.05) / (Math.min(a, b) + 0.05);
    }

    // Seed-agnostic guardrail: if the authored (or, later, user-supplied)
    // accent falls below 4.5:1 against `primary`, walk its OKLCh lightness
    // away from the background until it clears, preserving hue and chroma.
    // Inert for the three shipped presets — they already clear this floor —
    // but is what makes an arbitrary hex safe to seed the whole ladder from.
    function _clampAccent(accent: color, primary: color): color {
        if (_contrastRatio(accent, primary) >= 4.5)
            return accent;
        const lch = _toLch(_srgbToOklab(accent));
        const primaryL = _srgbToOklab(primary).L;
        const direction = primaryL > 0.5 ? -1 : 1;
        let L = lch.L;
        let adjusted = accent;
        for (let i = 0; i < 40; i++) {
            L = Math.max(0, Math.min(1, L + direction * 0.02));
            adjusted = _gamutFit(L, lch.C, lch.h);
            if (_contrastRatio(adjusted, primary) >= 4.5 || L <= 0 || L >= 1)
                break;
        }
        console.warn("ColorSchemes: accent adjusted in lightness to clear 4.5:1 against primary");
        return adjusted;
    }

    // ── Semantic tier ────────────────────────────────────────────────────────
    // `onAccent` starts from whichever of the preset's own primary/text poles
    // already contrasts better against `accent`, then walks that pole's OKLCh
    // lightness further in the same direction (holding its chroma and hue —
    // so a preset with a tinted background/text keeps that identity) until it
    // clears 4.5:1. This is `SelectionBar.contentColor` (previously a flat
    // `Theme.bgDeep`), `TileBadge`'s label/fill, and the favorite heart's
    // on-accent fill.
    function _onAccentFor(accent: color, primary: color, text: color): color {
        const primaryContrast = _contrastRatio(primary, accent);
        const textContrast = _contrastRatio(text, accent);
        const start = textContrast >= primaryContrast ? text : primary;
        const startLch = _toLch(_srgbToOklab(start));
        const accentL = _srgbToOklab(accent).L;
        const direction = startLch.L >= accentL ? 1 : -1;
        let L = startLch.L;
        let candidate = _gamutFit(L, startLch.C, startLch.h);
        for (let i = 0; i < 60 && _contrastRatio(candidate, accent) < 4.5; i++) {
            L = Math.max(0, Math.min(1, L + direction * 0.01));
            candidate = _gamutFit(L, startLch.C, startLch.h);
            if (L <= 0 || L >= 1)
                break;
        }
        return candidate;
    }

    // Secondary on-accent content (tag suffixes, the "off" toggle track on a
    // selected row): `onAccent`'s own hue and chroma, lightness walked toward
    // `accent` until contrast lands in the 3.0-4.5 band — present, but
    // subordinate to `onAccent`'s full-contrast text.
    function _onAccentMutedFor(onAccent: color, accent: color): color {
        const lch = _toLch(_srgbToOklab(onAccent));
        const accentL = _srgbToOklab(accent).L;
        const direction = lch.L >= accentL ? -1 : 1;
        let L = lch.L;
        let candidate = onAccent;
        for (let i = 0; i < 200 && _contrastRatio(candidate, accent) > 4.5; i++) {
            L = Math.max(0, Math.min(1, L + direction * 0.005));
            candidate = _gamutFit(L, lch.C, lch.h);
            if (L <= 0 || L >= 1)
                break;
        }
        for (let i = 0; i < 200 && _contrastRatio(candidate, accent) < 3.0; i++) {
            L = Math.max(0, Math.min(1, L - direction * 0.002));
            candidate = _gamutFit(L, lch.C, lch.h);
        }
        return candidate;
    }

    function _hueDegrees(radians: real): real {
        let deg = radians * 180 / Math.PI;
        while (deg < 0)
            deg += 360;
        while (deg >= 360)
            deg -= 360;
        return deg;
    }

    function _hueDelta(a: real, b: real): real {
        const d = Math.abs(a - b) % 360;
        return d > 180 ? 360 - d : d;
    }

    // A fixed warm-red hue — the favorite heart and Hidden badge, so
    // favorites read the same red across every theme switch rather than
    // riding the accent (an amber preset should not signal "favorited" in
    // amber, the same reasoning `errorHex` documents at the top of this
    // file). Rotated in 90° steps away from `accent`'s own hue when they
    // would collide, so a red-accented preset can't cancel the marker's
    // identity. Lightness is fitted for >=3:1 against `surface`.
    function _markerFor(accent: color, surface: color): color {
        const baseHueDeg = 29;
        const accentHueDeg = _hueDegrees(_toLch(_srgbToOklab(accent)).h);
        let hueDeg = baseHueDeg;
        let guard = 0;
        while (_hueDelta(hueDeg, accentHueDeg) < 40 && guard < 8) {
            hueDeg += 90;
            guard++;
        }
        const hue = hueDeg * Math.PI / 180;
        const chroma = 0.19;
        const surfaceL = _srgbToOklab(surface).L;
        const direction = surfaceL > 0.5 ? -1 : 1;
        let L = surfaceL > 0.5 ? 0.35 : 0.65;
        let candidate = _gamutFit(L, chroma, hue);
        for (let i = 0; i < 60 && _contrastRatio(candidate, surface) < 3.0; i++) {
            L = Math.max(0, Math.min(1, L + direction * 0.01));
            candidate = _gamutFit(L, chroma, hue);
            if (L <= 0 || L >= 1)
                break;
        }
        return candidate;
    }

    function palette(id: string): var {
        const source = _sources[effectiveId(id)];
        const primary = Qt.color(source.primary);
        const text = Qt.color(source.text);
        const accent = _clampAccent(Qt.color(source.accent), primary);

        // `up` is true when text is lighter than the background, i.e. a dark
        // preset. `ink` is the pole *away* from the text, so mixing toward it
        // deepens a surface whichever direction the preset runs.
        const up = _luma(text) > _luma(primary);
        const ink = Qt.color(up ? "#000000" : "#ffffff");
        // The marker outline anchors on these instead of on `primary`/`text`
        // directly, so it orders by luma rather than by authoring role.
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

        // The pressable front edge rides the accent ramp. It starts one rung
        // up the neutral ladder so it keeps a little body on a near-black
        // primary instead of collapsing into the background.
        const edgeBase = _mix(primary, text, 0.06);

        // Semantic tier — see the derivation functions above.
        const onAccent = _onAccentFor(accent, primary, text);
        const onAccentMuted = _onAccentMutedFor(onAccent, accent);
        const marker = _markerFor(accent, card);

        // Both logo ramps ride the accent's own OKLCh hue and (mostly) its
        // chroma, so mixing toward a lightness extreme cannot desaturate it —
        // that desaturation was items 1 and 3 (near-white focus artwork, and
        // an orange tint reading brown). The resting ramp reuses the same
        // three lightness rungs, desaturated to grey and pulled dimmer than
        // their focus counterparts, so a page of unfocused tiles reads as a
        // plain grid and a focused one visibly gains both color and light.
        const accentLch = _toLch(_srgbToOklab(accent));
        const focusShadowL = Math.max(0.32, accentLch.L - 0.22);
        const focusPrimaryL = Math.min(0.90, accentLch.L + 0.16);

        return {
            "bgDeep": primary,
            "bgPanel": panel,
            "bgBar": _mix(primary, ink, 0.35),
            "surfaceCard": card,
            "tileEdge": _mix(edgeBase, accent, 0.44),
            "controlEdge": _mix(edgeBase, accent, 0.54),
            "scrim": "#cc000000",
            "borderSubtle": subtle,
            "borderMid": mid,
            "textPrimary": text,
            "textLabel": _mix(primary, text, 0.62),
            "textVariant": variant,
            "accent": accent,
            "onAccent": onAccent,
            "onAccentMuted": onAccentMuted,
            "logoShadow": _greyAt(Math.max(0.26, accentLch.L - 0.28)),
            "logoSecondary": _greyAt(Math.max(0.10, accentLch.L - 0.16)),
            "logoPrimary": _greyAt(Math.min(0.74, accentLch.L)),
            "logoFocusShadow": _gamutFit(focusShadowL, accentLch.C, accentLch.h),
            "logoFocusSecondary": accent,
            "logoFocusPrimary": _gamutFit(focusPrimaryL, accentLch.C * 0.72, accentLch.h),
            "marker": marker,
            // The pole opposite the marker's own luma, nudged toward the
            // marker itself so it reads as "this marker's rim," not a flat
            // black/white sticker outline.
            "markerOutline": _mix(_luma(marker) > 0.5 ? darkPole : lightPole, marker, 0.12),
            "errorHex": up ? "#ff4f91" : "#c2185b"
        };
    }
}
