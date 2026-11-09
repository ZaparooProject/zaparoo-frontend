// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

//! Shapes read out of the logo raster: where the bolt mark sits at the
//! wordmark's left, and the mark's outline as evenly spaced points, which
//! the Polygons style morphs into.

/// An alpha mask, row-major, one byte per pixel.
#[derive(Debug, Clone, Copy)]
pub struct Mask<'a> {
    pub alpha: &'a [u8],
    pub width: usize,
    pub height: usize,
}

impl Mask<'_> {
    fn solid(&self, x: isize, y: isize) -> bool {
        if x < 0 || y < 0 || x as usize >= self.width || y as usize >= self.height {
            return false;
        }
        self.alpha[y as usize * self.width + x as usize] >= 128
    }
}

/// The mark's box `(x, y, width, height)`: the first run of columns with
/// anything opaque in them, up to the first clear column after it, then
/// trimmed to its opaque rows. `None` for an empty mask.
pub fn mark_bounds(mask: Mask<'_>) -> Option<(usize, usize, usize, usize)> {
    let column_has = |x: usize| (0..mask.height).any(|y| mask.alpha[y * mask.width + x] >= 16);
    let start = (0..mask.width).find(|x| column_has(*x))?;
    let end = (start..mask.width)
        .find(|x| !column_has(*x))
        .unwrap_or(mask.width);
    let row_has = |y: usize| (start..end).any(|x| mask.alpha[y * mask.width + x] >= 16);
    let top = (0..mask.height).find(|y| row_has(*y))?;
    let bottom = (top..mask.height).rev().find(|y| row_has(*y))? + 1;
    Some((start, top, end - start, bottom - top))
}

// Clockwise neighbors starting west.
const DIRS: [(isize, isize); 8] = [
    (-1, 0),
    (-1, -1),
    (0, -1),
    (1, -1),
    (1, 0),
    (1, 1),
    (0, 1),
    (-1, 1),
];

/// The outer outline of the first solid shape found scanning rows top to
/// bottom (Moore-neighbor tracing), as pixel corners in order.
pub fn trace(mask: Mask<'_>) -> Vec<(f32, f32)> {
    let mut start = None;
    'scan: for y in 0..mask.height as isize {
        for x in 0..mask.width as isize {
            if mask.solid(x, y) {
                start = Some((x, y));
                break 'scan;
            }
        }
    }
    let Some(start) = start else {
        return Vec::new();
    };
    let mut out = vec![(start.0 as f32, start.1 as f32)];
    let mut current = start;
    // We entered `start` from the west (scanning left to right).
    let mut back = 0usize;
    let limit = mask.width * mask.height * 4;
    for _ in 0..limit {
        let mut found = None;
        for k in 1..=8 {
            let d = (back + k) % 8;
            let (nx, ny) = (current.0 + DIRS[d].0, current.1 + DIRS[d].1);
            if mask.solid(nx, ny) {
                found = Some((d, (nx, ny)));
                break;
            }
        }
        let Some((d, next)) = found else {
            break; // A single pixel.
        };
        if next == start {
            break;
        }
        out.push((next.0 as f32, next.1 as f32));
        // Look again starting from the neighbor before the one we moved to.
        back = (d + 4) % 8;
        current = next;
    }
    out
}

/// `points` (a closed outline) resampled to `n` points evenly spaced
/// along its perimeter.
#[allow(
    clippy::many_single_char_names,
    reason = "geometry reads best in x, y, w, h"
)]
pub fn resample(points: &[(f32, f32)], n: usize) -> Vec<(f32, f32)> {
    if points.is_empty() || n == 0 {
        return Vec::new();
    }
    if points.len() == 1 {
        return vec![points[0]; n];
    }
    let seg = |i: usize| {
        let a = points[i];
        let b = points[(i + 1) % points.len()];
        (a, b, ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt())
    };
    let total: f32 = (0..points.len()).map(|i| seg(i).2).sum();
    let mut out = Vec::with_capacity(n);
    let mut i = 0;
    let mut walked = 0.0;
    for k in 0..n {
        let target = total * k as f32 / n as f32;
        while i < points.len() - 1 && walked + seg(i).2 < target {
            walked += seg(i).2;
            i += 1;
        }
        let (a, b, len) = seg(i);
        let t = if len > 0.0 {
            ((target - walked) / len).clamp(0.0, 1.0)
        } else {
            0.0
        };
        out.push((a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t));
    }
    out
}

/// `points` scaled into a box of height 1 at the origin, keeping their
/// aspect; returns the points and the box's width.
pub fn normalize(points: &[(f32, f32)]) -> (Vec<(f32, f32)>, f32) {
    let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for (x, y) in points {
        x0 = x0.min(*x);
        y0 = y0.min(*y);
        x1 = x1.max(*x);
        y1 = y1.max(*y);
    }
    let h = (y1 - y0).max(1e-6);
    let out = points
        .iter()
        .map(|(x, y)| ((x - x0) / h, (y - y0) / h))
        .collect();
    (out, (x1 - x0) / h)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn square_mask() -> (Vec<u8>, usize, usize) {
        let (w, h) = (20, 10);
        let mut a = vec![0u8; w * h];
        // A 4x6 block at (2, 2), a gap, then "wordmark" pixels.
        for y in 2..8 {
            for x in 2..6 {
                a[y * w + x] = 255;
            }
            a[y * w + 12] = 255;
        }
        (a, w, h)
    }

    #[test]
    fn the_mark_is_the_first_column_run() {
        let (a, w, h) = square_mask();
        let mask = Mask {
            alpha: &a,
            width: w,
            height: h,
        };
        assert_eq!(mark_bounds(mask), Some((2, 2, 4, 6)));
    }

    #[test]
    fn tracing_walks_the_outline_and_resampling_is_even() {
        let (a, w, h) = square_mask();
        let mut only = a.clone();
        for y in 0..h {
            only[y * w + 12] = 0;
        }
        let outline = trace(Mask {
            alpha: &only,
            width: w,
            height: h,
        });
        // The boundary pixels of a 4x6 block.
        assert_eq!(outline.len(), 2 * (4 + 6) - 4);
        let points = resample(&outline, 32);
        assert_eq!(points.len(), 32);
        let (norm, width) = normalize(&points);
        assert!((width - 3.0 / 5.0).abs() < 0.01);
        assert!(norm
            .iter()
            .all(|(x, y)| (0.0..=1.0 + 1e-4).contains(y) && *x >= 0.0));
    }

    #[test]
    fn empty_masks_give_nothing() {
        let a = vec![0u8; 16];
        let mask = Mask {
            alpha: &a,
            width: 4,
            height: 4,
        };
        assert_eq!(mark_bounds(mask), None);
        assert!(trace(mask).is_empty());
        assert!(resample(&[], 8).is_empty());
    }
}
