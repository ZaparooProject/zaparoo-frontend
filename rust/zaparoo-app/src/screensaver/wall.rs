// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

//! Cover wall: a grid of covers, one tile swapping to another cover every
//! few seconds, and one tile showing the clock that moves to another slot
//! every so often. Each change repaints one or two tiles.

use super::Rng;

/// Seconds between cover swaps.
pub const SWAP_EVERY: f32 = 3.0;
/// Seconds between clock moves.
pub const CLOCK_EVERY: f32 = 60.0;
/// Rows on the wall.
pub const ROWS: usize = 2;
/// Columns per unit of the scene's aspect: four across 16:9, three
/// across 4:3.
const COLUMNS_PER_ASPECT: f32 = 2.25;
/// The gap between tiles, in height units.
pub const GAP: f32 = 0.012;

/// What one tile shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tile {
    Cover(usize),
    Clock,
}

/// The wall's grid, in height units.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Grid {
    pub columns: usize,
    pub rows: usize,
    pub tile_w: f32,
    pub tile_h: f32,
    /// The left and top inset that centers the grid.
    pub left: f32,
    pub top: f32,
}

impl Grid {
    /// Square tiles, `ROWS` high and a few across by the scene's shape,
    /// as large as fit and centered both ways.
    pub fn for_scene(aspect: f32) -> Self {
        let rows = ROWS;
        let columns = ((aspect * COLUMNS_PER_ASPECT).round() as usize).max(1);
        let side = ((1.0 - GAP * (rows as f32 + 1.0)) / rows as f32)
            .min((aspect - GAP * (columns as f32 + 1.0)) / columns as f32);
        let used_w = columns as f32 * (side + GAP) - GAP;
        let used_h = rows as f32 * (side + GAP) - GAP;
        Self {
            columns,
            rows,
            tile_w: side,
            tile_h: side,
            left: (aspect - used_w) / 2.0,
            top: (1.0 - used_h) / 2.0,
        }
    }

    pub fn len(&self) -> usize {
        self.columns * self.rows
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Where a picture `aspect` wide over high sits in tile `i`: as large
    /// as fits, keeping its shape, centered. (x, y, width, height).
    pub fn fit(&self, tile: usize, aspect: f32) -> (f32, f32, f32, f32) {
        let (left, top) = self.origin(tile);
        let aspect = if aspect > 0.0 { aspect } else { 1.0 };
        let (width, height) = if aspect >= self.tile_w / self.tile_h {
            (self.tile_w, self.tile_w / aspect)
        } else {
            (self.tile_h * aspect, self.tile_h)
        };
        (
            left + (self.tile_w - width) / 2.0,
            top + (self.tile_h - height) / 2.0,
            width,
            height,
        )
    }

    /// Tile `i`'s top-left corner.
    pub fn origin(&self, i: usize) -> (f32, f32) {
        let c = i % self.columns;
        let r = i / self.columns;
        (
            self.left + c as f32 * (self.tile_w + GAP),
            self.top + r as f32 * (self.tile_h + GAP),
        )
    }
}

#[derive(Debug, Clone)]
pub struct Wall {
    pub grid: Grid,
    tiles: Vec<Tile>,
    covers: usize,
    swap_t: f32,
    clock_t: f32,
    /// The next cover to bring in, cycling through a shuffled order.
    order: Vec<usize>,
    next: usize,
    rng: Rng,
}

impl Wall {
    pub fn new(grid: Grid, covers: usize, seed: u64) -> Self {
        let mut rng = Rng::new(seed);
        let mut order: Vec<usize> = (0..covers).collect();
        rng.shuffle(&mut order);
        let len = grid.len();
        let mut tiles = Vec::with_capacity(len);
        let mut next = 0;
        for _ in 0..len {
            if covers == 0 {
                tiles.push(Tile::Clock);
                continue;
            }
            tiles.push(Tile::Cover(order[next % covers]));
            next += 1;
        }
        let clock_at = rng.below(len);
        if let Some(tile) = tiles.get_mut(clock_at) {
            *tile = Tile::Clock;
        }
        Self {
            grid,
            tiles,
            covers,
            swap_t: 0.0,
            clock_t: 0.0,
            order,
            next,
            rng,
        }
    }

    pub fn tiles(&self) -> &[Tile] {
        &self.tiles
    }

    /// Advance by `dt`; returns the tiles that changed.
    pub fn step(&mut self, dt: f32) -> Vec<usize> {
        let mut changed = Vec::new();
        if self.covers == 0 || self.tiles.is_empty() {
            return changed;
        }
        self.swap_t += dt;
        self.clock_t += dt;
        if self.swap_t >= SWAP_EVERY {
            self.swap_t -= SWAP_EVERY;
            if let Some(i) = self.swap_one() {
                changed.push(i);
            }
        }
        if self.clock_t >= CLOCK_EVERY && self.tiles.len() > 1 {
            self.clock_t -= CLOCK_EVERY;
            let from = self.tiles.iter().position(|t| *t == Tile::Clock);
            let mut to = self.rng.below(self.tiles.len());
            if Some(to) == from {
                to = (to + 1) % self.tiles.len();
            }
            if let Some(from) = from {
                self.tiles.swap(from, to);
                changed.push(from);
            }
            changed.push(to);
        }
        changed
    }

    /// Swap a random cover tile for a cover not on the wall, if there is
    /// one; else for another cover from elsewhere on the wall.
    fn swap_one(&mut self) -> Option<usize> {
        let candidates: Vec<usize> = (0..self.tiles.len())
            .filter(|i| matches!(self.tiles[*i], Tile::Cover(_)))
            .collect();
        if candidates.is_empty() {
            return None;
        }
        let at = candidates[self.rng.below(candidates.len())];
        let current = self.tiles[at];
        for _ in 0..self.covers {
            let cover = self.order[self.next % self.covers];
            self.next += 1;
            if Tile::Cover(cover) != current
                && (self.covers <= self.tiles.len() || !self.tiles.contains(&Tile::Cover(cover)))
            {
                self.tiles[at] = Tile::Cover(cover);
                return Some(at);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_fits_the_scene() {
        for (aspect, columns) in [(16.0 / 9.0, 4), (4.0 / 3.0, 3), (16.0 / 10.0, 4)] {
            let g = Grid::for_scene(aspect);
            assert_eq!((g.columns, g.rows), (columns, ROWS));
            assert!((g.tile_w - g.tile_h).abs() < 1e-6, "square tiles");
            let (x, y) = g.origin(g.len() - 1);
            assert!(x + g.tile_w <= aspect + 1e-4);
            assert!(y + g.tile_h <= 1.0 + 1e-4);
            assert!(g.left >= 0.0 && g.top >= 0.0);
        }
    }

    #[test]
    fn a_cover_fills_its_square_and_keeps_its_shape() {
        let grid = Grid::for_scene(16.0 / 9.0);
        let (ox, oy) = grid.origin(5);
        for aspect in [0.7, 1.0, 1.4] {
            let (x, y, w, h) = grid.fit(5, aspect);
            assert!((w / h - aspect).abs() < 1e-4);
            assert!(
                (w - grid.tile_w).abs() < 1e-6 || (h - grid.tile_h).abs() < 1e-6,
                "touches two sides"
            );
            assert!(x >= ox - 1e-6 && y >= oy - 1e-6);
            assert!(x + w <= ox + grid.tile_w + 1e-6 && y + h <= oy + grid.tile_h + 1e-6);
            assert!(
                ((x - ox) - (ox + grid.tile_w - x - w)).abs() < 1e-5,
                "centered"
            );
        }
    }

    #[test]
    fn one_clock_always_and_one_tile_per_swap() {
        let grid = Grid::for_scene(16.0 / 9.0);
        let mut wall = Wall::new(grid, 40, 5);
        for _ in 0..2000 {
            let changed = wall.step(0.5);
            assert!(changed.len() <= 3);
            assert_eq!(
                wall.tiles().iter().filter(|t| **t == Tile::Clock).count(),
                1
            );
        }
    }

    #[test]
    fn the_clock_moves() {
        let grid = Grid::for_scene(16.0 / 9.0);
        let mut wall = Wall::new(grid, 12, 6);
        let at = |w: &Wall| w.tiles().iter().position(|t| *t == Tile::Clock);
        let start = at(&wall);
        wall.step(CLOCK_EVERY + 0.01);
        assert_ne!(at(&wall), start);
    }

    #[test]
    fn many_covers_keep_the_wall_free_of_duplicates() {
        let grid = Grid::for_scene(16.0 / 9.0);
        let mut wall = Wall::new(grid, 200, 8);
        for _ in 0..500 {
            wall.step(SWAP_EVERY);
            let mut covers: Vec<_> = wall
                .tiles()
                .iter()
                .filter_map(|t| match t {
                    Tile::Cover(c) => Some(*c),
                    Tile::Clock => None,
                })
                .collect();
            let n = covers.len();
            covers.sort_unstable();
            covers.dedup();
            assert_eq!(covers.len(), n);
        }
    }
}
