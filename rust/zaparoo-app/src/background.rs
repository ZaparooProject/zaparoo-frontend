// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

//! The screen background: a user image, or the focused game's art, made
//! once into an opaque bitmap the size of the output so the renderer only
//! ever copies it 1:1.
//!
//! Preparing runs in two parts. `prepare_base` crops the source to fill the
//! output, blurs it at a reduced working size and scales it back up: the
//! slow part, which depends only on the image, the output size and the
//! blur. `compose` then dims it toward the scheme's ground, darkens the
//! help bar band and the glass list card, and quantizes it for an RGB565
//! presenter: one pass, rerun when the theme, the dimming or the screen's
//! layout changes.
//!
//! The `MiSTer`'s Cortex-A9 has no integer divide instruction, so nothing
//! here divides per pixel: averages multiply by a fixed-point reciprocal
//! and dithering indexes by bit masks.

/// What the "Game background" setting can name: off, or one of Core's
/// image types. A game shows art of exactly this type or nothing.
pub const GAME_BACKGROUND_TYPES: &[&str] = &["off", "screenshot", "fanart", "titleshot", "boxart"];

/// The browse list card over a background image.
pub const LIST_STYLES: &[&str] = &["glass", "solid"];

/// How long focus rests on an item before its art becomes the background.
pub const REST_MS: u64 = 300;

/// Blur slider: 0 to 100 percent in steps of 10.
pub const BLUR_STEP: i32 = 10;
pub const BLUR_MAX: i32 = 100;
pub const DEFAULT_BLUR: i32 = 50;

/// Dimming slider, in steps relative to the readable level. The lightest
/// step is no dimming at all, whatever the image needs.
pub const DIM_LIGHTEST: i32 = -6;
pub const DIM_DARKEST: i32 = 3;
const DIM_STEP: f64 = 0.10;
const DIM_MAX: f64 = 0.95;

/// Contrast the readable level holds for primary text: WCAG AA. Secondary
/// labels are not held to a floor of their own: doing so darkened most art
/// past recognition, and blur already removes the busy detail that hurts
/// small text most.
pub const TEXT_CONTRAST: f64 = 4.5;

/// How much of what the dim leaves the help bar band and the glass card
/// take away on top of it.
const BAR_EXTRA: f64 = 0.5;
const CARD_EXTRA: f64 = 0.65;

/// Blur strength at 100%: a Gaussian sigma of this share of the output
/// height.
const SIGMA_SHARE: f64 = 0.025;

/// Samples across the working image's width for the readability measure.
/// The blurred image is what text sits on, so it is what is measured.
const SAMPLES_ACROSS: u32 = 64;

/// The image type a game shows for a "Game background" setting, or none
/// when it is off or unknown.
pub fn game_type(setting: &str) -> Option<&'static str> {
    GAME_BACKGROUND_TYPES
        .iter()
        .copied()
        .find(|t| *t == setting)
        .filter(|t| *t != "off")
}

/// A Blur slider's track: one cell a step, and how many are lit.
pub fn blur_cells(value: i32) -> (i32, i32) {
    (BLUR_MAX / BLUR_STEP, value.clamp(0, BLUR_MAX) / BLUR_STEP)
}

/// A Dimming slider's track: one cell a step past no dimming at all, so
/// the lightest stop lights none and the darkest lights every cell.
pub fn dim_cells(value: i32) -> (i32, i32) {
    (
        DIM_DARKEST - DIM_LIGHTEST,
        value.clamp(DIM_LIGHTEST, DIM_DARKEST) - DIM_LIGHTEST,
    )
}

/// Whether the screen's main card is glass for a "Card style" setting.
pub fn glass(setting: &str) -> bool {
    setting != "solid"
}

/// Step a slider value: blur by 10% within 0..=100, dim by one step within
/// its range.
pub fn step_blur(value: i32, direction: i32) -> i32 {
    let snapped = (value / BLUR_STEP) * BLUR_STEP;
    (snapped + direction.signum() * BLUR_STEP).clamp(0, BLUR_MAX)
}

pub fn step_dim(value: i32, direction: i32) -> i32 {
    (value + direction.signum()).clamp(DIM_LIGHTEST, DIM_DARKEST)
}

/// The dim a slider step applies over an image whose readable level is
/// `readable`, as a share toward the ground.
pub fn dim_for(readable: f64, step: i32) -> f64 {
    if step <= DIM_LIGHTEST {
        return 0.0;
    }
    (readable + f64::from(step) * DIM_STEP).clamp(0.0, DIM_MAX)
}

/// A rectangle in output pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

impl Rect {
    fn rows(&self) -> std::ops::Range<u32> {
        self.y..self.y.saturating_add(self.height)
    }

    fn columns(&self, width: u32) -> (usize, usize) {
        let start = self.x.min(width) as usize;
        let end = self.x.saturating_add(self.width).min(width) as usize;
        (start, end)
    }
}

/// The parts of the screen that sit darker than the rest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Regions {
    /// The help bar band, full width, when the bar shows.
    pub help_bar: Option<Rect>,
    /// The screen's main card, when it is glass on the current screen.
    pub card: Option<Rect>,
    /// The card's corner radius, so the darker area follows its outline.
    pub card_radius: u32,
}

/// How far the card's darker area starts in from each side on `row`
/// (relative to the card's top), so its corners follow the card's
/// rounded outline. Square rows inset nothing. The `MiSTer` presenter
/// cuts the card out of the card-darkness bitmap by the same rule.
pub fn corner_inset(rect: &Rect, radius: u32, row: u32) -> u32 {
    let radius = radius.min(rect.width / 2).min(rect.height / 2);
    let from_edge = row.min(rect.height.saturating_sub(1).saturating_sub(row));
    if from_edge >= radius {
        return 0;
    }
    let r = f64::from(radius);
    // The distance from the corner circle's center to this row's middle.
    let dy = r - f64::from(from_edge) - 0.5;
    (r - (r * r - dy * dy).max(0.0).sqrt()).round() as u32
}

/// The pixel format the composed bitmap is made for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    /// Eight bits a channel, as GPU and 32-bit presenters show it.
    Rgb888,
    /// Ordered-dithered to RGB565 codes, written back as RGB8 values the
    /// software renderer's truncating conversion maps onto those codes
    /// exactly.
    Rgb565,
}

/// The slow half of a background: cropped, blurred and scaled to the
/// output, not yet dimmed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Base {
    pub width: u32,
    pub height: u32,
    /// Opaque RGB8, row-major.
    pub rgb: Vec<u8>,
    /// A coarse sample of the blurred image, for the readable level.
    pub samples: Vec<[u8; 3]>,
}

impl Base {
    pub fn bytes(&self) -> usize {
        self.rgb.len() + self.samples.len() * 3
    }
}

/// A source cropped and scaled to fill the output, before any blur. Kept
/// for the background on show, so a Blur change reruns only the blur.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fitted {
    pub width: u32,
    pub height: u32,
    pub rgb: Vec<u8>,
}

/// The source region that fills an `out_w x out_h` output at the source's
/// own aspect, centered.
pub fn crop_to_fill(src_w: u32, src_h: u32, out_w: u32, out_h: u32) -> Rect {
    let (src_w, src_h) = (src_w.max(1), src_h.max(1));
    let (out_w, out_h) = (u64::from(out_w.max(1)), u64::from(out_h.max(1)));
    let (w, h) = if u64::from(src_w) * out_h > u64::from(src_h) * out_w {
        (((u64::from(src_h) * out_w) / out_h) as u32, src_h)
    } else {
        (src_w, ((u64::from(src_w) * out_h) / out_w) as u32)
    };
    let (w, h) = (w.clamp(1, src_w), h.clamp(1, src_h));
    Rect {
        x: (src_w - w) / 2,
        y: (src_h - h) / 2,
        width: w,
        height: h,
    }
}

/// Blur sigma in output pixels for a Blur percentage.
fn sigma_out(blur: i32, out_h: u32) -> f64 {
    f64::from(out_h) * SIGMA_SHARE * f64::from(blur.clamp(0, BLUR_MAX)) / 100.0
}

/// The working size divisor: the blur runs at a reduced size, as reduced
/// as the blur hides. No blur works at full size.
pub fn work_div(blur: i32, out_h: u32) -> u32 {
    let sigma = sigma_out(blur, out_h);
    if sigma <= 0.0 {
        return 1;
    }
    ((sigma / 3.0).round() as u32).clamp(1, 4)
}

/// Crop and scale an RGB8 source to fill an `out_w x out_h` output.
pub fn fit(src: &[u8], src_w: u32, src_h: u32, out_w: u32, out_h: u32) -> Fitted {
    let (out_w, out_h) = (out_w.max(1), out_h.max(1));
    let crop = crop_to_fill(src_w, src_h, out_w, out_h);
    Fitted {
        width: out_w,
        height: out_h,
        rgb: resample(src, src_w as usize, crop, out_w as usize, out_h as usize),
    }
}

/// Blur a fitted image at the reduced working size the blur allows and
/// scale it back to the output: the base.
pub fn prepare_base(fitted: &Fitted, blur: i32) -> Base {
    let (ow, oh) = (fitted.width as usize, fitted.height as usize);
    let div = work_div(blur, fitted.height) as usize;
    let (w, h) = ((ow / div).max(1), (oh / div).max(1));
    let whole = |w: usize, h: usize| Rect {
        x: 0,
        y: 0,
        width: w as u32,
        height: h as u32,
    };
    let mut work = if div == 1 {
        fitted.rgb.clone()
    } else {
        resample(&fitted.rgb, ow, whole(ow, oh), w, h)
    };
    let sigma = sigma_out(blur, fitted.height) / div as f64;
    if sigma > 0.0 {
        box_blur3(&mut work, w, h, sigma);
    }
    let samples = sample_pixels(&work, w, h);
    let rgb = if div == 1 {
        work
    } else {
        resample(&work, w, whole(w, h), ow, oh)
    };
    Base {
        width: fitted.width,
        height: fitted.height,
        rgb,
        samples,
    }
}

/// The dim, as a share toward `ground`, that holds `text` at
/// `TEXT_CONTRAST` against the worst 1% of the samples.
pub fn readable_dim(samples: &[[u8; 3]], text: [u8; 3], ground: [u8; 3]) -> f64 {
    let Some(px) = worst_pixel(samples, text, ground) else {
        return 0.0;
    };
    (0..=256)
        .map(|i| f64::from(i) / 256.0)
        .find(|&dim| contrast(text, dimmed(px, ground, dim)) >= TEXT_CONTRAST)
        .unwrap_or(1.0)
}

/// The whole image at the glass card's darkness: what shows through a
/// glass card wherever it sits, so a screen's card can move or change
/// without preparing the background again.
pub fn compose_card(base: &Base, ground: [u8; 3], dim: f64, format: Format) -> Vec<u8> {
    let regions = Regions {
        help_bar: None,
        card: Some(Rect {
            x: 0,
            y: 0,
            width: base.width,
            height: base.height,
        }),
        card_radius: 0,
    };
    compose(base, ground, dim, &regions, format)
}

/// Dim the base toward `ground`, darken the help bar and the glass card
/// further, and write the bitmap in `format`.
pub fn compose(
    base: &Base,
    ground: [u8; 3],
    dim: f64,
    regions: &Regions,
    format: Format,
) -> Vec<u8> {
    let (width, height) = (base.width as usize, base.height as usize);
    let factor = |dim: f64| ((1.0 - dim.clamp(0.0, 1.0)) * 256.0).round() as u32;
    let plain = factor(dim);
    let bar = factor(dim + (1.0 - dim) * BAR_EXTRA);
    let card = factor(dim + (1.0 - dim) * CARD_EXTRA);
    let stride = width * 3;
    let mut out = vec![0u8; base.rgb.len()];
    let mut keep_row = vec![plain; width];
    for row in 0..height {
        let row_y = row as u32;
        keep_row.fill(plain);
        if let Some(rect) = regions.card.filter(|r| r.rows().contains(&row_y)) {
            let inset = corner_inset(&rect, regions.card_radius, row_y - rect.y);
            let inner = Rect {
                x: rect.x + inset,
                width: rect.width.saturating_sub(2 * inset),
                ..rect
            };
            let (start, end) = inner.columns(base.width);
            keep_row[start..end].fill(card);
        }
        if regions.help_bar.is_some_and(|r| r.rows().contains(&row_y)) {
            keep_row.fill(bar);
        }
        let source = &base.rgb[row * stride..(row + 1) * stride];
        let target = &mut out[row * stride..(row + 1) * stride];
        let bayer = &BAYER4[row & 3];
        for (column, ((pixel, input), &keep)) in target
            .chunks_exact_mut(3)
            .zip(source.chunks_exact(3))
            .zip(keep_row.iter())
            .enumerate()
        {
            let red = blend(input[0], ground[0], keep);
            let green = blend(input[1], ground[1], keep);
            let blue = blend(input[2], ground[2], keep);
            match format {
                Format::Rgb888 => {
                    pixel[0] = red as u8;
                    pixel[1] = green as u8;
                    pixel[2] = blue as u8;
                }
                Format::Rgb565 => {
                    let threshold = u32::from(bayer[column & 3]);
                    pixel[0] = quantize5(red, threshold);
                    pixel[1] = quantize6(green, threshold);
                    pixel[2] = quantize5(blue, threshold);
                }
            }
        }
    }
    out
}

/// `value` kept `keep` parts in 256 of the way from `ground`.
fn blend(value: u8, ground: u8, keep: u32) -> u32 {
    (u32::from(value) * keep + u32::from(ground) * (256 - keep) + 128) >> 8
}

const BAYER4: [[u8; 4]; 4] = [[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]];

/// Five bits with a threshold in [0, 8), expanded so truncation by `>> 3`
/// returns the code.
fn quantize5(value: u32, t: u32) -> u8 {
    let code = ((value + t / 2).min(255) >> 3) as u8;
    (code << 3) | (code >> 2)
}

/// Six bits with a threshold in [0, 4), expanded so truncation by `>> 2`
/// returns the code.
fn quantize6(value: u32, t: u32) -> u8 {
    let code = ((value + t / 4).min(255) >> 2) as u8;
    (code << 2) | (code >> 4)
}

/// Crop `crop` of an RGB8 image `stride_w` wide to `ow x oh`: area average
/// when shrinking on both axes, bilinear otherwise.
fn resample(src: &[u8], stride_w: usize, crop: Rect, ow: usize, oh: usize) -> Vec<u8> {
    let (cx, cy, cw, ch) = (
        crop.x as usize,
        crop.y as usize,
        crop.width.max(1) as usize,
        crop.height.max(1) as usize,
    );
    if ow > cw || oh > ch {
        return bilinear(src, stride_w, (cx, cy, cw, ch), ow, oh);
    }
    let bin: Vec<usize> = (0..cw).map(|x| x * ow / cw).collect();
    let row_edge: Vec<usize> = (0..=oh).map(|y| y * ch / oh).collect();
    let mut col_count = vec![0u32; ow];
    for &b in &bin {
        col_count[b] += 1;
    }
    let mut out = vec![0u8; ow * oh * 3];
    let mut acc = vec![0u32; ow * 3];
    for oy in 0..oh {
        acc.fill(0);
        let (y0, y1) = (row_edge[oy], row_edge[oy + 1].max(row_edge[oy] + 1).min(ch));
        for y in y0..y1 {
            let start = ((cy + y) * stride_w + cx) * 3;
            let row = &src[start..start + cw * 3];
            for (px, &b) in row.chunks_exact(3).zip(&bin) {
                let b = b * 3;
                acc[b] += u32::from(px[0]);
                acc[b + 1] += u32::from(px[1]);
                acc[b + 2] += u32::from(px[2]);
            }
        }
        let rows = (y1 - y0).max(1) as u32;
        let dst = &mut out[oy * ow * 3..(oy + 1) * ow * 3];
        for (ox, px) in dst.chunks_exact_mut(3).enumerate() {
            let inv = (1u64 << 24) / u64::from((col_count[ox] * rows).max(1));
            for c in 0..3 {
                px[c] = ((u64::from(acc[ox * 3 + c]) * inv + (1 << 23)) >> 24).min(255) as u8;
            }
        }
    }
    out
}

fn bilinear(
    src: &[u8],
    stride_w: usize,
    (cx, cy, sw, sh): (usize, usize, usize, usize),
    ow: usize,
    oh: usize,
) -> Vec<u8> {
    // Source index pairs and an 8-bit weight per output column and row.
    let axis = |out: usize, inp: usize, offset: usize| -> Vec<(usize, usize, u32)> {
        (0..out)
            .map(|o| {
                let pos = ((o as f64 + 0.5) * inp as f64 / out as f64 - 0.5).max(0.0);
                let i0 = (pos.floor() as usize).min(inp - 1);
                let i1 = (i0 + 1).min(inp - 1);
                let f = ((pos - i0 as f64) * 256.0).round() as u32;
                (i0 + offset, i1 + offset, f.min(256))
            })
            .collect()
    };
    let xs = axis(ow, sw, cx);
    let ys = axis(oh, sh, cy);
    let mut out = vec![0u8; ow * oh * 3];
    // Each output row blends two source rows expanded to the output width;
    // consecutive output rows mostly share them.
    let mut row0 = vec![0u32; ow * 3];
    let mut row1 = vec![0u32; ow * 3];
    let mut cached = (usize::MAX, usize::MAX);
    let expand = |row: usize, dst: &mut [u32]| {
        let base = row * stride_w * 3;
        for (d, &(x0, x1, f)) in dst.chunks_exact_mut(3).zip(&xs) {
            for c in 0..3 {
                let a = u32::from(src[base + x0 * 3 + c]);
                let b = u32::from(src[base + x1 * 3 + c]);
                d[c] = a * (256 - f) + b * f;
            }
        }
    };
    for (dst, &(y0, y1, f)) in out.chunks_exact_mut(ow * 3).zip(&ys) {
        if cached != (y0, y1) {
            if cached.1 == y0 {
                std::mem::swap(&mut row0, &mut row1);
            } else {
                expand(y0, &mut row0);
            }
            expand(y1, &mut row1);
            cached = (y0, y1);
        }
        for ((d, &a), &b) in dst.iter_mut().zip(&row0).zip(&row1) {
            *d = ((a * (256 - f) + b * f + 32768) >> 16) as u8;
        }
    }
    out
}

/// Three equal boxes approximating a Gaussian of `sigma`.
fn box_radius(sigma: f64) -> usize {
    let ideal = (4.0 * sigma * sigma + 1.0).sqrt();
    (((ideal - 1.0) / 2.0).round() as usize).max(1)
}

fn box_blur3(rgb: &mut [u8], w: usize, h: usize, sigma: f64) {
    let r = box_radius(sigma);
    let mut tmp = vec![0u8; rgb.len()];
    for _ in 0..3 {
        box_h(rgb, &mut tmp, w, h, r);
        box_v(&tmp, rgb, w, h, r);
    }
}

/// A 16.16 reciprocal of a box's width.
fn box_inverse(r: usize) -> u32 {
    let width = 2 * r as u32 + 1;
    (65536 + width / 2) / width
}

fn box_h(src: &[u8], dst: &mut [u8], w: usize, h: usize, r: usize) {
    let inv = box_inverse(r);
    let last = w - 1;
    for y in 0..h {
        let row = &src[y * w * 3..(y + 1) * w * 3];
        let out = &mut dst[y * w * 3..(y + 1) * w * 3];
        for c in 0..3 {
            let px = |x: usize| u32::from(row[x.min(last) * 3 + c]);
            let mut sum = px(0) * (r as u32 + 1);
            for i in 1..=r {
                sum += px(i);
            }
            for x in 0..w {
                out[x * 3 + c] = ((sum * inv + 32768) >> 16).min(255) as u8;
                sum = sum + px(x + r + 1) - px(x.saturating_sub(r));
            }
        }
    }
}

fn box_v(src: &[u8], dst: &mut [u8], w: usize, h: usize, r: usize) {
    let inv = box_inverse(r);
    let last = h - 1;
    let stride = w * 3;
    let mut sums: Vec<u32> = (0..stride)
        .map(|i| {
            let mut sum = u32::from(src[i]) * (r as u32 + 1);
            for k in 1..=r {
                sum += u32::from(src[k.min(last) * stride + i]);
            }
            sum
        })
        .collect();
    for y in 0..h {
        let add = (y + r + 1).min(last) * stride;
        let sub = y.saturating_sub(r) * stride;
        let out = &mut dst[y * stride..(y + 1) * stride];
        for (i, (o, sum)) in out.iter_mut().zip(sums.iter_mut()).enumerate() {
            *o = ((*sum * inv + 32768) >> 16).min(255) as u8;
            *sum = *sum + u32::from(src[add + i]) - u32::from(src[sub + i]);
        }
    }
}

fn sample_pixels(rgb: &[u8], width: usize, height: usize) -> Vec<[u8; 3]> {
    let step = (width / SAMPLES_ACROSS as usize).max(1);
    let mut out = Vec::new();
    for row in (step / 2..height).step_by(step) {
        for column in (step / 2..width).step_by(step) {
            let at = (row * width + column) * 3;
            out.push([rgb[at], rgb[at + 1], rgb[at + 2]]);
        }
    }
    out
}

fn srgb_to_linear(c: u8) -> f64 {
    let c = f64::from(c) / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn luminance(c: [u8; 3]) -> f64 {
    0.2126 * srgb_to_linear(c[0]) + 0.7152 * srgb_to_linear(c[1]) + 0.0722 * srgb_to_linear(c[2])
}

/// WCAG contrast ratio between two colors.
pub fn contrast(a: [u8; 3], b: [u8; 3]) -> f64 {
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

fn dimmed(px: [u8; 3], ground: [u8; 3], dim: f64) -> [u8; 3] {
    [0, 1, 2].map(|c| (f64::from(px[c]) * (1.0 - dim) + f64::from(ground[c]) * dim).round() as u8)
}

/// The sample at the worst 1% for `text`: the brightest for light text,
/// the darkest for dark text.
fn worst_pixel(samples: &[[u8; 3]], text: [u8; 3], ground: [u8; 3]) -> Option<[u8; 3]> {
    if samples.is_empty() {
        return None;
    }
    let light_text = luminance(text) > luminance(ground);
    let mut ranked: Vec<(f64, [u8; 3])> = samples.iter().map(|&px| (luminance(px), px)).collect();
    let k = ranked.len() / 100;
    let index = if light_text { ranked.len() - 1 - k } else { k };
    ranked.select_nth_unstable_by(index, |a, b| a.0.total_cmp(&b.0));
    Some(ranked[index].1)
}

/// Worst-1% contrast of `text` over the samples once dimmed, for tests and
/// diagnostics.
pub fn worst_contrast(samples: &[[u8; 3]], text: [u8; 3], ground: [u8; 3], dim: f64) -> f64 {
    worst_pixel(samples, text, ground).map_or_else(
        || contrast(text, ground),
        |px| contrast(text, dimmed(px, ground, dim)),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEXT: [u8; 3] = [0xf7, 0xf7, 0xf5];
    const GROUND: [u8; 3] = [0x05, 0x06, 0x08];

    fn solid(w: u32, h: u32, c: [u8; 3]) -> Vec<u8> {
        (0..w * h).flat_map(|_| c).collect()
    }

    #[test]
    fn rgb565_values_survive_the_renderers_truncation() {
        for value in 0..=255u32 {
            for t in 0..16 {
                let r = quantize5(value, t);
                let g = quantize6(value, t);
                // Slint keeps the top five and six bits; expanding them
                // again must give back the same byte.
                let r5 = r >> 3;
                let g6 = g >> 2;
                assert_eq!((r5 << 3) | (r5 >> 2), r);
                assert_eq!((g6 << 2) | (g6 >> 4), g);
                // Threshold and expansion each add at most seven.
                assert!(i32::from(r).abs_diff(value as i32) <= 14);
            }
        }
    }

    #[test]
    fn white_needs_the_dim_that_holds_primary_text() {
        let samples = vec![[255, 255, 255]; 100];
        let dim = readable_dim(&samples, TEXT, GROUND);
        assert!(worst_contrast(&samples, TEXT, GROUND, dim) >= TEXT_CONTRAST);
        // 4.5:1 for near-white text over white needs a little over half.
        assert!((0.5..0.6).contains(&dim), "{dim}");
    }

    #[test]
    fn a_dark_image_needs_no_dim() {
        let samples = vec![[10, 12, 14]; 100];
        assert!(readable_dim(&samples, TEXT, GROUND).abs() < f64::EPSILON);
    }

    #[test]
    fn a_light_scheme_dims_toward_its_light_ground() {
        let samples = vec![[0, 0, 0]; 100];
        let (text, ground) = ([0x10, 0x10, 0x10], [0xf4, 0xf4, 0xf0]);
        let dim = readable_dim(&samples, text, ground);
        assert!(dim > 0.4, "{dim}");
        assert!(worst_contrast(&samples, text, ground, dim) >= TEXT_CONTRAST);
    }

    #[test]
    fn dim_steps_are_relative_and_the_lightest_is_none() {
        let near = |a: f64, b: f64| (a - b).abs() < 1e-9;
        assert!(near(dim_for(0.6, 0), 0.6));
        assert!(near(dim_for(0.6, -2), 0.4));
        assert!(near(dim_for(0.6, 3), 0.9));
        assert!(near(dim_for(0.6, DIM_LIGHTEST), 0.0));
        assert!(near(dim_for(0.2, -5), 0.0));
        assert!(near(dim_for(0.9, DIM_DARKEST), DIM_MAX));
        assert_eq!(step_dim(DIM_DARKEST, 1), DIM_DARKEST);
        assert_eq!(step_dim(0, -1), -1);
        assert_eq!(step_blur(100, 1), 100);
        assert_eq!(step_blur(100, -1), 90);
        assert_eq!(step_blur(35, 1), 40);
        assert_eq!(step_blur(0, -1), 0);
    }

    #[test]
    fn crop_fills_the_output_aspect_centered() {
        assert_eq!(
            crop_to_fill(256, 224, 960, 540),
            Rect {
                x: 0,
                y: 40,
                width: 256,
                height: 144
            }
        );
        assert_eq!(
            crop_to_fill(4000, 1000, 960, 540),
            Rect {
                x: 1111,
                y: 0,
                width: 1777,
                height: 1000
            }
        );
    }

    #[test]
    fn a_base_is_the_output_size_and_a_flat_image_stays_flat() {
        let src = solid(320, 240, [200, 100, 50]);
        for blur in [0, 10, 50, 100] {
            let base = prepare_base(&fit(&src, 320, 240, 96, 54), blur);
            assert_eq!((base.width, base.height), (96, 54));
            assert_eq!(base.rgb.len(), 96 * 54 * 3);
            assert!(
                base.rgb.chunks_exact(3).all(|p| p == [200, 100, 50]),
                "{blur}"
            );
        }
    }

    #[test]
    fn blur_spreads_an_edge() {
        let mut src = solid(200, 100, [0, 0, 0]);
        for y in 0..100 {
            for x in 100..200 {
                src[(y * 200 + x) * 3..(y * 200 + x) * 3 + 3].copy_from_slice(&[255, 255, 255]);
            }
        }
        let fitted = fit(&src, 200, 100, 200, 100);
        let sharp = prepare_base(&fitted, 0);
        let soft = prepare_base(&fitted, 100);
        let at = |b: &Base, x: usize| b.rgb[(50 * 200 + x) * 3];
        assert_eq!(at(&sharp, 95), 0);
        assert!(at(&soft, 98) > 20, "{}", at(&soft, 98));
        assert!(at(&soft, 102) < 235, "{}", at(&soft, 102));
    }

    #[test]
    fn compose_darkens_the_card_and_bar_more_than_the_rest() {
        let base = Base {
            width: 10,
            height: 10,
            rgb: solid(10, 10, [200, 200, 200]),
            samples: Vec::new(),
        };
        let regions = Regions {
            help_bar: Some(Rect {
                x: 0,
                y: 8,
                width: 10,
                height: 2,
            }),
            card: Some(Rect {
                x: 2,
                y: 2,
                width: 6,
                height: 4,
            }),
            card_radius: 0,
        };
        let out = compose(&base, [0, 0, 0], 0.5, &regions, Format::Rgb888);
        let at = |x: usize, y: usize| out[(y * 10 + x) * 3];
        let (plain, card, bar) = (at(0, 0), at(4, 3), at(4, 9));
        assert_eq!(plain, 100);
        assert!(card < plain && bar < plain, "{plain} {card} {bar}");
        assert_eq!(at(1, 3), plain);
        let none = compose(&base, [0, 0, 0], 0.0, &Regions::default(), Format::Rgb888);
        assert!(none.iter().all(|&v| v == 200));
    }

    #[test]
    fn slider_cells_light_one_cell_a_step() {
        assert_eq!(blur_cells(0), (10, 0));
        assert_eq!(blur_cells(50), (10, 5));
        assert_eq!(blur_cells(100), (10, 10));
        assert_eq!(dim_cells(DIM_LIGHTEST), (9, 0));
        assert_eq!(dim_cells(0), (9, 6));
        assert_eq!(dim_cells(DIM_DARKEST), (9, 9));
    }

    #[test]
    fn a_rounded_card_leaves_its_corners_plain() {
        let base = Base {
            width: 20,
            height: 20,
            rgb: solid(20, 20, [200, 200, 200]),
            samples: Vec::new(),
        };
        let regions = Regions {
            help_bar: None,
            card: Some(Rect {
                x: 2,
                y: 2,
                width: 16,
                height: 16,
            }),
            card_radius: 5,
        };
        let out = compose(&base, [0, 0, 0], 0.5, &regions, Format::Rgb888);
        let at = |x: usize, y: usize| out[(y * 20 + x) * 3];
        let plain = at(0, 0);
        // The very corner pixels stay outside the outline, on all four.
        for (x, y) in [(2, 2), (17, 2), (2, 17), (17, 17)] {
            assert_eq!(at(x, y), plain, "corner {x},{y}");
        }
        // The middle of every edge and the inside are the card.
        for (x, y) in [(10, 2), (2, 10), (17, 10), (10, 17), (10, 10), (4, 4)] {
            assert!(at(x, y) < plain, "edge {x},{y}");
        }
        // Past the radius the rows run edge to edge.
        assert!(at(2, 7) < plain);
    }

    #[test]
    fn rgb565_compose_writes_only_exact_codes() {
        let base = Base {
            width: 8,
            height: 8,
            rgb: (0..64u32)
                .flat_map(|i| [(i * 4) as u8, (i * 3) as u8, (255 - i) as u8])
                .collect(),
            samples: Vec::new(),
        };
        let out = compose(&base, GROUND, 0.3, &Regions::default(), Format::Rgb565);
        for px in out.chunks_exact(3) {
            assert_eq!(quantize5(u32::from(px[0]) & !7, 0), px[0]);
            assert_eq!(quantize6(u32::from(px[1]) & !3, 0), px[1]);
            assert_eq!(quantize5(u32::from(px[2]) & !7, 0), px[2]);
        }
    }

    #[test]
    fn game_types_and_list_styles() {
        assert_eq!(game_type("off"), None);
        assert_eq!(game_type("screenshot"), Some("screenshot"));
        assert_eq!(game_type("bogus"), None);
        assert!(glass("glass"));
        assert!(glass("bogus"));
        assert!(!glass("solid"));
    }
}
