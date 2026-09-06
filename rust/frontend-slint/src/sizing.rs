// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// Sizing for the Slint frontend. The product rules (grid shapes, cover
// decode tiers, type ladders) live in the toolkit-free `zaparoo-app` crate,
// pinned to `tests/fixtures/sizing_golden.txt`, which was dumped from the Qt
// app's `Sizing.qml`; this module only adapts the scene the Slint window
// describes into that crate's `Inputs`. The transition-geometry helpers at
// the bottom are Slint-only (they describe this frontend's own chrome) and
// stay here.

pub use zaparoo_app::sizing::GridShape;
use zaparoo_app::sizing::{self as rules, Inputs, InterfaceProfile};

/// The scene as the Slint window describes it: logical dimensions after the
/// CRT safe-area inset and the TATE transpose, plus the rendering flags the
/// rules key off. Mirrors the writable inputs of the Qt `Sizing` singleton.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Scene {
    pub width: f64,
    pub height: f64,
    /// CRT native path: action-safe canvas, 3x3 / 3x2 grids.
    pub crt: bool,
    /// The 6x8 bitmap face is in use, so type quantizes to 8 or 16 px.
    pub bitmap_fonts: bool,
    /// Rotated (TATE) layout: percentage helpers read the other axis.
    pub swap_axes: bool,
}

impl Scene {
    fn inputs(self) -> Inputs {
        Inputs {
            screen_width: self.width,
            screen_height: self.height,
            crt_native_path: self.crt,
            bitmap_type: self.bitmap_fonts,
            swap_percentage_axes: self.swap_axes,
            // The handheld profile is a Settings feature that has not been
            // ported yet (see the Settings row of the parity ledger in
            // docs/plans/slint-migration.md); every scene is Standard until
            // then.
            interface_profile: InterfaceProfile::Standard,
        }
    }
}

/// Games page shape for the scene (the whole scene is the viewport, as in
/// the demo; the Qt app passes the grid's own area).
pub fn games_grid_shape(scene: Scene) -> GridShape {
    rules::games_grid_shape(&scene.inputs(), scene.width, scene.height)
}

/// Systems page shape for the scene.
pub fn systems_grid_shape(scene: Scene) -> GridShape {
    rules::systems_grid_shape(&scene.inputs(), scene.width, scene.height)
}

/// Stable per-view cover decode size for the games grid, snapped to a Core
/// tier, so the request size equals the decode size.
pub fn games_grid_cover_source_size(scene: Scene) -> u32 {
    tier_to_u32(rules::games_grid_cover_source_size(
        &scene.inputs(),
        scene.width,
        scene.height,
    ))
}

/// Detail-pane cover decode size: about twice the grid cover, snapped to its
/// own tier and capped at what the viewport can express.
pub fn detail_cover_source_size(scene: Scene) -> u32 {
    tier_to_u32(rules::detail_cover_source_size(
        &scene.inputs(),
        scene.width,
        scene.height,
    ))
}

/// Tiers are positive by construction (Core's 128..768 ladder); a negative
/// value would be a rules bug, and the smallest tier is the safe answer.
fn tier_to_u32(tier: i32) -> u32 {
    u32::try_from(tier).unwrap_or(128)
}

#[cfg_attr(
    not(feature = "mister"),
    allow(
        dead_code,
        reason = "only the MiSTer presenters drive cached transitions"
    )
)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BrowseGridTransitionGeometry {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    pub gap: u32,
}

#[cfg_attr(
    not(feature = "mister"),
    allow(
        dead_code,
        reason = "only the MiSTer presenters drive cached transitions"
    )
)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RouteTransitionGeometry {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// Exact non-CRT Systems grid viewport in Slint render pixels. Cached
/// transitions use this rectangle only when the scene is horizontal and
/// rendered one-to-one, leaving header, counter, active label, and help bar
/// stationary. Keep the arithmetic synchronized with `SystemsScreen` and
/// `Sizing` in `ui/app.slint` / `ui/theme.slint`.
#[cfg_attr(
    not(feature = "mister"),
    allow(
        dead_code,
        reason = "only the MiSTer presenters drive cached transitions"
    )
)]
pub fn mister_browse_grid_transition_geometry(
    screen_width: u32,
    screen_height: u32,
    columns: u32,
    rows: u32,
) -> Option<BrowseGridTransitionGeometry> {
    if screen_width == 0 || screen_height == 0 || columns == 0 || rows == 0 {
        return None;
    }

    let pct_w = |percent: f64| (f64::from(screen_width) * percent / 100.0).round() as u32;
    let pct_h = |percent: f64| (f64::from(screen_height) * percent / 100.0).round() as u32;
    let quantize_low = |size: u32| {
        if size < 12 {
            8
        } else if size < 16 {
            12
        } else if size < 20 {
            16
        } else if size < 24 {
            20
        } else {
            24
        }
    };
    let raw_font = pct_h(3.4).max(8);
    let header_row = if screen_height >= 1000 {
        2 * quantize_low((f64::from(raw_font) / 2.0).round() as u32)
    } else {
        quantize_low(raw_font)
    };
    let header_bottom = pct_h(2.0) + 2 * header_row + pct_h(0.8);
    let grid_top = header_bottom + pct_h(8.0);
    let bottom_margin = pct_h(15.0);
    let side_budget = pct_w(8.0);
    let gap = pct_w(1.2);
    let horizontal_gaps = columns.saturating_sub(1).checked_mul(gap)?;
    let vertical_gaps = rows.saturating_sub(1).checked_mul(gap)?;
    let tile_width = screen_width
        .checked_sub(side_budget)?
        .checked_sub(horizontal_gaps)?
        / columns;
    let tile_height = screen_height
        .checked_sub(grid_top)?
        .checked_sub(bottom_margin)?
        .checked_sub(vertical_gaps)?
        / rows;
    if tile_width == 0 || tile_height == 0 {
        return None;
    }
    let grid_width = tile_width
        .checked_mul(columns)?
        .checked_add(horizontal_gaps)?;
    let period = tile_height.checked_add(gap)?.checked_mul(rows)?;
    let grid_height = period.checked_sub(gap)?;
    let x = ((f64::from(screen_width) - f64::from(grid_width)) / 2.0).round() as u32;
    let bottom = grid_top.checked_add(grid_height)?;
    let right = x.checked_add(grid_width)?;
    if right > screen_width || bottom > screen_height {
        return None;
    }

    Some(BrowseGridTransitionGeometry {
        x,
        y: grid_top,
        width: grid_width,
        height: grid_height,
        gap,
    })
}

/// Exact horizontal route-content viewport in Slint render pixels. Header
/// and help chrome remain stationary while cached screen content pushes
/// between them. Keep arithmetic synchronized with `HeaderBar`, `HelpBar`,
/// and `Sizing.header-bottom()` in Slint.
#[cfg_attr(
    not(feature = "mister"),
    allow(
        dead_code,
        reason = "only the MiSTer presenters drive cached transitions"
    )
)]
pub fn mister_route_transition_geometry(
    screen_width: u32,
    screen_height: u32,
) -> Option<RouteTransitionGeometry> {
    if screen_width == 0 || screen_height == 0 {
        return None;
    }

    let pct_h = |percent: f64| (f64::from(screen_height) * percent / 100.0).round() as u32;
    let quantize_low = |size: u32| {
        if size < 12 {
            8
        } else if size < 16 {
            12
        } else if size < 20 {
            16
        } else if size < 24 {
            20
        } else {
            24
        }
    };
    let raw_font = pct_h(3.4).max(8);
    let header_row = if screen_height >= 1000 {
        2 * quantize_low((f64::from(raw_font) / 2.0).round() as u32)
    } else {
        quantize_low(raw_font)
    };
    let y = pct_h(2.0) + 2 * header_row + pct_h(0.8);
    let help_top = screen_height.checked_sub(pct_h(6.0))?;
    let height = help_top.checked_sub(y)?;
    (height > 0).then_some(RouteTransitionGeometry {
        x: 0,
        y,
        width: screen_width,
        height,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_720_browse_transition_matches_slint_grid_viewport() {
        assert_eq!(
            mister_browse_grid_transition_geometry(1280, 720, 4, 3),
            Some(BrowseGridTransitionGeometry {
                x: 52,
                y: 126,
                width: 1177,
                height: 486,
                gap: 15,
            })
        );
    }

    #[test]
    fn browse_transition_rejects_impossible_geometry() {
        assert_eq!(mister_browse_grid_transition_geometry(64, 32, 5, 5), None);
    }

    #[test]
    fn fixed_720_route_transition_leaves_header_and_help_stationary() {
        assert_eq!(
            mister_route_transition_geometry(1280, 720),
            Some(RouteTransitionGeometry {
                x: 0,
                y: 68,
                width: 1280,
                height: 609,
            })
        );
    }

    #[test]
    fn route_transition_rejects_empty_geometry() {
        assert_eq!(mister_route_transition_geometry(0, 720), None);
        assert_eq!(mister_route_transition_geometry(1280, 0), None);
    }
}
