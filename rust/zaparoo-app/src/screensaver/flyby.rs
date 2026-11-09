// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

//! Flyby: a slow parade from the top right to the bottom left of the
//! Zaparoo logo, system logos and covers, at three depths: far ones small
//! and slow, near ones large and quicker, drawn over the far ones. The
//! count is capped, two per depth.

use super::{Look, Rng, Sprite};

/// Depth layers, far to near: (height, speed in height units a second).
pub const LAYERS: [(f32, f32); 3] = [(0.11, 0.05), (0.18, 0.085), (0.28, 0.13)];
/// Slots per layer.
pub const PER_LAYER: usize = 2;
/// Seconds between launches, at least.
pub const LAUNCH_EVERY: f32 = 2.5;
/// A cover's width over its height.
pub const COVER_ASPECT: f32 = 0.75;

#[derive(Debug, Clone, Copy, PartialEq)]
struct Item {
    sprite: Sprite,
    speed: f32,
}

#[derive(Debug, Clone)]
pub struct Flyby {
    slots: [Option<Item>; LAYERS.len() * PER_LAYER],
    since_launch: f32,
    systems: usize,
    covers: usize,
    rng: Rng,
}

/// The parade's heading: down and to the left, a little flatter than 45
/// degrees on a wide screen.
fn heading(aspect: f32) -> (f32, f32) {
    let (dx, dy) = (-aspect, 1.0);
    let len = (dx * dx + dy * dy).sqrt();
    (dx / len, dy / len)
}

impl Flyby {
    pub fn new(systems: usize, covers: usize, seed: u64) -> Self {
        Self {
            slots: [None; LAYERS.len() * PER_LAYER],
            since_launch: LAUNCH_EVERY,
            systems,
            covers,
            rng: Rng::new(seed),
        }
    }

    pub fn step(&mut self, dt: f32, aspect: f32, face_aspect: impl Fn(Look) -> f32) {
        let (hx, hy) = heading(aspect);
        for slot in &mut self.slots {
            if let Some(item) = slot {
                item.sprite.x += hx * item.speed * dt;
                item.sprite.y += hy * item.speed * dt;
                if item.sprite.x + item.sprite.w < 0.0 || item.sprite.y > 1.0 {
                    *slot = None;
                }
            }
        }
        self.since_launch += dt;
        if self.since_launch >= LAUNCH_EVERY {
            let layer = self.rng.below(LAYERS.len());
            let base = layer * PER_LAYER;
            if let Some(free) = (base..base + PER_LAYER).find(|i| self.slots[*i].is_none()) {
                self.since_launch = -self.rng.range(0.0, LAUNCH_EVERY);
                let look = self.pick();
                let (tall, speed) = LAYERS[layer];
                let ratio = match look {
                    Look::Cover(_) => COVER_ASPECT,
                    _ => face_aspect(look),
                };
                let wide = (tall * ratio).min(aspect * 0.5);
                let tall = wide / ratio.max(0.01);
                // Enter across the top edge or down the right one, off
                // screen, anywhere along the band the heading crosses.
                let along = self.rng.range(0.0, aspect + 1.0);
                let (x, y) = if along < aspect {
                    (along + 0.3, -tall)
                } else {
                    (aspect, along - aspect - tall)
                };
                self.slots[free] = Some(Item {
                    sprite: Sprite::new(x, y, wide, tall, look),
                    speed,
                });
            }
        }
    }

    fn pick(&mut self) -> Look {
        let choices = 1 + usize::from(self.systems > 0) + usize::from(self.covers > 0);
        match self.rng.below(choices + 1) {
            0 => Look::Logo,
            1 | 2 if self.covers > 0 && (self.systems == 0 || self.rng.coin()) => {
                Look::Cover(self.rng.below(self.covers))
            }
            _ if self.systems > 0 => Look::System(self.rng.below(self.systems)),
            _ if self.covers > 0 => Look::Cover(self.rng.below(self.covers)),
            _ => Look::Logo,
        }
    }

    /// One sprite per slot, far layers first; empty slots have opacity 0.
    pub fn sprites(&self) -> Vec<Sprite> {
        self.slots
            .iter()
            .map(|slot| match slot {
                Some(item) => item.sprite,
                None => Sprite {
                    opacity: 0.0,
                    ..Sprite::new(0.0, 0.0, 0.0, 0.0, Look::Logo)
                },
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn items_cross_the_screen_and_leave() {
        let mut f = Flyby::new(3, 5, 1);
        let mut looks = std::collections::HashSet::new();
        let mut visible_frames = 0;
        for _ in 0..(30 * 300) {
            f.step(1.0 / 30.0, 16.0 / 9.0, |_| 1.5);
            let sprites = f.sprites();
            assert_eq!(sprites.len(), LAYERS.len() * PER_LAYER);
            for s in sprites.iter().filter(|s| s.opacity > 0.0) {
                looks.insert(std::mem::discriminant(&s.look));
                if s.x < 16.0 / 9.0 && s.x + s.w > 0.0 && s.y < 1.0 && s.y + s.h > 0.0 {
                    visible_frames += 1;
                }
            }
        }
        assert_eq!(looks.len(), 3, "logo, systems and covers all fly");
        assert!(visible_frames > 1000);
    }

    #[test]
    fn without_art_it_is_the_logo() {
        let mut f = Flyby::new(0, 0, 2);
        for _ in 0..3000 {
            f.step(0.05, 4.0 / 3.0, |_| 4.4);
            assert!(f
                .sprites()
                .iter()
                .all(|s| s.opacity == 0.0 || s.look == Look::Logo));
        }
    }
}
