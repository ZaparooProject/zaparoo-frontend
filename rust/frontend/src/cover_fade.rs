// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

//! Grid covers that land while their tile is on screen fade in. On `MiSTer`
//! the presenter does the fade on the way to the display, so Slint paints
//! each cover once (`Motion.tile-cover-reveal-ms` is 0 there); a driver
//! that publishes a grid page hands the cells whose covers just landed to
//! `request`. Elsewhere Slint fades them itself and this does nothing.

use crate::App;
use zaparoo_app::paged_grid::{Fit, Insets};

/// Where a grid page's cells sit in the scene: `top` is the grid's own
/// top edge, which `fit` and `insets` lay the cells out from.
#[cfg_attr(
    not(any(feature = "mister", test)),
    allow(dead_code, reason = "only the MiSTer presenters fade covers")
)]
pub struct Grid<'a> {
    pub fit: &'a Fit,
    pub insets: &'a Insets,
    pub top: i32,
    pub columns: i32,
}

/// Fades in the covers that just landed on the page-local cells `landed`
/// (from `view_model::publish_cells`). `selected` is the focused cell,
/// which is drawn grown by the focus zoom.
#[cfg(feature = "mister")]
pub fn request(app: &App, grid: &Grid, landed: &[usize], selected: Option<usize>) {
    use slint::ComponentHandle;
    let motion = app.global::<crate::Motion>();
    if landed.is_empty() || !motion.get_enabled() || crate::motion_test().cover_fade() {
        return;
    }
    let grow = if motion.get_zoom_by_size() {
        (motion.get_focus_zoom() / 100.0 - 1.0).max(0.0)
    } else {
        0.0
    };
    let rects: Vec<_> = landed
        .iter()
        .map(|&index| {
            cell_rect(
                grid,
                index,
                if selected == Some(index) { grow } else { 0.0 },
            )
        })
        .collect();
    crate::mister::request_cover_fades(app, &rects);
}

#[cfg(not(feature = "mister"))]
pub fn request(_app: &App, _grid: &Grid, _landed: &[usize], _selected: Option<usize>) {}

/// The scene region cell `index` covers, grown about its center by `grow`
/// (0.04 for a 104% zoom) the way `ui/tiles.slint` grows the focused cell.
/// Rounded outward to whole pixels and clipped at the scene's top and left.
#[cfg(any(feature = "mister", test))]
fn cell_rect(grid: &Grid, index: usize, grow: f32) -> crate::frame_transition::Rect {
    let columns = grid.columns.max(1);
    let index = i32::try_from(index).unwrap_or(i32::MAX);
    let cell =
        zaparoo_app::paged_grid::cell_rect(grid.fit, grid.insets, index / columns, index % columns);
    let grow_x = cell.width as f32 * grow / 2.0;
    let grow_y = cell.height as f32 * grow / 2.0;
    let x0 = (cell.x as f32 - grow_x).floor().max(0.0);
    let y0 = ((grid.top + cell.y) as f32 - grow_y).floor().max(0.0);
    let x1 = ((cell.x + cell.width) as f32 + grow_x).ceil().max(x0);
    let y1 = ((grid.top + cell.y + cell.height) as f32 + grow_y)
        .ceil()
        .max(y0);
    crate::frame_transition::Rect {
        x: x0 as usize,
        y: y0 as usize,
        width: (x1 - x0) as usize,
        height: (y1 - y0) as usize,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frame_transition::Rect;

    #[test]
    fn a_cell_is_its_slot_and_the_focused_one_grows_about_its_center() {
        let fit = zaparoo_app::paged_grid::fit(3, 2, 320, 200, None, false, &Insets::default());
        let insets = Insets::default();
        let grid = Grid {
            fit: &fit,
            insets: &insets,
            top: 20,
            columns: 3,
        };
        let (width, height) = (fit.cell_width as usize, fit.cell_height as usize);
        assert_eq!(
            cell_rect(&grid, 4, 0.0),
            Rect {
                x: width + fit.block_offset_x as usize,
                y: 20 + height + fit.block_offset_y as usize,
                width,
                height,
            }
        );
        let grown = cell_rect(&grid, 4, 0.04);
        let rest = cell_rect(&grid, 4, 0.0);
        assert!(grown.x < rest.x && grown.y < rest.y);
        assert!(grown.x + grown.width > rest.x + rest.width);
        assert!(grown.y + grown.height > rest.y + rest.height);
    }
}
