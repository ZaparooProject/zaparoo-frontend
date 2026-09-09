// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// Port of `src/ui/theme/ColorSchemes.qml`: the preset catalog and the
// derivation ladder that turns a preset's three authored colors (primary,
// accent, text) into every component role the frontend paints with. Pinned
// to `tests/fixtures/palette_golden.txt`, which was dumped from the QML
// implementation, so every role matches the Qt build to the 8-bit hex.
//
// Matching the QML exactly means matching Qt's color arithmetic, not just
// the math: every `Qt.rgba(...)`/`Qt.color(...)` boundary in the QML stores
// channels as 16-bit integers (`QColor`), reads them back as `float`, and
// prints them through a rounding division by 257. `Rgb16` reproduces those
// three conversions; the derivation itself runs in f64 like the JS did.
//
// Two roles deliberately do not derive: `scrim` is always a dark veil, and
// `error` is a semantic constant (deriving it from the accent would make an
// amber preset signal danger in amber). The reasoning behind each walk and
// factor is in the QML's comments and `docs/style.md`; this file keeps the
// numbers and the order of operations.

use core::f64::consts::PI;

/// A color the way `QColor` stores it: three 16-bit channels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Rgb16 {
    r: u16,
    g: u16,
    b: u16,
}

impl Rgb16 {
    /// `Qt.color("#rrggbb")`: each 8-bit channel expands to 16 bits by 257.
    pub fn from_hex(hex: &str) -> Option<Self> {
        let digits = hex.strip_prefix('#')?;
        if digits.len() != 6 {
            return None;
        }
        let channel = |at: usize| u8::from_str_radix(digits.get(at..at + 2)?, 16).ok();
        Some(Self {
            r: u16::from(channel(0)?) * 0x101,
            g: u16::from(channel(2)?) * 0x101,
            b: u16::from(channel(4)?) * 0x101,
        })
    }

    /// `Qt.rgba(r, g, b, 1)`: the doubles narrow to `float`, then
    /// `qRound(channel * 65535)` in single precision.
    pub fn from_f(r: f64, g: f64, b: f64) -> Self {
        Self {
            r: qround_65535(r),
            g: qround_65535(g),
            b: qround_65535(b),
        }
    }

    /// `color.r` in QML: `redF()`, a `float` division that JS then widens.
    pub fn r(self) -> f64 {
        f64::from(f32::from(self.r) / 65535.0f32)
    }

    pub fn g(self) -> f64 {
        f64::from(f32::from(self.g) / 65535.0f32)
    }

    pub fn b(self) -> f64 {
        f64::from(f32::from(self.b) / 65535.0f32)
    }

    /// The 8-bit channels `QColor::name()` prints.
    pub fn rgb8(self) -> (u8, u8, u8) {
        (div_257(self.r), div_257(self.g), div_257(self.b))
    }

    /// `"#rrggbb"`, as `String(color)` yields for an opaque color in QML.
    pub fn hex(self) -> String {
        let (r, g, b) = self.rgb8();
        format!("#{r:02x}{g:02x}{b:02x}")
    }
}

fn qround_65535(value: f64) -> u16 {
    let scaled = (value as f32) * 65535.0f32;
    // qRound(float): int(d + 0.5f) for the non-negative range fromRgbF accepts.
    let rounded = (scaled + 0.5f32).floor();
    rounded.clamp(0.0, 65535.0) as u16
}

/// Qt's `qt_div_257`: 16-bit to 8-bit with rounding.
fn div_257(value: u16) -> u8 {
    let x = u32::from(value) + 128;
    ((x - (x >> 8)) >> 8) as u8
}

/// A preset's three authored colors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Source {
    pub primary: &'static str,
    pub accent: &'static str,
    pub text: &'static str,
}

/// The catalog, in picker order (identity first, then retro/console, then
/// editor/terminal, then light; alphabetical by display name within a
/// block). `ids()` is the public ordering.
const CATALOG: [(&str, Source); 19] = [
    (
        "zaparoo-dark",
        Source {
            primary: "#050608",
            accent: "#168bff",
            text: "#f7f7f5",
        },
    ),
    (
        "zaparoo-light",
        Source {
            primary: "#f2f3f5",
            accent: "#0a63c9",
            text: "#101418",
        },
    ),
    (
        "classic-purple",
        Source {
            primary: "#120e26",
            accent: "#ffbc4d",
            text: "#f2eeff",
        },
    ),
    (
        "amber-phosphor",
        Source {
            primary: "#100b00",
            accent: "#ffb000",
            text: "#ffcc66",
        },
    ),
    (
        "game-boy",
        Source {
            primary: "#041004",
            accent: "#b4d420",
            text: "#9bbc0f",
        },
    ),
    (
        "green-phosphor",
        Source {
            primary: "#001b00",
            accent: "#00cc33",
            text: "#33ff33",
        },
    ),
    (
        "neo-geo",
        Source {
            primary: "#101010",
            accent: "#ffb300",
            text: "#f0f0f0",
        },
    ),
    (
        "nes",
        Source {
            primary: "#0d0d0d",
            accent: "#f83800",
            text: "#fcfcfc",
        },
    ),
    (
        "virtual-boy",
        Source {
            primary: "#100000",
            accent: "#ff2020",
            text: "#ff8080",
        },
    ),
    (
        "dracula",
        Source {
            primary: "#282a36",
            accent: "#bd93f9",
            text: "#f8f8f2",
        },
    ),
    (
        "everforest",
        Source {
            primary: "#1b2228",
            accent: "#a7c080",
            text: "#d3c6aa",
        },
    ),
    (
        "gruvbox",
        Source {
            primary: "#282828",
            accent: "#fe8019",
            text: "#ebdbb2",
        },
    ),
    (
        "nord",
        Source {
            primary: "#2e3440",
            accent: "#88c0d0",
            text: "#eceff4",
        },
    ),
    (
        "oxocarbon",
        Source {
            primary: "#161616",
            accent: "#3ddbd9",
            text: "#f2f4f8",
        },
    ),
    (
        "rose-pine",
        Source {
            primary: "#191724",
            accent: "#eb6f92",
            text: "#e0def4",
        },
    ),
    (
        "solarized-dark",
        Source {
            primary: "#002b36",
            accent: "#2aa198",
            text: "#eee8d5",
        },
    ),
    (
        "synthwave-84",
        Source {
            primary: "#262335",
            accent: "#ff7edb",
            text: "#ffffff",
        },
    ),
    (
        "flexoki-paper",
        Source {
            primary: "#fffcf0",
            accent: "#af3029",
            text: "#100f0f",
        },
    ),
    (
        "solarized-light",
        Source {
            primary: "#fdf6e3",
            accent: "#268bd2",
            text: "#073642",
        },
    ),
];

pub const DEFAULT_ID: &str = "zaparoo-dark";
pub const DEFAULT_INTENSITY: &str = "subtle";

/// Preset ids in picker order.
pub fn ids() -> impl Iterator<Item = &'static str> {
    CATALOG.iter().map(|(id, _)| *id)
}

pub fn is_known(id: &str) -> bool {
    CATALOG.iter().any(|(known, _)| *known == id)
}

/// The id actually used: an unknown id falls back to the default preset.
pub fn effective_id(id: &str) -> &'static str {
    CATALOG
        .iter()
        .map(|(known, _)| *known)
        .find(|known| *known == id)
        .unwrap_or(DEFAULT_ID)
}

fn source(id: &str) -> Source {
    let effective = effective_id(id);
    CATALOG
        .iter()
        .find(|(known, _)| *known == effective)
        .map_or(CATALOG[0].1, |(_, source)| *source)
}

/// Whether a preset's background reads as the light end of its own
/// primary/text pair. Drives asset selection a color role cannot express,
/// such as the light versus dark wordmark.
pub fn is_light_surface(id: &str) -> bool {
    let source = source(id);
    luma(hex(source.primary)) > luma(hex(source.text))
}

/// The three authored colors, in primary/accent/text order, for a picker
/// swatch that previews any preset without switching to it.
pub fn preview_colors(id: &str) -> [Rgb16; 3] {
    let source = source(id);
    [hex(source.primary), hex(source.accent), hex(source.text)]
}

/// The two intensity settings, as multipliers on the ambient-accent terms.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Intensity {
    edge_chroma: f64,
    edge_cap: f64,
    edge_contrast: f64,
    selection_chroma: f64,
}

const SUBTLE: Intensity = Intensity {
    edge_chroma: 1.0,
    edge_cap: 1.0,
    edge_contrast: 1.0,
    selection_chroma: 0.7,
};

const VIVID: Intensity = Intensity {
    edge_chroma: 1.8,
    edge_cap: 3.4,
    edge_contrast: 1.6,
    selection_chroma: 0.9,
};

fn intensity(name: &str) -> Intensity {
    match name {
        "vivid" => VIVID,
        _ => SUBTLE,
    }
}

/// Every component role, in the order `Theme.qml` publishes them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Palette {
    pub bg_deep: Rgb16,
    pub bg_panel: Rgb16,
    pub bg_bar: Rgb16,
    pub surface_card: Rgb16,
    pub tile_edge: Rgb16,
    pub control_edge: Rgb16,
    /// Always a dark veil, with alpha: `#cc000000` in QML's `#aarrggbb`.
    pub scrim: &'static str,
    pub border_subtle: Rgb16,
    pub border_mid: Rgb16,
    pub text_primary: Rgb16,
    pub text_label: Rgb16,
    pub text_variant: Rgb16,
    pub accent: Rgb16,
    pub selection_fill: Rgb16,
    pub on_accent: Rgb16,
    pub on_accent_muted: Rgb16,
    pub logo_primary: Rgb16,
    pub logo_secondary: Rgb16,
    pub logo_shadow: Rgb16,
    pub logo_focus_primary: Rgb16,
    pub logo_focus_secondary: Rgb16,
    pub logo_focus_shadow: Rgb16,
    pub marker: Rgb16,
    pub marker_outline: Rgb16,
    /// Semantic constant, one per light/dark direction.
    pub error: Rgb16,
    pub qr_light: Rgb16,
    pub qr_dark: Rgb16,
}

impl Palette {
    /// `(role name as Theme.qml spells it, hex)` for every role, in order.
    pub fn roles(&self) -> [(&'static str, String); 27] {
        [
            ("bgDeep", self.bg_deep.hex()),
            ("bgPanel", self.bg_panel.hex()),
            ("bgBar", self.bg_bar.hex()),
            ("surfaceCard", self.surface_card.hex()),
            ("tileEdge", self.tile_edge.hex()),
            ("controlEdge", self.control_edge.hex()),
            ("scrim", self.scrim.to_string()),
            ("borderSubtle", self.border_subtle.hex()),
            ("borderMid", self.border_mid.hex()),
            ("textPrimary", self.text_primary.hex()),
            ("textLabel", self.text_label.hex()),
            ("textVariant", self.text_variant.hex()),
            ("accent", self.accent.hex()),
            ("selectionFill", self.selection_fill.hex()),
            ("onAccent", self.on_accent.hex()),
            ("onAccentMuted", self.on_accent_muted.hex()),
            ("logoPrimary", self.logo_primary.hex()),
            ("logoSecondary", self.logo_secondary.hex()),
            ("logoShadow", self.logo_shadow.hex()),
            ("logoFocusPrimary", self.logo_focus_primary.hex()),
            ("logoFocusSecondary", self.logo_focus_secondary.hex()),
            ("logoFocusShadow", self.logo_focus_shadow.hex()),
            ("marker", self.marker.hex()),
            ("markerOutline", self.marker_outline.hex()),
            ("errorHex", self.error.hex()),
            ("qrLight", self.qr_light.hex()),
            ("qrDark", self.qr_dark.hex()),
        ]
    }
}

fn hex(value: &str) -> Rgb16 {
    Rgb16::from_hex(value).unwrap_or(Rgb16 { r: 0, g: 0, b: 0 })
}

// Gamma-space Rec.709 weights: used only to order two colors, never to
// measure contrast.
fn luma(value: Rgb16) -> f64 {
    0.2126 * value.r() + 0.7152 * value.g() + 0.0722 * value.b()
}

fn mix(from: Rgb16, to: Rgb16, amount: f64) -> Rgb16 {
    Rgb16::from_f(
        from.r() + (to.r() - from.r()) * amount,
        from.g() + (to.g() - from.g()) * amount,
        from.b() + (to.b() - from.b()) * amount,
    )
}

// OKLab / OKLCh (Bjorn Ottosson).

#[derive(Debug, Clone, Copy)]
struct Lab {
    l: f64,
    a: f64,
    b: f64,
}

#[derive(Debug, Clone, Copy)]
struct Lch {
    l: f64,
    c: f64,
    h: f64,
}

fn cbrt(value: f64) -> f64 {
    if value < 0.0 {
        -(-value).powf(1.0 / 3.0)
    } else {
        value.powf(1.0 / 3.0)
    }
}

fn srgb_to_linear(value: f64) -> f64 {
    if value <= 0.040_45 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_srgb(value: f64) -> f64 {
    let clamped = value.clamp(0.0, 1.0);
    let encoded = if clamped <= 0.003_130_8 {
        clamped * 12.92
    } else {
        1.055 * clamped.powf(1.0 / 2.4) - 0.055
    };
    encoded.clamp(0.0, 1.0)
}

#[allow(
    clippy::many_single_char_names,
    reason = "the OKLab reference implementation names these l, m, s; keeping them keeps the port comparable"
)]
fn srgb_to_oklab(value: Rgb16) -> Lab {
    let r = srgb_to_linear(value.r());
    let g = srgb_to_linear(value.g());
    let b = srgb_to_linear(value.b());
    let l = 0.412_221_470_8 * r + 0.536_332_536_3 * g + 0.051_445_992_9 * b;
    let m = 0.211_903_498_2 * r + 0.680_699_545_1 * g + 0.107_396_956_6 * b;
    let s = 0.088_302_461_9 * r + 0.281_718_837_6 * g + 0.629_978_700_5 * b;
    let l_ = cbrt(l);
    let m_ = cbrt(m);
    let s_ = cbrt(s);
    Lab {
        l: 0.210_454_255_3 * l_ + 0.793_617_785_0 * m_ - 0.004_072_046_8 * s_,
        a: 1.977_998_495_1 * l_ - 2.428_592_205_0 * m_ + 0.450_593_709_9 * s_,
        b: 0.025_904_037_1 * l_ + 0.782_771_766_2 * m_ - 0.808_675_766_0 * s_,
    }
}

#[allow(
    clippy::many_single_char_names,
    reason = "the OKLab reference implementation names these l, m, s; keeping them keeps the port comparable"
)]
fn oklab_to_linear(lab: Lab) -> (f64, f64, f64) {
    let l_ = lab.l + 0.396_337_777_4 * lab.a + 0.215_803_757_3 * lab.b;
    let m_ = lab.l - 0.105_561_345_8 * lab.a - 0.063_854_172_8 * lab.b;
    let s_ = lab.l - 0.089_484_177_5 * lab.a - 1.291_485_548_0 * lab.b;
    let l = l_ * l_ * l_;
    let m = m_ * m_ * m_;
    let s = s_ * s_ * s_;
    (
        4.076_741_662_1 * l - 3.307_711_591_3 * m + 0.230_969_929_2 * s,
        -1.268_438_004_6 * l + 2.609_757_401_1 * m - 0.341_319_396_5 * s,
        -0.004_196_086_3 * l - 0.703_418_614_7 * m + 1.707_614_701_0 * s,
    )
}

fn oklab_to_srgb(lab: Lab) -> Rgb16 {
    let (r, g, b) = oklab_to_linear(lab);
    Rgb16::from_f(linear_to_srgb(r), linear_to_srgb(g), linear_to_srgb(b))
}

fn to_lch(lab: Lab) -> Lch {
    Lch {
        l: lab.l,
        c: (lab.a * lab.a + lab.b * lab.b).sqrt(),
        h: lab.b.atan2(lab.a),
    }
}

fn from_lch(l: f64, c: f64, h: f64) -> Lab {
    Lab {
        l,
        a: c * h.cos(),
        b: c * h.sin(),
    }
}

#[allow(
    clippy::many_single_char_names,
    reason = "the OKLab reference implementation names these l, m, s; keeping them keeps the port comparable"
)]
fn oklch_in_gamut(l: f64, c: f64, h: f64) -> bool {
    let (r, g, b) = oklab_to_linear(from_lch(l, c, h));
    let eps = 0.000_05;
    let inside = |v: f64| v >= -eps && v <= 1.0 + eps;
    inside(r) && inside(g) && inside(b)
}

/// Reduce chroma at a fixed L/h until every sRGB channel lands in gamut.
/// Binary search; hue is never altered, only chroma.
fn gamut_fit(l: f64, c: f64, h: f64) -> Rgb16 {
    let clamped_l = l.clamp(0.0, 1.0);
    if c <= 0.0 {
        return oklab_to_srgb(from_lch(clamped_l, 0.0, 0.0));
    }
    if oklch_in_gamut(clamped_l, c, h) {
        return oklab_to_srgb(from_lch(clamped_l, c, h));
    }
    let mut lo: f64 = 0.0;
    let mut hi = c;
    for _ in 0..24 {
        let mid = lo.midpoint(hi);
        if oklch_in_gamut(clamped_l, mid, h) {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    oklab_to_srgb(from_lch(clamped_l, lo, h))
}

/// A neutral at a given `OKLCh` lightness.
fn grey_at(l: f64) -> Rgb16 {
    oklab_to_srgb(Lab {
        l: l.clamp(0.0, 1.0),
        a: 0.0,
        b: 0.0,
    })
}

// Contrast (WCAG relative luminance).

fn relative_luminance(value: Rgb16) -> f64 {
    0.2126 * srgb_to_linear(value.r())
        + 0.7152 * srgb_to_linear(value.g())
        + 0.0722 * srgb_to_linear(value.b())
}

fn contrast_ratio(first: Rgb16, second: Rgb16) -> f64 {
    let a = relative_luminance(first);
    let b = relative_luminance(second);
    (a.max(b) + 0.05) / (a.min(b) + 0.05)
}

/// Seed-agnostic guardrail: walk the accent's `OKLCh` lightness away from
/// the background until it clears 4.5:1, preserving hue and chroma.
fn clamp_accent(accent: Rgb16, primary: Rgb16) -> Rgb16 {
    if contrast_ratio(accent, primary) >= 4.5 {
        return accent;
    }
    let lch = to_lch(srgb_to_oklab(accent));
    let primary_l = srgb_to_oklab(primary).l;
    let direction = if primary_l > 0.5 { -1.0 } else { 1.0 };
    let mut l = lch.l;
    let mut adjusted = accent;
    for _ in 0..40 {
        l = (l + direction * 0.02).clamp(0.0, 1.0);
        adjusted = gamut_fit(l, lch.c, lch.h);
        if contrast_ratio(adjusted, primary) >= 4.5 || l <= 0.0 || l >= 1.0 {
            break;
        }
    }
    adjusted
}

/// Same hue and lightness as the accent with chroma pulled down: the fill
/// behind text on a selected row.
fn selection_fill_for(accent: Rgb16, chroma_factor: f64) -> Rgb16 {
    let lch = to_lch(srgb_to_oklab(accent));
    gamut_fit(lch.l, lch.c * chroma_factor, lch.h)
}

/// Starts from whichever of primary/text already contrasts better against
/// the accent, then walks that pole's lightness further until 4.5:1.
fn on_accent_for(accent: Rgb16, primary: Rgb16, text: Rgb16) -> Rgb16 {
    let primary_contrast = contrast_ratio(primary, accent);
    let text_contrast = contrast_ratio(text, accent);
    let start = if text_contrast >= primary_contrast {
        text
    } else {
        primary
    };
    let start_lch = to_lch(srgb_to_oklab(start));
    let accent_l = srgb_to_oklab(accent).l;
    let direction = if start_lch.l >= accent_l { 1.0 } else { -1.0 };
    let mut l = start_lch.l;
    let mut candidate = gamut_fit(l, start_lch.c, start_lch.h);
    let mut i = 0;
    while i < 60 && contrast_ratio(candidate, accent) < 4.5 {
        l = (l + direction * 0.01).clamp(0.0, 1.0);
        candidate = gamut_fit(l, start_lch.c, start_lch.h);
        if l <= 0.0 || l >= 1.0 {
            break;
        }
        i += 1;
    }
    candidate
}

/// `onAccent`'s hue and chroma, lightness walked toward the accent until
/// contrast lands in the 3.0 to 4.5 band.
fn on_accent_muted_for(on_accent: Rgb16, accent: Rgb16) -> Rgb16 {
    let lch = to_lch(srgb_to_oklab(on_accent));
    let accent_l = srgb_to_oklab(accent).l;
    let direction = if lch.l >= accent_l { -1.0 } else { 1.0 };
    let mut l = lch.l;
    let mut candidate = on_accent;
    let mut i = 0;
    while i < 200 && contrast_ratio(candidate, accent) > 4.5 {
        l = (l + direction * 0.005).clamp(0.0, 1.0);
        candidate = gamut_fit(l, lch.c, lch.h);
        if l <= 0.0 || l >= 1.0 {
            break;
        }
        i += 1;
    }
    let mut i = 0;
    while i < 200 && contrast_ratio(candidate, accent) < 3.0 {
        l = (l - direction * 0.002).clamp(0.0, 1.0);
        candidate = gamut_fit(l, lch.c, lch.h);
        i += 1;
    }
    candidate
}

/// The resting front edge of a tile or control: accent hue, capped chroma,
/// lightness solved for minimum separation against the card and the ground.
#[allow(
    clippy::too_many_arguments,
    reason = "mirrors the QML signature so the two stay comparable line by line"
)]
fn edge_for(
    accent: Rgb16,
    card: Rgb16,
    chroma_factor: f64,
    chroma_cap: f64,
    card_min_contrast: f64,
    ground: Rgb16,
    ground_min_contrast: f64,
) -> Rgb16 {
    let accent_lch = to_lch(srgb_to_oklab(accent));
    let card_lab = srgb_to_oklab(card);
    let card_lch = to_lch(card_lab);
    let chroma = (accent_lch.c * chroma_factor)
        .min(chroma_cap)
        .max(card_lch.c + 0.01);
    let direction = if card_lab.l > 0.5 { -1.0 } else { 1.0 };
    let mut l = card_lab.l;
    let mut candidate = gamut_fit(l, chroma, accent_lch.h);
    let mut i = 0;
    while i < 80
        && (contrast_ratio(candidate, card) < card_min_contrast
            || contrast_ratio(candidate, ground) < ground_min_contrast)
    {
        l = (l + direction * 0.01).clamp(0.0, 1.0);
        candidate = gamut_fit(l, chroma, accent_lch.h);
        if l <= 0.0 || l >= 1.0 {
            break;
        }
        i += 1;
    }
    candidate
}

fn hue_degrees(radians: f64) -> f64 {
    let mut deg = radians * 180.0 / PI;
    while deg < 0.0 {
        deg += 360.0;
    }
    while deg >= 360.0 {
        deg -= 360.0;
    }
    deg
}

fn hue_delta(a: f64, b: f64) -> f64 {
    let d = (a - b).abs() % 360.0;
    if d > 180.0 {
        360.0 - d
    } else {
        d
    }
}

/// A fixed warm-red hue for the favorite heart and Hidden badge, rotated in
/// 90 degree steps away from the accent when they would collide, lightness
/// fitted for at least 3:1 against the surface.
fn marker_for(accent: Rgb16, surface: Rgb16) -> Rgb16 {
    let base_hue_deg = 29.0;
    let accent_hue_deg = hue_degrees(to_lch(srgb_to_oklab(accent)).h);
    let mut hue_deg = base_hue_deg;
    let mut guard = 0;
    while hue_delta(hue_deg, accent_hue_deg) < 40.0 && guard < 8 {
        hue_deg += 90.0;
        guard += 1;
    }
    let hue = hue_deg * PI / 180.0;
    let chroma = 0.19;
    let surface_l = srgb_to_oklab(surface).l;
    let direction = if surface_l > 0.5 { -1.0 } else { 1.0 };
    let mut l = if surface_l > 0.5 { 0.35 } else { 0.65 };
    let mut candidate = gamut_fit(l, chroma, hue);
    let mut i = 0;
    while i < 60 && contrast_ratio(candidate, surface) < 3.0 {
        l = (l + direction * 0.01).clamp(0.0, 1.0);
        candidate = gamut_fit(l, chroma, hue);
        if l <= 0.0 || l >= 1.0 {
            break;
        }
        i += 1;
    }
    candidate
}

/// Every role for a preset at an intensity. Unknown ids fall back to the
/// default preset; unknown intensities to Subtle.
pub fn palette(id: &str, intensity_name: &str) -> Palette {
    let scale = intensity(intensity_name);
    let source = source(id);
    let primary = hex(source.primary);
    let text = hex(source.text);
    let accent = clamp_accent(hex(source.accent), primary);

    // `up` is true when text is lighter than the background (a dark
    // preset); `ink` is the pole away from the text.
    let up = luma(text) > luma(primary);
    let ink = hex(if up { "#000000" } else { "#ffffff" });
    let light_pole = if up { text } else { primary };
    let dark_pole = if up { primary } else { text };

    // Neutral ladder: step the background toward the text, then give each
    // rung a slight accent cast.
    let panel = mix(mix(primary, text, 0.05), accent, 0.04);
    let card = mix(mix(primary, text, 0.08), accent, 0.05);
    let subtle = mix(mix(primary, text, 0.14), accent, 0.03);
    let mid = mix(mix(primary, text, 0.32), accent, 0.05);
    let variant = mix(mix(primary, text, 0.58), accent, 0.14);

    let selection_fill = selection_fill_for(accent, scale.selection_chroma);
    let on_accent = on_accent_for(selection_fill, primary, text);
    let on_accent_muted = on_accent_muted_for(on_accent, accent);
    let marker = marker_for(accent, card);

    let accent_lch = to_lch(srgb_to_oklab(accent));
    let focus_shadow_l = (accent_lch.l - 0.22).max(0.32);
    let focus_primary_l = (accent_lch.l + 0.16).min(0.90);

    Palette {
        bg_deep: primary,
        bg_panel: panel,
        bg_bar: mix(primary, ink, 0.35),
        surface_card: card,
        tile_edge: edge_for(
            accent,
            card,
            (0.5 * scale.edge_chroma).min(1.0),
            0.05 * scale.edge_cap,
            1.5 * scale.edge_contrast,
            primary,
            1.8 * scale.edge_contrast,
        ),
        control_edge: edge_for(
            accent,
            card,
            (0.6 * scale.edge_chroma).min(1.0),
            0.06 * scale.edge_cap,
            1.65 * scale.edge_contrast,
            panel,
            2.0 * scale.edge_contrast,
        ),
        scrim: "#cc000000",
        border_subtle: subtle,
        border_mid: mid,
        text_primary: text,
        text_label: mix(primary, text, 0.62),
        text_variant: variant,
        accent,
        selection_fill,
        on_accent,
        on_accent_muted,
        logo_shadow: grey_at((accent_lch.l - 0.28).max(0.26)),
        logo_secondary: grey_at((accent_lch.l - 0.16).max(0.10)),
        logo_primary: grey_at(accent_lch.l.min(0.74)),
        logo_focus_shadow: gamut_fit(focus_shadow_l, accent_lch.c, accent_lch.h),
        logo_focus_secondary: accent,
        logo_focus_primary: gamut_fit(focus_primary_l, accent_lch.c * 0.72, accent_lch.h),
        marker,
        marker_outline: mix(
            if luma(marker) > 0.5 {
                dark_pole
            } else {
                light_pole
            },
            marker,
            0.12,
        ),
        error: hex(if up { "#ff4f91" } else { "#c2185b" }),
        qr_light: gamut_fit(0.965, accent_lch.c.min(0.022), accent_lch.h),
        qr_dark: gamut_fit(accent_lch.l.min(0.45), accent_lch.c, accent_lch.h),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_round_trips_through_qt_quantization() {
        for value in ["#000000", "#ffffff", "#050608", "#168bff", "#7f7f7f"] {
            assert_eq!(
                Rgb16::from_hex(value).map(Rgb16::hex).as_deref(),
                Some(value)
            );
        }
    }

    #[test]
    fn unknown_ids_fall_back_to_the_default() {
        assert_eq!(effective_id("nope"), DEFAULT_ID);
        assert_eq!(palette("nope", "subtle"), palette(DEFAULT_ID, "subtle"));
        assert_eq!(palette("nes", "nope"), palette("nes", "subtle"));
    }

    #[test]
    fn catalog_order_is_the_picker_order() {
        let ids: Vec<_> = ids().collect();
        assert_eq!(ids.len(), 19);
        assert_eq!(ids[0], "zaparoo-dark");
        assert_eq!(ids[1], "zaparoo-light");
        assert_eq!(ids[18], "solarized-light");
    }
}
