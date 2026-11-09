// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

//! Maze: a first-person walk through a generated maze, drawn by a
//! raycaster into a small RGB buffer the view scales up. Walls are flat
//! palette colors shaded by distance, over a flat floor and ceiling; some
//! walls carry framed covers, and spinning Zaparoo logos float at some
//! junctions. Walking through one turns the view upside down (and the
//! next one back). The walker keeps its right hand on the wall, which
//! always finds the exit, and a new maze starts there.

use super::{slot_rgb, Rng};

/// Tiles a second, walking.
pub const WALK_SPEED: f32 = 1.7;
/// Seconds per quarter turn.
pub const TURN_SECONDS: f32 = 0.5;
/// Logos placed per maze, at most.
pub const MAX_LOGOS: usize = 5;
/// Framed covers per maze, at most.
pub const MAX_COVERS: usize = 10;
/// The camera plane's half width: about a 66 degree view.
const PLANE: f32 = 0.66;
const CEILING: (u8, u8, u8) = (0x0d, 0x0e, 0x16);
const FLOOR: (u8, u8, u8) = (0x1c, 0x18, 0x16);
const FRAME: (u8, u8, u8) = (0xe8, 0xe4, 0xdc);

/// Which face of a wall cell a ray hit (the side facing the camera), and
/// where across it, 0 to 1 left to right as seen from in front. `ns` is
/// a hit while stepping along y, `east` and `south` are the ray's
/// directions, and `along` is the hit's world coordinate along the face.
/// The camera plane points to the screen's right, a quarter turn from
/// the view direction (`Maze::camera`), so the world coordinate runs
/// left to right on faces 1 and 2 and right to left on faces 0 and 3,
/// which are read backwards.
fn face_and_u(ns: bool, east: bool, south: bool, along: f32) -> (usize, f32) {
    let face = match (ns, east, south) {
        (true, _, true) => 3,
        (true, _, false) => 1,
        (false, true, _) => 2,
        (false, false, _) => 0,
    };
    let u = along - along.floor();
    (face, if face == 0 || face == 3 { 1.0 - u } else { u })
}

/// An image the raycaster samples: RGBA, row-major.
#[derive(Debug, Clone, Default)]
pub struct Texture {
    pub width: usize,
    pub height: usize,
    pub rgba: Vec<u8>,
}

impl Texture {
    fn sample(&self, u: f32, v: f32) -> [u8; 4] {
        if self.width == 0 || self.height == 0 {
            return [0, 0, 0, 0];
        }
        let col = (u.clamp(0.0, 0.9999) * self.width as f32) as usize;
        let row = (v.clamp(0.0, 0.9999) * self.height as f32) as usize;
        let at = (row * self.width + col) * 4;
        [
            self.rgba[at],
            self.rgba[at + 1],
            self.rgba[at + 2],
            self.rgba[at + 3],
        ]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Action {
    Forward,
    Left,
    Right,
}

/// A generated maze on a tile map: odd tiles are cells, walls between.
#[derive(Debug, Clone)]
pub struct Map {
    pub width: usize,
    pub height: usize,
    /// True for a wall tile.
    pub walls: Vec<bool>,
}

impl Map {
    /// A perfect maze of `cells_w` by `cells_h` cells (recursive
    /// backtracker), entered at the top left, left at the bottom right.
    pub fn generate(cells_w: usize, cells_h: usize, rng: &mut Rng) -> Self {
        let width = cells_w * 2 + 1;
        let height = cells_h * 2 + 1;
        let mut walls = vec![true; width * height];
        let mut seen = vec![false; cells_w * cells_h];
        let mut stack = vec![(0usize, 0usize)];
        seen[0] = true;
        walls[width + 1] = false;
        while let Some(&(cx, cy)) = stack.last() {
            let mut next = Vec::with_capacity(4);
            if cx > 0 && !seen[cy * cells_w + cx - 1] {
                next.push((cx - 1, cy));
            }
            if cx + 1 < cells_w && !seen[cy * cells_w + cx + 1] {
                next.push((cx + 1, cy));
            }
            if cy > 0 && !seen[(cy - 1) * cells_w + cx] {
                next.push((cx, cy - 1));
            }
            if cy + 1 < cells_h && !seen[(cy + 1) * cells_w + cx] {
                next.push((cx, cy + 1));
            }
            if next.is_empty() {
                stack.pop();
                continue;
            }
            let (nx, ny) = next[rng.below(next.len())];
            seen[ny * cells_w + nx] = true;
            let (tx, ty) = (cx * 2 + 1, cy * 2 + 1);
            let (ux, uy) = (nx * 2 + 1, ny * 2 + 1);
            walls[ty.midpoint(uy) * width + tx.midpoint(ux)] = false;
            walls[uy * width + ux] = false;
            stack.push((nx, ny));
        }
        Self {
            width,
            height,
            walls,
        }
    }

    pub fn wall(&self, x: i32, y: i32) -> bool {
        x < 0
            || y < 0
            || x as usize >= self.width
            || y as usize >= self.height
            || self.walls[y as usize * self.width + x as usize]
    }

    pub fn start(&self) -> (i32, i32) {
        (1, 1)
    }

    pub fn exit(&self) -> (i32, i32) {
        (self.width as i32 - 2, self.height as i32 - 2)
    }

    fn open_neighbors(&self, (x, y): (i32, i32)) -> usize {
        DIRS.iter()
            .filter(|(dx, dy)| !self.wall(x + dx, y + dy))
            .count()
    }
}

/// East, south, west, north: a right turn is the next one.
const DIRS: [(i32, i32); 4] = [(1, 0), (0, 1), (-1, 0), (0, -1)];

fn ahead(at: (i32, i32), heading: usize) -> (i32, i32) {
    (at.0 + DIRS[heading].0, at.1 + DIRS[heading].1)
}

/// The right-hand walk from the start to the exit, as actions, starting
/// facing `heading`.
fn wall_follow(map: &Map, heading: usize) -> Vec<Action> {
    let mut actions = Vec::new();
    let mut at = map.start();
    let mut h = heading;
    let limit = map.width * map.height * 8;
    while at != map.exit() && actions.len() < limit {
        let right = (h + 1) % 4;
        let front = ahead(at, h);
        let to_right = ahead(at, right);
        if !map.wall(to_right.0, to_right.1) {
            actions.push(Action::Right);
            h = right;
            actions.push(Action::Forward);
            at = to_right;
        } else if !map.wall(front.0, front.1) {
            actions.push(Action::Forward);
            at = front;
        } else {
            actions.push(Action::Left);
            h = (h + 3) % 4;
        }
    }
    actions
}

#[derive(Debug, Clone)]
struct Logo {
    tile: (i32, i32),
    /// Spin phase offset.
    phase: f32,
}

/// The camera's basis for one frame.
#[derive(Debug, Clone, Copy)]
struct Camera {
    pos: (f32, f32),
    dir: (f32, f32),
    plane: (f32, f32),
}

#[derive(Debug, Clone)]
pub struct Maze {
    pub map: Map,
    actions: Vec<Action>,
    action: usize,
    /// Seconds into the current action.
    t: f32,
    /// Tile and heading at the start of the current action.
    at: (i32, i32),
    heading: usize,
    logos: Vec<Logo>,
    /// Covers on wall faces: (wall tile, face direction index, cover).
    covers: Vec<((i32, i32), usize, usize)>,
    cover_count: usize,
    flipped: bool,
    wall_slot: u8,
    clock: f32,
    cells: (usize, usize),
    rng: Rng,
}

impl Maze {
    /// A maze of `cells` (width, height) with `covers` covers to hang.
    pub fn new(cells: (usize, usize), covers: usize, seed: u64) -> Self {
        let mut rng = Rng::new(seed);
        let map = Map::generate(cells.0, cells.1, &mut rng);
        let mut maze = Self {
            map,
            actions: Vec::new(),
            action: 0,
            t: 0.0,
            at: (1, 1),
            heading: 0,
            logos: Vec::new(),
            covers: Vec::new(),
            cover_count: covers,
            flipped: false,
            wall_slot: 0,
            clock: 0.0,
            cells,
            rng,
        };
        maze.furnish();
        maze
    }

    /// The walk's tiles, in order, start included.
    fn path(&self) -> Vec<(i32, i32)> {
        let mut path = vec![self.at];
        let mut at = self.at;
        let mut h = self.heading;
        for a in &self.actions {
            match a {
                Action::Forward => {
                    at = ahead(at, h);
                    path.push(at);
                }
                Action::Left => h = (h + 3) % 4,
                Action::Right => h = (h + 1) % 4,
            }
        }
        path
    }

    fn furnish(&mut self) {
        self.at = self.map.start();
        // Face south when east is a wall.
        self.heading = usize::from(self.map.wall(2, 1));
        self.actions = wall_follow(&self.map, self.heading);
        self.action = 0;
        self.t = 0.0;
        self.wall_slot = self.rng.below(super::SLOTS as usize) as u8;
        let path = self.path();
        // Logos at junctions along the way.
        let mut junctions: Vec<(i32, i32)> = path
            .iter()
            .copied()
            .filter(|t| *t != self.map.start() && self.map.open_neighbors(*t) >= 3)
            .collect();
        junctions.sort_unstable();
        junctions.dedup();
        self.rng.shuffle(&mut junctions);
        junctions.truncate(MAX_LOGOS);
        self.logos = junctions
            .into_iter()
            .map(|tile| Logo { tile, phase: 0.0 })
            .collect();
        for logo in &mut self.logos {
            logo.phase = self.rng.range(0.0, std::f32::consts::TAU);
        }
        // Covers on walls beside the path, facing it.
        self.covers.clear();
        if self.cover_count == 0 {
            return;
        }
        let mut faces = Vec::new();
        for t in &path {
            for (d, (dx, dy)) in DIRS.iter().enumerate() {
                let wall = (t.0 + dx, t.1 + dy);
                let inside = wall.0 > 0
                    && wall.1 > 0
                    && wall.0 < self.map.width as i32 - 1
                    && wall.1 < self.map.height as i32 - 1;
                if inside && self.map.wall(wall.0, wall.1) {
                    // The face of `wall` looking back at `t`.
                    faces.push((wall, (d + 2) % 4));
                }
            }
        }
        faces.sort_unstable();
        faces.dedup();
        self.rng.shuffle(&mut faces);
        faces.truncate(MAX_COVERS);
        let first = self.rng.below(self.cover_count);
        for (i, (wall, face)) in faces.into_iter().enumerate() {
            self.covers
                .push((wall, face, (first + i) % self.cover_count));
        }
    }

    /// Advance by `dt` seconds.
    pub fn step(&mut self, dt: f32) {
        self.clock += dt;
        self.t += dt;
        loop {
            let Some(action) = self.actions.get(self.action).copied() else {
                // At the exit: a new maze.
                self.map = Map::generate(self.cells.0, self.cells.1, &mut self.rng);
                self.furnish();
                return;
            };
            let length = match action {
                Action::Forward => 1.0 / WALK_SPEED,
                _ => TURN_SECONDS,
            };
            if self.t < length {
                break;
            }
            self.t -= length;
            match action {
                Action::Forward => {
                    self.at = ahead(self.at, self.heading);
                    if let Some(i) = self.logos.iter().position(|l| l.tile == self.at) {
                        self.logos.remove(i);
                        self.flipped = !self.flipped;
                    }
                }
                Action::Left => self.heading = (self.heading + 3) % 4,
                Action::Right => self.heading = (self.heading + 1) % 4,
            }
            self.action += 1;
        }
    }

    /// The camera for this moment, mid-step or mid-turn.
    fn camera(&self) -> Camera {
        let base = (self.at.0 as f32 + 0.5, self.at.1 as f32 + 0.5);
        let mut angle = self.heading as f32 * std::f32::consts::FRAC_PI_2;
        let mut pos = base;
        let ease = |t: f32| t * t * (3.0 - 2.0 * t);
        let turn = ease((self.t / TURN_SECONDS).clamp(0.0, 1.0)) * std::f32::consts::FRAC_PI_2;
        match self.actions.get(self.action) {
            Some(Action::Forward) => {
                let k = ease((self.t * WALK_SPEED).clamp(0.0, 1.0));
                let (dx, dy) = DIRS[self.heading];
                pos = (base.0 + dx as f32 * k, base.1 + dy as f32 * k);
            }
            Some(Action::Left) => angle -= turn,
            Some(Action::Right) => angle += turn,
            None => {}
        }
        let dir = (angle.cos(), angle.sin());
        Camera {
            pos,
            dir,
            plane: (-dir.1 * PLANE, dir.0 * PLANE),
        }
    }

    pub fn flipped(&self) -> bool {
        self.flipped
    }

    pub fn logos_left(&self) -> usize {
        self.logos.len()
    }

    /// Draw the view into `out`, RGB at 3 bytes a pixel, `width` by
    /// `height`. `covers` are the cover textures (indexes as given to
    /// `new`), `logo` the logo's. `depth` is scratch, one per column.
    pub fn render(
        &self,
        out: &mut [u8],
        (width, height): (usize, usize),
        covers: &[Texture],
        logo: &Texture,
        depth: &mut Vec<f32>,
    ) {
        if width == 0 || height == 0 || out.len() < width * height * 3 {
            return;
        }
        depth.clear();
        let cam = self.camera();
        for x in 0..width {
            let dist = self.column(out, (width, height), x, cam, covers);
            depth.push(dist);
        }
        self.render_logos(out, (width, height), logo, depth, cam);
    }

    fn put(
        &self,
        out: &mut [u8],
        (width, height): (usize, usize),
        x: usize,
        y: usize,
        c: (u8, u8, u8),
    ) {
        let row = if self.flipped { height - 1 - y } else { y };
        let at = (row * width + x) * 3;
        out[at] = c.0;
        out[at + 1] = c.1;
        out[at + 2] = c.2;
    }

    /// Cast one column's ray, draw the column, and return the wall's
    /// distance.
    #[allow(
        clippy::many_single_char_names,
        clippy::too_many_lines,
        reason = "raycasting reads best in the textbook's names, as one pass per column"
    )]
    fn column(
        &self,
        out: &mut [u8],
        (width, height): (usize, usize),
        x: usize,
        cam: Camera,
        covers: &[Texture],
    ) -> f32 {
        let (px, py) = cam.pos;
        let camera_x = 2.0 * x as f32 / width as f32 - 1.0;
        let (rx, ry) = (
            cam.dir.0 + cam.plane.0 * camera_x,
            cam.dir.1 + cam.plane.1 * camera_x,
        );
        let (mut mx, mut my) = (px.floor() as i32, py.floor() as i32);
        let ddx = if rx == 0.0 {
            f32::MAX
        } else {
            (1.0 / rx).abs()
        };
        let ddy = if ry == 0.0 {
            f32::MAX
        } else {
            (1.0 / ry).abs()
        };
        let (step_x, mut side_x) = if rx < 0.0 {
            (-1, (px - mx as f32) * ddx)
        } else {
            (1, (mx as f32 + 1.0 - px) * ddx)
        };
        let (step_y, mut side_y) = if ry < 0.0 {
            (-1, (py - my as f32) * ddy)
        } else {
            (1, (my as f32 + 1.0 - py) * ddy)
        };
        let mut ns = false;
        let mut hit = false;
        for _ in 0..256 {
            if side_x < side_y {
                side_x += ddx;
                mx += step_x;
                ns = false;
            } else {
                side_y += ddy;
                my += step_y;
                ns = true;
            }
            if self.map.wall(mx, my) {
                hit = true;
                break;
            }
        }
        let dist = if !hit {
            f32::MAX
        } else if ns {
            (side_y - ddy).max(0.01)
        } else {
            (side_x - ddx).max(0.01)
        };
        let half = height as f32 / 2.0;
        let line = (height as f32 / dist).min(height as f32 * 8.0);
        let top = half - line / 2.0;
        let y0 = top.max(0.0) as usize;
        let y1 = ((half + line / 2.0).max(0.0) as usize).min(height);
        let along = if ns { px + dist * rx } else { py + dist * ry };
        let (face, u) = face_and_u(ns, step_x > 0, step_y > 0, along);
        let cover = self
            .covers
            .iter()
            .find(|(t, f, _)| *t == (mx, my) && *f == face)
            .and_then(|(_, _, c)| covers.get(*c));
        let shade = (1.0 / (1.0 + dist * 0.22)).clamp(0.12, 1.0) * if ns { 0.78 } else { 1.0 };
        let shaded = |c: (u8, u8, u8)| {
            (
                (f32::from(c.0) * shade) as u8,
                (f32::from(c.1) * shade) as u8,
                (f32::from(c.2) * shade) as u8,
            )
        };
        let flat = shaded(slot_rgb(self.wall_slot));
        for y in 0..height {
            let c = if y < y0 {
                CEILING
            } else if y >= y1 {
                FLOOR
            } else if let Some(tex) = cover {
                let v = (y as f32 - top) / line;
                // A light frame around the cover.
                let (fu, fv) = ((u - 0.18) / 0.64, (v - 0.14) / 0.6);
                if (0.0..1.0).contains(&fu) && (0.0..1.0).contains(&fv) {
                    let s = tex.sample(fu, fv);
                    shaded((s[0], s[1], s[2]))
                } else if (-0.05..1.05).contains(&fu) && (-0.06..1.06).contains(&fv) {
                    shaded(FRAME)
                } else {
                    flat
                }
            } else {
                flat
            };
            self.put(out, (width, height), x, y, c);
        }
        dist
    }

    /// The logos as spinning billboards, far to near, hidden behind walls.
    fn render_logos(
        &self,
        out: &mut [u8],
        size: (usize, usize),
        logo: &Texture,
        depth: &[f32],
        cam: Camera,
    ) {
        if logo.width == 0 || logo.height == 0 {
            return;
        }
        let (px, py) = cam.pos;
        let inv = 1.0 / (cam.plane.0 * cam.dir.1 - cam.dir.0 * cam.plane.1);
        let dist2 = |l: &Logo| {
            let (lx, ly) = (l.tile.0 as f32 + 0.5 - px, l.tile.1 as f32 + 0.5 - py);
            lx * lx + ly * ly
        };
        let mut order: Vec<&Logo> = self.logos.iter().collect();
        order.sort_by(|a, b| dist2(b).total_cmp(&dist2(a)));
        let aspect = logo.width as f32 / logo.height as f32;
        for l in order {
            let (sx, sy) = (l.tile.0 as f32 + 0.5 - px, l.tile.1 as f32 + 0.5 - py);
            let across = inv * (cam.dir.1 * sx - cam.dir.0 * sy);
            let into = inv * (-cam.plane.1 * sx + cam.plane.0 * sy);
            if into <= 0.1 {
                continue;
            }
            let spin = (self.clock * 2.2 + l.phase).cos();
            let tall = size.1 as f32 / into * 0.32;
            let wide = tall * aspect * spin.abs();
            if wide < 1.0 {
                continue;
            }
            let left = (size.0 as f32 / 2.0) * (1.0 + across / into) - wide / 2.0;
            let top = size.1 as f32 / 2.0 - tall / 2.0;
            self.billboard(
                out,
                size,
                logo,
                depth,
                (left, top, wide, tall),
                into,
                spin < 0.0,
            );
        }
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "one billboard: target, texture, depth and its box"
    )]
    fn billboard(
        &self,
        out: &mut [u8],
        size: (usize, usize),
        logo: &Texture,
        depth: &[f32],
        (left, top, wide, tall): (f32, f32, f32, f32),
        into: f32,
        back: bool,
    ) {
        let k = if back { 0.55 } else { 1.0 };
        let xs = left.max(0.0) as usize;
        let xe = ((left + wide).max(0.0) as usize).min(size.0);
        let ys = top.max(0.0) as usize;
        let ye = ((top + tall).max(0.0) as usize).min(size.1);
        for (col, wall) in depth.iter().enumerate().take(xe).skip(xs) {
            if into >= *wall {
                continue;
            }
            let mut u = (col as f32 - left) / wide;
            if back {
                u = 1.0 - u;
            }
            for row in ys..ye {
                let s = logo.sample(u, (row as f32 - top) / tall);
                if s[3] < 128 {
                    continue;
                }
                let rgb = (
                    (f32::from(s[0]) * k) as u8,
                    (f32::from(s[1]) * k) as u8,
                    (f32::from(s[2]) * k) as u8,
                );
                self.put(out, size, col, row, rgb);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mazes_are_perfect_and_the_walk_reaches_the_exit() {
        for seed in 0..20 {
            let mut rng = Rng::new(seed);
            let map = Map::generate(9, 6, &mut rng);
            // Perfect: open tiles are the cells plus one passage fewer.
            let open = map.walls.iter().filter(|w| !**w).count();
            assert_eq!(open, 9 * 6 * 2 - 1);
            let actions = wall_follow(&map, 0);
            let mut at = map.start();
            let mut h = 0;
            for a in &actions {
                match a {
                    Action::Forward => {
                        at = ahead(at, h);
                        assert!(!map.wall(at.0, at.1));
                    }
                    Action::Left => h = (h + 3) % 4,
                    Action::Right => h = (h + 1) % 4,
                }
            }
            assert_eq!(at, map.exit());
        }
    }

    #[test]
    fn walking_through_a_logo_flips_the_view_and_a_new_maze_follows() {
        let mut maze = Maze::new((7, 5), 3, 4);
        let logos = maze.logos_left();
        assert!(logos > 0);
        let first = maze.map.walls.clone();
        let mut flips = 0;
        let mut was = maze.flipped();
        let mut renewed = false;
        for _ in 0..(30 * 600) {
            maze.step(1.0 / 30.0);
            if maze.map.walls != first {
                renewed = true;
                break;
            }
            if maze.flipped() != was {
                flips += 1;
                was = maze.flipped();
            }
        }
        assert!(renewed);
        assert_eq!(flips, logos);
    }

    #[test]
    fn covers_read_left_to_right_on_every_face() {
        // From the middle of a cell, look at the wall one cell away in
        // each direction; the left of the view must sample the left of
        // the face.
        for dir in [(1.0_f32, 0.0_f32), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0)] {
            let plane = (-dir.1 * PLANE, dir.0 * PLANE);
            let u_at = |camera_x: f32| {
                let ray = (dir.0 + plane.0 * camera_x, dir.1 + plane.1 * camera_x);
                let ns = dir.0 == 0.0;
                // Distance to the face 1.5 cells ahead along the view axis.
                let dist = if ns {
                    1.0 / ray.1.abs()
                } else {
                    1.0 / ray.0.abs()
                };
                let along = if ns {
                    0.5 + dist * ray.0
                } else {
                    0.5 + dist * ray.1
                };
                face_and_u(ns, ray.0 > 0.0, ray.1 > 0.0, along).1
            };
            assert!(u_at(-0.5) < u_at(0.5), "mirrored looking {dir:?}");
        }
    }

    #[test]
    fn a_frame_has_ceiling_walls_and_floor() {
        let maze = Maze::new((5, 4), 2, 7);
        let (w, h) = (64, 40);
        let mut out = vec![0u8; w * h * 3];
        let tex = Texture {
            width: 2,
            height: 2,
            rgba: vec![255; 16],
        };
        let mut depth = Vec::new();
        maze.render(
            &mut out,
            (w, h),
            &[tex.clone(), tex.clone()],
            &tex,
            &mut depth,
        );
        let px = |x: usize, y: usize| {
            let i = (y * w + x) * 3;
            (out[i], out[i + 1], out[i + 2])
        };
        assert_eq!(px(w / 2, 0), CEILING);
        assert_eq!(px(w / 2, h - 1), FLOOR);
        assert_ne!(px(w / 2, h / 2), (0, 0, 0));
        assert_eq!(depth.len(), w);
        assert!(depth.iter().all(|d| *d < 100.0));
        // Same seed, same frame.
        let mut again = vec![0u8; w * h * 3];
        Maze::new((5, 4), 2, 7).render(
            &mut again,
            (w, h),
            &[tex.clone(), tex.clone()],
            &tex,
            &mut depth,
        );
        assert_eq!(out, again);
    }
}
