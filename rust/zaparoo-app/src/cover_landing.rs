// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

//! Covers for the page on screen land together. On the software renderer
//! a frame that paints newly landed covers is expensive, and covers fetched
//! a few at a time would each cost one such frame while the ones before
//! them are still fading in, which stalls the fades. So covers that arrive
//! while the page still waits on others are held, and land together once
//! the page has them all or the first has waited `HOLD`.

use std::time::{Duration, Instant};

/// The longest the first cover of a hold waits for the rest of its page.
pub const HOLD: Duration = Duration::from_millis(150);

#[derive(Debug, Default)]
pub struct Hold {
    since: Option<Instant>,
    generation: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Landing {
    /// Land everything held now.
    Now,
    /// Keep holding. An arrival that starts a hold names its deadline and
    /// generation: the caller lands the held covers then, through
    /// `Hold::expired`, unless they have landed sooner.
    Hold(Option<(Instant, u64)>),
}

impl Hold {
    /// Covers arrived at `now`; `page_waiting` is whether the page on
    /// screen still waits on any of its covers.
    pub fn arrived(&mut self, now: Instant, page_waiting: bool) -> Landing {
        if !page_waiting {
            self.since = None;
            return Landing::Now;
        }
        match self.since {
            Some(since) if now.saturating_duration_since(since) >= HOLD => {
                self.since = None;
                Landing::Now
            }
            Some(_) => Landing::Hold(None),
            None => {
                self.since = Some(now);
                self.generation = self.generation.wrapping_add(1);
                Landing::Hold(Some((now + HOLD, self.generation)))
            }
        }
    }

    /// The deadline of hold `generation` came. True when that hold is
    /// still waiting, and its covers land now.
    pub fn expired(&mut self, generation: u64) -> bool {
        if self.since.is_some() && self.generation == generation {
            self.since = None;
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_page_with_nothing_left_to_wait_for_lands_at_once() {
        let mut hold = Hold::default();
        assert_eq!(hold.arrived(Instant::now(), false), Landing::Now);
    }

    #[test]
    fn covers_wait_for_the_rest_of_the_page_then_land_together() {
        let start = Instant::now();
        let mut hold = Hold::default();
        let first = hold.arrived(start, true);
        assert!(
            matches!(first, Landing::Hold(Some(_))),
            "the first arrival starts a hold"
        );
        let Landing::Hold(Some((deadline, generation))) = first else {
            return;
        };
        assert_eq!(deadline, start + HOLD);
        assert_eq!(
            hold.arrived(start + Duration::from_millis(40), true),
            Landing::Hold(None),
            "a later arrival joins the hold"
        );
        assert_eq!(
            hold.arrived(start + Duration::from_millis(80), false),
            Landing::Now,
            "the page's last cover lands them all"
        );
        assert!(!hold.expired(generation), "its deadline finds nothing held");
    }

    #[test]
    fn a_hold_lands_at_its_deadline_and_a_stale_deadline_does_nothing() {
        let start = Instant::now();
        let mut hold = Hold::default();
        let started = hold.arrived(start, true);
        assert!(
            matches!(started, Landing::Hold(Some(_))),
            "the first arrival starts a hold"
        );
        let Landing::Hold(Some((_, first))) = started else {
            return;
        };
        assert!(hold.expired(first));
        let restarted = hold.arrived(start + HOLD, true);
        assert!(
            matches!(restarted, Landing::Hold(Some(_))),
            "the next arrival starts a new hold"
        );
        let Landing::Hold(Some((_, second))) = restarted else {
            return;
        };
        assert!(!hold.expired(first), "the old deadline belongs to no hold");
        assert_eq!(
            hold.arrived(start + HOLD * 2, true),
            Landing::Now,
            "an arrival past the deadline lands at once"
        );
        assert!(!hold.expired(second));
    }
}
