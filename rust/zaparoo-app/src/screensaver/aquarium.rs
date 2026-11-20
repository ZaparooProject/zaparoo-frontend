// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

//! A random school on diagonal passes, with pauses beyond the edges,
//! a one-pixel bob and four intermittent bubble plumes. Artwork and motion
//! keep their native pixel sizes: larger outputs show more open water.

use super::{Look, Rng, Sprite};

/// Population targets interpolate between 240p, 720p and 1080p, capped at 40.
pub fn fish_count_range(height: f32) -> (usize, usize) {
    let (low, high, amount) = if height <= 720.0 {
        (
            (6.0, 10.0),
            (17.0, 25.0),
            ((height - 240.0) / 480.0).clamp(0.0, 1.0),
        )
    } else {
        (
            (17.0, 25.0),
            (30.0, 40.0),
            ((height - 720.0) / 360.0).clamp(0.0, 1.0),
        )
    };
    (
        (low.0 + (high.0 - low.0) * amount).round() as usize,
        (low.1 + (high.1 - low.1) * amount).round() as usize,
    )
}
pub const DECORATION_SIZE: u32 = 128;
/// Four decorations and twenty bubbles precede the fish in the published model.
pub const FISH_ROW: usize = 24;
/// The sand starts at the bottom tenth of the output.
pub const SAND_TOP: f32 = 0.9;
pub const WATER_COLORS: [u32; 5] = [
    0x0008_1e33,
    0x000d_2b42,
    0x0012_364b,
    0x0017_3b49,
    0x0021_4b60,
];
pub const SAND_COLORS: [u32; 5] = [
    0x00e5_d9b6,
    0x00cd_b78c,
    0x00b4_9a70,
    0x0091_8270,
    0x0070_6553,
];
const CORALS: [&str; 12] = [
    "coral/brain-coral.png",
    "coral/branching-coral.png",
    "coral/bubble-coral.png",
    "coral/frogspawn-coral.png",
    "coral/hammer-coral.png",
    "coral/mushroom-coral.png",
    "coral/plate-coral.png",
    "coral/red-sea-fan.png",
    "coral/table-acropora.png",
    "coral/toadstool-leather-coral.png",
    "coral/torch-coral.png",
    "coral/zoanthid-colony.png",
];
const SCENERY: [&str; 12] = [
    "scenery/blue-sea-star.png",
    "scenery/giant-clam.png",
    "scenery/low-reef-rock.png",
    "scenery/reef-arch.png",
    "scenery/reef-cave.png",
    "scenery/reef-pillar.png",
    "scenery/reef-shelf.png",
    "scenery/round-reef-boulder.png",
    "scenery/rubble-pile.png",
    "scenery/sand-patch.png",
    "scenery/seagrass-tuft.png",
    "scenery/spiral-shell.png",
];

/// One static combination, chosen when a run starts and retained while it swims.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Seabed {
    pub water: u32,
    pub sand: u32,
    pub decorations: [&'static str; 4],
}

impl Seabed {
    fn new(rng: &mut Rng) -> Self {
        let mut corals = CORALS;
        let mut scenery = SCENERY;
        rng.shuffle(&mut corals);
        rng.shuffle(&mut scenery);
        Self {
            water: WATER_COLORS[rng.below(WATER_COLORS.len())],
            sand: SAND_COLORS[rng.below(SAND_COLORS.len())],
            decorations: [scenery[0], corals[0], corals[1], scenery[1]],
        }
    }

    /// Guarantee a fresh combination even if two random draws happen to match.
    pub fn avoid_repeat(&mut self, previous: Option<Self>) {
        if previous == Some(*self) {
            let index = WATER_COLORS
                .iter()
                .position(|color| *color == self.water)
                .unwrap_or(0);
            self.water = WATER_COLORS[(index + 1) % WATER_COLORS.len()];
        }
    }
}
const SPEEDS: [f32; 6] = [18.0, 30.0, 22.0, 13.0, 24.0, 10.0];
const ANCHORS: [f32; 4] = [0.13, 0.36, 0.72, 0.91];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Direction {
    Left,
    Right,
}

impl Direction {
    fn sign(self) -> f32 {
        match self {
            Self::Left => -1.0,
            Self::Right => 1.0,
        }
    }

    fn reverse(self) -> Self {
        match self {
            Self::Left => Self::Right,
            Self::Right => Self::Left,
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum Phase {
    Swimming,
    Waiting(f32),
}

#[derive(Debug, Clone, Copy)]
struct Fish {
    catalog_index: usize,
    width: f32,
    height: f32,
    speed: f32,
    max_wait: f32,
    lane: f32,
    bob_phase: f32,
    x: f32,
    y: f32,
    slope: f32,
    direction: Direction,
    phase: Phase,
}

/// A drawable with a vertical offset in output pixels. Keeping this offset
/// separate preserves the bob and bottom anchors on a second, smaller head.
#[derive(Debug, Clone, Copy)]
pub struct Drawing {
    pub sprite: Sprite,
    pub pixel_y: f32,
}

#[derive(Debug, Clone)]
pub struct Aquarium {
    width: f32,
    height: f32,
    elapsed: f32,
    seabed: Seabed,
    fish: Vec<Fish>,
    crt_count: usize,
    /// Negative is the pause before a plume; 0..3 is its visible cycle.
    plumes: [f32; 4],
    rng: Rng,
}

impl Aquarium {
    /// Catalog sizes are prepared native pixels; only selected fish get decoded by the adapter.
    pub fn new(width: f32, height: f32, seed: u64, catalog: &[(u32, u32)]) -> Self {
        let width = width.max(1.0);
        let height = height.max(1.0);
        // Keep the motion generator independent so scenery does not change the bubbles.
        let seabed = Seabed::new(&mut Rng::new(seed));
        let mut rng = Rng::new(seed);
        // Independent draws preserve the school and starting positions for a given seed.
        let mut speed_rng = Rng::new(seed ^ 0xa530_91cd_8e7b_642f);
        let (min, max) = fish_count_range(height);
        let count = (min + rng.below(max - min + 1)).min(catalog.len());
        let mut indices: Vec<_> = (0..catalog.len()).collect();
        rng.shuffle(&mut indices);
        let fish = indices
            .into_iter()
            .take(count)
            .map(|catalog_index| {
                let (w, h) = catalog[catalog_index];
                let y = rng.range(14.0, (height * SAND_TOP - h as f32 - 8.0).max(14.0));
                let movement = rng.below(SPEEDS.len());
                Fish {
                    catalog_index,
                    width: w as f32,
                    height: h as f32,
                    speed: SPEEDS[movement] * speed_rng.range(0.5, 2.5),
                    max_wait: if movement == 1 { 12.0 } else { 6.0 },
                    lane: y / height,
                    bob_phase: rng.unit() * std::f32::consts::TAU,
                    x: rng.range(0.0, (width - w as f32).max(0.0)),
                    y,
                    slope: if rng.coin() { 0.055 } else { -0.055 },
                    direction: if rng.coin() {
                        Direction::Right
                    } else {
                        Direction::Left
                    },
                    phase: Phase::Swimming,
                }
            })
            .collect();
        Self {
            width,
            height,
            elapsed: 0.0,
            seabed,
            fish,
            crt_count: 6 + Rng::new(seed).below(5),
            plumes: [0.7, -1.1, -2.2, -3.3],
            rng: Rng::new(seed),
        }
    }

    pub fn seabed(&self) -> Seabed {
        self.seabed
    }

    pub fn fish_indices(&self) -> impl ExactSizeIterator<Item = usize> + '_ {
        self.fish.iter().map(|fish| fish.catalog_index)
    }

    /// A mirrored CRT shows a smaller subset of the same swimming school.
    pub fn crt_fish_count(&self) -> usize {
        self.crt_count.min(self.fish.len())
    }

    pub fn avoid_repeat(&mut self, previous: Option<Seabed>) {
        self.seabed.avoid_repeat(previous);
    }

    pub fn step(&mut self, dt: f32) {
        // The phase repeats exactly, keeping the bob stable over long runs.
        self.elapsed = (self.elapsed + dt) % 1.6;
        for fish in &mut self.fish {
            let width = fish.width;
            if let Phase::Waiting(remaining) = fish.phase {
                let remaining = (remaining - dt).max(0.0);
                if remaining > 0.0 {
                    fish.phase = Phase::Waiting(remaining);
                } else {
                    fish.phase = Phase::Swimming;
                    fish.direction = fish.direction.reverse();
                    fish.x = match fish.direction {
                        Direction::Right => -width,
                        Direction::Left => self.width,
                    };
                    fish.y = (fish.lane * self.height + self.rng.range(-8.0, 8.0))
                        .clamp(14.0, (self.height * SAND_TOP - fish.height - 8.0).max(14.0));
                }
                continue;
            }
            fish.x += fish.direction.sign() * fish.speed * dt;
            fish.y += fish.speed * fish.slope * dt;
            if fish.y < 14.0 {
                fish.slope = fish.slope.abs();
            }
            if fish.y > self.height * SAND_TOP - fish.height - 8.0 {
                fish.slope = -fish.slope.abs();
            }
            let exited = match fish.direction {
                Direction::Right => fish.x > self.width,
                Direction::Left => fish.x + width < 0.0,
            };
            if exited {
                fish.phase = Phase::Waiting(self.rng.range(2.0, fish.max_wait));
            }
        }
        for phase in &mut self.plumes {
            *phase += dt;
            if *phase > 3.0 {
                *phase = -self.rng.range(3.0, 9.0);
            }
        }
    }

    /// Per-run fixed model slots: four static decorations, twenty bubbles and the school.
    pub fn sprites(&self) -> Vec<Drawing> {
        let mut rows = Vec::with_capacity(FISH_ROW + self.fish.len());
        for (i, anchor) in [0.14, 0.34, 0.66, 0.86].into_iter().enumerate() {
            rows.push(Drawing {
                sprite: Sprite::new(
                    (anchor * self.width - DECORATION_SIZE as f32 / 2.0)
                        .clamp(0.0, (self.width - DECORATION_SIZE as f32).max(0.0))
                        / self.height,
                    SAND_TOP,
                    DECORATION_SIZE as f32 / self.height,
                    DECORATION_SIZE as f32 / self.height,
                    Look::Aquarium(self.fish.len() * 2 + i),
                ),
                // The original canvas has ten transparent pixels below the base.
                pixel_y: 18.0 - DECORATION_SIZE as f32,
            });
        }
        for (anchor, phase) in ANCHORS.into_iter().zip(self.plumes) {
            for i in 0..5 {
                let progress = phase / 3.0 - i as f32 * 0.075;
                let mut sprite = Sprite::new(
                    (anchor * self.width + (progress * 9.0 + i as f32).sin() * 3.0) / self.height,
                    SAND_TOP,
                    if i % 2 == 0 { 2.0 } else { 1.0 } / self.height,
                    if i % 2 == 0 { 2.0 } else { 1.0 } / self.height,
                    Look::Block(0),
                );
                sprite.opacity = if (0.0..=1.0).contains(&progress) {
                    1.0
                } else {
                    0.0
                };
                rows.push(Drawing {
                    sprite,
                    pixel_y: -5.0 - progress * 90.0_f32.min(self.height * 0.38),
                });
            }
        }
        for (i, fish) in self.fish.iter().enumerate() {
            let right = matches!(fish.direction, Direction::Right);
            let mut sprite = Sprite::new(
                fish.x / self.height,
                fish.y / self.height,
                fish.width / self.height,
                fish.height / self.height,
                Look::Aquarium(i + if right { self.fish.len() } else { 0 }),
            );
            if matches!(fish.phase, Phase::Waiting(_)) {
                sprite.opacity = 0.0;
            }
            let bob = (self.elapsed * std::f32::consts::TAU / 1.6 + fish.bob_phase).sin();
            rows.push(Drawing {
                sprite,
                pixel_y: if bob >= 0.0 { 1.0 } else { -1.0 },
            });
        }
        rows
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const CATALOG: [(u32, u32); 178] = [(88, 48); 178];

    #[test]
    fn speed_factors_span_the_requested_range_and_stay_fixed_during_a_run() {
        let mut factors = Vec::new();
        for seed in 0..100 {
            let mut aquarium = Aquarium::new(320.0, 240.0, seed, &CATALOG);
            let speeds: Vec<_> = aquarium.fish.iter().map(|fish| fish.speed).collect();
            for fish in &aquarium.fish {
                assert!((5.0..=75.0).contains(&fish.speed));
                // The fast profile is identified by its existing longer off-screen wait.
                if fish.max_wait > 6.0 {
                    let factor = fish.speed / 30.0;
                    assert!((0.5..=2.5).contains(&factor));
                    factors.push(factor);
                }
            }
            aquarium.step(0.05);
            assert_eq!(
                speeds,
                aquarium
                    .fish
                    .iter()
                    .map(|fish| fish.speed)
                    .collect::<Vec<_>>()
            );
        }
        assert!(factors.iter().any(|factor| *factor < 0.6));
        assert!(factors.iter().any(|factor| *factor > 2.4));
    }

    #[test]
    fn populations_follow_output_size_and_each_start_draws_a_new_school() {
        assert_eq!(fish_count_range(240.0), (6, 10));
        assert_eq!(fish_count_range(720.0), (17, 25));
        assert_eq!(fish_count_range(1080.0), (30, 40));
        assert_eq!(fish_count_range(2160.0), (30, 40));
        for (width, height, min, max) in [
            (320.0, 240.0, 6, 10),
            (1280.0, 720.0, 17, 25),
            (1920.0, 1080.0, 30, 40),
        ] {
            let mut counts = std::collections::BTreeSet::new();
            let mut seen = std::collections::BTreeSet::new();
            let mut previous = Vec::new();
            let mut directions = [false; 2];
            for seed in 0..200 {
                let aquarium = Aquarium::new(width, height, seed, &CATALOG);
                let count = aquarium.fish.len();
                assert!((min..=max).contains(&count));
                assert!((6..=10).contains(&aquarium.crt_fish_count()));
                counts.insert(count);
                let ids: Vec<_> = aquarium.fish_indices().collect();
                assert_ne!(ids, previous);
                let unique: std::collections::BTreeSet<_> = ids.iter().copied().collect();
                assert_eq!(unique.len(), count);
                seen.extend(unique);
                previous = ids;
                for fish in &aquarium.fish {
                    assert!(fish.x >= 0.0 && fish.x + fish.width <= width);
                    assert!(fish.y >= 14.0 && fish.y + fish.height < height * SAND_TOP);
                    directions[usize::from(fish.direction == Direction::Right)] = true;
                }
            }
            assert_eq!(counts.first(), Some(&min));
            assert_eq!(counts.last(), Some(&max));
            assert_eq!(seen.len(), CATALOG.len());
            assert_eq!(directions, [true, true]);
        }
        let empty = Aquarium::new(320.0, 240.0, 42, &[]);
        assert_eq!(empty.sprites().len(), 24);
    }

    #[test]
    fn long_runs_wait_reenter_and_keep_the_same_bounded_slots() {
        for (width, height) in [(320.0, 240.0), (1920.0, 1080.0)] {
            let mut aquarium = Aquarium::new(width, height, 17, &CATALOG);
            let mut waiting = vec![false; aquarium.fish.len()];
            let mut reversed = vec![false; aquarium.fish.len()];
            let initial: Vec<_> = aquarium.fish.iter().map(|fish| fish.direction).collect();
            for _ in 0..24_000 {
                aquarium.step(0.05);
                for (i, fish) in aquarium.fish.iter().enumerate() {
                    waiting[i] |= matches!(fish.phase, Phase::Waiting(_));
                    reversed[i] |= fish.direction != initial[i];
                    assert!(fish.x.is_finite() && fish.y.is_finite());
                    assert!(fish.y >= 10.0 && fish.y <= height - fish.height);
                    assert!(fish.y + fish.height < height * SAND_TOP);
                }
                assert_eq!(aquarium.sprites().len(), 24 + aquarium.fish.len());
            }
            assert!(waiting.into_iter().all(|seen| seen));
            assert!(reversed.into_iter().all(|seen| seen));
        }
    }

    #[test]
    fn fish_sizes_speeds_and_bob_are_in_output_pixels() {
        for height in [240.0, 1080.0] {
            let mut aquarium = Aquarium::new(height * 4.0 / 3.0, height, 8, &CATALOG);
            let initial_x = aquarium.fish[0].x;
            let speed = aquarium.fish[0].speed;
            aquarium.step(0.05);
            assert!(((aquarium.fish[0].x - initial_x).abs() - speed * 0.05).abs() < 0.001);
            let mut bob = [false; 2];
            for _ in 0..32 {
                for (i, row) in aquarium.sprites()[24..].iter().enumerate() {
                    assert!((row.sprite.w * height - aquarium.fish[i].width).abs() < 0.001);
                    assert!((row.pixel_y.abs() - 1.0).abs() < f32::EPSILON);
                    bob[usize::from(row.pixel_y > 0.0)] = true;
                }
                aquarium.step(0.05);
            }
            assert_eq!(bob, [true, true]);
        }
    }

    #[test]
    fn seabed_is_stable_during_a_run_and_varies_between_starts() {
        let mut combinations = std::collections::HashSet::new();
        for seed in 0..100 {
            let mut aquarium = Aquarium::new(320.0, 240.0, seed, &CATALOG);
            let seabed = aquarium.seabed();
            assert!(WATER_COLORS.contains(&seabed.water));
            assert!(SAND_COLORS.contains(&seabed.sand));
            assert_eq!(
                seabed
                    .decorations
                    .iter()
                    .filter(|p| p.starts_with("coral/"))
                    .count(),
                2
            );
            assert_eq!(
                seabed
                    .decorations
                    .iter()
                    .filter(|p| p.starts_with("scenery/"))
                    .count(),
                2
            );
            assert_ne!(seabed.decorations[0], seabed.decorations[3]);
            assert_ne!(seabed.decorations[1], seabed.decorations[2]);
            combinations.insert((seabed.water, seabed.sand, seabed.decorations));
            aquarium.step(60.0);
            assert_eq!(aquarium.seabed(), seabed);
            aquarium.avoid_repeat(Some(seabed));
            assert_ne!(aquarium.seabed(), seabed);
        }
        assert!(combinations.len() > 90);
    }

    #[test]
    fn seeded_pauses_and_bubble_phases_are_repeatable() {
        let mut a = Aquarium::new(320.0, 240.0, 42, &CATALOG);
        let mut b = a.clone();
        for _ in 0..2400 {
            a.step(0.05);
            b.step(0.05);
        }
        for (left, right) in a.sprites().iter().zip(b.sprites()) {
            assert_eq!(left.sprite, right.sprite);
            assert_eq!(left.pixel_y.to_bits(), right.pixel_y.to_bits());
        }
        assert!((a.plumes[0] - a.plumes[1]).abs() > f32::EPSILON);
    }
}
