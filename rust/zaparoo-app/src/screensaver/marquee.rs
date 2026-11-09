// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

//! Recent games: one line at a time crosses the screen right to left, a
//! recently played game's system logo and name, at a new height each pass.
//! The view moves the line and says when it has left; this picks what the
//! next pass shows and where. With no recent games the clock crosses.

use super::Rng;

/// Seconds a line takes to cross one scene width.
pub const CROSS_SECONDS: f32 = 9.0;
/// The line's height, in height units.
pub const LINE_HEIGHT: f32 = 0.09;

/// What one pass shows.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pass {
    /// The recent game, or `None` for the clock.
    pub entry: Option<usize>,
    /// The line's top, in height units.
    pub y: f32,
}

#[derive(Debug, Clone)]
pub struct Marquee {
    entries: usize,
    order: Vec<usize>,
    next: usize,
    last_y: f32,
    rng: Rng,
}

impl Marquee {
    pub fn new(entries: usize, seed: u64) -> Self {
        let mut rng = Rng::new(seed);
        let mut order: Vec<usize> = (0..entries).collect();
        rng.shuffle(&mut order);
        Self {
            entries,
            order,
            next: 0,
            last_y: 0.5,
            rng,
        }
    }

    /// The next pass: every recent game once, in a shuffled order, then
    /// round again; each at a height at least a fifth of the screen from
    /// the last one.
    pub fn next_pass(&mut self) -> Pass {
        let entry = if self.entries == 0 {
            None
        } else {
            let e = self.order[self.next % self.entries];
            self.next += 1;
            if self.next.is_multiple_of(self.entries) {
                self.rng.shuffle(&mut self.order);
            }
            Some(e)
        };
        let lo = 0.05;
        let hi = 1.0 - LINE_HEIGHT - 0.05;
        let mut y = self.rng.range(lo, hi);
        for _ in 0..8 {
            if (y - self.last_y).abs() >= 0.2 {
                break;
            }
            y = self.rng.range(lo, hi);
        }
        self.last_y = y;
        Pass { entry, y }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_entry_shows_before_any_repeats() {
        let mut m = Marquee::new(5, 2);
        let mut seen: Vec<_> = (0..5)
            .map(|_| m.next_pass().entry.unwrap_or(usize::MAX))
            .collect();
        seen.sort_unstable();
        assert_eq!(seen, vec![0, 1, 2, 3, 4]);
    }

    #[test]
    fn no_entries_cross_the_clock_inside_the_screen() {
        let mut m = Marquee::new(0, 3);
        for _ in 0..100 {
            let p = m.next_pass();
            assert_eq!(p.entry, None);
            assert!(p.y >= 0.0 && p.y + LINE_HEIGHT <= 1.0);
        }
    }
}
