// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

//! System logos: color system logos fade in at random places, hold, and
//! fade out, two slots taking turns half a cycle apart, with the Zaparoo
//! logo now and then. The view animates the fades; this only says which
//! logo each slot shows, where, and whether it is up.

use super::{Look, Rng, Sprite};

/// Seconds a logo stays up.
pub const HOLD: f32 = 5.0;
/// Seconds a slot stays empty after its logo fades out (the view's fade
/// runs inside this).
pub const GAP: f32 = 1.4;
/// Slots on screen at once.
pub const SLOTS: usize = 2;
/// One in this many appearances is the Zaparoo logo.
pub const LOGO_ODDS: usize = 6;

#[derive(Debug, Clone, Copy, PartialEq)]
struct Slot {
    sprite: Sprite,
    up: bool,
    /// Seconds into this slot's cycle.
    t: f32,
}

#[derive(Debug, Clone)]
pub struct Hops {
    slots: [Slot; SLOTS],
    systems: usize,
    last_system: Option<usize>,
    rng: Rng,
}

impl Hops {
    pub fn new(systems: usize, seed: u64) -> Self {
        let blank = Slot {
            sprite: Sprite::new(0.0, 0.0, 0.0, 0.0, Look::Logo),
            up: false,
            t: 0.0,
        };
        let mut slots = [blank; SLOTS];
        // Stagger: the second slot starts half a cycle later.
        let cycle = HOLD + GAP;
        for (i, slot) in slots.iter_mut().enumerate() {
            slot.t = cycle - cycle * i as f32 / SLOTS as f32 - 0.5;
        }
        Self {
            slots,
            systems,
            last_system: None,
            rng: Rng::new(seed),
        }
    }

    /// Advance by `dt`. `face_aspect` gives a look's width over height.
    /// True when anything visible changed.
    pub fn step(&mut self, dt: f32, aspect: f32, face_aspect: impl Fn(Look) -> f32) -> bool {
        let cycle = HOLD + GAP;
        let mut changed = false;
        for i in 0..SLOTS {
            self.slots[i].t += dt;
            if self.slots[i].up && self.slots[i].t >= HOLD {
                self.slots[i].up = false;
                changed = true;
            }
            if self.slots[i].t >= cycle {
                self.slots[i].t -= cycle;
                let look = self.pick();
                let sprite = self.place(i, look, aspect, &face_aspect);
                self.slots[i].sprite = sprite;
                self.slots[i].up = true;
                changed = true;
            }
        }
        changed
    }

    fn pick(&mut self) -> Look {
        if self.systems == 0 || self.rng.below(LOGO_ODDS) == 0 {
            return Look::Logo;
        }
        let mut n = self.rng.below(self.systems);
        if self.systems > 1 && Some(n) == self.last_system {
            n = (n + 1) % self.systems;
        }
        self.last_system = Some(n);
        Look::System(n)
    }

    /// A place clear of the other slots' boxes, tried a few times.
    fn place(
        &mut self,
        slot: usize,
        look: Look,
        aspect: f32,
        face_aspect: &impl Fn(Look) -> f32,
    ) -> Sprite {
        let h = self.rng.range(0.16, 0.26);
        let w = (h * face_aspect(look)).min(aspect * 0.6);
        let h = w / face_aspect(look).max(0.01);
        let mut best = Sprite::new(0.0, 0.0, w, h, look);
        for _ in 0..12 {
            let x = self.rng.range(0.03, (aspect - w - 0.03).max(0.04));
            let y = self.rng.range(0.04, (1.0 - h - 0.04).max(0.05));
            best = Sprite::new(x, y, w, h, look);
            let clear = self
                .slots
                .iter()
                .enumerate()
                .filter(|(j, _)| *j != slot)
                .all(|(_, other)| !overlaps(&best, &other.sprite, 0.04));
            if clear {
                break;
            }
        }
        best
    }

    /// Each slot's sprite, with opacity 1 while up and 0 while down (the
    /// view fades between them).
    pub fn sprites(&self) -> [Sprite; SLOTS] {
        self.slots.map(|slot| Sprite {
            opacity: if slot.up { 1.0 } else { 0.0 },
            ..slot.sprite
        })
    }
}

fn overlaps(a: &Sprite, b: &Sprite, margin: f32) -> bool {
    a.x < b.x + b.w + margin
        && b.x < a.x + a.w + margin
        && a.y < b.y + b.h + margin
        && b.y < a.y + a.h + margin
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn at_most_two_up_staggered_and_never_overlapping() {
        let mut hops = Hops::new(10, 4);
        let mut both_up = 0;
        let mut shown = 0;
        for _ in 0..(120 * 10) {
            if hops.step(0.1, 16.0 / 9.0, |_| 1.5) {
                shown += 1;
            }
            let s = hops.sprites();
            let up: Vec<_> = s.iter().filter(|s| s.opacity > 0.0).collect();
            if up.len() == 2 {
                both_up += 1;
                assert!(!overlaps(up[0], up[1], 0.0));
            }
            for sprite in up {
                assert!(sprite.x >= 0.0 && sprite.x + sprite.w <= 16.0 / 9.0);
                assert!(sprite.y >= 0.0 && sprite.y + sprite.h <= 1.0);
            }
        }
        assert!(shown > 20);
        assert!(both_up > 0, "the slots overlap in time");
    }

    #[test]
    fn the_zaparoo_logo_shows_now_and_then() {
        let mut hops = Hops::new(10, 11);
        let mut logos = 0;
        for _ in 0..(600 * 10) {
            if hops.step(0.1, 4.0 / 3.0, |_| 1.0) {
                logos += hops
                    .sprites()
                    .iter()
                    .filter(|s| s.opacity > 0.0 && s.look == Look::Logo)
                    .count();
            }
        }
        assert!(logos > 0);
    }
}
