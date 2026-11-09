// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

//! A box drifting at 45 degrees and bouncing off the scene's edges, the
//! walk the spinning logo shares with the Bouncing style's own.

/// Which edges a step touched.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Hits {
    pub x: bool,
    pub y: bool,
}

impl Hits {
    pub fn any(self) -> bool {
        self.x || self.y
    }
}

/// A box of `w` by `h` at `(x, y)` inside an `aspect` by 1 scene.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Walk {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    dx: f32,
    dy: f32,
}

impl Walk {
    /// Starts at `(x, y)` heading down and right.
    pub fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self {
            x,
            y,
            w,
            h,
            dx: 1.0,
            dy: 1.0,
        }
    }

    /// Move `distance` along each axis and reflect off the edges.
    pub fn step(&mut self, distance: f32, aspect: f32) -> Hits {
        let mut hits = Hits::default();
        self.x += self.dx * distance;
        self.y += self.dy * distance;
        let max_x = (aspect - self.w).max(0.0);
        let max_y = (1.0 - self.h).max(0.0);
        if self.x <= 0.0 {
            self.x = 0.0;
            self.dx = 1.0;
            hits.x = true;
        } else if self.x >= max_x {
            self.x = max_x;
            self.dx = -1.0;
            hits.x = true;
        }
        if self.y <= 0.0 {
            self.y = 0.0;
            self.dy = 1.0;
            hits.y = true;
        } else if self.y >= max_y {
            self.y = max_y;
            self.dy = -1.0;
            hits.y = true;
        }
        hits
    }

    /// Change the box size about its center, kept inside the scene.
    pub fn resize(&mut self, w: f32, h: f32, aspect: f32) {
        let cx = self.x + self.w / 2.0;
        let cy = self.y + self.h / 2.0;
        self.w = w;
        self.h = h;
        self.x = (cx - w / 2.0).clamp(0.0, (aspect - w).max(0.0));
        self.y = (cy - h / 2.0).clamp(0.0, (1.0 - h).max(0.0));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stays_inside_and_reports_wall_hits() {
        let mut walk = Walk::new(0.0, 0.0, 0.3, 0.1);
        let mut hits = 0;
        for _ in 0..10_000 {
            let h = walk.step(0.01, 16.0 / 9.0);
            if h.any() {
                hits += 1;
            }
            assert!(walk.x >= 0.0 && walk.x + walk.w <= 16.0 / 9.0 + 1e-4);
            assert!(walk.y >= 0.0 && walk.y + walk.h <= 1.0 + 1e-4);
        }
        assert!(hits > 10);
    }

    #[test]
    fn resize_keeps_the_center_and_the_bounds() {
        let mut walk = Walk::new(1.5, 0.8, 0.2, 0.2);
        walk.resize(0.4, 0.4, 16.0 / 9.0);
        assert!(walk.x + walk.w <= 16.0 / 9.0 + 1e-6);
        assert!(walk.y + walk.h <= 1.0 + 1e-6);
    }
}
