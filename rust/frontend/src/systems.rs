// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// The Systems screen driver: projects a category's rows through
// `zaparoo_app::systems`, drives the cursor through `paged_grid`, runs
// the page swoop, owns the Options menu, and paints the current page into
// the `SystemsView` global.

use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel};
use zaparoo_app::layouts::{self, Body, ThemeId, View};
use zaparoo_app::media_list as list_rules;
use zaparoo_app::paged_grid::{self, Grid, Insets};
use zaparoo_app::systems::{self as rules, CatalogSystem, Region, SystemRow};
use zaparoo_app::user_flags::Flag;
use zaparoo_core::endpoints::media_history_top::{MediaHistoryTopEndpoint, TopArgs};
use zaparoo_core::endpoints::systems_user_list::SystemsUserListEndpoint;
use zaparoo_core::input_actions::actions;
use zaparoo_core::media_types::{MediaHistoryTopResult, SystemInfo, SystemsResult};
use zaparoo_core::remote_resource::ResourceStatus;

use crate::navigation::EntryMode;
use crate::router::{lock, Ctx, Shared};
use crate::{App, GridCell, SystemsInput, SystemsView};

/// Which list the screen shows: one category's systems, or the systems
/// that hold favorites (structurally a Systems screen, and grouped with
/// it by the layout profile).
pub use crate::SystemsMode;

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
    /// The list's catalog Core answered with, kept so a hide toggle or
    /// region change can re-project without refetching.
    pub favorites: Vec<CatalogSystem>,
    /// The list `favorites` is the catalog of.
    pub favorites_list: Flag,
    /// The systems of Core's most played games, for Top played grouped by
    /// system, each counting its games in that list.
    pub top: Vec<CatalogSystem>,
    pub loading: bool,
    pub focus_armed: bool,
    pub restore_done: bool,
    pub activate_pulse: i32,
    pub release_pulse: i32,
    /// The page turn in flight (`page_motion`); input waits for it.
    pub page: crate::page_motion::PageMotion,
    pub transition_seq: u64,
    pub fill_task: crate::scoped_task::ScopedTask,
}

impl SystemsModel {
    pub fn new() -> Self {
        Self {
            grid: Grid::new(4, 3),
            rows: Vec::new(),
            mode: SystemsMode::Category,
            category: String::new(),
            favorites: Vec::new(),
            favorites_list: Flag::Favorite,
            top: Vec::new(),
            loading: false,
            focus_armed: false,
            restore_done: false,
            activate_pulse: 0,
            release_pulse: 0,
            page: crate::page_motion::PageMotion::default(),
            transition_seq: 0,
            fill_task: crate::scoped_task::ScopedTask::default(),
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

/// Where `Main_MiSTer` keeps its core display names.
const MISTER_NAMES_PATH: &str = "/media/fat/names.txt";

/// Read `names.txt` into `shared` once; a missing or unreadable file
/// leaves an empty table, so every system keeps its usual name.
pub(crate) fn load_mister_names(shared: &mut Shared) {
    if shared.mister_names.is_some() {
        return;
    }
    let names = match std::fs::read(MISTER_NAMES_PATH) {
        Ok(bytes) => rules::parse_mister_names(&String::from_utf8_lossy(&bytes)),
        Err(err) => {
            tracing::info!("names.txt: {MISTER_NAMES_PATH} not read: {err}");
            std::collections::HashMap::new()
        }
    };
    shared.mister_names = Some(names);
}

/// The name sources the settings select: the region's bundled names, and
/// `names.txt` ahead of them when it is on (`MiSTer` only).
fn name_sources(shared: &Shared) -> rules::NameSources<'_> {
    rules::NameSources {
        region: region(shared),
        mister_names: shared
            .persist
            .settings
            .mister_names
            .then_some(shared.mister_names.as_ref())
            .flatten(),
    }
}

/// The user's logo preferences, read once per render for screens that
/// draw a system's logo from its id.
#[derive(Clone, Copy)]
pub(crate) struct LogoPrefs {
    color: bool,
    region: Region,
}

impl LogoPrefs {
    pub(crate) fn of(shared: &Shared) -> Self {
        Self {
            color: shared.persist.settings.system_logo_style == "color",
            region: region(shared),
        }
    }

    pub(crate) fn key(
        self,
        ctx: &Ctx,
        id: &str,
        bounds: zaparoo_app::logo_cache::Bounds,
    ) -> Option<crate::system_logos::Key> {
        let stem = rules::logo_artwork_stem(id, self.region);
        ctx.logos.key(id, stem, self.color, bounds)
    }

    /// The same preferences with full-color logos, for surfaces that
    /// always draw in color (the screensaver).
    pub(crate) fn in_color(mut self) -> Self {
        self.color = true;
        self
    }
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
        name_sources(shared),
        &user_name,
    )
}

/// The rows the favorites catalog shows right now.
fn project_favorites(shared: &Shared) -> Vec<SystemRow> {
    rules::rows_for_favorites(
        &shared.systems_model.favorites,
        &shared.hidden_system_ids,
        shared.show_hidden,
        name_sources(shared),
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
    rules::display_name(id, &fallback, name_sources(shared), user.as_deref())
}

fn catalog_entry(shared: &Shared, id: &str) -> Option<SystemInfo> {
    shared.systems.iter().find(|s| s.id == id).cloned()
}

/// Enter a category, starting at its first system on a fresh visit and
/// preserving the saved system and viewport on resume.
pub fn enter(ctx: &Ctx, app: &App, category: &str, entry: EntryMode, animate: bool) {
    lock(&ctx.shared).persist.active_screen = "systems".to_string();
    fill(ctx, app, category, entry);
    // The focused tile paints its zoomed logo from the first frame. It is
    // normally warm already; when it is not, prepare that one logo now
    // rather than commit a frame with the resting copy in a grown card.
    let missing = {
        let shared = lock(&ctx.shared);
        focus_art_key(ctx, app, &shared, &geometry(app, &shared))
            .filter(|key| ctx.logos.get(key).is_none() && !ctx.logos.is_negative(key))
    };
    if let Some(key) = missing {
        if ctx.logos.prepare_now(key).is_some() {
            render(ctx, app);
        }
    }
    if animate {
        crate::router::transition_to_screen(app, crate::Screen::Systems, 1);
    } else {
        app.global::<crate::Shell>()
            .set_active_screen(crate::Screen::Systems);
        crate::router::refresh_layout(app);
    }
}

/// Fill the category a restored Games screen sits under, so Back finds
/// it complete, without showing it or naming it the saved screen: on a
/// cold start the Games screen is the first thing shown.
pub fn prepare_parent(ctx: &Ctx, app: &App, category: &str) {
    fill(ctx, app, category, EntryMode::Restore);
}

fn fill(ctx: &Ctx, app: &App, category: &str, entry: EntryMode) {
    {
        let mut shared = lock(&ctx.shared);
        let rows = project(&shared, category);
        if entry == EntryMode::Fresh {
            shared.persist.systems.system_id =
                rows.first().map(|row| row.id.clone()).unwrap_or_default();
            shared.persist.systems.list_top = Some(0);
        }
        let restore_id = shared.persist.systems.system_id.clone();
        let index = rows.iter().position(|s| s.id == restore_id).unwrap_or(0);
        shared.persist.hub.category = category.to_string();
        let model = &mut shared.systems_model;
        model.fill_task.cancel();
        model.mode = SystemsMode::Category;
        model.loading = false;
        model.category = category.to_string();
        model.rows = rows;
        model.grid.set_item_count(model.rows.len());
        model.grid.set_current_index_immediate(index);
        model.restore_done = true;
        model.page.sliding = false;
        model.transition_seq += 1;
    }
    crate::router::save_persist(&ctx.shared);
    let view = app.global::<SystemsView>();
    view.set_category(SharedString::from(category));
    view.set_error(SharedString::default());
    view.set_loading(false);
    render(ctx, app);
}

/// A built-in list's Hub action with Group by: System: the systems that
/// hold its games, from Core's list-scoped catalog.
pub fn enter_list(ctx: &Ctx, app: &App, list: Flag, entry: EntryMode) {
    crate::user_lists::set_active(&mut lock(&ctx.shared), app, list);
    enter_list_with_direction(ctx, app, entry, 1);
}

/// Back out of a scoped list onto the systems that hold its games.
pub fn return_to_list(ctx: &Ctx, app: &App) {
    enter_list_with_direction(ctx, app, EntryMode::Restore, -1);
}

fn enter_list_with_direction(ctx: &Ctx, app: &App, entry: EntryMode, direction: i32) {
    crate::navigation::stage(ctx, app);
    let list = {
        let mut shared = lock(&ctx.shared);
        let list = crate::user_lists::active(&shared);
        if entry == EntryMode::Fresh {
            crate::user_lists::reset_systems(&mut shared.persist, list);
        }
        shared.persist.active_screen = "favorite-systems".to_string();
        let model = &mut shared.systems_model;
        model.fill_task.cancel();
        model.mode = SystemsMode::Favorites;
        if model.favorites_list != list {
            model.favorites.clear();
            model.favorites_list = list;
        }
        model.loading = model.favorites.is_empty();
        model.focus_armed = false;
        model.restore_done = false;
        model.page.sliding = false;
        model.transition_seq += 1;
        model.grid.prepare_for_model_replacement();
        let rows = project_favorites(&shared);
        seat_favorites(&mut shared, rows);
        list
    };
    crate::router::save_persist(&ctx.shared);
    let waiting_for_data = lock(&ctx.shared).systems_model.loading;
    let ticket = lock(&ctx.shared).systems_model.transition_seq;
    if waiting_for_data {
        crate::router::begin_pending_with_direction(app, crate::Screen::FavoriteSystems, direction);
    } else {
        crate::navigation::finish(app);
        let view = app.global::<SystemsView>();
        view.set_error(SharedString::default());
        view.set_loading(false);
        render(ctx, app);
        crate::router::save_persist(&ctx.shared);
        crate::router::transition_to_screen(app, crate::Screen::FavoriteSystems, direction);
    }

    let resource = ctx
        .store
        .subscribe::<SystemsUserListEndpoint>(crate::user_lists::user_list(list));
    let mut rx = resource.subscribe();
    let weak = app.as_weak();
    let ctx2 = ctx.clone();
    let task = ctx.handle.spawn(async move {
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
                                crate::Screen::FavoriteSystems,
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
    lock(&ctx.shared).systems_model.fill_task.replace(task);
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
        crate::router::transition_to_screen(app, crate::Screen::FavoriteSystems, direction);
    }
}

// ---------- Top played, grouped by system ----------

/// The systems Top played's games are in, as rows.
fn project_top(shared: &Shared) -> Vec<SystemRow> {
    rules::rows_for_favorites(
        &shared.systems_model.top,
        &shared.hidden_system_ids,
        shared.show_hidden,
        name_sources(shared),
        &user_name,
    )
}

/// The Top played Hub action with Group by: System.
pub fn enter_top_played(ctx: &Ctx, app: &App, entry: EntryMode) {
    enter_top_played_with_direction(ctx, app, entry, 1);
}

/// Back out of one system's Top played onto its systems.
pub fn return_to_top_played(ctx: &Ctx, app: &App) {
    enter_top_played_with_direction(ctx, app, EntryMode::Restore, -1);
}

fn enter_top_played_with_direction(ctx: &Ctx, app: &App, entry: EntryMode, direction: i32) {
    crate::navigation::stage(ctx, app);
    {
        let mut shared = lock(&ctx.shared);
        if entry == EntryMode::Fresh {
            shared.persist.top_played.system_id.clear();
            shared.persist.top_played.systems_list_top = None;
        }
        shared.persist.active_screen = crate::Screen::TopPlayedSystems.token().to_string();
        let model = &mut shared.systems_model;
        model.fill_task.cancel();
        model.mode = SystemsMode::TopPlayed;
        model.loading = model.top.is_empty();
        model.focus_armed = false;
        model.restore_done = false;
        model.page.sliding = false;
        model.transition_seq += 1;
        model.grid.prepare_for_model_replacement();
        let rows = project_top(&shared);
        seat_top(&mut shared, rows);
    }
    crate::router::save_persist(&ctx.shared);
    let waiting = lock(&ctx.shared).systems_model.loading;
    let ticket = lock(&ctx.shared).systems_model.transition_seq;
    if waiting {
        crate::router::begin_pending_with_direction(
            app,
            crate::Screen::TopPlayedSystems,
            direction,
        );
    } else {
        crate::navigation::finish(app);
        let view = app.global::<SystemsView>();
        view.set_error(SharedString::default());
        view.set_loading(false);
        render(ctx, app);
        crate::router::transition_to_screen(app, crate::Screen::TopPlayedSystems, direction);
    }

    let resource = ctx
        .store
        .subscribe::<MediaHistoryTopEndpoint>(TopArgs::default());
    let mut rx = resource.subscribe();
    let weak = app.as_weak();
    let ctx2 = ctx.clone();
    let task = ctx.handle.spawn(async move {
        loop {
            let snapshot = rx.borrow_and_update().clone();
            match snapshot {
                ResourceStatus::Ready(result) => {
                    let _ = weak.upgrade_in_event_loop(move |app| {
                        if lock(&ctx2.shared).systems_model.transition_seq != ticket {
                            return;
                        }
                        apply_top(&ctx2, &app, &result, direction, waiting);
                    });
                    return;
                }
                ResourceStatus::Errored { message, .. } => {
                    let _ = weak.upgrade_in_event_loop(move |app| {
                        if lock(&ctx2.shared).systems_model.transition_seq != ticket {
                            return;
                        }
                        if waiting && crate::navigation::fail(&ctx2, &app, &message) {
                            return;
                        }
                        lock(&ctx2.shared).systems_model.loading = false;
                        let view = app.global::<SystemsView>();
                        view.set_loading(false);
                        view.set_error(SharedString::from(message.as_str()));
                        render(&ctx2, &app);
                        if waiting {
                            crate::router::transition_to_screen(
                                &app,
                                crate::Screen::TopPlayedSystems,
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
    lock(&ctx.shared).systems_model.fill_task.replace(task);
}

/// Core answered Top played: keep its systems, counted, and seat the saved
/// one. A system the catalog no longer knows keeps Core's name for it.
fn apply_top(
    ctx: &Ctx,
    app: &App,
    result: &MediaHistoryTopResult,
    direction: i32,
    route_when_ready: bool,
) {
    {
        let mut shared = lock(&ctx.shared);
        let ranked = zaparoo_app::top_played::systems_by_rank(
            result.entries.iter().map(|entry| entry.system_id.as_str()),
        );
        let catalog = catalog_systems(&shared.systems);
        shared.systems_model.top = ranked
            .into_iter()
            .map(|(id, count)| {
                let mut system = catalog
                    .iter()
                    .find(|system| system.id == id)
                    .cloned()
                    .unwrap_or_else(|| CatalogSystem {
                        id: id.clone(),
                        name: result
                            .entries
                            .iter()
                            .find(|entry| entry.system_id == id)
                            .map_or_else(|| id.clone(), |entry| entry.system_name.clone()),
                        category: String::new(),
                        zap_script: String::new(),
                        release_date: String::new(),
                        manufacturer: String::new(),
                        media_count: None,
                    });
                system.media_count = Some(count);
                system
            })
            .collect();
        shared.systems_model.loading = false;
        let rows = project_top(&shared);
        seat_top(&mut shared, rows);
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
        crate::router::transition_to_screen(app, crate::Screen::TopPlayedSystems, direction);
    }
}

fn seat_top(shared: &mut Shared, rows: Vec<SystemRow>) {
    let restore_id = shared.persist.top_played.system_id.clone();
    let index = rows.iter().position(|s| s.id == restore_id).unwrap_or(0);
    let model = &mut shared.systems_model;
    model.rows = rows;
    model.grid.set_item_count(model.rows.len());
    model.grid.set_current_index_immediate(index);
    model.restore_done = true;
}

/// Seat the list's persisted system on the rows (the restore path and
/// every re-projection share it).
fn seat_favorites(shared: &mut Shared, rows: Vec<SystemRow>) {
    let list = crate::user_lists::active(shared);
    let restore_id = crate::user_lists::systems_focus(&shared.persist, list).0;
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
    let mut dismiss_context = false;
    {
        let mut shared = lock(&ctx.shared);
        let rows = match shared.systems_model.mode {
            SystemsMode::Favorites => project_favorites(&shared),
            SystemsMode::TopPlayed => project_top(&shared),
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
        if app.global::<crate::Overlays>().get_context_open()
            && shared.context_owner == crate::router::ContextOwner::Systems
        {
            let target = shared.systems_model.rows.get(shared.context_target);
            if let Some(index) =
                target.and_then(|target| rows.iter().position(|s| s.id == target.id))
            {
                shared.context_target = index;
            } else {
                dismiss_context = true;
            }
        }
        let model = &mut shared.systems_model;
        model.rows = rows;
        model.grid.set_item_count(model.rows.len());
        model.grid.set_current_index_immediate(index);
    }
    if dismiss_context {
        crate::router::close_context_menu(ctx, app);
    }
    render(ctx, app);
}

/// One tile: the tinted logo pair (or the original art under the color
/// style). Known logos stay blank while preparing; only missing or failed
/// artwork uses the wordmark fallback.
fn cell_for(
    ctx: &Ctx,
    row: &SystemRow,
    logo_style: &str,
    bounds: zaparoo_app::logo_cache::Bounds,
    art_size: u32,
) -> GridCell {
    let mut cell = text_cell(row);
    // A user override is served as supplied: no tint ramp, one image
    // for both states.
    if let Some(image) = crate::customization::system_image(&row.id, art_size) {
        cell.cover = image.clone();
        cell.has_cover = true;
        cell.cover_focus = image;
        cell.has_cover_focus = true;
        return cell;
    }
    if let Some(key) = logo_key(ctx, row, logo_style, bounds) {
        if let Some(pair) = ctx.logos.get(&key) {
            (cell.cover, cell.cover_focus) = pair.images();
            cell.has_cover = true;
            cell.has_cover_focus = true;
            // Prepared for the box it is painted in.
            cell.cover_exact = true;
        } else {
            cell.wordmark = ctx.logos.is_negative(&key);
        }
    }
    cell
}

fn text_cell(row: &SystemRow) -> GridCell {
    GridCell {
        name: SharedString::from(row.name.as_str()),
        wordmark: true,
        hidden: row.hidden,
        ..Default::default()
    }
}

/// The grid's band and cell fit for the scene, resolved through the
/// browse layout profile.
struct Geometry {
    columns: i32,
    rows: i32,
    grid_y: i32,
    grid_height: i32,
    insets: Insets,
    fit: paged_grid::Fit,
}

impl Geometry {
    fn artwork_size(&self) -> u32 {
        zaparoo_app::customization::artwork_size(
            self.fit.cell_width as f32,
            self.fit.cell_height as f32,
        )
    }
}

fn geometry(app: &App, shared: &Shared) -> Geometry {
    let inputs = crate::router::output_scene(app).inputs();
    let derived = zaparoo_app::sizing::derive(&inputs);
    let profile = layouts::profile(ThemeId::current(&inputs), View::SystemsGrid, &inputs);
    let (grid, footer) = match profile.body {
        Body::Grid { grid, footer } => (grid, footer),
        Body::List { .. } => unreachable!("the systems grid view resolves to a grid body"),
    };
    let grid_y = derived.header_bottom
        + profile.status.top_margin
        + profile.status.strip_height
        + crate::page_bar::systems_row_block(shared.systems_model.mode, &derived);
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

/// Top played grouped by system.
fn top_played(shared: &Shared) -> bool {
    shared.systems_model.mode == SystemsMode::TopPlayed
}

fn favorites(shared: &Shared) -> bool {
    shared.systems_model.mode == SystemsMode::Favorites
}

fn list_layout(shared: &Shared) -> bool {
    matches!(
        shared.persist.settings.systems_browse_layout.as_str(),
        "list" | "list-only"
    )
}

fn list_only_layout(shared: &Shared) -> bool {
    shared.persist.settings.systems_browse_layout == "list-only"
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
            // The Menu chip row sits between the strip and the card.
            strip_height: profile.status.strip_height
                + crate::page_bar::systems_row_block(shared.systems_model.mode, &derived),
            help_bar_height: derived.help_bar_height,
            tier_240: derived.tier == zaparoo_app::sizing::Tier::T240,
            safe_bottom_gap: derived.help_bar_band,
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

/// The focused tile's larger copy, once it is prepared. Until then the
/// tile paints its resting copy at that copy's own size.
fn push_focus_art(
    ctx: &Ctx,
    view: &SystemsView<'_>,
    key: Option<crate::system_logos::Key>,
    selected_local: i32,
) {
    match key.and_then(|key| ctx.logos.get(&key)) {
        Some(pair) if selected_local >= 0 => {
            let (rest, focus) = pair.images();
            view.set_focus_art(focus);
            view.set_focus_art_muted(rest);
            view.set_focus_art_local(selected_local);
        }
        _ => view.set_focus_art_local(-1),
    }
}

fn logo_key(
    ctx: &Ctx,
    row: &SystemRow,
    style: &str,
    bounds: zaparoo_app::logo_cache::Bounds,
) -> Option<crate::system_logos::Key> {
    let stem = row.cover_key.strip_prefix("systems/").unwrap_or(&row.id);
    ctx.logos.key(&row.id, stem, style == "color", bounds)
}

/// What a Systems grid tile gives its art: no caption, no top label, the
/// standard padding.
const TILE_ART: zaparoo_app::sizing::TileArt = zaparoo_app::sizing::TileArt {
    compact_padding: false,
    caption: false,
};

/// The bounds a logo is prepared to. Where logos are painted one pixel for
/// one (`system_logos::exact_art`) that is the whole pixels of the box that
/// paints it, so it is never fitted: a grid tile's art box is solved from
/// the cell, and the list's detail pane reports its own. Until it has, and
/// wherever the renderer fits logos, they keep their bucketed bounds.
fn logo_bounds(app: &App, shared: &Shared, geometry: &Geometry) -> zaparoo_app::logo_cache::Bounds {
    if list_layout(shared) {
        let view = app.global::<SystemsView>();
        let (width, height) = (view.get_detail_art_width(), view.get_detail_art_height());
        if crate::system_logos::exact_art(app) && width > 0 && height > 0 {
            return zaparoo_app::logo_cache::Bounds::exact(width as u32, height as u32);
        }
        let size = crate::sizing::detail_cover_source_size(crate::router::output_scene(app));
        zaparoo_app::logo_cache::Bounds::new(size, size)
    } else if !crate::system_logos::exact_art(app) {
        crate::system_logos::cell_bounds(app, geometry.fit.cell_width, geometry.fit.cell_height)
    } else {
        crate::system_logos::tile_bounds(
            app,
            geometry.fit.cell_width,
            geometry.fit.cell_height,
            1.0,
            TILE_ART,
        )
    }
}

/// The focused system's logo at the art box of its grown tile: the larger
/// copy the view paints for the focused tile where it draws the focus zoom
/// by size. Only ever the one tile, so the cache holds one extra logo.
fn focus_art_key(
    ctx: &Ctx,
    app: &App,
    shared: &Shared,
    geometry: &Geometry,
) -> Option<crate::system_logos::Key> {
    logo_key(
        ctx,
        shared.systems_model.current()?,
        &shared.persist.settings.system_logo_style,
        focus_bounds(app, shared, geometry)?,
    )
}

/// The bounds of the focused tile's larger copy of its logo, where the
/// grid paints one.
fn focus_bounds(
    app: &App,
    shared: &Shared,
    geometry: &Geometry,
) -> Option<zaparoo_app::logo_cache::Bounds> {
    if list_layout(shared) {
        return None;
    }
    let zoom = crate::system_logos::focus_zoom(app)?;
    Some(crate::system_logos::tile_bounds(
        app,
        geometry.fit.cell_width,
        geometry.fit.cell_height,
        zoom,
        TILE_ART,
    ))
}

/// Tell the logo cache which logos the Systems grid can ever show, so it
/// prepares them in the background: every system of every category, at the
/// grid's tile size and the chosen style. The list layout shows one logo at
/// a time in its detail pane and warms nothing.
pub(crate) fn warm_logos(ctx: &Ctx, app: &App) {
    thread_local! {
        // What the last list was built from. Renders call this on every
        // move, and the list only changes with these.
        static BUILT_FROM: std::cell::RefCell<String> = const { std::cell::RefCell::new(String::new()) };
    }
    let keys = {
        let shared = lock(&ctx.shared);
        let list = list_layout(&shared);
        let geometry = geometry(app, &shared);
        let bounds = logo_bounds(app, &shared, &geometry);
        let focus = focus_bounds(app, &shared, &geometry);
        let style = &shared.persist.settings.system_logo_style;
        let built_from = format!(
            "{style}|{list}|{bounds:?}|{focus:?}|{:?}|{}|{}|{}|{}|{:?}",
            region(&shared),
            // names.txt renames re-sort a category, changing its first tile.
            shared.persist.settings.mister_names,
            shared.categories.len(),
            shared.systems.len(),
            shared.show_hidden,
            shared.hidden_system_ids,
        );
        if BUILT_FROM.with(|last| last.replace(built_from.clone()) == built_from) {
            return;
        }
        if list {
            Vec::new()
        } else {
            // First, the larger copy for the tile each category opens on,
            // so entering a category paints its focused tile zoomed from
            // the first frame. Then every logo at grid size.
            let projected: Vec<Vec<SystemRow>> = shared
                .categories
                .iter()
                .map(|category| project(&shared, category))
                .collect();
            let entry_focus = focus.into_iter().flat_map(|focus| {
                projected
                    .iter()
                    .filter_map(move |rows| rows.first())
                    .filter_map(move |row| logo_key(ctx, row, style, focus))
            });
            let mut seen = std::collections::HashSet::new();
            let grid = projected
                .iter()
                .flatten()
                .filter(|row| seen.insert(row.id.clone()))
                .filter_map(|row| logo_key(ctx, row, style, bounds));
            entry_focus.chain(grid).collect()
        }
    };
    ctx.logos.set_warm(keys);
}

fn request_logos(ctx: &Ctx, app: &App, shared: &Shared, bounds: zaparoo_app::logo_cache::Bounds) {
    if !matches!(
        app.global::<crate::Shell>().get_active_screen(),
        crate::Screen::Systems | crate::Screen::FavoriteSystems | crate::Screen::TopPlayedSystems
    ) {
        crate::system_logos::defer_refresh(ctx, app);
        return;
    }
    let model = &shared.systems_model;
    let (start, size) = if list_layout(shared) {
        // List rows contain text only; prepare the detail cover and its neighbors.
        (model.grid.current_index(), 1)
    } else {
        (
            model.grid.current_page() * model.grid.page_size(),
            model.grid.page_size(),
        )
    };
    let style = &shared.persist.settings.system_logo_style;
    let visible = model
        .current()
        .into_iter()
        .chain(model.rows.iter().skip(start).take(size))
        .filter_map(|row| logo_key(ctx, row, style, bounds))
        .chain(focus_art_key(ctx, app, shared, &geometry(app, shared)));
    let neighbors = [start.saturating_add(size), start.saturating_sub(size)]
        .into_iter()
        .flat_map(|first| model.rows.iter().skip(first).take(size))
        .filter_map(|row| logo_key(ctx, row, style, bounds));
    ctx.logos.request_window(visible, neighbors);
}

fn page_cells(
    ctx: &Ctx,
    shared: &Shared,
    page: usize,
    bounds: zaparoo_app::logo_cache::Bounds,
    art_size: u32,
) -> Vec<GridCell> {
    if list_layout(shared) {
        return Vec::new();
    }
    let model = &shared.systems_model;
    let page_size = model.grid.page_size();
    model
        .rows
        .iter()
        .skip(page * page_size)
        .take(page_size)
        .map(|row| {
            cell_for(
                ctx,
                row,
                &shared.persist.settings.system_logo_style,
                bounds,
                art_size,
            )
        })
        .collect()
}

/// Paint the current page, the cursor, the caption and the geometry.
pub fn render(ctx: &Ctx, app: &App) {
    // The grid's tile size or logo style may have changed since the list
    // of logos to keep ready was built.
    warm_logos(ctx, app);
    render_with_page(ctx, app, false);
    // A pending route keeps the source's row, as it keeps its page.
    if !crate::navigation::active() {
        let shared = lock(&ctx.shared);
        crate::page_bar::publish_systems(app, &shared, shared.systems_model.mode);
    }
}

fn render_with_page(ctx: &Ctx, app: &App, reuse_page: bool) {
    if crate::navigation::active() {
        return;
    }
    let geometry = geometry(app, &lock(&ctx.shared));
    let art_size = geometry.artwork_size();
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
    let bounds = logo_bounds(app, &shared, &geometry);
    request_logos(ctx, app, &shared, bounds);
    let model = &shared.systems_model;
    let page = model.grid.current_page();
    let start = page * model.grid.page_size();
    if !model.page.sliding && view.get_page_slide().abs() > f32::EPSILON {
        view.set_slide_anim(false);
        view.set_page_slide(0.0);
        view.set_cached_transition(false);
    }
    let strip_sliding = model.page.sliding && !view.get_cached_transition();
    if strip_sliding {
        crate::view_model::publish_cells(
            &view.get_next_cells(),
            page_cells(ctx, &shared, page, bounds, art_size),
            |rows| view.set_next_cells(rows),
        );
    } else {
        // Cursor moves reuse the page. Worker completions and theme/size
        // changes explicitly refresh it; shared prepared pixels keep image
        // identity stable even through those full publications.
        let page_len = model
            .rows
            .len()
            .saturating_sub(start)
            .min(model.grid.page_size());
        if !reuse_page
            || shape_changed
            || view.get_page() != i32::try_from(page).unwrap_or(0)
            || view.get_cells().row_count() != page_len
        {
            crate::view_model::publish_cells(
                &view.get_cells(),
                page_cells(ctx, &shared, page, bounds, art_size),
                |rows| {
                    view.set_cells(rows);
                },
            );
        }
        view.set_next_cells(ModelRc::default());
    }
    let local = model.grid.current_index().saturating_sub(start);
    let selected_local = i32::try_from(local).map_or(0, |i| if strip_sliding { -1 } else { i });
    view.set_selected_local(selected_local);
    let focus_art = focus_art_key(ctx, app, &shared, &geometry);
    push_focus_art(ctx, &view, focus_art, selected_local);
    view.set_mode(model.mode);
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
    publish_label(&view, &shared);
    render_list(ctx, app, &shared, bounds);
    drop(shared);
    remember_list_top(&mut lock(&ctx.shared), app);
}

/// The focused tile's footer: its name, and what the screen notes of it.
fn publish_label(view: &SystemsView<'_>, shared: &Shared) {
    if let Some(row) = shared.systems_model.current() {
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
}

fn saved_list_top(shared: &Shared) -> Option<usize> {
    match shared.systems_model.mode {
        SystemsMode::Category => shared.persist.systems.list_top,
        SystemsMode::TopPlayed => shared.persist.top_played.systems_list_top,
        SystemsMode::Favorites => {
            crate::user_lists::systems_focus(&shared.persist, crate::user_lists::active(shared)).1
        }
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
        SystemsMode::TopPlayed => shared.persist.top_played.systems_list_top = top,
        SystemsMode::Favorites => {
            let list = crate::user_lists::active(shared);
            crate::user_lists::set_systems_top(&mut shared.persist, list, top);
        }
    }
}

/// Bounded row window with overscan for minimal focus-following scroll.
fn render_list(ctx: &Ctx, app: &App, shared: &Shared, bounds: zaparoo_app::logo_cache::Bounds) {
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
        crate::browse_motion::publish_list(
            &view,
            &shared.input,
            current,
            count,
            visible,
            previous,
            &model.rows,
            text_cell,
        );
        if let Some(row) = model.current() {
            if list_only_layout(shared) {
                view.set_detail_title(SharedString::default());
                view.set_detail_has_cover(false);
                view.set_detail_wordmark(false);
                view.set_detail_rows(ModelRc::new(VecModel::from(Vec::<crate::DetailRow>::new())));
            } else {
                let cell = cell_for(
                    ctx,
                    row,
                    &shared.persist.settings.system_logo_style,
                    bounds,
                    crate::sizing::detail_cover_source_size(crate::router::output_scene(app)),
                );
                view.set_detail_title(SharedString::from(row.name.as_str()));
                view.set_detail_has_cover(cell.has_cover);
                view.set_detail_cover_exact(cell.cover_exact);
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
            }
        }
    } else {
        view.set_list_rows(ModelRc::new(VecModel::from(Vec::<GridCell>::new())));
    }
}

/// Screen state: loading, error, empty or ready.
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

pub(crate) fn persist_selection(ctx: &Ctx) {
    {
        let mut shared = lock(&ctx.shared);
        let id = shared.systems_model.current().map(|s| s.id.clone());
        if let Some(id) = id {
            match shared.systems_model.mode {
                SystemsMode::Category => shared.persist.systems.system_id = id,
                SystemsMode::TopPlayed => shared.persist.top_played.system_id = id,
                SystemsMode::Favorites => {
                    let list = crate::user_lists::active(&shared);
                    crate::user_lists::set_systems_path(&mut shared.persist, list, id);
                }
            }
        }
    }
    crate::router::save_persist(&ctx.shared);
}

/// The systems grid on the shared page turn.
pub(crate) struct SystemsScreen;

impl crate::page_motion::PagedScreen for SystemsScreen {
    type View<'a> = SystemsView<'a>;
    fn view(app: &App) -> SystemsView<'_> {
        app.global::<SystemsView>()
    }
    fn motion(shared: &mut Shared) -> &mut crate::page_motion::PageMotion {
        &mut shared.systems_model.page
    }
    fn render(ctx: &Ctx, app: &App) {
        render(ctx, app);
    }
    fn on_screen(app: &App) -> bool {
        matches!(
            app.global::<crate::Shell>().get_active_screen(),
            crate::Screen::Systems
                | crate::Screen::FavoriteSystems
                | crate::Screen::TopPlayedSystems
        )
    }
    fn list_layout(app: &App) -> bool {
        app.global::<crate::Shell>().get_systems_list_layout()
    }
}

/// Turn to the grid's current page in its committed direction, including
/// wrapped turns.
fn slide_to_current_page(ctx: &Ctx, app: &App) {
    let (direction, local) = {
        let shared = lock(&ctx.shared);
        let grid = &shared.systems_model.grid;
        (
            grid.page_direction().offset(),
            grid.current_index() - grid.current_page() * grid.page_size(),
        )
    };
    crate::page_motion::turn::<SystemsScreen>(ctx, app, direction, local, true);
}

/// A directional or page move: returns whether the index changed and the
/// page it started on.
fn move_cursor(ctx: &Ctx, app: &App, action: &str) -> Option<(bool, usize)> {
    let mut shared = lock(&ctx.shared);
    let list_layout = list_layout(&shared);
    let list_page = list_visible_rows(app, &shared) as i64;
    let model = &mut shared.systems_model;
    if model.page.sliding {
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
    crate::page_motion::interrupt::<SystemsScreen>(ctx, app);
}

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
            let list_layout = list_layout(&lock(&ctx.shared));
            let to_page = lock(&ctx.shared).systems_model.grid.current_page();
            if !list_layout && to_page != from_page {
                slide_to_current_page(ctx, app);
                return;
            }
        }
        // Only same-page grid moves reuse the existing image-bearing cells.
        // A page change is handled by slide_to_current_page above.
        let reuse_page = {
            let shared = lock(&ctx.shared);
            !list_layout(&shared)
        };
        render_with_page(ctx, app, reuse_page);
        return;
    }
    match action {
        actions::ACCEPT => match state(app) {
            "loading" => {}
            "error" | "empty" if top_played(&lock(&ctx.shared)) => {
                enter_top_played(ctx, app, EntryMode::Restore);
            }
            "error" | "empty" => crate::router::retry_catalog(ctx),
            _ => activate_current(ctx, app),
        },
        actions::CONTEXT_MENU => {
            if state(app) == "ready" {
                persist_selection(ctx);
                open_context_menu(ctx, app);
            }
        }
        // Above 240p the Menu is the chip row. The favorites list's Menu
        // carries the grouping switch, so it stays reachable even when the
        // list came back empty. At 240p a category's one action, Search
        // here, runs straight away.
        actions::PAGE_MENU => {
            let favorites = favorites(&lock(&ctx.shared));
            let top = top_played(&lock(&ctx.shared));
            let reachable = match state(app) {
                "ready" => true,
                // The grouping switch stays reachable on an empty list.
                "empty" => favorites || top,
                _ => false,
            };
            if !reachable || crate::page_bar::enter(ctx, app) {
                return;
            }
            if top {
                crate::router::open_top_played_page_menu(ctx, app);
            } else if favorites {
                crate::router::open_favorite_systems_page_menu(ctx, app);
            } else {
                crate::search::enter_scoped(ctx, app);
            }
        }
        actions::CANCEL => {
            lock(&ctx.shared).persist.active_screen = "hub".to_string();
            crate::router::save_persist(&ctx.shared);
            crate::router::return_to_hub(ctx, app);
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
    if lock(&ctx.shared).systems_model.current().is_none() {
        return;
    }
    persist_selection(ctx);
    if app.global::<crate::Shell>().get_systems_list_layout() {
        pulse_list_row(ctx, app);
    }
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
            crate::games::enter_list_for_system(ctx, app, &id);
        }
        return;
    }
    if mode == SystemsMode::TopPlayed {
        if let Some(id) = id {
            crate::games::enter_top_played_for_system(ctx, app, &id);
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

fn pulse_list_row(ctx: &Ctx, app: &App) {
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
}

/// What `row` can do, for its Options menu. Shared with the Hub, whose
/// pinned system tiles show the same menu with their shortcut's actions.
pub(crate) fn menu_input(
    app: &App,
    shared: &Shared,
    row: &SystemRow,
    favorites: bool,
    tile: bool,
) -> zaparoo_app::options_menu::SystemInput {
    zaparoo_app::options_menu::SystemInput {
        launch_only: row.is_launchable(),
        favorites,
        has_launchers: shared.launchers.iter().any(|l| l.system_id == row.id),
        on_hub: crate::hub::has_target(shared, "system", &row.id, "", "", "", ""),
        is_hidden: row.hidden,
        media_busy: crate::router::media_busy(app),
        tile,
    }
}

/// The system `id` as a row, for a surface that is not listing systems.
/// `None` when the catalog does not have it.
pub(crate) fn row_for(shared: &Shared, id: &str) -> Option<SystemRow> {
    let info = catalog_entry(shared, id)?;
    Some(SystemRow {
        id: info.id.clone(),
        name: display_name(shared, id),
        category: info.category.clone(),
        hidden: shared.hidden_system_ids.iter().any(|hidden| hidden == id),
        zap_script: info.zap_script,
        ..SystemRow::default()
    })
}

/// Options on the focused system. Favorite systems shows the same system
/// the same way; only random play stays inside its favorites.
fn open_context_menu(ctx: &Ctx, app: &App) {
    let (menu, index) = {
        let shared = lock(&ctx.shared);
        let model = &shared.systems_model;
        let Some(row) = model.current() else {
            return;
        };
        let favorites = model.mode == SystemsMode::Favorites;
        let menu =
            zaparoo_app::options_menu::system(&menu_input(app, &shared, row, favorites, false));
        (menu, model.grid.current_index())
    };
    let anchor = cell_anchor(ctx, app);
    crate::router::set_context_anchor(app, &anchor);
    crate::router::present_systems_context_menu(ctx, app, index, menu);
}

/// Where the menu sits when Back brings it back: the focused cell now.
pub(crate) fn context_anchor(ctx: &Ctx, app: &App) -> crate::router::ContextAnchor {
    cell_anchor(ctx, app)
}

/// The scene rect of the focused tile or list row (the menu anchor).
fn cell_anchor(ctx: &Ctx, app: &App) -> crate::router::ContextAnchor {
    let shared = lock(&ctx.shared);
    let model = &shared.systems_model;
    if list_layout(&shared) {
        let g = list_geometry(app, &shared);
        let layout = app.global::<crate::Layout>();
        let top = app.global::<SystemsView>().get_list_scroll_top().max(0) as usize;
        let local = model.grid.current_index().saturating_sub(top) as f32;
        let row_h = g.row_height as f32;
        crate::router::ContextAnchor {
            x: (g.card_x + g.list_x) as f32 + layout.get_card_padding_left(),
            y: (g.card_y + g.list_y) as f32
                + layout.get_card_padding_top()
                + local * (row_h + layout.get_row_spacing()),
            w: g.list_width as f32
                - layout.get_card_padding_left()
                - layout.get_card_padding_right(),
            h: row_h,
            // A row's silhouette is its selection fill, not a card.
            radius: layout.get_row_radius(),
            zoomed: false,
        }
    } else {
        let geometry = geometry(app, &shared);
        let rect = paged_grid::cell_rect(
            &geometry.fit,
            &geometry.insets,
            i32::try_from(model.grid.current_row()).unwrap_or(0),
            i32::try_from(model.grid.current_column()).unwrap_or(0),
        );
        crate::router::ContextAnchor {
            x: rect.x as f32,
            y: (geometry.grid_y + rect.y) as f32,
            w: rect.width as f32,
            h: rect.height as f32,
            radius: app.global::<crate::Layout>().get_card_radius(),
            zoomed: true,
        }
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
    if id == "add_to_hub" {
        crate::hub::toggle_target(
            ctx,
            app,
            "system",
            &row.id,
            "",
            "",
            "",
            row.hub_name(),
            "",
            "",
        );
        return;
    }
    system_action(ctx, app, id, &row);
}

/// A system's own actions, whichever screen it was reached from.
pub(crate) fn system_action(ctx: &Ctx, app: &App, id: &str, row: &SystemRow) {
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
        "artwork" => crate::artwork::open_picker(ctx, app, &row.id, ""),
        "toggle_hide_system" => crate::router::toggle_hidden_system(ctx, app, &row.id),
        "index_system" => crate::router::start_index(ctx, app, Some(vec![row.id.clone()])),
        "scrape_system" => crate::router::open_scrape_setup(
            ctx,
            app,
            zaparoo_app::media_setup::Scope::System(row.id.clone()),
        ),
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
        if model.page.sliding {
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
        input.on_detail_art_box(move |width, height| {
            let Some(app) = weak.upgrade() else { return };
            let view = app.global::<SystemsView>();
            if view.get_detail_art_width() == width && view.get_detail_art_height() == height {
                return;
            }
            view.set_detail_art_width(width);
            view.set_detail_art_height(height);
            // The detail logo is keyed by its box: ask again at this one.
            // A turn later, because the view reports its box while it is
            // being laid out, which can be inside a driver's own call.
            let ctx = ctx.clone();
            let weak = weak.clone();
            slint::Timer::single_shot(std::time::Duration::ZERO, move || {
                if let Some(app) = weak.upgrade() {
                    render(&ctx, &app);
                }
            });
        });
    }
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
