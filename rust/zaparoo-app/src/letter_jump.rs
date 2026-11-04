// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

//! Area fit and row-local navigation for the letter-jump modal.

/// Pick the columns that maximize square cell size within both axes.
/// The comparison is strict, so ties keep the first candidate.
pub fn fit_columns(count: usize, width: f64, height: f64, gap: f64) -> usize {
    let mut best = 1;
    let mut best_cell = 0.0_f64;
    for columns in 1..=count {
        let rows = count.div_ceil(columns);
        let cell = ((width - (columns - 1) as f64 * gap) / columns as f64)
            .min((height - (rows - 1) as f64 * gap) / rows as f64);
        if cell > best_cell {
            best_cell = cell;
            best = columns;
        }
    }
    best
}

/// A full set of sections: `#` and the 26 letters. A list with fewer
/// sections keeps the cell size a full set would get, rather than growing
/// a handful of buttons to fill the panel.
pub const FULL_SET: usize = 27;

/// The square cell for `count` sections laid out in `columns`: what fits
/// both axes, and never larger than a full set's cell.
pub fn cell_size(count: usize, columns: usize, width: f64, height: f64, gap: f64) -> f64 {
    let fit = |count: usize, columns: usize| {
        let columns = columns.max(1);
        let rows = count.max(1).div_ceil(columns);
        ((width - (columns - 1) as f64 * gap) / columns as f64)
            .min((height - (rows - 1) as f64 * gap) / rows as f64)
    };
    let full = FULL_SET.max(count);
    fit(count, columns)
        .min(fit(full, fit_columns(full, width, height, gap)))
        .max(0.0)
}

/// Horizontal moves wrap within the current row, including a partial tail.
/// Vertical moves wrap between the first and last rows, clamping the column
/// to the final cell when the tail is partial.
pub fn next_index(action: &str, index: usize, count: usize, columns: usize) -> usize {
    if count == 0 {
        return 0;
    }
    let index = index.min(count - 1);
    let columns = columns.max(1);
    let start = index / columns * columns;
    let end = (start + columns - 1).min(count - 1);
    match action {
        "left" => {
            if index == start {
                end
            } else {
                index - 1
            }
        }
        "right" => {
            if index == end {
                start
            } else {
                index + 1
            }
        }
        "up" => index.checked_sub(columns).unwrap_or_else(|| {
            let last_start = (count - 1) / columns * columns;
            (last_start + index).min(count - 1)
        }),
        "down" => {
            let last_start = (count - 1) / columns * columns;
            if index < last_start {
                (index + columns).min(count - 1)
            } else {
                index % columns
            }
        }
        _ => index,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn qt_row_wrap_and_partial_tail_cases() {
        for (action, index, count, cols, expected) in [
            ("right", 3, 8, 4, 0),
            ("right", 7, 8, 4, 4),
            ("left", 0, 8, 4, 3),
            ("left", 4, 8, 4, 7),
            ("right", 5, 6, 4, 4),
            ("left", 4, 6, 4, 5),
            ("down", 1, 8, 4, 5),
            ("up", 5, 8, 4, 1),
            ("up", 2, 8, 4, 6),
            ("down", 2, 6, 4, 5),
            ("down", 5, 6, 4, 1),
            ("right", 0, 0, 4, 0),
        ] {
            assert_eq!(next_index(action, index, count, cols), expected);
        }
    }

    #[test]
    fn vertical_wrap_reaches_both_edges_and_clamps_a_partial_tail() {
        for (action, index, count, columns, expected) in [
            ("up", 1, 12, 4, 9),
            ("down", 9, 12, 4, 1),
            ("up", 3, 10, 4, 9),
            ("down", 9, 10, 4, 1),
            ("down", 7, 10, 4, 9),
            ("up", 9, 10, 4, 5),
            ("up", 1, 3, 4, 1),
            ("down", 2, 3, 4, 2),
            ("up", 0, 3, 0, 2),
            ("down", 2, 3, 0, 0),
            ("up", usize::MAX, 10, 4, 5),
            ("up", 0, 0, 4, 0),
        ] {
            assert_eq!(
                next_index(action, index, count, columns),
                expected,
                "{action} from {index}, {count}/{columns}"
            );
        }
    }

    #[test]
    fn qt_area_fit_cases() {
        assert_eq!(fit_columns(0, 1000.0, 500.0, 10.0), 1);
        assert_eq!(fit_columns(3, 800.0, 200.0, 10.0), 3);
        let cols = fit_columns(28, 900.0, 350.0, 10.0);
        assert!(cols >= 7);
        assert!(fit_columns(28, 1400.0, 400.0, 10.0) > fit_columns(28, 600.0, 400.0, 10.0));
        assert!(fit_columns(28, 300.0, 600.0, 10.0) < cols);
    }

    #[test]
    fn a_short_list_keeps_a_full_sets_cell_size() {
        let (width, height, gap) = (1500.0, 560.0, 19.0);
        let full = cell_size(
            FULL_SET,
            fit_columns(FULL_SET, width, height, gap),
            width,
            height,
            gap,
        );
        for count in [1, 3, 8, 15] {
            let cell = cell_size(
                count,
                fit_columns(count, width, height, gap),
                width,
                height,
                gap,
            );
            assert!((cell - full).abs() < 0.01, "{count}: {cell} vs {full}");
        }
        // More sections than a full set shrink to fit.
        let more = cell_size(40, fit_columns(40, width, height, gap), width, height, gap);
        assert!(more < full);
    }
}
