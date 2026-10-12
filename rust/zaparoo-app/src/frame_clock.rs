// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

//! A fixed-step animation timeline for a frame loop paced by its display.
//!
//! Each presented frame moves the timeline by exactly one refresh period,
//! however long the frame took, so motion is sampled at even steps and a
//! late frame slows it by that frame instead of enlarging the next step.
//! A turn that presented nothing moves it by the real time that passed, in
//! whole periods, so timers keep real time while nothing animates.
//!
//! The clock is injected: the driver passes the real time since its own
//! start and reads the timeline back.

use std::time::Duration;

/// What the frame loop did on the turn that just ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Turn {
    /// A frame went to the display.
    Presented,
    /// Nothing was drawn; the loop waited for the next refresh.
    Idle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameClock {
    now: Duration,
    /// Real time at the last advance.
    last_real: Duration,
    /// Real idle time not yet on the timeline, in nanoseconds. Negative
    /// while the timeline is ahead of it.
    idle_balance: i128,
}

impl FrameClock {
    /// A timeline that starts level with real time.
    pub const fn new(real_now: Duration) -> Self {
        Self {
            now: real_now,
            last_real: real_now,
            idle_balance: 0,
        }
    }

    pub const fn now(&self) -> Duration {
        self.now
    }

    /// Accounts for the turn that just ended and returns the new time.
    ///
    /// An idle turn advances at least one period, so a wait that returned a
    /// little early still moves the timers; the borrowed time is repaid by
    /// the waits that follow, and a turn adds nothing once the timeline is
    /// a whole period ahead. Over a long idle it therefore tracks real time
    /// without ever leading it by more than two periods.
    pub fn advance(&mut self, previous: Turn, real_now: Duration, period: Duration) -> Duration {
        let elapsed = real_now.saturating_sub(self.last_real);
        self.last_real = self.last_real.max(real_now);
        let period_ns = i128::try_from(period.as_nanos()).unwrap_or(i128::MAX);
        if period_ns == 0 {
            return self.now;
        }
        let periods = match previous {
            Turn::Presented => 1,
            Turn::Idle => {
                self.idle_balance = self
                    .idle_balance
                    .saturating_add(i128::try_from(elapsed.as_nanos()).unwrap_or(i128::MAX));
                let periods = if self.idle_balance >= period_ns {
                    self.idle_balance / period_ns
                } else {
                    i128::from(self.idle_balance > -period_ns)
                };
                self.idle_balance -= periods * period_ns;
                periods
            }
        };
        let step = period.saturating_mul(u32::try_from(periods).unwrap_or(u32::MAX));
        self.now = self.now.saturating_add(step);
        self.now
    }
}

/// The refresh period of a display that does not report one, measured from
/// the waits for its vertical blank.
///
/// Only waits on turns that drew nothing are fed in, so each sample is one
/// whole refresh. The estimate is the median of the recent samples and only
/// moves when that median leaves it by more than 2%, so the step the
/// timeline takes does not wander with the jitter of the wait.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RefreshEstimate {
    samples: [Duration; Self::WINDOW],
    len: usize,
    next: usize,
    period: Duration,
}

impl RefreshEstimate {
    const WINDOW: usize = 15;
    /// Samples needed before the nominal period is replaced.
    const SETTLED: usize = 8;
    /// 200 Hz and 20 Hz: a wait outside these is not one refresh.
    const SHORTEST: Duration = Duration::from_millis(5);
    const LONGEST: Duration = Duration::from_millis(50);

    pub const fn new(nominal: Duration) -> Self {
        Self {
            samples: [Duration::ZERO; Self::WINDOW],
            len: 0,
            next: 0,
            period: nominal,
        }
    }

    pub const fn period(&self) -> Duration {
        self.period
    }

    /// Records the time between two consecutive vertical blanks.
    pub fn observe(&mut self, interval: Duration) {
        if interval < Self::SHORTEST || interval > Self::LONGEST {
            return;
        }
        self.samples[self.next] = interval;
        self.next = (self.next + 1) % Self::WINDOW;
        self.len = (self.len + 1).min(Self::WINDOW);
        if self.len < Self::SETTLED {
            return;
        }
        let mut sorted = self.samples;
        let sorted = &mut sorted[..self.len];
        sorted.sort_unstable();
        let median = sorted[self.len / 2];
        if median.abs_diff(self.period) * 50 > self.period {
            self.period = median;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NTSC: Duration = Duration::from_micros(16_667);
    const PAL: Duration = Duration::from_millis(20);

    #[test]
    fn a_presented_frame_is_one_period_however_long_it_took() {
        let start = Duration::from_secs(3);
        let mut clock = FrameClock::new(start);
        let mut real = start;
        for (frame, took_ms) in [1_u32, 16, 17, 45, 400, 2].into_iter().enumerate() {
            real += Duration::from_millis(u64::from(took_ms));
            let now = clock.advance(Turn::Presented, real, NTSC);
            assert_eq!(now, start + NTSC * (frame as u32 + 1));
            assert_eq!(clock.now(), now);
        }
    }

    #[test]
    fn idle_catches_up_real_time_in_whole_periods() {
        let mut clock = FrameClock::new(Duration::ZERO);
        // A stall of 100 ms is five whole periods and 16.665 ms to carry.
        assert_eq!(
            clock.advance(Turn::Idle, Duration::from_millis(100), NTSC),
            NTSC * 5
        );
        // The carry plus 2 microseconds completes a sixth.
        assert_eq!(
            clock.advance(Turn::Idle, Duration::from_micros(100_002), NTSC),
            NTSC * 6
        );
        assert_eq!(clock.now().as_nanos() % NTSC.as_nanos(), 0);
    }

    #[test]
    fn an_idle_wait_that_returns_early_still_moves_one_period() {
        let mut clock = FrameClock::new(Duration::ZERO);
        let mut real = Duration::ZERO;
        for turn in 1..=600_u32 {
            // Alternating short and long waits around the nominal period.
            real += Duration::from_micros(if turn % 2 == 0 { 16_900 } else { 16_434 });
            assert_eq!(clock.advance(Turn::Idle, real, NTSC), NTSC * turn);
        }
    }

    #[test]
    fn a_long_idle_never_outruns_real_time() {
        // A display a little faster than the nominal period.
        let wait = Duration::from_micros(16_600);
        let mut clock = FrameClock::new(Duration::ZERO);
        let mut real = Duration::ZERO;
        for _ in 0..100_000 {
            real += wait;
            let now = clock.advance(Turn::Idle, real, NTSC);
            assert!(now <= real + NTSC * 2, "{now:?} leads {real:?}");
            assert!(now + NTSC * 2 >= real, "{now:?} trails {real:?}");
        }
        // A display slower than it: five 20 ms waits are six periods.
        let mut clock = FrameClock::new(Duration::ZERO);
        let mut real = Duration::ZERO;
        for _ in 0..50_000 {
            real += PAL;
            let now = clock.advance(Turn::Idle, real, NTSC);
            assert!(now <= real + NTSC * 2 && now + NTSC * 2 >= real);
        }
    }

    #[test]
    fn late_frames_slow_motion_and_idle_does_not_win_the_time_back() {
        let mut clock = FrameClock::new(Duration::ZERO);
        let mut real = Duration::ZERO;
        // Ten frames that each took 50 ms.
        for _ in 0..10 {
            real += Duration::from_millis(50);
            clock.advance(Turn::Presented, real, NTSC);
        }
        assert_eq!(clock.now(), NTSC * 10);
        // The idle turn after them counts only its own wait.
        real += NTSC;
        assert_eq!(clock.advance(Turn::Idle, real, NTSC), NTSC * 11);
    }

    #[test]
    fn the_timeline_never_goes_backwards() {
        let mut clock = FrameClock::new(Duration::from_secs(5));
        let mut previous = clock.now();
        // Real time that stands still, jumps, and reads earlier than before.
        for (turn, real_ms) in [
            (Turn::Idle, 5_000),
            (Turn::Idle, 5_000),
            (Turn::Presented, 4_000),
            (Turn::Idle, 4_500),
            (Turn::Idle, 9_000),
            (Turn::Idle, 0),
            (Turn::Presented, 9_001),
            (Turn::Idle, 9_001),
        ] {
            let now = clock.advance(turn, Duration::from_millis(real_ms), NTSC);
            assert!(now >= previous);
            previous = now;
        }
        assert_eq!(clock.advance(Turn::Idle, Duration::MAX, NTSC), clock.now());
        assert!(clock.now() >= previous);
    }

    #[test]
    fn a_pal_presenter_steps_twenty_milliseconds() {
        let mut clock = FrameClock::new(Duration::ZERO);
        let mut real = Duration::ZERO;
        for frame in 1..=50_u32 {
            real += Duration::from_millis(23);
            assert_eq!(clock.advance(Turn::Presented, real, PAL), PAL * frame);
        }
        assert_eq!(clock.now(), Duration::from_secs(1));
        real += Duration::from_millis(60);
        assert_eq!(
            clock.advance(Turn::Idle, real, PAL),
            Duration::from_millis(1_060)
        );
    }

    #[test]
    fn a_zero_period_holds_the_timeline_still() {
        let mut clock = FrameClock::new(Duration::from_secs(1));
        assert_eq!(
            clock.advance(Turn::Idle, Duration::from_secs(2), Duration::ZERO),
            Duration::from_secs(1)
        );
    }

    #[test]
    fn a_fifty_hertz_display_is_found_from_its_waits() {
        let mut estimate = RefreshEstimate::new(NTSC);
        for wait in 0..7_u64 {
            estimate.observe(Duration::from_micros(19_950 + wait * 15));
            assert_eq!(estimate.period(), NTSC, "too few samples to move");
        }
        estimate.observe(PAL);
        let found = estimate.period();
        assert!(
            found.abs_diff(PAL) < Duration::from_micros(100),
            "{found:?}"
        );
        // Jitter inside 2% leaves the step alone.
        for wait in 0..200_u64 {
            estimate.observe(Duration::from_micros(19_800 + (wait * 37) % 400));
            assert_eq!(estimate.period(), found);
        }
    }

    #[test]
    fn stray_waits_do_not_move_the_estimate() {
        let mut estimate = RefreshEstimate::new(NTSC);
        // A wait that returned at once, a stall, and a few doubled waits
        // among ordinary ones.
        for turn in 0..300_u32 {
            estimate.observe(match turn % 10 {
                0 => Duration::from_micros(40),
                3 => Duration::from_millis(400),
                6 => NTSC * 2,
                _ => NTSC,
            });
            assert_eq!(estimate.period(), NTSC);
        }
    }

    #[test]
    fn a_mode_change_is_followed() {
        let mut estimate = RefreshEstimate::new(NTSC);
        for _ in 0..20 {
            estimate.observe(PAL);
        }
        assert_eq!(estimate.period(), PAL);
        for _ in 0..20 {
            estimate.observe(NTSC);
        }
        assert_eq!(estimate.period(), NTSC);
    }
}
