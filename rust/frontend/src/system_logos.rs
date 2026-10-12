// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

//! Embedded logo preparation. UI lookups never decode or copy RGBA pixels.
//! One worker serves a bounded page window into a byte-capped LRU; no original
//! or secondary Slint-image cache survives alongside the prepared buffers.

use slint::ComponentHandle;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use tokio::sync::Notify;
use zaparoo_app::logo_cache::{Bounds, Cache, CACHE_BYTES};

include!(concat!(env!("OUT_DIR"), "/system_logos_table.rs"));
include!(concat!(env!("OUT_DIR"), "/system_half_logos_table.rs"));
include!(concat!(env!("OUT_DIR"), "/system_color_logos_table.rs"));

type Tints = [(u8, u8, u8); 3];
const DEFAULT_RAMPS: (Tints, Tints) = (
    [(0x8c, 0x8c, 0x8c), (0x5e, 0x5e, 0x5e), (0x3d, 0x3d, 0x3d)],
    [(0x8f, 0xc1, 0xff), (0x16, 0x8b, 0xff), (0x00, 0x4c, 0x94)],
);
type Pixels = slint::SharedPixelBuffer<slint::Rgba8Pixel>;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Asset {
    Gray(usize),
    Color(usize),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Key {
    asset: Asset,
    tinted: bool,
    bounds: Bounds,
    generation: u64,
}

#[derive(Debug, Clone)]
pub struct Prepared {
    rest: Pixels,
    /// Original-color rest/focus share one allocation and one image identity.
    focus: Option<Pixels>,
}

impl Prepared {
    pub fn images(&self) -> (slint::Image, slint::Image) {
        let rest = slint::Image::from_rgba8(self.rest.clone());
        let focus = self
            .focus
            .as_ref()
            .map_or_else(|| rest.clone(), |p| slint::Image::from_rgba8(p.clone()));
        (rest, focus)
    }

    fn bytes(&self) -> usize {
        self.rest.as_bytes().len() + self.focus.as_ref().map_or(0, |p| p.as_bytes().len())
    }
}

#[derive(Debug)]
struct Inner {
    cache: Cache<Key, Option<Prepared>>,
    ramps: (Tints, Tints),
    generation: u64,
    paused: bool,
    visible: std::collections::HashSet<Key>,
    /// Every logo the Systems grid can show, at its current style and tile
    /// size: prepared in the background behind whatever a screen asks for,
    /// so a page turned to later has its art ready instead of popping in.
    /// Kept across cache resets and re-keyed to the generation in force.
    warm: Vec<Key>,
}

#[derive(Debug)]
pub struct Logos {
    inner: Mutex<Inner>,
    wake: Notify,
    delivery_pending: AtomicBool,
}

impl Logos {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            inner: Mutex::new(Inner {
                cache: Cache::new(CACHE_BYTES),
                ramps: DEFAULT_RAMPS,
                generation: 0,
                paused: false,
                warm: Vec::new(),
                visible: std::collections::HashSet::new(),
            }),
            wake: Notify::new(),
            delivery_pending: AtomicBool::new(false),
        })
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    pub fn set_tints(&self, rest: Tints, focus: Tints) {
        let mut inner = self.lock();
        if inner.ramps != (rest, focus) {
            inner.ramps = (rest, focus);
            inner.generation = inner.generation.wrapping_add(1);
            inner.cache.clear();
        }
    }

    /// Clear both cached and queued art; an old in-flight preparation cannot
    /// repopulate storage after memory pressure or a palette change.
    pub fn clear(&self) {
        let mut inner = self.lock();
        inner.generation = inner.generation.wrapping_add(1);
        inner.cache.clear();
    }

    pub fn suspend(&self, paused: bool) {
        self.lock().paused = paused;
        if paused {
            self.clear();
        }
        self.wake.notify_one();
    }

    /// Resolve regional and style fallbacks without decoding. Unknown IDs do
    /// not allocate negative-cache entries or background jobs.
    pub fn key(&self, id: &str, stem: &str, color: bool, bounds: Bounds) -> Option<Key> {
        let index = |table: &[(&str, &[u8])], name: &str| {
            table.binary_search_by(|(id, _)| id.cmp(&name)).ok()
        };
        let gray = || {
            index(EMBEDDED_LOGOS, stem)
                .or_else(|| index(EMBEDDED_LOGOS, id))
                .map(Asset::Gray)
        };
        let asset = if color {
            index(EMBEDDED_COLOR_LOGOS, stem)
                .or_else(|| index(EMBEDDED_COLOR_LOGOS, id))
                .map(Asset::Color)
                .or_else(gray)
        } else {
            gray()
        }?;
        Some(Key {
            asset,
            tinted: !color,
            bounds,
            generation: self.lock().generation,
        })
    }

    pub fn get(&self, key: &Key) -> Option<Prepared> {
        self.lock().cache.get(key).cloned().flatten()
    }

    /// Distinguish an unusable asset from one still waiting for preparation.
    /// Only the former needs the text fallback before another cache reset.
    pub fn is_negative(&self, key: &Key) -> bool {
        matches!(self.lock().cache.get(key), Some(None))
    }

    pub fn request(&self, keys: impl IntoIterator<Item = Key>) {
        self.request_window(keys, []);
    }

    pub fn request_window(
        &self,
        visible: impl IntoIterator<Item = Key>,
        neighbors: impl IntoIterator<Item = Key>,
    ) {
        // Iterators may resolve keys through this cache; never evaluate one
        // under its lock. Memory pressure can also race that key resolution.
        let mut visible: Vec<_> = visible.into_iter().collect();
        let neighbors: Vec<_> = neighbors.into_iter().collect();
        let mut inner = self.lock();
        if !inner.paused {
            let generation = inner.generation;
            visible.retain(|key| key.generation == generation);
            inner.visible = visible.iter().copied().collect();
            let warm: Vec<Key> = inner
                .warm
                .iter()
                .map(|key| Key { generation, ..*key })
                .collect();
            inner.cache.request(
                visible
                    .into_iter()
                    .chain(
                        neighbors
                            .into_iter()
                            .filter(|key| key.generation == generation),
                    )
                    .chain(warm),
            );
            self.wake.notify_one();
        }
    }

    /// Name every logo worth having ready before it is asked for. They are
    /// prepared behind the window a screen requests, at a pace that leaves
    /// the processor to whatever the user is doing (`spawn_driver`).
    pub fn set_warm(&self, keys: impl IntoIterator<Item = Key>) {
        let keys: Vec<Key> = keys.into_iter().collect();
        let mut inner = self.lock();
        let same = inner.warm.len() == keys.len()
            && inner.warm.iter().zip(&keys).all(|(old, new)| {
                (old.asset, old.tinted, old.bounds) == (new.asset, new.tinted, new.bounds)
            });
        if same {
            return;
        }
        inner.warm = keys;
        if !inner.paused {
            let generation = inner.generation;
            let warm: Vec<Key> = inner
                .warm
                .iter()
                .map(|key| Key { generation, ..*key })
                .collect();
            inner.cache.request_more(warm);
            self.wake.notify_one();
        }
    }

    /// Whether `key` is on the page in view, as opposed to warm-up work.
    fn is_visible(&self, key: &Key) -> bool {
        self.lock().visible.contains(key)
    }

    fn next_job(&self) -> Option<(Key, (Tints, Tints))> {
        let mut inner = self.lock();
        if inner.paused {
            return None;
        }
        let key = inner.cache.next_job()?;
        Some((key, inner.ramps))
    }

    fn finish(&self, key: Key, image: Option<Prepared>) -> bool {
        let bytes = image.as_ref().map_or(0, Prepared::bytes);
        let mut inner = self.lock();
        let applied = inner.cache.finish(key, image, bytes);
        tracing::debug!(bytes = inner.cache.bytes(), applied, "system logo cache");
        // Prefetch cannot repaint the visible page or repeatedly requeue an
        // evicted neighbor when the entire lookahead exceeds the byte cap.
        applied && inner.visible.contains(&key)
    }

    #[cfg(all(test, feature = "mister"))]
    pub(crate) fn hold_job(self: &Arc<Self>) -> Option<impl FnOnce()> {
        let (key, ramps) = self.next_job()?;
        let cache = self.clone();
        Some(move || {
            cache.finish(key, prepare(key, ramps));
        })
    }

    #[cfg(test)]
    pub(crate) fn prepare_queued(&self) {
        while let Some((key, ramps)) = self.next_job() {
            self.finish(key, prepare(key, ramps));
        }
    }
}

/// These are art bounds from the resolved cell, not another grid-density rule.
pub fn cell_bounds(app: &crate::App, width: i32, height: i32) -> Bounds {
    let pad = app.global::<crate::Sizing>().get_screen_height() * 0.02;
    Bounds::new(
        (width as f32 - 2.0 * pad).ceil().max(1.0) as u32,
        (height as f32 - 2.0 * pad).ceil().max(1.0) as u32,
    )
}

/// Whether logos are prepared to the exact box that paints them and painted
/// one pixel for one: where the software renderer draws, which samples a
/// fitted bitmap nearest-neighbor. Elsewhere they keep their bucketed
/// bounds and the renderer fits them.
pub fn exact_art(app: &crate::App) -> bool {
    app.global::<crate::Motion>().get_zoom_by_size()
}

/// The bounds a logo is prepared to for a grid tile under `exact_art`: the
/// whole pixels of the art box the tile paints
/// (`zaparoo_app::sizing::tile_art_pixels`, the mirror of `Tile` in
/// `ui/tiles.slint`), so the view can paint the result one pixel for one.
/// `zoom` is 1.0 for a tile at rest and the focus zoom for the focused
/// tile's larger copy.
pub fn tile_bounds(
    app: &crate::App,
    cell_width: i32,
    cell_height: i32,
    zoom: f64,
    art: zaparoo_app::sizing::TileArt,
) -> Bounds {
    let inputs = crate::router::output_scene(app).inputs();
    let (width, height) =
        zaparoo_app::sizing::tile_art_pixels(&inputs, cell_width, cell_height, zoom, art);
    Bounds::exact(width, height)
}

/// The focused tile's growth where the view draws it by size and paints a
/// second, larger copy of the art: the factor to prepare that copy for.
/// None where the tile does not grow that way, and one copy serves.
pub fn focus_zoom(app: &crate::App) -> Option<f64> {
    let motion = app.global::<crate::Motion>();
    let zoom = f64::from(motion.get_focus_zoom()) / 100.0;
    (exact_art(app) && motion.get_enabled() && zoom > 1.0).then_some(zoom)
}

/// The size a `width` x `height` source is drawn at inside `bounds`, aspect
/// kept: one side meets its bound and neither passes it.
fn fitted_size(width: u32, height: u32, bounds: Bounds) -> (u32, u32) {
    let (w, h) = (u64::from(width.max(1)), u64::from(height.max(1)));
    let (bw, bh) = (u64::from(bounds.width), u64::from(bounds.height));
    let (fw, fh) = if w * bh >= h * bw {
        (bw, (h * bw + w / 2) / w)
    } else {
        ((w * bh + h / 2) / h, bh)
    };
    (fw.clamp(1, bw.max(1)) as u32, fh.clamp(1, bh.max(1)) as u32)
}

fn prepare(key: Key, ramps: (Tints, Tints)) -> Option<Prepared> {
    let start = std::time::Instant::now();
    let (id, bytes) = match key.asset {
        Asset::Gray(index) => {
            let (id, full) = EMBEDDED_LOGOS[index];
            (id, half_size_source(id, full, key.bounds).unwrap_or(full))
        }
        Asset::Color(index) => EMBEDDED_COLOR_LOGOS[index],
    };
    // Embedded assets currently peak at 1716x160. Keep decoder scratch
    // bounded even if a future generated asset exceeds that contract.
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(4096);
    limits.max_image_height = Some(256);
    limits.max_alloc = Some(8 * 1024 * 1024);
    let mut reader =
        image::ImageReader::with_format(std::io::Cursor::new(bytes), image::ImageFormat::Png);
    reader.limits(limits);
    let decoded = reader.decode().ok()?;
    let rgba = decoded.to_rgba8();
    let rgba = if rgba.width() > key.bounds.width || rgba.height() > key.bounds.height {
        let (width, height) = fitted_size(rgba.width(), rgba.height(), key.bounds);
        // The view paints the result one pixel for one, so this is the
        // only filtering the logo gets. The tint ramp reads straight
        // color, so the filtered pixels are taken back out of alpha.
        let mut scaled = crate::brand::resize_premultiplied(rgba, width, height);
        for pixel in scaled.pixels_mut() {
            let alpha = u16::from(pixel.0[3]);
            for channel in &mut pixel.0[..3] {
                let straight = (u16::from(*channel) * 255 + alpha / 2).checked_div(alpha);
                *channel = straight.unwrap_or(0).min(255) as u8;
            }
        }
        scaled
    } else {
        rgba
    };
    let base = Pixels::clone_from_slice(&rgba, rgba.width(), rgba.height());
    let prepared = if key.tinted {
        Prepared {
            rest: tint(&base, ramps.0),
            focus: Some(tint(&base, ramps.1)),
        }
    } else {
        Prepared {
            rest: base,
            focus: None,
        }
    };
    tracing::debug!(
        id,
        elapsed_us = start.elapsed().as_micros(),
        bytes = prepared.bytes(),
        "system logo prepared"
    );
    Some(prepared)
}

/// A PNG's pixel size, from its header alone.
fn png_size(bytes: &[u8]) -> Option<(u32, u32)> {
    let field = |at: usize| {
        bytes
            .get(at..at + 4)
            .and_then(|b| <[u8; 4]>::try_from(b).ok())
            .map(u32::from_be_bytes)
    };
    bytes
        .starts_with(b"\x89PNG\r\n\x1a\n")
        .then(|| field(16).zip(field(20)))
        .flatten()
}

/// The half-size copy of a grayscale logo, when the tile draws it at half
/// size or less. Decoding and scaling are most of what a logo costs to
/// prepare, and both go by source pixels, so the copy does a quarter of the
/// work. It is still at least as large as what gets drawn: no tile ever
/// shows a logo scaled up from it, and one large enough to want more than
/// half keeps the full-size source.
fn half_size_source(id: &str, full: &'static [u8], bounds: Bounds) -> Option<&'static [u8]> {
    let (width, height) = png_size(full)?;
    let fits_half = u64::from(bounds.width) * 2 <= u64::from(width)
        || u64::from(bounds.height) * 2 <= u64::from(height);
    if !fits_half {
        return None;
    }
    EMBEDDED_HALF_LOGOS
        .binary_search_by(|(name, _)| name.cmp(&id))
        .ok()
        .map(|index| EMBEDDED_HALF_LOGOS[index].1)
}

fn luma(r: u8, g: u8, b: u8) -> i32 {
    (i32::from(r) * 299 + i32::from(g) * 587 + i32::from(b) * 114 + 500) / 1000
}
fn mix(a: u8, b: u8, amount_b: i32) -> u8 {
    ((i32::from(a) * (255 - amount_b) + i32::from(b) * amount_b + 127) / 255).clamp(0, 255) as u8
}

fn tint(base: &Pixels, ramp: Tints) -> Pixels {
    let (mut min, mut max, mut visible) = (255i32, 0i32, 0u32);
    for px in base.as_bytes().chunks_exact(4) {
        if px[3] > 16 {
            let tone = luma(px[0], px[1], px[2]);
            min = min.min(tone);
            max = max.max(tone);
            visible += 1;
        }
    }
    let single_tone = visible == 0 || max - min < 16;
    let mut out = Pixels::new(base.width(), base.height());
    for (src, dst) in base
        .as_bytes()
        .chunks_exact(4)
        .zip(out.make_mut_bytes().chunks_exact_mut(4))
    {
        if src[3] == 0 {
            dst.copy_from_slice(src);
            continue;
        }
        let (r, g, b) = if single_tone {
            ramp[0]
        } else {
            let tone =
                ((luma(src[0], src[1], src[2]) - min) * 255 / (max - min).max(1)).clamp(0, 255);
            let (a, b, amount) = if tone < 128 {
                (ramp[2], ramp[1], tone * 2)
            } else {
                (ramp[1], ramp[0], (tone - 128) * 2)
            };
            (
                mix(a.0, b.0, amount),
                mix(a.1, b.1, amount),
                mix(a.2, b.2, amount),
            )
        };
        dst.copy_from_slice(&[r, g, b, src[3]]);
    }
    out
}

/// An inactive view can be filled immediately before its route commits. Recheck
/// the active view after that turn, sharing the worker's single batched delivery.
/// The callback resolves current state, so canceled destinations cannot steal work.
pub(crate) fn defer_refresh(ctx: &crate::router::Ctx, app: &crate::App) {
    if !ctx.logos.delivery_pending.swap(true, Ordering::AcqRel) {
        arm_refresh(ctx.clone(), app);
    }
}

fn arm_refresh(ctx: crate::router::Ctx, app: &crate::App) {
    let weak = app.as_weak();
    slint::Timer::single_shot(std::time::Duration::from_millis(16), move || {
        ctx.logos.delivery_pending.store(false, Ordering::Release);
        if let Some(app) = weak.upgrade() {
            refresh(&ctx, &app);
        }
    });
}

/// Re-resolve current identities, never saved indices from an earlier page.
/// Defer patching while cached/sliding endpoints own the presentation.
pub(crate) fn refresh(ctx: &crate::router::Ctx, app: &crate::App) {
    if app.global::<crate::Shell>().get_dormant() || crate::navigation::active() {
        return;
    }
    match app.global::<crate::Shell>().get_active_screen() {
        crate::Screen::Systems | crate::Screen::FavoriteSystems => {
            let sliding = crate::router::lock(&ctx.shared).systems_model.sliding;
            if !sliding {
                crate::systems::render(ctx, app);
            }
        }
        crate::Screen::Hub => {
            if !crate::router::lock(&ctx.shared).hub.sliding {
                crate::hub::render(ctx, app);
            }
        }
        crate::Screen::Games
        | crate::Screen::Favorites
        | crate::Screen::Recents
        | crate::Screen::SearchResults => {
            let sliding = {
                let shared = crate::router::lock(&ctx.shared);
                shared.games.sliding || shared.games.folder_sliding
            };
            if !sliding {
                crate::games::render(ctx, app);
            }
        }
        _ => ctx.logos.request([]),
    }
}

pub fn spawn_driver(ctx: &Arc<crate::router::Ctx>, app: &crate::App) {
    let cache = ctx.logos.clone();
    let weak = app.as_weak();
    let lifetime = Arc::downgrade(ctx);
    let mut dormant = ctx.dormant.subscribe();
    ctx.handle.spawn(async move {
        loop {
            let notified = cache.wake.notified();
            let Some((key, ramps)) = cache.next_job() else {
                tokio::select! {
                    () = notified => {},
                    changed = dormant.changed() => { if changed.is_err() { return; } }
                }
                continue;
            };
            // No UI, palette, queue or cache lock crosses the decode/tint.
            let urgent = cache.is_visible(&key);
            let started = std::time::Instant::now();
            let image = tokio::task::spawn_blocking(move || prepare(key, ramps)).await;
            // Work nobody is looking at yet takes half a core at most: rest
            // as long as it ran. A page that comes into view meanwhile waits
            // out one such rest, a few tens of milliseconds, then runs at
            // full pace.
            if !urgent {
                tokio::time::sleep(started.elapsed()).await;
            }
            let image = match image {
                Ok(image) => image,
                Err(error) => {
                    tracing::warn!(%error, "system logo worker failed");
                    None
                }
            };
            if !cache.finish(key, image) || cache.delivery_pending.swap(true, Ordering::AcqRel) {
                continue;
            }
            let Some(ctx) = lifetime.upgrade() else {
                return;
            };
            if weak
                .upgrade_in_event_loop(move |app| arm_refresh((*ctx).clone(), &app))
                .is_err()
            {
                return;
            }
        }
    });
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, reason = "required embedded fixtures")]
    use super::*;

    #[test]
    fn a_small_tile_decodes_the_half_size_copy_and_never_scales_it_up() {
        for (id, full) in EMBEDDED_LOGOS {
            let (width, height) = png_size(full).expect("embedded logo is a PNG");
            // Every grayscale logo has its half-size copy.
            let small = Bounds::new(64, 64);
            if u64::from(small.width) * 2 <= u64::from(width)
                || u64::from(small.height) * 2 <= u64::from(height)
            {
                let half = half_size_source(id, full, small).expect("half-size copy");
                let (half_w, half_h) = png_size(half).expect("half-size copy is a PNG");
                assert!(half_w.abs_diff(width.div_ceil(2)) <= 1, "{id}");
                assert!(half_h.abs_diff(height.div_ceil(2)) <= 1, "{id}");
                // What the tile draws is the full art fitted to its box;
                // the copy must hold at least that many pixels each way.
                let scale = (f64::from(small.width) / f64::from(width))
                    .min(f64::from(small.height) / f64::from(height));
                assert!(f64::from(half_w) + 1.0 >= f64::from(width) * scale, "{id}");
                assert!(f64::from(half_h) + 1.0 >= f64::from(height) * scale, "{id}");
            }
            // A tile that shows the logo at more than half size keeps the
            // full-size source.
            let roomy = Bounds::new(width.max(32), height.max(32));
            if roomy.width * 2 > width && roomy.height * 2 > height {
                assert!(half_size_source(id, full, roomy).is_none(), "{id}");
            }
        }
    }

    #[test]
    fn the_half_size_copy_prepares_to_the_same_size_as_the_full_source() {
        let logos = Logos::new();
        let bounds = Bounds::new(128, 64);
        let key = logos
            .key("NES", "NES", false, bounds)
            .expect("the NES logo is embedded");
        assert!(
            matches!(key.asset, Asset::Gray(_)),
            "a tinted key is grayscale"
        );
        let Asset::Gray(index) = key.asset else {
            return;
        };
        let (id, full) = EMBEDDED_LOGOS[index];
        assert!(half_size_source(id, full, bounds).is_some());
        let prepared = prepare(key, DEFAULT_RAMPS).expect("prepared");
        let (width, height) = png_size(full).expect("png");
        let scale = (f64::from(bounds.width) / f64::from(width))
            .min(f64::from(bounds.height) / f64::from(height));
        let expected_w = (f64::from(width) * scale).round() as u32;
        assert!(prepared.rest.width().abs_diff(expected_w) <= 1);
        assert!(prepared.rest.width() <= bounds.width);
        assert!(prepared.rest.height() <= bounds.height);
    }

    #[test]
    fn the_warm_list_is_prepared_behind_the_page_in_view_and_survives_a_reset() {
        let logos = Logos::new();
        let bounds = Bounds::new(128, 64);
        let key = |id: &str| logos.key(id, id, false, bounds).expect("embedded logo");
        let (page, later) = (key("NES"), key("SNES"));
        logos.set_warm([page, later]);
        // The page in view goes first, the rest of the list after it.
        logos.request_window([page], []);
        assert_eq!(logos.next_job().map(|(job, _)| job), Some(page));
        logos.finish(page, prepare(page, DEFAULT_RAMPS));
        assert!(!logos.is_visible(&later));
        logos.prepare_queued();
        assert!(logos.get(&later).is_some(), "ready before anyone asks");

        // A game launch or a palette change empties the cache; the list is
        // kept and prepared again for the generation now in force.
        logos.clear();
        logos.request([]);
        logos.prepare_queued();
        let later = key("SNES");
        assert!(logos.get(&later).is_some());
    }

    #[test]
    fn a_logo_is_fitted_inside_its_bounds_and_meets_one_of_them() {
        // Wide art meets the width, tall art the height.
        assert_eq!(fitted_size(400, 100, Bounds::exact(101, 60)), (101, 25));
        assert_eq!(fitted_size(100, 400, Bounds::exact(101, 60)), (15, 60));
        assert_eq!(fitted_size(300, 300, Bounds::exact(77, 77)), (77, 77));
        for (width, height) in [(1716, 160), (512, 121), (97, 333), (640, 640), (3, 900)] {
            for bounds in [(1, 1), (60, 40), (93, 61), (300, 180), (17, 400)] {
                let bounds = Bounds::exact(bounds.0, bounds.1);
                let (w, h) = fitted_size(width, height, bounds);
                assert!((1..=bounds.width).contains(&w) && (1..=bounds.height).contains(&h));
                assert!(w == bounds.width || h == bounds.height);
            }
        }
    }

    #[test]
    fn a_prepared_logo_fits_its_bounds_exactly_and_keeps_its_tints() {
        let cache = Logos::new();
        for (bounds, tinted) in [
            (Bounds::exact(93, 61), false),
            (Bounds::exact(57, 44), false),
            (Bounds::exact(93, 61), true),
        ] {
            let key = cache.key("SNES", "SNES", tinted, bounds);
            assert!(key.is_some(), "embedded logo");
            let Some(key) = key else { return };
            cache.request([key]);
            cache.prepare_queued();
            let prepared = cache.get(&key);
            assert!(prepared.is_some(), "prepared logo");
            let Some(prepared) = prepared else { return };
            let (rest, focus) = prepared.images();
            let size = rest.size();
            assert_eq!(focus.size(), size);
            assert!(size.width <= bounds.width && size.height <= bounds.height);
            assert!(size.width == bounds.width || size.height == bounds.height);
            // Filtering leaves straight color: opaque pixels are not
            // darkened, and nothing transparent carries color weight.
            let opaque = prepared
                .rest
                .as_bytes()
                .chunks_exact(4)
                .filter(|px| px[3] == 255)
                .count();
            assert!(opaque > 0, "the logo has solid pixels");
        }
    }

    #[test]
    fn tint_preserves_alpha_and_tonal_ramp() {
        let base = Pixels::clone_from_slice(
            &[
                0, 0, 0, 255, 128, 128, 128, 255, 255, 255, 255, 77, 0, 0, 0, 0,
            ],
            4,
            1,
        );
        let out = tint(&base, [(200, 200, 200), (100, 100, 100), (20, 20, 20)]);
        let px = out.as_bytes();
        assert_eq!(&px[..4], &[20, 20, 20, 255]);
        assert!((95..=105).contains(&px[4]));
        assert_eq!(&px[8..], &[200, 200, 200, 77, 0, 0, 0, 0]);
        let single = Pixels::clone_from_slice(&[255, 255, 255, 77, 250, 250, 250, 255], 2, 1);
        assert_eq!(
            tint(&single, [(1, 2, 3), (4, 5, 6), (7, 8, 9)]).as_bytes(),
            &[1, 2, 3, 77, 1, 2, 3, 255]
        );
    }

    #[test]
    fn lookup_is_cache_only_preparation_is_sized_and_image_identity_is_reused() {
        let cache = Logos::new();
        let key = cache
            .key("SNES", "SNES", false, Bounds::new(130, 65))
            .expect("SNES");
        assert!(cache.get(&key).is_none());
        cache.request([key]);
        cache.prepare_queued();
        let pair = cache.get(&key).expect("prepared");
        assert!(pair.rest.width() <= 256 && pair.rest.height() <= 128);
        assert!(pair.focus.is_some());
        assert_eq!(pair.images(), cache.get(&key).expect("cached").images());
        let color = cache
            .key("SNES", "SNES", true, Bounds::new(130, 65))
            .expect("color");
        cache.request([color]);
        cache.prepare_queued();
        let pair = cache.get(&color).expect("color prepared");
        assert_eq!(pair.images().0, pair.images().1);
        assert_eq!(pair.bytes(), pair.rest.as_bytes().len());
    }

    #[test]
    fn only_failed_preparation_is_negative_not_pending_or_evicted_art() {
        let cache = Logos::new();
        let key = cache
            .key("SNES", "SNES", false, Bounds::new(128, 64))
            .expect("known asset");
        assert!(!cache.is_negative(&key));
        cache.request([key]);
        let (job, _) = cache.next_job().expect("queued asset");
        assert!(!cache.is_negative(&key));
        assert!(cache.finish(job, None));
        assert!(cache.is_negative(&key));
        cache.clear();
        assert!(
            !cache.is_negative(&key),
            "memory trim permits a fresh preparation"
        );
    }

    #[test]
    fn neighbor_prefetch_is_silent_and_pre_clear_keys_cannot_requeue() {
        let cache = Logos::new();
        let bounds = Bounds::new(128, 64);
        let visible = cache.key("SNES", "SNES", false, bounds).expect("visible");
        let neighbor = cache.key("NES", "NES", false, bounds).expect("neighbor");
        cache.request_window([visible], [neighbor]);
        let (key, ramps) = cache.next_job().expect("visible first");
        assert_eq!(key, visible);
        assert!(cache.finish(key, prepare(key, ramps)));
        let (key, ramps) = cache.next_job().expect("neighbor next");
        assert_eq!(key, neighbor);
        assert!(
            !cache.finish(key, prepare(key, ramps)),
            "prefetch cannot trigger a render/requeue loop"
        );
        assert!(cache.get(&neighbor).is_some());
        cache.clear();
        cache.request([visible]);
        assert!(
            cache.next_job().is_none(),
            "memory pressure invalidates keys resolved before it"
        );
    }

    #[test]
    fn regional_fallbacks_unknown_ids_and_stale_palettes() {
        let cache = Logos::new();
        let bounds = Bounds::new(256, 128);
        assert_eq!(
            cache.key("SNES", "missing.region", true, bounds),
            cache.key("SNES", "SNES", true, bounds)
        );
        for id in ["", "../etc/passwd", "NotARealSystem"] {
            assert!(cache.key(id, id, false, bounds).is_none());
        }
        let old = cache.key("SNES", "SNES", false, bounds).expect("gray");
        cache.request([old]);
        let (_, ramps) = cache.next_job().expect("job");
        cache.set_tints([(1, 2, 3); 3], [(4, 5, 6); 3]);
        let new = cache
            .key("SNES", "SNES", false, bounds)
            .expect("new palette");
        assert_ne!(old, new);
        cache.request([new]);
        assert!(!cache.finish(old, prepare(old, ramps)));
        assert!(cache.get(&old).is_none());
        cache.prepare_queued();
        assert!(cache.get(&new).is_some());
        cache.suspend(true);
        assert_eq!(cache.lock().cache.bytes(), 0);
        cache.request([new]);
        assert!(cache.next_job().is_none());
    }

    #[test]
    fn embedded_catalog_prepares_with_bounded_storage() {
        let cache = Logos::new();
        let bounds = Bounds::new(256, 128);
        for (color, table) in [(false, EMBEDDED_LOGOS), (true, EMBEDDED_COLOR_LOGOS)] {
            for (id, _) in table {
                let key = cache.key(id, id, color, bounds).expect("embedded key");
                cache.request([key]);
                cache.prepare_queued();
                let pair = cache.get(&key).expect("every embedded PNG must decode");
                assert!(pair.rest.width() <= bounds.width && pair.rest.height() <= bounds.height);
                assert!(cache.lock().cache.bytes() <= CACHE_BYTES);
                if color {
                    assert!(
                        EMBEDDED_LOGOS
                            .binary_search_by_key(id, |(id, _)| id)
                            .is_ok(),
                        "color requires grayscale fallback"
                    );
                }
            }
        }
    }

    #[test]
    fn embedded_catalog_fits_decoder_contract() {
        assert!(EMBEDDED_LOGOS.len() > 100 && EMBEDDED_COLOR_LOGOS.len() > 100);
        for (id, bytes) in EMBEDDED_LOGOS.iter().chain(EMBEDDED_COLOR_LOGOS) {
            let reader = image::ImageReader::with_format(
                std::io::Cursor::new(bytes),
                image::ImageFormat::Png,
            );
            let (w, h) = reader.into_dimensions().expect("PNG dimensions");
            assert!(w <= 4096 && h <= 256, "{id}: decoder limits");
        }
    }

    #[test]
    #[ignore = "manual cold-versus-warm embedded logo timing probe"]
    #[allow(clippy::print_stderr, reason = "manual measured timings")]
    fn profile_cold_and_warm_logo_page() {
        let cache = Logos::new();
        let keys: Vec<_> = EMBEDDED_LOGOS
            .iter()
            .take(28)
            .filter_map(|(id, _)| cache.key(id, id, false, Bounds::new(200, 120)))
            .collect();
        let start = std::time::Instant::now();
        cache.request(keys.clone());
        for key in &keys {
            assert!(cache.get(key).is_none());
        }
        eprintln!(
            "cold cache-only publication: {} pairs in {:?}",
            keys.len(),
            start.elapsed()
        );
        let start = std::time::Instant::now();
        cache.prepare_queued();
        eprintln!(
            "background preparation: {:?}; retained={} bytes",
            start.elapsed(),
            cache.lock().cache.bytes()
        );
        let start = std::time::Instant::now();
        for key in &keys {
            std::hint::black_box(cache.get(key).expect("warm").images());
        }
        eprintln!(
            "warm image sharing: {} pairs in {:?}",
            keys.len(),
            start.elapsed()
        );
    }
}
