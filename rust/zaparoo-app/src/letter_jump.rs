// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

//! Area fit and row-local navigation from `LetterJumpModal.qml`.

/// Pick the columns that maximize square cell size within both axes.
/// Ties keep the first candidate, matching Qt's strict comparison.
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

/// Horizontal moves wrap within the current row, including a partial tail.
/// Vertical moves stop at the edges; down from a full row reaches the tail.
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
        "up" => index.checked_sub(columns).unwrap_or(index),
        "down" => {
            let last_start = (count - 1) / columns * columns;
            if index < last_start {
                (index + columns).min(count - 1)
            } else {
                index
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
            ("up", 2, 8, 4, 2),
            ("down", 2, 6, 4, 5),
            ("down", 5, 6, 4, 5),
            ("right", 0, 0, 4, 0),
        ] {
            assert_eq!(next_index(action, index, count, cols), expected);
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
}
