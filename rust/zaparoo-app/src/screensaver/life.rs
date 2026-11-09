// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

//! Life: Conway's Game of Life on a wrapping grid, a few generations a
//! second, each cell tinted by how long it has lived. The first generation
//! is the Zaparoo logo or the time in block digits; when the board settles
//! into a still life or a short loop, or nearly dies out, it starts over
//! from the logo, the time, or a random soup.

use super::{slot_rgb, Rng};

/// Generations per second.
pub const RATE: f32 = 4.0;
/// Generations remembered to spot a loop.
pub const MEMORY: usize = 16;
/// Generations a board may run before it is reseeded anyway.
pub const MAX_GENERATIONS: u32 = 1500;

/// What a new board starts from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Seed {
    Logo,
    Clock,
    Soup,
}

/// 3x5 block digits, one row per byte, bit 2 the left column.
const DIGITS: [[u8; 5]; 10] = [
    [0b111, 0b101, 0b101, 0b101, 0b111],
    [0b010, 0b110, 0b010, 0b010, 0b111],
    [0b111, 0b001, 0b111, 0b100, 0b111],
    [0b111, 0b001, 0b111, 0b001, 0b111],
    [0b101, 0b101, 0b111, 0b001, 0b001],
    [0b111, 0b100, 0b111, 0b001, 0b111],
    [0b111, 0b100, 0b111, 0b101, 0b111],
    [0b111, 0b001, 0b010, 0b010, 0b010],
    [0b111, 0b101, 0b111, 0b101, 0b111],
    [0b111, 0b101, 0b111, 0b001, 0b111],
];

#[derive(Debug, Clone)]
pub struct Life {
    pub width: usize,
    pub height: usize,
    /// 0 is dead; otherwise generations alive, saturating.
    ages: Vec<u8>,
    scratch: Vec<u8>,
    history: Vec<u64>,
    generation: u32,
    carry: f32,
    /// The logo as a mask at the grid's scale, `(mask, width, height)`.
    logo: Option<(Vec<bool>, usize, usize)>,
    next_seed: usize,
    rng: Rng,
}

impl Life {
    pub fn new(
        width: usize,
        height: usize,
        logo: Option<(Vec<bool>, usize, usize)>,
        seed: u64,
    ) -> Self {
        let mut life = Self {
            width,
            height,
            ages: vec![0; width * height],
            scratch: vec![0; width * height],
            history: Vec::with_capacity(MEMORY),
            generation: 0,
            carry: 0.0,
            logo,
            next_seed: 0,
            rng: Rng::new(seed),
        };
        let first = if life.logo.is_some() && life.rng.coin() {
            Seed::Logo
        } else {
            Seed::Clock
        };
        life.seed(first, (12, 0));
        life
    }

    /// Start the board over from `seed`; `time` is (hour, minute) for the
    /// clock seed.
    pub fn seed(&mut self, seed: Seed, time: (u8, u8)) {
        self.ages.fill(0);
        self.history.clear();
        self.generation = 0;
        match seed {
            Seed::Logo => {
                if let Some((mask, w, h)) = self.logo.take() {
                    let x0 = self.width.saturating_sub(w) / 2;
                    let y0 = self.height.saturating_sub(h) / 2;
                    for y in 0..h.min(self.height) {
                        for x in 0..w.min(self.width) {
                            if mask[y * w + x] {
                                self.ages[(y0 + y) * self.width + x0 + x] = 1;
                            }
                        }
                    }
                    self.logo = Some((mask, w, h));
                } else {
                    self.soup();
                }
            }
            Seed::Clock => self.digits(time),
            Seed::Soup => self.soup(),
        }
    }

    fn soup(&mut self) {
        for cell in &mut self.ages {
            *cell = u8::from(self.rng.below(100) < 30);
        }
    }

    fn digits(&mut self, (hour, minute): (u8, u8)) {
        let text = [hour / 10, hour % 10, 10, minute / 10, minute % 10];
        // Each digit is 3 wide plus a gap; the colon is 1 wide plus a gap.
        let cols: usize = text
            .iter()
            .map(|d| if *d == 10 { 2 } else { 4 })
            .sum::<usize>()
            - 1;
        let scale = (self.width / (cols + 4)).min(self.height / 9).max(1);
        let x0 = self.width.saturating_sub(cols * scale) / 2;
        let y0 = self.height.saturating_sub(5 * scale) / 2;
        let mut x = x0;
        for d in text {
            if d == 10 {
                for row in [1usize, 3] {
                    self.block(x, y0 + row * scale, scale);
                }
                x += 2 * scale;
                continue;
            }
            for (row, bits) in DIGITS[d as usize].iter().enumerate() {
                for col in 0..3 {
                    if bits & (0b100 >> col) != 0 {
                        self.block(x + col * scale, y0 + row * scale, scale);
                    }
                }
            }
            x += 4 * scale;
        }
    }

    fn block(&mut self, x: usize, y: usize, size: usize) {
        for dy in 0..size {
            for dx in 0..size {
                let (cx, cy) = (x + dx, y + dy);
                if cx < self.width && cy < self.height {
                    self.ages[cy * self.width + cx] = 1;
                }
            }
        }
    }

    /// Advance by `dt` seconds; true when a new generation is ready to
    /// draw. `time` seeds the clock when the board starts over.
    pub fn step(&mut self, dt: f32, time: (u8, u8)) -> bool {
        self.carry += dt * RATE;
        if self.carry < 1.0 {
            return false;
        }
        self.carry -= self.carry.floor();
        self.generate();
        let hash = self.hash();
        let alive = self.ages.iter().filter(|a| **a > 0).count();
        if self.history.contains(&hash)
            || alive < (self.width * self.height) / 200
            || self.generation >= MAX_GENERATIONS
        {
            let seeds = [Seed::Soup, Seed::Logo, Seed::Clock];
            self.next_seed = (self.next_seed + 1) % seeds.len();
            self.seed(seeds[self.next_seed], time);
        } else {
            if self.history.len() == MEMORY {
                self.history.remove(0);
            }
            self.history.push(hash);
        }
        true
    }

    fn generate(&mut self) {
        let (w, h) = (self.width, self.height);
        for y in 0..h {
            let up = (y + h - 1) % h;
            let down = (y + 1) % h;
            for x in 0..w {
                let left = (x + w - 1) % w;
                let right = (x + 1) % w;
                let n = [
                    up * w + left,
                    up * w + x,
                    up * w + right,
                    y * w + left,
                    y * w + right,
                    down * w + left,
                    down * w + x,
                    down * w + right,
                ]
                .iter()
                .filter(|i| self.ages[**i] > 0)
                .count();
                let age = self.ages[y * w + x];
                self.scratch[y * w + x] = match (age > 0, n) {
                    (true, 2 | 3) => age.saturating_add(1),
                    (false, 3) => 1,
                    _ => 0,
                };
            }
        }
        std::mem::swap(&mut self.ages, &mut self.scratch);
        self.generation += 1;
    }

    /// Which cells live, ages aside: a still life hashes the same each
    /// generation.
    fn hash(&self) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for (i, a) in self.ages.iter().enumerate() {
            if *a > 0 {
                h ^= i as u64;
                h = h.wrapping_mul(0x0100_0000_01b3);
            }
        }
        h
    }

    /// The board as RGB, one pixel a cell, into `out` (3 bytes a cell).
    /// Newborn cells are near white, then they take the palette's colors
    /// and darken as they age.
    pub fn render(&self, out: &mut [u8]) {
        for (i, age) in self.ages.iter().enumerate() {
            let rgb = tint(*age);
            if let Some(px) = out.get_mut(i * 3..i * 3 + 3) {
                px.copy_from_slice(&[rgb.0, rgb.1, rgb.2]);
            }
        }
    }

    pub fn alive(&self) -> usize {
        self.ages.iter().filter(|a| **a > 0).count()
    }
}

/// A cell's color by age.
pub fn tint(age: u8) -> (u8, u8, u8) {
    match age {
        0 => (0, 0, 0),
        1 => (0xF4, 0xF0, 0xFF),
        2..=3 => slot_rgb(0),
        4..=8 => slot_rgb(1),
        9..=20 => slot_rgb(5),
        _ => {
            let (r, g, b) = slot_rgb(2);
            (r / 2, g / 2, b / 2)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_blinker_blinks() {
        let mut life = Life::new(8, 8, None, 1);
        life.ages.fill(0);
        for x in 2..5 {
            life.ages[3 * 8 + x] = 1;
        }
        life.generate();
        let column: Vec<_> = (2..5).map(|y| life.ages[y * 8 + 3] > 0).collect();
        assert_eq!(column, vec![true, true, true]);
        assert_eq!(life.alive(), 3);
    }

    #[test]
    fn a_still_board_starts_over() {
        let mut life = Life::new(16, 16, None, 2);
        life.ages.fill(0);
        // A block: a still life.
        for (x, y) in [(5, 5), (6, 5), (5, 6), (6, 6)] {
            life.ages[y * 16 + x] = 1;
        }
        life.history.clear();
        // Enough population to not count as dying out on a 16x16 board.
        let mut reseeded = false;
        for _ in 0..4 {
            life.step(1.0 / RATE, (10, 30));
            if life.generation == 0 {
                reseeded = true;
            }
        }
        assert!(reseeded);
    }

    #[test]
    fn the_clock_seed_draws_digits_and_logo_seeds_center() {
        let mut life = Life::new(80, 45, Some((vec![true; 4 * 3], 4, 3)), 3);
        life.seed(Seed::Clock, (12, 34));
        assert!(life.alive() > 20);
        life.seed(Seed::Logo, (0, 0));
        assert_eq!(life.alive(), 12);
        assert!(life.ages[(45 - 3) / 2 * 80 + (80 - 4) / 2] > 0);
    }

    #[test]
    fn rendering_fills_three_bytes_a_cell() {
        let life = Life::new(10, 6, None, 4);
        let mut out = vec![0u8; 10 * 6 * 3];
        life.render(&mut out);
        let lit = out.chunks(3).filter(|p| p != &[0, 0, 0]).count();
        assert_eq!(lit, life.alive());
    }
}
