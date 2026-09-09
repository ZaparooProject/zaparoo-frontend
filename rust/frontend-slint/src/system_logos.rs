// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// Tinted system logo art. The Qt frontend embeds the neutral-grayscale
// SVGs in qrc and tints them through the tinted-svg provider; here the
// SVGs are pre-rasterized to PNG (assets/systems/{id}.png, 160 px tall),
// embedded into the binary by build.rs, and decoded on demand. One static
// binary, as with Qt; nothing is read from disk. Tinting happens in .slint
// via `colorize`, same as the category glyphs. Regional variants are out
// of scope for now (base art only).

use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard, OnceLock, PoisonError};

include!(concat!(env!("OUT_DIR"), "/system_logos_table.rs"));

#[derive(Debug, Clone)]
pub struct LogoPixels {
    pub rgba: std::sync::Arc<Vec<u8>>,
    pub width: u32,
    pub height: u32,
}

static CACHE: OnceLock<Mutex<HashMap<String, Option<LogoPixels>>>> = OnceLock::new();

fn cache() -> MutexGuard<'static, HashMap<String, Option<LogoPixels>>> {
    CACHE
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
}

/// Tint triplets (highlight, midtone, shadow) for the resting and focused
/// logo ramps: the palette's `logo*` and `logoFocus*` roles, pushed by
/// `theme::apply_palette` through `set_tints`. The defaults are
/// zaparoo-dark at Subtle, matching the `Theme` global's own defaults.
type Tints = [(u8, u8, u8); 3];
static TINTS: OnceLock<Mutex<(Tints, Tints)>> = OnceLock::new();

fn tints() -> MutexGuard<'static, (Tints, Tints)> {
    TINTS
        .get_or_init(|| {
            Mutex::new((
                [(0x8c, 0x8c, 0x8c), (0x5e, 0x5e, 0x5e), (0x3d, 0x3d, 0x3d)],
                [(0x8f, 0xc1, 0xff), (0x16, 0x8b, 0xff), (0x00, 0x4c, 0x94)],
            ))
        })
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
}

/// Installs new ramps (on a scheme change) and drops every tinted variant
/// so the next request re-tints from the base art.
pub fn set_tints(rest: Tints, focus: Tints) {
    *tints() = (rest, focus);
    tint_cache().clear();
}

type TintCacheMap = HashMap<(String, bool), Option<LogoPixels>>;

static TINT_CACHE: OnceLock<Mutex<TintCacheMap>> = OnceLock::new();

fn tint_cache() -> MutexGuard<'static, TintCacheMap> {
    TINT_CACHE
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
}

/// Tinted logo variant for a system, memoized. Port of the Qt
/// tinted-svg provider's `tintImage`: a COLOR-GRADE, not a flat
/// recolor - the darkest source tones map to the shadow tint,
/// midtones to the theme tint, and the brightest stay primary, on a
/// monotonic curve so gradients and antialiasing survive. A flat
/// `colorize` destroys exactly the multi-tone art this preserves.
pub fn tinted_logo_for(system_id: &str, focused: bool) -> Option<LogoPixels> {
    let key = (system_id.to_string(), focused);
    if let Some(hit) = tint_cache().get(&key) {
        return hit.clone();
    }
    let (rest, focus) = *tints();
    let tinted = logo_for(system_id).map(|base| {
        let ramp = if focused { focus } else { rest };
        tint(&base, ramp[0], ramp[1], ramp[2])
    });
    tint_cache().insert(key, tinted.clone());
    tinted
}

fn luma(r: u8, g: u8, b: u8) -> i32 {
    (i32::from(r) * 299 + i32::from(g) * 587 + i32::from(b) * 114 + 500) / 1000
}

fn mix(a: u8, b: u8, amount_b: i32) -> u8 {
    let v = (i32::from(a) * (255 - amount_b) + i32::from(b) * amount_b + 127) / 255;
    v.clamp(0, 255) as u8
}

fn tint(
    base: &LogoPixels,
    highlight: (u8, u8, u8),
    midtone: (u8, u8, u8),
    shadow: (u8, u8, u8),
) -> LogoPixels {
    // Tone range over meaningfully-opaque pixels (alpha > 16).
    let (mut min, mut max, mut visible) = (255i32, 0i32, 0u32);
    for px in base.rgba.chunks_exact(4) {
        if px[3] > 16 {
            let l = luma(px[0], px[1], px[2]);
            min = min.min(l);
            max = max.max(l);
            visible += 1;
        }
    }
    let single_tone = visible == 0 || (max - min) < 16;

    let mut out = Vec::with_capacity(base.rgba.len());
    for px in base.rgba.chunks_exact(4) {
        let alpha = px[3];
        if alpha == 0 {
            out.extend_from_slice(px);
            continue;
        }
        let (r, g, b) = if single_tone {
            highlight
        } else {
            let tone = ((luma(px[0], px[1], px[2]) - min) * 255 / (max - min).max(1)).clamp(0, 255);
            if tone < 128 {
                let amount = tone * 2;
                (
                    mix(shadow.0, midtone.0, amount),
                    mix(shadow.1, midtone.1, amount),
                    mix(shadow.2, midtone.2, amount),
                )
            } else {
                let amount = (tone - 128) * 2;
                (
                    mix(midtone.0, highlight.0, amount),
                    mix(midtone.1, highlight.1, amount),
                    mix(midtone.2, highlight.2, amount),
                )
            }
        };
        out.extend_from_slice(&[r, g, b, alpha]);
    }
    LogoPixels {
        rgba: std::sync::Arc::new(out),
        width: base.width,
        height: base.height,
    }
}

/// Decoded logo pixels for a system id, memoized (including misses).
pub fn logo_for(system_id: &str) -> Option<LogoPixels> {
    if let Some(hit) = cache().get(system_id) {
        return hit.clone();
    }
    let loaded = load(system_id);
    cache().insert(system_id.to_string(), loaded.clone());
    loaded
}

fn load(system_id: &str) -> Option<LogoPixels> {
    let index = EMBEDDED_LOGOS
        .binary_search_by(|(id, _)| (*id).cmp(system_id))
        .ok()?;
    let bytes = EMBEDDED_LOGOS[index].1;
    let decoded = image::load_from_memory(bytes).ok()?.to_rgba8();
    let (width, height) = decoded.dimensions();
    Some(LogoPixels {
        rgba: std::sync::Arc::new(decoded.into_raw()),
        width,
        height,
    })
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        reason = "tests should fail-fast on unexpected errors"
    )]

    use super::*;

    fn px(r: u8, g: u8, b: u8, a: u8) -> [u8; 4] {
        [r, g, b, a]
    }

    #[test]
    fn tint_maps_dark_mid_bright_to_the_triplet() {
        // Black, mid-gray, white opaque pixels: the grade must land on
        // shadow, midtone, and highlight respectively.
        let rgba: Vec<u8> = [
            px(0, 0, 0, 255),
            px(128, 128, 128, 255),
            px(255, 255, 255, 255),
        ]
        .concat();
        let base = LogoPixels {
            rgba: std::sync::Arc::new(rgba),
            width: 3,
            height: 1,
        };
        let out = tint(&base, (200, 200, 200), (100, 100, 100), (20, 20, 20));
        assert_eq!(&out.rgba[0..3], &[20, 20, 20]);
        let mid = out.rgba[4];
        assert!((95..=105).contains(&mid), "midtone landed at {mid}");
        assert_eq!(&out.rgba[8..11], &[200, 200, 200]);
    }

    #[test]
    fn tint_single_tone_goes_full_highlight() {
        let rgba: Vec<u8> = [px(255, 255, 255, 255), px(250, 250, 250, 255)].concat();
        let base = LogoPixels {
            rgba: std::sync::Arc::new(rgba),
            width: 2,
            height: 1,
        };
        let out = tint(&base, (1, 2, 3), (100, 100, 100), (20, 20, 20));
        assert_eq!(&out.rgba[0..3], &[1, 2, 3]);
        assert_eq!(&out.rgba[4..7], &[1, 2, 3]);
    }

    #[test]
    fn tint_preserves_alpha() {
        let rgba: Vec<u8> = [px(255, 255, 255, 77), px(0, 0, 0, 0)].concat();
        let base = LogoPixels {
            rgba: std::sync::Arc::new(rgba),
            width: 2,
            height: 1,
        };
        let out = tint(&base, (9, 9, 9), (5, 5, 5), (1, 1, 1));
        assert_eq!(out.rgba[3], 77);
        assert_eq!(out.rgba[7], 0);
    }

    #[test]
    fn traversal_tokens_are_refused() {
        assert!(logo_for("../etc/passwd").is_none());
        assert!(logo_for("a/b").is_none());
        assert!(logo_for("").is_none());
    }

    #[test]
    fn known_system_is_embedded() {
        assert!(
            EMBEDDED_LOGOS.len() > 100,
            "the logo table is generated by build.rs"
        );
        let logo = logo_for("SNES");
        assert!(
            logo.is_some(),
            "SNES.png must be embedded from assets/systems"
        );
        let logo = logo.expect("checked above");
        assert!(logo.width > 0 && logo.height > 0);
    }

    #[test]
    fn misses_are_memoized_without_panic() {
        assert!(logo_for("NotARealSystem").is_none());
        assert!(logo_for("NotARealSystem").is_none());
    }
}
