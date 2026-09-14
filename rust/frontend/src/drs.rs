// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// Heavy-phase signal for dynamic resolution scaling. The router
// raises the count around work it KNOWS repaints the whole viewport
// for a sustained stretch (page swoops); the MiSTer frame loop drops
// to motion res only while the count is high and pops back right
// after - both switches land at moments when the entire screen is
// already changing, which is what masks them. Ordinary navigation
// never switches: small dirty rects are cheap at native res.
//
// Plain statics because the router and the frame loop share the UI
// thread; atomics keep it sound if a signal ever moves off-thread.

use std::sync::atomic::{AtomicU32, Ordering};

static HEAVY: AtomicU32 = AtomicU32::new(0);

/// Enter a heavy phase. Pair every call with exactly one `heavy_end`.
pub fn heavy_begin() {
    HEAVY.fetch_add(1, Ordering::SeqCst);
}

/// Leave a heavy phase. Saturates at zero so a double-end cannot
/// wedge the counter below its floor.
pub fn heavy_end() {
    let _ = HEAVY.fetch_update(Ordering::SeqCst, Ordering::SeqCst, |v| {
        Some(v.saturating_sub(1))
    });
}

/// True while any heavy phase is active.
#[cfg_attr(
    not(feature = "mister"),
    allow(
        dead_code,
        reason = "read by the MiSTer frame loop; desktop has no DRS"
    )
)]
pub fn heavy_active() -> bool {
    HEAVY.load(Ordering::SeqCst) > 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nests_and_saturates() {
        assert!(!heavy_active());
        heavy_begin();
        heavy_begin();
        heavy_end();
        assert!(heavy_active());
        heavy_end();
        assert!(!heavy_active());
        heavy_end(); // must not underflow
        assert!(!heavy_active());
        heavy_begin();
        assert!(heavy_active());
        heavy_end();
    }
}
