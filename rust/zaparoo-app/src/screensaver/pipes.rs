// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

//! Pipes: flat pipes grow across a grid one segment at a time, turning at
//! right angles with a round joint at each turn, each pipe in its own
//! palette color. A pipe that boxes itself in ends and another starts;
//! when the screen has filled to the cap it clears and starts over. Every
//! step adds one or two small pieces, so a frame repaints only those.

use super::{Look, Rng, Sprite, SLOTS};

/// Steps per second.
pub const RATE: f32 = 12.0;
/// The cap on pieces before the screen clears.
pub const MAX_PIECES: usize = 420;
/// A pipe's thickness as a fraction of a cell.
pub const THICKNESS: f32 = 0.36;
/// Odds (1 in n) that a pipe turns at a step.
pub const TURN_ODDS: usize = 5;

const DIRS: [(i32, i32); 4] = [(1, 0), (0, 1), (-1, 0), (0, -1)];

/// What a step does to the picture.
#[derive(Debug, Clone, PartialEq)]
pub enum Change {
    /// Add these pieces on top of what is there.
    Add(Vec<Sprite>),
    /// Wipe the screen.
    Clear,
    Nothing,
}

#[derive(Debug, Clone)]
pub struct Pipes {
    columns: i32,
    rows: i32,
    cell: f32,
    left: f32,
    top: f32,
    used: Vec<bool>,
    head: Option<((i32, i32), usize)>,
    color: u8,
    pieces: usize,
    carry: f32,
    rng: Rng,
}

impl Pipes {
    pub fn new(aspect: f32, low_res: bool, seed: u64) -> Self {
        let rows = if low_res { 12 } else { 18 };
        let cell = 1.0 / rows as f32;
        let columns = (aspect / cell).floor() as i32;
        Self {
            columns,
            rows,
            cell,
            left: (aspect - columns as f32 * cell) / 2.0,
            top: 0.0,
            used: vec![false; (columns * rows) as usize],
            head: None,
            color: 0,
            pieces: 0,
            carry: 0.0,
            rng: Rng::new(seed),
        }
    }

    fn free(&self, (x, y): (i32, i32)) -> bool {
        x >= 0
            && y >= 0
            && x < self.columns
            && y < self.rows
            && !self.used[(y * self.columns + x) as usize]
    }

    fn mark(&mut self, (x, y): (i32, i32)) {
        self.used[(y * self.columns + x) as usize] = true;
    }

    fn center(&self, (x, y): (i32, i32)) -> (f32, f32) {
        (
            self.left + (x as f32 + 0.5) * self.cell,
            self.top + (y as f32 + 0.5) * self.cell,
        )
    }

    /// Advance by `dt` seconds.
    pub fn step(&mut self, dt: f32) -> Change {
        self.carry += dt * RATE;
        let mut added = Vec::new();
        while self.carry >= 1.0 {
            self.carry -= 1.0;
            if self.pieces >= MAX_PIECES {
                self.reset();
                return Change::Clear;
            }
            added.extend(self.grow());
        }
        if added.is_empty() {
            Change::Nothing
        } else {
            self.pieces += added.len();
            Change::Add(added)
        }
    }

    fn reset(&mut self) {
        self.used.fill(false);
        self.head = None;
        self.pieces = 0;
    }

    fn grow(&mut self) -> Vec<Sprite> {
        let Some((at, dir)) = self.head else {
            return self.start();
        };
        let mut options = vec![dir];
        if self.rng.below(TURN_ODDS) == 0 {
            options.insert(
                0,
                if self.rng.coin() {
                    (dir + 1) % 4
                } else {
                    (dir + 3) % 4
                },
            );
        }
        options.extend([(dir + 1) % 4, (dir + 3) % 4]);
        for d in options {
            let next = (at.0 + DIRS[d].0, at.1 + DIRS[d].1);
            if self.free(next) {
                self.mark(next);
                self.head = Some((next, d));
                let mut out = Vec::new();
                if d != dir {
                    out.push(self.joint(at));
                }
                out.push(self.segment(at, next));
                return out;
            }
        }
        // Boxed in: this pipe ends with a cap, and the next one starts.
        self.head = None;
        vec![self.joint(at)]
    }

    fn start(&mut self) -> Vec<Sprite> {
        let free: Vec<i32> = (0..self.columns * self.rows)
            .filter(|i| !self.used[*i as usize])
            .collect();
        if free.is_empty() {
            self.pieces = MAX_PIECES;
            return Vec::new();
        }
        let i = free[self.rng.below(free.len())];
        let at = (i % self.columns, i / self.columns);
        self.mark(at);
        self.color = (self.color + 1 + self.rng.below(SLOTS as usize - 1) as u8) % SLOTS;
        self.head = Some((at, self.rng.below(4)));
        vec![self.joint(at)]
    }

    fn joint(&self, at: (i32, i32)) -> Sprite {
        let d = self.cell * THICKNESS * 1.5;
        let (cx, cy) = self.center(at);
        Sprite::new(cx - d / 2.0, cy - d / 2.0, d, d, Look::Round(self.color))
    }

    fn segment(&self, a: (i32, i32), b: (i32, i32)) -> Sprite {
        let t = self.cell * THICKNESS;
        let (ax, ay) = self.center(a);
        let (bx, by) = self.center(b);
        let (x0, x1) = (ax.min(bx), ax.max(bx));
        let (y0, y1) = (ay.min(by), ay.max(by));
        Sprite::new(
            x0 - t / 2.0,
            y0 - t / 2.0,
            x1 - x0 + t,
            y1 - y0 + t,
            Look::Block(self.color),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pieces_are_small_inside_and_the_screen_clears_at_the_cap() {
        let aspect = 16.0 / 9.0;
        let mut pipes = Pipes::new(aspect, false, 1);
        let mut total = 0;
        let mut cleared = 0;
        for _ in 0..(30 * 120) {
            match pipes.step(1.0 / 30.0) {
                Change::Add(pieces) => {
                    assert!(pieces.len() <= 2);
                    for p in pieces {
                        assert!(p.w <= 2.0 / 18.0 && p.h <= 2.0 / 18.0, "{p:?}");
                        assert!(
                            p.x >= 0.0 && p.x + p.w <= aspect && p.y >= 0.0 && p.y + p.h <= 1.0
                        );
                        total += 1;
                    }
                    assert!(total <= MAX_PIECES + 2);
                }
                Change::Clear => {
                    cleared += 1;
                    total = 0;
                }
                Change::Nothing => {}
            }
        }
        assert!(cleared >= 1);
    }

    #[test]
    fn turns_get_a_joint() {
        let mut pipes = Pipes::new(4.0 / 3.0, true, 2);
        let mut joints = 0;
        for _ in 0..600 {
            if let Change::Add(pieces) = pipes.step(0.1) {
                joints += pieces
                    .iter()
                    .filter(|p| matches!(p.look, Look::Round(_)))
                    .count();
            }
        }
        assert!(joints > 5);
    }
}
