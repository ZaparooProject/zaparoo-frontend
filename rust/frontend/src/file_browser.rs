// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// The file browser driver: one folder of the frontend's own disk at a time,
// as a list (`Mode::Plain`), as a grid of image previews (`Mode::Images`),
// or as a list of folders with a row that picks the folder itself
// (`Mode::Folders`), with Places above every folder. A caller opens it with
// a `Purpose`, which decides what a pick does; the browser itself only
// walks folders. Folder reads run off the event loop and land under a
// ticket, and the folder on screen stays until the next one is ready.
// Paging, the list window, the fast-scroll rail, the selection debounce,
// the press cue and the cover fade are the games screens' own
// (`page_motion`, `browse_motion`, `press_feedback`, `cover_fade`), so the
// browser moves exactly as they do. Rules live in `zaparoo_app::file_browser`.

use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use zaparoo_app::file_browser::{self as rules, EntryKind, Mode, Place, PlaceKind, Platform, Up};
use zaparoo_app::media_list::{self as list_rules, SelectionPersist};
use zaparoo_app::paged_grid::Grid;
use zaparoo_core::input_actions::actions;
use zaparoo_core::persist::{FileBrowserState, HubTarget};

use crate::browse_motion::Rapid;
use crate::page_motion::PageMotion;
use crate::router::{lock, Ctx, Shared};
use crate::{App, ContentState, FileBrowserInput, FileBrowserView, GridCell};

/// What a pick is for. A new caller adds a variant here, not a browser.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Purpose {
    /// Change icon… on a Hub tile.
    HubIcon(HubTarget),
    /// Add item > Custom folder…: a live folder tile, placed on the Hub
    /// cell `target.hub_index`.
    HubFolder(HubTarget),
}

impl Purpose {
    /// The purpose a browser in `mode` was opened for: the mode is what
    /// the persisted state records.
    fn for_mode(mode: Mode, target: HubTarget) -> Self {
        match mode {
            Mode::Folders => Self::HubFolder(target),
            Mode::Plain | Mode::Images => Self::HubIcon(target),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum RowKind {
    Place(PlaceKind),
    /// "Add this folder": picks the folder on screen.
    PickFolder,
    Folder,
    File,
    /// The cap left this many entries out.
    More(usize),
}

#[derive(Debug, Clone)]
struct Row {
    kind: RowKind,
    name: String,
    /// The absolute path; empty for the row that only reports.
    path: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Fill {
    #[default]
    Ready,
    Empty,
    Error,
}

#[derive(Debug)]
pub struct Model {
    purpose: Option<Purpose>,
    mode: Mode,
    places: Vec<Place>,
    /// The folder on screen; `None` for Places.
    dir: Option<String>,
    rows: Vec<Row>,
    grid: Grid,
    list_top: usize,
    fill: Fill,
    /// The folder's letter buckets, for the rail and a long hold's steps.
    letters: rules::Letters,
    page: PageMotion,
    rapid: Rapid,
    persist: SelectionPersist,
    persist_seq: u64,
    press_seq: u64,
    activate_pulse: i32,
    release_pulse: i32,
    ticket: u64,
    task: crate::scoped_task::ScopedTask,
}

impl Default for Model {
    fn default() -> Self {
        Self {
            purpose: None,
            mode: Mode::default(),
            places: Vec::new(),
            dir: None,
            rows: Vec::new(),
            grid: Grid::new(1, 1),
            list_top: 0,
            fill: Fill::default(),
            letters: rules::Letters::default(),
            page: PageMotion::default(),
            rapid: Rapid::default(),
            persist: SelectionPersist::new(),
            persist_seq: 0,
            press_seq: 0,
            activate_pulse: 0,
            release_pulse: 0,
            ticket: 0,
            task: crate::scoped_task::ScopedTask::default(),
        }
    }
}

/// What a folder read hands back to the event loop.
struct Landing {
    places: Vec<Place>,
    dir: Option<String>,
    rows: Vec<Row>,
    letters: rules::Letters,
    fill: Fill,
}

fn platform(ctx: &Ctx) -> Platform {
    if ctx.is_mister {
        Platform::Mister
    } else if cfg!(target_os = "macos") {
        Platform::MacOs
    } else {
        Platform::Linux
    }
}

fn is_dir(path: &str) -> bool {
    std::fs::metadata(path).is_ok_and(|meta| meta.is_dir())
}

fn list_names(dir: &str) -> Vec<String> {
    std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .filter(|entry| entry.path().is_dir())
                .filter_map(|entry| entry.file_name().into_string().ok())
                .collect()
        })
        .unwrap_or_default()
}

/// The Places this machine has. Blocking: every candidate is a stat.
fn read_places(platform: Platform, custom_dir: &str) -> Vec<Place> {
    let home = std::env::var("HOME").ok();
    let user = std::env::var("USER").ok();
    rules::places(
        platform,
        custom_dir,
        home.as_deref(),
        user.as_deref(),
        &is_dir,
        &list_names,
    )
}

/// The most names one read collects before the rest are only counted.
const READ_LIMIT: usize = 20_000;

/// One folder's rows. Blocking. Names that are not UTF-8 are skipped: a
/// path made from a lossy name would not open.
fn read_folder(dir: &str, mode: Mode) -> Result<(Vec<Row>, rules::Letters, Fill), std::io::Error> {
    let mut raw = Vec::new();
    let mut unread = 0usize;
    for entry in std::fs::read_dir(dir)? {
        let Ok(entry) = entry else {
            continue;
        };
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        if raw.len() >= READ_LIMIT {
            unread += 1;
            continue;
        }
        // A link is listed as what it points at.
        let is_dir = entry
            .file_type()
            .is_ok_and(|kind| kind.is_dir() || (kind.is_symlink() && entry.path().is_dir()));
        raw.push(rules::RawEntry { name, is_dir });
    }
    let listing = rules::listing(raw, mode, rules::LIST_CAP);
    let mut letters = rules::letters(&listing.entries);
    let mut rows = Vec::new();
    if mode == Mode::Folders {
        rows.push(Row {
            kind: RowKind::PickFolder,
            name: String::new(),
            path: dir.to_string(),
        });
        letters.leading += 1;
    }
    rows.extend(listing.entries.into_iter().map(|entry| Row {
        path: rules::join(dir, &entry.name),
        kind: match entry.kind {
            EntryKind::Folder => RowKind::Folder,
            EntryKind::File => RowKind::File,
        },
        name: entry.name,
    }));
    let omitted = listing.omitted + unread;
    let fill = if rows.is_empty() && omitted == 0 {
        Fill::Empty
    } else {
        Fill::Ready
    };
    if omitted > 0 {
        rows.push(Row {
            kind: RowKind::More(omitted),
            name: String::new(),
            path: String::new(),
        });
    }
    Ok((rows, letters, fill))
}

fn place_rows(places: &[Place]) -> Vec<Row> {
    places
        .iter()
        .map(|place| Row {
            kind: RowKind::Place(place.kind.clone()),
            name: String::new(),
            path: place.path.clone(),
        })
        .collect()
}

/// Read a folder, or Places for `None`, off the event loop. Places are
/// re-read every time: a drive can come and go while the browser is open.
/// A folder picked for Core only offers the drives Core browses.
fn read(platform: Platform, custom_dir: &str, dir: Option<String>, mode: Mode) -> Landing {
    let places = read_places(platform, custom_dir);
    let places = if mode == Mode::Folders {
        rules::core_places(places)
    } else {
        places
    };
    match dir {
        None => Landing {
            rows: place_rows(&places),
            places,
            dir: None,
            letters: rules::Letters::default(),
            fill: Fill::Ready,
        },
        Some(dir) => match read_folder(&dir, mode) {
            Ok((rows, letters, fill)) => Landing {
                places,
                dir: Some(dir),
                rows,
                letters,
                fill,
            },
            Err(error) => {
                tracing::warn!("file browser: cannot read a folder: {error}");
                Landing {
                    places,
                    dir: Some(dir),
                    rows: Vec::new(),
                    letters: rules::Letters::default(),
                    fill: Fill::Error,
                }
            }
        },
    }
}

/// Where a fill puts focus once it lands.
#[derive(Debug, Clone)]
enum Seat {
    First,
    /// The entry with this path, as Back from a folder or a restore asks.
    Path(String),
}

/// Run `read` off the event loop and show what it finds. The folder on
/// screen stays, and takes nothing but Back, until the new one is ready.
fn start(
    ctx: &Ctx,
    app: &App,
    read: impl FnOnce(Platform, String, Mode) -> (Landing, Seat) + Send + 'static,
) {
    let (ticket, mode) = {
        let mut shared = lock(&ctx.shared);
        let model = &mut shared.file_browser;
        model.ticket = model.ticket.wrapping_add(1);
        (model.ticket, model.mode)
    };
    crate::router::begin_pending(app, crate::Screen::FileBrowser);
    let platform = platform(ctx);
    let custom_dir = crate::customization::root().to_string_lossy().into_owned();
    let weak = app.as_weak();
    let ctx2 = ctx.clone();
    let task = ctx.handle.spawn(async move {
        let landing = tokio::task::spawn_blocking(move || read(platform, custom_dir, mode))
            .await
            .ok();
        let _ = weak.upgrade_in_event_loop(move |app| {
            if let Some((landing, seat)) = landing {
                land(&ctx2, &app, ticket, landing, &seat);
            }
        });
    });
    lock(&ctx.shared).file_browser.task.replace(task);
}

/// Fill `dir`, or Places for `None`.
fn fill(ctx: &Ctx, app: &App, dir: Option<String>, seat: Seat) {
    start(ctx, app, move |platform, custom_dir, mode| {
        (read(platform, &custom_dir, dir, mode), seat)
    });
}

fn land(ctx: &Ctx, app: &App, ticket: u64, landing: Landing, seat: &Seat) {
    let shell = app.global::<crate::Shell>();
    if !shell.get_transitioning() || shell.get_transition_target() != crate::Screen::FileBrowser {
        // Cancelled, or another route has the screen.
        return;
    }
    {
        let mut shared = lock(&ctx.shared);
        let model = &mut shared.file_browser;
        if model.ticket != ticket {
            return;
        }
        model.places = landing.places;
        model.dir = landing.dir;
        model.rows = landing.rows;
        model.letters = landing.letters;
        model.fill = landing.fill;
        model.list_top = 0;
        model.page.expire();
        model.persist.discard();
        model.grid.set_item_count(model.rows.len());
        let index = match seat {
            Seat::First => 0,
            Seat::Path(path) => model
                .rows
                .iter()
                .position(|row| row.path == *path)
                .unwrap_or(0),
        };
        model.grid.set_current_index_immediate(index);
        shared.persist.active_screen = crate::Screen::FileBrowser.token().to_string();
    }
    write_selection(&mut lock(&ctx.shared));
    crate::router::save_persist(&ctx.shared);
    shape(ctx, app);
    render(ctx, app);
    crate::router::transition_to_screen(app, crate::Screen::FileBrowser, 1);
}

/// Change icon… on a Hub tile: images, from the folder of the tile's
/// current icon or the custom folder.
pub(crate) fn open_hub_icon(ctx: &Ctx, app: &App, target: HubTarget, icon: &str) {
    let icon = icon.to_string();
    {
        let mut shared = lock(&ctx.shared);
        shared.file_browser = Model {
            purpose: Some(Purpose::HubIcon(target.clone())),
            mode: Mode::Images,
            ticket: shared.file_browser.ticket,
            ..Model::default()
        };
        shared.persist.file_browser = FileBrowserState {
            target,
            mode: Mode::Images.as_str().to_string(),
            dir: String::new(),
            selected: String::new(),
        };
    }
    // Where it starts is decided off the event loop too: the icon's folder
    // may be on a drive that is slow to answer.
    start(ctx, app, move |platform, custom_dir, mode| {
        let dir = rules::start_dir(&icon, &custom_dir, &is_dir);
        (read(platform, &custom_dir, dir, mode), Seat::Path(icon))
    });
}

/// Add item > Custom folder…: folders of the disk Core browses, starting
/// on the SD card, for a live folder tile that lands on Hub cell
/// `hub_index`.
pub(crate) fn open_hub_folder(ctx: &Ctx, app: &App, hub_index: u32) {
    let target = HubTarget {
        hub_index,
        identity: String::new(),
    };
    {
        let mut shared = lock(&ctx.shared);
        shared.file_browser = Model {
            purpose: Some(Purpose::HubFolder(target.clone())),
            mode: Mode::Folders,
            ticket: shared.file_browser.ticket,
            ..Model::default()
        };
        shared.persist.file_browser = FileBrowserState {
            target,
            mode: Mode::Folders.as_str().to_string(),
            dir: String::new(),
            selected: String::new(),
        };
    }
    start(ctx, app, move |platform, custom_dir, mode| {
        let dir = Some(rules::FOLDER_START.to_string()).filter(|dir| is_dir(dir));
        (read(platform, &custom_dir, dir, mode), Seat::First)
    });
}

/// Cold start on the browser: the same tile, folder and entry. A tile that
/// moved restores the Hub; a folder that is gone restores Places.
pub(crate) fn restore(ctx: &Ctx, app: &App) {
    let saved = lock(&ctx.shared).persist.file_browser.clone();
    let mode = Mode::parse(&saved.mode);
    // A folder pick has no tile yet; its cell is only where the new one lands.
    if mode != Mode::Folders && crate::hub::edit_target_matching(ctx, app, &saved.target).is_none()
    {
        lock(&ctx.shared).persist.active_screen = crate::Screen::Hub.token().to_string();
        crate::router::save_persist(&ctx.shared);
        return;
    }
    let dir = (!saved.dir.is_empty()).then_some(saved.dir.clone());
    {
        let mut shared = lock(&ctx.shared);
        shared.file_browser = Model {
            purpose: Some(Purpose::for_mode(mode, saved.target.clone())),
            mode,
            ticket: shared.file_browser.ticket,
            ..Model::default()
        };
    }
    let seat = Seat::Path(match &dir {
        Some(dir) => rules::join(dir, &saved.selected),
        None => saved.selected.clone(),
    });
    start(ctx, app, move |platform, custom_dir, mode| {
        let dir = dir.filter(|dir| is_dir(dir));
        (read(platform, &custom_dir, dir, mode), seat)
    });
}

/// What a restore needs to find the entry again: a Place's path, or a
/// name inside the folder on screen.
fn selection_key(model: &Model) -> String {
    model
        .rows
        .get(model.grid.current_index())
        .map(|row| match (&row.kind, &model.dir) {
            (RowKind::PickFolder, _) => String::new(),
            (RowKind::Place(_), _) | (_, None) => row.path.clone(),
            _ => rules::base_name(&row.path).to_string(),
        })
        .unwrap_or_default()
}

fn write_selection(shared: &mut Shared) {
    let model = &shared.file_browser;
    let selected = selection_key(model);
    let dir = model.dir.clone().unwrap_or_default();
    let mode = model.mode.as_str().to_string();
    let state = &mut shared.persist.file_browser;
    state.dir = dir;
    state.selected = selected;
    state.mode = mode;
}

/// A move landed: write the selection once it rests, as the games screens
/// do (`PERSIST_DEBOUNCE_MS`).
fn schedule_persist(ctx: &Ctx) {
    let seq = {
        let mut shared = lock(&ctx.shared);
        let model = &mut shared.file_browser;
        let key = selection_key(model);
        if !model.persist.schedule(&key) {
            return;
        }
        model.persist_seq += 1;
        model.persist_seq
    };
    let ctx = ctx.clone();
    slint::Timer::single_shot(
        std::time::Duration::from_millis(list_rules::PERSIST_DEBOUNCE_MS),
        move || {
            if lock(&ctx.shared).file_browser.persist_seq == seq {
                flush_persist(&ctx);
            }
        },
    );
}

/// Commit a scheduled selection write now (Accept, Back, hold release).
fn flush_persist(ctx: &Ctx) {
    let pending = {
        let mut shared = lock(&ctx.shared);
        shared.file_browser.persist_seq += 1;
        let pending = shared.file_browser.persist.flush().is_some();
        if pending {
            write_selection(&mut shared);
        }
        pending
    };
    if pending {
        crate::router::save_persist(&ctx.shared);
    }
}

// ---------- Geometry and rendering ----------

fn rotated(ctx: &Ctx) -> bool {
    matches!(
        lock(&ctx.shared).persist.settings.orientation.as_str(),
        "cw" | "ccw"
    )
}

/// The games grid, with no Menu chip band.
fn grid_geometry(app: &App) -> crate::games::Geometry {
    let inputs = crate::router::output_scene(app).inputs();
    crate::games::grid_geometry_for(&inputs, crate::GamesMode::Favorites, 0)
}

/// Fit the grid to the scene: the games grid in images mode, a column of
/// list rows otherwise.
fn shape(ctx: &Ctx, app: &App) {
    let (columns, rows) = if lock(&ctx.shared).file_browser.mode == Mode::Images {
        let geometry = grid_geometry(app);
        (
            usize::try_from(geometry.columns).unwrap_or(1),
            usize::try_from(geometry.rows).unwrap_or(1),
        )
    } else {
        (1, list_visible(ctx, app))
    };
    lock(&ctx.shared).file_browser.grid.set_shape(columns, rows);
}

fn list_geometry(ctx: &Ctx, app: &App) -> list_rules::ListGeometry {
    let inputs = crate::router::output_scene(app).inputs();
    let rotated = rotated(ctx);
    crate::games::list_geometry_for(
        &inputs,
        rotated,
        0,
        list_rules::list_visible_rows(ctx.crt_enabled, rotated),
    )
}

fn list_visible(ctx: &Ctx, app: &App) -> usize {
    list_geometry(ctx, app).visible_rows.max(1)
}

/// The words for a row the view cannot take as a name: a Place, and the
/// count of entries left out.
fn row_name(app: &App, row: &Row) -> String {
    let labels = app.global::<crate::Labels>();
    let place = |key: &str, name: &str| {
        labels
            .invoke_place(SharedString::from(key), SharedString::from(name))
            .to_string()
    };
    match &row.kind {
        RowKind::Place(kind) => match kind {
            PlaceKind::Custom => place("custom", ""),
            PlaceKind::SdCard => place("sd", ""),
            PlaceKind::Usb(n) => place("usb", &n.to_string()),
            PlaceKind::Network => place("network", ""),
            PlaceKind::Home => place("home", ""),
            PlaceKind::Drive(name) => name.clone(),
            PlaceKind::Root => place("root", ""),
        },
        RowKind::More(n) => labels
            .invoke_more_not_shown(i32::try_from(*n).unwrap_or(i32::MAX))
            .to_string(),
        RowKind::PickFolder => labels
            .invoke_menu(SharedString::from("file_add_folder"), SharedString::new())
            .to_string(),
        RowKind::Folder | RowKind::File => row.name.clone(),
    }
}

fn cell_for(app: &App, row: &Row, mode: Mode, art_size: u32) -> GridCell {
    let mut cell = GridCell {
        name: SharedString::from(row_name(app, row)),
        disabled: matches!(row.kind, RowKind::More(_)),
        ..Default::default()
    };
    match row.kind {
        // A list row has no art, so a Place names its path beside it and a
        // folder ends in a slash.
        RowKind::Place(_) => {
            cell.glyph_key = "icons/Folder".into();
            if mode.is_list() {
                cell.tags = SharedString::from(row.path.as_str());
            }
        }
        RowKind::PickFolder => cell.glyph_key = "icons/Folder".into(),
        RowKind::Folder => {
            cell.glyph_key = "icons/Folder".into();
            if mode.is_list() {
                cell.name = SharedString::from(format!("{}/", row.name));
            }
        }
        RowKind::File if mode == Mode::Images => {
            // Prepared off the event loop, so it lands like a cover: faded
            // in where the tile is on screen.
            cell.fetched = true;
            match crate::customization::path_image(&row.path, art_size) {
                Some(image) => {
                    cell.cover = image;
                    cell.has_cover = true;
                }
                None => cell.glyph_key = "icons/File".into(),
            }
        }
        RowKind::File => cell.glyph_key = "icons/File".into(),
        RowKind::More(_) => {}
    }
    cell
}

fn page_cells(app: &App, model: &Model, page: usize, art_size: u32) -> Vec<GridCell> {
    let size = model.grid.page_size();
    model
        .rows
        .iter()
        .skip(page * size)
        .take(size)
        .map(|row| cell_for(app, row, model.mode, art_size))
        .collect()
}

/// The rail: the folder's letters with the current one, and the position.
fn publish_rail(view: &FileBrowserView<'_>, model: &Model) {
    let index = model.grid.current_index();
    let letters = &model.letters;
    let at = list_rules::letter_at(&letters.offsets, letters.leading, index);
    let labels: Vec<SharedString> = letters.labels.iter().map(SharedString::from).collect();
    let current = view.get_rail_letters();
    let same = slint::Model::row_count(&current) == labels.len()
        && labels
            .iter()
            .enumerate()
            .all(|(i, label)| slint::Model::row_data(&current, i).as_ref() == Some(label));
    view.set_rail_index(at.map_or(-1, |i| i32::try_from(i).unwrap_or(-1)));
    view.set_rail_letter(at.and_then(|i| labels.get(i).cloned()).unwrap_or_default());
    if !same {
        view.set_rail_letters(ModelRc::new(VecModel::from(labels)));
    }
    view.set_rail_fraction(list_rules::rail_fraction(index, model.rows.len()));
    // A folder's listing is whole: its total holds still.
    view.set_rail_visible(
        model.rapid.rail_visible && list_rules::rail_offered(Some(model.rows.len())),
    );
}

/// The images grid on the shared page-turn strip. While a page turns, the
/// destination goes to the incoming half of the strip and the page on show
/// stays as it was.
fn publish_grid(app: &App, view: &FileBrowserView<'_>, model: &Model) {
    let current = model.grid.current_index();
    let focused = model.rows.get(current);
    let geometry = grid_geometry(app);
    let art_size = zaparoo_app::customization::artwork_size(
        geometry.fit.cell_width as f32,
        geometry.fit.cell_height as f32,
    );
    let page = model.grid.current_page();
    let start = page * model.grid.page_size();
    let strip_sliding = model.page.sliding && !view.get_cached_transition();
    if strip_sliding {
        crate::view_model::publish_cells(
            &view.get_next_cells(),
            page_cells(app, model, page, art_size),
            |rows| view.set_next_cells(rows),
        );
    } else {
        let landed = crate::view_model::publish_cells(
            &view.get_cells(),
            page_cells(app, model, page, art_size),
            |rows| view.set_cells(rows),
        );
        let grid = crate::cover_fade::Grid {
            fit: &geometry.fit,
            insets: &geometry.insets,
            top: geometry.grid_y,
            columns: geometry.columns,
        };
        let selected = current.checked_sub(start);
        crate::cover_fade::request(app, &[crate::Screen::FileBrowser], &grid, &landed, selected);
        view.set_next_cells(ModelRc::new(VecModel::from(Vec::<GridCell>::new())));
    }
    view.set_selected_local(if strip_sliding {
        -1
    } else {
        i32::try_from(current.saturating_sub(start)).unwrap_or(0)
    });
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
    view.set_total_pages(i32::try_from(model.grid.total_page_count()).unwrap_or(1));
    view.set_has_pages_above(model.grid.has_pages_above());
    view.set_has_pages_below(model.grid.has_pages_below());
    view.set_label_name(SharedString::from(
        focused.map(|row| row_name(app, row)).unwrap_or_default(),
    ));
    view.set_list_rows(ModelRc::new(VecModel::from(Vec::<GridCell>::new())));
}

pub(crate) fn render(ctx: &Ctx, app: &App) {
    let list_visible_rows = list_visible(ctx, app);
    let list_geometry = list_geometry(ctx, app);
    let mut shared = lock(&ctx.shared);
    let view = app.global::<FileBrowserView>();
    let shared_ref = &*shared;
    let model = &shared_ref.file_browser;
    view.set_mode(match model.mode {
        Mode::Plain | Mode::Folders => crate::FileBrowserMode::Plain,
        Mode::Images => crate::FileBrowserMode::Images,
    });
    view.set_places(model.dir.is_none());
    view.set_title(SharedString::from(
        model
            .dir
            .as_deref()
            .map(rules::base_name)
            .unwrap_or_default(),
    ));
    view.set_state(match model.fill {
        Fill::Ready => ContentState::Ready,
        Fill::Empty => ContentState::Empty,
        Fill::Error => ContentState::Error,
    });
    view.set_focus_ready(true);
    let current = model.grid.current_index();
    let count = model.rows.len();
    let focused = model.rows.get(current);
    view.set_focused_opens(matches!(
        focused.map(|row| &row.kind),
        Some(RowKind::Place(_) | RowKind::Folder)
    ));
    view.set_focused_picks(matches!(
        focused.map(|row| &row.kind),
        Some(RowKind::File | RowKind::PickFolder)
    ));
    view.set_has_menu(matches!(model.purpose, Some(Purpose::HubIcon(_))));
    view.set_count(i32::try_from(count).unwrap_or(i32::MAX));
    view.set_current_index(i32::try_from(current).unwrap_or(0));
    view.set_activate_pulse(model.activate_pulse);
    view.set_release_pulse(model.release_pulse);
    publish_rail(&view, model);

    if !model.mode.is_list() {
        publish_grid(app, &view, model);
        return;
    }
    let visible = list_visible_rows;
    let paging = list_rules::list_paging(current, count, Some(count), true, visible, false);
    view.set_list_page(i32::try_from(paging.current_page).unwrap_or(0));
    view.set_list_total_pages(i32::try_from(paging.total_pages).unwrap_or(1));
    view.set_has_items_above(paging.has_items_above);
    view.set_has_items_below(paging.has_items_below);
    view.set_list_x(list_geometry.card_x as f32);
    view.set_list_y(list_geometry.card_y as f32);
    view.set_list_width(list_geometry.card_width as f32);
    view.set_list_height(list_geometry.card_height as f32);
    view.set_list_row_height(list_geometry.row_height as f32);
    view.set_label_y(list_geometry.card_y as f32 + list_geometry.card_height as f32);
    let top = crate::browse_motion::publish_list(
        &view,
        &shared_ref.input,
        current,
        count,
        visible,
        model.list_top,
        &model.rows,
        |row| cell_for(app, row, Mode::Plain, 0),
    );
    view.set_cells(ModelRc::new(VecModel::from(Vec::<GridCell>::new())));
    shared.file_browser.list_top = top;
}

/// Prepared previews landed while the browser is on screen.
pub(crate) fn images_landed(ctx: &Ctx, app: &App) {
    if !lock(&ctx.shared).file_browser.mode.is_list() {
        render(ctx, app);
    }
}

/// The browser's grid on the shared page turn.
pub(crate) struct FileBrowserScreen;

impl crate::page_motion::PagedScreen for FileBrowserScreen {
    type View<'a> = FileBrowserView<'a>;
    fn view(app: &App) -> FileBrowserView<'_> {
        app.global::<FileBrowserView>()
    }
    fn motion(shared: &mut Shared) -> &mut PageMotion {
        &mut shared.file_browser.page
    }
    fn render(ctx: &Ctx, app: &App) {
        render(ctx, app);
    }
    fn on_screen(app: &App) -> bool {
        app.global::<crate::Shell>().get_active_screen() == crate::Screen::FileBrowser
    }
    fn list_layout(app: &App) -> bool {
        app.global::<FileBrowserView>().get_mode() == crate::FileBrowserMode::Plain
    }
    /// A new folder replaces the rows a turn was showing.
    fn epoch(shared: &Shared) -> u64 {
        shared.file_browser.ticket
    }
}

pub(crate) fn interrupt_page(ctx: &Ctx, app: &App) {
    crate::page_motion::interrupt::<FileBrowserScreen>(ctx, app);
}

/// A held fast scroll: the rail comes up with it and lingers after it, as
/// on the games screens.
pub(crate) fn set_rapid(ctx: &Ctx, app: &App, active: bool) {
    let (changed, linger) = lock(&ctx.shared).file_browser.rapid.set(active);
    if let Some(seq) = linger {
        crate::browse_motion::linger_rail(
            ctx,
            app,
            seq,
            |shared| &mut shared.file_browser.rapid,
            |app| app.global::<FileBrowserView>().set_rail_visible(false),
        );
    }
    if changed {
        render(ctx, app);
    }
}

// ---------- Input ----------

/// A grid move or page turn, then the page's own turn if it changed.
fn grid_move(ctx: &Ctx, app: &App, d_col: i32, d_row: i32, page_delta: i32) {
    let (moved, from_page, to) = {
        let mut shared = lock(&ctx.shared);
        let model = &mut shared.file_browser;
        if model.page.sliding {
            return;
        }
        let from_page = model.grid.current_page();
        let moved = if page_delta != 0 {
            model.grid.page_by(page_delta)
        } else {
            model.grid.move_selection(d_col, d_row)
        };
        let grid = &model.grid;
        let to = (
            grid.current_page(),
            grid.page_direction().offset(),
            grid.current_index() - grid.current_page() * grid.page_size(),
        );
        (moved, from_page, to)
    };
    if !moved {
        return;
    }
    schedule_persist(ctx);
    let (to_page, direction, local) = to;
    if to_page == from_page {
        render(ctx, app);
    } else {
        crate::page_motion::turn::<FileBrowserScreen>(ctx, app, direction, local, true);
    }
}

/// A list move: a page step lands on the end before wrapping, and a held
/// one stops there.
fn list_move(ctx: &Ctx, app: &App, delta: i64) {
    let held = list_rules::is_page_move(delta) && crate::input::dispatching_repeat(ctx);
    let moved = {
        let mut shared = lock(&ctx.shared);
        let model = &mut shared.file_browser;
        let count = model.rows.len();
        match list_rules::linear_move(model.grid.current_index(), count, delta, false, held) {
            list_rules::LinearMove::To { index, .. } => {
                model.grid.set_current_index_immediate(index);
                true
            }
            list_rules::LinearMove::WrapToTail => {
                model
                    .grid
                    .set_current_index_immediate(count.saturating_sub(1));
                true
            }
            list_rules::LinearMove::FetchMore | list_rules::LinearMove::Stay { .. } => false,
        }
    };
    if moved {
        schedule_persist(ctx);
        render(ctx, app);
    }
}

/// A long hold's letter step: the next or previous letter of the folder.
/// False where there is no further letter, so the hold keeps paging.
fn letter_step(ctx: &Ctx, app: &App, forward: bool) -> bool {
    let target = {
        let shared = lock(&ctx.shared);
        let model = &shared.file_browser;
        let letters = &model.letters;
        let Some(bucket) = list_rules::letter_step(
            &letters.offsets,
            letters.leading,
            model.grid.current_index(),
            forward,
        ) else {
            return false;
        };
        letters.leading + letters.offsets[bucket] as usize
    };
    let (from_page, list) = {
        let mut shared = lock(&ctx.shared);
        let model = &mut shared.file_browser;
        let from_page = model.grid.current_page();
        model.grid.set_current_index_immediate(target);
        (from_page, model.mode.is_list())
    };
    schedule_persist(ctx);
    let (to_page, local) = {
        let shared = lock(&ctx.shared);
        let grid = &shared.file_browser.grid;
        (
            grid.current_page(),
            grid.current_index() - grid.current_page() * grid.page_size(),
        )
    };
    if !list && to_page != from_page {
        let direction = if to_page > from_page { 1 } else { -1 };
        crate::page_motion::turn::<FileBrowserScreen>(ctx, app, direction, local, true);
    } else {
        render(ctx, app);
    }
    true
}

/// A held page flip stops at the folder's ends, as on the games screens: a
/// tapped one wraps, but a held one wrapping sends the user away from the
/// end they were heading for.
fn at_rapid_edge(ctx: &Ctx, dir: i64) -> bool {
    let shared = lock(&ctx.shared);
    let grid = &shared.file_browser.grid;
    if dir < 0 {
        grid.current_page() == 0
    } else {
        grid.current_page() + 1 >= grid.total_page_count()
    }
}

/// Leave for the Hub, on the tile the browser was opened for.
fn leave(ctx: &Ctx, app: &App) {
    flush_persist(ctx);
    let purpose = {
        let mut shared = lock(&ctx.shared);
        shared.file_browser.task.cancel();
        shared.file_browser.page.expire();
        shared.persist.active_screen = crate::Screen::Hub.token().to_string();
        shared.persist.file_browser = FileBrowserState::default();
        shared.file_browser.rows.clear();
        shared.file_browser.purpose.take()
    };
    crate::router::save_persist(&ctx.shared);
    match purpose {
        Some(Purpose::HubIcon(target)) => crate::hub::return_to_tile(ctx, app, &target),
        Some(Purpose::HubFolder(_)) | None => crate::router::return_to_hub(ctx, app),
    }
}

fn pick(ctx: &Ctx, app: &App, path: &str) {
    let purpose = lock(&ctx.shared).file_browser.purpose.clone();
    match purpose {
        Some(Purpose::HubIcon(target)) => crate::hub::set_tile_icon(ctx, app, &target, path),
        Some(Purpose::HubFolder(target)) => {
            // The Hub takes the new tile and arms it for Move once it is
            // back on screen; leaving first keeps that order.
            crate::router::forget_menu_return(ctx);
            leave(ctx, app);
            crate::hub::add_live_folder(ctx, app, target.hub_index, path);
            return;
        }
        None => {}
    }
    crate::router::forget_menu_return(ctx);
    leave(ctx, app);
}

fn back(ctx: &Ctx, app: &App) {
    flush_persist(ctx);
    let (dir, places) = {
        let shared = lock(&ctx.shared);
        (
            shared.file_browser.dir.clone(),
            shared.file_browser.places.clone(),
        )
    };
    let Some(dir) = dir else {
        let folder_pick = matches!(
            lock(&ctx.shared).file_browser.purpose,
            Some(Purpose::HubFolder(_))
        );
        leave(ctx, app);
        if folder_pick {
            crate::hub::open_add_picker(ctx, app);
        } else {
            crate::router::return_to_menu(ctx, app, zaparoo_app::options_menu::Opens::IconBrowser);
        }
        return;
    };
    let root = rules::place_for(&places, &dir).map_or("/", |place| place.path.as_str());
    match rules::up(&dir, root) {
        Up::Folder(parent) => fill(ctx, app, Some(parent), Seat::Path(dir)),
        Up::Places => {
            let place = rules::place_for(&places, &dir)
                .map(|place| place.path.clone())
                .unwrap_or_default();
            fill(ctx, app, None, Seat::Path(place));
        }
    }
}

/// Accept on the focused entry. The grid's tile is pushed by the router's
/// press cue before this runs; a list row flashes as a games row does.
fn accept(ctx: &Ctx, app: &App) {
    let (row, seq, list) = {
        let mut shared = lock(&ctx.shared);
        let model = &mut shared.file_browser;
        let Some(row) = model.rows.get(model.grid.current_index()).cloned() else {
            return;
        };
        if matches!(row.kind, RowKind::More(_)) {
            return;
        }
        model.press_seq += 1;
        let list = model.mode.is_list();
        if list {
            model.activate_pulse += 1;
        }
        (row, model.press_seq, list)
    };
    flush_persist(ctx);
    if list {
        render(ctx, app);
        let ctx2 = ctx.clone();
        let weak = app.as_weak();
        slint::Timer::single_shot(
            std::time::Duration::from_millis(crate::games::press_duration(app)),
            move || {
                {
                    let mut shared = lock(&ctx2.shared);
                    if shared.file_browser.press_seq != seq {
                        return;
                    }
                    shared.file_browser.release_pulse += 1;
                }
                if let Some(app) = weak.upgrade() {
                    if app.global::<crate::Shell>().get_active_screen()
                        == crate::Screen::FileBrowser
                    {
                        render(&ctx2, &app);
                    }
                }
            },
        );
    }
    match row.kind {
        RowKind::Place(_) | RowKind::Folder => fill(ctx, app, Some(row.path), Seat::First),
        RowKind::File | RowKind::PickFolder => pick(ctx, app, &row.path),
        RowKind::More(_) => {}
    }
}

fn open_menu(ctx: &Ctx, app: &App) {
    let purpose = lock(&ctx.shared).file_browser.purpose.clone();
    let entries = match purpose {
        Some(Purpose::HubIcon(_)) => vec![crate::router::menu_row("file_default_icon")],
        Some(Purpose::HubFolder(_)) | None => return,
    };
    crate::router::present_file_browser_menu(ctx, app, entries);
}

/// A row of the browser's Menu list.
pub(crate) fn menu_accept(ctx: &Ctx, app: &App, id: &str) {
    if id == "file_default_icon" {
        let purpose = lock(&ctx.shared).file_browser.purpose.clone();
        if let Some(Purpose::HubIcon(target)) = purpose {
            crate::hub::set_tile_icon(ctx, app, &target, "");
        }
        crate::router::forget_menu_return(ctx);
        leave(ctx, app);
    }
}

/// The browser's input, with the games screens' navigation: single steps,
/// a long hold paging and then stepping a letter at a time, and held page
/// flips that stop at the ends.
pub fn handle_action(ctx: &Ctx, app: &App, action: &str) {
    interrupt_page(ctx, app);
    let (list, ready) = {
        let shared = lock(&ctx.shared);
        let model = &shared.file_browser;
        (model.mode.is_list(), model.fill == Fill::Ready)
    };
    let list_page = i64::try_from(list_visible(ctx, app)).unwrap_or(10);
    match action {
        actions::LEFT | actions::RIGHT if ready => {
            let dir: i64 = if action == actions::LEFT { -1 } else { 1 };
            if list {
                list_move(ctx, app, dir * list_page);
            } else {
                grid_move(ctx, app, dir as i32, 0, 0);
            }
        }
        actions::UP | actions::DOWN if ready => {
            let dir: i64 = if action == actions::UP { -1 } else { 1 };
            if crate::input::letter_step(ctx) && letter_step(ctx, app, dir > 0) {
                return;
            }
            if crate::input::rapid_page(ctx) {
                if list {
                    list_move(ctx, app, dir * list_page);
                } else if !at_rapid_edge(ctx, dir) {
                    grid_move(ctx, app, 0, 0, dir as i32);
                }
                return;
            }
            if list {
                list_move(ctx, app, dir);
            } else {
                grid_move(ctx, app, 0, dir as i32, 0);
            }
        }
        actions::PAGE_PREV | actions::PAGE_NEXT if ready => {
            let dir: i64 = if action == actions::PAGE_PREV { -1 } else { 1 };
            if crate::input::letter_step(ctx) && letter_step(ctx, app, dir > 0) {
                return;
            }
            if list {
                list_move(ctx, app, dir * list_page);
            } else if !(crate::input::dispatching_repeat(ctx) && at_rapid_edge(ctx, dir)) {
                grid_move(ctx, app, 0, 0, dir as i32);
            }
        }
        actions::ACCEPT => accept(ctx, app),
        actions::PAGE_MENU => open_menu(ctx, app),
        actions::CANCEL => back(ctx, app),
        _ => {}
    }
}

impl Shared {
    /// A page turn is in flight on the browser, for route tests.
    #[cfg(all(test, feature = "mister"))]
    pub(crate) fn file_browser_sliding(&self) -> bool {
        self.file_browser.page.sliding
    }

    /// A fill started, as a folder read would, for route tests.
    #[cfg(all(test, feature = "mister"))]
    pub(crate) fn file_browser_bump_ticket(&mut self) {
        self.file_browser.ticket = self.file_browser.ticket.wrapping_add(1);
    }
}

/// The folder read a route test cannot run on its undriven runtime: read
/// `dir` now and land it as the pending fill would.
#[cfg(all(test, feature = "mister"))]
pub(crate) fn land_now(ctx: &Ctx, app: &App, dir: Option<&str>, select: &str) {
    let (ticket, mode) = {
        let shared = lock(&ctx.shared);
        (shared.file_browser.ticket, shared.file_browser.mode)
    };
    let custom_dir = crate::customization::root().to_string_lossy().into_owned();
    let landing = read(platform(ctx), &custom_dir, dir.map(str::to_string), mode);
    land(ctx, app, ticket, landing, &Seat::Path(select.to_string()));
}

/// The scene changed size: refit the grid and repaint.
pub(crate) fn relayout(ctx: &Ctx, app: &App) {
    if app.global::<crate::Shell>().get_active_screen() == crate::Screen::FileBrowser {
        shape(ctx, app);
        render(ctx, app);
    }
}

fn pointer_allowed(ctx: &Ctx, app: &App) -> bool {
    let shell = app.global::<crate::Shell>();
    shell.get_active_screen() == crate::Screen::FileBrowser
        && !shell.get_transitioning()
        && !crate::input::modal_open(app)
        && lock(&ctx.shared).persist.settings.mouse_enabled
}

/// Move focus to the visible entry `local`; false when there is none.
fn point_at(ctx: &Ctx, app: &App, local: i32) -> bool {
    let Ok(local) = usize::try_from(local) else {
        return false;
    };
    // The list reports rows from the start of its published window, which
    // runs a page ahead of the rows on screen, as the games lists do.
    let list_base =
        usize::try_from(app.global::<FileBrowserView>().get_list_view_top()).unwrap_or(0);
    let mut shared = lock(&ctx.shared);
    let model = &mut shared.file_browser;
    let first = if model.mode == Mode::Images {
        model.grid.current_page() * model.grid.page_size()
    } else {
        list_base
    };
    let index = first + local;
    if index >= model.rows.len() {
        return false;
    }
    model.grid.set_current_index_immediate(index);
    true
}

/// Wire the view's pointer callbacks.
pub fn bind(ctx: &Ctx, app: &App) {
    let input = app.global::<FileBrowserInput>();
    {
        let ctx = ctx.clone();
        let weak = app.as_weak();
        input.on_hovered(move |local| {
            let Some(app) = weak.upgrade() else {
                return;
            };
            if pointer_allowed(&ctx, &app) && point_at(&ctx, &app, local) {
                render(&ctx, &app);
            }
        });
    }
    {
        let ctx = ctx.clone();
        let weak = app.as_weak();
        input.on_clicked(move |local| {
            let Some(app) = weak.upgrade() else {
                return;
            };
            if pointer_allowed(&ctx, &app) && point_at(&ctx, &app, local) {
                render(&ctx, &app);
                // Through the router, so a click gets the press cue too.
                crate::router::handle_action(&ctx, &app, actions::ACCEPT);
            }
        });
    }
    let page = |ctx: Ctx, weak: slint::Weak<App>| {
        move |delta: i32| {
            let Some(app) = weak.upgrade() else {
                return;
            };
            if !pointer_allowed(&ctx, &app) {
                return;
            }
            let action = if delta > 0 {
                actions::PAGE_NEXT
            } else {
                actions::PAGE_PREV
            };
            handle_action(&ctx, &app, action);
        }
    };
    input.on_wheel(page(ctx.clone(), app.as_weak()));
    input.on_page_requested(page(ctx.clone(), app.as_weak()));
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    reason = "tests should fail fast on a filesystem they just set up"
)]
mod tests {
    use super::*;

    /// A scratch folder for one test, removed on the way out.
    struct Scratch(std::path::PathBuf);

    impl Scratch {
        fn new(name: &str) -> Self {
            let path =
                std::env::temp_dir().join(format!("zaparoo-browser-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).expect("scratch dir");
            Self(path)
        }

        fn path(&self) -> String {
            self.0.to_string_lossy().into_owned()
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn a_folder_lists_its_images_folders_first() {
        let dir = Scratch::new("images");
        std::fs::create_dir(dir.0.join("art")).expect("dir");
        std::fs::write(dir.0.join("b.png"), b"x").expect("file");
        std::fs::write(dir.0.join("notes.txt"), b"x").expect("file");
        std::fs::write(dir.0.join(".hidden.png"), b"x").expect("file");
        let path = dir.path();
        let (rows, _, fill) = read_folder(&path, Mode::Images).expect("read");
        assert_eq!(fill, Fill::Ready);
        let names: Vec<&str> = rows.iter().map(|row| row.name.as_str()).collect();
        assert_eq!(names, ["art", "b.png"]);
        assert_eq!(rows[0].kind, RowKind::Folder);
        assert_eq!(rows[1].path, rules::join(&path, "b.png"));
        let (plain, _, _) = read_folder(&path, Mode::Plain).expect("read");
        assert_eq!(plain.len(), 3);
        assert!(read_folder("/nonexistent-zaparoo-dir", Mode::Plain).is_err());
    }

    #[test]
    fn an_empty_folder_reads_as_empty() {
        let dir = Scratch::new("empty");
        let (rows, _, fill) = read_folder(&dir.path(), Mode::Images).expect("read");
        assert!(rows.is_empty());
        assert_eq!(fill, Fill::Empty);
    }

    #[test]
    fn places_read_as_rows_that_open_their_path() {
        let rows = place_rows(&[Place {
            kind: PlaceKind::Root,
            path: "/".into(),
        }]);
        assert_eq!(rows[0].kind, RowKind::Place(PlaceKind::Root));
        assert_eq!(rows[0].path, "/");
    }
}
