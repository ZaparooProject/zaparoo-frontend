// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// Sizing for the frontend. The product rules (grid shapes, cover decode
// tiers, type ladders) live in the toolkit-free `zaparoo-app` crate,
// pinned to `rust/zaparoo-app/tests/fixtures/sizing_golden.txt`; this
// module only adapts the scene the Slint window describes into that
// crate's `Inputs`. The transition-geometry helpers at the bottom are
// Slint-only (they describe this frontend's own chrome) and stay here.

use crate::{App, GamesView, Layout, Motion, Shell, Sizing as SizingGlobal, SystemsView};
use slint::ComponentHandle;
use zaparoo_app::layouts::{self, Body, Profile, ThemeId, View};
pub use zaparoo_app::sizing::GridShape;
use zaparoo_app::sizing::{self as rules, Derived, Inputs, InterfaceProfile};

/// The scene as the Slint window describes it: logical dimensions after the
/// CRT safe-area inset and the TATE transpose, plus the rendering flags the
/// rules key off. One field per writable sizing input.
#[derive(Debug, Clone, Copy, PartialEq)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "one flag per writable sizing input the scene carries"
)]
pub struct Scene {
    pub width: f64,
    pub height: f64,
    /// CRT native path: action-safe canvas, 3x3 / 3x2 grids.
    pub crt: bool,
    /// The 6x8 bitmap face is in use, so type quantizes to 8 or 16 px.
    pub bitmap_fonts: bool,
    /// Rotated (TATE) layout: percentage helpers read the other axis.
    pub swap_axes: bool,
    /// The handheld interface profile (Settings > Display).
    pub handheld: bool,
    /// The help entries on screen wrap, so a 240p help bar is two rows.
    pub help_bar_two_rows: bool,
}

impl Scene {
    /// The scene for logical dimensions, with the rendering flags read
    /// from the app's `Sizing` global.
    pub fn of(app: &App, width: f64, height: f64, crt: bool) -> Self {
        let sizing = app.global::<SizingGlobal>();
        Self {
            width,
            height,
            crt,
            bitmap_fonts: sizing.get_bitmap_fonts(),
            swap_axes: sizing.get_swap_axes(),
            handheld: sizing.get_handheld(),
            help_bar_two_rows: sizing.get_help_bar_two_rows(),
        }
    }

    pub fn inputs(self) -> Inputs {
        Inputs {
            screen_width: self.width,
            screen_height: self.height,
            crt_native_path: self.crt,
            bitmap_type: self.bitmap_fonts,
            swap_percentage_axes: self.swap_axes,
            interface_profile: if self.handheld {
                InterfaceProfile::Handheld
            } else {
                InterfaceProfile::Standard
            },
            help_bar_two_rows: self.help_bar_two_rows,
        }
    }
}

/// Copy the help bar's wrap measurement into the `Sizing` input the scene
/// reads. Returns whether it changed, so the caller re-solves only then.
pub fn sync_help_bar_rows(app: &App) -> bool {
    let wraps = app.get_help_entries_wrap();
    let sizing = app.global::<SizingGlobal>();
    if sizing.get_help_bar_two_rows() == wraps {
        return false;
    }
    sizing.set_help_bar_two_rows(wraps);
    true
}

/// Push everything the scene decides: the derived `Sizing` table, the
/// page grid shapes and the browse layout profile for the view on screen.
/// Called on every viewport, orientation or profile change.
pub fn apply_scene(app: &App, scene: Scene) {
    let inputs = scene.inputs();
    apply_derived(app, &rules::derive(&inputs));
    let games = games_grid_shape(scene);
    app.global::<GamesView>().set_columns(games.columns);
    app.global::<GamesView>().set_rows(games.rows);
    let systems = systems_grid_shape(scene);
    app.global::<SystemsView>().set_columns(systems.columns);
    app.global::<SystemsView>().set_rows(systems.rows);
    apply_layout(app, &inputs);
}

/// Re-resolve only the layout profile, for a view change at the same
/// geometry (screen switch, browse layout setting).
pub fn refresh_layout(app: &App, scene: Scene) {
    apply_layout(app, &scene.inputs());
}

/// The grid and list views of the active screen's family: Systems screens
/// follow the systems tables, every other screen the games ones, and a
/// rotated scene uses the TATE list.
fn family_views(app: &App) -> (View, View) {
    let shell = app.global::<Shell>();
    let rotated = matches!(
        shell.get_orientation(),
        crate::Orientation::Cw | crate::Orientation::Ccw
    );
    match shell.get_active_screen() {
        crate::Screen::Systems | crate::Screen::FavoriteSystems => (
            View::SystemsGrid,
            if rotated {
                View::SystemsListTate
            } else {
                View::SystemsList
            },
        ),
        _ => (
            View::GamesGrid,
            if rotated {
                View::GamesListTate
            } else {
                View::GamesList
            },
        ),
    }
}

/// `MainLayout._browseViewId`: Systems screens follow the systems layout
/// preference, games-style screens the games one; every other screen
/// resolves against the games preference.
fn current_view(app: &App) -> View {
    let shell = app.global::<Shell>();
    let (grid, list) = family_views(app);
    let list_layout = match shell.get_active_screen() {
        crate::Screen::Systems | crate::Screen::FavoriteSystems => shell.get_systems_list_layout(),
        _ => shell.get_browse_list_layout(),
    };
    if list_layout {
        list
    } else {
        grid
    }
}

fn apply_layout(app: &App, inputs: &Inputs) {
    app.global::<SizingGlobal>()
        .on_fit_letter_columns(|count, width, height, gap| {
            zaparoo_app::letter_jump::fit_columns(
                count.max(0) as usize,
                f64::from(width),
                f64::from(height),
                f64::from(gap),
            ) as i32
        });
    let theme = ThemeId::current(inputs);
    let profile = layouts::profile(theme, current_view(app), inputs);
    // `Layout` carries a grid table and a list table, and views read both
    // whatever the browse layout is: Search takes the list card's side
    // margin, a list keeps the grid footer's cue slot. Both are resolved
    // on every push so neither holds a `.slint` default or the last
    // screen's values.
    let (grid_view, list_view) = family_views(app);
    let grid = layouts::profile(theme, grid_view, inputs);
    let list = layouts::profile(theme, list_view, inputs);
    // The page cue's size and placement live in the grid tables, but the
    // cue itself is on screen in both layouts, on every screen.
    let cue = layouts::profile(theme, View::GamesGrid, inputs);
    push_profile(app, &profile, &grid, &list, &cue);
}

fn px(value: i32) -> f32 {
    value as f32
}

#[allow(
    clippy::too_many_lines,
    reason = "one setter per derived sizing value keeps the inventory reviewable"
)]
/// The `zoom` name of the `ZAPAROO_MOTION` test switch, as startup read it.
static FOCUS_ZOOM_DISABLED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// Record whether the test switch turned the focus zoom off. The zoom is
/// pushed with every scene, so this takes effect at the next one.
#[allow(dead_code, reason = "the snapshot tool has no test switch")]
pub fn set_focus_zoom_disabled(disabled: bool) {
    FOCUS_ZOOM_DISABLED.store(disabled, std::sync::atomic::Ordering::SeqCst);
}

fn apply_derived(app: &App, d: &Derived) {
    // The focus zoom follows the tier, so it is pushed with it: everything
    // that reserves room for the growth, and the larger art prepared for a
    // grown tile, reads this one value.
    app.global::<Motion>()
        .set_focus_zoom(rules::focus_zoom_percent(
            d.tier,
            FOCUS_ZOOM_DISABLED.load(std::sync::atomic::Ordering::SeqCst),
        ));
    let s = app.global::<SizingGlobal>();
    s.set_tier_240(d.tier == rules::Tier::T240);
    s.set_handheld(d.handheld_profile);
    s.set_radius_md(px(d.radius_md));
    s.set_radius_sm(px(d.radius_sm));
    s.set_font_hero(px(d.font_hero));
    s.set_font_title(px(d.font_title));
    s.set_font_section(px(d.font_section));
    s.set_font_body(px(d.font_body));
    s.set_font_caption(px(d.font_caption));
    s.set_font_small(px(d.font_small));
    s.set_card_border_width(px(d.card_border_width));
    s.set_focus_border_width(px(d.focus_border_width));
    s.set_focus_ring_width(px(d.focus_ring_width));
    s.set_press_edge_height(px(d.press_edge_height));
    s.set_header_row_height(px(d.header_row_height));
    s.set_header_stack_gap(px(d.header_stack_gap));
    s.set_header_top_margin(px(d.header_top_margin));
    s.set_header_side_margin(px(d.header_side_margin));
    s.set_header_height(px(d.header_height));
    s.set_header_bottom(px(d.header_bottom));
    s.set_help_bar_height(px(d.help_bar_height));
    s.set_help_bar_clearance(px(d.help_bar_clearance));
    s.set_visible_covers(d.visible_covers);
    s.set_hub_grid_columns(d.hub_grid_columns);
    s.set_hub_grid_rows(d.hub_grid_rows);
    s.set_hub_active_label_height(px(d.hub_active_label_height));
    s.set_hub_grid_top_margin(px(d.hub_grid_top_margin));
    s.set_hub_grid_height_budget(px(d.hub_grid_height_budget));
    s.set_hub_grid_side_inset(px(d.hub_grid_side_inset));
    s.set_hub_grid_column_gap(px(d.hub_grid_column_gap));
    s.set_hub_grid_top_inset(px(d.hub_grid_top_inset));
    s.set_hub_grid_bottom_inset(px(d.hub_grid_bottom_inset));
    s.set_hub_grid_row_gap(px(d.hub_grid_row_gap));
    s.set_hub_tile_width(px(d.hub_tile_width));
    s.set_hub_tile_height(px(d.hub_tile_height));
}

#[allow(
    clippy::too_many_lines,
    reason = "one setter per browse layout profile key keeps the inventory reviewable"
)]
fn push_profile(app: &App, p: &Profile, grid: &Profile, list: &Profile, cue: &Profile) {
    let l = app.global::<Layout>();
    if let Body::Grid { grid, footer } = &cue.body {
        l.set_grid_page_chevron_size(px(grid.page_chevron_size));
        l.set_page_cue_in_footer(footer.page_cue_in_footer);
    }
    l.set_title_in_header(p.header.title_in_header);
    l.set_hud_bottom_aligned(p.header.hud_bottom_aligned);
    l.set_status_pill_pinned_top(p.header.status_pill_pinned_top);
    l.set_top_strip_visible(p.status.top_strip_visible);
    l.set_strip_height(px(p.status.strip_height));
    l.set_slot_margin(px(p.status.slot_margin));
    l.set_top_margin(px(p.status.top_margin));
    l.set_card_radius(px(p.surface.card_radius));
    l.set_row_radius(px(p.surface.row_radius));
    l.set_bottom_unsafe_height(px(p.bottom_unsafe_height()));
    if let Body::Grid { grid, footer } = &grid.body {
        l.set_grid_left_inset(px(grid.left_inset));
        l.set_grid_right_inset(px(grid.right_inset));
        l.set_grid_column_gap(px(grid.column_gap));
        l.set_grid_top_inset(px(grid.top_inset));
        l.set_grid_bottom_inset(px(grid.bottom_inset));
        l.set_grid_row_gap(px(grid.row_gap));
        l.set_active_label_height(px(footer.active_label_height));
        l.set_active_label_bottom_margin(px(footer.active_label_bottom_margin));
        l.set_bottom_status_left_margin(px(footer.bottom_status_left_margin));
        l.set_bottom_status_right_margin(px(footer.bottom_status_right_margin));
        l.set_grid_bottom_margin(px(footer.grid_bottom_margin));
    }
    if let Body::List { list, detail, .. } = &list.body {
        l.set_list_vertical(list.content_axis == layouts::Axis::Vertical);
        l.set_list_share(list.list_share);
        l.set_detail_share(list.detail_share);
        l.set_divider_width(px(list.divider_width));
        l.set_divider_margin(px(list.divider_margin));
        l.set_card_side_margin(px(list.card_side_margin));
        l.set_card_top_margin(px(list.card_top_margin));
        l.set_card_bottom_margin(px(list.card_bottom_margin));
        l.set_card_padding_left(px(list.card_padding_left));
        l.set_card_padding_right(px(list.card_padding_right));
        l.set_card_padding_top(px(list.card_padding_top));
        l.set_card_padding_bottom(px(list.card_padding_bottom));
        l.set_row_height(px(list.row_height));
        l.set_row_spacing(px(list.row_spacing));
        l.set_center_slot(list.center_slot);
        l.set_row_text_left_padding(px(list.row_text_left_padding));
        l.set_row_text_right_padding(px(list.row_text_right_padding));
        l.set_favorite_right_padding(px(list.favorite_right_padding));
        l.set_overlay_bottom_margin(px(list.overlay_bottom_margin));
        l.set_detail_vertical(detail.content_axis == layouts::Axis::Vertical);
        l.set_section_gap(px(detail.section_gap));
        l.set_image_share(detail.image_share);
        l.set_metadata_share(detail.metadata_share);
        l.set_image_height_ratio_with_title(detail.image_height_ratio_with_title);
        l.set_image_reserved_width(px(detail.image_reserved_width));
        l.set_image_reserved_height(px(detail.image_reserved_height));
        l.set_image_bottom_margin(px(detail.image_bottom_margin));
        l.set_pane_padding_left(px(detail.pane_padding_left));
        l.set_pane_padding_right(px(detail.pane_padding_right));
        l.set_pane_padding_top(px(detail.pane_padding_top));
        l.set_pane_padding_bottom(px(detail.pane_padding_bottom));
        l.set_image_padding_left(px(detail.image_padding_left));
        l.set_image_padding_right(px(detail.image_padding_right));
        l.set_image_padding_top(px(detail.image_padding_top));
        l.set_image_padding_bottom(px(detail.image_padding_bottom));
        l.set_metadata_padding_left(px(detail.metadata_padding_left));
        l.set_metadata_padding_right(px(detail.metadata_padding_right));
        l.set_metadata_padding_top(px(detail.metadata_padding_top));
        l.set_metadata_padding_bottom(px(detail.metadata_padding_bottom));
        l.set_metadata_top_margin(px(detail.metadata_top_margin));
        l.set_metadata_left_margin(px(detail.metadata_left_margin));
        l.set_metadata_right_margin(px(detail.metadata_right_margin));
        l.set_metadata_height_adjustment(px(detail.metadata_height_adjustment));
        l.set_metadata_bottom_aligned(detail.metadata_bottom_aligned);
        l.set_metadata_label_max_width(px(detail.metadata_label_max_width.unwrap_or(0)));
        l.set_title_bottom_margin(px(detail.title_bottom_margin));
        l.set_tag_row_height(px(detail.tag_row_height));
        l.set_tag_row_spacing(px(detail.tag_row_spacing));
    }
}

/// Games page shape for the scene; the whole scene is the viewport.
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

/// Published browse viewport in the scene's own pixels, before any
/// rotation the presenter renders it with. One-to-one cached scenes move
/// the same full-width band as the live UI, leaving header, counter, active
/// label and help bar stationary.
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
    grid_top: u32,
    grid_height: u32,
) -> Option<BrowseGridTransitionGeometry> {
    if screen_width == 0 || grid_height == 0 || grid_top.checked_add(grid_height)? > screen_height {
        return None;
    }
    // Slint clips the whole screen-width band, not the centered tile block.
    // Its page period is exactly grid-height, without an extra row gap.
    Some(BrowseGridTransitionGeometry {
        x: 0,
        y: grid_top,
        width: screen_width,
        height: grid_height,
        gap: 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_720_browse_transition_matches_slint_grid_viewport() {
        assert_eq!(
            mister_browse_grid_transition_geometry(1280, 720, 126, 486),
            Some(BrowseGridTransitionGeometry {
                x: 0,
                y: 126,
                width: 1280,
                height: 486,
                gap: 0,
            })
        );
    }

    #[test]
    fn browse_transition_rejects_impossible_geometry() {
        assert_eq!(mister_browse_grid_transition_geometry(64, 32, 30, 5), None);
    }

    #[test]
    fn cached_540p_browse_bounds_include_the_left_edge() {
        assert_eq!(
            mister_browse_grid_transition_geometry(960, 540, 110, 350),
            Some(BrowseGridTransitionGeometry {
                x: 0,
                y: 110,
                width: 960,
                height: 350,
                gap: 0,
            })
        );
        assert_eq!(
            mister_browse_grid_transition_geometry(960, 540, u32::MAX, 350),
            None
        );
    }
}
