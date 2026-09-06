// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// The Systems screen driver: projects a category's rows through
// `zaparoo_app::systems`, drives the cursor through `paged_grid`, runs
// the page swoop, owns the Options menu, and paints the current page into
// the `SystemsView` global. Mirrors SystemsScreen.qml plus the Systems
// half of Main.qml's routing and `models/systems.rs`.

use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use zaparoo_app::layouts::{self, Body, ThemeId, View};
use zaparoo_app::paged_grid::{self, Grid, Insets};
use zaparoo_app::systems::{self as rules, CatalogSystem, Region, SystemRow};
use zaparoo_core::input_actions::actions;
use zaparoo_core::media_types::SystemInfo;

use crate::router::{lock, Ctx, Shared};
use crate::{App, GridCell, SystemsInput, SystemsView};

/// Swoop duration and the settle before the strip re-arms.
const SWOOP_MS: u64 = 260;
const REARM_MS: u64 = 50;

#[derive(Debug)]
pub struct SystemsModel {
    pub grid: Grid,
    pub rows: Vec<SystemRow>,
    pub category: String,
    pub focus_armed: bool,
    pub restore_done: bool,
    pub activate_pulse: i32,
    pub release_pulse: i32,
    /// A page swoop is in flight; input waits for the commit.
    pub sliding: bool,
}

impl SystemsModel {
    pub fn new() -> Self {
        Self {
            grid: Grid::new(4, 3),
            rows: Vec::new(),
            category: String::new(),
            focus_armed: false,
            restore_done: false,
            activate_pulse: 0,
            release_pulse: 0,
            sliding: false,
        }
    }

    pub fn current(&self) -> Option<&SystemRow> {
        self.rows.get(self.grid.current_index())
    }
}

impl Default for SystemsModel {
    fn default() -> Self {
        Self::new()
    }
}

/// The display region for names and logo art: the setting, with `auto`
/// following the effective UI language.
pub fn region(shared: &Shared) -> Region {
    let locale = crate::effective_language(&shared.persist.settings.language);
    rules::resolve_region(&shared.persist.settings.region, &locale)
}

fn catalog_systems(systems: &[SystemInfo]) -> Vec<CatalogSystem> {
    systems
        .iter()
        .map(|s| CatalogSystem {
            id: s.id.clone(),
            name: s.name.clone(),
            category: s.category.clone(),
            zap_script: s.zap_script.clone(),
        })
        .collect()
}

/// The rows a category shows right now.
pub fn project(shared: &Shared, category: &str) -> Vec<SystemRow> {
    rules::rows_for_category(
        &catalog_systems(&shared.systems),
        category,
        &shared.hidden_system_ids,
        shared.show_hidden,
        region(shared),
        &|_| None,
    )
}

/// Localized display name for a system id (Hub tiles and titles).
pub fn display_name(shared: &Shared, id: &str) -> String {
    let fallback = shared
        .systems
        .iter()
        .find(|s| s.id == id)
        .map_or_else(|| id.to_string(), |s| s.name.clone());
    rules::display_name(id, &fallback, region(shared), None)
}

fn catalog_entry(shared: &Shared, id: &str) -> Option<SystemInfo> {
    shared.systems.iter().find(|s| s.id == id).cloned()
}

/// Enter the category's systems: project the rows, seat the persisted
/// system, and route to the screen (with or without the slide).
pub fn enter(ctx: &Ctx, app: &App, category: &str, animate: bool) {
    {
        let mut shared = lock(&ctx.shared);
        let rows = project(&shared, category);
        let restore_id = shared.persist.systems.system_id.clone();
        let index = rows.iter().position(|s| s.id == restore_id).unwrap_or(0);
        shared.persist.hub.category = category.to_string();
        shared.persist.active_screen = "systems".to_string();
        let model = &mut shared.systems_model;
        model.category = category.to_string();
        model.rows = rows;
        model.grid.set_item_count(model.rows.len());
        model.grid.set_current_index_immediate(index);
        model.restore_done = true;
        model.sliding = false;
    }
    crate::router::save_persist(&ctx.shared);
    let view = app.global::<SystemsView>();
    view.set_category(SharedString::from(category));
    view.set_error(SharedString::default());
    view.set_loading(false);
    render(ctx, app);
    if animate {
        crate::router::transition_to_screen(app, "systems", 1);
    } else {
        app.global::<crate::Shell>()
            .set_active_screen(SharedString::from("systems"));
        crate::router::refresh_layout(app);
    }
}

/// Re-run the projection after a hide toggle, a Show hidden flip or a
/// region change, keeping the focused system when it survives.
pub fn reproject(ctx: &Ctx, app: &App) {
    {
        let mut shared = lock(&ctx.shared);
        let category = shared.systems_model.category.clone();
        if category.is_empty() {
            return;
        }
        let rows = project(&shared, &category);
        let current_id = shared.systems_model.current().map(|s| s.id.clone());
        let index = current_id
            .and_then(|id| rows.iter().position(|s| s.id == id))
            .unwrap_or(0);
        let model = &mut shared.systems_model;
        model.rows = rows;
        model.grid.set_item_count(model.rows.len());
        model.grid.set_current_index_immediate(index);
    }
    render(ctx, app);
}

fn logo_image(px: &crate::system_logos::LogoPixels) -> slint::Image {
    slint::Image::from_rgba8(
        slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(
            &px.rgba, px.width, px.height,
        ),
    )
}

/// One tile: the tinted logo pair (or the original art under the color
/// style), else the wordmark fallback.
fn cell_for(row: &SystemRow, logo_style: &str) -> GridCell {
    let mut cell = GridCell {
        name: SharedString::from(row.name.as_str()),
        wordmark: true,
        hidden: row.hidden,
        ..Default::default()
    };
    let stem = row.cover_key.strip_prefix("systems/").unwrap_or(&row.id);
    let (rest, focus) = if logo_style == "color" {
        let original =
            crate::system_logos::logo_for(stem).or_else(|| crate::system_logos::logo_for(&row.id));
        (original.clone(), original)
    } else {
        (
            crate::system_logos::tinted_logo_for(stem, false)
                .or_else(|| crate::system_logos::tinted_logo_for(&row.id, false)),
            crate::system_logos::tinted_logo_for(stem, true)
                .or_else(|| crate::system_logos::tinted_logo_for(&row.id, true)),
        )
    };
    if let Some(rest) = rest {
        cell.cover = logo_image(&rest);
        cell.has_cover = true;
    }
    if let Some(focus) = focus {
        cell.cover_focus = logo_image(&focus);
        cell.has_cover_focus = true;
    }
    cell
}

/// The grid's band and cell fit for the scene (SystemsScreen.qml's
/// anchors resolved through the browse layout profile).
struct Geometry {
    columns: i32,
    rows: i32,
    grid_y: i32,
    grid_height: i32,
    fit: paged_grid::Fit,
}

fn geometry(app: &App) -> Geometry {
    let inputs = crate::router::output_scene(app).inputs();
    let derived = zaparoo_app::sizing::derive(&inputs);
    let profile = layouts::profile(ThemeId::current(&inputs), View::SystemsGrid, &inputs);
    let (grid, footer) = match profile.body {
        Body::Grid { grid, footer } => (grid, footer),
        Body::List { .. } => unreachable!("the systems grid view resolves to a grid body"),
    };
    let grid_y = derived.header_bottom + profile.status.top_margin + profile.status.strip_height;
    let bottom = if derived.tier == zaparoo_app::sizing::Tier::T240 {
        derived.help_bar_height + footer.active_label_height
    } else {
        footer.grid_bottom_margin
    };
    let grid_height = (inputs.screen_height as i32 - grid_y - bottom).max(0);
    let insets = Insets {
        left: grid.left_inset,
        right: grid.right_inset,
        top: grid.top_inset,
        bottom: grid.bottom_inset,
        column_gap: grid.column_gap,
        row_gap: grid.row_gap,
    };
    let columns = derived.systems_grid_columns;
    let rows = derived.systems_grid_rows;
    Geometry {
        columns,
        rows,
        grid_y,
        grid_height,
        fit: paged_grid::fit(
            columns,
            rows,
            inputs.screen_width as i32,
            grid_height,
            None,
            false,
            &insets,
        ),
    }
}

fn page_cells(shared: &Shared, page: usize) -> Vec<GridCell> {
    let model = &shared.systems_model;
    let page_size = model.grid.page_size();
    model
        .rows
        .iter()
        .skip(page * page_size)
        .take(page_size)
        .map(|row| cell_for(row, &shared.persist.settings.system_logo_style))
        .collect()
}

/// Paint the current page, the cursor, the caption and the geometry.
pub fn render(ctx: &Ctx, app: &App) {
    let geometry = geometry(app);
    let view = app.global::<SystemsView>();
    let mut shared = lock(&ctx.shared);
    let shape_changed = {
        let model = &mut shared.systems_model;
        let changed = model.grid.columns() != geometry.columns as usize
            || model.grid.rows() != geometry.rows as usize;
        if changed {
            let current = model.current().map(|s| s.id.clone());
            model.grid.set_shape(
                geometry.columns.max(1) as usize,
                geometry.rows.max(1) as usize,
            );
            if let Some(index) = current.and_then(|id| model.rows.iter().position(|s| s.id == id)) {
                model.grid.set_current_index_immediate(index);
            }
        }
        changed
    };
    let _ = shape_changed;
    let model = &shared.systems_model;
    let page = model.grid.current_page();
    let start = page * model.grid.page_size();
    view.set_cells(ModelRc::new(VecModel::from(page_cells(&shared, page))));
    view.set_next_cells(ModelRc::new(VecModel::from(Vec::<GridCell>::new())));
    view.set_selected_local(
        i32::try_from(model.grid.current_index().saturating_sub(start)).unwrap_or(0),
    );
    view.set_count(i32::try_from(model.rows.len()).unwrap_or(0));
    view.set_focus_ready(model.focus_armed || model.restore_done);
    view.set_columns(geometry.columns);
    view.set_rows(geometry.rows);
    view.set_cell_width(geometry.fit.cell_width as f32);
    view.set_cell_height(geometry.fit.cell_height as f32);
    view.set_block_offset_x(geometry.fit.block_offset_x as f32);
    view.set_block_offset_y(geometry.fit.block_offset_y as f32);
    view.set_grid_y(geometry.grid_y as f32);
    view.set_grid_height(geometry.grid_height as f32);
    view.set_page(i32::try_from(page).unwrap_or(0));
    view.set_total_pages(i32::try_from(model.grid.total_page_count()).unwrap_or(1));
    view.set_has_pages_above(model.grid.has_pages_above());
    view.set_has_pages_below(model.grid.has_pages_below());
    view.set_activate_pulse(model.activate_pulse);
    view.set_release_pulse(model.release_pulse);
    if let Some(row) = model.current() {
        view.set_label_name(SharedString::from(row.name.as_str()));
        view.set_label_hidden(row.hidden);
    } else {
        view.set_label_name(SharedString::default());
        view.set_label_hidden(false);
    }
    // The detailed list shows every row; the grid page is enough for
    // the swoop strip.
    let list_rows: Vec<GridCell> = model
        .rows
        .iter()
        .map(|row| cell_for(row, &shared.persist.settings.system_logo_style))
        .collect();
    view.set_list_rows(ModelRc::new(VecModel::from(list_rows)));
    view.set_list_index(i32::try_from(model.grid.current_index()).unwrap_or(0));
}

/// Screen state (SystemsScreen.qml's `_state`).
fn state(app: &App) -> &'static str {
    let view = app.global::<SystemsView>();
    if view.get_loading() {
        "loading"
    } else if !view.get_error().is_empty() {
        "error"
    } else if view.get_count() == 0 {
        "empty"
    } else {
        "ready"
    }
}

fn persist_selection(ctx: &Ctx) {
    {
        let mut shared = lock(&ctx.shared);
        let id = shared.systems_model.current().map(|s| s.id.clone());
        if let Some(id) = id {
            shared.persist.systems.system_id = id;
        }
    }
    crate::router::save_persist(&ctx.shared);
}

#[cfg(feature = "mister")]
fn request_cached_page_transition(app: &App, direction: i32, columns: i32, rows: i32) -> bool {
    let shell = app.global::<crate::Shell>();
    if shell.get_orientation().as_str() != "horizontal" || shell.get_systems_list_layout() {
        return false;
    }
    let sizing = app.global::<crate::Sizing>();
    let width = sizing.get_screen_width().round().max(0.0) as u32;
    let height = sizing.get_screen_height().round().max(0.0) as u32;
    let Some(geometry) = crate::sizing::mister_browse_grid_transition_geometry(
        width,
        height,
        columns.max(0) as u32,
        rows.max(0) as u32,
    ) else {
        return false;
    };
    crate::mister::request_page_transition(geometry, direction)
}

#[cfg(not(feature = "mister"))]
fn request_cached_page_transition(_app: &App, _direction: i32, _columns: i32, _rows: i32) -> bool {
    false
}

/// A cursor move landed on another page: swoop the strip one period in
/// `dir`, then commit the new page. Reduce motion cuts instead.
fn slide_to_current_page(ctx: &Ctx, app: &App, from_page: usize) {
    let (to_page, columns, rows, reduce_motion) = {
        let mut shared = lock(&ctx.shared);
        let reduce_motion = shared.persist.settings.reduce_motion;
        let model = &mut shared.systems_model;
        model.sliding = true;
        (
            model.grid.current_page(),
            model.grid.columns() as i32,
            model.grid.rows() as i32,
            reduce_motion,
        )
    };
    let dir: i32 = if to_page > from_page { 1 } else { -1 };
    let view = app.global::<SystemsView>();
    let target_local = {
        let shared = lock(&ctx.shared);
        let model = &shared.systems_model;
        model.grid.current_index() - to_page * model.grid.page_size()
    };
    view.set_slide_dir(dir);
    view.set_transition_target_index(i32::try_from(target_local).unwrap_or(0));
    if reduce_motion {
        lock(&ctx.shared).systems_model.sliding = false;
        render(ctx, app);
        return;
    }
    if request_cached_page_transition(app, dir, columns, rows) {
        view.set_slide_anim(false);
        view.set_cached_transition(true);
        render(ctx, app);
        let weak = app.as_weak();
        let ctx = ctx.clone();
        slint::Timer::single_shot(std::time::Duration::from_millis(SWOOP_MS), move || {
            if let Some(app) = weak.upgrade() {
                lock(&ctx.shared).systems_model.sliding = false;
                let view = app.global::<SystemsView>();
                view.set_cached_transition(false);
                view.set_slide_anim(true);
            }
        });
        return;
    }
    crate::drs::heavy_begin();
    let next: Vec<GridCell> = {
        let shared = lock(&ctx.shared);
        page_cells(&shared, to_page)
    };
    view.set_slide_anim(true);
    view.set_selected_local(-1);
    view.set_next_cells(ModelRc::new(VecModel::from(next)));
    view.set_page_slide(dir as f32);
    let weak = app.as_weak();
    let ctx = ctx.clone();
    slint::Timer::single_shot(std::time::Duration::from_millis(SWOOP_MS), move || {
        crate::drs::heavy_end();
        let Some(app) = weak.upgrade() else {
            return;
        };
        lock(&ctx.shared).systems_model.sliding = false;
        let view = app.global::<SystemsView>();
        view.set_slide_anim(false);
        render(&ctx, &app);
        view.set_page_slide(0.0);
        let weak = app.as_weak();
        slint::Timer::single_shot(std::time::Duration::from_millis(REARM_MS), move || {
            if let Some(app) = weak.upgrade() {
                app.global::<SystemsView>().set_slide_anim(true);
            }
        });
    });
}

/// A directional or page move: returns whether the index changed and the
/// page it started on.
fn move_cursor(ctx: &Ctx, action: &str) -> Option<(bool, usize)> {
    let mut shared = lock(&ctx.shared);
    let list_layout = shared.persist.settings.systems_browse_layout == "list";
    let model = &mut shared.systems_model;
    if model.sliding {
        return None;
    }
    let from_page = model.grid.current_page();
    let count = model.grid.item_count();
    let moved = if list_layout {
        // The list is one linear column; Left and Right do nothing.
        let delta: i64 = match action {
            actions::UP => -1,
            actions::DOWN => 1,
            actions::PAGE_PREV => -(model.grid.page_size() as i64),
            actions::PAGE_NEXT => model.grid.page_size() as i64,
            _ => return Some((false, from_page)),
        };
        if count == 0 {
            false
        } else {
            let current = model.grid.current_index() as i64;
            let mut next = current + delta;
            if delta.abs() == 1 {
                if next < 0 {
                    next = count as i64 - 1;
                } else if next >= count as i64 {
                    next = 0;
                }
            } else {
                next = next.clamp(0, count as i64 - 1);
            }
            if next == current {
                false
            } else {
                model.grid.set_current_index_immediate(next as usize);
                true
            }
        }
    } else {
        match action {
            actions::LEFT => model.grid.move_selection(-1, 0),
            actions::RIGHT => model.grid.move_selection(1, 0),
            actions::DOWN => model.grid.move_selection(0, 1),
            actions::UP => model.grid.move_selection(0, -1),
            actions::PAGE_PREV => model.grid.page_by(-1),
            actions::PAGE_NEXT => model.grid.page_by(1),
            _ => return Some((false, from_page)),
        }
    };
    Some((moved, from_page))
}

/// SystemsScreen.qml's `handleAction`.
pub fn handle_action(ctx: &Ctx, app: &App, action: &str) {
    let loading_cue = app.global::<SystemsView>().get_loading();
    let is_move = matches!(
        action,
        actions::LEFT
            | actions::RIGHT
            | actions::UP
            | actions::DOWN
            | actions::PAGE_PREV
            | actions::PAGE_NEXT
    );
    if is_move && loading_cue {
        return;
    }
    lock(&ctx.shared).systems_model.focus_armed = true;
    if is_move {
        if matches!(action, actions::PAGE_PREV | actions::PAGE_NEXT) && state(app) != "ready" {
            return;
        }
        let Some((moved, from_page)) = move_cursor(ctx, action) else {
            return;
        };
        if moved {
            persist_selection(ctx);
            let list_layout = lock(&ctx.shared).persist.settings.systems_browse_layout == "list";
            let to_page = lock(&ctx.shared).systems_model.grid.current_page();
            if !list_layout && to_page != from_page {
                slide_to_current_page(ctx, app, from_page);
                return;
            }
        }
        render(ctx, app);
        return;
    }
    match action {
        actions::ACCEPT => match state(app) {
            "loading" => {}
            "error" | "empty" => crate::router::retry_catalog(ctx),
            _ => activate_current(ctx, app),
        },
        actions::CONTEXT_MENU => {
            if state(app) == "ready" {
                persist_selection(ctx);
                open_context_menu(ctx, app);
            }
        }
        actions::CANCEL => {
            lock(&ctx.shared).persist.active_screen = "hub".to_string();
            crate::router::save_persist(&ctx.shared);
            crate::router::transition_to_screen(app, "hub", -1);
        }
        _ => {}
    }
}

fn activate_current(ctx: &Ctx, app: &App) {
    {
        let mut shared = lock(&ctx.shared);
        if shared.systems_model.current().is_none() {
            return;
        }
        shared.systems_model.activate_pulse += 1;
    }
    persist_selection(ctx);
    render(ctx, app);
    let delay = if app.global::<crate::Motion>().get_enabled() {
        34
    } else {
        0
    };
    let ctx = ctx.clone();
    let weak = app.as_weak();
    slint::Timer::single_shot(std::time::Duration::from_millis(delay), move || {
        let Some(app) = weak.upgrade() else {
            return;
        };
        let system = {
            let shared = lock(&ctx.shared);
            shared
                .systems_model
                .current()
                .and_then(|row| catalog_entry(&shared, &row.id))
        };
        if let Some(system) = system {
            lock(&ctx.shared).persist.games.entered_from_hub = false;
            crate::router::enter_games(&ctx, &app, &system);
            if !system.zap_script.is_empty() {
                lock(&ctx.shared).systems_model.release_pulse += 1;
                render(&ctx, &app);
            }
        }
    });
}

/// Options on the focused system (Main.qml's `systems` owner).
fn open_context_menu(ctx: &Ctx, app: &App) {
    let (row, has_launchers) = {
        let shared = lock(&ctx.shared);
        let Some(row) = shared.systems_model.current().cloned() else {
            return;
        };
        let has_launchers = shared.launchers.iter().any(|l| l.system_id == row.id);
        (row, has_launchers)
    };
    let launchable = row.is_launchable();
    let mut entries = vec![crate::router::menu_entry("launch_system", "Launch system")];
    if !launchable {
        entries.push(crate::router::menu_entry(
            "launch_random_system",
            "Random game",
        ));
    }
    if !launchable && has_launchers {
        entries.push(crate::router::menu_entry(
            "change_launcher",
            "Change launcher",
        ));
    }
    entries.push(crate::router::menu_entry("add_to_hub", "Add to Hub"));
    entries.push(crate::router::menu_entry(
        "toggle_hide_system",
        if row.hidden { "Unhide" } else { "Hide" },
    ));
    if !launchable && !crate::router::media_busy(app) {
        entries.push(crate::router::menu_entry(
            "index_system",
            "Update media database",
        ));
        entries.push(crate::router::menu_entry(
            "scrape_system",
            "Update metadata",
        ));
    }
    crate::router::present_systems_context_menu(ctx, app, entries);
}

pub fn context_accept(ctx: &Ctx, app: &App, id: &str) {
    let row = {
        let shared = lock(&ctx.shared);
        shared.systems_model.current().cloned()
    };
    let Some(row) = row else {
        return;
    };
    match id {
        "launch_system" => crate::router::launch(ctx, app, row.launch_text()),
        "launch_random_system" => {
            if let Some(text) = rules::random_launch_text(std::slice::from_ref(&row.id)) {
                crate::router::launch(ctx, app, text);
            }
        }
        "change_launcher" => crate::router::open_launcher_picker(ctx, app, &row.id),
        "add_to_hub" => crate::hub::add_target(ctx, app, "system", &row.id, "", "", "", "", ""),
        "toggle_hide_system" => crate::router::toggle_hidden_system(ctx, app, &row.id),
        "index_system" => crate::router::start_index(ctx, app, Some(vec![row.id.clone()])),
        "scrape_system" => crate::router::start_scrape(ctx, vec![row.id.clone()], false),
        _ => {}
    }
}

fn pointer_select(ctx: &Ctx, app: &App, local: i32) -> bool {
    {
        let mut shared = lock(&ctx.shared);
        let model = &mut shared.systems_model;
        if model.sliding {
            return false;
        }
        let Ok(local) = usize::try_from(local) else {
            return false;
        };
        let index = model.grid.current_page() * model.grid.page_size() + local;
        if index >= model.rows.len() {
            return false;
        }
        model.focus_armed = true;
        model.grid.set_current_index_immediate(index);
    }
    persist_selection(ctx);
    render(ctx, app);
    true
}

/// Wire the pointer and page-cue callbacks.
pub fn bind_input(ctx: &std::sync::Arc<Ctx>, app: &App) {
    let input = app.global::<SystemsInput>();
    {
        let ctx = ctx.clone();
        let weak = app.as_weak();
        input.on_cell_hovered(move |local| {
            if let Some(app) = weak.upgrade() {
                pointer_select(&ctx, &app, local);
            }
        });
    }
    {
        let ctx = ctx.clone();
        let weak = app.as_weak();
        input.on_cell_clicked(move |local| {
            if let Some(app) = weak.upgrade() {
                if pointer_select(&ctx, &app, local) {
                    handle_action(&ctx, &app, actions::ACCEPT);
                }
            }
        });
    }
    {
        let ctx = ctx.clone();
        let weak = app.as_weak();
        input.on_cell_right_clicked(move |local| {
            if let Some(app) = weak.upgrade() {
                if pointer_select(&ctx, &app, local) {
                    handle_action(&ctx, &app, actions::CONTEXT_MENU);
                }
            }
        });
    }
    let page = |ctx: std::sync::Arc<Ctx>, weak: slint::Weak<App>| {
        move |delta: i32| {
            if let Some(app) = weak.upgrade() {
                let action = if delta > 0 {
                    actions::PAGE_NEXT
                } else {
                    actions::PAGE_PREV
                };
                handle_action(&ctx, &app, action);
            }
        }
    };
    input.on_wheel(page(ctx.clone(), app.as_weak()));
    input.on_page_requested(page(ctx.clone(), app.as_weak()));
}
