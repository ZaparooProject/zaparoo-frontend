// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

//! Maze: a first-person walk through a generated maze, drawn by a
//! raycaster into a small RGB buffer the view scales up. Walls, floor and
//! ceiling carry textures generated for each maze, shaded by distance;
//! some walls hang framed covers, spinning Zaparoo logos float at some
//! junctions, and a system logo or two scurries along the corridors.
//! Walking through a Zaparoo logo rolls the view upside down (and the
//! next one back). The walker keeps its right hand on the wall, which
//! always finds the exit, gliding round corners without stopping, and a
//! new maze starts there.

use super::{slot_rgb, Rng};

/// Tiles a second, walking.
pub const WALK_SPEED: f32 = 1.6;
/// Seconds to turn round at a dead end.
pub const TURN_SECONDS: f32 = 1.1;
/// Seconds the view takes to roll over.
pub const ROLL_SECONDS: f32 = 1.4;
/// Logos placed per maze, at most.
pub const MAX_LOGOS: usize = 5;
/// Framed covers per maze, at most.
pub const MAX_COVERS: usize = 36;
/// System logos running about, at most.
pub const MAX_RATS: usize = 2;
/// Tiles a second, running.
const RAT_SPEED: f32 = 2.4;
/// The camera plane's half width: about a 66 degree view.
const PLANE: f32 = 0.66;
const FRAME: [u8; 3] = [0xe8, 0xe4, 0xdc];
/// The generated textures' side, a power of two.
const TEX: usize = 64;
/// The walk's length through a corner tile (a quarter curve).
const CORNER_LEN: f32 = 0.81;
/// How near the walker comes to a logo before it rolls the view.
const LOGO_REACH: f32 = 0.35;

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

/// A generated surface: `TEX` by `TEX` RGB, tiling seamlessly.
#[derive(Debug, Clone)]
struct Surface(Vec<[u8; 3]>);

impl Surface {
    fn new(mut texel: impl FnMut(usize, usize) -> [u8; 3]) -> Self {
        let mut px = Vec::with_capacity(TEX * TEX);
        for y in 0..TEX {
            for x in 0..TEX {
                px.push(texel(x, y));
            }
        }
        Self(px)
    }

    fn at(&self, x: usize, y: usize) -> [u8; 3] {
        self.0[(y & (TEX - 1)) * TEX + (x & (TEX - 1))]
    }
}

fn scale(c: [u8; 3], k: f32) -> [u8; 3] {
    let f = |v: u8| (f32::from(v) * k).clamp(0.0, 255.0) as u8;
    [f(c[0]), f(c[1]), f(c[2])]
}

/// Shade by `k`, 0 to 256.
fn shade(c: [u8; 3], k: u32) -> [u8; 3] {
    let f = |v: u8| ((u32::from(v) * k) >> 8) as u8;
    [f(c[0]), f(c[1]), f(c[2])]
}

/// Brick walls in `base`: offset courses, per-brick tone, speckle and
/// dark mortar.
fn bricks(base: [u8; 3], rng: &mut Rng) -> Surface {
    const COURSE: usize = 8;
    const BRICK: usize = 16;
    let tones: Vec<f32> = (0..(TEX / COURSE) * (TEX / BRICK))
        .map(|_| rng.range(0.72, 1.0))
        .collect();
    let mut grain = rng.clone();
    Surface::new(|x, y| {
        let course = y / COURSE;
        let shifted = (x + if course % 2 == 1 { BRICK / 2 } else { 0 }) % TEX;
        let speckle = grain.range(0.88, 1.0);
        if y % COURSE == COURSE - 1 || shifted % BRICK == BRICK - 1 {
            return scale(base, 0.28 * speckle);
        }
        let tone = tones[course * (TEX / BRICK) + shifted / BRICK];
        // A lit top edge on every brick.
        let lip = if y % COURSE == 0 { 1.12 } else { 1.0 };
        scale(base, 0.82 * tone * speckle * lip)
    })
}

/// Flagstones: four to a tile in two tones, dark joints, speckle.
fn flagstones(rng: &mut Rng) -> Surface {
    const STONE: usize = TEX / 2;
    let light = [0x4a, 0x42, 0x3a];
    let dark = [0x3a, 0x34, 0x2f];
    let mut grain = rng.clone();
    Surface::new(|x, y| {
        let speckle = grain.range(0.84, 1.0);
        if x % STONE == 0 || y % STONE == 0 {
            return scale(dark, 0.45 * speckle);
        }
        let base = if (x / STONE + y / STONE).is_multiple_of(2) {
            light
        } else {
            dark
        };
        scale(base, speckle)
    })
}

/// Ceiling panels: a two by two grid, each lit on its top and left edge
/// and shadowed on the others.
fn panels(rng: &mut Rng) -> Surface {
    const PANEL: usize = TEX / 2;
    let base = [0x24, 0x27, 0x36];
    let mut grain = rng.clone();
    Surface::new(|x, y| {
        let (px, py) = (x % PANEL, y % PANEL);
        let speckle = grain.range(0.92, 1.0);
        let k = if px == 0 || py == 0 {
            0.4
        } else if px == 1 || py == 1 {
            1.45
        } else if px == PANEL - 1 || py == PANEL - 1 {
            0.7
        } else {
            1.0
        };
        scale(base, k * speckle)
    })
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

fn center(t: (i32, i32)) -> (f32, f32) {
    (t.0 as f32 + 0.5, t.1 as f32 + 0.5)
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

/// The walk's tiles from `start` facing `heading`, in order.
fn walk_tiles(start: (i32, i32), heading: usize, actions: &[Action]) -> Vec<(i32, i32)> {
    let mut path = vec![start];
    let mut at = start;
    let mut h = heading;
    for a in actions {
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

/// One stretch of the walk.
#[derive(Debug, Clone, Copy)]
enum Piece {
    /// A quadratic curve: in at `a`, pulled toward `c`, out at `b`. A
    /// straight stretch has `c` halfway; a corner has it at the tile's
    /// center, so the walker rounds it without stopping.
    Curve {
        a: (f32, f32),
        c: (f32, f32),
        b: (f32, f32),
        len: f32,
    },
    /// Turning on the spot, from `from` radians by `sweep`.
    Spin {
        at: (f32, f32),
        from: f32,
        sweep: f32,
    },
}

impl Piece {
    fn line(a: (f32, f32), b: (f32, f32)) -> Self {
        let len = ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt();
        Self::Curve {
            a,
            c: (a.0.midpoint(b.0), a.1.midpoint(b.1)),
            b,
            len,
        }
    }

    fn seconds(self) -> f32 {
        match self {
            Self::Curve { len, .. } => len / WALK_SPEED,
            Self::Spin { sweep, .. } => sweep.abs() / std::f32::consts::PI * TURN_SECONDS,
        }
    }

    /// Where the walker is and which way it looks, `k` (0 to 1) through.
    fn at(self, k: f32) -> ((f32, f32), f32) {
        match self {
            Self::Curve { a, c, b, .. } => {
                let j = 1.0 - k;
                let pos = (
                    j * j * a.0 + 2.0 * k * j * c.0 + k * k * b.0,
                    j * j * a.1 + 2.0 * k * j * c.1 + k * k * b.1,
                );
                let d = (
                    j * (c.0 - a.0) + k * (b.0 - c.0),
                    j * (c.1 - a.1) + k * (b.1 - c.1),
                );
                (pos, d.1.atan2(d.0))
            }
            Self::Spin { at, from, sweep } => {
                let eased = k * k * (3.0 - 2.0 * k);
                (at, from + sweep * eased)
            }
        }
    }
}

/// The signed turn from `from` to `to`, a half turn going left.
fn turn_between(from: f32, to: f32) -> f32 {
    use std::f32::consts::{PI, TAU};
    let mut d = (to - from).rem_euclid(TAU);
    if d > PI + 0.01 {
        d -= TAU;
    }
    if (d - PI).abs() < 0.01 {
        -PI
    } else {
        d
    }
}

/// The walk through `tiles` as continuous pieces, starting facing
/// `heading`.
fn pieces(tiles: &[(i32, i32)], heading: usize) -> Vec<Piece> {
    let mut out = Vec::new();
    if tiles.len() < 2 {
        return out;
    }
    let dir = |i: usize| {
        (
            (tiles[i + 1].0 - tiles[i].0) as f32,
            (tiles[i + 1].1 - tiles[i].1) as f32,
        )
    };
    let angle = |d: (f32, f32)| d.1.atan2(d.0);
    let half = |c: (f32, f32), d: (f32, f32), k: f32| (c.0 + d.0 * k, c.1 + d.1 * k);
    let c0 = center(tiles[0]);
    let start = heading as f32 * std::f32::consts::FRAC_PI_2;
    let sweep = turn_between(start, angle(dir(0)));
    if sweep.abs() > 0.01 {
        out.push(Piece::Spin {
            at: c0,
            from: start,
            sweep,
        });
    }
    out.push(Piece::line(c0, half(c0, dir(0), 0.5)));
    for (i, tile) in tiles.iter().enumerate().take(tiles.len() - 1).skip(1) {
        let (din, dout) = (dir(i - 1), dir(i));
        let c = center(*tile);
        let a = half(c, din, -0.5);
        let b = half(c, dout, 0.5);
        if din == dout {
            out.push(Piece::line(a, b));
        } else if din == (-dout.0, -dout.1) {
            out.push(Piece::line(a, c));
            out.push(Piece::Spin {
                at: c,
                from: angle(din),
                sweep: -std::f32::consts::PI,
            });
            out.push(Piece::line(c, b));
        } else {
            out.push(Piece::Curve {
                a,
                c,
                b,
                len: CORNER_LEN,
            });
        }
    }
    let last = tiles.len() - 1;
    let c = center(tiles[last]);
    out.push(Piece::line(half(c, dir(last - 1), -0.5), c));
    out
}

#[derive(Debug, Clone)]
struct Logo {
    tile: (i32, i32),
    /// Spin phase offset.
    phase: f32,
}

/// A system logo running about: from one tile to the next, `t` through.
#[derive(Debug, Clone)]
struct Rat {
    from: (i32, i32),
    to: (i32, i32),
    t: f32,
    face: usize,
    phase: f32,
}

impl Rat {
    fn pos(&self) -> (f32, f32) {
        let (a, b) = (center(self.from), center(self.to));
        (a.0 + (b.0 - a.0) * self.t, a.1 + (b.1 - a.1) * self.t)
    }
}

/// The camera's basis for one frame.
#[derive(Debug, Clone, Copy)]
struct Camera {
    pos: (f32, f32),
    dir: (f32, f32),
    plane: (f32, f32),
}

/// Buffers `Maze::render` reuses from frame to frame.
#[derive(Debug, Clone, Default)]
pub struct Scratch {
    depth: Vec<f32>,
    /// Per row: the floor or ceiling's distance, and its shade.
    rows: Vec<(f32, u32)>,
    frame: Vec<u8>,
}

impl Scratch {
    /// The wall's distance in each column of the last frame.
    pub fn depth(&self) -> &[f32] {
        &self.depth
    }
}

/// A billboard to draw: where it stands, what it shows, and how.
struct Sprite<'a> {
    pos: (f32, f32),
    tex: &'a Texture,
    /// Height in tiles.
    tall: f32,
    /// Standing on the floor rather than floating at eye level.
    grounded: bool,
    /// The spin's cosine: narrows the width, below zero shows the back.
    spin: f32,
    /// Raised off the floor by this many tiles.
    lift: f32,
}

#[derive(Debug, Clone)]
pub struct Maze {
    pub map: Map,
    pieces: Vec<Piece>,
    piece: usize,
    /// Seconds into the current piece.
    t: f32,
    logos: Vec<Logo>,
    rats: Vec<Rat>,
    rat_faces: usize,
    /// Covers on wall faces: (wall tile, face direction index, cover).
    covers: Vec<((i32, i32), usize, usize)>,
    cover_count: usize,
    walls: Surface,
    floor: Surface,
    ceiling: Surface,
    flipped: bool,
    /// The view's roll: from one angle to the next, `roll_t` seconds in.
    roll_from: f32,
    roll_to: f32,
    roll_t: f32,
    clock: f32,
    cells: (usize, usize),
    rng: Rng,
}

impl Maze {
    /// A maze of `cells` (width, height) with `covers` covers to hang and
    /// `rats` system logos to set running.
    pub fn new(cells: (usize, usize), covers: usize, rats: usize, seed: u64) -> Self {
        let mut rng = Rng::new(seed);
        let map = Map::generate(cells.0, cells.1, &mut rng);
        let blank = Surface(vec![[0; 3]; TEX * TEX]);
        let mut maze = Self {
            map,
            pieces: Vec::new(),
            piece: 0,
            t: 0.0,
            logos: Vec::new(),
            rats: Vec::new(),
            rat_faces: rats,
            covers: Vec::new(),
            cover_count: covers,
            walls: blank.clone(),
            floor: blank.clone(),
            ceiling: blank,
            flipped: false,
            roll_from: 0.0,
            roll_to: 0.0,
            roll_t: ROLL_SECONDS,
            clock: 0.0,
            cells,
            rng,
        };
        maze.furnish();
        maze
    }

    fn furnish(&mut self) {
        let start = self.map.start();
        // Face south when east is a wall.
        let heading = usize::from(self.map.wall(2, 1));
        let actions = wall_follow(&self.map, heading);
        let path = walk_tiles(start, heading, &actions);
        self.pieces = pieces(&path, heading);
        self.piece = 0;
        self.t = 0.0;
        let slot = self.rng.below(super::SLOTS as usize) as u8;
        let (r, g, b) = slot_rgb(slot);
        self.walls = bricks([r, g, b], &mut self.rng);
        self.floor = flagstones(&mut self.rng);
        self.ceiling = panels(&mut self.rng);
        // Logos at junctions along the way.
        let mut junctions: Vec<(i32, i32)> = path
            .iter()
            .copied()
            .filter(|t| *t != start && self.map.open_neighbors(*t) >= 3)
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
        self.place_rats(start);
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

    /// Set the rats off from open cells away from the start.
    fn place_rats(&mut self, start: (i32, i32)) {
        let mut cells: Vec<(i32, i32)> = (0..self.map.height as i32)
            .flat_map(|y| (0..self.map.width as i32).map(move |x| (x, y)))
            .filter(|&(x, y)| {
                x % 2 == 1
                    && y % 2 == 1
                    && !self.map.wall(x, y)
                    && (x - start.0).abs() + (y - start.1).abs() > 4
            })
            .collect();
        self.rng.shuffle(&mut cells);
        let count = self.rat_faces.min(MAX_RATS);
        self.rats = cells
            .into_iter()
            .take(count)
            .enumerate()
            .map(|(face, cell)| {
                let mut rat = Rat {
                    from: cell,
                    to: cell,
                    t: 1.0,
                    face,
                    phase: 0.0,
                };
                rat.phase = self.rng.range(0.0, std::f32::consts::TAU);
                rat
            })
            .collect();
        for i in 0..self.rats.len() {
            self.next_tile(i);
        }
    }

    /// Send rat `i` on to a neighbor of its tile, not back the way it
    /// came unless it has to.
    fn next_tile(&mut self, i: usize) {
        let (from, at) = (self.rats[i].from, self.rats[i].to);
        let open: Vec<(i32, i32)> = DIRS
            .iter()
            .map(|(dx, dy)| (at.0 + dx, at.1 + dy))
            .filter(|t| !self.map.wall(t.0, t.1))
            .collect();
        let onward: Vec<(i32, i32)> = open.iter().copied().filter(|t| *t != from).collect();
        let choices = if onward.is_empty() { &open } else { &onward };
        if choices.is_empty() {
            return;
        }
        let next = choices[self.rng.below(choices.len())];
        let rat = &mut self.rats[i];
        rat.from = at;
        rat.to = next;
        rat.t = 0.0;
    }

    /// Advance by `dt` seconds.
    pub fn step(&mut self, dt: f32) {
        self.clock += dt;
        self.roll_t += dt;
        for i in 0..self.rats.len() {
            self.rats[i].t += dt * RAT_SPEED;
            while self.rats[i].t >= 1.0 {
                let left = self.rats[i].t - 1.0;
                self.next_tile(i);
                self.rats[i].t = left;
            }
        }
        self.t += dt;
        loop {
            let Some(piece) = self.pieces.get(self.piece).copied() else {
                // At the exit: a new maze.
                self.map = Map::generate(self.cells.0, self.cells.1, &mut self.rng);
                self.furnish();
                return;
            };
            if self.t < piece.seconds() {
                break;
            }
            self.t -= piece.seconds();
            self.piece += 1;
        }
        let pos = self.camera().pos;
        let reached = self.logos.iter().position(|l| {
            let c = center(l.tile);
            (c.0 - pos.0).powi(2) + (c.1 - pos.1).powi(2) < LOGO_REACH * LOGO_REACH
        });
        if let Some(i) = reached {
            self.logos.remove(i);
            self.flipped = !self.flipped;
            self.roll_from = self.roll();
            self.roll_to += std::f32::consts::PI;
            self.roll_t = 0.0;
        }
    }

    /// The view's roll right now, in radians.
    fn roll(&self) -> f32 {
        let k = (self.roll_t / ROLL_SECONDS).clamp(0.0, 1.0);
        let eased = k * k * (3.0 - 2.0 * k);
        self.roll_from + (self.roll_to - self.roll_from) * eased
    }

    /// The camera for this moment.
    fn camera(&self) -> Camera {
        let (pos, angle) = self.pieces.get(self.piece).map_or_else(
            || {
                self.pieces
                    .last()
                    .map_or((center(self.map.exit()), 0.0), |p| p.at(1.0))
            },
            |p| p.at((self.t / p.seconds().max(1e-6)).clamp(0.0, 1.0)),
        );
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
    /// `new`), `logo` the Zaparoo logo's, `rats` the running logos'.
    pub fn render(
        &self,
        out: &mut [u8],
        (width, height): (usize, usize),
        covers: &[Texture],
        logo: &Texture,
        rats: &[Texture],
        scratch: &mut Scratch,
    ) {
        let len = width * height * 3;
        if width == 0 || height == 0 || out.len() < len {
            return;
        }
        let roll = self.roll().rem_euclid(std::f32::consts::TAU);
        let (s, c) = roll.sin_cos();
        let upright = s.abs() < 1e-3;
        let mut frame = std::mem::take(&mut scratch.frame);
        if upright {
            self.draw(out, (width, height), covers, logo, rats, scratch);
            if c < 0.0 {
                // Upside down: half a turn.
                let px = &mut out[..len];
                px.reverse();
                for p in px.chunks_exact_mut(3) {
                    p.swap(0, 2);
                }
            }
        } else {
            frame.resize(len, 0);
            self.draw(&mut frame, (width, height), covers, logo, rats, scratch);
            rotate(&frame, out, (width, height), s, c);
        }
        scratch.frame = frame;
    }

    fn draw(
        &self,
        out: &mut [u8],
        (width, height): (usize, usize),
        covers: &[Texture],
        logo: &Texture,
        rats: &[Texture],
        scratch: &mut Scratch,
    ) {
        let cam = self.camera();
        let half = height as f32 / 2.0;
        scratch.rows.clear();
        for y in 0..height {
            let p = (y as f32 + 0.5 - half).abs().max(0.5);
            let dist = half / p;
            scratch.rows.push((dist, light(dist)));
        }
        scratch.depth.clear();
        for x in 0..width {
            let dist = self.column(out, (width, height), x, cam, covers, &scratch.rows);
            scratch.depth.push(dist);
        }
        self.render_sprites(out, (width, height), logo, rats, &scratch.depth, cam);
    }

    /// Cast one column's ray, draw the column, and return the wall's
    /// distance.
    #[allow(
        clippy::many_single_char_names,
        clippy::too_many_lines,
        clippy::too_many_arguments,
        reason = "raycasting reads best in the textbook's names, as one pass per column"
    )]
    fn column(
        &self,
        out: &mut [u8],
        (width, height): (usize, usize),
        x: usize,
        cam: Camera,
        covers: &[Texture],
        rows: &[(f32, u32)],
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
        let y0 = (top.max(0.0) as usize).min(height);
        let y1 = ((half + line / 2.0).max(0.0) as usize).min(height);
        let along = if ns { px + dist * rx } else { py + dist * ry };
        let (face, u) = face_and_u(ns, step_x > 0, step_y > 0, along);
        let cover = self
            .covers
            .iter()
            .find(|(t, f, _)| *t == (mx, my) && *f == face)
            .and_then(|(_, _, c)| covers.get(*c));
        let wall_light = (light(dist) * if ns { 200 } else { 256 }) >> 8;
        let tex_x = ((u * TEX as f32) as usize).min(TEX - 1);
        let tex = TEX as f32;
        let mut put = |y: usize, c: [u8; 3]| {
            let at = (y * width + x) * 3;
            out[at..at + 3].copy_from_slice(&c);
        };
        // Floor and ceiling: where this column's ray meets them, row by row.
        for (y, &(d, k)) in rows.iter().enumerate().take(y0) {
            let (fx, fy) = (px + d * rx, py + d * ry);
            put(
                y,
                shade(self.ceiling.at((fx * tex) as usize, (fy * tex) as usize), k),
            );
        }
        for (y, &(d, k)) in rows.iter().enumerate().skip(y1) {
            let (fx, fy) = (px + d * rx, py + d * ry);
            put(
                y,
                shade(self.floor.at((fx * tex) as usize, (fy * tex) as usize), k),
            );
        }
        for y in y0..y1 {
            let v = (y as f32 - top) / line;
            let brick = || {
                shade(
                    self.walls.at(tex_x, ((v * tex) as usize).min(TEX - 1)),
                    wall_light,
                )
            };
            let c = if let Some(art) = cover {
                // A light frame around the cover.
                let (fu, fv) = ((u - 0.18) / 0.64, (v - 0.14) / 0.6);
                if (0.0..1.0).contains(&fu) && (0.0..1.0).contains(&fv) {
                    let s = art.sample(fu, fv);
                    shade([s[0], s[1], s[2]], wall_light)
                } else if (-0.05..1.05).contains(&fu) && (-0.06..1.06).contains(&fv) {
                    shade(FRAME, wall_light)
                } else {
                    brick()
                }
            } else {
                brick()
            };
            put(y, c);
        }
        dist
    }

    /// The Zaparoo logos and the rats as billboards, far to near, hidden
    /// behind walls.
    fn render_sprites(
        &self,
        out: &mut [u8],
        size: (usize, usize),
        logo: &Texture,
        rats: &[Texture],
        depth: &[f32],
        cam: Camera,
    ) {
        let mut sprites: Vec<Sprite<'_>> = Vec::new();
        if logo.width > 0 && logo.height > 0 {
            for l in &self.logos {
                sprites.push(Sprite {
                    pos: center(l.tile),
                    tex: logo,
                    tall: 0.32,
                    grounded: false,
                    spin: (self.clock * 2.2 + l.phase).cos(),
                    lift: 0.0,
                });
            }
        }
        for r in &self.rats {
            if let Some(tex) = rats.get(r.face).filter(|t| t.width > 0 && t.height > 0) {
                // A little scurrying hop.
                let hop = (self.clock * 14.0 + r.phase).sin().abs() * 0.04;
                sprites.push(Sprite {
                    pos: r.pos(),
                    tex,
                    tall: 0.22,
                    grounded: true,
                    spin: 1.0,
                    lift: hop,
                });
            }
        }
        let (px, py) = cam.pos;
        let dist2 = |s: &Sprite<'_>| (s.pos.0 - px).powi(2) + (s.pos.1 - py).powi(2);
        sprites.sort_by(|a, b| dist2(b).total_cmp(&dist2(a)));
        let inv = 1.0 / (cam.plane.0 * cam.dir.1 - cam.dir.0 * cam.plane.1);
        for s in &sprites {
            let (sx, sy) = (s.pos.0 - px, s.pos.1 - py);
            let across = inv * (cam.dir.1 * sx - cam.dir.0 * sy);
            let into = inv * (-cam.plane.1 * sx + cam.plane.0 * sy);
            if into <= 0.1 {
                continue;
            }
            let unit = size.1 as f32 / into;
            let tall = unit * s.tall;
            let aspect = s.tex.width as f32 / s.tex.height as f32;
            let wide = (tall * aspect).min(unit * 0.6) * s.spin.abs();
            let tall = if s.grounded { wide / aspect } else { tall };
            if wide < 1.0 {
                continue;
            }
            let left = (size.0 as f32 / 2.0) * (1.0 + across / into) - wide / 2.0;
            let top = if s.grounded {
                size.1 as f32 / 2.0 + unit / 2.0 - tall - s.lift * unit
            } else {
                size.1 as f32 / 2.0 - tall / 2.0
            };
            billboard(
                out,
                size,
                s.tex,
                depth,
                (left, top, wide, tall),
                into,
                s.spin < 0.0,
            );
        }
    }
}

/// The light at `dist` tiles, 0 to 256.
fn light(dist: f32) -> u32 {
    ((1.0 / (1.0 + dist * 0.22)).clamp(0.12, 1.0) * 256.0) as u32
}

#[allow(
    clippy::too_many_arguments,
    reason = "one billboard: target, texture, depth and its box"
)]
fn billboard(
    out: &mut [u8],
    size: (usize, usize),
    tex: &Texture,
    depth: &[f32],
    (left, top, wide, tall): (f32, f32, f32, f32),
    into: f32,
    back: bool,
) {
    let k = if back { 140 } else { light(into * 0.5) };
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
            let s = tex.sample(u, (row as f32 - top) / tall);
            if s[3] < 128 {
                continue;
            }
            let at = (row * size.0 + col) * 3;
            out[at..at + 3].copy_from_slice(&shade([s[0], s[1], s[2]], k));
        }
    }
}

/// Copy `src` into `out` turned by the angle whose sine and cosine are
/// `s` and `c`, about the center, zoomed just enough that no corner
/// shows past the picture's edge.
fn rotate(src: &[u8], out: &mut [u8], (width, height): (usize, usize), s: f32, c: f32) {
    let (w, h) = (width as f32, height as f32);
    let zoom = ((w * c.abs() + h * s.abs()) / w).max((w * s.abs() + h * c.abs()) / h);
    let (cx, cy) = (w / 2.0, h / 2.0);
    let (ks, kc) = (s / zoom, c / zoom);
    for y in 0..height {
        let dy = y as f32 + 0.5 - cy;
        // The source point for x = 0, then a step per pixel.
        let dx0 = 0.5 - cx;
        let mut sx = kc * dx0 + ks * dy + cx;
        let mut sy = -ks * dx0 + kc * dy + cy;
        for x in 0..width {
            let ix = (sx.max(0.0) as usize).min(width - 1);
            let iy = (sy.max(0.0) as usize).min(height - 1);
            let from = (iy * width + ix) * 3;
            let to = (y * width + x) * 3;
            out[to..to + 3].copy_from_slice(&src[from..from + 3]);
            sx += kc;
            sy -= ks;
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
    fn the_walk_glides_without_jumps_and_stays_off_the_walls() {
        let dt = 1.0 / 60.0;
        for seed in 0..10 {
            let mut maze = Maze::new((7, 5), 0, 0, seed);
            let first = maze.map.walls.clone();
            let mut last = maze.camera();
            for _ in 0..(60 * 600) {
                maze.step(dt);
                if maze.map.walls != first {
                    break;
                }
                let cam = maze.camera();
                let moved =
                    ((cam.pos.0 - last.pos.0).powi(2) + (cam.pos.1 - last.pos.1).powi(2)).sqrt();
                assert!(
                    moved <= WALK_SPEED * dt * 1.6,
                    "seed {seed}: jumped {moved}"
                );
                let turned = (cam.dir.0 * last.dir.1 - cam.dir.1 * last.dir.0)
                    .asin()
                    .abs();
                assert!(turned < 0.2, "seed {seed}: snapped {turned} rad");
                let tile = (cam.pos.0.floor() as i32, cam.pos.1.floor() as i32);
                assert!(!maze.map.wall(tile.0, tile.1), "seed {seed}: inside a wall");
                last = cam;
            }
        }
    }

    #[test]
    fn walking_through_a_logo_rolls_the_view_and_a_new_maze_follows() {
        let mut maze = Maze::new((7, 5), 3, 0, 4);
        let logos = maze.logos_left();
        assert!(logos > 0);
        let first = maze.map.walls.clone();
        let mut flips = 0;
        let mut was = maze.flipped();
        let mut renewed = false;
        let mut mid_roll = false;
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
            let roll = maze.roll().rem_euclid(std::f32::consts::PI);
            mid_roll |= roll > 0.5 && roll < 2.6;
        }
        assert!(renewed);
        assert_eq!(flips, logos);
        assert!(mid_roll, "the roll passes through the angles between");
    }

    #[test]
    fn rats_keep_to_the_corridors() {
        let mut maze = Maze::new((7, 5), 0, 2, 9);
        assert_eq!(maze.rats.len(), 2);
        let start = maze.rats.iter().map(Rat::pos).collect::<Vec<_>>();
        for _ in 0..600 {
            maze.step(1.0 / 30.0);
            for rat in &maze.rats {
                assert!(!maze.map.wall(rat.from.0, rat.from.1));
                assert!(!maze.map.wall(rat.to.0, rat.to.1));
            }
        }
        let now = maze.rats.iter().map(Rat::pos).collect::<Vec<_>>();
        assert_ne!(start, now, "the rats move");
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
    fn a_frame_has_textured_ceiling_walls_and_floor() {
        let maze = Maze::new((5, 4), 2, 1, 7);
        let (w, h) = (64, 40);
        let mut out = vec![0u8; w * h * 3];
        let tex = Texture {
            width: 2,
            height: 2,
            rgba: vec![255; 16],
        };
        let mut scratch = Scratch::default();
        let covers = [tex.clone(), tex.clone()];
        maze.render(
            &mut out,
            (w, h),
            &covers,
            &tex,
            std::slice::from_ref(&tex),
            &mut scratch,
        );
        let px = |out: &[u8], x: usize, y: usize| {
            let i = (y * w + x) * 3;
            [out[i], out[i + 1], out[i + 2]]
        };
        let lit = |c: [u8; 3]| c != [0, 0, 0];
        assert!(lit(px(&out, w / 2, 0)), "ceiling");
        assert!(lit(px(&out, w / 2, h - 1)), "floor");
        assert!(lit(px(&out, w / 2, h / 2)), "wall");
        // Textured: the floor's bottom row is not one flat color.
        let floor: std::collections::HashSet<[u8; 3]> =
            (0..w).map(|x| px(&out, x, h - 1)).collect();
        assert!(floor.len() > 3);
        assert_eq!(scratch.depth().len(), w);
        assert!(scratch.depth().iter().all(|d| *d < 100.0));
        // Same seed, same frame.
        let mut again = vec![0u8; w * h * 3];
        Maze::new((5, 4), 2, 1, 7).render(
            &mut again,
            (w, h),
            &covers,
            &tex,
            std::slice::from_ref(&tex),
            &mut scratch,
        );
        assert_eq!(out, again);
    }

    #[test]
    fn upside_down_is_the_picture_turned_half_round() {
        let mut maze = Maze::new((5, 4), 0, 0, 3);
        let (w, h) = (32, 20);
        let mut scratch = Scratch::default();
        let mut upright = vec![0u8; w * h * 3];
        maze.render(
            &mut upright,
            (w, h),
            &[],
            &Texture::default(),
            &[],
            &mut scratch,
        );
        maze.flipped = true;
        maze.roll_to = std::f32::consts::PI;
        maze.roll_from = std::f32::consts::PI;
        let mut over = vec![0u8; w * h * 3];
        maze.render(
            &mut over,
            (w, h),
            &[],
            &Texture::default(),
            &[],
            &mut scratch,
        );
        for y in 0..h {
            for x in 0..w {
                let a = (y * w + x) * 3;
                let b = ((h - 1 - y) * w + (w - 1 - x)) * 3;
                assert_eq!(upright[a..a + 3], over[b..b + 3]);
            }
        }
    }
}
