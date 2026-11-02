// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

//! Single-threaded handoff between router page commits and the presenter
//! that owns the screen, and the cached page slide and cover fades every
//! presenter runs from it.

use super::scanout::Damage;
use crate::frame_transition::{Active, Blend, CoverFades, Rect, Spec, Started, BLEND_ONE};
use slint::platform::software_renderer::{PhysicalRegion, PremultipliedRgbaColor, Rgb565Pixel};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, PoisonError};

static AVAILABLE: AtomicBool = AtomicBool::new(false);
static BUSY: AtomicBool = AtomicBool::new(false);
static CANCELLED: AtomicBool = AtomicBool::new(false);
static PENDING: Mutex<Option<Spec>> = Mutex::new(None);
static FADES_AVAILABLE: AtomicBool = AtomicBool::new(false);
static FADE_REQUESTS: Mutex<Vec<Rect>> = Mutex::new(Vec::new());

pub(super) fn set_available(available: bool) {
    AVAILABLE.store(available, Ordering::SeqCst);
    if !available {
        *PENDING.lock().unwrap_or_else(PoisonError::into_inner) = None;
        BUSY.store(false, Ordering::SeqCst);
        CANCELLED.store(false, Ordering::SeqCst);
    }
}

pub(super) fn request(spec: Spec) -> bool {
    if !AVAILABLE.load(Ordering::SeqCst)
        || BUSY
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
    {
        return false;
    }
    *PENDING.lock().unwrap_or_else(PoisonError::into_inner) = Some(spec);
    true
}

pub(super) fn take_request() -> Option<Spec> {
    PENDING
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .take()
}

pub(super) fn finish() {
    BUSY.store(false, Ordering::SeqCst);
}

pub(super) fn cancel() {
    *PENDING.lock().unwrap_or_else(PoisonError::into_inner) = None;
    CANCELLED.store(true, Ordering::SeqCst);
    BUSY.store(false, Ordering::SeqCst);
}

pub(super) fn take_cancelled() -> bool {
    CANCELLED.swap(false, Ordering::SeqCst)
}

pub(super) fn set_fades_available(available: bool) {
    FADES_AVAILABLE.store(available, Ordering::SeqCst);
    if !available {
        take_fade_requests();
    }
}

/// Asks for the covers that just landed in `rects`, in the presenter's
/// frame, to fade in. False when no presenter runs cover fades or a page
/// slide owns the frame.
pub(super) fn request_fades(rects: &[Rect]) -> bool {
    if !FADES_AVAILABLE.load(Ordering::SeqCst) || BUSY.load(Ordering::SeqCst) {
        return false;
    }
    FADE_REQUESTS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .extend_from_slice(rects);
    true
}

pub(super) fn take_fade_requests() -> Vec<Rect> {
    std::mem::take(&mut *FADE_REQUESTS.lock().unwrap_or_else(PoisonError::into_inner))
}

/// Theme.bg (`#0f0f23`) for the inter-page gap, as the 32-bit presenters
/// render it.
pub(super) const BACKGROUND_RGBA: PremultipliedRgbaColor = PremultipliedRgbaColor {
    red: 0x0f,
    green: 0x0f,
    blue: 0x23,
    alpha: 0xff,
};

/// One presenter's cached page slide. Slint renders the outgoing and the
/// incoming page once each into the presenter's canonical frame; between
/// them the presenter shows this second frame, which only ever holds the
/// two pages composed at the current step.
pub(super) struct CachedSlide<T> {
    /// Presentation-only buffer for cached page motion. Slint never
    /// renders into it, so its destination cache remains coherent.
    frame: Vec<T>,
    active: Option<Active<T>>,
    /// Shown in the gap between the two pages.
    background: T,
}

impl<T: Copy + Default> CachedSlide<T> {
    pub(super) fn new(pixels: usize, background: T) -> Self {
        Self {
            frame: vec![T::default(); pixels],
            active: None,
            background,
        }
    }

    /// The canonical frame changed size. Motion in flight was composed for
    /// the old one, so it is dropped along with any request waiting to
    /// start, and the next ordinary frame republishes everything.
    pub(super) fn resize(&mut self, pixels: usize) {
        if self.active.take().is_some() || take_request().is_some() {
            cancel();
        }
        self.frame = vec![T::default(); pixels];
    }

    pub(super) const fn is_active(&self) -> bool {
        self.active.is_some()
    }

    pub(super) fn frame(&self) -> &[T] {
        &self.frame
    }

    /// Forgets motion in flight without publishing anything.
    pub(super) fn abandon(&mut self) {
        self.active = None;
    }

    /// Starts a requested slide. A request arrives before the destination
    /// properties render, so the caller runs this first to preserve the
    /// outgoing pixels, then lets Slint build its canonical endpoint.
    pub(super) fn begin(&mut self, canonical: &[T], width: usize, height: usize) {
        let Some(spec) = take_request() else {
            return;
        };
        let frame_len = width * height;
        self.frame[..frame_len].copy_from_slice(&canonical[..frame_len]);
        match Active::new(
            &canonical[..frame_len],
            width,
            height,
            spec,
            self.background,
        ) {
            Ok(transition) => {
                tracing::info!(
                    x = spec.rect.x,
                    y = spec.rect.y,
                    width = spec.rect.width,
                    height = spec.rect.height,
                    direction = ?spec.direction,
                    frames = spec.total_frames,
                    "cached page transition started"
                );
                self.active = Some(transition);
            }
            Err(error) => {
                finish();
                tracing::warn!(?error, "cached page transition rejected");
            }
        }
    }

    /// Composes the current step into the presentation frame. Returns what
    /// it changed and whether that was the last step, or None when no slide
    /// is active. The last step is the whole canonical frame.
    pub(super) fn compose(
        &mut self,
        canonical: &[T],
        width: usize,
        height: usize,
    ) -> Option<(Damage, bool)> {
        let active = self.active.as_mut()?;
        let frame_len = width * height;
        let result = active.compose_now(
            &canonical[..frame_len],
            &mut self.frame[..frame_len],
            super::platform::timeline().unwrap_or_default(),
        );
        match result {
            Ok(step) => Some((
                Damage {
                    x0: step.damage.x as u32,
                    y0: step.damage.y as u32,
                    x1: (step.damage.x + step.damage.width) as u32,
                    y1: (step.damage.y + step.damage.height) as u32,
                },
                step.finished,
            )),
            Err(error) => {
                tracing::warn!(?error, "cached page transition composition failed");
                self.frame[..frame_len].copy_from_slice(&canonical[..frame_len]);
                Some((
                    Damage {
                        x0: 0,
                        y0: 0,
                        x1: width as u32,
                        y1: height as u32,
                    },
                    true,
                ))
            }
        }
    }

    /// The last step has been shown.
    pub(super) fn finish(&mut self) {
        self.active = None;
        finish();
    }
}

impl Blend for Rgb565Pixel {
    fn same(self, other: Self) -> bool {
        self == other
    }

    fn same_run(left: &[Self], right: &[Self]) -> bool {
        left == right
    }

    fn mix(old: Self, new: Self, weight: u32) -> Self {
        // Green moves to the high half so each channel has room for the
        // product: one multiply mixes all three.
        const SPREAD: u32 = 0x07E0_F81F;
        let spread = |pixel: Self| (u32::from(pixel.0) | u32::from(pixel.0) << 16) & SPREAD;
        let mixed =
            ((spread(old) * (BLEND_ONE - weight) + spread(new) * weight) / BLEND_ONE) & SPREAD;
        Self((mixed | mixed >> 16) as u16)
    }
}

impl Blend for PremultipliedRgbaColor {
    fn same(self, other: Self) -> bool {
        (self.red, self.green, self.blue, self.alpha)
            == (other.red, other.green, other.blue, other.alpha)
    }

    fn mix(old: Self, new: Self, weight: u32) -> Self {
        let channel = |old: u8, new: u8| {
            ((u32::from(old) * (BLEND_ONE - weight) + u32::from(new) * weight) / BLEND_ONE) as u8
        };
        Self {
            red: channel(old.red, new.red),
            green: channel(old.green, new.green),
            blue: channel(old.blue, new.blue),
            alpha: channel(old.alpha, new.alpha),
        }
    }
}

/// One presenter's cover fades. Unlike a page slide they never take the
/// frame over: Slint goes on rendering everything else, and the presenter
/// reads each row it publishes through `row`.
pub(super) struct PresenterFades<T> {
    fades: CoverFades<T>,
    available: bool,
}

impl<T: Blend> PresenterFades<T> {
    /// `available` is whether this presenter owns the screen the drivers
    /// lay their grids out on. It answers fade requests for as long as it
    /// lives.
    pub(super) fn new(available: bool) -> Self {
        if available {
            set_fades_available(true);
        }
        Self {
            fades: CoverFades::new(),
            available,
        }
    }

    pub(super) fn is_active(&self) -> bool {
        self.fades.is_active()
    }

    /// Drops every fade, for a caller about to republish the whole frame.
    pub(super) fn clear(&mut self) {
        self.fades.clear();
    }

    /// Before Slint renders: keeps what is under each cover about to land.
    /// A page slide owns the frame while it runs, so `sliding` ends the
    /// fades rather than starting more.
    pub(super) fn before_render(
        &mut self,
        canonical: &[T],
        width: usize,
        height: usize,
        sliding: bool,
    ) {
        if !self.available {
            return;
        }
        let requests = take_fade_requests();
        if sliding {
            self.fades.clear();
        } else if !requests.is_empty() {
            let started = super::platform::timeline().map_or_else(
                || Started::Wall(std::time::Instant::now()),
                Started::Timeline,
            );
            self.fades
                .begin(canonical, width, height, &requests, started);
        }
    }

    /// After Slint renders `region` into `canonical`.
    pub(super) fn rendered(&mut self, canonical: &[T], region: &PhysicalRegion) {
        if !self.fades.is_active() {
            return;
        }
        let damage: Vec<Rect> = region
            .iter()
            .map(|(origin, size)| Rect {
                x: origin.x.max(0) as usize,
                y: origin.y.max(0) as usize,
                width: size.width as usize,
                height: size.height as usize,
            })
            .collect();
        self.fades.rendered(canonical, &damage);
    }

    /// Advances every fade to this frame and returns the regions the
    /// display has to be brought up to date with.
    pub(super) fn step(&mut self) -> &[Rect] {
        self.fades
            .step(super::platform::timeline().unwrap_or_default())
    }

    /// See `CoverFades::mixed_segments`.
    pub(super) fn mixed_segments(
        &mut self,
        y: usize,
        x0: usize,
        x1: usize,
        row: &[T],
        out: impl FnMut(usize, &[T]),
    ) {
        self.fades.mixed_segments(y, x0, x1, row, out);
    }

    /// See `CoverFades::row`.
    pub(super) fn row<'a>(&'a mut self, y: usize, x0: usize, canonical: &'a [T]) -> &'a [T] {
        self.fades.row(y, x0, canonical)
    }
}

impl<T> Drop for PresenterFades<T> {
    fn drop(&mut self) {
        if self.available {
            set_fades_available(false);
        }
    }
}

/// `rects` as one box, for the presenters that bring a slot up to date by
/// box.
pub(super) fn bounds(rects: &[Rect]) -> Damage {
    rects.iter().fold(Damage::EMPTY, |damage, rect| {
        damage.union(Damage {
            x0: rect.x as u32,
            y0: rect.y as u32,
            x1: (rect.x + rect.width) as u32,
            y1: (rect.y + rect.height) as u32,
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame_transition::Direction;
    use std::time::{Duration, Instant};

    #[test]
    fn rgb565_mixes_each_channel_on_its_own() {
        // All red against all green and blue.
        let old = Rgb565Pixel(0xF800);
        let new = Rgb565Pixel(0x07FF);
        assert_eq!(Rgb565Pixel::mix(old, new, 0), old);
        assert_eq!(Rgb565Pixel::mix(old, new, BLEND_ONE), new);
        let half = Rgb565Pixel::mix(old, new, BLEND_ONE / 2).0;
        assert_eq!((half >> 11, (half >> 5) & 0x3f, half & 0x1f), (15, 31, 15));
        let white = Rgb565Pixel(0xffff);
        for weight in 0..=BLEND_ONE {
            assert_eq!(
                Rgb565Pixel::mix(white, white, weight),
                white,
                "no channel bleeds"
            );
        }
    }

    #[test]
    fn cancellation_discards_obsolete_request_and_allows_immediate_retarget() {
        set_available(true);
        let spec = Spec {
            rect: Rect {
                x: 0,
                y: 0,
                width: 10,
                height: 10,
            },
            gap: 1,
            direction: Direction::Up,
            total_frames: 14,
            started: Started::Wall(Instant::now()),
        };
        assert!(request(spec));
        assert!(!request(spec), "never queue a second animation");
        cancel();
        assert_eq!(take_request(), None);
        assert!(request(spec), "new input does not wait for an old deadline");
        assert!(take_cancelled());
        assert!(!take_cancelled());
        assert_eq!(take_request(), Some(spec));
        finish();
        set_available(false);
    }

    fn band(started: Started) -> Spec {
        Spec {
            rect: Rect {
                x: 1,
                y: 1,
                width: 2,
                height: 2,
            },
            gap: 0,
            direction: Direction::Up,
            total_frames: 4,
            started,
        }
    }

    #[test]
    fn a_slide_presents_its_own_frame_and_only_the_band_changes() {
        set_available(true);
        let mut slide = CachedSlide::<u8>::new(16, 0xff);
        let mut canonical: Vec<u8> = (0..16).collect();
        // Nothing was requested, so nothing begins.
        slide.begin(&canonical, 4, 4);
        assert!(!slide.is_active());
        assert_eq!(slide.compose(&canonical, 4, 4), None);

        assert!(request(band(Started::Wall(Instant::now()))));
        slide.begin(&canonical, 4, 4);
        assert!(slide.is_active());
        assert_eq!(take_request(), None, "the request was consumed");
        // Slint renders the destination, and dirties more than the band.
        for pixel in &mut canonical {
            *pixel += 100;
        }
        assert_eq!(
            slide.compose(&canonical, 4, 4),
            Some((
                Damage {
                    x0: 1,
                    y0: 1,
                    x1: 3,
                    y1: 3
                },
                false
            ))
        );
        // Outside the band the presented frame is still the outgoing page.
        for index in [0, 3, 4, 7, 8, 11, 12, 15] {
            assert_eq!(slide.frame()[index], index as u8);
        }
        assert!(
            !request(band(Started::Wall(Instant::now()))),
            "one slide at a time"
        );
        slide.abandon();
        assert!(!slide.is_active());
        set_available(false);
    }

    #[test]
    fn the_last_step_is_the_whole_canonical_frame_and_frees_the_channel() {
        set_available(true);
        let started = Instant::now().checked_sub(Duration::from_secs(1));
        assert!(started.is_some(), "clock supports fixture offset");
        let Some(started) = started else { return };
        let mut slide = CachedSlide::<u8>::new(16, 0xff);
        let mut canonical: Vec<u8> = (0..16).collect();
        assert!(request(band(Started::Wall(started))));
        slide.begin(&canonical, 4, 4);
        for pixel in &mut canonical {
            *pixel += 100;
        }
        assert_eq!(
            slide.compose(&canonical, 4, 4),
            Some((
                Damage {
                    x0: 0,
                    y0: 0,
                    x1: 4,
                    y1: 4
                },
                true
            ))
        );
        assert_eq!(slide.frame(), canonical.as_slice());
        slide.finish();
        assert!(!slide.is_active());
        assert!(request(band(Started::Wall(Instant::now()))));
        set_available(false);
    }

    #[test]
    fn a_resize_drops_motion_in_flight_and_asks_for_a_full_republish() {
        set_available(true);
        let mut slide = CachedSlide::<u8>::new(16, 0xff);
        let canonical: Vec<u8> = (0..16).collect();
        // Nothing in flight: a resize is only a resize.
        slide.resize(12);
        assert_eq!(slide.frame().len(), 12);
        assert!(!take_cancelled());

        slide.resize(16);
        assert!(request(band(Started::Wall(Instant::now()))));
        slide.begin(&canonical, 4, 4);
        slide.resize(9);
        assert!(!slide.is_active());
        assert_eq!(slide.frame().len(), 9);
        assert!(take_cancelled(), "the next frame republishes everything");
        // The band was solved for the old frame: it must not start in the
        // new one, and the channel is free for the next request.
        assert!(request(band(Started::Wall(Instant::now()))));
        slide.resize(16);
        assert_eq!(take_request(), None);
        assert!(take_cancelled());
        assert!(request(band(Started::Wall(Instant::now()))));
        set_available(false);
    }

    #[test]
    fn a_band_that_does_not_fit_the_frame_is_rejected_and_frees_the_channel() {
        set_available(true);
        let mut slide = CachedSlide::<u8>::new(16, 0xff);
        let canonical: Vec<u8> = (0..16).collect();
        assert!(request(band(Started::Wall(Instant::now()))));
        // A 2x2 frame cannot hold a band that ends at column and row 3.
        slide.begin(&canonical, 2, 2);
        assert!(!slide.is_active());
        assert!(request(band(Started::Wall(Instant::now()))));
        set_available(false);
    }
}
