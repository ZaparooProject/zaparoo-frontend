// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

//! Spinning logo: the logo drifts like the bouncing one and turns about
//! its upright axis like a coin. Its width follows the cosine of the turn,
//! the back face shows darker, and each time it is edge-on it can swap to
//! another face: a random system logo, with the Zaparoo logo back every
//! few turns.

use super::bounce::Walk;
use super::{Look, Rng, Sprite};

/// Seconds per half turn (front to back).
pub const HALF_TURN: f32 = 1.6;
/// Width steps per half turn: the sprite's width moves in this many
/// discrete sizes, so a frame only repaints when the step changes.
pub const STEPS: u32 = 12;
/// Swaps between Zaparoo logo appearances.
pub const LOGO_EVERY: u32 = 4;
/// Drift in height units per second.
pub const SPEED: f32 = 0.09;
/// The logo box's height, in height units.
pub const HEIGHT: f32 = 0.16;
/// How dark the back face is drawn.
pub const BACK_OPACITY: f32 = 0.55;

#[derive(Debug, Clone)]
pub struct Spin {
    walk: Walk,
    /// Seconds into the current half turn.
    phase: f32,
    look: Look,
    /// Half turns since the Zaparoo logo was last shown.
    since_logo: u32,
    back: bool,
    /// Full width for the current face.
    face_w: f32,
    systems: usize,
    rng: Rng,
}

impl Spin {
    /// `logo_aspect` is the Zaparoo logo's width over height; `systems` is
    /// how many system logos the adapter can draw.
    pub fn new(aspect: f32, logo_aspect: f32, systems: usize, seed: u64) -> Self {
        let face_w = HEIGHT * logo_aspect;
        let walk = Walk::new(
            (aspect - face_w) / 2.0,
            (1.0 - HEIGHT) / 2.0,
            face_w,
            HEIGHT,
        );
        Self {
            walk,
            phase: 0.0,
            look: Look::Logo,
            since_logo: 0,
            back: false,
            face_w,
            systems,
            rng: Rng::new(seed),
        }
    }

    /// Advance by `dt` seconds. `face_aspect` gives a face's width over
    /// height for the look about to show (system logos vary).
    pub fn step(&mut self, dt: f32, aspect: f32, face_aspect: impl Fn(Look) -> f32) {
        self.walk.step(SPEED * dt, aspect);
        self.phase += dt;
        while self.phase >= HALF_TURN {
            self.phase -= HALF_TURN;
            self.edge_on(&face_aspect, aspect);
        }
    }

    fn edge_on(&mut self, face_aspect: &impl Fn(Look) -> f32, aspect: f32) {
        self.back = !self.back;
        self.since_logo += 1;
        self.look = if self.since_logo >= LOGO_EVERY || self.systems == 0 {
            self.since_logo = 0;
            Look::Logo
        } else {
            Look::System(self.rng.below(self.systems))
        };
        self.face_w = HEIGHT * face_aspect(self.look);
        // The walk keeps the face's full width, so the narrowing face
        // turns about the center of a box that bounces at an even pace.
        self.walk.resize(self.face_w, HEIGHT, aspect);
    }

    /// The face's width as a fraction of its full width, stepped.
    pub fn width_fraction(&self) -> f32 {
        let t = self.phase / HALF_TURN; // 0 = facing, 1 = edge-on (half turn).
                                        // Facing at t = 0.5, edge-on at 0 and 1: a half turn runs between
                                        // two edge-on points.
        let c = (std::f32::consts::PI * (t - 0.5)).cos().abs();
        let step = (c * STEPS as f32).round() / STEPS as f32;
        step.max(1.0 / STEPS as f32)
    }

    pub fn sprite(&self) -> Sprite {
        let w = self.face_w * self.width_fraction();
        let x = self.walk.x + (self.walk.w - w) / 2.0;
        let mut s = Sprite::new(x, self.walk.y, w, HEIGHT, self.look);
        if self.back {
            s.opacity = BACK_OPACITY;
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn swaps_at_edge_on_and_brings_the_logo_back() {
        let mut spin = Spin::new(16.0 / 9.0, 4.4, 5, 9);
        let mut looks = Vec::new();
        let mut last = spin.sprite().look;
        for _ in 0..(60 * 30) {
            spin.step(1.0 / 30.0, 16.0 / 9.0, |_| 2.0);
            let look = spin.sprite().look;
            if look != last {
                // A swap only happens at the narrowest step.
                assert!(spin.width_fraction() <= 2.0 / STEPS as f32);
                looks.push(look);
                last = look;
            }
        }
        assert!(looks.iter().any(|l| matches!(l, Look::System(_))));
        assert!(looks.contains(&Look::Logo));
    }

    #[test]
    fn without_systems_it_is_always_the_logo() {
        let mut spin = Spin::new(4.0 / 3.0, 4.4, 0, 1);
        for _ in 0..600 {
            spin.step(0.05, 4.0 / 3.0, |_| 4.4);
            assert_eq!(spin.sprite().look, Look::Logo);
        }
    }

    #[test]
    fn stays_inside_the_scene() {
        let mut spin = Spin::new(4.0 / 3.0, 4.4, 3, 2);
        for _ in 0..5000 {
            spin.step(1.0 / 30.0, 4.0 / 3.0, |_| 1.0);
            let s = spin.sprite();
            assert!(s.x >= 0.0 && s.x + s.w <= 4.0 / 3.0 + 1e-4);
            assert!(s.y >= 0.0 && s.y + s.h <= 1.0 + 1e-4);
        }
    }
}
