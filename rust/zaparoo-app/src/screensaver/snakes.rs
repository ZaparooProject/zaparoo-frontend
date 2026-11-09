// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

//! Snakes: a few fixed-length snakes wander a grid, each in its own palette
//! color. A head moves one cell per step, turning at right angles, never
//! into a wall or a body; the tail follows. A snake that boxes itself in
//! turns around. Each snake owns a fixed set of segment slots, so a step
//! rewrites one slot per snake (the old tail segment becomes the new head
//! segment) and a frame repaints only those cells.

use std::collections::VecDeque;

use super::{Look, Rng, Sprite, SLOTS};

/// Steps per second.
pub const RATE: f32 = 12.0;
/// A body's thickness as a fraction of a cell.
pub const THICKNESS: f32 = 0.7;
/// Odds (1 in n) that a snake turns at a step when it could go straight.
pub const TURN_ODDS: usize = 6;

const DIRS: [(i32, i32); 4] = [(1, 0), (0, 1), (-1, 0), (0, -1)];

type Cell = (i32, i32);

#[derive(Debug, Clone)]
struct Snake {
    /// Cells from tail (front) to head (back).
    body: VecDeque<Cell>,
    dir: usize,
    color: u8,
    /// Each segment's slot within this snake's ring, tail first:
    /// `segs[i]` joins `body[i]` and `body[i + 1]`.
    segs: VecDeque<usize>,
}

#[derive(Debug, Clone)]
pub struct Snakes {
    columns: i32,
    rows: i32,
    cell: f32,
    left: f32,
    length: usize,
    used: Vec<bool>,
    herd: Vec<Snake>,
    carry: f32,
    rng: Rng,
}

impl Snakes {
    pub fn new(aspect: f32, low_res: bool, seed: u64) -> Self {
        let (rows, count, length) = if low_res { (12, 3, 9) } else { (18, 4, 13) };
        let cell = 1.0 / rows as f32;
        let columns = (aspect / cell).floor() as i32;
        let mut rng = Rng::new(seed);
        let first = rng.below(SLOTS as usize) as u8;
        let mut used = vec![false; (columns * rows) as usize];
        let mut snakes = Vec::with_capacity(count);
        for n in 0..count {
            // Start each snake on its own free cell; it grows to length as
            // it moves off.
            let at = loop {
                let i = rng.below(used.len()) as i32;
                if !used[i as usize] {
                    break (i % columns, i / columns);
                }
            };
            used[(at.1 * columns + at.0) as usize] = true;
            snakes.push(Snake {
                body: VecDeque::from([at]),
                dir: rng.below(4),
                color: (first + n as u8) % SLOTS,
                segs: VecDeque::new(),
            });
        }
        Self {
            columns,
            rows,
            cell,
            left: (aspect - columns as f32 * cell) / 2.0,
            length,
            used,
            herd: snakes,
            carry: 0.0,
            rng,
        }
    }

    /// Segment slots per snake: one between each pair of adjacent cells.
    fn ring(&self) -> usize {
        self.length - 1
    }

    /// Every slot, hidden: the starting picture.
    pub fn sprites(&self) -> Vec<Sprite> {
        let mut out = Vec::with_capacity(self.herd.len() * self.ring());
        for snake in &self.herd {
            out.extend((0..self.ring()).map(|_| Self::hidden(snake.color)));
        }
        out
    }

    /// Advance by `dt` seconds. Returns the slots that changed, by index
    /// into `sprites()`, last write wins.
    pub fn step(&mut self, dt: f32) -> Vec<(usize, Sprite)> {
        self.carry += dt * RATE;
        let mut out = Vec::new();
        while self.carry >= 1.0 {
            self.carry -= 1.0;
            for n in 0..self.herd.len() {
                if let Some(change) = self.advance(n) {
                    out.push(change);
                }
            }
        }
        out
    }

    fn index(&self, (x, y): Cell) -> usize {
        (y * self.columns + x) as usize
    }

    fn inside(&self, (x, y): Cell) -> bool {
        x >= 0 && y >= 0 && x < self.columns && y < self.rows
    }

    fn free(&self, at: Cell) -> bool {
        self.inside(at) && !self.used[self.index(at)]
    }

    /// Take the tail off a full snake, freeing its cell and handing back
    /// its segment's slot for the new head.
    fn retract(&mut self, n: usize) -> Option<(Cell, usize)> {
        let snake = &mut self.herd[n];
        if snake.body.len() < self.length {
            return None;
        }
        let tail = snake.body.pop_front()?;
        let slot = snake.segs.pop_front()?;
        let i = self.index(tail);
        self.used[i] = false;
        Some((tail, slot))
    }

    fn restore(&mut self, n: usize, taken: Option<(Cell, usize)>) {
        if let Some((tail, slot)) = taken {
            let i = self.index(tail);
            self.used[i] = true;
            let snake = &mut self.herd[n];
            snake.body.push_front(tail);
            snake.segs.push_front(slot);
        }
    }

    fn advance(&mut self, n: usize) -> Option<(usize, Sprite)> {
        let mut taken = self.retract(n);
        let mut moved = self.pick(n);
        if moved.is_none() {
            // Boxed in: turn around and leave from the other end.
            self.restore(n, taken);
            let snake = &mut self.herd[n];
            snake.body.make_contiguous().reverse();
            snake.segs.make_contiguous().reverse();
            snake.dir = Self::facing(&snake.body);
            taken = self.retract(n);
            moved = self.pick(n);
        }
        let Some((next, dir)) = moved else {
            // Still stuck: stay put this step.
            self.restore(n, taken);
            return None;
        };
        let i = self.index(next);
        self.used[i] = true;
        let ring = self.ring();
        let snake = &mut self.herd[n];
        let head = *snake.body.back()?;
        let local = match taken {
            Some((_, slot)) => slot,
            None => snake.segs.len(),
        };
        snake.body.push_back(next);
        snake.segs.push_back(local);
        snake.dir = dir;
        let color = snake.color;
        Some((n * ring + local, self.segment(head, next, color)))
    }

    /// The direction a body's head points, from its last two cells.
    fn facing(body: &VecDeque<Cell>) -> usize {
        let len = body.len();
        if len < 2 {
            return 0;
        }
        let (a, b) = (body[len - 2], body[len - 1]);
        DIRS.iter()
            .position(|d| *d == (b.0 - a.0, b.1 - a.1))
            .unwrap_or(0)
    }

    /// The head's next cell and direction, or `None` when every way is
    /// blocked.
    fn pick(&mut self, n: usize) -> Option<(Cell, usize)> {
        let snake = &self.herd[n];
        let head = *snake.body.back()?;
        let dir = snake.dir;
        let left = (dir + 3) % 4;
        let right = (dir + 1) % 4;
        let (first, second) = if self.rng.coin() {
            (left, right)
        } else {
            (right, left)
        };
        let options = if self.rng.below(TURN_ODDS) == 0 {
            [first, dir, second]
        } else {
            [dir, first, second]
        };
        options.into_iter().find_map(|d| {
            let next = (head.0 + DIRS[d].0, head.1 + DIRS[d].1);
            self.free(next).then_some((next, d))
        })
    }

    fn hidden(color: u8) -> Sprite {
        Sprite::new(0.0, 0.0, 0.0, 0.0, Look::Block(color))
    }

    /// The bar joining two adjacent cells, extended by half its width at
    /// each end so turns meet in a square corner.
    fn segment(&self, a: Cell, b: Cell, color: u8) -> Sprite {
        let t = self.cell * THICKNESS;
        let center = |(x, y): Cell| {
            (
                self.left + (x as f32 + 0.5) * self.cell,
                (y as f32 + 0.5) * self.cell,
            )
        };
        let (ax, ay) = center(a);
        let (bx, by) = center(b);
        let (x0, x1) = (ax.min(bx), ax.max(bx));
        let (y0, y1) = (ay.min(by), ay.max(by));
        Sprite::new(
            x0 - t / 2.0,
            y0 - t / 2.0,
            x1 - x0 + t,
            y1 - y0 + t,
            Look::Block(color),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(aspect: f32, low_res: bool, seed: u64, steps: usize) -> (Snakes, Vec<Sprite>) {
        let mut snakes = Snakes::new(aspect, low_res, seed);
        let mut picture = snakes.sprites();
        for _ in 0..steps {
            for (i, s) in snakes.step(1.0 / RATE) {
                picture[i] = s;
            }
        }
        (snakes, picture)
    }

    #[test]
    fn snakes_keep_their_length_and_never_overlap() {
        let (snakes, _) = run(16.0 / 9.0, false, 1, 2000);
        let mut seen = std::collections::HashSet::new();
        for snake in &snakes.herd {
            assert_eq!(snake.body.len(), snakes.length);
            for c in &snake.body {
                assert!(snakes.inside(*c));
                assert!(seen.insert(*c), "cell {c:?} held twice");
            }
            for pair in snake.body.iter().collect::<Vec<_>>().windows(2) {
                let (a, b) = (pair[0], pair[1]);
                assert_eq!((a.0 - b.0).abs() + (a.1 - b.1).abs(), 1);
            }
        }
        assert_eq!(seen.len(), snakes.used.iter().filter(|u| **u).count());
    }

    #[test]
    fn a_step_rewrites_at_most_one_slot_per_snake() {
        let mut snakes = Snakes::new(4.0 / 3.0, true, 2);
        let slots = snakes.sprites().len();
        let ring = snakes.ring();
        for _ in 0..600 {
            let changes = snakes.step(1.0 / RATE);
            assert!(changes.len() <= snakes.herd.len());
            let mut owners: Vec<usize> = changes.iter().map(|(i, _)| i / ring).collect();
            owners.dedup();
            assert_eq!(owners.len(), changes.len());
            assert!(changes.iter().all(|(i, _)| *i < slots));
        }
    }

    #[test]
    fn segments_are_small_square_bars_inside_the_screen() {
        let aspect = 16.0 / 9.0;
        let (snakes, picture) = run(aspect, false, 3, 400);
        let t = snakes.cell * THICKNESS;
        for p in picture {
            assert!(p.x >= 0.0 && p.x + p.w <= aspect && p.y >= 0.0 && p.y + p.h <= 1.0);
            let (long, short) = (p.w.max(p.h), p.w.min(p.h));
            assert!((short - t).abs() < 1e-5, "{p:?}");
            assert!((long - (snakes.cell + t)).abs() < 1e-5, "{p:?}");
            assert!(matches!(p.look, Look::Block(_)));
        }
    }

    #[test]
    fn each_snake_has_its_own_color() {
        let snakes = Snakes::new(16.0 / 9.0, false, 4);
        let mut colors: Vec<u8> = snakes.herd.iter().map(|s| s.color).collect();
        colors.sort_unstable();
        colors.dedup();
        assert_eq!(colors.len(), snakes.herd.len());
    }
}
