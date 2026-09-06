// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// The Hub driver: owns the persisted `[[hub.items]]` layout
// (`zaparoo_core::hub_layout`), resolves it into tiles through
// `zaparoo_app::hub`, drives the cursor through `zaparoo_app::paged_grid`,
// runs Move sessions, and projects the current page into the `HubView`
// global. Mirrors HubScreen.qml plus the Hub half of Main.qml's routing
// and `models/hub_layout.rs`'s session logic.

use std::path::PathBuf;

use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use zaparoo_app::hub::{self as rules, Entry, Kind, LayoutItem, Live, Resolver, Saved};
use zaparoo_app::paged_grid::Grid;
use zaparoo_core::hub_layout::{load_hub_layout, save_hub_layout, HubLayout};
use zaparoo_core::media_types::MediaHistoryLatestEntry;

use crate::media_cache::{MediaCache, MediaKey};
use crate::router::{lock, Ctx, Shared};
use crate::{App, GridCell, HubInput, HubView};
use zaparoo_core::input_actions::actions;

/// Cover key prefix for a media cover the cache already holds; the rest
/// is `system` + `\u{1f}` + `path`.
const MEDIA_PREFIX: &str = "media:";
const LOADING_KEY: &str = "icons/Loading";
/// Cover decode tier for Hub tiles (the Qt build's fixed 256 raster).
const HUB_COVER_TIER: u32 = 256;

/// What Core history says about the resumable game.
#[derive(Debug, Clone, Default)]
pub struct Resume {
    pub requested: bool,
    pub loading: bool,
    pub entry: Option<MediaHistoryLatestEntry>,
}

#[derive(Debug)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "one flag per HubScreen.qml state property"
)]
pub struct HubModel {
    pub layout: HubLayout,
    pub layout_path: PathBuf,
    pub grid: Grid,
    pub entries: Vec<Entry>,
    /// Focus paints once the user has driven it or the restore has run.
    pub focus_armed: bool,
    pub restore_done: bool,
    pub move_snapshot: Option<HubLayout>,
    pub move_origin: Option<usize>,
    pub move_start_flat: usize,
    pub move_armed_total_pages: usize,
    pub resume: Resume,
    pub categories_loaded: bool,
    pub internet_available: bool,
    pub activate_pulse: i32,
    pub release_pulse: i32,
    /// Layout position the open Options menu targets.
    pub menu_hub_index: i32,
    pub menu_kind: Option<Kind>,
}

impl HubModel {
    pub fn new(layout_path: PathBuf) -> Self {
        Self {
            layout: load_hub_layout(&layout_path),
            layout_path,
            grid: Grid::new(5, 2),
            entries: Vec::new(),
            focus_armed: false,
            restore_done: false,
            move_snapshot: None,
            move_origin: None,
            move_start_flat: 0,
            move_armed_total_pages: 0,
            resume: Resume::default(),
            categories_loaded: false,
            internet_available: false,
            activate_pulse: 0,
            release_pulse: 0,
            menu_hub_index: -1,
            menu_kind: None,
        }
    }

    pub fn move_armed(&self) -> bool {
        self.move_snapshot.is_some()
    }

    fn items(&self) -> Vec<LayoutItem> {
        self.layout
            .visible()
            .map(|item| LayoutItem {
                kind: item.kind_raw.clone(),
                id: item.id.clone(),
                path: item.path.clone(),
                script: item.script.clone(),
                name: item.name.clone(),
                icon: item.icon.clone(),
                system: item.system.clone(),
            })
            .collect()
    }

    fn item_count(&self) -> usize {
        self.layout.visible().count()
    }

    fn current(&self) -> Option<&Entry> {
        self.entries.get(self.grid.current_index())
    }

    /// `resumeKnownUnavailable`: Core history confirmed nothing to resume.
    fn resume_known_unavailable(&self, connected: bool) -> bool {
        self.resume.requested && !self.resume.loading && self.resume.entry.is_none() && connected
    }

    fn save(&self) {
        if let Err(e) = save_hub_layout(&self.layout_path, &self.layout) {
            tracing::warn!("could not save hub layout: {e}");
        }
    }
}

/// Name and art lookups over the shared catalog and the media cache.
struct SharedResolver<'a> {
    shared: &'a Shared,
    media: &'a MediaCache,
}

impl Resolver for SharedResolver<'_> {
    fn system_name(&self, id: &str) -> String {
        crate::systems::display_name(self.shared, id)
    }

    fn system_cover_key(&self, id: &str) -> String {
        format!("systems/{id}")
    }

    fn media_cover_key(&self, system: &str, path: &str) -> String {
        let key = MediaKey {
            media_id: None,
            system: system.to_string(),
            path: path.to_string(),
            max_size: HUB_COVER_TIER,
        };
        if self.media.get(&key).is_some() {
            return format!("{MEDIA_PREFIX}{system}\u{1f}{path}");
        }
        self.media.enqueue(key);
        LOADING_KEY.to_string()
    }
}

/// Re-resolve the entries from the layout and the live state, then paint.
pub fn rebuild(ctx: &Ctx, app: &App) {
    {
        let mut shared = lock(&ctx.shared);
        let connected = !app.global::<crate::Status>().get_is_error()
            && app.global::<crate::Shell>().get_boot_complete();
        let shared_ref = &*shared;
        let hub = &shared_ref.hub;
        let resume_name = hub
            .resume
            .entry
            .as_ref()
            .map(|e| e.media_name.clone())
            .unwrap_or_default();
        let resume_cover = hub
            .resume
            .entry
            .as_ref()
            .filter(|e| !e.system_id.is_empty() && !e.media_path.is_empty())
            .map(|e| {
                SharedResolver {
                    shared: shared_ref,
                    media: &ctx.media,
                }
                .media_cover_key(&e.system_id, &e.media_path)
            })
            .map(|key| {
                if key == LOADING_KEY {
                    String::new()
                } else {
                    key
                }
            })
            .unwrap_or_default();
        let live = Live {
            categories_loaded: hub.categories_loaded,
            confirmed_categories: &shared_ref.all_categories,
            resume_enabled: hub.resume.requested,
            resume_name: &resume_name,
            resume_cover_key: &resume_cover,
            resume_known_unavailable: hub.resume_known_unavailable(connected),
            // The Update screen has not been ported; the tile is a
            // structural absence until it lands.
            update_enabled: false,
            internet_available: hub.internet_available,
        };
        let page_size = hub.grid.page_size();
        let min_pages = if hub.move_armed() {
            hub.move_armed_total_pages
        } else {
            0
        };
        let entries = rules::entries(
            &hub.items(),
            hub.layout.is_unseeded(),
            &live,
            &SharedResolver {
                shared: shared_ref,
                media: &ctx.media,
            },
            page_size,
            min_pages,
        );
        let hub = &mut shared.hub;
        hub.grid.set_item_count(entries.len());
        hub.grid
            .set_empty_flags(entries.iter().map(Entry::is_empty).collect());
        hub.grid.skip_empty_cells = !hub.move_armed();
        hub.entries = entries;
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

fn cell_for(ctx: &Ctx, entry: &Entry) -> GridCell {
    let mut cell = GridCell {
        label_key: SharedString::from(entry.label_key.as_str()),
        name: SharedString::from(entry.name.as_str()),
        hidden: false,
        disabled: entry.disabled,
        is_empty: entry.is_empty(),
        ..Default::default()
    };
    if entry.is_empty() {
        return cell;
    }
    let key = entry.cover_key.as_str();
    if let Some(id) = key.strip_prefix("systems/") {
        cell.wordmark = true;
        if let Some(rest) = crate::system_logos::tinted_logo_for(id, false) {
            cell.cover = logo_image(&rest);
            cell.has_cover = true;
            if let Some(focus) = crate::system_logos::tinted_logo_for(id, true) {
                cell.cover_focus = logo_image(&focus);
                cell.has_cover_focus = true;
            }
        }
    } else if let Some(rest) = key.strip_prefix(MEDIA_PREFIX) {
        if let Some((system, path)) = rest.split_once('\u{1f}') {
            let media_key = MediaKey {
                media_id: None,
                system: system.to_string(),
                path: path.to_string(),
                max_size: HUB_COVER_TIER,
            };
            if let Some(decoded) = ctx.media.get(&media_key) {
                cell.cover = slint::Image::from_rgba8(decoded.buffer);
                cell.has_cover = true;
            }
        }
    } else if key != LOADING_KEY && !key.is_empty() {
        cell.glyph_key = SharedString::from(key);
    }
    cell
}

/// Push the current page, the cursor, the caption and the geometry.
pub fn render(ctx: &Ctx, app: &App) {
    let scene = crate::router::output_scene(app);
    let inputs = scene.inputs();
    let derived = zaparoo_app::sizing::derive(&inputs);
    let geometry = rules::geometry(&inputs, &derived);
    let view = app.global::<HubView>();
    let shared = lock(&ctx.shared);
    let hub = &shared.hub;
    let page_size = hub.grid.page_size();
    let page = hub.grid.current_page();
    let start = page * page_size;
    let cells: Vec<GridCell> = hub
        .entries
        .iter()
        .skip(start)
        .take(page_size)
        .map(|entry| cell_for(ctx, entry))
        .collect();
    view.set_cells(ModelRc::new(VecModel::from(cells)));
    view.set_selected_local(i32::try_from(hub.grid.current_index() - start).unwrap_or(0));
    view.set_columns(geometry.columns);
    view.set_rows(geometry.rows);
    view.set_cell_width(geometry.fit.cell_width as f32);
    view.set_cell_height(geometry.fit.cell_height as f32);
    view.set_block_offset_x(geometry.fit.block_offset_x as f32);
    view.set_block_offset_y(geometry.fit.block_offset_y as f32);
    view.set_grid_y(geometry.grid_y as f32);
    view.set_grid_height(geometry.grid_height as f32);
    view.set_label_y(geometry.label_y as f32);
    view.set_label_height(geometry.label_height as f32);
    view.set_page(i32::try_from(page).unwrap_or(0));
    view.set_total_pages(i32::try_from(hub.grid.total_page_count()).unwrap_or(1));
    view.set_has_pages_above(hub.grid.has_pages_above());
    view.set_has_pages_below(hub.grid.has_pages_below());
    view.set_focus_ready(hub.focus_armed || hub.restore_done);
    view.set_move_armed(hub.move_armed());
    view.set_held_local(if hub.move_armed() {
        i32::try_from(hub.grid.current_index() - start).unwrap_or(-1)
    } else {
        -1
    });
    view.set_activate_pulse(hub.activate_pulse);
    view.set_release_pulse(hub.release_pulse);
    view.set_loaded(hub.categories_loaded);
    view.set_catalog_empty(shared.all_categories.is_empty());
    view.set_indexing(app.global::<crate::Status>().get_kind().as_str() == "indexing");
    let error = !view.get_hub_error().is_empty();
    match hub.current() {
        Some(entry) if !entry.is_empty() => {
            view.set_label_key(SharedString::from(entry.label_key.as_str()));
            view.set_label_name(SharedString::from(entry.name.as_str()));
            view.set_label_reason(SharedString::from(if entry.disabled {
                entry.reason.as_str()
            } else {
                ""
            }));
            let is_category = entry.kind == Some(Kind::Category);
            view.set_focused_is_category(is_category);
            view.set_label_visible(!is_category || !error);
            view.set_options_available(entry.hub_index >= 0 && !(is_category && error));
        }
        _ => {
            view.set_label_key(SharedString::default());
            view.set_label_name(SharedString::default());
            view.set_label_reason(SharedString::default());
            view.set_focused_is_category(false);
            view.set_label_visible(true);
            view.set_options_available(false);
        }
    }
}

/// A media cover landed: repaint if a tile shows that game.
pub fn cover_landed(ctx: &Ctx, app: &App, key: &MediaKey) {
    let relevant = {
        let shared = lock(&ctx.shared);
        let hub = &shared.hub;
        let resume_match = hub
            .resume
            .entry
            .as_ref()
            .is_some_and(|e| e.system_id == key.system && e.media_path == key.path);
        resume_match
            || hub.layout.visible().any(|item| {
                item.kind_raw == "zapscript" && item.system == key.system && item.path == key.path
            })
    };
    if relevant {
        rebuild(ctx, app);
    }
}

/// The catalog answered: reconcile the layout (add-only), note the
/// categories, and seat the persisted focus.
pub fn on_catalog_ready(ctx: &Ctx, app: &App) {
    let (ids, migrate_hidden) = {
        let shared = lock(&ctx.shared);
        (
            shared.all_categories.clone(),
            shared.hidden_categories.clone(),
        )
    };
    {
        let mut shared = lock(&ctx.shared);
        let hub = &mut shared.hub;
        hub.categories_loaded = true;
        if hub.layout.reconcile(&ids, &migrate_hidden) {
            hub.save();
        }
    }
    rebuild(ctx, app);
    restore(ctx, app);
}

/// `restoreFromCategoriesReset`: seat focus from persisted state. The
/// Qt cascade into `SystemsModel.set_category` has no counterpart: the
/// Systems screen projects its rows on entry.
pub fn restore(ctx: &Ctx, app: &App) {
    {
        let mut shared = lock(&ctx.shared);
        let connected = app.global::<crate::Shell>().get_boot_complete();
        let ids = shared.all_categories.clone();
        let persisted = shared.persist.hub.clone();
        let hub = &mut shared.hub;
        hub.restore_done = true;
        let resume_visible = !hub.resume_known_unavailable(connected);
        // Update is a structural absence until its screen is ported, so
        // an empty catalog always lands on Settings.
        let fallback = "settings";
        let saved = Saved {
            category: &persisted.category,
            selected_row: persisted.selected_row,
            selected_action: &persisted.selected_action,
            selected_item: &persisted.selected_item,
        };
        let index = rules::restore_index(&hub.entries, &saved, &ids, resume_visible, fallback);
        hub.grid.set_current_index_immediate(index);
    }
    render(ctx, app);
}

/// Persist the focused entry (`_commitCurrent`).
fn commit_current(ctx: &Ctx) {
    let commit = {
        let shared = lock(&ctx.shared);
        shared.hub.current().and_then(rules::commit)
    };
    let Some(commit) = commit else {
        return;
    };
    {
        let mut shared = lock(&ctx.shared);
        let hub_state = &mut shared.persist.hub;
        hub_state.selected_item = commit.selected_item;
        hub_state.selected_row = commit.selected_row;
        if let Some(category) = commit.category {
            hub_state.category = category;
        }
        if let Some(action) = commit.selected_action {
            hub_state.selected_action = action;
        }
    }
    crate::router::save_persist(&ctx.shared);
}

fn set_index(ctx: &Ctx, app: &App, index: usize) {
    lock(&ctx.shared)
        .hub
        .grid
        .set_current_index_immediate(index);
    render(ctx, app);
}

/// HubScreen.qml's `handleAction`.
pub fn handle_action(ctx: &Ctx, app: &App, action: &str) {
    let move_armed = {
        let mut shared = lock(&ctx.shared);
        shared.hub.focus_armed = true;
        shared.hub.move_armed()
    };
    if move_armed {
        handle_move_action(ctx, app, action);
        return;
    }
    let moved = {
        let mut shared = lock(&ctx.shared);
        let grid = &mut shared.hub.grid;
        match action {
            actions::LEFT => Some(grid.move_selection(-1, 0)),
            actions::RIGHT => Some(grid.move_selection(1, 0)),
            actions::DOWN => Some(grid.move_selection(0, 1)),
            actions::UP => Some(grid.move_selection(0, -1)),
            actions::PAGE_PREV => Some(grid.page_by(-1)),
            actions::PAGE_NEXT => Some(grid.page_by(1)),
            _ => None,
        }
    };
    if let Some(moved) = moved {
        if moved {
            commit_current(ctx);
        }
        render(ctx, app);
        return;
    }
    match action {
        actions::ACCEPT => activate_current(ctx, app),
        actions::CONTEXT_MENU => open_context_menu(ctx, app),
        actions::PAGE_MENU => open_page_menu(ctx, app),
        // Cancel is a deliberate no-op on the Hub root; Quit lives in
        // the View menu.
        _ => {}
    }
}

fn activate_current(ctx: &Ctx, app: &App) {
    {
        let mut shared = lock(&ctx.shared);
        let hub = &mut shared.hub;
        if hub.current().is_none_or(Entry::is_empty) {
            return;
        }
        hub.activate_pulse += 1;
    }
    commit_current(ctx);
    render(ctx, app);
    // The press cue plays before the accept lands (DeferredAction.qml).
    let delay = if app.global::<crate::Motion>().get_enabled() {
        34
    } else {
        0
    };
    let ctx = ctx.clone();
    let weak = app.as_weak();
    slint::Timer::single_shot(std::time::Duration::from_millis(delay), move || {
        if let Some(app) = weak.upgrade() {
            emit_activate(&ctx, &app);
        }
    });
}

/// Settle the push-in cue for accepts that keep the Hub on screen.
fn release_activate(ctx: &Ctx, app: &App) {
    lock(&ctx.shared).hub.release_pulse += 1;
    render(ctx, app);
}

fn emit_activate(ctx: &Ctx, app: &App) {
    let entry = {
        let shared = lock(&ctx.shared);
        shared.hub.current().cloned()
    };
    let Some(entry) = entry else {
        return;
    };
    let error = !app.global::<HubView>().get_hub_error().is_empty();
    match entry.kind {
        Some(Kind::Category) => {
            if entry.disabled {
                return;
            }
            if error {
                crate::router::retry_catalog(ctx);
                return;
            }
            crate::systems::enter(ctx, app, &entry.id, true);
        }
        Some(Kind::Action) => {
            if entry.disabled {
                return;
            }
            match entry.id.as_str() {
                "resume" => {
                    let path = lock(&ctx.shared)
                        .hub
                        .resume
                        .entry
                        .as_ref()
                        .map(|e| e.media_path.clone());
                    match path {
                        Some(path) if !path.is_empty() => {
                            crate::router::launch(ctx, app, path);
                            release_activate(ctx, app);
                        }
                        _ => release_activate(ctx, app),
                    }
                }
                "favorites" => crate::games::enter_favorites(ctx, app),
                "recents" => crate::games::enter_recents(ctx, app),
                "settings" => crate::router::enter_settings(ctx, app),
                _ => release_activate(ctx, app),
            }
        }
        Some(Kind::System) => {
            let system = lock(&ctx.shared)
                .systems
                .iter()
                .find(|s| s.id == entry.id)
                .cloned();
            match system {
                Some(system) if !system.zap_script.is_empty() => {
                    crate::router::launch(ctx, app, system.zap_script.clone());
                    release_activate(ctx, app);
                }
                Some(system) => crate::games::enter_from_hub(ctx, app, &system),
                None => release_activate(ctx, app),
            }
        }
        Some(Kind::Folder) => {
            crate::games::enter_folder_from_hub(ctx, app, &entry.system, &entry.path);
        }
        Some(Kind::ZapScript) => {
            crate::router::launch(ctx, app, entry.script.clone());
            release_activate(ctx, app);
        }
        _ => {}
    }
}

// ---------- Move session ----------

fn begin_move(ctx: &Ctx, app: &App, hub_index: i32) {
    {
        let mut shared = lock(&ctx.shared);
        let hub = &mut shared.hub;
        if hub_index < 0 || hub.move_armed() {
            return;
        }
        let Some(current) = hub.current() else {
            return;
        };
        if current.hub_index != hub_index {
            return;
        }
        let Ok(origin) = usize::try_from(hub_index) else {
            return;
        };
        if origin >= hub.item_count() {
            return;
        }
        hub.move_snapshot = Some(hub.layout.clone());
        hub.move_origin = Some(origin);
        hub.move_start_flat = hub.grid.current_index();
        hub.move_armed_total_pages =
            rules::move_armed_total_pages(hub.item_count(), hub.grid.page_size());
    }
    rebuild(ctx, app);
}

/// View > Add item arms Move on what it just placed.
fn arm_move_for_hub_index(ctx: &Ctx, app: &App, hub_index: i32) {
    let flat = {
        let shared = lock(&ctx.shared);
        rules::flat_index_for_hub_index(&shared.hub.entries, hub_index)
    };
    let Some(flat) = flat else {
        return;
    };
    set_index(ctx, app, flat);
    commit_current(ctx);
    begin_move(ctx, app, hub_index);
}

fn clear_move_state(hub: &mut HubModel) {
    hub.move_snapshot = None;
    hub.move_origin = None;
    hub.move_start_flat = 0;
    hub.move_armed_total_pages = 0;
}

fn accept_move(ctx: &Ctx, app: &App) {
    {
        let mut shared = lock(&ctx.shared);
        let hub = &mut shared.hub;
        if hub.move_snapshot.take().is_none() {
            return;
        }
        hub.layout.trim_trailing_blanks();
        hub.save();
        clear_move_state(hub);
    }
    rebuild(ctx, app);
}

fn cancel_move(ctx: &Ctx, app: &App) {
    {
        let mut shared = lock(&ctx.shared);
        let hub = &mut shared.hub;
        let Some(snapshot) = hub.move_snapshot.take() else {
            return;
        };
        hub.layout = snapshot;
        let start = hub.move_start_flat;
        clear_move_state(hub);
        hub.grid.set_current_index_immediate(start);
    }
    rebuild(ctx, app);
    commit_current(ctx);
}

/// Rebuild from the session snapshot with the held tile at `to`.
fn move_held_to(hub: &mut HubModel, to: usize) -> bool {
    let (Some(snapshot), Some(origin)) = (hub.move_snapshot.clone(), hub.move_origin) else {
        return false;
    };
    hub.layout.reseat_held_item(&snapshot, origin, to)
}

fn move_step(ctx: &Ctx, app: &App, d_col: i32, d_row: i32, page_delta: i32) {
    let moved = {
        let mut shared = lock(&ctx.shared);
        let hub = &mut shared.hub;
        let from = hub.grid.current_index();
        if hub.current().is_none_or(|e| e.hub_index < 0) {
            return;
        }
        if page_delta != 0 {
            if rules::would_wrap_page(&hub.grid, page_delta) || !hub.grid.page_by(page_delta) {
                return;
            }
        } else {
            if rules::would_wrap_column(&hub.grid, d_col)
                || rules::would_wrap_vertically(&hub.grid, d_row)
            {
                return;
            }
            if !hub.grid.move_selection(d_col, d_row) {
                return;
            }
        }
        let to =
            rules::real_index_for_flat(&hub.entries, hub.grid.current_index(), hub.item_count());
        match to {
            Some(to) if move_held_to(hub, to) => true,
            _ => {
                hub.grid.set_current_index_immediate(from);
                false
            }
        }
    };
    if moved {
        rebuild(ctx, app);
        commit_current(ctx);
    } else {
        render(ctx, app);
    }
}

fn handle_move_action(ctx: &Ctx, app: &App, action: &str) {
    match action {
        actions::LEFT => move_step(ctx, app, -1, 0, 0),
        actions::RIGHT => move_step(ctx, app, 1, 0, 0),
        actions::DOWN => move_step(ctx, app, 0, 1, 0),
        actions::UP => move_step(ctx, app, 0, -1, 0),
        actions::PAGE_PREV => move_step(ctx, app, 0, 0, -1),
        actions::PAGE_NEXT => move_step(ctx, app, 0, 0, 1),
        actions::ACCEPT => accept_move(ctx, app),
        actions::CANCEL => cancel_move(ctx, app),
        _ => {}
    }
}

// ---------- Menus ----------

/// Options on the focused tile: kind-specific entries first, then the
/// universal Move and Hide (category, action) or Remove (shortcut).
fn open_context_menu(ctx: &Ctx, app: &App) {
    let entry = {
        let shared = lock(&ctx.shared);
        shared.hub.current().cloned()
    };
    let Some(entry) = entry else {
        return;
    };
    let Some(kind) = entry.kind else {
        return;
    };
    if kind == Kind::Empty || entry.hub_index < 0 {
        return;
    }
    let error = !app.global::<HubView>().get_hub_error().is_empty();
    if kind == Kind::Category && error {
        return;
    }
    // "Random game" waits for the shared random-launch helper the
    // Systems and Favorites rows bring.
    let mut entries = Vec::new();
    if kind == Kind::Category {
        let has_indexable = crate::router::category_has_indexable(ctx, &entry.id);
        entries.push(crate::router::menu_entry("hub_move", "Move"));
        entries.push(crate::router::menu_entry("hub_remove", "Hide"));
        if has_indexable && !crate::router::media_busy(app) {
            entries.push(crate::router::menu_entry(
                "index_category",
                "Update media database",
            ));
            entries.push(crate::router::menu_entry(
                "scrape_category",
                "Update metadata",
            ));
        }
    } else {
        entries.push(crate::router::menu_entry("hub_move", "Move"));
        entries.push(crate::router::menu_entry(
            "hub_remove",
            if kind == Kind::Action {
                "Hide"
            } else {
                "Remove"
            },
        ));
    }
    {
        let mut shared = lock(&ctx.shared);
        shared.hub.menu_hub_index = entry.hub_index;
        shared.hub.menu_kind = Some(kind);
    }
    crate::router::present_hub_context_menu(ctx, app, entries);
}

/// Options menu accept for a Hub-owned menu.
pub fn context_accept(ctx: &Ctx, app: &App, id: &str) {
    let (hub_index, category) = {
        let shared = lock(&ctx.shared);
        let hub = &shared.hub;
        let category = rules::flat_index_for_hub_index(&hub.entries, hub.menu_hub_index)
            .and_then(|flat| hub.entries.get(flat))
            .filter(|e| e.kind == Some(Kind::Category))
            .map(|e| e.id.clone());
        (hub.menu_hub_index, category)
    };
    match id {
        "hub_move" => begin_move(ctx, app, hub_index),
        "hub_remove" => {
            if let Ok(index) = usize::try_from(hub_index) {
                let removed = {
                    let mut shared = lock(&ctx.shared);
                    let hub = &mut shared.hub;
                    let removed = hub.layout.remove_visible_item(index);
                    if removed {
                        hub.save();
                    }
                    removed
                };
                if removed {
                    rebuild(ctx, app);
                }
            }
        }
        "index_category" => {
            if let Some(category) = category {
                crate::router::index_category(ctx, app, &category);
            }
        }
        "scrape_category" => {
            if let Some(category) = category {
                crate::router::scrape_category(ctx, &category);
            }
        }
        _ => {}
    }
}

/// West: the page-scoped View menu.
fn open_page_menu(ctx: &Ctx, app: &App) {
    crate::router::present_hub_page_menu(
        ctx,
        app,
        vec![
            crate::router::menu_entry("hub_add", "Add item…"),
            crate::router::menu_entry("hub_reset", "Reset layout"),
            crate::router::menu_entry("hub_settings", "Settings"),
            crate::router::menu_entry("hub_quit", "Quit"),
        ],
    );
}

/// View menu accept.
pub fn page_menu_accept(ctx: &Ctx, app: &App, id: &str) {
    match id {
        "hub_add" => open_add_picker(ctx, app),
        "hub_reset" => {
            let ids = lock(&ctx.shared).all_categories.clone();
            {
                let mut shared = lock(&ctx.shared);
                let hub = &mut shared.hub;
                hub.layout.reset(&ids);
                hub.save();
            }
            rebuild(ctx, app);
        }
        "hub_settings" => crate::router::enter_settings(ctx, app),
        "hub_quit" => crate::router::open_quit_confirm(app),
        _ => {}
    }
}

fn open_add_picker(ctx: &Ctx, app: &App) {
    let entries = {
        let shared = lock(&ctx.shared);
        let hub = &shared.hub;
        let available: Vec<(String, String)> = hub
            .layout
            .available_known()
            .into_iter()
            .filter_map(|key| {
                key.split_once(':')
                    .map(|(k, i)| (k.to_string(), i.to_string()))
            })
            .collect();
        let live = Live {
            categories_loaded: hub.categories_loaded,
            confirmed_categories: &shared.all_categories,
            update_enabled: false,
            ..Live::default()
        };
        rules::add_entries(
            &available,
            &live,
            &SharedResolver {
                shared: &shared,
                media: &ctx.media,
            },
        )
    };
    if entries.is_empty() {
        crate::router::open_action_error(
            app,
            "Nothing left to add",
            "All categories and actions are already on the Hub. To add a game or system, open its Options menu and choose \"Add to Hub\".",
        );
        return;
    }
    let rows: Vec<crate::MenuEntry> = entries
        .iter()
        .map(|e| crate::MenuEntry {
            id: SharedString::from(e.id.as_str()),
            label: SharedString::default(),
            // The picker shows the same words the tile will; the view
            // maps the key.
            label_key: SharedString::from(e.label_key.as_str()),
        })
        .collect();
    crate::router::present_hub_add_picker(ctx, app, rows);
}

/// "Add to Hub" from a browse screen: append a system, folder or
/// `ZapScript` shortcut after the last real tile.
#[allow(
    clippy::too_many_arguments,
    reason = "one argument per HubItem column, mirroring the core API"
)]
pub fn add_target(
    ctx: &Ctx,
    app: &App,
    kind: &str,
    id: &str,
    path: &str,
    script: &str,
    name: &str,
    icon: &str,
    system: &str,
) {
    let added = {
        let mut shared = lock(&ctx.shared);
        let hub = &mut shared.hub;
        let added = hub
            .layout
            .add_target_item(kind, id, path, script, name, icon, system);
        if added {
            hub.save();
        }
        added
    };
    if added {
        rebuild(ctx, app);
    }
}

/// Add picker accept: place the item, then hand it to the user to carry.
pub fn add_picked(ctx: &Ctx, app: &App, id: &str) {
    let Some((kind, item_id)) = id.split_once(':') else {
        return;
    };
    let added = {
        let mut shared = lock(&ctx.shared);
        let target = rules::real_index_for_flat(
            &shared.hub.entries,
            shared.hub.grid.current_index(),
            shared.hub.item_count(),
        )
        .unwrap_or(usize::MAX);
        let hub = &mut shared.hub;
        let added = hub.layout.add_item(kind, item_id, target);
        if added {
            hub.save();
        }
        added.then(|| {
            hub.layout
                .visible()
                .position(|item| item.kind_raw == kind && item.id == item_id)
        })
    };
    let Some(Some(hub_index)) = added else {
        return;
    };
    rebuild(ctx, app);
    arm_move_for_hub_index(ctx, app, i32::try_from(hub_index).unwrap_or(-1));
}

// ---------- Pointer ----------

fn pointer_select(ctx: &Ctx, app: &App, local: i32) -> bool {
    {
        let mut shared = lock(&ctx.shared);
        let hub = &mut shared.hub;
        hub.focus_armed = true;
        let Ok(local) = usize::try_from(local) else {
            return false;
        };
        let index = hub.grid.current_page() * hub.grid.page_size() + local;
        if index >= hub.entries.len() {
            return false;
        }
        if hub.grid.skip_empty_cells && hub.grid.is_empty_at(index) {
            return false;
        }
        hub.grid.set_current_index_immediate(index);
    }
    commit_current(ctx);
    render(ctx, app);
    true
}

/// Wire the Hub's pointer callbacks.
pub fn bind_input(ctx: &std::sync::Arc<Ctx>, app: &App) {
    let input = app.global::<HubInput>();
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
                    if lock(&ctx.shared).hub.move_armed() {
                        accept_move(&ctx, &app);
                    } else {
                        activate_current(&ctx, &app);
                    }
                }
            }
        });
    }
    {
        let ctx = ctx.clone();
        let weak = app.as_weak();
        input.on_cell_right_clicked(move |local| {
            if let Some(app) = weak.upgrade() {
                if pointer_select(&ctx, &app, local) && !lock(&ctx.shared).hub.move_armed() {
                    open_context_menu(&ctx, &app);
                }
            }
        });
    }
    input.on_wheel(page_handler(ctx.clone(), app.as_weak()));
    input.on_page_requested(page_handler(ctx.clone(), app.as_weak()));
}

/// Page cue and mouse wheel: a page turn, or a held tile carried a page.
fn page_handler(ctx: std::sync::Arc<Ctx>, weak: slint::Weak<App>) -> impl Fn(i32) {
    move |delta: i32| {
        let Some(app) = weak.upgrade() else {
            return;
        };
        if lock(&ctx.shared).hub.move_armed() {
            move_step(&ctx, &app, 0, 0, delta);
            return;
        }
        let moved = lock(&ctx.shared).hub.grid.page_by(delta);
        if moved {
            commit_current(&ctx);
        }
        render(&ctx, &app);
    }
}

/// Resume state from Core history (`media.history.latest`).
pub fn set_resume(ctx: &Ctx, app: &App, resume: Resume) {
    lock(&ctx.shared).hub.resume = resume;
    rebuild(ctx, app);
}

pub fn set_internet(ctx: &Ctx, app: &App, available: bool) {
    let changed = {
        let mut shared = lock(&ctx.shared);
        let changed = shared.hub.internet_available != available;
        shared.hub.internet_available = available;
        changed
    };
    if changed {
        rebuild(ctx, app);
    }
}
