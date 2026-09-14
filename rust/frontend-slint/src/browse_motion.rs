//! Minimal scrolling for Slint's bounded list window. Unlike the Qt centered
//! window, selection remains stationary until it reaches a viewport edge.

pub fn window_top(current: usize, count: usize, visible: usize, previous: usize) -> usize {
    let visible = visible.max(1);
    previous
        .min(current)
        .max(current.saturating_sub(visible - 1))
        .min(count.saturating_sub(visible))
}

#[cfg(test)]
mod tests {
    use super::window_top;

    #[test]
    fn focus_moves_without_recentering_until_it_reaches_an_edge() {
        assert_eq!(window_top(4, 100, 8, 0), 0);
        assert_eq!(window_top(7, 100, 8, 0), 0);
        assert_eq!(window_top(8, 100, 8, 0), 1);
        assert_eq!(window_top(7, 100, 8, 1), 1);
        assert_eq!(window_top(0, 100, 8, 1), 0);
    }

    #[test]
    fn jumps_shrinkage_and_empty_models_clamp_the_window() {
        assert_eq!(window_top(99, 100, 8, 1), 92);
        assert_eq!(window_top(0, 0, 0, 92), 0);
        assert_eq!(window_top(2, 3, 8, 92), 0);
        assert_eq!(window_top(2, 100, 1, 0), 2);
    }
}
