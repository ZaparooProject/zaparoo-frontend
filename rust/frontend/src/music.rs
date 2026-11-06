// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// The music player driver. Three pages of one screen: the player, which
// shows what Core's background slot plays with its transport row; the
// open playlist; and the library, which browses Core's Audio index a
// folder at a time and starts a folder as a playlist. Everything playing
// comes from `BackgroundMediaResource`; this driver only sends commands
// and renders. The position between Core's answers comes from
// `zaparoo_app::music::PlaybackClock`, ticked once a second while the
// player is on screen and the music plays. Library reads are ticketed and
// the folder on screen stays until the next one lands, as in the file
// browser. The transport row is pressed through `press_feedback` like
// every other control, and the page actions are the Menu chip row's.

use std::time::Duration;

use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use zaparoo_app::file_browser::{self as fb_rules, Letters};
use zaparoo_app::media_list::{self as list_rules, SelectionPersist};
use zaparoo_app::music::{self as rules, Control, EntryKind, PlaybackClock, Repeat};
use zaparoo_core::input_actions::actions;
use zaparoo_core::media_types::{
    MediaActiveImageParams, MediaBrowseParams, MediaControlParams, PlaylistState,
    PlaylistsUpdateParams, RunParams, UpdateSettingsParams, SLOT_BACKGROUND,
};
use zaparoo_core::persist::MusicState;
use zaparoo_core::store::BackgroundMediaState;

use crate::browse_motion::Rapid;
use crate::music_layout::{layout_for, push_layout, Layout};
use crate::router::{lock, Ctx, Shared};
use crate::{App, ContentState, GridCell, MusicInput, MusicView};

/// Where the player was opened from; Back returns there.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Origin {
    #[default]
    Settings,
    Hub,
}

impl Origin {
    fn from_hub(from_hub: bool) -> Self {
        if from_hub {
            Self::Hub
        } else {
            Self::Settings
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Page {
    #[default]
    Player,
    /// The open playlist, a page of its own.
    Playlist,
    Library,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Fill {
    #[default]
    Ready,
    Empty,
    Error,
}

#[derive(Debug, Clone)]
struct LibRow {
    kind: EntryKind,
    name: String,
    path: String,
}

/// Where Core's music settings stand.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum SettingsState {
    /// Asked for, no answer yet.
    #[default]
    Loading,
    Ready,
    /// Core did not answer; asked again when the Menu opens.
    Failed,
}

/// Core's music settings, `None` where Core does not offer them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Settings {
    state: SettingsState,
    volume: Option<i32>,
    autoplay: Option<bool>,
    pause_games: Option<bool>,
}

impl Settings {
    /// Why a setting row cannot act, as a `Labels.menu` reason key; empty
    /// when it can.
    fn reason(self, offered: bool) -> &'static str {
        match self.state {
            SettingsState::Loading => "music:loading",
            SettingsState::Failed => "music:no_answer",
            SettingsState::Ready if offered => "",
            SettingsState::Ready => "music:not_supported",
        }
    }
}

#[derive(Debug, Default)]
pub struct Model {
    page: Page,
    origin: Origin,
    /// The focused control on the player.
    control: Control,
    /// The focused row on the playlist page.
    row: usize,
    /// The background slot as last heard.
    bg: BackgroundMediaState,
    clock: PlaybackClock,
    playlist_top: usize,
    /// The library folders opened, outermost first; empty at the top.
    stack: Vec<String>,
    /// The selected path and list top at each level, top level first.
    selected: Vec<String>,
    tops: Vec<usize>,
    /// Core lists one route only, so the library starts inside it and
    /// Back from it returns to the player.
    single_route: bool,
    rows: Vec<LibRow>,
    /// The folder's letter buckets, for the rail and a long hold.
    letters: Letters,
    index: usize,
    list_top: usize,
    fill: Fill,
    rapid: Rapid,
    /// The selection write a move scheduled, committed once it rests.
    persist: SelectionPersist,
    settings: Settings,
    /// The track the cover belongs to, the cover, and the art box it was
    /// decoded for.
    cover_path: String,
    cover: Option<slint::SharedPixelBuffer<slint::Rgba8Pixel>>,
    cover_size: i32,
    /// The cover for `cover_path` at `cover_size` has been asked for.
    cover_requested: bool,
    /// Bumped with each slider step, so only the last one a held button
    /// made is sent to Core.
    volume_seq: u64,
    /// A track this player asked for, until Core reports it.
    start: rules::TrackStart,
    /// Bumped with each list row flash, so a late release cannot cut a
    /// newer one.
    press_seq: u64,
    persist_seq: u64,
    activate_pulse: i32,
    release_pulse: i32,
    ticks: u64,
    ticket: u64,
    task: crate::scoped_task::ScopedTask,
    /// A folder read to play it (Options > Play folder): its own ticket,
    /// so leaving the screen drops it.
    play_ticket: u64,
    play_task: crate::scoped_task::ScopedTask,
    cover_task: crate::scoped_task::ScopedTask,
}

thread_local! {
    /// The once-a-second tick that moves the time text and the progress
    /// cells while the music plays. Lives on the event loop; `Shared`
    /// crosses threads and cannot hold a timer.
    static TICK: slint::Timer = slint::Timer::default();
}

/// Milliseconds on a monotonic clock, for the playback clock.
fn now_ms() -> u64 {
    use std::sync::OnceLock;
    static START: OnceLock<std::time::Instant> = OnceLock::new();
    u64::try_from(
        START
            .get_or_init(std::time::Instant::now)
            .elapsed()
            .as_millis(),
    )
    .unwrap_or(u64::MAX)
}

// ---------- State ----------

fn playlist(model: &Model) -> Option<&PlaylistState> {
    model.bg.playlist.as_deref()
}

fn now_of(bg: &BackgroundMediaState) -> rules::Now {
    let track = bg.track.as_ref();
    let playlist = bg.playlist.as_deref();
    rules::Now {
        has_track: track.is_some(),
        playback: rules::Playback::from_core(
            track.is_some(),
            track.map_or("", |t| t.playback_state.as_str()),
        ),
        controls: track
            .map(|t| t.launcher_controls.clone())
            .unwrap_or_default(),
        has_playlist: playlist.is_some(),
        playlist_playing: playlist.is_some_and(|p| p.playing),
        playlist_total: playlist.map_or(0, |p| p.total.max(p.items.len())),
        can_update_playlist: playlist.is_some_and(|p| p.shuffle.is_some()),
        shuffle: playlist.and_then(|p| p.shuffle).unwrap_or(false),
        repeat: playlist.map_or(Repeat::default(), |p| Repeat::from_core(&p.repeat)),
        starting: false,
    }
}

/// The slot as the player sees it: Core's state, and whether a track this
/// player asked for is still starting.
fn now(model: &Model) -> rules::Now {
    rules::Now {
        starting: model.start.pending(),
        ..now_of(&model.bg)
    }
}

fn track_path(model: &Model) -> Option<&str> {
    model.bg.track.as_ref().map(|t| t.media_path.as_str())
}

/// A request that starts a track went out: the controls that launch wait
/// until Core reports the new one, or `START_TIMEOUT_MS` passes.
fn begin_start(ctx: &Ctx, app: &App) {
    let seq = {
        let mut shared = lock(&ctx.shared);
        let model = &mut shared.music;
        let current = track_path(model).map(str::to_string);
        model.start.begin(current.as_deref())
    };
    let ctx = ctx.clone();
    let weak = app.as_weak();
    slint::Timer::single_shot(Duration::from_millis(rules::START_TIMEOUT_MS), move || {
        let ended = lock(&ctx.shared).music.start.end(Some(seq));
        if ended {
            if let Some(app) = weak.upgrade() {
                refresh(&ctx, &app);
            }
        }
    });
}

/// The current track's place in the open playlist.
fn current_track(model: &Model) -> usize {
    playlist(model).map_or(0, |p| p.index)
}

fn playlist_len(model: &Model) -> usize {
    playlist(model).map_or(0, |p| p.items.len())
}

// ---------- Geometry ----------

fn layout(ctx: &Ctx, app: &App) -> Layout {
    let inputs = crate::router::output_scene(app).inputs();
    let (rotated, page) = {
        let shared = lock(&ctx.shared);
        (crate::games::rotated(&shared), shared.music.page)
    };
    layout_for(&inputs, rotated, page != Page::Playlist)
}

// ---------- Rendering ----------

fn playback_of(now: &rules::Now) -> crate::PlaybackState {
    if now.playing() {
        crate::PlaybackState::Playing
    } else if now.has_track {
        crate::PlaybackState::Paused
    } else {
        crate::PlaybackState::Stopped
    }
}

fn control_of(control: Control) -> crate::MusicControl {
    match control {
        Control::Shuffle => crate::MusicControl::Shuffle,
        Control::Previous => crate::MusicControl::Previous,
        Control::PlayPause => crate::MusicControl::PlayPause,
        Control::Next => crate::MusicControl::Next,
        Control::Repeat => crate::MusicControl::Repeat,
    }
}

fn unavailable_of(why: rules::Unavailable) -> crate::MusicUnavailable {
    match why {
        rules::Unavailable::None => crate::MusicUnavailable::None,
        rules::Unavailable::NothingPlaying => crate::MusicUnavailable::NothingPlaying,
        rules::Unavailable::NoPlaylist => crate::MusicUnavailable::NoPlaylist,
        rules::Unavailable::NotSupported => crate::MusicUnavailable::NotSupported,
        rules::Unavailable::Starting => crate::MusicUnavailable::Starting,
    }
}

fn repeat_of(repeat: Repeat) -> crate::RepeatMode {
    match repeat {
        Repeat::Off => crate::RepeatMode::Off,
        Repeat::All => crate::RepeatMode::All,
        Repeat::One => crate::RepeatMode::One,
    }
}

/// The time text and the progress cells, from the clock.
fn publish_clock(view: &MusicView<'_>, model: &Model) {
    let now = now_ms();
    let position = model.clock.position_at(now);
    let duration = model.clock.duration();
    let text = SharedString::from(rules::clock_text(position));
    if view.get_position_text() != text {
        view.set_position_text(text);
    }
    let milli = rules::progress_milli(position, duration);
    if view.get_progress_milli() != milli {
        view.set_progress_milli(milli);
    }
    let text = SharedString::from(duration.map(rules::clock_text).unwrap_or_default());
    if view.get_duration_text() != text {
        view.set_duration_text(text);
    }
}

/// A playlist item's name, else its file's name without the extension,
/// as a games row names a file.
fn track_name(item_name: &str, script: &str) -> String {
    list_rules::display_title(item_name, script, false)
}

fn library_cell(row: &LibRow) -> GridCell {
    GridCell {
        name: SharedString::from(row.name.as_str()),
        opens: row.kind != EntryKind::Track,
        ..Default::default()
    }
}

#[allow(
    clippy::too_many_lines,
    reason = "one pass publishes the card, the transport row and whichever list the page shows"
)]
pub(crate) fn render(ctx: &Ctx, app: &App) {
    let geometry = layout(ctx, app);
    let mut shared = lock(&ctx.shared);
    let view = app.global::<MusicView>();
    push_layout(&view, &geometry);
    let visible = geometry.list.visible_rows.max(1);
    let shared_ref = &*shared;
    let model = &shared_ref.music;
    let now = now(model);
    view.set_page(match model.page {
        Page::Player => crate::MusicPage::Player,
        Page::Playlist => crate::MusicPage::Playlist,
        Page::Library => crate::MusicPage::Library,
    });
    view.set_focus_ready(true);

    // Now playing.
    let track = model.bg.track.as_ref();
    let tags = track.and_then(|t| t.track.as_ref());
    view.set_has_track(track.is_some());
    let title = tags
        .map(|t| t.title.clone())
        .filter(|t| !t.is_empty())
        .or_else(|| track.map(|t| t.media_name.clone()))
        .unwrap_or_default();
    view.set_title(SharedString::from(title));
    view.set_artist(SharedString::from(
        tags.map(|t| t.artist.as_str()).unwrap_or_default(),
    ));
    view.set_album(SharedString::from(
        tags.map(|t| t.album.as_str()).unwrap_or_default(),
    ));
    let list = playlist(model);
    view.set_playlist_name(SharedString::from(
        list.map(|p| p.name.as_str()).unwrap_or_default(),
    ));
    view.set_track_number(list.map_or(0, |p| i32::try_from(p.index + 1).unwrap_or(0)));
    view.set_track_total(list.map_or(0, |p| {
        i32::try_from(p.total.max(p.items.len())).unwrap_or(0)
    }));
    let next = list
        .and_then(|p| {
            rules::up_next(p.index, p.items.len(), now.repeat).and_then(|i| p.items.get(i))
        })
        .map(|item| track_name(&item.name, &item.zap_script))
        .unwrap_or_default();
    view.set_up_next(SharedString::from(next));
    view.set_playback(playback_of(&now));
    view.set_shuffle(now.shuffle);
    view.set_repeat(repeat_of(now.repeat));
    match &model.cover {
        Some(buffer)
            if model.cover_path == track.map(|t| t.media_path.as_str()).unwrap_or_default() =>
        {
            view.set_cover(slint::Image::from_rgba8(buffer.clone()));
            view.set_has_cover(true);
        }
        _ => view.set_has_cover(false),
    }
    publish_clock(&view, model);
    let buttons: Vec<crate::MusicButton> = rules::CONTROLS
        .iter()
        .map(|control| crate::MusicButton {
            control: control_of(*control),
            unavailable: unavailable_of(rules::unavailable(*control, &now)),
        })
        .collect();
    let same = slint::Model::row_count(&view.get_buttons()) == buttons.len()
        && buttons
            .iter()
            .enumerate()
            .all(|(i, b)| slint::Model::row_data(&view.get_buttons(), i).as_ref() == Some(b));
    if !same {
        view.set_buttons(ModelRc::new(VecModel::from(buttons)));
    }
    view.set_control_index(i32::try_from(model.control.index()).unwrap_or(2));
    view.set_activate_pulse(model.activate_pulse);
    view.set_release_pulse(model.release_pulse);

    let top = match model.page {
        Page::Player => {
            view.set_state(ContentState::Ready);
            model.playlist_top
        }
        Page::Playlist => {
            let items = list.map(|p| p.items.as_slice()).unwrap_or_default();
            let current_track = current_track(model);
            let count = items.len();
            let current = model.row.min(count.saturating_sub(1));
            view.set_state(ContentState::Ready);
            view.set_count(i32::try_from(count).unwrap_or(0));
            view.set_current_index(i32::try_from(current).unwrap_or(0));
            let paging = list_rules::list_paging(current, count, Some(count), true, visible, false);
            view.set_list_total_pages(i32::try_from(paging.total_pages).unwrap_or(1));
            view.set_has_items_above(paging.has_items_above);
            view.set_has_items_below(paging.has_items_below);
            let indexed: Vec<(usize, &zaparoo_core::media_types::PlaylistItem)> =
                items.iter().enumerate().collect();
            crate::browse_motion::publish_list(
                &view,
                &shared_ref.input,
                current,
                count,
                visible,
                model.playlist_top,
                &indexed,
                |(i, item)| GridCell {
                    name: SharedString::from(track_name(&item.name, &item.zap_script)),
                    now_playing: *i == current_track,
                    ..Default::default()
                },
            )
        }
        Page::Library => {
            view.set_state(match model.fill {
                Fill::Ready => ContentState::Ready,
                Fill::Empty => ContentState::Empty,
                Fill::Error => ContentState::Error,
            });
            view.set_library_top(
                model.stack.is_empty() || (model.single_route && model.stack.len() == 1),
            );
            view.set_library_title(SharedString::from(
                model
                    .stack
                    .last()
                    .map(|p| list_rules::folder_name_for_path(p))
                    .unwrap_or_default(),
            ));
            let count = model.rows.len();
            let current = model.index.min(count.saturating_sub(1));
            let focused = model.rows.get(current);
            view.set_focused_opens(focused.is_some_and(|r| r.kind != EntryKind::Track));
            view.set_count(i32::try_from(count).unwrap_or(0));
            view.set_current_index(i32::try_from(current).unwrap_or(0));
            let paging = list_rules::list_paging(current, count, Some(count), true, visible, false);
            view.set_list_total_pages(i32::try_from(paging.total_pages).unwrap_or(1));
            view.set_has_items_above(paging.has_items_above);
            view.set_has_items_below(paging.has_items_below);
            publish_rail(&view, model, current);
            crate::browse_motion::publish_list(
                &view,
                &shared_ref.input,
                current,
                count,
                visible,
                model.list_top,
                &model.rows,
                library_cell,
            )
        }
    };
    let surface = surface_of(model.page);
    crate::page_bar::publish_music(app, shared_ref, surface);
    match shared.music.page {
        Page::Player | Page::Playlist => shared.music.playlist_top = top,
        Page::Library => shared.music.list_top = top,
    }
}

/// The rail: the folder's letters with the current one, and the position.
/// A folder's listing is whole, so its total holds still.
fn publish_rail(view: &MusicView<'_>, model: &Model, index: usize) {
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
    view.set_rail_visible(
        model.rapid.rail_visible && list_rules::rail_offered(Some(model.rows.len())),
    );
}

/// A folder's letter buckets: routes and folders lead, tracks are
/// bucketed by their first letter, as the file browser buckets files.
fn letters_of(rows: &[LibRow]) -> Letters {
    let entries: Vec<fb_rules::Entry> = rows
        .iter()
        .map(|row| fb_rules::Entry {
            kind: if row.kind == EntryKind::Track {
                fb_rules::EntryKind::File
            } else {
                fb_rules::EntryKind::Folder
            },
            name: row.name.clone(),
        })
        .collect();
    fb_rules::letters(&entries)
}

/// The page on show, as far as its Menu chips go.
fn surface_of(page: Page) -> zaparoo_app::page_bar::Surface {
    use zaparoo_app::page_bar::Surface;
    match page {
        Page::Player => Surface::MusicPlayer,
        Page::Library => Surface::MusicLibrary,
        Page::Playlist => Surface::Other,
    }
}

/// The chips of the page on show, with their current values. A chip the
/// slot cannot take right now is left off: a chip is never drawn disabled.
pub(crate) fn chip_entries(
    shared: &Shared,
    surface: zaparoo_app::page_bar::Surface,
) -> Vec<crate::MenuEntry> {
    use zaparoo_app::page_bar::Chip;
    let model = &shared.music;
    let now = now(model);
    let has_tracks = model.rows.iter().any(|r| r.kind == EntryKind::Track);
    zaparoo_app::page_bar::chips(surface)
        .iter()
        .filter_map(|&chip| match chip {
            Chip::MusicPlaylist if !now.has_playlist => None,
            Chip::StopMusic if rules::stop_command(&now).is_none() => None,
            Chip::MusicVolume => model
                .settings
                .volume
                .filter(|_| model.settings.state == SettingsState::Ready)
                .map(|v| crate::router::menu_row_keyed(chip.id(), "music_volume", &v.to_string())),
            Chip::PlayAll | Chip::ShuffleAll if !has_tracks || model.fill != Fill::Ready => None,
            _ => Some(crate::router::menu_row(chip.id())),
        })
        .collect()
}

fn on_screen(app: &App) -> bool {
    app.global::<crate::Shell>().get_active_screen() == crate::Screen::Music
}

fn refresh(ctx: &Ctx, app: &App) {
    if on_screen(app) {
        render(ctx, app);
    }
    update_tick(ctx, app);
}

// ---------- Live state ----------

/// Follow the background slot: the header's music note everywhere, and the
/// player while it is on screen.
pub fn bind_background(ctx: &Ctx, app: &App) {
    let resource = ctx.store.background_media();
    let mut rx = resource.subscribe();
    let weak = app.as_weak();
    let ctx = ctx.clone();
    ctx.handle.clone().spawn(async move {
        loop {
            let state = rx.borrow_and_update().clone();
            let ctx2 = ctx.clone();
            if weak
                .upgrade_in_event_loop(move |app| background_changed(&ctx2, &app, state))
                .is_err()
            {
                return;
            }
            if rx.changed().await.is_err() {
                return;
            }
        }
    });
}

pub(crate) fn background_changed(ctx: &Ctx, app: &App, state: BackgroundMediaState) {
    tracing::debug!(
        track = state.track.as_ref().map_or("", |t| t.media_name.as_str()),
        index = state.playlist.as_ref().map(|p| p.index),
        playing = state.playing(),
        on_screen = on_screen(app),
        "music: the background slot changed"
    );
    app.global::<crate::Shell>()
        .set_music_playing(state.playing());
    let fetch_cover = {
        let mut shared = lock(&ctx.shared);
        let model = &mut shared.music;
        let now = now_of(&state);
        let track = state.track.as_ref();
        // Only an answer about the track moves the clock: a playlist change
        // carries the track as last heard, and syncing to that would jump
        // the time back.
        let clock_changed =
            model.bg.track.as_ref() != track || now_of(&model.bg).playing() != now.playing();
        if clock_changed {
            model.clock.sync(
                track
                    .and_then(|t| t.position_ms)
                    .and_then(|v| u64::try_from(v).ok()),
                track
                    .and_then(|t| t.duration_ms)
                    .and_then(|v| u64::try_from(v).ok()),
                now.playing(),
                now_ms(),
            );
        }
        let path = track.map(|t| t.media_path.clone()).unwrap_or_default();
        let has_art = track
            .and_then(|t| t.track.as_ref())
            .is_some_and(|t| t.has_art);
        let new_track = path != model.cover_path;
        if new_track {
            model.cover_path.clone_from(&path);
            model.cover = None;
            model.cover_size = 0;
            model.cover_task.cancel();
            model.cover_requested = false;
        }
        // Core reads a track's tags just after it starts, so `hasArt` can
        // arrive on a later answer than the track itself.
        let fetch = has_art && !path.is_empty() && model.cover.is_none() && !model.cover_requested;
        if fetch {
            model.cover_requested = true;
        }
        // A new playlist seats the playlist focus on its current track.
        let changed_list = model.bg.playlist.as_ref().map(|p| p.id.clone())
            != state.playlist.as_ref().map(|p| p.id.clone());
        model.bg = state;
        // The track this player asked for has started: the controls that
        // launch come back.
        let current = track_path(model).map(str::to_string);
        model.start.heard(current.as_deref());
        let rows = playlist_len(model);
        if changed_list {
            model.row = current_track(model);
            model.playlist_top = 0;
        }
        model.row = model.row.min(rows.saturating_sub(1));
        // Core reads a new track's tags just after it starts. A Core that
        // pushes `media.playback` sends them when they land; an older one
        // is asked again shortly rather than showing the file name until
        // the next resync.
        let untagged = new_track
            && !model.bg.pushes_playback
            && !path.is_empty()
            && model.bg.track.as_ref().is_some_and(|t| t.track.is_none());
        (fetch.then_some(path), untagged)
    };
    let (fetch_cover, untagged) = fetch_cover;
    if let Some(path) = fetch_cover {
        fetch_cover_art(ctx, app, path);
    }
    if untagged {
        let ctx = ctx.clone();
        slint::Timer::single_shot(Duration::from_millis(TAGS_RETRY_MS), move || {
            resync_now(&ctx);
        });
    }
    refresh(ctx, app);
}

/// Ask Core for the cover at the art box's size. The box changes with the
/// scene, and a cover decoded for another box would be resampled.
fn fetch_cover_art(ctx: &Ctx, app: &App, path: String) {
    let size = layout(ctx, app).player.art_size.max(1);
    lock(&ctx.shared).music.cover_size = size;
    let client = ctx.store.client();
    let weak = app.as_weak();
    let ctx2 = ctx.clone();
    let task = ctx.handle.spawn(async move {
        let Ok(result) = client
            .media_active_image(MediaActiveImageParams {
                slot: SLOT_BACKGROUND.to_string(),
                max_size: u32::try_from(size).ok(),
            })
            .await
        else {
            return;
        };
        let fit = zaparoo_app::covers::Fit::new(size, size);
        let decoded = tokio::task::spawn_blocking(move || {
            crate::media_cache::decode_inline(&result.data, fit)
        })
        .await
        .ok()
        .flatten();
        let Some(decoded) = decoded else {
            return;
        };
        let _ = weak.upgrade_in_event_loop(move |app| {
            {
                let mut shared = lock(&ctx2.shared);
                if shared.music.cover_path != path || shared.music.cover_size != size {
                    return;
                }
                shared.music.cover = Some(decoded.buffer);
            }
            refresh(&ctx2, &app);
        });
    });
    lock(&ctx.shared).music.cover_task.replace(task);
}

/// Start or stop the once-a-second tick: only while the player page is on
/// screen and the music plays. A tick under the screensaver or while the
/// frontend sleeps behind a game does nothing.
fn update_tick(ctx: &Ctx, app: &App) {
    let wanted = {
        let shared = lock(&ctx.shared);
        let model = &shared.music;
        rules::should_poll(
            on_screen(app) && model.page == Page::Player,
            true,
            now_of(&model.bg).playing(),
        )
    };
    TICK.with(|timer| {
        if !wanted {
            timer.stop();
            return;
        }
        if timer.running() {
            return;
        }
        let weak = app.as_weak();
        let ctx = ctx.clone();
        timer.start(
            slint::TimerMode::Repeated,
            Duration::from_millis(rules::TICK_MS),
            move || {
                let Some(app) = weak.upgrade() else {
                    return;
                };
                // The player was left by a route of its own (a held Back
                // to the Hub): the tick ends with it.
                if !on_screen(&app) || lock(&ctx.shared).music.page != Page::Player {
                    TICK.with(slint::Timer::stop);
                    return;
                }
                let shell = app.global::<crate::Shell>();
                if shell.get_dormant() || shell.get_saver_armed() {
                    return;
                }
                let resync = {
                    let mut shared = lock(&ctx.shared);
                    shared.music.ticks += 1;
                    let view = app.global::<MusicView>();
                    publish_clock(&view, &shared.music);
                    // Core's own notifications keep a pushing Core current;
                    // asking now and then is only the backup.
                    let every = if shared.music.bg.pushes_playback {
                        rules::BACKUP_RESYNC_MS
                    } else {
                        rules::RESYNC_MS
                    };
                    shared.music.ticks.is_multiple_of(every / rules::TICK_MS)
                };
                if resync {
                    resync_now(&ctx);
                }
            },
        );
    });
}

fn resync_now(ctx: &Ctx) {
    let resource = ctx.store.background_media();
    ctx.handle.spawn(async move {
        if let Err(e) = resource.resync().await {
            tracing::debug!("music: resync failed: {}", e.message);
        }
    });
}

fn resync_soon(ctx: &Ctx) {
    let ctx = ctx.clone();
    slint::Timer::single_shot(Duration::from_millis(rules::AFTER_ACTION_MS), move || {
        resync_now(&ctx);
    });
}

// ---------- Commands ----------

fn report(ctx: &Ctx, app: &App, context: &str) {
    crate::router::report_action_error(ctx, app, "music", context);
}

/// Send a command to Core; a failure is reported once it lands.
fn send(ctx: &Ctx, app: &App, command: rules::Command) {
    let client = ctx.store.client();
    let weak = app.as_weak();
    let ctx2 = ctx.clone();
    ctx.handle.spawn(async move {
        let result = match command {
            rules::Command::Run(text) => client.run(RunParams { text }).await,
            rules::Command::Control(action) => {
                client
                    .media_control(MediaControlParams {
                        action: action.to_string(),
                        slot: SLOT_BACKGROUND.to_string(),
                        args: std::collections::BTreeMap::new(),
                    })
                    .await
            }
            rules::Command::Update { shuffle, repeat } => {
                client
                    .playlists_update(PlaylistsUpdateParams {
                        slot: SLOT_BACKGROUND.to_string(),
                        shuffle,
                        repeat: repeat.map(|r| r.core_token().to_string()),
                    })
                    .await
            }
        };
        if let Err(e) = result {
            tracing::warn!("music: command failed: {}", e.message);
            let _ = weak.upgrade_in_event_loop(move |app| {
                lock(&ctx2.shared).music.start.end(None);
                refresh(&ctx2, &app);
                report(&ctx2, &app, "");
            });
        }
    });
    resync_soon(ctx);
}

/// Run a transport control. The router has already pushed the button
/// through `press_feedback`; a control that cannot act has no press target
/// and lands here to do nothing.
fn press(ctx: &Ctx, app: &App, control: Control) {
    let command = rules::transport_command(control, &now(&lock(&ctx.shared).music));
    let Some(command) = command else {
        return;
    };
    if matches!(control, Control::Previous | Control::Next) {
        begin_start(ctx, app);
    }
    // A pause shows at once; Core confirms it on the next resync.
    if control == Control::PlayPause {
        let mut shared = lock(&ctx.shared);
        if now(&shared.music).playing() {
            shared.music.clock.hold(now_ms());
        }
    }
    send(ctx, app, command);
    render(ctx, app);
}

/// Skip with L and R: the playlist's own previous and next.
fn skip(ctx: &Ctx, app: &App, forward: bool) {
    let control = if forward {
        Control::Next
    } else {
        Control::Previous
    };
    let command = rules::transport_command(control, &now(&lock(&ctx.shared).music));
    if let Some(command) = command {
        begin_start(ctx, app);
        send(ctx, app, command);
    }
}

// ---------- Entry, persistence and leaving ----------

fn write_persist(shared: &mut Shared) {
    let model = &shared.music;
    let items = playlist(model)
        .map(|p| p.items.as_slice())
        .unwrap_or_default();
    shared.persist.music = MusicState {
        page: match model.page {
            Page::Player => "player".into(),
            Page::Playlist => "playlist".into(),
            Page::Library => "library".into(),
        },
        control: model.control.as_str().into(),
        playlist_index: model.row,
        playlist_item: items
            .get(model.row)
            .map(|i| i.zap_script.clone())
            .unwrap_or_default(),
        entered_from_hub: model.origin == Origin::Hub,
        library_path_stack: model.stack.clone(),
        library_selected_at_level: {
            let mut selected = model.selected.clone();
            if model.page == Page::Library {
                let level = model.stack.len();
                selected.resize(level + 1, String::new());
                selected[level] = model
                    .rows
                    .get(model.index)
                    .map(|r| r.path.clone())
                    .unwrap_or_default();
            }
            selected
        },
        library_list_top_at_level: {
            let mut tops = model.tops.clone();
            if model.page == Page::Library {
                let level = model.stack.len();
                tops.resize(level + 1, 0);
                tops[level] = model.list_top;
            }
            tops
        },
    };
    shared.persist.active_screen = crate::Screen::Music.token().to_string();
}

/// What a focus move changes, as one key for `SelectionPersist`.
fn selection_key(model: &Model) -> String {
    match model.page {
        Page::Player => format!("player:{}", model.control.as_str()),
        Page::Playlist => format!("playlist:{}", model.row),
        Page::Library => format!(
            "library:{}",
            model
                .rows
                .get(model.index)
                .map(|r| r.path.as_str())
                .unwrap_or_default()
        ),
    }
}

/// A focus move landed: write it once it rests (`PERSIST_DEBOUNCE_MS`).
fn schedule_persist(ctx: &Ctx) {
    let seq = {
        let mut shared = lock(&ctx.shared);
        let model = &mut shared.music;
        let key = selection_key(model);
        if !model.persist.schedule(&key) {
            return;
        }
        model.persist_seq += 1;
        model.persist_seq
    };
    let ctx = ctx.clone();
    slint::Timer::single_shot(
        Duration::from_millis(list_rules::PERSIST_DEBOUNCE_MS),
        move || {
            if lock(&ctx.shared).music.persist_seq == seq {
                flush_persist(&ctx);
            }
        },
    );
}

/// Commit a scheduled selection write now (Accept, Back, hold release);
/// nothing to write when no move is pending.
fn flush_persist(ctx: &Ctx) {
    {
        let mut shared = lock(&ctx.shared);
        shared.music.persist_seq += 1;
        let pending = shared.music.persist.flush().is_some();
        if !pending || shared.persist.active_screen != crate::Screen::Music.token() {
            return;
        }
        write_persist(&mut shared);
    }
    crate::router::save_persist(&ctx.shared);
}

/// The music screen's selection write, flushed when a held key is let go.
pub(crate) fn flush_selection(ctx: &Ctx) {
    flush_persist(ctx);
}

/// Open the player from Settings or a pinned Hub tile.
pub(crate) fn enter(ctx: &Ctx, app: &App, origin: Origin) {
    {
        let mut shared = lock(&ctx.shared);
        let bg = ctx.store.background_media().current();
        let model = &mut shared.music;
        model.task.cancel();
        model.page = Page::Player;
        model.origin = origin;
        model.bg = bg;
        model.control = Control::default();
        model.row = current_track(model);
        model.playlist_top = 0;
        model.persist.discard();
        write_persist(&mut shared);
    }
    crate::router::save_persist(&ctx.shared);
    fetch_settings(ctx, app);
    render(ctx, app);
    crate::router::transition_to_screen(app, crate::Screen::Music, 1);
    update_tick(ctx, app);
}

/// Cold start on the player: the same page, control and row. The player
/// waits under the curtain for Core's first answer about the background
/// slot; the library waits for its folder.
pub(crate) fn restore(ctx: &Ctx, app: &App) {
    let saved = lock(&ctx.shared).persist.music.clone();
    {
        let mut shared = lock(&ctx.shared);
        let model = &mut shared.music;
        model.origin = Origin::from_hub(saved.entered_from_hub);
        model.control = Control::parse(&saved.control);
        model.row = saved.playlist_index;
        model.stack.clone_from(&saved.library_path_stack);
        model.selected.clone_from(&saved.library_selected_at_level);
        model.tops.clone_from(&saved.library_list_top_at_level);
        model.persist.discard();
    }
    fetch_settings(ctx, app);
    if saved.page == "library" {
        let inside = !saved.library_path_stack.is_empty();
        let seat = saved
            .library_selected_at_level
            .get(saved.library_path_stack.len())
            .cloned();
        open_library_at(ctx, app, inside, seat);
        return;
    }
    crate::router::begin_pending(app, crate::Screen::Music);
    let resource = ctx.store.background_media();
    let mut rx = resource.subscribe();
    let weak = app.as_weak();
    let ctx2 = ctx.clone();
    let task = ctx.handle.spawn(async move {
        loop {
            let seeded = { rx.borrow_and_update().seeded };
            if seeded {
                break;
            }
            if rx.changed().await.is_err() {
                return;
            }
        }
        let state = { rx.borrow().clone() };
        let _ = weak.upgrade_in_event_loop(move |app| restore_landed(&ctx2, &app, state));
    });
    lock(&ctx.shared).music.task.replace(task);
}

/// Core's first answer about the background slot reached a restore that is
/// still waiting: seat the saved row and show the player.
pub(crate) fn restore_landed(ctx: &Ctx, app: &App, state: BackgroundMediaState) {
    let shell = app.global::<crate::Shell>();
    if !shell.get_transitioning() || shell.get_transition_target() != crate::Screen::Music {
        return;
    }
    {
        let mut shared = lock(&ctx.shared);
        let saved = shared.persist.music.clone();
        let model = &mut shared.music;
        model.bg = state;
        let items: Vec<String> = playlist(model)
            .map(|p| p.items.iter().map(|i| i.zap_script.clone()).collect())
            .unwrap_or_default();
        model.row = rules::reseat_row(
            saved.playlist_index,
            &saved.playlist_item,
            &items,
            current_track(model),
        );
        // The playlist page comes back only while a playlist is open.
        model.page = if saved.page == "playlist" && !items.is_empty() {
            Page::Playlist
        } else {
            Page::Player
        };
    }
    render(ctx, app);
    crate::router::transition_to_screen(app, crate::Screen::Music, 1);
    update_tick(ctx, app);
}

/// A restore that never finished: drop its fill.
pub(crate) fn abandon(ctx: &Ctx) {
    let mut shared = lock(&ctx.shared);
    shared.music.task.cancel();
    shared.music.ticket = shared.music.ticket.wrapping_add(1);
}

/// Where a restore that timed out lands: the screen Back would return to.
pub(crate) fn restore_fallback_is_settings(shared: &Shared) -> bool {
    shared.persist.active_screen == crate::Screen::Music.token()
        && !shared.persist.music.entered_from_hub
}

/// A restore that timed out lands on Settings, seated on this tile. The
/// saved state keeps naming the player, so the next start tries it again.
pub(crate) fn land_on_settings(ctx: &Ctx, app: &App) {
    crate::settings::show_return(
        ctx,
        app,
        crate::SettingsPage::Root,
        zaparoo_app::settings::MUSIC_PAGE,
    );
}

fn leave(ctx: &Ctx, app: &App) {
    flush_persist(ctx);
    let origin = {
        let mut shared = lock(&ctx.shared);
        shared.music.task.cancel();
        drop_play_read(&mut shared.music);
        shared.music.rows.clear();
        shared.music.stack.clear();
        shared.persist.music = MusicState::default();
        shared.music.origin
    };
    TICK.with(slint::Timer::stop);
    match origin {
        Origin::Settings => {
            lock(&ctx.shared).persist.active_screen = crate::Screen::Settings.token().to_string();
            crate::router::save_persist(&ctx.shared);
            crate::settings::show_return(
                ctx,
                app,
                crate::SettingsPage::Root,
                zaparoo_app::settings::MUSIC_PAGE,
            );
        }
        Origin::Hub => {
            lock(&ctx.shared).persist.active_screen = crate::Screen::Hub.token().to_string();
            crate::router::save_persist(&ctx.shared);
            crate::router::return_to_hub(ctx, app);
        }
    }
}

// ---------- Settings ----------

/// Ask Core for its music settings. Until it answers the setting rows say
/// so; if it never does they say that instead, and opening the Menu asks
/// again.
fn fetch_settings(ctx: &Ctx, app: &App) {
    {
        let mut shared = lock(&ctx.shared);
        if shared.music.settings.state != SettingsState::Ready {
            shared.music.settings.state = SettingsState::Loading;
        }
    }
    let client = ctx.store.client();
    let weak = app.as_weak();
    let ctx2 = ctx.clone();
    ctx.handle.spawn(async move {
        let settings = match client.settings().await {
            Ok(result) => Settings {
                state: SettingsState::Ready,
                volume: result.audio_music_volume,
                autoplay: result.audio_music_autoplay,
                pause_games: result.audio_music_pause_on_launch,
            },
            Err(e) => {
                tracing::warn!("music: cannot read Core's settings: {}", e.message);
                Settings {
                    state: SettingsState::Failed,
                    ..Settings::default()
                }
            }
        };
        let _ = weak.upgrade_in_event_loop(move |app| {
            {
                let mut shared = lock(&ctx2.shared);
                // A failed refresh keeps values Core already gave.
                if settings.state == SettingsState::Failed
                    && shared.music.settings.state == SettingsState::Ready
                {
                    return;
                }
                shared.music.settings = settings;
            }
            menu_changed(&ctx2, &app);
            if on_screen(&app) {
                render(&ctx2, &app);
            }
        });
    });
}

fn save_setting(ctx: &Ctx, app: &App, params: UpdateSettingsParams) {
    let client = ctx.store.client();
    let weak = app.as_weak();
    let ctx2 = ctx.clone();
    ctx.handle.spawn(async move {
        let result = client.settings_update(params).await;
        let _ = weak.upgrade_in_event_loop(move |app| {
            if let Err(e) = result {
                tracing::warn!("music: saving a setting failed: {}", e.message);
                crate::router::report_action_error(&ctx2, &app, "setting", "");
            }
            fetch_settings(&ctx2, &app);
        });
    });
}

/// The volume as a slider row in the list panel: Left and Right adjust it
/// in place. Muted, with its reason, until Core has told us its volume.
fn slider_row(settings: Settings) -> crate::MenuEntry {
    let reason = settings.reason(settings.volume.is_some());
    let mut entry = crate::router::menu_row_full(
        "music_volume",
        "music_volume",
        &settings.volume.unwrap_or(0).to_string(),
        reason.is_empty(),
        reason,
    );
    entry.role = crate::MenuRole::Slider;
    entry.detail = SharedString::from(settings.volume.unwrap_or(0).to_string());
    entry
}

/// The player's two settings toggles, behind More on the chip row and at
/// the end of the Menu list.
fn toggle_entries(settings: Settings) -> Vec<crate::MenuEntry> {
    let switch = |id: &str, value: Option<bool>| {
        let reason = settings.reason(value.is_some());
        crate::router::with_role(
            crate::router::menu_row_full(id, id, "", reason.is_empty(), reason),
            if value.unwrap_or(false) {
                crate::MenuRole::ToggleOn
            } else {
                crate::MenuRole::ToggleOff
            },
        )
    };
    vec![
        switch("music_autoplay", settings.autoplay),
        switch("music_pause_games", settings.pause_games),
    ]
}

/// The player's Menu list: the open playlist, choosing music and stopping
/// it, then the music settings. It is what the chip row offers, for the
/// scenes without one. A row that cannot act stays, muted, and says why.
fn player_menu_entries(settings: Settings, now: &rules::Now) -> Vec<crate::MenuEntry> {
    let when = |id: &str, ok: bool, reason: &str| {
        crate::router::menu_row_full(id, id, "", ok, if ok { "" } else { reason })
    };
    let mut rows = vec![
        when("music_playlist", now.has_playlist, "music:no_playlist"),
        crate::router::menu_row("music_choose"),
        when(
            "music_stop",
            rules::stop_command(now).is_some(),
            "music:nothing_playing",
        ),
        slider_row(settings),
    ];
    rows.extend(toggle_entries(settings));
    rows
}

/// Core's settings answered: an open Menu or More list shows the new
/// values in place.
fn menu_changed(ctx: &Ctx, app: &App) {
    let entries = {
        let shared = lock(&ctx.shared);
        if !app.global::<crate::Overlays>().get_list_open() {
            return;
        }
        match shared.list_context {
            crate::router::ListContext::MusicMenu => {
                player_menu_entries(shared.music.settings, &now(&shared.music))
            }
            crate::router::ListContext::MusicMore => toggle_entries(shared.music.settings),
            crate::router::ListContext::MusicVolume => vec![slider_row(shared.music.settings)],
            _ => return,
        }
    };
    let overlays = app.global::<crate::Overlays>();
    let index = overlays.get_list_index();
    // The same rows with new values are written in place, so a toggle's
    // knob slides to its new side instead of the list being rebuilt.
    let current = overlays.get_list_entries();
    let same_rows = slint::Model::row_count(&current) == entries.len()
        && entries
            .iter()
            .enumerate()
            .all(|(i, e)| slint::Model::row_data(&current, i).is_some_and(|c| c.id == e.id));
    if same_rows {
        for (i, entry) in entries.into_iter().enumerate() {
            if slint::Model::row_data(&current, i).as_ref() != Some(&entry) {
                slint::Model::set_row_data(&current, i, entry);
            }
        }
        return;
    }
    overlays.set_list_entries(ModelRc::new(VecModel::from(entries)));
    overlays.set_list_index(index);
}

/// The Menu button on the player: the chip row where the scene draws one,
/// else the Menu list.
fn open_player_menu(ctx: &Ctx, app: &App) {
    if crate::page_bar::enter(ctx, app) {
        return;
    }
    if lock(&ctx.shared).music.settings.state == SettingsState::Failed {
        fetch_settings(ctx, app);
    }
    // Built before the list opens: `present_list` takes the shared lock.
    let entries = menu_entries_now(ctx);
    crate::router::present_list(
        ctx,
        app,
        crate::router::ListContext::MusicMenu,
        "title:menu",
        entries,
    );
}

/// The More chip: the player's settings toggles.
pub(crate) fn open_more(ctx: &Ctx, app: &App) {
    if lock(&ctx.shared).music.settings.state == SettingsState::Failed {
        fetch_settings(ctx, app);
    }
    let entries = toggle_entries(lock(&ctx.shared).music.settings);
    crate::router::present_list(
        ctx,
        app,
        crate::router::ListContext::MusicMore,
        "title:more",
        entries,
    );
}

/// The Menu's rows as Core's settings and the slot stand now.
fn menu_entries_now(ctx: &Ctx) -> Vec<crate::MenuEntry> {
    let shared = lock(&ctx.shared);
    player_menu_entries(shared.music.settings, &now(&shared.music))
}

/// The Volume chip: a panel holding the volume slider alone. In the Menu
/// list the same slider is a row of its own.
fn open_volume(ctx: &Ctx, app: &App) {
    let settings = lock(&ctx.shared).music.settings;
    crate::router::present_list(
        ctx,
        app,
        crate::router::ListContext::MusicVolume,
        "title:volume",
        vec![slider_row(settings)],
    );
}

/// How long after the last slider step the volume is sent to Core.
const VOLUME_SAVE_MS: u64 = 250;

/// One step of the volume slider, Left (-1) or Right (+1). Core applies the
/// volume as it is set, so each step is heard; the value is sent once the
/// steps stop.
pub(crate) fn slide(ctx: &Ctx, app: &App, direction: i32) {
    let next = {
        let shared = lock(&ctx.shared);
        let settings = shared.music.settings;
        match (settings.state, settings.volume) {
            (SettingsState::Ready, Some(volume)) => Some(rules::volume_step(volume, direction > 0)),
            _ => None,
        }
    };
    let Some(next) = next else {
        return;
    };
    set_volume(ctx, app, next);
}

/// Set the volume slider to `volume`: the rows change in place now, Core
/// hears it once the changes stop.
fn set_volume(ctx: &Ctx, app: &App, volume: i32) {
    let seq = {
        let mut shared = lock(&ctx.shared);
        if shared.music.settings.volume == Some(volume) {
            return;
        }
        shared.music.settings.volume = Some(volume);
        shared.music.volume_seq += 1;
        shared.music.volume_seq
    };
    menu_changed(ctx, app);
    let ctx = ctx.clone();
    let weak = app.as_weak();
    slint::Timer::single_shot(Duration::from_millis(VOLUME_SAVE_MS), move || {
        let volume = {
            let shared = lock(&ctx.shared);
            (shared.music.volume_seq == seq)
                .then_some(shared.music.settings.volume)
                .flatten()
        };
        if let (Some(volume), Some(app)) = (volume, weak.upgrade()) {
            save_volume(&ctx, &app, volume);
        }
    });
}

/// Tell Core the volume. It is not asked again after: the slider already
/// shows it, and an answer to an older step would move it back.
fn save_volume(ctx: &Ctx, app: &App, volume: i32) {
    let client = ctx.store.client();
    let weak = app.as_weak();
    let ctx2 = ctx.clone();
    ctx.handle.spawn(async move {
        let result = client
            .settings_update(UpdateSettingsParams {
                audio_music_volume: Some(volume),
                ..UpdateSettingsParams::default()
            })
            .await;
        if let Err(e) = result {
            tracing::warn!("music: saving the volume failed: {}", e.message);
            let _ = weak.upgrade_in_event_loop(move |app| {
                crate::router::report_action_error(&ctx2, &app, "setting", "");
                fetch_settings(&ctx2, &app);
            });
        }
    });
}

/// Whether a pick on the Music Menu or More keeps the panel open: the
/// toggles show their new value in place, and Volume turns the panel to
/// its page.
pub(crate) fn menu_stays_open(id: &str) -> bool {
    matches!(id, "music_autoplay" | "music_pause_games" | "music_volume")
}

/// A page action, from the Menu list (`from_menu`) or its chip.
pub(crate) fn page_action(ctx: &Ctx, app: &App, id: &str, from_menu: bool) {
    let settings = lock(&ctx.shared).music.settings;
    let ready = settings.state == SettingsState::Ready;
    match id {
        "music_playlist" => show_playlist(ctx, app),
        "music_choose" => open_library(ctx, app),
        "music_stop" => {
            let command = rules::stop_command(&now(&lock(&ctx.shared).music));
            if let Some(command) = command {
                send(ctx, app, command);
            }
        }
        // In the Menu list the volume is a slider row, adjusted in place;
        // the chip opens it on its own.
        "music_volume" if ready && settings.volume.is_some() && !from_menu => open_volume(ctx, app),
        "music_play_all" => play_listing(ctx, app, false, None),
        "music_shuffle_all" => play_listing(ctx, app, true, None),
        "music_autoplay" if ready && settings.autoplay.is_some() => {
            let value = !settings.autoplay.unwrap_or(false);
            lock(&ctx.shared).music.settings.autoplay = Some(value);
            menu_changed(ctx, app);
            save_setting(
                ctx,
                app,
                UpdateSettingsParams {
                    audio_music_autoplay: Some(value),
                    ..UpdateSettingsParams::default()
                },
            );
        }
        "music_pause_games" if ready && settings.pause_games.is_some() => {
            let value = !settings.pause_games.unwrap_or(true);
            lock(&ctx.shared).music.settings.pause_games = Some(value);
            menu_changed(ctx, app);
            save_setting(
                ctx,
                app,
                UpdateSettingsParams {
                    audio_music_pause_on_launch: Some(value),
                    ..UpdateSettingsParams::default()
                },
            );
        }
        _ => {}
    }
}

/// A pick on one of the music lists.
pub(crate) fn list_accept(ctx: &Ctx, app: &App, context: &crate::router::ListContext, id: &str) {
    use crate::router::ListContext;
    match context {
        ListContext::MusicMenu | ListContext::MusicMore | ListContext::MusicLibraryMenu => {
            page_action(ctx, app, id, matches!(context, ListContext::MusicMenu));
        }
        ListContext::MusicFolderOptions(folder) => match id {
            "music_play_folder" => play_folder(ctx, app, folder, false),
            "music_shuffle_folder" => play_folder(ctx, app, folder, true),
            _ => {}
        },
        // The volume slider is adjusted with Left and Right; Accept on it
        // does nothing.
        _ => {}
    }
}

// ---------- Library ----------

/// The rows of one library folder, or Core's Audio routes for `None`, read
/// a cursor page at a time until the folder is whole.
async fn read_folder(
    client: std::sync::Arc<zaparoo_core::client::Client>,
    dir: Option<String>,
) -> Result<Vec<LibRow>, String> {
    let mut rows = Vec::new();
    let mut cursor = None;
    loop {
        let page = client
            .media_browse(MediaBrowseParams {
                path: dir.clone().unwrap_or_default(),
                systems: vec![AUDIO_SYSTEM.to_string()],
                max_results: Some(BROWSE_PAGE),
                cursor: cursor.clone(),
                sort: Some("filename-asc".to_string()),
                ..MediaBrowseParams::default()
            })
            .await
            .map_err(|e| e.message)?;
        for entry in page.entries {
            let kind = match entry.entry_type.as_str() {
                "root" => EntryKind::Route,
                "directory" => EntryKind::Folder,
                "media" => EntryKind::Track,
                _ => continue,
            };
            let name = if kind == EntryKind::Route {
                entry.path.clone()
            } else {
                entry.name
            };
            rows.push(LibRow {
                kind,
                name,
                path: entry.path,
            });
        }
        match page.pagination {
            Some(p) if p.has_next_page && p.next_cursor.is_some() => cursor = p.next_cursor,
            _ => break,
        }
    }
    Ok(rows)
}

/// How soon after a track change without tags Core is asked again.
const TAGS_RETRY_MS: u64 = 1000;

/// Core's system for native audio.
const AUDIO_SYSTEM: &str = "Audio";
/// Entries per `media.browse` page.
const BROWSE_PAGE: u32 = 500;

/// What a folder read hands back.
struct Landing {
    stack: Vec<String>,
    rows: Vec<LibRow>,
    fill: Fill,
    single_route: bool,
    seat: Option<String>,
}

/// Read `dir` (the routes for `None`) off the event loop and show it. The
/// page on screen stays, taking only Back, until the folder is ready. At
/// the top, a single route is opened at once.
fn start_read(ctx: &Ctx, app: &App, stack: Vec<String>, seat: Option<String>) {
    let ticket = {
        let mut shared = lock(&ctx.shared);
        shared.music.ticket = shared.music.ticket.wrapping_add(1);
        shared.music.ticket
    };
    crate::router::begin_pending(app, crate::Screen::Music);
    let client = ctx.store.client();
    let weak = app.as_weak();
    let ctx2 = ctx.clone();
    let task = ctx.handle.spawn(async move {
        let mut stack = stack;
        let mut single_route = false;
        let mut result = read_folder(client.clone(), stack.last().cloned()).await;
        if stack.is_empty() {
            if let Ok(rows) = &result {
                if let [only] = rows.as_slice() {
                    if only.kind == EntryKind::Route {
                        single_route = true;
                        stack.push(only.path.clone());
                        result = read_folder(client.clone(), Some(only.path.clone())).await;
                    }
                }
            }
        } else if stack.len() == 1 {
            // A restore inside the only route keeps Back going to the
            // player.
            if let Ok(routes) = read_folder(client.clone(), None).await {
                single_route = routes.len() == 1;
            }
        }
        let landing = match result {
            Ok(rows) => Landing {
                fill: if rows.is_empty() {
                    Fill::Empty
                } else {
                    Fill::Ready
                },
                stack,
                rows,
                single_route,
                seat,
            },
            Err(e) => {
                tracing::warn!("music: cannot read a library folder: {e}");
                Landing {
                    stack,
                    rows: Vec::new(),
                    fill: Fill::Error,
                    single_route,
                    seat,
                }
            }
        };
        let _ = weak.upgrade_in_event_loop(move |app| land(&ctx2, &app, ticket, landing));
    });
    lock(&ctx.shared).music.task.replace(task);
}

fn land(ctx: &Ctx, app: &App, ticket: u64, landing: Landing) {
    let shell = app.global::<crate::Shell>();
    if !shell.get_transitioning() || shell.get_transition_target() != crate::Screen::Music {
        return;
    }
    {
        let mut shared = lock(&ctx.shared);
        let model = &mut shared.music;
        if model.ticket != ticket {
            return;
        }
        let level = landing.stack.len();
        model.persist.begin_replacement();
        model.page = Page::Library;
        model.single_route = landing.single_route;
        model.stack = landing.stack;
        model.letters = letters_of(&landing.rows);
        model.rows = landing.rows;
        model.fill = landing.fill;
        model.index = landing
            .seat
            .and_then(|seat| model.rows.iter().position(|r| r.path == seat))
            .unwrap_or(0);
        model.list_top = model.tops.get(level).copied().unwrap_or(0);
        model.selected.truncate(level);
        model.tops.truncate(level);
        model.persist.end_replacement();
        write_persist(&mut shared);
    }
    crate::router::save_persist(&ctx.shared);
    TICK.with(slint::Timer::stop);
    render(ctx, app);
    crate::router::transition_to_screen(app, crate::Screen::Music, 1);
}

/// A library read a route test cannot run on its undriven runtime: land
/// `rows` as the read with `ticket` would.
#[cfg(all(test, feature = "mister"))]
pub(crate) fn land_for_test(
    ctx: &Ctx,
    app: &App,
    ticket: u64,
    stack: Vec<String>,
    rows: Vec<(EntryKind, &str, &str)>,
) {
    let rows: Vec<LibRow> = rows
        .into_iter()
        .map(|(kind, name, path)| LibRow {
            kind,
            name: name.to_string(),
            path: path.to_string(),
        })
        .collect();
    land(
        ctx,
        app,
        ticket,
        Landing {
            fill: if rows.is_empty() {
                Fill::Empty
            } else {
                Fill::Ready
            },
            stack,
            rows,
            single_route: false,
            seat: None,
        },
    );
}

impl Shared {
    /// The library read in flight, for route tests.
    #[cfg(all(test, feature = "mister"))]
    pub(crate) fn music_ticket(&self) -> u64 {
        self.music.ticket
    }

    /// A newer library read started, for route tests.
    #[cfg(all(test, feature = "mister"))]
    pub(crate) fn music_bump_ticket(&mut self) {
        self.music.ticket = self.music.ticket.wrapping_add(1);
    }

    /// Core's music settings as if it had answered, for route tests.
    #[cfg(all(test, feature = "mister"))]
    pub(crate) fn music_settings_for_test(&mut self, volume: Option<i32>) {
        self.music.settings = Settings {
            state: SettingsState::Ready,
            volume,
            autoplay: Some(false),
            pause_games: Some(true),
        };
    }

    /// The volume the slider shows, for route tests.
    #[cfg(all(test, feature = "mister"))]
    pub(crate) fn music_volume_for_test(&self) -> Option<i32> {
        self.music.settings.volume
    }

    /// The player's focused control and the playlist page's row, for
    /// route tests.
    #[cfg(all(test, feature = "mister"))]
    pub(crate) fn music_focus(&self) -> (Control, usize) {
        (self.music.control, self.music.row)
    }
}

/// Menu > Browse music: the library's top level.
fn open_library(ctx: &Ctx, app: &App) {
    {
        let mut shared = lock(&ctx.shared);
        shared.music.selected.clear();
        shared.music.tops.clear();
    }
    start_read(ctx, app, Vec::new(), None);
}

fn open_library_at(ctx: &Ctx, app: &App, inside: bool, seat: Option<String>) {
    let stack = lock(&ctx.shared).music.stack.clone();
    let stack = if inside { stack } else { Vec::new() };
    start_read(ctx, app, stack, seat);
}

/// Open the focused folder, remembering where this level was left.
fn open_folder(ctx: &Ctx, app: &App, path: String) {
    let stack = {
        let mut shared = lock(&ctx.shared);
        let model = &mut shared.music;
        let level = model.stack.len();
        let here = model
            .rows
            .get(model.index)
            .map(|r| r.path.clone())
            .unwrap_or_default();
        model.selected.resize(level + 1, String::new());
        model.selected[level] = here;
        model.tops.resize(level + 1, 0);
        model.tops[level] = model.list_top;
        let mut stack = model.stack.clone();
        stack.push(path);
        // Cut the accept flash before the rows swap.
        model.release_pulse += 1;
        stack
    };
    start_read(ctx, app, stack, None);
}

/// Back on the library: up a level, or to the player from the top.
fn library_back(ctx: &Ctx, app: &App) {
    let (stack, seat, to_player) = {
        let mut shared = lock(&ctx.shared);
        let model = &mut shared.music;
        // Disarm a pending row flash.
        model.press_seq += 1;
        model.release_pulse += 1;
        let at_top = model.stack.is_empty() || (model.single_route && model.stack.len() == 1);
        let mut stack = model.stack.clone();
        let left = stack.pop();
        (stack, left, at_top)
    };
    if to_player {
        show_player(ctx, app);
        return;
    }
    start_read(ctx, app, stack, seat);
}

fn show_player(ctx: &Ctx, app: &App) {
    {
        let mut shared = lock(&ctx.shared);
        let model = &mut shared.music;
        model.task.cancel();
        model.ticket = model.ticket.wrapping_add(1);
        model.page = Page::Player;
        model.bg = ctx.store.background_media().current();
        write_persist(&mut shared);
    }
    crate::router::save_persist(&ctx.shared);
    render(ctx, app);
    update_tick(ctx, app);
}

/// The repeat mode a new playlist starts with: the one the open playlist
/// had, so a choice survives picking new music, else all.
fn new_repeat(model: &Model) -> Repeat {
    playlist(model).map_or(Repeat::All, |p| Repeat::from_core(&p.repeat))
}

/// Start the listing on screen as a playlist, from track `start` when one
/// was picked.
fn play_listing(ctx: &Ctx, app: &App, shuffle: bool, start: Option<&str>) {
    let (folder, start) = {
        let shared = lock(&ctx.shared);
        let model = &shared.music;
        let rows: Vec<(EntryKind, &str)> = model
            .rows
            .iter()
            .map(|r| (r.kind, r.path.as_str()))
            .collect();
        (
            model.stack.last().cloned(),
            start.and_then(|picked| rules::track_start(&rows, picked)),
        )
    };
    let Some(folder) = folder else {
        return;
    };
    start_music(ctx, app, &folder, shuffle, start);
}

/// Play `folder` as a playlist, from track `start` when one was picked.
/// The library stays on screen, taking only Back, until Core takes the
/// command: then the player opens on it; if Core refuses, the library
/// stays where it was and says so. A Back meanwhile, or a newer request,
/// drops the answer.
fn start_music(ctx: &Ctx, app: &App, folder: &str, shuffle: bool, start: Option<usize>) {
    let (ticket, script) = {
        let mut shared = lock(&ctx.shared);
        let model = &mut shared.music;
        drop_play_read(model);
        (
            model.play_ticket,
            rules::play_folder_script(folder, shuffle, new_repeat(model), start),
        )
    };
    crate::router::begin_pending(app, crate::Screen::Music);
    let client = ctx.store.client();
    let weak = app.as_weak();
    let ctx2 = ctx.clone();
    let task = ctx.handle.spawn(async move {
        let result = client.run(RunParams { text: script }).await;
        let _ = weak.upgrade_in_event_loop(move |app| {
            let shell = app.global::<crate::Shell>();
            let current = lock(&ctx2.shared).music.play_ticket == ticket;
            if !current
                || !shell.get_transitioning()
                || shell.get_transition_target() != crate::Screen::Music
            {
                return;
            }
            if let Err(e) = result {
                tracing::warn!("music: cannot start a folder: {}", e.message);
                crate::router::clear_pending(&app);
                render(&ctx2, &app);
                report(&ctx2, &app, "");
                return;
            }
            begin_start(&ctx2, &app);
            resync_soon(&ctx2);
            lock(&ctx2.shared).music.control = Control::default();
            show_player(&ctx2, &app);
            crate::router::transition_to_screen(&app, crate::Screen::Music, 1);
        });
    });
    lock(&ctx.shared).music.play_task.replace(task);
}

/// Drop a request to start music still waiting on Core.
fn drop_play_read(model: &mut Model) {
    model.play_task.cancel();
    model.play_ticket = model.play_ticket.wrapping_add(1);
}

/// Options > Play folder on a folder that is not open.
fn play_folder(ctx: &Ctx, app: &App, folder: &str, shuffle: bool) {
    start_music(ctx, app, folder, shuffle, None);
}

/// The Menu button on the library: the chip row where the scene draws
/// one, else the Menu list. A folder without tracks of its own keeps the
/// rows, muted, with the reason.
fn library_menu(ctx: &Ctx, app: &App) {
    if crate::page_bar::enter(ctx, app) {
        return;
    }
    let has_tracks = lock(&ctx.shared)
        .music
        .rows
        .iter()
        .any(|r| r.kind == EntryKind::Track);
    let reason = if has_tracks { "" } else { "music:no_tracks" };
    crate::router::present_list(
        ctx,
        app,
        crate::router::ListContext::MusicLibraryMenu,
        "title:menu",
        vec![
            crate::router::menu_row_full(
                "music_play_all",
                "music_play_all",
                "",
                has_tracks,
                reason,
            ),
            crate::router::menu_row_full(
                "music_shuffle_all",
                "music_shuffle_all",
                "",
                has_tracks,
                reason,
            ),
        ],
    );
}

/// Options on a library row: play or shuffle the focused folder, or, on a
/// track, the folder it is in.
fn folder_options(ctx: &Ctx, app: &App) {
    let target = {
        let shared = lock(&ctx.shared);
        let model = &shared.music;
        model.rows.get(model.index).map(|r| {
            if r.kind == EntryKind::Track {
                None
            } else {
                Some(r.path.clone())
            }
        })
    };
    let Some(target) = target else {
        return;
    };
    match target {
        Some(folder) => crate::router::present_list(
            ctx,
            app,
            crate::router::ListContext::MusicFolderOptions(folder),
            "title:options",
            vec![
                crate::router::menu_row("music_play_folder"),
                crate::router::menu_row("music_shuffle_folder"),
            ],
        ),
        // The folder on screen is already read: its rows play as they are.
        None => crate::router::present_list(
            ctx,
            app,
            crate::router::ListContext::MusicLibraryMenu,
            "title:options",
            vec![
                crate::router::menu_row_keyed("music_play_all", "music_play_folder", ""),
                crate::router::menu_row_keyed("music_shuffle_all", "music_shuffle_folder", ""),
            ],
        ),
    }
}

/// Accept on a library row: a row flash, then open or play.
fn library_accept(ctx: &Ctx, app: &App) {
    let row = {
        let shared = lock(&ctx.shared);
        shared.music.rows.get(shared.music.index).cloned()
    };
    let Some(row) = row else {
        return;
    };
    flush_persist(ctx);
    flash_row(ctx, app);
    match row.kind {
        EntryKind::Route | EntryKind::Folder => open_folder(ctx, app, row.path),
        EntryKind::Track => play_listing(ctx, app, false, Some(&row.path)),
    }
}

/// The row a list row flash belongs to: the flash, then its release once
/// the press has had its moment. A newer flash or a Back disarms it.
fn flash_row(ctx: &Ctx, app: &App) {
    let seq = {
        let mut shared = lock(&ctx.shared);
        let model = &mut shared.music;
        model.press_seq += 1;
        model.activate_pulse += 1;
        model.press_seq
    };
    render(ctx, app);
    let ctx2 = ctx.clone();
    let weak = app.as_weak();
    slint::Timer::single_shot(
        Duration::from_millis(crate::games::press_duration(app)),
        move || {
            {
                let mut shared = lock(&ctx2.shared);
                if shared.music.press_seq != seq {
                    return;
                }
                shared.music.release_pulse += 1;
            }
            if let Some(app) = weak.upgrade() {
                if on_screen(&app) {
                    render(&ctx2, &app);
                }
            }
        },
    );
}

/// A move on the page's list (the playlist or the library folder):
/// single steps wrap, page steps stop at the ends, and a held page move
/// stops there too.
fn list_move(ctx: &Ctx, app: &App, delta: i64) {
    let held = list_rules::is_page_move(delta) && crate::input::dispatching_repeat(ctx);
    let moved = {
        let mut shared = lock(&ctx.shared);
        let model = &mut shared.music;
        let (current, count) = match model.page {
            Page::Playlist => (model.row, playlist_len(model)),
            Page::Library => (model.index, model.rows.len()),
            Page::Player => return,
        };
        let next = match list_rules::linear_move(current, count, delta, false, held) {
            list_rules::LinearMove::To { index, .. } => Some(index),
            list_rules::LinearMove::WrapToTail => Some(count.saturating_sub(1)),
            list_rules::LinearMove::FetchMore | list_rules::LinearMove::Stay { .. } => None,
        };
        if let Some(index) = next {
            match model.page {
                Page::Playlist => model.row = index,
                _ => model.index = index,
            }
        }
        next.is_some()
    };
    if moved {
        schedule_persist(ctx);
        render(ctx, app);
    }
}

/// A long hold's letter step on the library: the next or previous letter
/// of the folder. False where there is no further letter, so the hold
/// keeps paging.
fn letter_step(ctx: &Ctx, app: &App, forward: bool) -> bool {
    {
        let mut shared = lock(&ctx.shared);
        let model = &mut shared.music;
        let letters = &model.letters;
        let Some(bucket) =
            list_rules::letter_step(&letters.offsets, letters.leading, model.index, forward)
        else {
            return false;
        };
        model.index = (letters.leading + letters.offsets[bucket] as usize)
            .min(model.rows.len().saturating_sub(1));
    }
    schedule_persist(ctx);
    render(ctx, app);
    true
}

/// A held fast scroll on the library: the rail comes up with it.
pub(crate) fn set_rapid(ctx: &Ctx, app: &App, active: bool) {
    let (changed, linger) = lock(&ctx.shared).music.rapid.set(active);
    if let Some(seq) = linger {
        crate::browse_motion::linger_rail(
            ctx,
            app,
            seq,
            |shared| &mut shared.music.rapid,
            |app| app.global::<MusicView>().set_rail_visible(false),
        );
    }
    if changed && on_screen(app) {
        render(ctx, app);
    }
}

// ---------- Input ----------

/// Left and Right walk the transport row and stop at its ends.
fn player_step(ctx: &Ctx, app: &App, right: bool) {
    let moved = {
        let mut shared = lock(&ctx.shared);
        let model = &mut shared.music;
        let next = model.control.step(right);
        if let Some(next) = next {
            model.control = next;
        }
        next.is_some()
    };
    if moved {
        schedule_persist(ctx);
        render(ctx, app);
    }
}

/// Accept on a playlist row: jump to that track, or play or pause the one
/// already playing. The row flashes as a games row does.
fn playlist_accept(ctx: &Ctx, app: &App) {
    flush_persist(ctx);
    let (row, current, playing, count, starting) = {
        let shared = lock(&ctx.shared);
        let model = &shared.music;
        (
            model.row,
            current_track(model),
            playlist(model).is_some_and(|p| p.playing),
            playlist_len(model),
            model.start.pending(),
        )
    };
    // Core takes no other launch while a track is starting.
    if count == 0 || starting {
        return;
    }
    if row == current {
        press(ctx, app, Control::PlayPause);
        return;
    }
    begin_start(ctx, app);
    send(
        ctx,
        app,
        rules::Command::Run(rules::goto_script(row, playing)),
    );
    flash_row(ctx, app);
}

/// Menu > Show playlist: the open playlist, seated on the playing track.
fn show_playlist(ctx: &Ctx, app: &App) {
    {
        let mut shared = lock(&ctx.shared);
        let model = &mut shared.music;
        if playlist_len(model) == 0 {
            return;
        }
        model.page = Page::Playlist;
        model.row = current_track(model);
        model.playlist_top = 0;
        write_persist(&mut shared);
    }
    crate::router::save_persist(&ctx.shared);
    TICK.with(slint::Timer::stop);
    render(ctx, app);
}

pub fn handle_action(ctx: &Ctx, app: &App, action: &str) {
    let page = lock(&ctx.shared).music.page;
    let page_rows = || i64::try_from(layout(ctx, app).list.visible_rows.max(1)).unwrap_or(1);
    match page {
        Page::Player => match action {
            actions::LEFT => player_step(ctx, app, false),
            actions::RIGHT => player_step(ctx, app, true),
            actions::PAGE_PREV => skip(ctx, app, false),
            actions::PAGE_NEXT => skip(ctx, app, true),
            actions::ACCEPT => {
                flush_persist(ctx);
                let control = lock(&ctx.shared).music.control;
                press(ctx, app, control);
            }
            actions::PAGE_MENU => open_player_menu(ctx, app),
            actions::CANCEL => leave(ctx, app),
            _ => {}
        },
        Page::Playlist => match action {
            actions::UP | actions::DOWN => {
                let dir: i64 = if action == actions::UP { -1 } else { 1 };
                let step = if crate::input::rapid_page(ctx) {
                    dir * page_rows()
                } else {
                    dir
                };
                list_move(ctx, app, step);
            }
            actions::LEFT | actions::PAGE_PREV => list_move(ctx, app, -page_rows()),
            actions::RIGHT | actions::PAGE_NEXT => list_move(ctx, app, page_rows()),
            actions::ACCEPT => playlist_accept(ctx, app),
            actions::CANCEL => {
                {
                    let mut shared = lock(&ctx.shared);
                    shared.music.press_seq += 1;
                    shared.music.release_pulse += 1;
                }
                flush_persist(ctx);
                show_player(ctx, app);
            }
            _ => {}
        },
        Page::Library => {
            let fill = lock(&ctx.shared).music.fill;
            let ready = fill == Fill::Ready;
            match action {
                actions::UP | actions::DOWN if ready => {
                    let dir: i64 = if action == actions::UP { -1 } else { 1 };
                    if crate::input::letter_step(ctx) && letter_step(ctx, app, dir > 0) {
                        return;
                    }
                    if crate::input::rapid_page(ctx) {
                        list_move(ctx, app, dir * page_rows());
                    } else {
                        list_move(ctx, app, dir);
                    }
                }
                actions::LEFT | actions::RIGHT if ready => {
                    let dir: i64 = if action == actions::LEFT { -1 } else { 1 };
                    list_move(ctx, app, dir * page_rows());
                }
                actions::PAGE_PREV | actions::PAGE_NEXT if ready => {
                    let forward = action == actions::PAGE_NEXT;
                    if crate::input::letter_step(ctx) && letter_step(ctx, app, forward) {
                        return;
                    }
                    list_move(ctx, app, if forward { page_rows() } else { -page_rows() });
                }
                actions::ACCEPT if ready => library_accept(ctx, app),
                // A folder that failed to read, or read empty, tries again.
                actions::ACCEPT => retry_read(ctx, app),
                actions::CONTEXT_MENU if ready => folder_options(ctx, app),
                actions::PAGE_MENU if ready => library_menu(ctx, app),
                actions::CANCEL => {
                    flush_persist(ctx);
                    library_back(ctx, app);
                }
                _ => {}
            }
        }
    }
}

/// Read the folder on screen again, after it failed or came back empty.
fn retry_read(ctx: &Ctx, app: &App) {
    let stack = lock(&ctx.shared).music.stack.clone();
    start_read(ctx, app, stack, None);
}

/// The scene changed size: repaint at the new geometry, and ask for the
/// cover again at the art box's new size. The one on show stays until the
/// new one lands.
pub(crate) fn relayout(ctx: &Ctx, app: &App) {
    if !on_screen(app) {
        return;
    }
    render(ctx, app);
    let size = layout(ctx, app).player.art_size.max(1);
    let refetch = {
        let shared = lock(&ctx.shared);
        let model = &shared.music;
        let has_art = model
            .bg
            .track
            .as_ref()
            .and_then(|t| t.track.as_ref())
            .is_some_and(|t| t.has_art);
        (has_art && !model.cover_path.is_empty() && model.cover_size != size)
            .then(|| model.cover_path.clone())
    };
    if let Some(path) = refetch {
        fetch_cover_art(ctx, app, path);
    }
}

fn pointer_allowed(ctx: &Ctx, app: &App) -> bool {
    let shell = app.global::<crate::Shell>();
    shell.get_active_screen() == crate::Screen::Music
        && !shell.get_transitioning()
        && !crate::input::modal_open(app)
        && lock(&ctx.shared).persist.settings.mouse_enabled
}

/// Focus the visible list row `local`; false when there is none.
fn point_at(ctx: &Ctx, app: &App, local: i32) -> bool {
    let Ok(local) = usize::try_from(local) else {
        return false;
    };
    let base = usize::try_from(app.global::<MusicView>().get_list_view_top()).unwrap_or(0);
    let index = base + local;
    let mut shared = lock(&ctx.shared);
    let model = &mut shared.music;
    match model.page {
        Page::Player => return false,
        Page::Playlist => {
            if index >= playlist_len(model) {
                return false;
            }
            model.row = index;
        }
        Page::Library => {
            if index >= model.rows.len() {
                return false;
            }
            model.index = index;
        }
    }
    true
}

/// Focus transport control `index` on the player; false when it is
/// already focused or there is none.
fn point_control(ctx: &Ctx, index: i32) -> bool {
    let Ok(index) = usize::try_from(index) else {
        return false;
    };
    if index >= rules::CONTROLS.len() {
        return false;
    }
    let mut shared = lock(&ctx.shared);
    let model = &mut shared.music;
    let control = Control::at(index);
    if model.page != Page::Player || model.control == control {
        return false;
    }
    model.control = control;
    true
}

/// Wire the view's pointer callbacks and the live background slot.
pub fn bind(ctx: &Ctx, app: &App) {
    let input = app.global::<MusicInput>();
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
                crate::router::handle_action(&ctx, &app, actions::ACCEPT);
            }
        });
    }
    {
        let ctx = ctx.clone();
        let weak = app.as_weak();
        input.on_row_right_clicked(move |local| {
            let Some(app) = weak.upgrade() else {
                return;
            };
            if pointer_allowed(&ctx, &app)
                && lock(&ctx.shared).music.page == Page::Library
                && point_at(&ctx, &app, local)
            {
                render(&ctx, &app);
                crate::router::handle_action(&ctx, &app, actions::CONTEXT_MENU);
            }
        });
    }
    {
        let ctx = ctx.clone();
        let weak = app.as_weak();
        input.on_control_hovered(move |index| {
            let Some(app) = weak.upgrade() else {
                return;
            };
            if pointer_allowed(&ctx, &app)
                && !crate::press_feedback::pending(&app)
                && point_control(&ctx, index)
            {
                render(&ctx, &app);
            }
        });
    }
    {
        let ctx = ctx.clone();
        let weak = app.as_weak();
        input.on_control_clicked(move |index| {
            let Some(app) = weak.upgrade() else {
                return;
            };
            if !pointer_allowed(&ctx, &app) || crate::press_feedback::pending(&app) {
                return;
            }
            point_control(&ctx, index);
            render(&ctx, &app);
            // Through the router, so a click is pressed and committed like
            // Accept on the focused control.
            crate::router::handle_action(&ctx, &app, actions::ACCEPT);
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
            // The player has no list to page; its L and R skip tracks.
            if lock(&ctx.shared).music.page == Page::Player {
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
    bind_background(ctx, app);
}

#[cfg(test)]
mod tests {
    use super::*;
    use zaparoo_core::media_types::{ActiveMediaInfo, PlaylistItem};

    fn state(playing: bool, shuffle: Option<bool>) -> BackgroundMediaState {
        BackgroundMediaState {
            seeded: true,
            pushes_playback: false,
            track: Some(ActiveMediaInfo {
                slot: "background".into(),
                media_name: "Moonlight Shadow".into(),
                playback_state: if playing { "playing" } else { "paused" }.into(),
                launcher_controls: vec!["toggle_pause".into(), "rewind".into()],
                ..ActiveMediaInfo::default()
            }),
            playlist: Some(std::sync::Arc::new(PlaylistState {
                id: "/m".into(),
                name: "m".into(),
                slot: "background".into(),
                repeat: "all".into(),
                items: vec![
                    PlaylistItem {
                        name: "One".into(),
                        zap_script: "/m/1.mp3".into(),
                    },
                    PlaylistItem {
                        name: String::new(),
                        zap_script: "/m/02 Two.mp3".into(),
                    },
                ],
                index: 1,
                total: 2,
                playing,
                shuffle,
            })),
        }
    }

    #[test]
    fn the_background_slot_maps_onto_the_rules() {
        let now = now_of(&state(true, Some(true)));
        assert!(now.playing());
        assert!(now.shuffle);
        assert!(now.can_update_playlist);
        assert_eq!(now.repeat, Repeat::All);
        assert_eq!(now.playlist_total, 2);
        let old_core = now_of(&state(false, None));
        assert!(!old_core.playing());
        assert!(!old_core.can_update_playlist);
        let idle = now_of(&BackgroundMediaState::default());
        assert!(!idle.has_track && !idle.has_playlist);
    }

    #[test]
    fn an_unnamed_item_shows_its_file_name_without_the_extension() {
        assert_eq!(track_name("", "/m/02 Two.mp3"), "02 Two");
        assert_eq!(track_name("One", "/m/1.mp3"), "One");
    }

    #[test]
    fn origins_follow_the_saved_flag() {
        assert_eq!(Origin::from_hub(true), Origin::Hub);
        assert_eq!(Origin::from_hub(false), Origin::Settings);
    }

    #[test]
    fn the_menu_disables_what_cannot_act_and_says_why() {
        let idle = now_of(&BackgroundMediaState::default());
        let entries = player_menu_entries(Settings::default(), &idle);
        let ids: Vec<&str> = entries.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "music_playlist",
                "music_choose",
                "music_stop",
                "music_volume",
                "music_autoplay",
                "music_pause_games"
            ]
        );
        assert!(entries[1].enabled, "Browse music always works");
        assert_eq!(entries[0].reason_key.as_str(), "music:no_playlist");
        assert_eq!(entries[2].reason_key.as_str(), "music:nothing_playing");
        assert!(entries[3..].iter().all(|e| !e.enabled));
        assert!(
            entries[3..]
                .iter()
                .all(|e| e.reason_key.as_str() == "music:loading"),
            "waiting on Core is not the same as Core lacking the setting"
        );
        assert_eq!(entries[3].role, crate::MenuRole::Slider);
        assert_eq!(entries[3].label_key.as_str(), "music_volume");
        let failed = player_menu_entries(
            Settings {
                state: SettingsState::Failed,
                ..Settings::default()
            },
            &idle,
        );
        assert_eq!(failed[3].reason_key.as_str(), "music:no_answer");
        let old_core = player_menu_entries(
            Settings {
                state: SettingsState::Ready,
                ..Settings::default()
            },
            &idle,
        );
        assert_eq!(old_core[3].reason_key.as_str(), "music:not_supported");
        let playing = now_of(&state(true, Some(false)));
        let offered = player_menu_entries(
            Settings {
                state: SettingsState::Ready,
                volume: Some(60),
                autoplay: Some(true),
                pause_games: Some(false),
            },
            &playing,
        );
        assert!(offered.iter().all(|e| e.enabled));
        assert_eq!(offered[3].label.as_str(), "60");
        // The two settings are switches: their state is their role.
        assert_eq!(offered[4].role, crate::MenuRole::ToggleOn);
        assert_eq!(offered[5].role, crate::MenuRole::ToggleOff);
        assert_eq!(offered[3].role, crate::MenuRole::Slider);
    }

    #[test]
    fn every_page_keeps_the_settings_card_margins() {
        for (w, h) in [
            (352.0, 240.0),
            (640.0, 480.0),
            (960.0, 540.0),
            (1280.0, 720.0),
            (1920.0, 1080.0),
        ] {
            let inputs = zaparoo_app::sizing::Inputs {
                screen_width: w,
                screen_height: h,
                ..zaparoo_app::sizing::Inputs::default()
            };
            let page = zaparoo_app::settings::page_geometry(&inputs);
            let derived = zaparoo_app::sizing::derive(&inputs);
            for chips in [false, true] {
                let layout = layout_for(&inputs, false, chips);
                let row = if chips && derived.menu_row {
                    derived.menu_row_block
                } else {
                    0
                };
                assert_eq!(
                    (layout.card_x, layout.card_width),
                    (page.card_x, page.card_width),
                    "{w}x{h}: the player keeps the Settings card's margins"
                );
                assert!(layout.card_height <= page.card_height - row, "{w}x{h}");
                assert_eq!(
                    layout.card_y - page.card_y - row,
                    (page.card_height - row - layout.card_height) / 2,
                    "{w}x{h}: the hugging card centers in the band under the row"
                );
                assert_eq!(
                    (
                        layout.list.card_x,
                        layout.list.card_y,
                        layout.list.card_width
                    ),
                    (page.card_x, page.card_y + row, page.card_width),
                    "{w}x{h}: the lists sit in the Settings card's band"
                );
                assert!(
                    layout.list.card_height <= page.card_height - row,
                    "{w}x{h}: the list ends within the band"
                );
                assert!(layout.list.visible_rows >= 1, "{w}x{h}");
            }
        }
    }

    #[test]
    fn library_letters_skip_the_folders_that_lead() {
        let row = |kind, name: &str| LibRow {
            kind,
            name: name.into(),
            path: format!("/m/{name}"),
        };
        let letters = letters_of(&[
            row(EntryKind::Folder, "Live"),
            row(EntryKind::Track, "Arrival"),
            row(EntryKind::Track, "Away"),
            row(EntryKind::Track, "Coastline"),
        ]);
        assert_eq!(letters.leading, 1);
        assert_eq!(letters.labels, ["A", "C"]);
        assert_eq!(letters.offsets, [0, 2]);
    }
}
