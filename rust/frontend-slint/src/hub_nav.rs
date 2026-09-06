// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// Pure navigation math for the Hub's two centered rows and the Games
// grid. Cross-row jumps map by horizontal center distance, not by
// index clamp: the bottom row is centered under the top row, so the
// nearest tile visually is the nearest tile logically.

/// Tile width and gap in percent of screen width, mirroring
/// `ui/app.slint`'s `HubScreen`: both rows share square cells of
/// min(pctH(22), fit) with pctW(3) spacing. pctH(22) at 16:9 is
/// ~12.4% of width; the mapping only needs the cell:gap ratio, so the
/// approximation holds across resolutions.
pub const HUB_TILE_W: f32 = 12.4;
pub const HUB_GAP: f32 = 3.0;

/// Horizontal center of tile `i` in a centered row of `n` tiles,
/// relative to the screen center, in percent of screen width.
fn center_of(i: usize, n: usize, tile_w: f32, gap: f32) -> f32 {
    let mid = (n as f32 - 1.0) / 2.0;
    (i as f32 - mid) * (tile_w + gap)
}

/// Index in the target row whose center is horizontally nearest to
/// tile `from` in the source row. Returns 0 for an empty target so
/// callers can clamp uniformly.
pub fn nearest_by_center(from: usize, from_n: usize, from_w: f32, to_n: usize, to_w: f32) -> usize {
    if to_n == 0 {
        return 0;
    }
    let x = center_of(from, from_n, from_w, HUB_GAP);
    let mut best = 0usize;
    let mut best_d = f32::MAX;
    for j in 0..to_n {
        let d = (center_of(j, to_n, to_w, HUB_GAP) - x).abs();
        if d < best_d {
            best_d = d;
            best = j;
        }
    }
    best
}

/// Clamped horizontal move inside a row. `delta` is -1 or +1.
pub fn row_move(index: usize, len: usize, delta: i32) -> usize {
    if len == 0 {
        return 0;
    }
    let next = index as i64 + i64::from(delta);
    next.clamp(0, len as i64 - 1) as usize
}

/// Grid move on a fixed-column grid, clamped to the item count.
/// Horizontal moves stop at row edges only when there is no next item;
/// vertical moves stay in the same column when possible.
pub fn grid_move(index: usize, len: usize, cols: usize, dx: i32, dy: i32) -> usize {
    if len == 0 || cols == 0 {
        return 0;
    }
    let next = if dy == 0 {
        index as i64 + i64::from(dx)
    } else {
        index as i64 + i64::from(dy) * cols as i64
    };
    if dy != 0 && (next < 0 || next >= len as i64) {
        // Vertical move out of range keeps the current position rather
        // than jumping to first/last, so the column is preserved.
        return index;
    }
    next.clamp(0, len as i64 - 1) as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn center_row_maps_middle_to_middle() {
        // 5 categories on top, 4 actions below: the center category
        // (index 2) sits between actions 1 and 2; either is nearest
        // depending on exact widths, but it must never map to an edge.
        let j = nearest_by_center(2, 5, HUB_TILE_W, 4, HUB_TILE_W);
        assert!(j == 1 || j == 2, "center should map near center, got {j}");
    }

    #[test]
    fn leftmost_maps_leftward_not_by_index() {
        // 8 categories over 4 actions: category 0 is far left of
        // action 0's center only when rows are left-aligned; centered
        // rows put action 0 nearest. Index-clamp would also say 0 here,
        // so assert the far-right case too for the real distinction.
        assert_eq!(nearest_by_center(0, 8, HUB_TILE_W, 4, HUB_TILE_W), 0);
        assert_eq!(nearest_by_center(7, 8, HUB_TILE_W, 4, HUB_TILE_W), 3);
    }

    #[test]
    fn single_item_source_maps_to_a_center_tile_and_single_target_to_zero() {
        // One centered category over four actions: its center (0) is
        // equidistant from actions 1 and 2; first-best wins, so 1.
        assert_eq!(nearest_by_center(0, 1, HUB_TILE_W, 4, HUB_TILE_W), 1);
        assert_eq!(nearest_by_center(3, 4, HUB_TILE_W, 1, HUB_TILE_W), 0);
    }

    #[test]
    fn empty_target_returns_zero() {
        assert_eq!(nearest_by_center(2, 5, HUB_TILE_W, 0, HUB_TILE_W), 0);
    }

    #[test]
    fn row_move_clamps_at_edges() {
        assert_eq!(row_move(0, 5, -1), 0);
        assert_eq!(row_move(4, 5, 1), 4);
        assert_eq!(row_move(2, 5, 1), 3);
        assert_eq!(row_move(0, 0, 1), 0);
    }

    #[test]
    fn grid_move_horizontal_clamps_to_len() {
        // 10 items, 4 cols: last row has 2 items.
        assert_eq!(grid_move(9, 10, 4, 1, 0), 9);
        assert_eq!(grid_move(8, 10, 4, 1, 0), 9);
        assert_eq!(grid_move(0, 10, 4, -1, 0), 0);
    }

    #[test]
    fn grid_move_vertical_preserves_column_or_stays() {
        assert_eq!(grid_move(1, 10, 4, 0, 1), 5);
        assert_eq!(grid_move(5, 10, 4, 0, -1), 1);
        // Moving down from index 7 (row 1, col 3) would land on 11,
        // past the end: stay put instead of clamping to 9.
        assert_eq!(grid_move(7, 10, 4, 0, 1), 7);
        assert_eq!(grid_move(2, 10, 4, 0, -1), 2);
    }
}
