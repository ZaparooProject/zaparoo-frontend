// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// Pure navigation math for the fixed-column grids that have not moved
// onto `zaparoo_app::paged_grid` yet (the Settings root grid and the
// games strip).

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
