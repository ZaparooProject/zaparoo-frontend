// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

//! Single-threaded handoff between router page commits and latch presentation.

use crate::frame_transition::Spec;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancellation_discards_obsolete_request_and_allows_immediate_retarget() {
        set_available(true);
        let spec = Spec {
            rect: crate::frame_transition::Rect {
                x: 0,
                y: 0,
                width: 10,
                height: 10,
            },
            gap: 1,
            direction: crate::frame_transition::Direction::Up,
            total_frames: 14,
            started: std::time::Instant::now(),
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
}
