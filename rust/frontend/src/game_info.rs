//! Game Info lifecycle, focus and image selection. Core metadata stays
//! transient.
//!
//! The panel is laid out as the Zaparoo App's details modal is: the art,
//! the actions, the description, then the facts. Focus moves over
//! what acts (`zaparoo_app::game_details::Stop`) and A does what the focused
//! item does: view the art full screen, open the manual, or search by a tag.

use crate::game_info_data::InfoRow;
use crate::games::GameRow;
use crate::media_cache::{DecodedImage, MediaKey};
use crate::router::{lock, Ctx};
use crate::{App, GameInfoView};
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use zaparoo_app::game_details::{self as rules, Stop};
use zaparoo_core::input_actions::actions;

/// Where Back from a tag search returns: the screen Game Info was opened
/// on, as it was, with Game Info open again on the same row.
#[derive(Debug)]
pub struct Drill {
    screen: crate::Screen,
    games: crate::games::GamesModel,
    search: zaparoo_core::persist::SearchState,
    entry: GameRow,
    row: String,
    /// Core's answer and the image on show, so Back lays the panel out in
    /// the same turn as the screen behind it, with nothing to wait for.
    meta: Option<Landed>,
    image: usize,
    /// The Options menu Details was opened from, with its rows, so Back
    /// from Game Info after the round trip still brings it back. Options
    /// on a result in between builds a menu of its own.
    menu: Option<crate::router::HeldReturn>,
}

/// The metadata the open panel was laid out from, and the key it was
/// asked for under. Process memory only, for the one panel.
type Landed = (MediaKey, zaparoo_core::media_types::MediaMeta);

#[derive(Debug, Default)]
pub struct GameInfoModel {
    ticket: u64,
    images: Vec<MediaKey>,
    selected: usize,
    /// The panel's painted art box as it last reported it; kept across
    /// opens so the next one asks for fitted art straight away.
    fit: zaparoo_app::covers::Fit,
    /// The game's manual, read in the panel's place.
    pub manual: crate::manual::ManualModel,
    entry: Option<GameRow>,
    system: String,
    rows: Vec<InfoRow>,
    stops: Vec<Stop>,
    focus: usize,
    /// The row whose values the values page lists, and the one focused.
    values: Option<usize>,
    value_index: usize,
    /// The image the full-screen viewer shows.
    viewer: Option<MediaKey>,
    /// The row a reopen after a tag search puts focus back on.
    refocus: Option<String>,
    /// The image the same reopen shows.
    reselect: usize,
    landed: Option<Landed>,
    drill: Option<Drill>,
}

impl GameInfoModel {
    fn reset(&mut self) -> u64 {
        self.ticket = self.ticket.wrapping_add(1);
        self.images.clear();
        self.selected = 0;
        self.rows.clear();
        self.stops.clear();
        self.focus = 0;
        self.values = None;
        self.value_index = 0;
        self.viewer = None;
        self.landed = None;
        self.ticket
    }

    /// The open in flight; a reply for another is dropped.
    #[cfg(all(test, feature = "mister"))]
    pub(crate) fn ticket(&self) -> u64 {
        self.ticket
    }

    fn cycle(&mut self, delta: i32) -> bool {
        let next = (self.selected as i64 + i64::from(delta))
            .clamp(0, self.images.len().saturating_sub(1) as i64) as usize;
        let changed = self.selected != next;
        self.selected = next;
        changed
    }

    fn focused(&self) -> Option<Stop> {
        self.stops.get(self.focus).copied()
    }

    fn stop_index(&self, stop: Stop) -> i32 {
        self.stops
            .iter()
            .position(|s| *s == stop)
            .and_then(|i| i32::try_from(i).ok())
            .unwrap_or(-1)
    }
}

/// Whether a tag search opened from Game Info is on screen, so its tags go
/// to Core as written, the way a Hub tile's do.
pub(crate) fn drilled(shared: &crate::router::Shared) -> bool {
    shared.game_info.drill.is_some()
}

/// A new search replaces the one Game Info opened; Back from it is the
/// Search screen's own again.
pub(crate) fn forget_drill(shared: &mut crate::router::Shared) {
    shared.game_info.drill = None;
}

/// The panel's art box: covers are resized to it before they are shown.
pub fn bind(ctx: &std::sync::Arc<Ctx>, app: &App) {
    let ctx = ctx.clone();
    let weak = app.as_weak();
    app.global::<GameInfoView>()
        .on_art_box(move |width, height| {
            // A turn later: the view reports its box while it is being
            // laid out, which can be inside a driver's own call.
            let ctx = ctx.clone();
            let weak = weak.clone();
            slint::Timer::single_shot(std::time::Duration::ZERO, move || {
                let Some(app) = weak.upgrade() else { return };
                let fit = zaparoo_app::covers::Fit::new(width, height).crisp();
                {
                    let mut shared = lock(&ctx.shared);
                    let model = &mut shared.game_info;
                    if model.fit == fit {
                        return;
                    }
                    model.fit = fit;
                    for key in &mut model.images {
                        key.fit = fit;
                    }
                }
                if app.global::<GameInfoView>().get_modal_open() {
                    show_image(&ctx, &app);
                }
            });
        });
}

/// Game Info opens whole: the panel waits, off screen, for Core's metadata,
/// so it never shows its title alone and then grows. Opened from Options,
/// the menu stays up with its row held until then; Back there calls the
/// read off. A wait that outlasts the delay says so in the header.
pub fn open(ctx: &Ctx, app: &App, entry: &GameRow) {
    let Some((ticket, fit, system)) = prepare(ctx, app, entry) else {
        return;
    };
    let hold = crate::router::hold_menu_for(|ctx| {
        lock(&ctx.shared).game_info.reset();
    })
    .then(|| crate::press_feedback::keep_held(app));
    let wait = crate::cue::begin(ctx, app, crate::AppCue::LoadingDetails, "", "");

    let folder = entry.entry_type == zaparoo_app::media_list::EntryType::Directory;
    let params = match entry.media_id.filter(|_| folder) {
        Some(disc) => zaparoo_core::media_types::MediaMetaParams::for_folder_disc(
            disc,
            system.clone(),
            entry.path.clone(),
        ),
        None => zaparoo_core::media_types::MediaMetaParams::for_media(
            system.clone(),
            entry.path.clone(),
        ),
    };
    // A multi-disc folder follows the disc it launches, as the list does.
    let key = MediaKey {
        media_id: entry.media_id,
        prefer: ctx.media.artwork_for(&system, &entry.relative_path),
        system,
        path: entry.path.clone(),
        max_size: crate::sizing::detail_cover_source_size(crate::router::output_scene(app)),
        fit,
        folder,
        image_type: None,
    };
    let ctx = ctx.clone();
    let weak = app.as_weak();
    ctx.handle.clone().spawn(async move {
        let result = ctx.store.client().media_meta(params).await;
        let _ = weak.upgrade_in_event_loop(move |app| {
            crate::cue::end(&ctx, &app, wait);
            if lock(&ctx.shared).game_info.ticket != ticket {
                return;
            }
            if let Some(hold) = hold {
                crate::press_feedback::release(&app, hold);
                crate::router::end_held_wait(&app);
            }
            let view = app.global::<GameInfoView>();
            // Everything below lands before the frame that shows the panel.
            view.set_modal_open(true);
            match result {
                Ok(result) => landed(&ctx, &app, &key, &result.media),
                Err(error) => {
                    tracing::warn!(path = %key.path, "game info fetch failed: {}", error.message);
                    view.set_failed(true);
                    publish_focus(&ctx, &app);
                }
            }
        });
    });
}

/// Clears the panel for `entry`. `None` when the entry cannot be looked
/// up, in which case the panel is already open with nothing to show.
fn prepare(
    ctx: &Ctx,
    app: &App,
    entry: &GameRow,
) -> Option<(u64, zaparoo_app::covers::Fit, String)> {
    let (ticket, fit, system) = {
        let mut shared = lock(&ctx.shared);
        let system = if entry.system_id.is_empty() {
            shared.games.system_id.clone()
        } else {
            entry.system_id.clone()
        };
        let model = &mut shared.game_info;
        let ticket = model.reset();
        model.entry = Some(entry.clone());
        model.system.clone_from(&system);
        (ticket, model.fit, system)
    };
    let view = app.global::<GameInfoView>();
    view.set_modal_name(entry.name.trim().into());
    view.set_modal_system(system.clone().into());
    view.set_modal_path(entry.path.clone().into());
    view.set_modal_description(SharedString::default());
    view.set_scroll_snap(true);
    view.set_info_rows(ModelRc::new(VecModel::from(Vec::<crate::InfoRow>::new())));
    view.set_media_missing(false);
    view.set_failed(false);
    view.set_scroll_position(0.0);
    view.set_image_count(0);
    view.set_image_index(0);
    view.set_modal_has_cover(false);
    view.set_modal_cover(slint::Image::default());
    publish_focus(ctx, app);
    close_viewer_view(app);
    view.set_values_open(false);
    crate::manual::clear(ctx, app);
    view.set_loading(false);
    let valid = !system.trim().is_empty() && !entry.path.trim().is_empty();
    if !valid {
        view.set_modal_open(true);
        return None;
    }
    Some((ticket, fit, system))
}

/// Core's metadata arrived: lay the panel out and seat focus.
fn landed(ctx: &Ctx, app: &App, key: &MediaKey, meta: &zaparoo_core::media_types::MediaMeta) {
    let view = app.global::<GameInfoView>();
    if !meta.title.name.trim().is_empty() {
        view.set_modal_name(meta.title.name.trim().into());
    }
    let description = crate::game_info_data::description(meta);
    view.set_modal_description(description.as_str().into());
    view.set_media_missing(meta.is_missing);
    let details = crate::game_info_data::details(meta);
    let rows = crate::game_info_data::info_rows(meta, &details, &key.path);
    publish_documents(ctx, app, key, meta);
    let has_manual = lock(&ctx.shared).game_info.manual.has_documents();
    let images: Vec<MediaKey> = {
        let preferred = ctx.media.effective_image_type(key.prefer.as_deref());
        let fit = lock(&ctx.shared).game_info.fit;
        zaparoo_app::media_list::detail_image_order(
            &crate::game_info_data::image_types(meta),
            &preferred,
        )
        .into_iter()
        .map(|kind| MediaKey {
            image_type: Some(kind),
            prefer: None,
            fit,
            ..key.clone()
        })
        .collect()
    };
    let searchable: Vec<bool> = rows.iter().map(InfoRow::searchable).collect();
    let stops = rules::stops(
        !images.is_empty(),
        has_manual,
        !description.is_empty(),
        &searchable,
    );
    let view_rows: Vec<crate::InfoRow> = rows
        .iter()
        .enumerate()
        .map(|(index, row)| crate::InfoRow {
            kind: if row.header {
                crate::RowKind::Header
            } else {
                crate::RowKind::Field
            },
            key: row.kind.as_str().into(),
            value: crate::game_info_data::joined(app, row).into(),
            stop: stops
                .iter()
                .position(|stop| *stop == Stop::Row(index))
                .and_then(|i| i32::try_from(i).ok())
                .unwrap_or(-1),
        })
        .collect();
    view.set_info_rows(ModelRc::new(VecModel::from(view_rows)));
    {
        let mut shared = lock(&ctx.shared);
        let model = &mut shared.game_info;
        // Back from a tag search lands on the row it searched from.
        let refocus = model.refocus.take();
        model.focus = refocus
            .and_then(|kind| {
                stops.iter().position(|stop| {
                    matches!(stop, Stop::Row(row) if rows.get(*row).is_some_and(|r| r.kind == kind))
                })
            })
            .unwrap_or(0);
        model.rows = rows;
        model.stops = stops;
        model.selected = std::mem::take(&mut model.reselect).min(images.len().saturating_sub(1));
        model.images = images;
        model.landed = Some((key.clone(), meta.clone()));
    }
    publish_focus(ctx, app);
    show_image(ctx, app);
}

/// Core's answer for the open panel, without asking Core.
#[cfg(all(test, feature = "mister"))]
pub(crate) fn land_for_test(ctx: &Ctx, app: &App, meta: &zaparoo_core::media_types::MediaMeta) {
    let key = {
        let shared = lock(&ctx.shared);
        let model = &shared.game_info;
        let Some(entry) = model.entry.clone() else {
            return;
        };
        MediaKey {
            media_id: entry.media_id,
            system: model.system.clone(),
            path: entry.path,
            max_size: 256,
            fit: model.fit,
            folder: entry.entry_type == zaparoo_app::media_list::EntryType::Directory,
            image_type: None,
            prefer: None,
        }
    };
    app.global::<GameInfoView>().set_modal_open(true);
    landed(ctx, app, &key, meta);
}

/// The stops and the focused one, for the view to ring.
fn publish_focus(ctx: &Ctx, app: &App) {
    let (focus, image, manual, description) = {
        let shared = lock(&ctx.shared);
        let model = &shared.game_info;
        (
            if model.stops.is_empty() {
                -1
            } else {
                i32::try_from(model.focus).unwrap_or(0)
            },
            model.stop_index(Stop::Image),
            model.stop_index(Stop::Manual),
            model.stop_index(Stop::Description),
        )
    };
    let view = app.global::<GameInfoView>();
    view.set_image_stop(image);
    view.set_manual_stop(manual);
    view.set_description_stop(description);
    view.set_focus(focus);
}

/// The game's documents, from its properties: the action row offers them.
fn publish_documents(
    ctx: &Ctx,
    app: &App,
    key: &MediaKey,
    meta: &zaparoo_core::media_types::MediaMeta,
) {
    let documents = zaparoo_app::manual::documents(
        meta.properties
            .keys()
            .chain(meta.title.properties.keys())
            .map(String::as_str),
    );
    let source = crate::manual::Source {
        media_id: key.media_id,
        system: key.system.clone(),
        path: key.path.clone(),
    };
    crate::manual::set_documents(ctx, app, source, documents);
}

pub fn close(ctx: &Ctx, app: &App) {
    lock(&ctx.shared).game_info.reset();
    crate::manual::clear(ctx, app);
    let view = app.global::<GameInfoView>();
    view.set_modal_open(false);
    view.set_loading(false);
    view.set_modal_cover(slint::Image::default());
    view.set_modal_has_cover(false);
    view.set_info_rows(ModelRc::new(VecModel::from(Vec::<crate::InfoRow>::new())));
    view.set_modal_description(SharedString::default());
    view.set_values_open(false);
    close_viewer_view(app);
}

fn show_image(ctx: &Ctx, app: &App) {
    let (index, count, key) = {
        let shared = lock(&ctx.shared);
        let model = &shared.game_info;
        (
            model.selected,
            model.images.len(),
            model.images.get(model.selected).cloned(),
        )
    };
    let view = app.global::<GameInfoView>();
    view.set_image_count(i32::try_from(count).unwrap_or(i32::MAX));
    view.set_image_index(i32::try_from(index).unwrap_or(0));
    view.set_image_kind(
        key.as_ref()
            .and_then(|key| key.image_type.as_deref())
            .unwrap_or_default()
            .into(),
    );
    view.set_modal_has_cover(false);
    view.set_modal_cover(slint::Image::default());
    // An image Core already said it does not have is never fetched again,
    // so nothing would arrive to end the wait.
    view.set_modal_cover_absent(key.as_ref().is_some_and(|key| ctx.media.is_negative(key)));
    if let Some(key) = key {
        if let Some(decoded) = ctx.media.get(&key) {
            cover_landed(ctx, app, &key, &decoded);
        } else {
            ctx.media.enqueue(key);
        }
    }
    if view.get_viewer_open() {
        show_viewer(ctx, app);
    }
}

/// Core answered that it has no image for `key`: stop waiting for it.
pub fn cover_absent(ctx: &Ctx, app: &App, key: &MediaKey) {
    let (panel, viewer) = {
        let shared = lock(&ctx.shared);
        let model = &shared.game_info;
        (
            model.images.get(model.selected) == Some(key),
            model.viewer.as_ref() == Some(key),
        )
    };
    let view = app.global::<GameInfoView>();
    if view.get_modal_open() && panel {
        view.set_modal_cover_absent(true);
    }
    if view.get_viewer_open() && viewer {
        view.set_viewer_absent(true);
    }
}

pub fn cover_landed(ctx: &Ctx, app: &App, key: &MediaKey, decoded: &DecodedImage) {
    let (panel, viewer) = {
        let shared = lock(&ctx.shared);
        let model = &shared.game_info;
        (
            model.images.get(model.selected) == Some(key),
            model.viewer.as_ref() == Some(key),
        )
    };
    let view = app.global::<GameInfoView>();
    if view.get_modal_open() && panel {
        view.set_modal_cover(slint::Image::from_rgba8(decoded.buffer.clone()));
        view.set_modal_has_cover(true);
    }
    if view.get_viewer_open() && viewer {
        view.set_viewer_image(slint::Image::from_rgba8(decoded.buffer.clone()));
        view.set_viewer_has_image(true);
    }
}

/// The focused image, full screen: asked for at the viewer's size, through
/// the same bounded cover cache.
fn show_viewer(ctx: &Ctx, app: &App) {
    let key = {
        let mut shared = lock(&ctx.shared);
        let (width, height, room) = shared.game_info.manual.page_box();
        let model = &mut shared.game_info;
        let room = if model.images.len() > 1 { 2 * room } else { 0 };
        let (width, height) = (width.saturating_sub(room), height.saturating_sub(room));
        let key = model.images.get(model.selected).map(|key| MediaKey {
            max_size: width.max(height).clamp(1, crate::sizing::VIEWER_MAX_SIZE),
            fit: zaparoo_app::covers::Fit::new(
                i32::try_from(width).unwrap_or(0),
                i32::try_from(height).unwrap_or(0),
            ),
            ..key.clone()
        });
        model.viewer.clone_from(&key);
        key
    };
    let view = app.global::<GameInfoView>();
    view.set_viewer_has_image(false);
    view.set_viewer_image(slint::Image::default());
    view.set_viewer_absent(key.as_ref().is_some_and(|key| ctx.media.is_negative(key)));
    if let Some(key) = key {
        if let Some(decoded) = ctx.media.get(&key) {
            cover_landed(ctx, app, &key, &decoded);
        } else {
            ctx.media.enqueue(key);
        }
    }
}

/// The viewer's box changed size: ask again at the new one.
pub(crate) fn viewer_box_changed(ctx: &Ctx, app: &App) {
    if app.global::<GameInfoView>().get_viewer_open() {
        show_viewer(ctx, app);
    }
}

fn close_viewer_view(app: &App) {
    let view = app.global::<GameInfoView>();
    view.set_viewer_open(false);
    view.set_viewer_has_image(false);
    view.set_viewer_absent(false);
    view.set_viewer_image(slint::Image::default());
}

fn viewer_action(ctx: &Ctx, app: &App, action: &str) {
    match action {
        actions::CANCEL => {
            lock(&ctx.shared).game_info.viewer = None;
            close_viewer_view(app);
        }
        actions::LEFT | actions::RIGHT | actions::PAGE_PREV | actions::PAGE_NEXT => {
            let delta = if action == actions::LEFT || action == actions::PAGE_PREV {
                -1
            } else {
                1
            };
            if lock(&ctx.shared).game_info.cycle(delta) {
                show_image(ctx, app);
            }
        }
        _ => {}
    }
}

/// The values page: the row's values, one per line, for A to search by.
fn open_values(ctx: &Ctx, app: &App, row: usize) {
    let (kind, names) = {
        let mut shared = lock(&ctx.shared);
        let model = &mut shared.game_info;
        let Some(info) = model.rows.get(row) else {
            return;
        };
        let typed = info.kind == "tags";
        let names: Vec<SharedString> = info
            .values
            .iter()
            .map(|value| crate::game_info_data::value_text(app, value, typed).into())
            .collect();
        let kind = info.kind.clone();
        model.values = Some(row);
        model.value_index = 0;
        (kind, names)
    };
    let view = app.global::<GameInfoView>();
    view.set_values_kind(kind.into());
    view.set_values(ModelRc::new(VecModel::from(names)));
    view.set_values_index(0);
    view.set_scroll_position(0.0);
    view.set_values_open(true);
}

fn values_action(ctx: &Ctx, app: &App, action: &str) {
    let view = app.global::<GameInfoView>();
    match action {
        actions::CANCEL => {
            lock(&ctx.shared).game_info.values = None;
            view.set_values_open(false);
            // Back on the row that opened the page, scrolled to it.
            view.set_scroll_position(0.0);
            let focus = view.get_focus();
            view.set_focus(-1);
            view.set_focus(focus);
        }
        actions::UP | actions::DOWN => {
            let index = {
                let mut shared = lock(&ctx.shared);
                let model = &mut shared.game_info;
                let count = model
                    .values
                    .and_then(|row| model.rows.get(row))
                    .map_or(0, |row| row.values.len());
                let delta = if action == actions::UP { -1 } else { 1 };
                model.value_index = rules::step(model.value_index, delta, count);
                model.value_index
            };
            view.set_values_index(i32::try_from(index).unwrap_or(0));
        }
        actions::ACCEPT => {
            let pick = {
                let shared = lock(&ctx.shared);
                let model = &shared.game_info;
                model
                    .values
                    .and_then(|row| model.rows.get(row))
                    .and_then(|row| {
                        row.values
                            .get(model.value_index)
                            .map(|value| (row.kind.clone(), value.search.clone()))
                    })
            };
            if let Some((kind, search)) = pick {
                drill(ctx, app, &kind, &search);
            }
        }
        _ => {}
    }
}

/// A on the focused item.
fn accept(ctx: &Ctx, app: &App) {
    let (focused, row) = {
        let shared = lock(&ctx.shared);
        let model = &shared.game_info;
        let focused = model.focused();
        let row = match focused {
            Some(Stop::Row(index)) => model.rows.get(index).cloned().map(|row| (index, row)),
            _ => None,
        };
        (focused, row)
    };
    match focused {
        Some(Stop::Image) => {
            app.global::<GameInfoView>().set_viewer_open(true);
            show_viewer(ctx, app);
        }
        Some(Stop::Manual) => crate::manual::open(ctx, app),
        Some(Stop::Row(_)) => {
            let Some((index, row)) = row else { return };
            match row.values.as_slice() {
                [one] => drill(ctx, app, &row.kind, &one.search),
                [] => {}
                _ => open_values(ctx, app, index),
            }
        }
        Some(Stop::Description) | None => {}
    }
}

/// Search the game's system for one of its tags, as the app's tag links
/// do. Back from the results comes back here.
fn drill(ctx: &Ctx, app: &App, kind: &str, search: &str) {
    let (system, entry) = {
        let shared = lock(&ctx.shared);
        let model = &shared.game_info;
        (model.system.clone(), model.entry.clone())
    };
    let Some(entry) = entry else { return };
    let screen = app.global::<crate::Shell>().get_active_screen();
    {
        let mut shared = lock(&ctx.shared);
        let drill = Drill {
            screen,
            games: shared.games.clone(),
            search: shared.persist.search.clone(),
            entry,
            row: kind.to_string(),
            meta: shared.game_info.landed.take(),
            image: shared.game_info.selected,
            menu: crate::router::take_menu_return(&mut shared),
        };
        shared.game_info.drill = Some(drill);
    }
    close(ctx, app);
    crate::search::enter_tag(ctx, app, &system, search);
}

/// Back from a tag search Game Info opened: the screen it was opened on,
/// as it was, and Game Info on the same row. False when no such search is
/// on screen.
pub(crate) fn return_from_drill(ctx: &Ctx, app: &App) -> bool {
    let drill = lock(&ctx.shared).game_info.drill.take();
    let Some(drill) = drill else {
        return false;
    };
    {
        let mut shared = lock(&ctx.shared);
        shared.games.fill_task.cancel();
        shared.games = drill.games;
        shared.persist.search = drill.search;
        shared.persist.active_screen = drill.screen.token().to_string();
        crate::router::restore_menu_return(&mut shared, drill.menu);
    }
    crate::router::save_persist(&ctx.shared);
    if drill.screen == crate::Screen::Hub {
        crate::router::return_to_hub(ctx, app);
    } else {
        crate::router::transition_to_screen(app, drill.screen, -1);
        crate::games::render(ctx, app);
    }
    // The screen and the panel over it change in this one turn.
    let Some((key, meta)) = drill.meta else {
        lock(&ctx.shared).game_info.refocus = Some(drill.row);
        open(ctx, app, &drill.entry);
        return true;
    };
    if prepare(ctx, app, &drill.entry).is_some() {
        {
            let mut shared = lock(&ctx.shared);
            shared.game_info.refocus = Some(drill.row);
            shared.game_info.reselect = drill.image;
        }
        app.global::<GameInfoView>().set_modal_open(true);
        landed(ctx, app, &key, &meta);
    }
    true
}

pub fn handle_action(ctx: &Ctx, app: &App, action: &str) {
    if lock(&ctx.shared).game_info.manual.is_open() {
        crate::manual::handle_action(ctx, app, action);
        return;
    }
    let view = app.global::<GameInfoView>();
    // The panel scrolls to its focus at once while it opens; a move the
    // user makes glides.
    view.set_scroll_snap(false);
    if view.get_viewer_open() {
        viewer_action(ctx, app, action);
        return;
    }
    if view.get_values_open() {
        values_action(ctx, app, action);
        return;
    }
    match action {
        actions::CANCEL => {
            close(ctx, app);
            crate::router::return_to_menu(ctx, app, zaparoo_app::options_menu::Opens::Details);
        }
        actions::ACCEPT => accept(ctx, app),
        actions::LEFT | actions::RIGHT => {
            let delta = if action == actions::LEFT { -1 } else { 1 };
            let changed = lock(&ctx.shared).game_info.cycle(delta);
            if changed {
                show_image(ctx, app);
            }
        }
        actions::UP | actions::DOWN => {
            let down = action == actions::DOWN;
            if app.invoke_game_info_step_within(down) {
                return;
            }
            let moved = {
                let mut shared = lock(&ctx.shared);
                let model = &mut shared.game_info;
                let next = rules::step(model.focus, if down { 1 } else { -1 }, model.stops.len());
                let moved = next != model.focus;
                model.focus = next;
                moved
            };
            if moved {
                publish_focus(ctx, app);
            } else {
                // Nothing to move to that way: read on past the last stop.
                app.invoke_game_info_scroll(if down {
                    crate::ScrollAction::Down
                } else {
                    crate::ScrollAction::Up
                });
            }
        }
        actions::PAGE_PREV => app.invoke_game_info_scroll(crate::ScrollAction::PagePrev),
        actions::PAGE_NEXT => app.invoke_game_info_scroll(crate::ScrollAction::PageNext),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn close_and_reopen_retire_metadata_and_carousel_is_clamped() {
        let mut model = GameInfoModel::default();
        let first = model.reset();
        model.images = ["boxart", "screenshot"]
            .into_iter()
            .map(|kind| MediaKey {
                media_id: None,
                system: "SNES".into(),
                path: "same.sfc".into(),
                max_size: 256,
                fit: zaparoo_app::covers::Fit::SOURCE,
                folder: false,
                image_type: Some(kind.into()),
                prefer: None,
            })
            .collect();
        model.stops = vec![Stop::Image, Stop::Row(1)];
        model.focus = 1;
        assert_eq!(model.focused(), Some(Stop::Row(1)));
        assert_eq!(model.stop_index(Stop::Image), 0);
        assert_eq!(model.stop_index(Stop::Manual), -1);
        assert!(!model.cycle(-1));
        assert!(model.cycle(1));
        assert!(!model.cycle(1));
        assert!(model.cycle(-1));
        model.reset();
        let reopened = model.reset();
        assert_ne!(first, reopened);
        assert!(model.images.is_empty());
        assert!(model.stops.is_empty());
        assert_eq!(model.selected, 0);
    }
}
