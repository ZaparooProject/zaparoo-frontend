// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

//! Polygons: two four-cornered outlines whose corners each bounce around
//! the screen on their own, trailing a few fading echoes, their color
//! drifting through the palette. A third outline does the same, but every
//! so often gathers itself into the Zaparoo bolt in the middle of the
//! screen, holds, and lets go again.

use super::{slot_rgb, Rng, SLOTS};

/// Corners per bouncing outline.
pub const CORNERS: usize = 4;
/// Echoes behind each outline, the outline itself not counted.
pub const ECHOES: usize = 3;
/// Seconds between echo snapshots.
pub const ECHO_GAP: f32 = 0.12;
/// Corner speed range, in height units per second.
pub const SPEED: (f32, f32) = (0.18, 0.34);
/// Seconds per palette slot as the color drifts.
pub const COLOR_SECONDS: f32 = 6.0;
/// Seconds between the third outline's visits to the bolt.
pub const MORPH_EVERY: f32 = 16.0;
/// Seconds to gather into the bolt (and to let go again).
pub const MORPH_SECONDS: f32 = 2.0;
/// Seconds the bolt holds.
pub const MORPH_HOLD: f32 = 3.0;
/// The bolt's height, in height units.
pub const MARK_HEIGHT: f32 = 0.55;

/// One outline to draw: its points, closed, and its color and opacity.
#[derive(Debug, Clone, PartialEq)]
pub struct Outline {
    pub points: Vec<(f32, f32)>,
    pub rgb: (u8, u8, u8),
    pub opacity: f32,
}

#[derive(Debug, Clone)]
struct Mover {
    corners: Vec<(f32, f32)>,
    velocity: Vec<(f32, f32)>,
    echoes: Vec<Vec<(f32, f32)>>,
    hue: f32,
}

impl Mover {
    fn new(n: usize, aspect: f32, hue: f32, rng: &mut Rng) -> Self {
        let corners = (0..n)
            .map(|_| (rng.range(0.0, aspect), rng.range(0.0, 1.0)))
            .collect();
        let velocity = (0..n)
            .map(|_| {
                let vx = rng.range(SPEED.0, SPEED.1) * if rng.coin() { 1.0 } else { -1.0 };
                let vy = rng.range(SPEED.0, SPEED.1) * if rng.coin() { 1.0 } else { -1.0 };
                (vx, vy)
            })
            .collect();
        Self {
            corners,
            velocity,
            echoes: Vec::new(),
            hue,
        }
    }

    fn step(&mut self, dt: f32, aspect: f32) {
        for (c, v) in self.corners.iter_mut().zip(self.velocity.iter_mut()) {
            c.0 += v.0 * dt;
            c.1 += v.1 * dt;
            if c.0 < 0.0 {
                c.0 = -c.0;
                v.0 = v.0.abs();
            } else if c.0 > aspect {
                c.0 = 2.0 * aspect - c.0;
                v.0 = -v.0.abs();
            }
            if c.1 < 0.0 {
                c.1 = -c.1;
                v.1 = v.1.abs();
            } else if c.1 > 1.0 {
                c.1 = 2.0 - c.1;
                v.1 = -v.1.abs();
            }
        }
        self.hue = (self.hue + dt / COLOR_SECONDS) % f32::from(SLOTS);
    }

    fn snapshot(&mut self, shape: Vec<(f32, f32)>) {
        self.echoes.insert(0, shape);
        self.echoes.truncate(ECHOES);
    }

    fn rgb(&self) -> (u8, u8, u8) {
        let base = self.hue.floor();
        let t = self.hue - base;
        let a = slot_rgb(base as u8);
        let b = slot_rgb(base as u8 + 1);
        let mix = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * t).round() as u8;
        (mix(a.0, b.0), mix(a.1, b.1), mix(a.2, b.2))
    }
}

#[derive(Debug, Clone)]
pub struct Polygons {
    movers: Vec<Mover>,
    /// The bolt's outline, normalized to height 1, and its width.
    mark: Vec<(f32, f32)>,
    mark_w: f32,
    t: f32,
    since_echo: f32,
}

impl Polygons {
    /// `mark` is the bolt's outline from `contour::normalize` (any count
    /// of points; empty leaves the third outline out).
    pub fn new(aspect: f32, mark: (Vec<(f32, f32)>, f32), seed: u64) -> Self {
        let mut rng = Rng::new(seed);
        let mut movers = vec![
            Mover::new(CORNERS, aspect, 0.0, &mut rng),
            Mover::new(CORNERS, aspect, 2.0, &mut rng),
        ];
        if !mark.0.is_empty() {
            movers.push(Mover::new(CORNERS, aspect, 4.0, &mut rng));
        }
        Self {
            movers,
            mark: mark.0,
            mark_w: mark.1,
            t: 0.0,
            since_echo: 0.0,
        }
    }

    pub fn step(&mut self, dt: f32, aspect: f32) {
        self.t += dt;
        for m in &mut self.movers {
            m.step(dt, aspect);
        }
        self.since_echo += dt;
        if self.since_echo >= ECHO_GAP {
            self.since_echo -= ECHO_GAP;
            for i in 0..self.movers.len() {
                let shape = self.shape(i, aspect);
                self.movers[i].snapshot(shape);
            }
        }
    }

    /// How far into the bolt the third outline is: 0 free, 1 the bolt.
    pub fn morph(&self) -> f32 {
        let cycle = MORPH_EVERY + 2.0 * MORPH_SECONDS + MORPH_HOLD;
        let t = self.t % cycle - MORPH_EVERY;
        if t < 0.0 {
            0.0
        } else if t < MORPH_SECONDS {
            smooth(t / MORPH_SECONDS)
        } else if t < MORPH_SECONDS + MORPH_HOLD {
            1.0
        } else {
            smooth(1.0 - (t - MORPH_SECONDS - MORPH_HOLD) / MORPH_SECONDS)
        }
    }

    fn shape(&self, i: usize, aspect: f32) -> Vec<(f32, f32)> {
        let corners = &self.movers[i].corners;
        if i < 2 || self.mark.is_empty() {
            return corners.clone();
        }
        let n = self.mark.len();
        let free = super::contour::resample(corners, n);
        let k = self.morph();
        let left = (aspect - self.mark_w * MARK_HEIGHT) / 2.0;
        let top = (1.0 - MARK_HEIGHT) / 2.0;
        // Start the bolt's points at the one nearest the free outline's
        // first, so the gathering does not twist.
        let offset = nearest(&self.mark, free[0], left, top);
        free.iter()
            .enumerate()
            .map(|(j, f)| {
                let m = self.mark[(j + offset) % n];
                let target = (left + m.0 * MARK_HEIGHT, top + m.1 * MARK_HEIGHT);
                (f.0 + (target.0 - f.0) * k, f.1 + (target.1 - f.1) * k)
            })
            .collect()
    }

    /// Every outline: echoes first (oldest first, dimmest), then the
    /// outlines themselves. The list's length is fixed once the echoes
    /// have filled.
    pub fn outlines(&self, aspect: f32) -> Vec<Outline> {
        let mut out = Vec::new();
        for (i, m) in self.movers.iter().enumerate() {
            let rgb = m.rgb();
            for (age, echo) in m.echoes.iter().enumerate().rev() {
                out.push(Outline {
                    points: echo.clone(),
                    rgb,
                    opacity: 0.7 - 0.2 * age as f32,
                });
            }
            out.push(Outline {
                points: self.shape(i, aspect),
                rgb,
                opacity: 1.0,
            });
        }
        out
    }
}

/// Draw `outlines` into an RGB picture `width` by `height` pixels: black,
/// one-pixel lines, each outline's color scaled by its opacity, and the
/// brighter color kept where lines cross. Points are in height units on a
/// scene `aspect` wide, stretched to fill the picture whatever its shape.
pub fn render(outlines: &[Outline], aspect: f32, out: &mut [u8], width: usize, height: usize) {
    out.fill(0);
    let scale_x = width as f32 / aspect.max(f32::EPSILON);
    let scale_y = height as f32;
    for outline in outlines {
        let shade = |c: u8| (f32::from(c) * outline.opacity.clamp(0.0, 1.0)) as u8;
        let rgb = [
            shade(outline.rgb.0),
            shade(outline.rgb.1),
            shade(outline.rgb.2),
        ];
        let n = outline.points.len();
        for i in 0..n {
            let (ax, ay) = outline.points[i];
            let (bx, by) = outline.points[(i + 1) % n];
            line(
                out,
                (width, height),
                ((ax * scale_x) as i32, (ay * scale_y) as i32),
                ((bx * scale_x) as i32, (by * scale_y) as i32),
                rgb,
            );
        }
    }
}

/// Bresenham's line, clipped per pixel.
fn line(
    out: &mut [u8],
    (width, height): (usize, usize),
    (mut x0, mut y0): (i32, i32),
    (x1, y1): (i32, i32),
    rgb: [u8; 3],
) {
    let dx = (x1 - x0).abs();
    let dy = -(y1 - y0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut err = dx + dy;
    loop {
        if let (Ok(px), Ok(py)) = (usize::try_from(x0), usize::try_from(y0)) {
            if px < width && py < height {
                let at = (py * width + px) * 3;
                if let Some(cell) = out.get_mut(at..at + 3) {
                    for (dst, src) in cell.iter_mut().zip(rgb) {
                        *dst = (*dst).max(src);
                    }
                }
            }
        }
        if x0 == x1 && y0 == y1 {
            break;
        }
        let twice = 2 * err;
        if twice >= dy {
            err += dy;
            x0 += sx;
        }
        if twice <= dx {
            err += dx;
            y0 += sy;
        }
    }
}

fn smooth(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn nearest(mark: &[(f32, f32)], to: (f32, f32), left: f32, top: f32) -> usize {
    let mut best = (0, f32::MAX);
    for (i, m) in mark.iter().enumerate() {
        let p = (left + m.0 * MARK_HEIGHT, top + m.1 * MARK_HEIGHT);
        let d = (p.0 - to.0).powi(2) + (p.1 - to.1).powi(2);
        if d < best.1 {
            best = (i, d);
        }
    }
    best.0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn diamond() -> (Vec<(f32, f32)>, f32) {
        let pts =
            super::super::contour::resample(&[(0.5, 0.0), (1.0, 0.5), (0.5, 1.0), (0.0, 0.5)], 24);
        (pts, 1.0)
    }

    #[test]
    fn render_draws_closed_outlines_and_keeps_the_brighter_color() {
        let square = Outline {
            points: vec![(0.1, 0.1), (0.9, 0.1), (0.9, 0.9), (0.1, 0.9)],
            rgb: (200, 100, 0),
            opacity: 0.5,
        };
        let bright = Outline {
            points: vec![(0.1, 0.1), (0.9, 0.1)],
            rgb: (255, 255, 255),
            opacity: 1.0,
        };
        let (w, h) = (20, 20);
        let mut out = vec![7; w * h * 3];
        render(
            std::slice::from_ref(&square),
            w as f32 / h as f32,
            &mut out,
            w,
            h,
        );
        let at = |x: usize, y: usize| &out[(y * w + x) * 3..(y * w + x) * 3 + 3];
        assert_eq!(at(10, 10), [0, 0, 0], "cleared inside");
        assert_eq!(
            at(2, 10),
            [100, 50, 0],
            "left edge, closed back to the start"
        );
        assert_eq!(at(18, 10), [100, 50, 0], "right edge");
        render(&[square, bright], w as f32 / h as f32, &mut out, w, h);
        assert_eq!(
            &out[(2 * w + 10) * 3..(2 * w + 10) * 3 + 3],
            [255, 255, 255]
        );
    }

    #[test]
    fn render_clips_points_off_the_picture() {
        let off = Outline {
            points: vec![(-1.0, -1.0), (3.0, 3.0)],
            rgb: (255, 0, 0),
            opacity: 1.0,
        };
        let mut out = vec![0; 10 * 10 * 3];
        render(&[off], 1.0, &mut out, 10, 10);
        assert_eq!(
            out[(5 * 10 + 5) * 3],
            255,
            "the diagonal crosses the middle"
        );
    }

    #[test]
    fn corners_stay_inside_and_echoes_are_capped() {
        let aspect = 16.0 / 9.0;
        let mut p = Polygons::new(aspect, diamond(), 1);
        for _ in 0..(30 * 60) {
            p.step(1.0 / 30.0, aspect);
            let outlines = p.outlines(aspect);
            assert!(outlines.len() <= 3 * (ECHOES + 1));
            for o in &outlines {
                for (x, y) in &o.points {
                    assert!(
                        (-1e-3..=aspect + 1e-3).contains(x) && (-1e-3..=1.0 + 1e-3).contains(y)
                    );
                }
            }
        }
        assert_eq!(p.outlines(aspect).len(), 3 * (ECHOES + 1));
    }

    #[test]
    fn the_third_outline_becomes_the_bolt_and_lets_go() {
        let aspect = 4.0 / 3.0;
        let mut p = Polygons::new(aspect, diamond(), 2);
        let mut peak = 0.0f32;
        let mut after = 1.0f32;
        let cycle = MORPH_EVERY + 2.0 * MORPH_SECONDS + MORPH_HOLD;
        let steps = (cycle * 30.0) as usize + 30;
        for i in 0..steps {
            p.step(1.0 / 30.0, aspect);
            peak = peak.max(p.morph());
            if i == steps - 1 {
                after = p.morph();
            }
        }
        assert!((peak - 1.0).abs() < 1e-6);
        assert!(after < 1e-6);
        // At the hold, the outline is the bolt: centered, MARK_HEIGHT tall.
        let mut p = Polygons::new(aspect, diamond(), 3);
        p.step(MORPH_EVERY + MORPH_SECONDS + MORPH_HOLD / 2.0, aspect);
        let outlines = p.outlines(aspect);
        let bolt = outlines
            .last()
            .map(|o| o.points.clone())
            .unwrap_or_default();
        assert!(!bolt.is_empty());
        let top = bolt.iter().map(|p| p.1).fold(f32::MAX, f32::min);
        let bottom = bolt.iter().map(|p| p.1).fold(f32::MIN, f32::max);
        assert!((bottom - top - MARK_HEIGHT).abs() < 0.02);
    }

    #[test]
    fn without_a_mark_there_are_two_outlines() {
        let p = Polygons::new(16.0 / 9.0, (Vec::new(), 0.0), 4);
        assert_eq!(p.outlines(16.0 / 9.0).len(), 2);
    }
}
