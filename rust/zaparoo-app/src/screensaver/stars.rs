// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

//! Starfield: stars stream out from the center, and now and then a system
//! logo or the Zaparoo logo flies past among them. The star count is
//! capped and the logos grow in steps, so a frame stays cheap on the
//! software renderer.

use super::{Look, Rng, Sprite};

/// Logos in flight at once.
pub const MAX_LOGOS: usize = 2;
/// Depth units per second.
pub const SPEED: f32 = 0.32;
/// Seconds between logo launches, at least.
pub const LOGO_EVERY: f32 = 5.0;
/// A star's size at the far plane, in height units; it grows to three
/// times that up close.
pub const STAR: f32 = 1.0 / 240.0;
/// A logo's height at depth 1.
pub const LOGO_FAR: f32 = 0.05;
/// Logo heights step by this factor, so its raster is reused for a while.
pub const LOGO_STEP: f32 = 1.15;

#[derive(Debug, Clone, Copy, PartialEq)]
struct Body {
    x: f32,
    y: f32,
    z: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Flier {
    body: Body,
    look: Look,
    aspect: f32,
}

#[derive(Debug, Clone)]
pub struct Stars {
    bodies: Vec<Body>,
    logos: [Option<Flier>; MAX_LOGOS],
    since_logo: f32,
    systems: usize,
    rng: Rng,
}

impl Stars {
    pub fn new(count: usize, systems: usize, seed: u64) -> Self {
        let mut rng = Rng::new(seed);
        let stars = (0..count)
            .map(|_| Body {
                x: rng.range(-1.0, 1.0),
                y: rng.range(-1.0, 1.0),
                z: rng.range(0.05, 1.0),
            })
            .collect();
        Self {
            bodies: stars,
            logos: [None; MAX_LOGOS],
            since_logo: 0.0,
            systems,
            rng,
        }
    }

    pub fn step(&mut self, dt: f32, aspect: f32, face_aspect: impl Fn(Look) -> f32) {
        for i in 0..self.bodies.len() {
            self.bodies[i].z -= SPEED * dt;
            let (sx, sy) = project(self.bodies[i], aspect);
            if self.bodies[i].z <= 0.03 || !(0.0..aspect).contains(&sx) || !(0.0..1.0).contains(&sy)
            {
                self.bodies[i] = Body {
                    x: self.rng.range(-1.0, 1.0),
                    y: self.rng.range(-1.0, 1.0),
                    z: 1.0,
                };
            }
        }
        for slot in &mut self.logos {
            if let Some(f) = slot {
                f.body.z -= SPEED * dt;
                let s = flier_sprite(f, aspect);
                if f.body.z <= 0.06
                    || s.x + s.w < 0.0
                    || s.x > aspect
                    || s.y + s.h < 0.0
                    || s.y > 1.0
                {
                    *slot = None;
                }
            }
        }
        self.since_logo += dt;
        if self.since_logo >= LOGO_EVERY {
            if let Some(free) = self.logos.iter().position(Option::is_none) {
                self.since_logo = -self.rng.range(0.0, LOGO_EVERY);
                let look = if self.systems == 0 || self.rng.below(4) == 0 {
                    Look::Logo
                } else {
                    Look::System(self.rng.below(self.systems))
                };
                // Off the axis, so it passes by rather than through.
                let angle = self.rng.range(0.0, std::f32::consts::TAU);
                let r = self.rng.range(0.12, 0.3);
                self.logos[free] = Some(Flier {
                    body: Body {
                        x: angle.cos() * r,
                        y: angle.sin() * r,
                        z: 1.0,
                    },
                    look,
                    aspect: face_aspect(look),
                });
            }
        }
    }

    /// Every star, then one sprite per logo slot (opacity 0 when empty),
    /// so the list's length never changes.
    pub fn sprites(&self, aspect: f32) -> Vec<Sprite> {
        let mut out = Vec::with_capacity(self.bodies.len() + MAX_LOGOS);
        for star in &self.bodies {
            let (x, y) = project(*star, aspect);
            let size = STAR * (1.0 + 2.0 * (1.0 - star.z)).round();
            let mut s = Sprite::new(x, y, size, size, Look::Block(0));
            // Far stars are dim.
            s.opacity = (1.2 - star.z).clamp(0.3, 1.0);
            out.push(s);
        }
        for slot in &self.logos {
            out.push(match slot {
                Some(f) => flier_sprite(f, aspect),
                None => Sprite {
                    opacity: 0.0,
                    ..Sprite::new(0.0, 0.0, 0.0, 0.0, Look::Logo)
                },
            });
        }
        out
    }
}

fn project(body: Body, aspect: f32) -> (f32, f32) {
    let k = 0.5;
    (
        aspect / 2.0 + body.x / body.z * k,
        0.5 + body.y / body.z * k,
    )
}

fn flier_sprite(f: &Flier, aspect: f32) -> Sprite {
    let raw = LOGO_FAR / f.body.z;
    // Heights in steps of LOGO_STEP from LOGO_FAR.
    let steps = (raw / LOGO_FAR).ln() / LOGO_STEP.ln();
    let h = LOGO_FAR * LOGO_STEP.powf(steps.floor());
    let w = h * f.aspect;
    let (cx, cy) = project(f.body, aspect);
    Sprite::new(cx - w / 2.0, cy - h / 2.0, w, h, f.look)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stars_stay_on_screen_and_the_list_keeps_its_length() {
        let mut stars = Stars::new(50, 4, 1);
        for _ in 0..3000 {
            stars.step(1.0 / 30.0, 16.0 / 9.0, |_| 2.0);
            let sprites = stars.sprites(16.0 / 9.0);
            assert_eq!(sprites.len(), 50 + MAX_LOGOS);
            for s in &sprites[..50] {
                assert!(s.x >= 0.0 && s.x < 16.0 / 9.0 && s.y >= 0.0 && s.y < 1.0);
            }
        }
    }

    #[test]
    fn logos_fly_by_two_at_most_and_grow_in_steps() {
        let mut stars = Stars::new(10, 4, 2);
        let mut seen = 0;
        let mut heights = std::collections::BTreeSet::new();
        for _ in 0..(30 * 120) {
            stars.step(1.0 / 30.0, 16.0 / 9.0, |_| 1.0);
            let logos: Vec<_> = stars.sprites(16.0 / 9.0)[10..]
                .iter()
                .filter(|s| s.opacity > 0.0)
                .copied()
                .collect();
            assert!(logos.len() <= MAX_LOGOS);
            seen += logos.len();
            for l in logos {
                heights.insert((l.h * 10_000.0) as u32);
            }
        }
        assert!(seen > 0);
        assert!(heights.len() < 40, "heights are stepped: {}", heights.len());
    }
}
