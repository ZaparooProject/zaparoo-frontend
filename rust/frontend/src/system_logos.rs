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
            inner.cache.request(
                visible.into_iter().chain(
                    neighbors
                        .into_iter()
                        .filter(|key| key.generation == generation),
                ),
            );
            self.wake.notify_one();
        }
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

fn prepare(key: Key, ramps: (Tints, Tints)) -> Option<Prepared> {
    let start = std::time::Instant::now();
    let (id, bytes) = match key.asset {
        Asset::Gray(index) => EMBEDDED_LOGOS[index],
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
    let scaled = if decoded.width() > key.bounds.width || decoded.height() > key.bounds.height {
        decoded.resize(
            key.bounds.width,
            key.bounds.height,
            image::imageops::FilterType::Triangle,
        )
    } else {
        decoded
    };
    let rgba = scaled.to_rgba8();
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
            let image = tokio::task::spawn_blocking(move || prepare(key, ramps)).await;
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
