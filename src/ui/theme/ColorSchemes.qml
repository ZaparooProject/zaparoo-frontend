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
    readonly property string defaultId: "zaparoo-dark"
    // Round 10: reordered into family blocks -- Zaparoo identity pinned
    // first, then retro/console, then editor/terminal, then light --
    // alphabetical by display name within each block. Previously plain
    // addition-history order (each round's new presets appended at the
    // end), which read as random rather than deliberate. See
    // docs/style.md -> "Preset catalog" for the full history.
    readonly property var ids: ["zaparoo-dark", "zaparoo-light", "classic-purple", "amber-phosphor", "game-boy", "green-phosphor", "neo-geo", "nes", "virtual-boy", "dracula", "everforest", "gruvbox", "nord", "oxocarbon", "rose-pine", "solarized-dark", "synthwave-84", "flexoki-paper", "solarized-light"]

    // `zaparoo-light`'s accent is a darkened brand blue: #168bff only reaches
    // 2.97:1 against a near-white page, just under the 3.0 floor the preset
    // guardrails enforce.
    //
    // Round 6 pruned the round-5 catalog from 24 presets to 11: Zaparoo,
    // phosphor, and console presets were kept unconditionally, and every
    // other family that read as a near-duplicate of one of these (three
    // Catppuccin variants next to Dracula; Tokyo Night/One Dark
    // Pro/Nightfox/Kanagawa all in the same blue-on-slate register as Nord;
    // Rose Pine and Everforest with no console/phosphor counterpart to
    // differentiate from) was cut rather than kept for its own sake.
    //
    // Round 7 grew the catalog back to 19: the round-6 prune fixed
    // redundancy but left the survivors too samey (several near-duplicate
    // "dark bg + one accent" presets) and didn't leave room for a preset
    // whose background register itself differs, not just its accent hue.
    // Eight presets were added back or introduced fresh -- Gruvbox,
    // Everforest, Solarized Dark, Rose Pine, and Oxocarbon (each picked for
    // a background register nothing else in the catalog covers: neutral
    // warm-grey, blue-green slate, deep teal-black, purple-black with a
    // rose rather than violet accent, and true neutral near-black,
    // respectively), Flexoki Paper and Solarized Light (a second and third
    // light preset, now that darkening a theme's own ink color -- rather
    // than using its softer default text tone -- was found to clear every
    // guardrail Gruvbox Light couldn't), and Game Boy (a third
    // retro-console preset, its DMG green deepened slightly off the real
    // hardware value to clear the text/bg contrast floor). Classic Purple's
    // triad was also retuned in round 7: its original blue-purple
    // background against its orange accent was a near-complementary
    // pairing that read as a clash rather than a preset in its own right;
    // the background was warmed toward the accent's hue instead. See
    // docs/style.md -> "Preset catalog" for the full round-6 and round-7
    // accounting, including why Gruvbox Light specifically still doesn't
    // survive even though this round's substitution technique fixed the
    // other light presets: its accent must sit dark to clear
    // `_clampAccent`'s 4.5:1 floor on a light page, which reads as a heavy
    // brown next to near-black body text regardless of which ink is
    // chosen -- a property of that specific accent hue on a light page, not
    // a tuning miss.
    readonly property var _sources: ({
            "zaparoo-dark": {
                "primary": "#050608",
                "accent": "#168bff",
                "text": "#f7f7f5"
            },
            "classic-purple": {
                "primary": "#120e26",
                "accent": "#ffbc4d",
                "text": "#f2eeff"
            },
            "zaparoo-light": {
                "primary": "#f2f3f5",
                "accent": "#0a63c9",
                "text": "#101418"
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
            },
            "gruvbox": {
                "primary": "#282828",
                "accent": "#fe8019",
                "text": "#ebdbb2"
            },
            "everforest": {
                "primary": "#1b2228",
                "accent": "#a7c080",
                "text": "#d3c6aa"
            },
            "solarized-dark": {
                "primary": "#002b36",
                "accent": "#2aa198",
                "text": "#eee8d5"
            },
            "rose-pine": {
                "primary": "#191724",
                "accent": "#eb6f92",
                "text": "#e0def4"
            },
            "oxocarbon": {
                "primary": "#161616",
                "accent": "#3ddbd9",
                "text": "#f2f4f8"
            },
            "flexoki-paper": {
                "primary": "#fffcf0",
                "accent": "#af3029",
                "text": "#100f0f"
            },
            "solarized-light": {
                "primary": "#fdf6e3",
                "accent": "#268bd2",
                "text": "#073642"
            },
            "game-boy": {
                "primary": "#041004",
                "accent": "#b4d420",
                "text": "#9bbc0f"
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
    // `Theme.bgDeep`) and the favorite heart's on-accent fill (the variant
    // drawn over a selected `SelectionBar` row).
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

    // Ambient accent-carrying edge roles (`tileEdge`/`controlEdge`): the
    // resting front-edge strip on every tile and button, painted
    // regardless of focus. These were the only two accent-carrying roles
    // still on `_mix` (sRGB per-channel lerp) instead of this file's OKLCh
    // path -- at the old 0.44/0.54 mix fractions toward a saturated accent
    // this was not the near-neutral mix `_mix`'s own module doc says is
    // fine, so it landed inconsistently loud across the catalog: hot on
    // high-chroma presets (`nes`, `virtual-boy`, `game-boy`,
    // `synthwave-84`), muddy on others. Reported independently by two beta
    // testers as overpowering cover art.
    //
    // Chroma is CAPPED, not scaled: proportional scaling preserves the
    // loudness ordering (the presets already flagged as too loud stay
    // loudest), a cap pulls the loud ones down to the same ceiling the
    // quiet presets already sit near. `controlEdge` gets a slightly higher
    // factor and cap than `tileEdge` so it stays a touch stronger even at
    // the ceiling -- the old 0.44/0.54 mix fractions kept that ordering by
    // construction, a shared cap alone would have erased it on
    // high-chroma presets.
    //
    // Lightness is SOLVED for a minimum separation ratio against `card`
    // rather than a fixed offset: a fixed delta cleared only ~1.27:1 at
    // the dark end of the catalog, under the 1.5:1 floor
    // `test_edge_reads_as_a_lit_bevel` asserts. Walking L away from
    // `card`'s own lightness until the ratio clears (the same
    // walk-until-it-clears pattern `_clampAccent`/`_onAccentFor` already
    // use) holds the floor by construction on every preset instead of
    // needing per-preset eyeballing.
    // `card` is the surface the edge sits directly against (giving the
    // "raised surface" 3D separation cue); `ground` is what's typically
    // behind the whole component (the Hub's deep background for a tile,
    // a settings panel for a menu-row-style control) —
    // `tst_pressable_surface.qml`'s `test_edge_uses_contextual_middle_tone`
    // asserts against `ground`, this file's own
    // `test_edge_reads_as_a_lit_bevel` asserts against `card`. Both must
    // clear at once: `card`/`panel`/`bgDeep` sit on the same neutral
    // ladder (consecutive rungs stepping from `primary` toward `text`), so
    // walking L away from whichever pole `card` itself leans toward moves
    // away from all of them together, not just the one the loop happens
    // to check last.
    function _edgeFor(accent: color, card: color, chromaFactor: real, chromaCap: real, cardMinContrast: real, ground: color, groundMinContrast: real): color {
        const accentLch = _toLch(_srgbToOklab(accent));
        const cardLab = _srgbToOklab(card);
        const cardLch = _toLch(cardLab);
        // Guaranteed separation from the card's OWN chroma, not just an
        // absolute ceiling independent of it. `_mix` is a plain sRGB lerp,
        // not OKLab-space, so it doesn't preserve chroma proportionally —
        // a near-black primary mixed even slightly toward a high-chroma
        // accent (e.g. `game-boy`'s saturated yellow-green over its
        // near-black primary) can push the card's own perceptual chroma up
        // more than the small mix fraction suggests, exactly the general
        // risk this file's own module doc already names for mixing a
        // saturated accent toward a near-black pole. A fixed cap alone
        // doesn't account for that; comparing against the card's measured
        // chroma does.
        const chroma = Math.max(Math.min(accentLch.C * chromaFactor, chromaCap), cardLch.C + 0.01);
        const direction = cardLab.L > 0.5 ? -1 : 1;
        let L = cardLab.L;
        let candidate = _gamutFit(L, chroma, accentLch.h);
        for (let i = 0; i < 80 && (_contrastRatio(candidate, card) < cardMinContrast || _contrastRatio(candidate, ground) < groundMinContrast); i++) {
            L = Math.max(0, Math.min(1, L + direction * 0.01));
            candidate = _gamutFit(L, chroma, accentLch.h);
            if (L <= 0 || L >= 1)
                break;
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
            "tileEdge": _edgeFor(accent, card, 0.5, 0.05, 1.5, primary, 1.8),
            "controlEdge": _edgeFor(accent, card, 0.6, 0.06, 1.65, panel, 2.0),
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
            "errorHex": up ? "#ff4f91" : "#c2185b",
            // QR quiet-zone/module colors (round 6, item 4 — see
            // docs/style.md -> "Themed QR codes"). `qrLight` stays the light
            // rung and `qrDark` stays the dark rung regardless of whether
            // the preset itself is light or dark: inverted QR is out of
            // spec and scans unreliably, and this is the primary path for
            // writing a token from a phone. Both ride the accent's own
            // OKLCh hue so the code still reads as themed — a faint tint on
            // the quiet zone, accent-hued ink — while measuring >=6.0:1
            // against each other on every preset (test_qr_rungs_stay_scannable).
            "qrLight": _gamutFit(0.965, Math.min(accentLch.C, 0.022), accentLch.h),
            "qrDark": _gamutFit(Math.min(accentLch.L, 0.45), accentLch.C, accentLch.h)
        };
    }
}
