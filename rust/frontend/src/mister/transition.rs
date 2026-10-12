// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

//! Single-threaded handoff between router page commits and the presenter
//! that owns the screen, and the cached page slide every presenter runs
//! from it.

use super::scanout::Damage;
use crate::frame_transition::{Active, Spec};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, PoisonError};

static AVAILABLE: AtomicBool = AtomicBool::new(false);
static BUSY: AtomicBool = AtomicBool::new(false);
static CANCELLED: AtomicBool = AtomicBool::new(false);
static PENDING: Mutex<Option<Spec>> = Mutex::new(None);

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

/// Theme.bg (`#0f0f23`) for the inter-page gap, as the 32-bit presenters
/// render it.
pub(super) const BACKGROUND_RGBA: slint::platform::software_renderer::PremultipliedRgbaColor =
    slint::platform::software_renderer::PremultipliedRgbaColor {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame_transition::{Direction, Rect, Started};
    use std::time::{Duration, Instant};

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
