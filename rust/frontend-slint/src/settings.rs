// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// The Settings screen driver: builds each page from
// `zaparoo_app::settings`, pushes the rows and the root grid into
// `SettingsView`, and applies what the user changes (persisting to the
// state file and mirroring the durable half into `frontend.toml`).
// Mirrors SettingsScreen.qml plus `models/settings.rs`.

use std::sync::Arc;

use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use zaparoo_app::layouts::{self, ThemeId, View};
use zaparoo_app::paged_grid::{self, Insets};
use zaparoo_app::settings::{self as rules, Control, Row};
use zaparoo_core::input_actions::actions;
use zaparoo_core::persist;

use crate::router::{lock, Ctx, ListContext, PendingRestart};
use crate::{App, GridCell, SettingsInput, SettingsRow, SettingsView};

/// What the registry needs to know about this machine.
fn inputs(ctx: &Ctx) -> rules::Inputs {
    rules::Inputs {
        is_mister: ctx.is_mister,
        crt_enabled: ctx.crt_enabled,
        debug_build: cfg!(debug_assertions),
    }
}

/// The canonical value a picker row currently holds.
fn current_value(ctx: &Ctx, id: &str) -> String {
    let shared = lock(&ctx.shared);
    let s = &shared.persist.settings;
    match id {
        "orientation" => s.orientation.clone(),
        "resolution" => s.resolution.clone(),
        "interfaceProfile" => s.interface_profile.clone(),
        "crtVideoStandard" => s.crt_video_standard.clone(),
        "screensaverTimeout" => s.screensaver_timeout.clone(),
        "systemsLayout" => s.systems_browse_layout.clone(),
        "gamesLayout" => s.games_browse_layout.clone(),
        "systemLogoStyle" => s.system_logo_style.clone(),
        "colorScheme" => s.color_scheme.clone(),
        "colorIntensity" => s.color_intensity.clone(),
        "mediaImageType" => s.media_image_type.clone(),
        "language" => s.language.clone(),
        "region" => s.region.clone(),
        "clockFormat" => s.clock_format.clone(),
        "buttonLayout" => s.button_layout.clone(),
        _ => String::new(),
    }
}

/// Whether a toggle row is on.
fn checked(ctx: &Ctx, id: &str) -> bool {
    let shared = lock(&ctx.shared);
    let s = &shared.persist.settings;
    match id {
        "showHidden" => shared.show_hidden,
        "showOriginalFilenames" => s.show_original_filenames,
        "reduceMotion" => s.reduce_motion,
        "mouseEnabled" => s.mouse_enabled,
        "debugLogging" => s.debug_logging,
        "swapConfirmCancel" => s.swap_confirm_cancel,
        "swapOptionsView" => s.swap_options_view,
        "crtEnabled" => ctx.crt_enabled,
        _ => false,
    }
}

/// The option list a picker offers: the fixed ones come from the rules,
/// the output modes and the palette from the host.
fn options(ctx: &Ctx, id: &str) -> Vec<String> {
    if let Some(values) = rules::options(id, &inputs(ctx)) {
        return values.into_iter().map(str::to_string).collect();
    }
    match id {
        "resolution" => {
            #[cfg(feature = "mister")]
            let output = crate::mister::video_mode::output_size();
            #[cfg(not(feature = "mister"))]
            let output = None;
            crate::display::resolution_options(output)
        }
        "colorScheme" => zaparoo_app::palette::ids().map(str::to_string).collect(),
        _ => Vec::new(),
    }
}

// ---------- Rendering ----------

/// Row metrics (`SettingsField.qml`): a field is one line tall, a
/// running action adds its status band, a header is its own short row.
struct Metrics {
    row: i32,
    action_band: i32,
    header: i32,
}

fn metrics(app: &App) -> Metrics {
    let inputs = crate::router::output_scene(app).inputs();
    Metrics {
        row: inputs.pct_h(8.0),
        action_band: inputs.pct_h(3.2),
        header: inputs.pct_h(5.0),
    }
}

/// `SettingsScreen`'s live caption ladder, including idle totals.
fn action_status(ms: &zaparoo_core::store::MediaStatusState, id: &str) -> (&'static str, i32) {
    match id {
        "updateMediaDb" if ms.optimizing => ("optimizing", 0),
        "updateMediaDb" if ms.indexing => (if ms.paused { "paused" } else { "running" }, 0),
        "updateMediaDb" if ms.total_media > 0 => ("indexed", ms.total_media),
        "runScraper" if ms.scraping => (
            if ms.scrape_paused {
                "paused"
            } else {
                "running"
            },
            0,
        ),
        "runScraper" if ms.scrape_total_scraped > 0 => ("imported", ms.scrape_total_scraped),
        _ => ("", 0),
    }
}

/// Build the rows of the open page, with their stacked geometry.
fn rows(ctx: &Ctx, app: &App, page: &str) -> Vec<SettingsRow> {
    let ms = crate::router::media_state(ctx);
    let index_busy = ms.indexing || ms.optimizing;
    let m = metrics(app);
    let language = lock(&ctx.shared).persist.settings.language.clone();
    let mut offset = 0;
    rules::page_rows(page, &inputs(ctx))
        .into_iter()
        .map(|row| {
            let mut out = SettingsRow {
                kind: SharedString::from(if row.is_field() { "field" } else { "header" }),
                id: SharedString::from(row.id()),
                enabled: true,
                y_offset: offset as f32,
                ..Default::default()
            };
            let height = match row {
                Row::Header(_) => m.header,
                Row::Field { id, control } => {
                    out.control = SharedString::from(match control {
                        Control::Toggle => "toggle",
                        Control::Picker => "picker",
                        Control::Action => "action",
                        Control::Navigate => "navigate",
                    });
                    match control {
                        Control::Toggle => out.checked = checked(ctx, id),
                        Control::Picker => {
                            out.value = SharedString::from(current_value(ctx, id).as_str());
                        }
                        Control::Action => {
                            let busy = rules::action_busy(id, index_busy, ms.scraping);
                            out.busy = busy;
                            out.enabled = !rules::action_disabled(id, index_busy, ms.scraping);
                            out.value = SharedString::from(rules::action_label_key(id, busy));
                            let (status, count) = action_status(&ms, id);
                            out.status_key = SharedString::from(status);
                            out.status_count =
                                zaparoo_app::format::count(i64::from(count), &language).into();
                        }
                        Control::Navigate => {}
                    }
                    if out.status_key.is_empty() {
                        m.row
                    } else {
                        m.row + m.action_band
                    }
                }
            };
            out.height = height as f32;
            offset += height;
            out
        })
        .collect()
}

/// The root grid's band and cell fit, on the shared grid geometry.
/// The category grid, as `SettingsScreen.qml`'s `categoryGrid` resolves
/// it: the Hub's own insets and gaps, its band from the status strip to
/// a bottom margin that clears the active label, and cells capped at the
/// Hub's resolved tile size so both grids read as the same object.
fn root_geometry(ctx: &Ctx, app: &App) -> (i32, i32, i32, i32, Insets, paged_grid::Fit) {
    root_geometry_for(
        &crate::router::output_scene(app).inputs(),
        lock(&ctx.shared).persist.settings.orientation != "horizontal",
    )
}

fn root_geometry_for(
    inputs: &zaparoo_app::sizing::Inputs,
    rotated: bool,
) -> (i32, i32, i32, i32, Insets, paged_grid::Fit) {
    let derived = zaparoo_app::sizing::derive(inputs);
    let profile = layouts::profile(ThemeId::current(inputs), View::GamesGrid, inputs);
    let compact = derived.tier == zaparoo_app::sizing::Tier::T240;
    let top_margin = inputs.pct_h(if compact { 1.0 } else { 2.0 });
    let grid_y = derived.header_bottom
        + profile.status.top_margin
        + profile.status.strip_height
        + top_margin;
    let bottom = if compact {
        derived.help_bar_height + derived.hub_active_label_height
    } else {
        inputs.pct_h(15.0)
    };
    let grid_height = (inputs.screen_height as i32 - grid_y - bottom).max(0);
    let insets = Insets {
        left: derived.hub_grid_side_inset,
        right: derived.hub_grid_side_inset,
        top: derived.hub_grid_top_inset,
        bottom: derived.hub_grid_bottom_inset,
        column_gap: derived.hub_grid_column_gap,
        row_gap: derived.hub_grid_row_gap,
    };
    let (columns, grid_rows) = rules::root_grid_shape(rules::PAGES.len(), rotated);
    let columns = i32::try_from(columns).unwrap_or(3);
    let grid_rows = i32::try_from(grid_rows).unwrap_or(2);
    let mut fit = paged_grid::fit(
        columns,
        grid_rows,
        inputs.screen_width as i32,
        grid_height,
        None,
        false,
        &insets,
    );
    // The Hub's tile size is the ceiling, per axis, and the block
    // re-centers on what the capped cells actually occupy.
    fit.cell_width = fit.cell_width.min(derived.hub_tile_width);
    fit.cell_height = fit.cell_height.min(derived.hub_tile_height);
    let fields = i32::try_from(rules::PAGES.len()).unwrap_or(columns);
    let visible_columns = columns.min(fields).max(1);
    let visible_rows = grid_rows.min(((fields + columns - 1) / columns).max(1));
    fit.content_width =
        visible_columns * fit.cell_width + (visible_columns - 1) * insets.column_gap;
    fit.content_height = visible_rows * fit.cell_height + (visible_rows - 1) * insets.row_gap;
    fit.block_offset_x = ((fit.available_width - fit.content_width) / 2).max(0);
    fit.block_offset_y = ((fit.available_height - fit.content_height) / 2).max(0);
    (columns, grid_rows, grid_y, grid_height, insets, fit)
}

/// The rows viewport inside the page card: what is left once the hint
/// band and the paddings are taken out.
fn rows_viewport(app: &App) -> i32 {
    rows_viewport_for(&crate::router::output_scene(app).inputs())
}

fn rows_viewport_for(inputs: &zaparoo_app::sizing::Inputs) -> i32 {
    let derived = zaparoo_app::sizing::derive(inputs);
    let profile = layouts::profile(ThemeId::current(inputs), View::GamesGrid, inputs);
    let card_y = derived.header_bottom
        + profile.status.top_margin
        + profile.status.strip_height
        + inputs.pct_h(4.0);
    let bottom = derived.help_bar_height + inputs.pct_h(4.0);
    let card_h = (inputs.screen_height as i32 - card_y - bottom).max(0);
    let hint = 2 * (f64::from(derived.font_body) * 1.362).ceil() as i32;
    // Card padding above, then the hint band and its divider below.
    (card_h - 2 * inputs.pct_h(2.0) - hint - inputs.pct_h(0.5)).max(0)
}

/// Push the open page: its rows, the cursor, and (on the root) the
/// category tiles and their geometry.
pub fn render(ctx: &Ctx, app: &App) {
    let view = app.global::<SettingsView>();
    let page = view.get_page().to_string();
    let rows = rows(ctx, app, &page);
    let index = (view.get_index().max(0) as usize).min(rows.len().saturating_sub(1));
    view.set_index(i32::try_from(index).unwrap_or(0));
    // Keep the focused row inside the viewport; the band never scrolls
    // past the last row.
    let viewport = rows_viewport(app);
    let total = rows.last().map_or(0.0, |r| r.y_offset + r.height);
    let focused = rows.get(index).map_or(0.0, |r| r.y_offset + r.height);
    let scroll = (focused - viewport as f32)
        .max(0.0)
        .min((total - viewport as f32).max(0.0));
    view.set_rows_height(viewport as f32);
    view.set_scroll(scroll);
    crate::view_model::publish(&view.get_rows(), rows, |rows| view.set_rows(rows));

    if page.is_empty() {
        let cells: Vec<GridCell> = rules::PAGES
            .iter()
            .map(|p| GridCell {
                label_key: SharedString::from(p.id),
                glyph_key: SharedString::from(p.glyph),
                ..Default::default()
            })
            .collect();
        let (columns, grid_rows, grid_y, grid_height, _, fit) = root_geometry(ctx, app);
        crate::view_model::publish_cells(&view.get_cells(), cells, |rows| view.set_cells(rows));
        view.set_columns(columns);
        view.set_rows_count(grid_rows);
        view.set_cell_width(fit.cell_width as f32);
        view.set_cell_height(fit.cell_height as f32);
        view.set_block_offset_x(fit.block_offset_x as f32);
        view.set_block_offset_y(fit.block_offset_y as f32);
        view.set_grid_y(grid_y as f32);
        view.set_grid_height(grid_height as f32);
    }
}

/// Re-push the open page after something it shows changed.
pub fn refresh(ctx: &Ctx, app: &App) {
    if app.global::<crate::Shell>().get_active_screen().as_str() != "settings" {
        return;
    }
    render(ctx, app);
}

/// Open a page (the empty id is the root grid) and seat the cursor.
pub fn open_page(ctx: &Ctx, app: &App, page: &str) {
    let view = app.global::<SettingsView>();
    view.set_page(SharedString::from(page));
    let seat = rules::first_navigable(&rules::page_rows(page, &inputs(ctx)));
    view.set_index(i32::try_from(seat).unwrap_or(0));
    render(ctx, app);
}

pub(crate) fn capture_outgoing(app: &App) {
    let view = app.global::<SettingsView>();
    view.set_outgoing_page(view.get_page());
    view.set_outgoing_rows(view.get_rows());
    view.set_outgoing_index(view.get_index());
    view.set_outgoing_scroll(view.get_scroll());
    view.set_outgoing_rows_height(view.get_rows_height());
}

/// A category is another level of Settings, not an in-place repaint.
pub(crate) fn navigate_page(ctx: &Ctx, app: &App, page: &str) {
    let view = app.global::<SettingsView>();
    if view.get_page().as_str() == page {
        return;
    }
    let from = view.get_page().to_string();
    let direction = if page.is_empty() { -1 } else { 1 };
    capture_outgoing(app);
    let ctx = ctx.clone();
    let page = page.to_string();
    crate::router::transition_settings_page(app, direction, move |app| {
        open_page(&ctx, app, &page);
        if page.is_empty() {
            let index = rules::PAGES
                .iter()
                .position(|page| page.id == from)
                .unwrap_or(0);
            app.global::<SettingsView>()
                .set_index(i32::try_from(index).unwrap_or(0));
        }
    });
}

pub fn enter(ctx: &Ctx, app: &App) {
    enter_with_direction(ctx, app, 1);
}

pub fn enter_with_direction(ctx: &Ctx, app: &App, direction: i32) {
    lock(&ctx.shared).persist.active_screen = "settings".to_string();
    crate::router::save_persist(&ctx.shared);
    open_page(ctx, app, "");
    crate::router::transition_to_screen(app, "settings", direction);
}

/// Back out of the About screen onto the page that opened it.
pub fn return_from_about(ctx: &Ctx, app: &App) {
    lock(&ctx.shared).persist.active_screen = "settings".to_string();
    crate::router::save_persist(&ctx.shared);
    show_about_return(ctx, app);
}

pub(crate) fn show_about_return(ctx: &Ctx, app: &App) {
    // Stage the actual destination before arming the route, not a temporary root grid.
    open_page(ctx, app, "pageSupportAbout");
    crate::router::transition_to_screen(app, "settings", -1);
}

// ---------- Input ----------

fn page_rows_now(ctx: &Ctx, app: &App) -> (String, Vec<Row>, usize) {
    let view = app.global::<SettingsView>();
    let page = view.get_page().to_string();
    let rows = rules::page_rows(&page, &inputs(ctx));
    let index = (view.get_index().max(0) as usize).min(rows.len().saturating_sub(1));
    (page, rows, index)
}

pub fn handle_action(ctx: &Ctx, app: &App, action: &str) {
    let (page, rows, index) = page_rows_now(ctx, app);
    let view = app.global::<SettingsView>();

    if page.is_empty() {
        match action {
            actions::LEFT | actions::RIGHT | actions::UP | actions::DOWN => {
                let (dx, dy) = match action {
                    actions::LEFT => (-1, 0),
                    actions::RIGHT => (1, 0),
                    actions::UP => (0, -1),
                    _ => (0, 1),
                };
                let columns = view.get_columns().max(1) as usize;
                let next = rules::root_grid_move(index, rows.len(), columns, dx, dy);
                view.set_index(i32::try_from(next).unwrap_or(0));
            }
            actions::ACCEPT => {
                if let Some(row) = rows.get(index) {
                    navigate_page(ctx, app, row.id());
                }
            }
            actions::CANCEL => {
                lock(&ctx.shared).persist.active_screen = "hub".to_string();
                crate::router::save_persist(&ctx.shared);
                crate::router::transition_to_screen(app, "hub", -1);
            }
            _ => {}
        }
        return;
    }

    match action {
        actions::UP => {
            let next = rules::seek_navigable(&rows, index, -1);
            view.set_index(i32::try_from(next).unwrap_or(0));
            render(ctx, app);
        }
        actions::DOWN => {
            let next = rules::seek_navigable(&rows, index, 1);
            view.set_index(i32::try_from(next).unwrap_or(0));
            render(ctx, app);
        }
        // Left and Right flip a toggle in place; pickers open their list
        // (the Qt screen's own rule, so a long option list never has to
        // be cycled blind).
        actions::LEFT | actions::RIGHT => {
            if let Some(Row::Field { id, control }) = rows.get(index) {
                if *control == Control::Toggle {
                    toggle(ctx, app, id);
                }
            }
        }
        actions::ACCEPT => {
            if let Some(Row::Field { id, control }) = rows.get(index) {
                accept(ctx, app, id, *control);
            }
        }
        actions::CANCEL => navigate_page(ctx, app, ""),
        _ => {}
    }
}

fn accept(ctx: &Ctx, app: &App, id: &str, control: Control) {
    let ms = crate::router::media_state(ctx);
    if rules::action_disabled(id, ms.indexing || ms.optimizing, ms.scraping) {
        return;
    }
    match control {
        Control::Toggle => toggle(ctx, app, id),
        Control::Picker => open_picker(ctx, app, id),
        Control::Navigate | Control::Action => match id {
            "aboutLicense" => crate::router::enter_about(ctx, app),
            "documentation" => crate::router::open_documentation_qr(app),
            "uploadLog" => crate::log_upload::open(ctx, app),
            "crtCalibration" => crate::router::open_crt_calibration(ctx, app),
            "updateMediaDb" => {
                if ms.indexing || ms.optimizing {
                    let client = ctx.store.client();
                    let ctx2 = ctx.clone();
                    let weak = app.as_weak();
                    ctx.handle.spawn(async move {
                        if let Err(e) = client.media_generate_cancel().await {
                            tracing::warn!("cancel index failed: {}", e.message);
                            let _ = weak.upgrade_in_event_loop(move |app| {
                                crate::router::report_action_error(&ctx2, &app, "media_cancel", "");
                            });
                        }
                    });
                } else {
                    crate::media_setup::open(ctx, app, zaparoo_app::media_setup::Kind::Index);
                }
            }
            "runScraper" => {
                if ms.scraping {
                    let client = ctx.store.client();
                    let ctx2 = ctx.clone();
                    let weak = app.as_weak();
                    ctx.handle.spawn(async move {
                        if let Err(e) = client.media_scrape_cancel().await {
                            tracing::warn!("cancel scrape failed: {}", e.message);
                            let _ = weak.upgrade_in_event_loop(move |app| {
                                crate::router::report_action_error(&ctx2, &app, "media_cancel", "");
                            });
                        }
                    });
                } else {
                    crate::media_setup::open(ctx, app, zaparoo_app::media_setup::Kind::Scrape);
                }
            }
            _ => {}
        },
    }
}

/// Flip a toggle and apply it. The CRT switch restarts instead.
fn toggle(ctx: &Ctx, app: &App, id: &str) {
    if id == "crtEnabled" {
        crate::router::stage_restart(ctx, app, PendingRestart::CrtEnabled(!ctx.crt_enabled));
        return;
    }
    let value = {
        let mut shared = lock(&ctx.shared);
        match id {
            "showHidden" => {
                shared.show_hidden = !shared.show_hidden;
                shared.persist.settings.show_hidden = shared.show_hidden;
                shared.show_hidden
            }
            "showOriginalFilenames" => {
                let v = !shared.persist.settings.show_original_filenames;
                shared.persist.settings.show_original_filenames = v;
                v
            }
            "reduceMotion" => {
                let v = !shared.persist.settings.reduce_motion;
                shared.persist.settings.reduce_motion = v;
                v
            }
            "mouseEnabled" => {
                let v = !shared.persist.settings.mouse_enabled;
                shared.persist.settings.mouse_enabled = v;
                v
            }
            "swapConfirmCancel" => {
                let v = !shared.persist.settings.swap_confirm_cancel;
                shared.persist.settings.swap_confirm_cancel = v;
                v
            }
            "swapOptionsView" => {
                let v = !shared.persist.settings.swap_options_view;
                shared.persist.settings.swap_options_view = v;
                v
            }
            "debugLogging" => {
                let v = !shared.persist.settings.debug_logging;
                shared.persist.settings.debug_logging = v;
                v
            }
            _ => return,
        }
    };
    save(ctx, app);
    match id {
        "showHidden" => {
            crate::router::reproject_hub(ctx, app);
            crate::systems::reproject(ctx, app);
        }
        "showOriginalFilenames" => crate::games::reproject(ctx, app),
        "reduceMotion" => {
            app.global::<crate::Shell>().set_reduce_motion(value);
            let scene = crate::router::output_scene(app);
            app.global::<crate::Motion>()
                .set_enabled(crate::display::motion_enabled(
                    value,
                    ctx.is_mister,
                    scene.crt,
                    scene.inputs().resolution_height().max(0) as u32,
                ));
        }
        "mouseEnabled" => app.global::<crate::Shell>().set_mouse_enabled(value),
        "swapConfirmCancel" | "swapOptionsView" => crate::apply_buttons(ctx, app),
        _ => {}
    }
    refresh(ctx, app);
}

/// Open a picker over the row's options, focused on the current value.
fn open_picker(ctx: &Ctx, app: &App, id: &str) {
    let values = options(ctx, id);
    if values.is_empty() {
        return;
    }
    let current = current_value(ctx, id);
    let initial = values.iter().position(|v| *v == current).unwrap_or(0);
    // The view translates the value; the picker rows carry the id so it
    // can, through the same vocabulary the field uses.
    let entries: Vec<crate::MenuEntry> = values
        .iter()
        .map(|value| crate::MenuEntry {
            id: SharedString::from(value.as_str()),
            label: SharedString::default(),
            label_key: SharedString::default(),
        })
        .collect();
    lock(&ctx.shared).list_context = ListContext::SettingsPicker(id.to_string());
    let overlays = app.global::<crate::Overlays>();
    // The view translates both the title and every row through the
    // settings vocabulary.
    overlays.set_list_setting_id(SharedString::from(id));
    overlays.set_list_title(SharedString::default());
    overlays.set_list_entries(ModelRc::new(VecModel::from(entries)));
    overlays.set_list_index(i32::try_from(initial).unwrap_or(0));
    overlays.set_list_open(true);
}

/// A picker row was accepted: stage the restart-applied ones, apply the
/// rest at once.
pub fn picker_selected(ctx: &Ctx, app: &App, id: &str, value: &str) {
    if id == "resolution" {
        if value != lock(&ctx.shared).persist.settings.resolution {
            crate::router::stage_restart(ctx, app, PendingRestart::Resolution(value.to_string()));
        }
        return;
    }
    if id == "crtVideoStandard" {
        if value != lock(&ctx.shared).persist.settings.crt_video_standard {
            crate::router::stage_restart(ctx, app, PendingRestart::CrtStandard(value.to_string()));
        }
        return;
    }
    {
        let mut shared = lock(&ctx.shared);
        let s = &mut shared.persist.settings;
        match id {
            "orientation" => s.orientation = value.to_string(),
            "interfaceProfile" => s.interface_profile = value.to_string(),
            "screensaverTimeout" => s.screensaver_timeout = value.to_string(),
            "systemsLayout" => s.systems_browse_layout = value.to_string(),
            "gamesLayout" => s.games_browse_layout = value.to_string(),
            "systemLogoStyle" => s.system_logo_style = value.to_string(),
            "colorScheme" => s.color_scheme = value.to_string(),
            "colorIntensity" => s.color_intensity = value.to_string(),
            "mediaImageType" => s.media_image_type = value.to_string(),
            "language" => {
                s.language = value.to_string();
                crate::apply_language(value);
                crate::status::set_language(&ctx.status, &crate::effective_language(value));
            }
            "region" => s.region = value.to_string(),
            "clockFormat" => s.clock_format = value.to_string(),
            "buttonLayout" => s.button_layout = value.to_string(),
            _ => return,
        }
    }
    save(ctx, app);
    apply(ctx, app, id, value);
    refresh(ctx, app);
}

/// The live half of a picked value.
fn apply(ctx: &Ctx, app: &App, id: &str, value: &str) {
    match id {
        "orientation" => {
            let rotated = matches!(value, "cw" | "ccw");
            app.global::<crate::Shell>()
                .set_orientation(SharedString::from(value));
            app.global::<crate::Sizing>().set_swap_axes(rotated);
            crate::set_live_orientation(app, value, ctx.framebuffer_size);
        }
        "interfaceProfile" => {
            app.global::<crate::Sizing>()
                .set_handheld(value == "handheld");
            crate::sizing::apply_scene(app, crate::router::output_scene(app));
        }
        "language" | "clockFormat" => crate::router::apply_clock_setting(ctx, app),
        "buttonLayout" => crate::apply_buttons(ctx, app),
        "systemLogoStyle" => crate::systems::reproject(ctx, app),
        "colorScheme" | "colorIntensity" => {
            let (scheme, intensity) = {
                let shared = lock(&ctx.shared);
                (
                    shared.persist.settings.color_scheme.clone(),
                    shared.persist.settings.color_intensity.clone(),
                )
            };
            // apply_palette re-tints the heart glyph itself; the logo
            // ramps follow from the new palette.
            let palette = crate::theme::apply_palette(app, &scheme, &intensity);
            let (rest, focus) = crate::theme::logo_tints(&palette);
            crate::system_logos::set_tints(rest, focus);
            crate::systems::reproject(ctx, app);
            crate::router::reproject_hub(ctx, app);
        }
        "mediaImageType" => ctx.media.set_preferred_image_type(value),
        "screensaverTimeout" => crate::router::reset_idle(ctx, app),
        "systemsLayout" => {
            app.global::<crate::Shell>()
                .set_systems_list_layout(value == "list");
            crate::router::refresh_layout(app);
            crate::systems::render(ctx, app);
        }
        "gamesLayout" => {
            app.global::<crate::Shell>()
                .set_browse_list_layout(value == "list");
            crate::router::refresh_layout(app);
            crate::games::on_layout_changed(ctx, app);
        }
        _ => {}
    }
}

/// Persist the state file and mirror the durable half into
/// `frontend.toml`.
pub fn save(ctx: &Ctx, app: &App) {
    let snapshot = {
        let shared = lock(&ctx.shared);
        shared.persist.clone()
    };
    persist::save(&snapshot);
    let s = &snapshot.settings;
    let mirror = zaparoo_core::config::SettingsMirror {
        resolution: &s.resolution,
        language: &s.language,
        orientation: &s.orientation,
        clock_format: &s.clock_format,
        interface_profile: &s.interface_profile,
        systems_browse_layout: &s.systems_browse_layout,
        games_browse_layout: &s.games_browse_layout,
        system_logo_style: &s.system_logo_style,
        color_scheme: &s.color_scheme,
        color_intensity: &s.color_intensity,
        metadata_scraper: &s.metadata_scraper,
        button_layout: &s.button_layout,
        mouse_enabled: s.mouse_enabled,
        reduce_motion: s.reduce_motion,
        debug_logging: s.debug_logging,
        screensaver_timeout: &s.screensaver_timeout,
        media_image_type: &s.media_image_type,
        favorites_grouping: &s.favorites_grouping,
        show_hidden: s.show_hidden,
        show_original_filenames: s.show_original_filenames,
        swap_confirm_cancel: s.swap_confirm_cancel,
        swap_options_view: s.swap_options_view,
        region: &s.region,
        crt_video_standard: &s.crt_video_standard,
        crt_h_offset: s.crt_h_offset,
        crt_v_offset: s.crt_v_offset,
    };
    if let Err(e) = zaparoo_core::config::save_settings_mirror(&ctx.config_path, mirror) {
        tracing::warn!("could not mirror settings to config: {e}");
        crate::router::report_action_error(ctx, app, "setting", "");
    }
}

/// Mirror semantic rows, then fit both route endpoints to the CRT's own geometry.
#[cfg(feature = "mister")]
pub(crate) fn mirror_geometry(primary: &App, crt: &App) {
    let source = primary.global::<SettingsView>();
    let target = crt.global::<SettingsView>();
    let sizing = crt.global::<crate::Sizing>();
    let inputs = crate::sizing::Scene::of(
        crt,
        f64::from(sizing.get_screen_width()),
        f64::from(sizing.get_screen_height()),
        true,
    )
    .inputs();
    let (columns, rows, y, height, _, fit) = root_geometry_for(
        &inputs,
        crt.global::<crate::Shell>().get_orientation().as_str() != "horizontal",
    );
    target.set_columns(columns);
    target.set_rows_count(rows);
    target.set_cell_width(fit.cell_width as f32);
    target.set_cell_height(fit.cell_height as f32);
    target.set_block_offset_x(fit.block_offset_x as f32);
    target.set_block_offset_y(fit.block_offset_y as f32);
    target.set_grid_y(y as f32);
    target.set_grid_height(height as f32);
    let viewport = rows_viewport_for(&inputs) as f32;
    let (rows, scroll) = mirrored_rows(&source.get_rows(), source.get_index(), viewport, &inputs);
    crate::view_model::publish(&target.get_rows(), rows, |rows| target.set_rows(rows));
    target.set_scroll(scroll);
    target.set_rows_height(viewport);
    let (rows, scroll) = mirrored_rows(
        &source.get_outgoing_rows(),
        source.get_outgoing_index(),
        viewport,
        &inputs,
    );
    crate::view_model::publish(&target.get_outgoing_rows(), rows, |rows| {
        target.set_outgoing_rows(rows);
    });
    target.set_outgoing_scroll(scroll);
    target.set_outgoing_rows_height(viewport);
}

#[cfg(feature = "mister")]
fn mirrored_rows(
    rows: &ModelRc<SettingsRow>,
    index: i32,
    viewport: f32,
    inputs: &zaparoo_app::sizing::Inputs,
) -> (Vec<SettingsRow>, f32) {
    use slint::Model;
    let mut offset = 0;
    let rows: Vec<_> = rows
        .iter()
        .map(|mut row| {
            let height = if row.kind.as_str() == "header" {
                inputs.pct_h(5.0)
            } else {
                inputs.pct_h(8.0)
                    + if row.status_key.is_empty() {
                        0
                    } else {
                        inputs.pct_h(3.2)
                    }
            };
            row.y_offset = offset as f32;
            row.height = height as f32;
            offset += height;
            row
        })
        .collect();
    let focused = rows
        .get(index.max(0) as usize)
        .map_or(0.0, |row| row.y_offset + row.height);
    let scroll = (focused - viewport)
        .max(0.0)
        .min((offset as f32 - viewport).max(0.0));
    (rows, scroll)
}

// ---------- Pointer ----------

fn focus(ctx: &Ctx, app: &App, index: usize) -> bool {
    let shell = app.global::<crate::Shell>();
    if shell.get_transitioning() || crate::press_feedback::pending(app) {
        return false;
    }
    let (_, rows, _) = page_rows_now(ctx, app);
    if !rows.get(index).is_some_and(|r| r.is_field()) {
        return false;
    }
    app.global::<SettingsView>()
        .set_index(i32::try_from(index).unwrap_or(0));
    true
}

pub fn bind_input(ctx: &Arc<Ctx>, app: &App) {
    let input = app.global::<SettingsInput>();
    for (hover, accept) in [(true, false), (false, true)] {
        let ctx = ctx.clone();
        let weak = app.as_weak();
        let handler = move |i: i32| {
            let Some(app) = weak.upgrade() else {
                return;
            };
            let Ok(index) = usize::try_from(i) else {
                return;
            };
            if focus(&ctx, &app, index) && accept {
                crate::router::handle_action(&ctx, &app, actions::ACCEPT);
            }
        };
        if hover {
            input.on_row_hovered(handler.clone());
            input.on_cell_hovered(handler);
        } else {
            input.on_row_clicked(handler.clone());
            input.on_cell_clicked(handler);
        }
    }
}

#[cfg(test)]
mod status_tests {
    use super::action_status;
    use zaparoo_core::store::MediaStatusState;

    #[test]
    fn qt_action_status_priority_and_idle_counts() {
        let mut ms = MediaStatusState {
            total_media: 1000,
            scrape_total_scraped: 200,
            ..Default::default()
        };
        assert_eq!(action_status(&ms, "updateMediaDb"), ("indexed", 1000));
        assert_eq!(action_status(&ms, "runScraper"), ("imported", 200));
        ms.indexing = true;
        assert_eq!(action_status(&ms, "updateMediaDb"), ("running", 0));
        ms.paused = true;
        assert_eq!(action_status(&ms, "updateMediaDb"), ("paused", 0));
        ms.optimizing = true;
        assert_eq!(action_status(&ms, "updateMediaDb"), ("optimizing", 0));
        ms.scraping = true;
        assert_eq!(action_status(&ms, "runScraper"), ("running", 0));
        ms.scrape_paused = true;
        assert_eq!(action_status(&ms, "runScraper"), ("paused", 0));
        assert_eq!(action_status(&ms, "uploadLog"), ("", 0));
        assert_eq!(
            action_status(&MediaStatusState::default(), "runScraper"),
            ("", 0)
        );
    }
}
