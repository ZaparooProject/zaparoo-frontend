// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

//! State projection from the input-owning HDMI component into the
//! view-only CRT component used by `MiSTer` dual-head mode.

use crate::{
    App, Buttons, GameInfoView, GamesView, GridCell, HubView, Motion, Overlays, SettingsView,
    Shell, Status, SystemsView,
};
use slint::{ComponentHandle as _, Model as _, ModelRc, VecModel};

macro_rules! set_if_changed {
    ($target:expr, $get:ident => $set:ident, $value:expr) => {{
        let value = $value;
        if $target.$get() != value {
            $target.$set(value);
        }
    }};
}

macro_rules! copy_properties {
    ($source:expr, $target:expr; $($get:ident => $set:ident),+ $(,)?) => {
        $(
            set_if_changed!($target, $get => $set, $source.$get());
        )+
    };
}

trait MirrorRow: Clone + 'static {
    fn same_content(&self, other: &Self) -> bool;
}

fn same_image(left: &slint::Image, right: &slint::Image) -> bool {
    left == right || (left.size().width == 0 && right.size().width == 0)
}

impl MirrorRow for GridCell {
    fn same_content(&self, other: &Self) -> bool {
        crate::view_model::same_cell(self, other)
    }
}

/// Project only the source page that fits the CRT grid. Returning
/// `None` preserves the existing `ModelRc`, which avoids invalidating
/// the CRT scene on every 16 ms mirror tick while source rows are idle.
fn project_page<T: MirrorRow>(
    model: &ModelRc<T>,
    current: &ModelRc<T>,
    selected: i32,
    capacity: i32,
) -> (Option<ModelRc<T>>, i32, usize) {
    let capacity = capacity.max(1) as usize;
    let selected = (selected.max(0) as usize).min(model.row_count().saturating_sub(1));
    let start = selected / capacity * capacity;
    let end = (start + capacity).min(model.row_count());
    let unchanged = current.row_count() == end - start
        && (start..end).all(|row| {
            model
                .row_data(row)
                .zip(current.row_data(row - start))
                .is_some_and(|(source, target)| source.same_content(&target))
        });
    let projected = if unchanged {
        None
    } else {
        let rows = (start..end)
            .filter_map(|row| model.row_data(row))
            .collect::<Vec<_>>();
        Some(ModelRc::new(VecModel::from(rows)))
    };
    (
        projected,
        i32::try_from(selected - start).unwrap_or(0),
        model.row_count().div_ceil(capacity),
    )
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum TransitionPhase {
    #[default]
    Idle,
    Active,
    Rearm,
}

#[derive(Default)]
struct SyncState {
    route: TransitionPhase,
    systems: TransitionPhase,
    games: TransitionPhase,
    /// Screen the CRT head last resolved its layout profile for.
    layout_screen: slint::SharedString,
}

/// Copy router-owned state while leaving profile-specific geometry
/// (`Sizing`, grid shapes, and visible-row count) under CRT ownership.
#[allow(
    clippy::too_many_lines,
    reason = "explicit property inventory makes missing mirror state reviewable"
)]
#[allow(
    clippy::float_cmp,
    reason = "the float compares are change detection on a mirrored property, not numeric equality"
)]
fn sync_with_state(primary: &App, crt: &App, state: &mut SyncState) {
    let source = primary.global::<Shell>();
    let target = crt.global::<Shell>();
    let route_in_progress = source.get_route_transitioning();
    let route_motion =
        source.get_route_cached_transition() || source.get_route_page_slide().abs() > f32::EPSILON;
    if route_in_progress {
        set_if_changed!(target, get_route_transitioning => set_route_transitioning, true);
        set_if_changed!(
            target,
            get_route_from_screen => set_route_from_screen,
            source.get_route_from_screen()
        );
        set_if_changed!(
            target,
            get_route_to_screen => set_route_to_screen,
            source.get_route_to_screen()
        );
        set_if_changed!(
            target,
            get_route_slide_dir => set_route_slide_dir,
            source.get_route_slide_dir()
        );
        set_if_changed!(
            target,
            get_route_cached_transition => set_route_cached_transition,
            false
        );
        set_if_changed!(
            target,
            get_route_from_gated => set_route_from_gated,
            source.get_route_from_gated()
        );
        if route_motion && state.route != TransitionPhase::Active {
            target.set_active_screen(source.get_route_from_screen());
            target.set_route_slide_anim(true);
            target.set_route_page_slide(source.get_route_slide_dir() as f32);
            state.route = TransitionPhase::Active;
        }
    } else if state.route == TransitionPhase::Active || target.get_route_transitioning() {
        target.set_route_slide_anim(false);
        target.set_active_screen(source.get_active_screen());
        target.set_route_from_gated(false);
        target.set_route_page_slide(0.0);
        target.set_route_from_screen(slint::SharedString::default());
        target.set_route_to_screen(slint::SharedString::default());
        target.set_route_transitioning(false);
        state.route = TransitionPhase::Rearm;
    } else if state.route == TransitionPhase::Rearm {
        target.set_route_slide_anim(true);
        state.route = TransitionPhase::Idle;
    } else {
        set_if_changed!(
            target,
            get_active_screen => set_active_screen,
            source.get_active_screen()
        );
    }
    copy_properties!(source, target;
        get_transitioning => set_transitioning,
        get_transition_cue => set_transition_cue,
        get_transition_target => set_transition_target,
        get_status_text => set_status_text,
        get_clock_text => set_clock_text,
        get_clock_sample => set_clock_sample,
        get_status_keys => set_status_keys,
        get_status_icons_enabled => set_status_icons_enabled,
        get_has_battery => set_has_battery,
        get_battery_percent => set_battery_percent,
        get_about_version_line => set_about_version_line,
        get_boot_curtain => set_boot_curtain,
        get_boot_complete => set_boot_complete,
        get_boot_text => set_boot_text,
        get_reduce_motion => set_reduce_motion,
        get_mouse_enabled => set_mouse_enabled,
        get_saver_armed => set_saver_armed,
        get_orientation => set_orientation,
        get_browse_list_layout => set_browse_list_layout,
        get_systems_list_layout => set_systems_list_layout,
        get_is_mister => set_is_mister,
        get_crt_enabled => set_crt_enabled,
        get_crt_standard => set_crt_standard,
    );
    // The CRT head resolves its own layout profile (its scene is the
    // 240p tier) whenever the mirrored screen changes.
    let screen = target.get_active_screen();
    if screen != state.layout_screen {
        state.layout_screen = screen;
        let sizing = crt.global::<crate::Sizing>();
        let scene = crate::sizing::Scene::of(
            crt,
            f64::from(sizing.get_screen_width()),
            f64::from(sizing.get_screen_height()),
            true,
        );
        crate::sizing::refresh_layout(crt, scene);
    }

    let source = primary.global::<Status>();
    let target = crt.global::<Status>();
    copy_properties!(source, target;
        get_kind => set_kind,
        get_arg => set_arg,
        get_arg2 => set_arg2,
        get_is_error => set_is_error,
        get_show_track => set_show_track,
        get_paused => set_paused,
        get_total_known => set_total_known,
        get_current_step => set_current_step,
        get_total_steps => set_total_steps,
        get_percent => set_percent,
    );

    let source = primary.global::<Buttons>();
    let target = crt.global::<Buttons>();
    copy_properties!(source, target;
        get_style => set_style,
        get_confirm => set_confirm,
        get_cancel => set_cancel,
        get_options => set_options,
        get_view => set_view,
    );

    let source = primary.global::<Motion>();
    let target = crt.global::<Motion>();
    copy_properties!(source, target;
        get_enabled => set_enabled,
    );

    // The CRT head shows the HDMI head's page at its own geometry: the
    // page shape follows the primary until the CRT/DDR takeback re-slices
    // it per head (docs/plans/slint-migration.md).
    let source = primary.global::<HubView>();
    let target = crt.global::<HubView>();
    copy_properties!(source, target;
        get_hub_error => set_hub_error,
        get_loaded => set_loaded,
        get_catalog_empty => set_catalog_empty,
        get_indexing => set_indexing,
        get_cells => set_cells,
        get_selected_local => set_selected_local,
        get_columns => set_columns,
        get_rows => set_rows,
        get_page => set_page,
        get_total_pages => set_total_pages,
        get_has_pages_above => set_has_pages_above,
        get_has_pages_below => set_has_pages_below,
        get_label_key => set_label_key,
        get_label_name => set_label_name,
        get_label_reason => set_label_reason,
        get_label_visible => set_label_visible,
        get_focused_is_category => set_focused_is_category,
        get_focus_ready => set_focus_ready,
        get_move_armed => set_move_armed,
        get_held_local => set_held_local,
        get_options_available => set_options_available,
        get_activate_pulse => set_activate_pulse,
        get_release_pulse => set_release_pulse,
    );
    {
        let sizing = crt.global::<crate::Sizing>();
        let scene = crate::sizing::Scene::of(
            crt,
            f64::from(sizing.get_screen_width()),
            f64::from(sizing.get_screen_height()),
            true,
        );
        let inputs = scene.inputs();
        let derived = zaparoo_app::sizing::derive(&inputs);
        let mut g = zaparoo_app::hub::geometry(&inputs, &derived);
        let insets = g.insets;
        g.fit = zaparoo_app::paged_grid::fit(
            source.get_columns(),
            source.get_rows(),
            inputs.screen_width as i32,
            inputs.screen_height as i32,
            Some(derived.hub_grid_height_budget),
            !g.compact_footer,
            &insets,
        );
        target.set_cell_width(g.fit.cell_width as f32);
        target.set_cell_height(g.fit.cell_height as f32);
        target.set_block_offset_x(g.fit.block_offset_x as f32);
        target.set_block_offset_y(g.fit.block_offset_y as f32);
        target.set_grid_y(g.grid_y as f32);
        target.set_grid_height(g.grid_height as f32);
        target.set_label_y(g.label_y as f32);
        target.set_label_height(g.label_height as f32);
    }

    let source = primary.global::<SystemsView>();
    let target = crt.global::<SystemsView>();
    copy_properties!(source, target;
        get_mode => set_mode,
        get_category => set_category,
        get_count => set_count,
        get_favorites_total => set_favorites_total,
        get_loading => set_loading,
        get_error => set_error,
        get_focus_ready => set_focus_ready,
        get_has_pages_above => set_has_pages_above,
        get_has_pages_below => set_has_pages_below,
        get_label_name => set_label_name,
        get_label_hidden => set_label_hidden,
        get_label_count => set_label_count,
        get_activate_pulse => set_activate_pulse,
        get_release_pulse => set_release_pulse,
        get_current_index => set_current_index,
        get_list_rows => set_list_rows,
        get_list_sel => set_list_sel,
        get_list_view_top => set_list_view_top,
        get_list_visible => set_list_visible,
        get_list_page => set_list_page,
        get_list_total_pages => set_list_total_pages,
        get_has_items_above => set_has_items_above,
        get_has_items_below => set_has_items_below,
        get_detail_title => set_detail_title,
        get_detail_has_cover => set_detail_has_cover,
        get_detail_wordmark => set_detail_wordmark,
        get_detail_rows => set_detail_rows,
    );
    let detail_cover = source.get_detail_cover();
    if !same_image(&target.get_detail_cover(), &detail_cover) {
        target.set_detail_cover(detail_cover);
    }
    {
        // The CRT head fits its own grid shape (already pushed by its
        // scene) inside its own layout band.
        let sizing = crt.global::<crate::Sizing>();
        let scene = crate::sizing::Scene::of(
            crt,
            f64::from(sizing.get_screen_width()),
            f64::from(sizing.get_screen_height()),
            true,
        );
        let inputs = scene.inputs();
        let derived = zaparoo_app::sizing::derive(&inputs);
        let profile = zaparoo_app::layouts::profile(
            zaparoo_app::layouts::ThemeId::current(&inputs),
            zaparoo_app::layouts::View::SystemsGrid,
            &inputs,
        );
        if let zaparoo_app::layouts::Body::Grid { grid, footer } = profile.body {
            let grid_y =
                derived.header_bottom + profile.status.top_margin + profile.status.strip_height;
            let bottom = if derived.tier == zaparoo_app::sizing::Tier::T240 {
                derived.help_bar_height + footer.active_label_height
            } else {
                footer.grid_bottom_margin
            };
            let grid_height = (inputs.screen_height as i32 - grid_y - bottom).max(0);
            let insets = zaparoo_app::paged_grid::Insets {
                left: grid.left_inset,
                right: grid.right_inset,
                top: grid.top_inset,
                bottom: grid.bottom_inset,
                column_gap: grid.column_gap,
                row_gap: grid.row_gap,
            };
            let fit = zaparoo_app::paged_grid::fit(
                target.get_columns(),
                target.get_rows(),
                inputs.screen_width as i32,
                grid_height,
                None,
                false,
                &insets,
            );
            target.set_cell_width(fit.cell_width as f32);
            target.set_cell_height(fit.cell_height as f32);
            target.set_block_offset_x(fit.block_offset_x as f32);
            target.set_block_offset_y(fit.block_offset_y as f32);
            target.set_grid_y(grid_y as f32);
            target.set_grid_height(grid_height as f32);
        }
    }
    let capacity = target.get_columns() * target.get_rows();
    let cached_transition = source.get_cached_transition();
    let strip_transition = source.get_page_slide().abs() > f32::EPSILON;
    let transition_requested = cached_transition || strip_transition;
    if transition_requested {
        if state.systems != TransitionPhase::Active {
            let incoming = if cached_transition {
                source.get_cells()
            } else {
                source.get_next_cells()
            };
            let (next_page, _, _) = project_page(
                &incoming,
                &ModelRc::default(),
                source.get_transition_target_index(),
                capacity,
            );
            target.set_next_cells(next_page.unwrap_or_default());
            set_if_changed!(
                target,
                get_slide_dir => set_slide_dir,
                source.get_slide_dir()
            );
            set_if_changed!(target, get_slide_anim => set_slide_anim, true);
            target.set_page_slide(source.get_slide_dir() as f32);
            state.systems = TransitionPhase::Active;
        }
    } else {
        if state.systems == TransitionPhase::Active {
            target.set_slide_anim(false);
        }
        let (systems, index, chunks) = project_page(
            &source.get_cells(),
            &target.get_cells(),
            source.get_selected_local(),
            capacity,
        );
        if let Some(systems) = systems {
            target.set_cells(systems);
        }
        set_if_changed!(target, get_selected_local => set_selected_local, index);
        set_if_changed!(
            target,
            get_next_cells => set_next_cells,
            ModelRc::default()
        );
        if target.get_page_slide().abs() > f32::EPSILON {
            target.set_page_slide(0.0);
        }
        set_if_changed!(
            target,
            get_page => set_page,
            source.get_page() * i32::try_from(chunks.max(1)).unwrap_or(1)
                + source.get_selected_local().max(0) / capacity.max(1)
        );
        set_if_changed!(
            target,
            get_total_pages => set_total_pages,
            source.get_total_pages() * i32::try_from(chunks.max(1)).unwrap_or(1)
        );
        if state.systems == TransitionPhase::Active {
            state.systems = TransitionPhase::Rearm;
        } else if state.systems == TransitionPhase::Rearm {
            target.set_slide_anim(true);
            state.systems = TransitionPhase::Idle;
        } else {
            set_if_changed!(target, get_slide_anim => set_slide_anim, true);
        }
    }

    let source = primary.global::<crate::SetupModalView>();
    let target = crt.global::<crate::SetupModalView>();
    copy_properties!(source, target;
        get_open => set_open,
        get_kind => set_kind,
        get_picker_page => set_picker_page,
        get_picker_title => set_picker_title,
        get_rows => set_rows,
        get_index => set_index,
        get_picker_rows => set_picker_rows,
        get_picker_sel => set_picker_sel,
        get_has_above => set_has_above,
        get_has_below => set_has_below,
        get_activate_pulse => set_activate_pulse,
    );

    let source = primary.global::<SettingsView>();
    let target = crt.global::<SettingsView>();
    copy_properties!(source, target;
        get_page => set_page,
        get_rows => set_rows,
        get_index => set_index,
        get_cells => set_cells,
        get_activate_pulse => set_activate_pulse,
        get_release_pulse => set_release_pulse,
    );

    let source = primary.global::<GamesView>();
    let target = crt.global::<GamesView>();
    copy_properties!(source, target;
        get_mode => set_mode,
        get_title => set_title,
        get_loading => set_loading,
        get_loading_more => set_loading_more,
        get_error => set_error,
        get_count => set_count,
        get_total_items => set_total_items,
        get_total_known => set_total_known,
        get_total_files => set_total_files,
        get_has_more => set_has_more,
        get_page_loading => set_page_loading,
        get_focus_ready => set_focus_ready,
        get_current_index => set_current_index,
        get_has_pages_above => set_has_pages_above,
        get_has_pages_below => set_has_pages_below,
        get_label_name => set_label_name,
        get_label_tags => set_label_tags,
        get_activate_pulse => set_activate_pulse,
        get_release_pulse => set_release_pulse,
        get_rapid_letter => set_rapid_letter,
        get_list_rows => set_list_rows,
        get_list_sel => set_list_sel,
        get_list_view_top => set_list_view_top,
        get_list_visible => set_list_visible,
        get_list_page => set_list_page,
        get_list_total_pages => set_list_total_pages,
        get_has_items_above => set_has_items_above,
        get_has_items_below => set_has_items_below,
        get_detail_title => set_detail_title,
        get_detail_path => set_detail_path,
        get_detail_has_cover => set_detail_has_cover,
        get_detail_cover_absent => set_detail_cover_absent,
        get_detail_rows => set_detail_rows,
        get_detail_loading => set_detail_loading,
        get_rapid_active => set_rapid_active,
    );
    let detail_cover = source.get_detail_cover();
    if !same_image(&target.get_detail_cover(), &detail_cover) {
        target.set_detail_cover(detail_cover);
    }
    {
        // The CRT head fits its own grid shape (already pushed by its
        // scene) inside its own layout band and footer slot.
        let sizing = crt.global::<crate::Sizing>();
        let scene = crate::sizing::Scene::of(
            crt,
            f64::from(sizing.get_screen_width()),
            f64::from(sizing.get_screen_height()),
            true,
        );
        let mode = match source.get_mode().as_str() {
            "favorites" => crate::games::GamesMode::Favorites,
            "recents" => crate::games::GamesMode::Recents,
            _ => crate::games::GamesMode::Browse,
        };
        let g = crate::games::geometry_for(&scene.inputs(), mode);
        let fit = zaparoo_app::paged_grid::fit(
            target.get_columns(),
            target.get_rows(),
            scene.width as i32,
            g.grid_height,
            None,
            false,
            &g.insets,
        );
        target.set_cell_width(fit.cell_width as f32);
        target.set_cell_height(fit.cell_height as f32);
        target.set_block_offset_x(fit.block_offset_x as f32);
        target.set_block_offset_y(fit.block_offset_y as f32);
        target.set_grid_y(g.grid_y as f32);
        target.set_grid_height(g.grid_height as f32);
        target.set_label_y(g.label_y as f32);
        target.set_label_height(g.label_height as f32);
    }
    let capacity = target.get_columns() * target.get_rows();
    let cached_transition = source.get_cached_transition();
    let strip_transition = source.get_page_slide().abs() > f32::EPSILON;
    let transition_requested = cached_transition || strip_transition;
    if transition_requested {
        if state.games != TransitionPhase::Active {
            let incoming = if cached_transition {
                source.get_cells()
            } else {
                source.get_next_cells()
            };
            let (next_page, _, _) = project_page(
                &incoming,
                &ModelRc::default(),
                source.get_transition_target_index(),
                capacity,
            );
            target.set_next_cells(next_page.unwrap_or_default());
            set_if_changed!(
                target,
                get_slide_dir => set_slide_dir,
                source.get_slide_dir()
            );
            set_if_changed!(target, get_slide_anim => set_slide_anim, true);
            target.set_page_slide(source.get_slide_dir() as f32);
            state.games = TransitionPhase::Active;
        }
    } else {
        if state.games == TransitionPhase::Active {
            target.set_slide_anim(false);
        }
        let (games, index, chunks) = project_page(
            &source.get_cells(),
            &target.get_cells(),
            source.get_selected_local(),
            capacity,
        );
        if let Some(games) = games {
            target.set_cells(games);
        }
        set_if_changed!(target, get_selected_local => set_selected_local, index);
        set_if_changed!(
            target,
            get_next_cells => set_next_cells,
            ModelRc::default()
        );
        if target.get_page_slide().abs() > f32::EPSILON {
            target.set_page_slide(0.0);
        }
        set_if_changed!(
            target,
            get_page => set_page,
            source.get_page() * i32::try_from(chunks.max(1)).unwrap_or(1)
                + source.get_selected_local().max(0) / capacity.max(1)
        );
        set_if_changed!(
            target,
            get_total_pages => set_total_pages,
            source.get_total_pages() * i32::try_from(chunks.max(1)).unwrap_or(1)
        );
        if state.games == TransitionPhase::Active {
            state.games = TransitionPhase::Rearm;
        } else if state.games == TransitionPhase::Rearm {
            target.set_slide_anim(true);
            state.games = TransitionPhase::Idle;
        } else {
            set_if_changed!(target, get_slide_anim => set_slide_anim, true);
        }
    }

    let source = primary.global::<crate::PressFeedback>();
    let target = crt.global::<crate::PressFeedback>();
    copy_properties!(source, target;
        get_index => set_index,
        get_owner => set_owner,
    );

    let source = primary.global::<GameInfoView>();
    let target = crt.global::<GameInfoView>();
    copy_properties!(source, target;
        get_modal_open => set_modal_open,
        get_modal_name => set_modal_name,
        get_modal_system => set_modal_system,
        get_modal_path => set_modal_path,
        get_modal_tags => set_modal_tags,
        get_modal_description => set_modal_description,
        get_modal_has_cover => set_modal_has_cover,
    );
    let modal_cover = source.get_modal_cover();
    if !same_image(&target.get_modal_cover(), &modal_cover) {
        target.set_modal_cover(modal_cover);
    }

    let source = primary.global::<Overlays>();
    let target = crt.global::<Overlays>();
    copy_properties!(source, target;
        get_context_open => set_context_open,
        get_context_entries => set_context_entries,
        get_context_index => set_context_index,
        get_context_anchor_x => set_context_anchor_x,
        get_context_anchor_y => set_context_anchor_y,
        get_context_anchor_w => set_context_anchor_w,
        get_context_anchor_h => set_context_anchor_h,
        get_list_open => set_list_open,
        get_list_title => set_list_title,
        get_list_entries => set_list_entries,
        get_list_index => set_list_index,
        get_letter_open => set_letter_open,
        get_letter_buckets => set_letter_buckets,
        get_letter_index => set_letter_index,
        get_letter_loading => set_letter_loading,
        get_card_write_open => set_card_write_open,
        get_card_write_failed => set_card_write_failed,
        get_qr_open => set_qr_open,
        get_qr_modules => set_qr_modules,
        get_dialog_open => set_dialog_open,
        get_dialog_kind => set_dialog_kind,
        get_dialog_detail => set_dialog_detail,
        get_dialog_arg => set_dialog_arg,
        get_dialog_status => set_dialog_status,
        get_dialog_status_step => set_dialog_status_step,
        get_dialog_status_total => set_dialog_status_total,
        get_dialog_status_name => set_dialog_status_name,
        get_dialog_buttons => set_dialog_buttons,
        get_dialog_focus => set_dialog_focus,
        get_crt_calibration_open => set_crt_calibration_open,
        get_crt_h_offset => set_crt_h_offset,
        get_crt_v_offset => set_crt_v_offset,
    );
    let qr_image = source.get_qr_image();
    if !same_image(&target.get_qr_image(), &qr_image) {
        target.set_qr_image(qr_image);
    }
}

#[cfg(test)]
pub fn sync(primary: &App, crt: &App) {
    sync_with_state(primary, crt, &mut SyncState::default());
}

pub struct Runtime {
    _mirror: App,
    _timer: slint::Timer,
}

impl Runtime {
    pub fn start(primary: &App, mirror: App) -> Result<Self, slint::PlatformError> {
        let mut state = SyncState::default();
        sync_with_state(primary, &mirror, &mut state);
        mirror.show()?;

        let primary = primary.as_weak();
        let mirror_weak = mirror.as_weak();
        let timer = slint::Timer::default();
        timer.start(
            slint::TimerMode::Repeated,
            std::time::Duration::from_millis(16),
            move || {
                if let (Some(primary), Some(mirror)) = (primary.upgrade(), mirror_weak.upgrade()) {
                    sync_with_state(&primary, &mirror, &mut state);
                }
            },
        );

        Ok(Self {
            _mirror: mirror,
            _timer: timer,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Theme;
    use slint::platform::software_renderer::{MinimalSoftwareWindow, RepaintBufferType};
    use slint::platform::{Platform, WindowAdapter};
    use slint::{Model, ModelRc, SharedString, VecModel};
    use std::rc::Rc;

    struct TestPlatform;

    impl Platform for TestPlatform {
        fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, slint::PlatformError> {
            Ok(MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer))
        }

        fn duration_since_start(&self) -> std::time::Duration {
            std::time::Duration::ZERO
        }
    }

    #[test]
    #[allow(
        clippy::too_many_lines,
        reason = "one platform-owning integration test covers idle identity plus Systems/Games transition lifecycles"
    )]
    fn copies_view_state_but_preserves_crt_geometry() -> Result<(), slint::PlatformError> {
        assert!(slint::platform::set_platform(Box::new(TestPlatform)).is_ok());
        let primary = App::new()?;
        let crt = App::new()?;

        primary
            .global::<Shell>()
            .set_active_screen(SharedString::from("games"));
        primary.global::<GamesView>().set_selected_local(4);
        primary.global::<GamesView>().set_columns(7);
        primary
            .global::<GamesView>()
            .set_cells(ModelRc::new(VecModel::from(
                (0..5)
                    .map(|i| GridCell {
                        name: SharedString::from(format!("Game {i}")),
                        ..GridCell::default()
                    })
                    .collect::<Vec<_>>(),
            )));
        crt.global::<GamesView>().set_columns(4);
        crt.global::<GamesView>().set_rows(1);
        crt.global::<Theme>().set_crt(true);

        sync(&primary, &crt);

        assert_eq!(crt.global::<Shell>().get_active_screen().as_str(), "games");
        assert_eq!(crt.global::<GamesView>().get_selected_local(), 0);
        assert_eq!(crt.global::<GamesView>().get_columns(), 4);
        assert!(crt.global::<Theme>().get_crt());
        assert_eq!(
            crt.global::<GamesView>()
                .get_cells()
                .row_data(0)
                .map(|game| game.name.to_string()),
            Some("Game 4".to_string())
        );

        let projected = crt.global::<GamesView>().get_cells();
        sync(&primary, &crt);
        assert_eq!(
            crt.global::<GamesView>().get_cells(),
            projected,
            "unchanged synchronization must preserve projected model identity"
        );

        let mut state = SyncState::default();

        // Cached HDMI route motion becomes native Slint motion on CRT.
        // Source screen stays mounted until primary transition completion.
        let shell = primary.global::<Shell>();
        shell.set_route_from_screen(SharedString::from("games"));
        shell.set_route_to_screen(SharedString::from("systems"));
        shell.set_route_slide_dir(1);
        shell.set_route_cached_transition(true);
        shell.set_route_transitioning(true);
        shell.set_active_screen(SharedString::from("systems"));
        sync_with_state(&primary, &crt, &mut state);
        assert_eq!(crt.global::<Shell>().get_active_screen().as_str(), "games");
        assert!(crt.global::<Shell>().get_route_transitioning());
        assert_eq!(
            crt.global::<Shell>().get_route_from_screen().as_str(),
            "games"
        );
        assert_eq!(
            crt.global::<Shell>().get_route_to_screen().as_str(),
            "systems"
        );
        assert!((crt.global::<Shell>().get_route_page_slide() - 1.0).abs() < f32::EPSILON);

        shell.set_route_cached_transition(false);
        shell.set_route_transitioning(false);
        sync_with_state(&primary, &crt, &mut state);
        assert_eq!(
            crt.global::<Shell>().get_active_screen().as_str(),
            "systems"
        );
        assert!(!crt.global::<Shell>().get_route_transitioning());
        assert!(!crt.global::<Shell>().get_route_slide_anim());
        sync_with_state(&primary, &crt, &mut state);
        assert!(crt.global::<Shell>().get_route_slide_anim());

        // Cached HDMI grid motion must not force a CRT cut. The CRT keeps
        // its current page, stages the destination at its own four-item
        // capacity, animates, then commits the exact selected system.
        crt.global::<SystemsView>().set_columns(2);
        crt.global::<SystemsView>().set_rows(2);
        crt.global::<SystemsView>()
            .set_cells(ModelRc::new(VecModel::from(
                (0..4)
                    .map(|i| GridCell {
                        name: SharedString::from(format!("Old {i}")),
                        ..GridCell::default()
                    })
                    .collect::<Vec<_>>(),
            )));
        primary
            .global::<SystemsView>()
            .set_cells(ModelRc::new(VecModel::from(
                (0..8)
                    .map(|i| GridCell {
                        name: SharedString::from(format!("New {i}")),
                        ..GridCell::default()
                    })
                    .collect::<Vec<_>>(),
            )));
        primary.global::<SystemsView>().set_selected_local(6);
        primary
            .global::<SystemsView>()
            .set_transition_target_index(6);
        primary.global::<SystemsView>().set_page(1);
        primary.global::<SystemsView>().set_total_pages(2);
        primary.global::<SystemsView>().set_slide_dir(-1);
        primary.global::<SystemsView>().set_cached_transition(true);

        sync_with_state(&primary, &crt, &mut state);
        assert_eq!(
            crt.global::<SystemsView>()
                .get_cells()
                .row_data(0)
                .map(|system| system.name.to_string()),
            Some("Old 0".to_string())
        );
        assert_eq!(
            crt.global::<SystemsView>()
                .get_next_cells()
                .row_data(2)
                .map(|system| system.name.to_string()),
            Some("New 6".to_string())
        );
        assert!((crt.global::<SystemsView>().get_page_slide() + 1.0).abs() < f32::EPSILON);
        assert!(crt.global::<SystemsView>().get_slide_anim());

        primary.global::<SystemsView>().set_cached_transition(false);
        sync_with_state(&primary, &crt, &mut state);
        assert_eq!(
            crt.global::<SystemsView>()
                .get_cells()
                .row_data(2)
                .map(|system| system.name.to_string()),
            Some("New 6".to_string())
        );
        assert_eq!(crt.global::<SystemsView>().get_selected_local(), 2);
        assert!(crt.global::<SystemsView>().get_page_slide().abs() < f32::EPSILON);
        assert_eq!(crt.global::<SystemsView>().get_next_cells().row_count(), 0);
        assert!(!crt.global::<SystemsView>().get_slide_anim());
        sync_with_state(&primary, &crt, &mut state);
        assert!(crt.global::<SystemsView>().get_slide_anim());

        // Games use the same cached-HDMI/native-CRT split. CRT still
        // chooses the destination chunk and focus from the router's
        // explicit incoming index.
        crt.global::<GamesView>().set_columns(2);
        crt.global::<GamesView>().set_rows(2);
        crt.global::<GamesView>()
            .set_cells(ModelRc::new(VecModel::from(
                (0..4)
                    .map(|i| GridCell {
                        name: SharedString::from(format!("Old {i}")),
                        ..GridCell::default()
                    })
                    .collect::<Vec<_>>(),
            )));
        let incoming_games = ModelRc::new(VecModel::from(
            (0..8)
                .map(|i| GridCell {
                    name: SharedString::from(format!("New {i}")),
                    ..GridCell::default()
                })
                .collect::<Vec<_>>(),
        ));
        primary
            .global::<GamesView>()
            .set_cells(incoming_games.clone());
        primary.global::<GamesView>().set_selected_local(6);
        primary.global::<GamesView>().set_transition_target_index(6);
        primary.global::<GamesView>().set_slide_dir(1);
        primary.global::<GamesView>().set_cached_transition(true);

        sync_with_state(&primary, &crt, &mut state);
        assert_eq!(
            crt.global::<GamesView>()
                .get_cells()
                .row_data(0)
                .map(|game| game.name.to_string()),
            Some("Old 0".to_string())
        );
        assert_eq!(
            crt.global::<GamesView>()
                .get_next_cells()
                .row_data(2)
                .map(|game| game.name.to_string()),
            Some("New 6".to_string())
        );
        assert!((crt.global::<GamesView>().get_page_slide() - 1.0).abs() < f32::EPSILON);
        assert!(crt.global::<GamesView>().get_slide_anim());

        primary.global::<GamesView>().set_cells(incoming_games);
        primary.global::<GamesView>().set_page(1);
        primary.global::<GamesView>().set_total_pages(2);
        primary.global::<GamesView>().set_cached_transition(false);
        sync_with_state(&primary, &crt, &mut state);
        assert_eq!(
            crt.global::<GamesView>()
                .get_cells()
                .row_data(2)
                .map(|game| game.name.to_string()),
            Some("New 6".to_string())
        );
        assert_eq!(crt.global::<GamesView>().get_selected_local(), 2);
        assert!(!crt.global::<GamesView>().get_slide_anim());
        sync_with_state(&primary, &crt, &mut state);
        assert!(crt.global::<GamesView>().get_slide_anim());

        Ok(())
    }
}
