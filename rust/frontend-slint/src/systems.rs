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
use zaparoo_app::media_list as list_rules;
use zaparoo_app::paged_grid::{self, Grid, Insets};
use zaparoo_app::systems::{self as rules, CatalogSystem, Region, SystemRow};
use zaparoo_core::endpoints::systems_favorites::SystemsFavoritesEndpoint;
use zaparoo_core::input_actions::actions;
use zaparoo_core::media_types::{SystemInfo, SystemsResult};
use zaparoo_core::remote_resource::ResourceStatus;

use crate::router::{lock, Ctx, Shared};
use crate::{App, GridCell, SystemsInput, SystemsView};

/// Swoop duration and the settle before the strip re-arms.
const SWOOP_MS: u64 = 260;
const REARM_MS: u64 = 50;

/// Which list the screen shows: one category's systems, or the systems
/// that hold favorites (`FavoriteSystemsScreen.qml`, structurally a
/// Systems screen and grouped with it by the layout profile).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SystemsMode {
    Category,
    Favorites,
}

impl SystemsMode {
    pub fn token(self) -> &'static str {
        match self {
            Self::Category => "systems",
            Self::Favorites => "favorite-systems",
        }
    }
}

#[derive(Debug, Clone)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "one flag per SystemsScreen property the shell binds"
)]
pub struct SystemsModel {
    pub grid: Grid,
    pub rows: Vec<SystemRow>,
    pub mode: SystemsMode,
    pub category: String,
    /// The favorites catalog Core answered with, kept so a hide toggle
    /// or region change can re-project without refetching.
    pub favorites: Vec<CatalogSystem>,
    pub loading: bool,
    pub focus_armed: bool,
    pub restore_done: bool,
    pub activate_pulse: i32,
    pub release_pulse: i32,
    /// A page swoop is in flight; input waits for the commit.
    pub sliding: bool,
    pub transition_seq: u64,
    pub page_seq: u64,
    pub cut_next_page: bool,
}

impl SystemsModel {
    pub fn new() -> Self {
        Self {
            grid: Grid::new(4, 3),
            rows: Vec::new(),
            mode: SystemsMode::Category,
            category: String::new(),
            favorites: Vec::new(),
            loading: false,
            focus_armed: false,
            restore_done: false,
            activate_pulse: 0,
            release_pulse: 0,
            sliding: false,
            transition_seq: 0,
            page_seq: 0,
            cut_next_page: false,
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

pub fn catalog_systems(systems: &[SystemInfo]) -> Vec<CatalogSystem> {
    systems
        .iter()
        .map(|s| CatalogSystem {
            id: s.id.clone(),
            name: s.name.clone(),
            category: s.category.clone(),
            zap_script: s.zap_script.clone(),
            release_date: s.release_date.clone().unwrap_or_default(),
            manufacturer: s.manufacturer.clone().unwrap_or_default(),
            media_count: s.media_count,
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
        &user_name,
    )
}

/// The rows the favorites catalog shows right now.
fn project_favorites(shared: &Shared) -> Vec<SystemRow> {
    rules::rows_for_favorites(
        &shared.systems_model.favorites,
        &shared.hidden_system_ids,
        shared.show_hidden,
        region(shared),
        &user_name,
    )
}

/// The `[custom.system_names]` override for a system, which wins over
/// the localized and Core names (`docs/customization.md`).
fn user_name(system_id: &str) -> Option<String> {
    crate::customization::system_name(system_id)
}

/// Localized display name for a system id (Hub tiles and titles).
pub fn display_name(shared: &Shared, id: &str) -> String {
    let fallback = shared
        .systems
        .iter()
        .find(|s| s.id == id)
        .map_or_else(|| id.to_string(), |s| s.name.clone());
    let user = user_name(id);
    rules::display_name(id, &fallback, region(shared), user.as_deref())
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
        model.mode = SystemsMode::Category;
        model.loading = false;
        model.category = category.to_string();
        model.rows = rows;
        model.grid.set_item_count(model.rows.len());
        model.grid.set_current_index_immediate(index);
        model.restore_done = true;
        model.sliding = false;
        model.transition_seq += 1;
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

/// The Hub's Favorites action with Group by: System: the systems that
/// hold favorites, from Core's favorites-scoped catalog.
pub fn enter_favorites(ctx: &Ctx, app: &App) {
    enter_favorites_with_direction(ctx, app, 1);
}

/// Back out of a scoped favorites list onto the systems that hold them.
pub fn return_to_favorites(ctx: &Ctx, app: &App) {
    enter_favorites_with_direction(ctx, app, -1);
}

fn enter_favorites_with_direction(ctx: &Ctx, app: &App, direction: i32) {
    crate::navigation::stage(ctx, app);
    {
        let mut shared = lock(&ctx.shared);
        shared.persist.active_screen = "favorite-systems".to_string();
        let model = &mut shared.systems_model;
        model.mode = SystemsMode::Favorites;
        model.loading = model.favorites.is_empty();
        model.focus_armed = false;
        model.restore_done = false;
        model.sliding = false;
        model.transition_seq += 1;
        model.grid.prepare_for_model_replacement();
        let rows = project_favorites(&shared);
        seat_favorites(&mut shared, rows);
    }
    crate::router::save_persist(&ctx.shared);
    let waiting_for_data = lock(&ctx.shared).systems_model.loading;
    let ticket = lock(&ctx.shared).systems_model.transition_seq;
    if waiting_for_data {
        crate::router::begin_pending_with_direction(app, "favorite-systems", direction);
    } else {
        crate::navigation::finish(app);
        let view = app.global::<SystemsView>();
        view.set_error(SharedString::default());
        view.set_loading(false);
        render(ctx, app);
        crate::router::save_persist(&ctx.shared);
        crate::router::transition_to_screen(app, "favorite-systems", direction);
    }

    let resource = ctx.store.subscribe::<SystemsFavoritesEndpoint>(());
    let mut rx = resource.subscribe();
    let weak = app.as_weak();
    let ctx2 = ctx.clone();
    ctx.handle.spawn(async move {
        loop {
            let snapshot = rx.borrow_and_update().clone();
            match snapshot {
                ResourceStatus::Ready(result) => {
                    let _ = weak.upgrade_in_event_loop(move |app| {
                        if lock(&ctx2.shared).systems_model.transition_seq != ticket {
                            return;
                        }
                        apply_favorites(&ctx2, &app, &result, direction, waiting_for_data);
                    });
                    return;
                }
                ResourceStatus::Errored { message, .. } => {
                    let _ = weak.upgrade_in_event_loop(move |app| {
                        if lock(&ctx2.shared).systems_model.transition_seq != ticket {
                            return;
                        }
                        if waiting_for_data && crate::navigation::fail(&ctx2, &app, &message) {
                            return;
                        }
                        {
                            let mut shared = lock(&ctx2.shared);
                            shared.systems_model.loading = false;
                        }
                        let view = app.global::<SystemsView>();
                        view.set_loading(false);
                        view.set_error(SharedString::from(message.as_str()));
                        render(&ctx2, &app);
                        if waiting_for_data {
                            crate::router::transition_to_screen(
                                &app,
                                "favorite-systems",
                                direction,
                            );
                        }
                    });
                    return;
                }
                ResourceStatus::Idle | ResourceStatus::Loading => {}
            }
            if rx.changed().await.is_err() {
                return;
            }
        }
    });
}

/// Core answered the favorites catalog: store it, re-project and seat the
/// persisted system.
pub(crate) fn apply_favorites(
    ctx: &Ctx,
    app: &App,
    result: &SystemsResult,
    direction: i32,
    route_when_ready: bool,
) {
    {
        let mut shared = lock(&ctx.shared);
        shared.systems_model.favorites = catalog_systems(&result.systems);
        shared.systems_model.loading = false;
        let rows = project_favorites(&shared);
        seat_favorites(&mut shared, rows);
    }
    if route_when_ready {
        crate::navigation::finish(app);
    }
    let view = app.global::<SystemsView>();
    view.set_loading(false);
    view.set_error(SharedString::default());
    render(ctx, app);
    if route_when_ready {
        crate::router::save_persist(&ctx.shared);
        crate::router::transition_to_screen(app, "favorite-systems", direction);
    }
}

/// Seat the persisted favorite system on the rows (the restore path and
/// every re-projection share it).
fn seat_favorites(shared: &mut Shared, rows: Vec<SystemRow>) {
    let restore_id = shared.persist.favorite_systems.selected_path.clone();
    let index = rows.iter().position(|s| s.id == restore_id).unwrap_or(0);
    let model = &mut shared.systems_model;
    model.rows = rows;
    model.grid.set_item_count(model.rows.len());
    model.grid.set_current_index_immediate(index);
    model.restore_done = true;
}

/// Re-run the projection after a hide toggle, a Show hidden flip or a
/// region change, keeping the focused system when it survives.
pub fn reproject(ctx: &Ctx, app: &App) {
    {
        let mut shared = lock(&ctx.shared);
        let rows = match shared.systems_model.mode {
            SystemsMode::Favorites => project_favorites(&shared),
            SystemsMode::Category => {
                let category = shared.systems_model.category.clone();
                if category.is_empty() {
                    return;
                }
                project(&shared, &category)
            }
        };
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
    // A user override is served as supplied: no tint ramp, one image
    // for both states (the Qt custom-image provider's own rule).
    if let Some(image) = crate::customization::system_image(&row.id) {
        cell.cover = image.clone();
        cell.has_cover = true;
        cell.cover_focus = image;
        cell.has_cover_focus = true;
        return cell;
    }
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
    insets: Insets,
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
        insets,
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

fn list_layout(shared: &Shared) -> bool {
    shared.persist.settings.systems_browse_layout == "list"
}

/// The list card's geometry from the systems list profile (the TATE
/// table on a rotated scene); the row count follows the row height.
fn list_geometry(app: &App, shared: &Shared) -> list_rules::ListGeometry {
    let inputs = crate::router::output_scene(app).inputs();
    let derived = zaparoo_app::sizing::derive(&inputs);
    let rotated = matches!(shared.persist.settings.orientation.as_str(), "cw" | "ccw");
    let view = if rotated {
        View::SystemsListTate
    } else {
        View::SystemsList
    };
    let profile = layouts::profile(ThemeId::current(&inputs), view, &inputs);
    let Body::List { list, .. } = profile.body else {
        unreachable!("the systems list views resolve to a list body");
    };
    list_rules::list_geometry(
        &list,
        &list_rules::ListFrame {
            screen_width: inputs.screen_width as i32,
            screen_height: inputs.screen_height as i32,
            header_bottom: derived.header_bottom,
            status_top_margin: profile.status.top_margin,
            strip_height: profile.status.strip_height,
            help_bar_height: derived.help_bar_height,
            tier_240: derived.tier == zaparoo_app::sizing::Tier::T240,
            safe_bottom_gap: inputs.pct_h(6.0),
            target_rows: 0,
            min_row_height: inputs.pct_h(3.0),
            default_row_height: inputs.pct_h(6.0),
        },
    )
}

/// The list's own page size: the rows that fit the card.
fn list_visible_rows(app: &App, shared: &Shared) -> usize {
    list_geometry(app, shared).visible_rows.max(1)
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
    if crate::navigation::active() {
        return;
    }
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
    if !model.sliding && view.get_page_slide().abs() > f32::EPSILON {
        view.set_slide_anim(false);
        view.set_page_slide(0.0);
        view.set_cached_transition(false);
    }
    let strip_sliding = model.sliding && !view.get_cached_transition();
    if strip_sliding {
        crate::view_model::publish_cells(
            &view.get_next_cells(),
            page_cells(&shared, page),
            |rows| view.set_next_cells(rows),
        );
    } else {
        crate::view_model::publish_cells(&view.get_cells(), page_cells(&shared, page), |rows| {
            view.set_cells(rows);
        });
        view.set_next_cells(ModelRc::default());
    }
    view.set_selected_local(if strip_sliding {
        -1
    } else {
        i32::try_from(model.grid.current_index().saturating_sub(start)).unwrap_or(0)
    });
    view.set_mode(SharedString::from(model.mode.token()));
    view.set_count(i32::try_from(model.rows.len()).unwrap_or(0));
    view.set_favorites_total(
        rules::favorites_total(&model.rows).map_or(-1, |total| i32::try_from(total).unwrap_or(0)),
    );
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
    publish_press(app, model.activate_pulse, model.release_pulse);
    if let Some(row) = model.current() {
        view.set_label_name(SharedString::from(row.name.as_str()));
        view.set_label_hidden(row.hidden);
        view.set_label_count(
            row.media_count
                .map_or(-1, |c| i32::try_from(c).unwrap_or(0)),
        );
    } else {
        view.set_label_name(SharedString::default());
        view.set_label_hidden(false);
        view.set_label_count(-1);
    }
    render_list(app, &shared);
    drop(shared);
    remember_list_top(&mut lock(&ctx.shared), app);
}

fn saved_list_top(shared: &Shared) -> Option<usize> {
    match shared.systems_model.mode {
        SystemsMode::Category => shared.persist.systems.list_top,
        SystemsMode::Favorites => shared.persist.favorite_systems.list_top,
    }
}

fn remember_list_top(shared: &mut Shared, app: &App) {
    if !list_layout(shared) {
        return;
    }
    let visible = list_visible_rows(app, shared).max(1);
    let model = &shared.systems_model;
    let current = model.grid.current_index();
    let count = model.rows.len();
    let previous = saved_list_top(shared)
        .unwrap_or_else(|| list_rules::list_view_top(current, count, visible, None));
    let top = Some(crate::browse_motion::window_top(
        current, count, visible, previous,
    ));
    match model.mode {
        SystemsMode::Category => shared.persist.systems.list_top = top,
        SystemsMode::Favorites => shared.persist.favorite_systems.list_top = top,
    }
}

/// Bounded row window with overscan for minimal focus-following scroll.
fn render_list(app: &App, shared: &Shared) {
    let view = app.global::<SystemsView>();
    let model = &shared.systems_model;
    let list_geometry = list_geometry(app, shared);
    let visible = list_geometry.visible_rows.max(1);
    let count = model.rows.len();
    let current = model.grid.current_index();
    let paging = list_rules::list_paging(current, count, None, true, visible, false);
    view.set_current_index(i32::try_from(current).unwrap_or(0));
    view.set_list_visible(i32::try_from(visible).unwrap_or(10));
    view.set_list_row_height(list_geometry.row_height as f32);
    view.set_list_page(i32::try_from(paging.current_page).unwrap_or(0));
    view.set_list_total_pages(i32::try_from(paging.total_pages).unwrap_or(1));
    view.set_has_items_above(paging.has_items_above);
    view.set_has_items_below(paging.has_items_below);
    if list_layout(shared) {
        let previous = saved_list_top(shared)
            .unwrap_or_else(|| list_rules::list_view_top(current, count, visible, None));
        let scroll_top = crate::browse_motion::window_top(current, count, visible, previous);
        let top = scroll_top.saturating_sub(1);
        let rows: Vec<GridCell> = model
            .rows
            .iter()
            .skip(top)
            .take(visible + 2)
            .map(|row| cell_for(row, &shared.persist.settings.system_logo_style))
            .collect();
        crate::view_model::publish_cells(&view.get_list_rows(), rows, |rows| {
            view.set_list_rows(rows);
        });
        view.set_list_sel(i32::try_from(current.saturating_sub(top)).unwrap_or(0));
        view.set_list_view_top(i32::try_from(top).unwrap_or(0));
        view.set_list_scroll_top(i32::try_from(scroll_top).unwrap_or(0));
        if let Some(row) = model.current() {
            let cell = cell_for(row, &shared.persist.settings.system_logo_style);
            view.set_detail_title(SharedString::from(row.name.as_str()));
            view.set_detail_has_cover(cell.has_cover);
            view.set_detail_wordmark(!cell.has_cover);
            view.set_detail_cover(if cell.has_cover_focus {
                cell.cover_focus
            } else {
                cell.cover
            });
            let rows: Vec<crate::DetailRow> = row
                .detail_rows()
                .into_iter()
                .map(|(key, value)| crate::DetailRow {
                    key: SharedString::from(key),
                    value: SharedString::from(value.as_str()),
                })
                .collect();
            view.set_detail_rows(ModelRc::new(VecModel::from(rows)));
        } else {
            view.set_detail_title(SharedString::default());
            view.set_detail_has_cover(false);
            view.set_detail_wordmark(false);
            view.set_detail_rows(ModelRc::new(VecModel::from(Vec::<crate::DetailRow>::new())));
        }
    } else {
        view.set_list_rows(ModelRc::new(VecModel::from(Vec::<GridCell>::new())));
    }
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
            match shared.systems_model.mode {
                SystemsMode::Category => shared.persist.systems.system_id = id,
                SystemsMode::Favorites => shared.persist.favorite_systems.selected_path = id,
            }
        }
    }
    crate::router::save_persist(&ctx.shared);
}

#[cfg(feature = "mister")]
fn request_cached_page_transition(app: &App, direction: i32, _columns: i32, _rows: i32) -> bool {
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
        app.global::<SystemsView>().get_grid_y().round().max(0.0) as u32,
        app.global::<SystemsView>()
            .get_grid_height()
            .round()
            .max(0.0) as u32,
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
pub(crate) fn slide_to_current_page(ctx: &Ctx, app: &App, from_page: usize) {
    let rapid = crate::input::rapid_page(ctx);
    let (to_page, columns, rows, reduce_motion) = {
        let mut shared = lock(&ctx.shared);
        let reduce_motion =
            shared.persist.settings.reduce_motion || !app.global::<crate::Motion>().get_enabled();
        let model = &mut shared.systems_model;
        model.sliding = true;
        model.page_seq = model.page_seq.wrapping_add(1);
        let reduce_motion = reduce_motion || std::mem::take(&mut model.cut_next_page);
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
    if reduce_motion || rapid {
        lock(&ctx.shared).systems_model.sliding = false;
        render(ctx, app);
        return;
    }
    let seq = lock(&ctx.shared).systems_model.page_seq;
    if request_cached_page_transition(app, dir, columns, rows) {
        view.set_slide_anim(false);
        view.set_cached_transition(true);
        render(ctx, app);
        let weak = app.as_weak();
        let ctx = ctx.clone();
        slint::Timer::single_shot(std::time::Duration::from_millis(SWOOP_MS), move || {
            if let Some(app) = weak.upgrade() {
                if lock(&ctx.shared).systems_model.page_seq != seq {
                    return;
                }
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
        if lock(&ctx.shared).systems_model.page_seq != seq {
            return;
        }
        lock(&ctx.shared).systems_model.sliding = false;
        let view = app.global::<SystemsView>();
        view.set_slide_anim(false);
        render(&ctx, &app);
        view.set_page_slide(0.0);
        let weak = app.as_weak();
        slint::Timer::single_shot(std::time::Duration::from_millis(REARM_MS), move || {
            if lock(&ctx.shared).systems_model.page_seq != seq {
                return;
            }
            if let Some(app) = weak.upgrade() {
                app.global::<SystemsView>().set_slide_anim(true);
            }
        });
    });
}

/// A directional or page move: returns whether the index changed and the
/// page it started on.
fn move_cursor(ctx: &Ctx, app: &App, action: &str) -> Option<(bool, usize)> {
    let mut shared = lock(&ctx.shared);
    let list_layout = list_layout(&shared);
    let list_page = list_visible_rows(app, &shared) as i64;
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
            actions::PAGE_PREV => -list_page,
            actions::PAGE_NEXT => list_page,
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
    if moved {
        remember_list_top(&mut shared, app);
    }
    Some((moved, from_page))
}

pub(crate) fn interrupt_page(ctx: &Ctx, app: &App) {
    let interrupted = {
        let mut shared = lock(&ctx.shared);
        let model = &mut shared.systems_model;
        let interrupted = model.sliding;
        model.cut_next_page = interrupted;
        if interrupted {
            model.page_seq = model.page_seq.wrapping_add(1);
            model.sliding = false;
        }
        interrupted
    };
    if !interrupted {
        return;
    }
    let view = app.global::<SystemsView>();
    #[cfg(feature = "mister")]
    if view.get_cached_transition() {
        crate::mister::cancel_page_transition();
    }
    view.set_cached_transition(false);
    view.set_slide_anim(false);
    view.set_page_slide(0.0);
    render(ctx, app);
    app.window().request_redraw();
}

/// SystemsScreen.qml's `handleAction`.
pub fn handle_action(ctx: &Ctx, app: &App, action: &str) {
    interrupt_page(ctx, app);
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
        let Some((moved, from_page)) = move_cursor(ctx, app, action) else {
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
        // The favorites list's View menu carries the grouping switch, so
        // it must stay reachable even when the list came back empty.
        actions::PAGE_MENU => {
            if lock(&ctx.shared).systems_model.mode == SystemsMode::Favorites
                && matches!(state(app), "ready" | "empty")
            {
                crate::router::open_favorites_grouping_menu(ctx, app);
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

/// Press feedback changes no row content. Rebuilding logo Images here
/// would replace the page model and destroy the delegates being animated.
pub(crate) fn publish_press(app: &App, activate: i32, release: i32) {
    let view = app.global::<SystemsView>();
    view.set_activate_pulse(activate);
    view.set_release_pulse(release);
}

fn activate_current(ctx: &Ctx, app: &App) {
    let (activate, release) = {
        let mut shared = lock(&ctx.shared);
        if shared.systems_model.current().is_none() {
            return;
        }
        shared.systems_model.activate_pulse += 1;
        (
            shared.systems_model.activate_pulse,
            shared.systems_model.release_pulse,
        )
    };
    persist_selection(ctx);
    publish_press(app, activate, release);
    let ctx2 = ctx.clone();
    let weak = app.as_weak();
    let duration = if app.global::<crate::Motion>().get_enabled() {
        34
    } else {
        0
    };
    slint::Timer::single_shot(std::time::Duration::from_millis(duration), move || {
        let release = {
            let mut shared = lock(&ctx2.shared);
            if shared.systems_model.activate_pulse != activate {
                return;
            }
            shared.systems_model.release_pulse += 1;
            shared.systems_model.release_pulse
        };
        if let Some(app) = weak.upgrade() {
            publish_press(&app, activate, release);
        }
    });
    // The router owns the preceding grid push; list feedback retires locally.
    let (mode, id) = {
        let shared = lock(&ctx.shared);
        (
            shared.systems_model.mode,
            shared.systems_model.current().map(|row| row.id.clone()),
        )
    };
    if mode == SystemsMode::Favorites {
        if let Some(id) = id {
            crate::games::enter_favorites_for_system(ctx, app, &id);
        }
        return;
    }
    let system = id.and_then(|id| catalog_entry(&lock(&ctx.shared), &id));
    if let Some(system) = system {
        if system.zap_script.is_empty() {
            crate::navigation::stage(ctx, app);
        }
        lock(&ctx.shared).persist.games.entered_from_hub = false;
        crate::games::enter(ctx, app, &system);
    }
}

/// Options on the focused system (Main.qml's `systems` owner, or the
/// one-entry `favorite_systems` menu).
fn open_context_menu(ctx: &Ctx, app: &App) {
    let (row, has_launchers, mode) = {
        let shared = lock(&ctx.shared);
        let Some(row) = shared.systems_model.current().cloned() else {
            return;
        };
        let has_launchers = shared.launchers.iter().any(|l| l.system_id == row.id);
        (row, has_launchers, shared.systems_model.mode)
    };
    if mode == SystemsMode::Favorites {
        let (x, y, w, h) = cell_anchor(ctx, app);
        crate::router::set_context_anchor(app, x, y, w, h);
        crate::router::present_systems_context_menu(
            ctx,
            app,
            vec![crate::router::menu_row("launch_random_favorite")],
        );
        return;
    }
    let launchable = row.is_launchable();
    let mut entries = vec![crate::router::menu_row("launch_system")];
    if !launchable {
        entries.push(crate::router::menu_row("launch_random_system"));
    }
    if !launchable && has_launchers {
        entries.push(crate::router::menu_row("change_launcher"));
    }
    entries.push(crate::router::menu_row("add_to_hub"));
    entries.push(crate::router::menu_row_keyed(
        "toggle_hide_system",
        if row.hidden {
            "hide:unhide"
        } else {
            "hide:hide"
        },
        "",
    ));
    if !launchable && !crate::router::media_busy(app) {
        entries.push(crate::router::menu_row("index_system"));
        entries.push(crate::router::menu_row("scrape_system"));
    }
    let (x, y, w, h) = cell_anchor(ctx, app);
    crate::router::set_context_anchor(app, x, y, w, h);
    crate::router::present_systems_context_menu(ctx, app, entries);
}

/// The scene rect of the focused tile or list row (the menu anchor).
fn cell_anchor(ctx: &Ctx, app: &App) -> (f32, f32, f32, f32) {
    let shared = lock(&ctx.shared);
    let model = &shared.systems_model;
    if list_layout(&shared) {
        let g = list_geometry(app, &shared);
        let layout = app.global::<crate::Layout>();
        let top = app.global::<SystemsView>().get_list_scroll_top().max(0) as usize;
        let local = model.grid.current_index().saturating_sub(top) as f32;
        let row_h = g.row_height as f32;
        (
            (g.card_x + g.list_x) as f32 + layout.get_card_padding_left(),
            (g.card_y + g.list_y) as f32
                + layout.get_card_padding_top()
                + local * (row_h + layout.get_row_spacing()),
            g.list_width as f32 - layout.get_card_padding_left() - layout.get_card_padding_right(),
            row_h,
        )
    } else {
        let geometry = geometry(app);
        let rect = paged_grid::cell_rect(
            &geometry.fit,
            &geometry.insets,
            i32::try_from(model.grid.current_row()).unwrap_or(0),
            i32::try_from(model.grid.current_column()).unwrap_or(0),
        );
        (
            rect.x as f32,
            (geometry.grid_y + rect.y) as f32,
            rect.width as f32,
            rect.height as f32,
        )
    }
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
        "launch_random_favorite" => {
            crate::router::launch(
                ctx,
                app,
                rules::random_favorite_launch_text(&row.id),
                &row.name,
            );
        }
        "launch_system" => crate::router::launch(ctx, app, row.launch_text(), &row.name),
        "launch_random_system" => {
            if let Some(text) = rules::random_launch_text(std::slice::from_ref(&row.id)) {
                crate::router::launch(ctx, app, text, &row.name);
            }
        }
        "change_launcher" => crate::launchers::open_system_picker(ctx, app, &row.id),
        "add_to_hub" => crate::hub::add_target(ctx, app, "system", &row.id, "", "", "", "", ""),
        "toggle_hide_system" => crate::router::toggle_hidden_system(ctx, app, &row.id),
        "index_system" => crate::router::start_index(ctx, app, Some(vec![row.id.clone()])),
        "scrape_system" => crate::router::start_scrape(ctx, app, vec![row.id.clone()], false),
        _ => {}
    }
}

fn pointer_select(ctx: &Ctx, app: &App, local: i32) -> bool {
    if crate::press_feedback::pending(app) {
        return false;
    }
    interrupt_page(ctx, app);
    {
        let mut shared = lock(&ctx.shared);
        let model = &mut shared.systems_model;
        if model.sliding {
            return false;
        }
        let Ok(local) = usize::try_from(local) else {
            return false;
        };
        let base = if list_layout(&shared) {
            app.global::<SystemsView>().get_list_view_top().max(0) as usize
        } else {
            let model = &shared.systems_model;
            model.grid.current_page() * model.grid.page_size()
        };
        let model = &mut shared.systems_model;
        let index = base + local;
        if index >= model.rows.len() {
            return false;
        }
        model.focus_armed = true;
        model.grid.set_current_index_immediate(index);
        remember_list_top(&mut shared, app);
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
                    crate::router::handle_action(&ctx, &app, actions::ACCEPT);
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
