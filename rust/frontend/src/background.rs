// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

//! The screen background: the user's image (or a random one from a
//! folder), or the focused game's art in a game list, prepared off the
//! event loop into one opaque bitmap per output and published through the
//! `Backdrop` global. The rules (fit, blur, the readable dim, the darker
//! help bar and glass card, RGB565 dithering) live in
//! `zaparoo_app::background`; this module decides what shows where and
//! when, keeps the prepared pieces, and hands them to the view and to the
//! `MiSTer` presenters' page slides.
//!
//! What shows: on a game list (Games, Favorites, Recents, Top played, the
//! collections, Decks/Deck, Search results) the focused item's art once
//! focus has rested `REST_MS`, never during a held scroll; everywhere else,
//! and for an item without art, the user's image, else the scheme's solid
//! `bg-deep`.
//!
//! The user's image is prepared once per output size and kept in `/tmp`
//! (RAM on `MiSTer`) for the next launch: the `MiSTer` wrapper relaunches
//! the frontend often, and a wallpaper takes seconds to decode there. Only
//! the prepared bitmap is kept, never the file or its metadata, and a
//! stale or unreadable entry is ignored.

use std::cell::RefCell;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use slint::ComponentHandle;
use zaparoo_app::background::{self as rules, Base, Fitted, Format, Rect, Regions};

use crate::{App, Backdrop, Screen, Theme};

/// The bitmap a head shows: the RGB8 pixels the view draws, on `MiSTer`
/// the presenter's own copy converted on the worker, and with glass cards
/// the same image at the card's darkness, shown inside the card.
#[cfg(feature = "mister")]
type Bitmap = crate::mister::Published;
#[cfg(not(feature = "mister"))]
#[derive(Clone)]
struct Bitmap {
    rgb: Arc<Vec<u8>>,
    card: Option<Box<Bitmap>>,
}

/// Bytes the backgrounds may hold, reserved out of the cover cache's cap so
/// the two together stay within it: the bitmaps on show and their copies
/// first, and the kept bases and fitted sources in what is left.
pub(crate) const CACHE_BYTES: usize = 16 * 1024 * 1024;

/// The longest a cold start waits for the user's image before showing the
/// restored screen over the solid color.
const CURTAIN_WAIT: Duration = Duration::from_secs(10);

/// Prepared bitmaps kept in the temporary directory, newest first.
const DISK_ENTRIES: usize = 2;

/// Bumped when the prepared format changes, so an old file is ignored.
const DISK_VERSION: u32 = 3;

/// The screens that follow the focused item.
const GAME_LISTS: [Screen; 7] = [
    Screen::Games,
    Screen::Favorites,
    Screen::Recents,
    Screen::SearchResults,
    Screen::Deck,
    Screen::Decks,
    Screen::TopPlayed,
];

/// The screens whose main card can be glass.
fn has_card(screen: Screen) -> bool {
    GAME_LISTS.contains(&screen)
        || matches!(
            screen,
            Screen::Systems
                | Screen::FavoriteSystems
                | Screen::TopPlayedSystems
                | Screen::Settings
                | Screen::About
                | Screen::Credits
                | Screen::Music
                | Screen::FileBrowser
        )
}

/// The background settings, as `SettingsState` holds them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Settings {
    pub image: String,
    pub game_type: Option<&'static str>,
    pub blur: i32,
    pub dim: i32,
    pub glass: bool,
}

impl Settings {
    pub(crate) fn from_state(s: &zaparoo_core::persist::SettingsState) -> Self {
        Self {
            image: s.background_image.trim().to_string(),
            game_type: rules::game_type(&s.game_background),
            blur: s.background_blur.clamp(0, rules::BLUR_MAX),
            dim: s
                .background_dim
                .clamp(rules::DIM_LIGHTEST, rules::DIM_DARKEST),
            glass: rules::glass(&s.background_list_style),
        }
    }

    /// A background image can show: the Blur, Dimming and Card style rows
    /// only matter then.
    pub(crate) fn active(&self) -> bool {
        !self.image.is_empty() || self.game_type.is_some()
    }
}

/// What the focused game-list item puts behind the screen, as reported by
/// the games driver on every render.
pub(crate) enum Focus {
    /// Its art has landed: an id for it and the decoded pixels.
    Art(u64, slint::SharedPixelBuffer<slint::Rgba8Pixel>),
    /// Its art is on its way; the background stays as it is.
    Waiting(u64),
    /// It has no art: the user's image or the solid color.
    None,
    /// A held scroll is running: nothing settles until it ends.
    Moving,
}

/// Where a background comes from.
#[derive(Clone)]
enum Source {
    /// The user's image setting: a file, or a folder to pick from.
    Custom(String),
    /// A focused item's art.
    Art(u64, slint::SharedPixelBuffer<slint::Rgba8Pixel>),
}

impl Source {
    fn id(&self) -> u64 {
        match self {
            Self::Custom(setting) => hash_of(&("custom", setting)),
            Self::Art(id, _) => *id,
        }
    }
}

/// Everything a head's bitmap depends on: equal keys show equal pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Shown {
    source: u64,
    width: u32,
    height: u32,
    blur: i32,
    dim: i32,
    ground: [u8; 3],
    text: [u8; 3],
    help_bar: Option<Rect>,
    /// Cards are glass: a card-darkness copy is made too.
    glass: bool,
}

impl Shown {
    fn base_key(&self) -> KeptKey {
        KeptKey::Base(self.source, self.width, self.height, self.blur)
    }

    fn fitted_key(&self) -> KeptKey {
        KeptKey::Fitted(self.source, self.width, self.height)
    }
}

/// What the kept cache holds: a blurred base by (source, size, blur), or a
/// fitted source by (source, size).
#[derive(Clone, PartialEq, Eq, Hash)]
enum KeptKey {
    Base(u64, u32, u32, i32),
    Fitted(u64, u32, u32),
}

#[derive(Clone)]
enum Kept {
    Base(Arc<Base>),
    Fitted(Arc<Fitted>),
}

/// Bytes a bitmap on show holds outside the kept cache: its pixels, the
/// presenter's or the view's copy, and the same for its card-darkness copy
/// (on `MiSTer` also the frame the presenter merges the card into).
fn shown_bytes(bitmap: &Bitmap) -> usize {
    let layer = |b: &Bitmap| {
        #[cfg(feature = "mister")]
        return b.rgb.len() + b.rgb565.as_ref().map_or(0, |p| p.len() * 2);
        #[cfg(not(feature = "mister"))]
        // The view's `Image` is a copy of its own.
        return b.rgb.len() * 2;
    };
    let card = bitmap.card.as_deref().map_or(0, |card| {
        let merged = if cfg!(feature = "mister") {
            card.rgb.len() / 3 * 2
        } else {
            0
        };
        layer(card) + merged
    });
    layer(bitmap) + card
}

/// Size the kept cache to what the bitmaps on show leave of
/// `CACHE_BYTES`, evicting the least recently used to fit.
fn rebudget(state: &mut State) {
    let shown: usize = state
        .heads
        .iter()
        .filter_map(|head| head.rgb.as_ref())
        .map(shown_bytes)
        .sum();
    state.kept.set_cap(CACHE_BYTES.saturating_sub(shown));
}

struct Head {
    app: slint::Weak<App>,
    /// The bitmap on show, kept to hand to the presenter again when the
    /// boot curtain lifts.
    #[cfg_attr(
        not(feature = "mister"),
        allow(dead_code, reason = "only the MiSTer presenters paint the background")
    )]
    rgb: Option<Bitmap>,
    format: Format,
    /// The main card the mounted screen reported, in logical pixels.
    card: Option<Rect>,
    /// Its corner radius, in logical pixels.
    card_radius: u32,
    /// The help bar's top edge in logical pixels, or none with the bar off.
    bar_top: Option<f32>,
    /// What the head shows now; `None` is the solid color.
    shown: Option<Shown>,
    /// A failed source, so it is not retried every refresh.
    failed: Option<u64>,
}

/// One job for the worker: make a head's bitmap.
struct Job {
    head: usize,
    shown: Shown,
    source: Source,
    format: Format,
    base: Option<Arc<Base>>,
    fitted: Option<Arc<Fitted>>,
}

struct Done {
    head: usize,
    shown: Shown,
    result: Result<Prepared, String>,
}

struct Prepared {
    base: Arc<Base>,
    fitted: Option<Arc<Fitted>>,
    bitmap: Bitmap,
}

/// What focus will show once it rests: art, or none.
struct Settle(Option<Source>);

struct State {
    settings: Settings,
    heads: Vec<Head>,
    crossfade: Duration,
    screen: Screen,
    /// The focused item's art once settled, and the latest report waiting
    /// out the rest delay.
    focus: Option<Source>,
    pending: Option<Settle>,
    /// The last focused art shown, for the Settings preview when no image
    /// of the user's own is set.
    last_art: Option<Source>,
    /// Blurred bases, and fitted sources a Blur change reruns the blur
    /// from instead of decoding again, sharing what the bitmaps on show
    /// leave of `CACHE_BYTES`.
    kept: Lru<KeptKey, Kept>,
    busy: bool,
    /// The cold start is holding its curtain for the user's image.
    curtain: Option<Instant>,
}

thread_local! {
    static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
    static REST: slint::Timer = slint::Timer::default();
    static FADE: slint::Timer = slint::Timer::default();
}

fn with_state<R>(f: impl FnOnce(&mut State) -> R) -> Option<R> {
    STATE.with(|cell| cell.borrow_mut().as_mut().map(f))
}

/// Bring the background up for `app` (and the `MiSTer` CRT head when there
/// is one) before the first frame. A user image holds the boot curtain
/// until it is ready, at most `CURTAIN_WAIT`.
pub(crate) fn start(app: &App, mirror: Option<&App>, settings: Settings, crossfade: Duration) {
    let format = if cfg!(feature = "mister") {
        Format::Rgb565
    } else {
        Format::Rgb888
    };
    let mut heads = vec![head(app, format)];
    if let Some(mirror) = mirror {
        heads.push(head(mirror, format));
    }
    for (index, view) in std::iter::once(app).chain(mirror).enumerate() {
        let backdrop = view.global::<Backdrop>();
        backdrop.set_fade_ms(fade_ms(crossfade));
        backdrop.on_card_moved(move |shown, x, y, w, h, radius| {
            let card = (shown && w > 0.0 && h > 0.0).then(|| Rect {
                x: x.max(0.0).round() as u32,
                y: y.max(0.0).round() as u32,
                width: w.round() as u32,
                height: h.round() as u32,
            });
            let radius = if card.is_some() {
                radius.max(0.0).round() as u32
            } else {
                0
            };
            with_state(|state| {
                let head = &mut state.heads[index];
                head.card = card;
                head.card_radius = radius;
            });
            push_cutout(index);
        });
        backdrop.on_bar_moved(move |top| {
            let changed = with_state(|state| {
                let head = &mut state.heads[index];
                let top = Some(top);
                let changed = head.bar_top != top;
                head.bar_top = top;
                changed
            });
            if changed == Some(true) {
                refresh();
            }
        });
    }
    let curtain = (!settings.image.is_empty()).then(Instant::now);
    STATE.with(|cell| {
        *cell.borrow_mut() = Some(State {
            settings,
            heads,
            crossfade,
            screen: Screen::None,
            focus: None,
            pending: None,
            last_art: None,
            kept: Lru::new(CACHE_BYTES),
            busy: false,
            curtain,
        });
    });
    if curtain.is_some() {
        let weak = app.as_weak();
        slint::Timer::single_shot(CURTAIN_WAIT, move || {
            let held = with_state(|state| state.curtain.take().is_some()).unwrap_or(false);
            if held {
                tracing::warn!("background image not ready in time; showing the screen");
                if let Some(app) = weak.upgrade() {
                    crate::router::background_ready(&app);
                }
            }
        });
    }
    refresh();
}

fn fade_ms(crossfade: Duration) -> i64 {
    i64::try_from(crossfade.as_millis()).unwrap_or(0)
}

/// Reduce motion changed: the GPU build's crossfade follows it.
pub(crate) fn set_crossfade(crossfade: Duration) {
    let apps = with_state(|state| {
        state.crossfade = crossfade;
        state
            .heads
            .iter()
            .filter_map(|head| head.app.upgrade())
            .collect::<Vec<_>>()
    })
    .unwrap_or_default();
    for app in apps {
        app.global::<Backdrop>().set_fade_ms(fade_ms(crossfade));
    }
}

fn head(app: &App, format: Format) -> Head {
    Head {
        app: app.as_weak(),
        rgb: None,
        format,
        card: None,
        card_radius: 0,
        // Until the bar reports, assume it is where the sizing table says.
        bar_top: None,
        shown: None,
        failed: None,
    }
}

/// Whether the cold start should keep its curtain up: the user's image is
/// still being prepared. The curtain comes down when it lands, fails, or
/// `CURTAIN_WAIT` runs out.
pub(crate) fn holds_curtain() -> bool {
    with_state(|state| state.curtain.is_some()).unwrap_or(false)
}

fn release_curtain(app: &App) {
    if with_state(|state| state.curtain.take().is_some()).unwrap_or(false) {
        crate::router::background_ready(app);
    }
}

/// The settings changed: show what they now ask for.
pub(crate) fn apply_settings(settings: Settings) {
    let changed = with_state(|state| {
        if state.settings == settings {
            return false;
        }
        if state.settings.image != settings.image {
            for head in &mut state.heads {
                head.failed = None;
            }
        }
        if settings.game_type.is_none() {
            state.focus = None;
            state.pending = None;
        }
        state.settings = settings;
        true
    });
    if changed == Some(true) {
        refresh();
    }
}

/// The game background type, for the games driver's art request.
pub(crate) fn game_type() -> Option<&'static str> {
    with_state(|state| state.settings.game_type).flatten()
}

/// The size to ask Core for: enough for the blur the user picked.
pub(crate) fn art_request_size() -> u32 {
    let blur = with_state(|state| state.settings.blur).unwrap_or(rules::DEFAULT_BLUR);
    if blur <= 20 {
        768
    } else {
        384
    }
}

/// The router committed a screen. Leaving the game lists drops the
/// focused art; a screen without a card drops the card.
pub(crate) fn screen_changed(target: Screen) {
    with_state(|state| {
        state.screen = target;
        if !GAME_LISTS.contains(&target) {
            state.focus = None;
            state.pending = None;
            REST.with(slint::Timer::stop);
        }
    });
    if !has_card(target) {
        let count = with_state(|state| {
            for head in &mut state.heads {
                head.card = None;
                head.card_radius = 0;
            }
            state.heads.len()
        })
        .unwrap_or(0);
        for index in 0..count {
            push_cutout(index);
        }
    }
    refresh();
}

/// Show a head's glass card where its screen last reported it, in the same
/// frame: the view draws the card-darkness image inside the card on the
/// GPU build, and the `MiSTer` presenter fills the card from it.
fn push_cutout(index: usize) {
    let Some((app, card, radius)) = with_state(|state| {
        let head = state.heads.get(index)?;
        Some((head.app.upgrade()?, head.card, head.card_radius))
    })
    .flatten() else {
        return;
    };
    let backdrop = app.global::<Backdrop>();
    if card.is_none() {
        backdrop.set_card_shown(false);
    }
    #[cfg(feature = "mister")]
    {
        let size = app.window().size();
        let scale = app.window().scale_factor();
        let scaled = |v: u32| (v as f32 * scale).round().max(0.0) as u32;
        let cutout = card.map(|r| crate::mister::Cutout {
            rect: Rect {
                x: scaled(r.x),
                y: scaled(r.y),
                width: scaled(r.width),
                height: scaled(r.height),
            },
            radius: scaled(radius),
        });
        crate::mister::set_cutout(size.width as usize, size.height as usize, cutout);
        app.window().request_redraw();
    }
    #[cfg(not(feature = "mister"))]
    let _ = radius;
}

/// The games driver's report of the focused item, on every render.
pub(crate) fn focus(report: Focus) {
    let next = match report {
        Focus::Moving => {
            // Nothing settles while a scroll runs; the next render after
            // it ends reports again.
            with_state(|state| state.pending = None);
            REST.with(slint::Timer::stop);
            return;
        }
        Focus::Waiting(id) => {
            let current = with_state(|state| state.focus.as_ref().map(Source::id)).flatten();
            if current == Some(id) {
                return;
            }
            // Keep showing what is there until the art lands.
            with_state(|state| state.pending = None);
            REST.with(slint::Timer::stop);
            return;
        }
        Focus::Art(id, buffer) => Some(Source::Art(id, buffer)),
        Focus::None => None,
    };
    let changed = with_state(|state| {
        if state.settings.game_type.is_none() {
            return false;
        }
        let next_id = next.as_ref().map(Source::id);
        let pending_id = state.pending.as_ref().map(|p| p.0.as_ref().map(Source::id));
        if pending_id == Some(next_id) {
            return false;
        }
        if state.pending.is_none() && state.focus.as_ref().map(Source::id) == next_id {
            return false;
        }
        state.pending = Some(Settle(next));
        true
    });
    if changed != Some(true) {
        return;
    }
    REST.with(|timer| {
        timer.start(
            slint::TimerMode::SingleShot,
            Duration::from_millis(rules::REST_MS),
            || {
                with_state(|state| {
                    if let Some(Settle(next)) = state.pending.take() {
                        if let Some(art) = &next {
                            state.last_art = Some(art.clone());
                        }
                        state.focus = next;
                    }
                });
                refresh();
            },
        );
    });
}

/// The source a head should show now.
fn wanted_source(state: &State) -> Option<Source> {
    if GAME_LISTS.contains(&state.screen) {
        if let Some(art) = &state.focus {
            return Some(art.clone());
        }
    }
    if !state.settings.image.is_empty() {
        return Some(Source::Custom(state.settings.image.clone()));
    }
    // Settings with only Game background on previews over the last art.
    if state.screen == Screen::Settings && state.settings.game_type.is_some() {
        return state.last_art.clone();
    }
    None
}

fn rgb(color: slint::Color) -> [u8; 3] {
    [color.red(), color.green(), color.blue()]
}

/// Bring every head to what it should show, one job at a time.
pub(crate) fn refresh() {
    let job = with_state(|state| {
        let source = wanted_source(state);
        let mut next_job = None;
        for index in 0..state.heads.len() {
            let Some(app) = state.heads[index].app.upgrade() else {
                continue;
            };
            let size = app.window().size();
            let scale = app.window().scale_factor();
            let Some(source) = source
                .clone()
                .filter(|s| state.heads[index].failed != Some(s.id()))
            else {
                show_none(state, index, &app);
                continue;
            };
            if size.width == 0 || size.height == 0 {
                continue;
            }
            let theme = app.global::<Theme>();
            let head = &state.heads[index];
            let scaled = |v: f32| (v * scale).round().max(0.0) as u32;
            let help_bar = head.bar_top.and_then(|top| {
                let top = scaled(top);
                (top < size.height).then(|| Rect {
                    x: 0,
                    y: top,
                    width: size.width,
                    height: size.height - top,
                })
            });
            let shown = Shown {
                source: source.id(),
                width: size.width,
                height: size.height,
                blur: state.settings.blur,
                dim: state.settings.dim,
                ground: rgb(theme.get_bg_deep()),
                text: rgb(theme.get_text_primary()),
                help_bar,
                glass: state.settings.glass,
            };
            if head.shown == Some(shown) {
                continue;
            }
            if next_job.is_none() && !state.busy {
                let base = match state.kept.get(&shown.base_key()) {
                    Some(Kept::Base(base)) => Some(base),
                    _ => None,
                };
                let fitted = match state.kept.get(&shown.fitted_key()) {
                    Some(Kept::Fitted(fitted)) => Some(fitted),
                    _ => None,
                };
                next_job = Some(Job {
                    head: index,
                    shown,
                    source,
                    format: head.format,
                    base,
                    fitted,
                });
            }
        }
        if next_job.is_some() {
            state.busy = true;
        }
        next_job
    })
    .flatten();
    if let Some(job) = job {
        run(job);
    }
}

/// Show the solid color on a head.
fn show_none(state: &mut State, index: usize, app: &App) {
    let head = &mut state.heads[index];
    if head.shown.is_none() && !app.global::<Backdrop>().get_on() {
        return;
    }
    head.shown = None;
    head.rgb = None;
    rebudget(state);
    let backdrop = app.global::<Backdrop>();
    backdrop.set_on(false);
    backdrop.set_glass(false);
    backdrop.set_fade(false);
    backdrop.set_card_image(slint::Image::default());
    #[cfg(feature = "mister")]
    {
        let size = app.window().size();
        publish(app, size.width, size.height, None);
    }
}

/// Hand a head's bitmap to its presenter, which then fills Slint's dirty
/// region from it with a plain copy and the view drops its `Image`
/// (`Backdrop.host-painted`): Slint's texture path costs a per-pixel gather
/// that made a glass list scroll miss its frames. Only an unrotated frame
/// matches the bitmap's layout; a rotated one keeps the `Image`. Held back
/// while the boot curtain is up, so the restored screen and its
/// background appear together (`curtain_lifted`).
#[cfg(feature = "mister")]
fn publish(app: &App, width: u32, height: u32, bitmap: Option<&Bitmap>) -> bool {
    let shell = app.global::<crate::Shell>();
    let unrotated = shell.get_orientation() == crate::Orientation::Horizontal;
    let bitmap = bitmap.filter(|_| unrotated && !shell.get_boot_curtain());
    let painted = bitmap.is_some();
    crate::mister::set_background(width as usize, height as usize, bitmap.cloned());
    app.global::<Backdrop>().set_host_painted(painted);
    // The presenter repaints the whole frame on the next render; make
    // sure there is one.
    app.window().request_redraw();
    painted
}

/// The boot curtain came down: the presenters take the bitmaps they were
/// held back from.
pub(crate) fn curtain_lifted() {
    #[cfg(feature = "mister")]
    {
        let shown = with_state(|state| {
            state
                .heads
                .iter()
                .filter_map(|head| Some((head.app.upgrade()?, head.shown?, head.rgb.clone()?)))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
        for (app, shown, rgb) in shown {
            if publish(&app, shown.width, shown.height, Some(&rgb)) {
                continue;
            }
            // Rotated: the view draws it after all.
            let backdrop = app.global::<Backdrop>();
            backdrop.set_image(view_image(&rgb, &shown));
            backdrop.set_card_image(card_image(&rgb, &shown));
        }
    }
}

fn run(job: Job) {
    let spawned = std::thread::Builder::new()
        .name("background".into())
        .spawn(move || {
            lowest_priority();
            let started = Instant::now();
            let result = prepare(&job);
            if let Ok(prepared) = &result {
                tracing::debug!(
                    width = job.shown.width,
                    height = job.shown.height,
                    reused_base = job.base.is_some(),
                    bytes = prepared.bitmap.rgb.len(),
                    ms = started.elapsed().as_millis() as u64,
                    "background prepared"
                );
            }
            let done = Done {
                head: job.head,
                shown: job.shown,
                result,
            };
            let _ = slint::invoke_from_event_loop(move || landed(done));
        });
    if let Err(error) = spawned {
        tracing::warn!(%error, "background worker did not start");
        with_state(|state| state.busy = false);
    }
}

/// Run the calling thread at the lowest CPU priority. On the `MiSTer`,
/// `MiSTer_Zaparoo` pins every frontend thread to CPU 0, so the worker
/// shares that one core with the render thread and gives way to it.
fn lowest_priority() {
    #[cfg(feature = "mister")]
    // SAFETY: `setpriority` on `PRIO_PROCESS` with who 0 changes only the
    // calling thread's nice value on Linux; it reads no memory of ours.
    unsafe {
        libc::setpriority(libc::PRIO_PROCESS, 0, 19);
    }
}

/// Make a head's bitmap: decode and fit when nothing is kept, blur when
/// no base is kept, then compose.
fn prepare(job: &Job) -> Result<Prepared, String> {
    let shown = &job.shown;
    let mut fitted = job.fitted.clone();
    let kept = match &job.source {
        Source::Custom(setting) if job.base.is_none() => disk::load(setting, shown).map(Arc::new),
        _ => job.base.clone(),
    };
    let base = if let Some(base) = kept {
        base
    } else {
        let fit = if let Some(fit) = fitted.clone() {
            fit
        } else {
            let fit = Arc::new(fit_source(&job.source, shown)?);
            fitted = Some(fit.clone());
            fit
        };
        let base = rules::prepare_base(&fit, shown.blur);
        if let Source::Custom(setting) = &job.source {
            disk::store(setting, shown, &base);
        }
        Arc::new(base)
    };
    let readable = rules::readable_dim(&base.samples, shown.text, shown.ground);
    let dim = rules::dim_for(readable, shown.dim);
    let regions = Regions {
        help_bar: shown.help_bar,
        card: None,
        card_radius: 0,
    };
    let rgb = rules::compose(&base, shown.ground, dim, &regions, job.format);
    let card = shown.glass.then(|| {
        bitmap(
            rules::compose_card(&base, shown.ground, dim, job.format),
            job.format,
            None,
        )
    });
    Ok(Prepared {
        base,
        fitted,
        bitmap: bitmap(rgb, job.format, card),
    })
}

fn bitmap(rgb: Vec<u8>, format: Format, card: Option<Bitmap>) -> Bitmap {
    let card = card.map(Box::new);
    #[cfg(feature = "mister")]
    {
        let rgb565 = (format == Format::Rgb565).then(|| Arc::new(crate::mister::to_rgb565(&rgb)));
        crate::mister::Published {
            rgb: Arc::new(rgb),
            rgb565,
            card,
        }
    }
    #[cfg(not(feature = "mister"))]
    {
        let _ = format;
        Bitmap {
            rgb: Arc::new(rgb),
            card,
        }
    }
}

fn fit_source(source: &Source, shown: &Shown) -> Result<Fitted, String> {
    match source {
        Source::Custom(setting) => {
            let path = disk::resolve(setting)?;
            let image = decode_file(&path)?;
            Ok(rules::fit(
                image.as_raw(),
                image.width(),
                image.height(),
                shown.width,
                shown.height,
            ))
        }
        Source::Art(_, buffer) => {
            let (w, h) = (buffer.width(), buffer.height());
            let rgb: Vec<u8> = buffer
                .as_bytes()
                .chunks_exact(4)
                .flat_map(|px| [px[0], px[1], px[2]])
                .collect();
            Ok(rules::fit(&rgb, w, h, shown.width, shown.height))
        }
    }
}

/// Decode a user image within the customization budgets.
fn decode_file(path: &Path) -> Result<image::RgbImage, String> {
    use zaparoo_app::customization::{ART_DECODE_BYTES, ART_FILE_BYTES, ART_SOURCE_EDGE};
    let size = std::fs::metadata(path).map_err(|e| e.to_string())?.len();
    if size > ART_FILE_BYTES {
        return Err(format!(
            "{} is larger than {ART_FILE_BYTES} bytes",
            path.display()
        ));
    }
    let mut reader = image::ImageReader::open(path)
        .map_err(|e| e.to_string())?
        .with_guessed_format()
        .map_err(|e| e.to_string())?;
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(ART_DECODE_BYTES);
    limits.max_image_width = Some(ART_SOURCE_EDGE);
    limits.max_image_height = Some(ART_SOURCE_EDGE);
    reader.limits(limits);
    Ok(reader.decode().map_err(|e| e.to_string())?.into_rgb8())
}

fn landed(done: Done) {
    let app = with_state(|state| {
        state.busy = false;
        state
            .heads
            .get(done.head)
            .and_then(|head| head.app.upgrade())
    })
    .flatten();
    let Some(app) = app else {
        return;
    };
    match done.result {
        Ok(prepared) => {
            let stale = with_state(|state| {
                let bytes = prepared.base.bytes();
                state.kept.put(
                    done.shown.base_key(),
                    Kept::Base(prepared.base.clone()),
                    bytes,
                );
                if let Some(fitted) = &prepared.fitted {
                    state.kept.put(
                        done.shown.fitted_key(),
                        Kept::Fitted(fitted.clone()),
                        fitted.rgb.len(),
                    );
                }
                // Applied only if it is still what this head should show;
                // otherwise the refresh below makes the right one.
                let still = wanted_source(state).map(|s| s.id()) == Some(done.shown.source);
                let size = app.window().size();
                !still || (size.width, size.height) != (done.shown.width, done.shown.height)
            })
            .unwrap_or(true);
            if !stale {
                show(&app, done.head, done.shown, &prepared.bitmap);
            }
        }
        Err(error) => {
            tracing::warn!(%error, "background image could not be prepared");
            with_state(|state| {
                if let Some(head) = state.heads.get_mut(done.head) {
                    head.failed = Some(done.shown.source);
                }
            });
        }
    }
    if done.head == 0 {
        release_curtain(&app);
    }
    refresh();
}

fn view_image(bitmap: &Bitmap, shown: &Shown) -> slint::Image {
    let buffer = slint::SharedPixelBuffer::<slint::Rgb8Pixel>::clone_from_slice(
        &bitmap.rgb,
        shown.width,
        shown.height,
    );
    slint::Image::from_rgb8(buffer)
}

/// The card-darkness image the view draws inside a glass card, or none.
fn card_image(bitmap: &Bitmap, shown: &Shown) -> slint::Image {
    bitmap
        .card
        .as_deref()
        .map_or_else(slint::Image::default, |card| view_image(card, shown))
}

fn show(app: &App, index: usize, shown: Shown, bitmap: &Bitmap) {
    let (crossfade, was_on, glass) = with_state(|state| {
        let head = &mut state.heads[index];
        let was_on = head.shown.is_some_and(|old| old.source != shown.source);
        head.shown = Some(shown);
        head.rgb = Some(bitmap.clone());
        rebudget(state);
        (state.crossfade, was_on, bitmap.card.is_some())
    })
    .unwrap_or_default();
    let backdrop = app.global::<Backdrop>();
    backdrop.set_glass(glass);
    // On MiSTer the presenter paints it; the view needs no copy of its own.
    #[cfg(feature = "mister")]
    let painted = {
        crate::mister::set_ground(shown.ground);
        publish(app, shown.width, shown.height, Some(bitmap))
    };
    #[cfg(not(feature = "mister"))]
    let painted = false;
    if painted {
        FADE.with(slint::Timer::stop);
        backdrop.set_fade(false);
    } else if !crossfade.is_zero() && was_on && backdrop.get_on() {
        // GPU build: the new image fades in over the old, then takes its
        // place.
        backdrop.set_next(view_image(bitmap, &shown));
        backdrop.set_card_next(card_image(bitmap, &shown));
        backdrop.set_fade(true);
        let weak = app.as_weak();
        FADE.with(|timer| {
            timer.start(slint::TimerMode::SingleShot, crossfade, move || {
                if let Some(app) = weak.upgrade() {
                    let backdrop = app.global::<Backdrop>();
                    backdrop.set_image(backdrop.get_next());
                    backdrop.set_card_image(backdrop.get_card_next());
                    backdrop.set_fade(false);
                }
            });
        });
    } else {
        FADE.with(slint::Timer::stop);
        backdrop.set_image(view_image(bitmap, &shown));
        backdrop.set_card_image(card_image(bitmap, &shown));
        backdrop.set_fade(false);
    }
    backdrop.set_on(true);
}

/// Publish the scheme's ground to the page slides after a palette change,
/// for the gap between pages when no image shows.
pub(crate) fn ground_changed(app: &App) {
    #[cfg(feature = "mister")]
    crate::mister::set_ground(rgb(app.global::<Theme>().get_bg_deep()));
    #[cfg(not(feature = "mister"))]
    let _ = app;
    refresh();
}

fn hash_of(value: &impl Hash) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}

/// A byte-capped least-recently-used map.
struct Lru<K, V> {
    cap: usize,
    bytes: usize,
    tick: u64,
    entries: HashMap<K, (V, usize, u64)>,
}

impl<K: Hash + Eq + Clone, V: Clone> Lru<K, V> {
    fn new(cap: usize) -> Self {
        Self {
            cap,
            bytes: 0,
            tick: 0,
            entries: HashMap::new(),
        }
    }

    fn get(&mut self, key: &K) -> Option<V> {
        self.tick += 1;
        let tick = self.tick;
        self.entries.get_mut(key).map(|(value, _, used)| {
            *used = tick;
            value.clone()
        })
    }

    fn put(&mut self, key: K, value: V, bytes: usize) {
        self.tick += 1;
        if let Some((_, old, _)) = self.entries.insert(key, (value, bytes, self.tick)) {
            self.bytes -= old;
        }
        self.bytes += bytes;
        self.trim();
    }

    /// A new budget, evicting to fit it.
    fn set_cap(&mut self, cap: usize) {
        self.cap = cap;
        self.trim();
    }

    /// Evict the least recently used until the bytes fit, keeping the
    /// newest entry even when it alone is over: it is what the head on
    /// show was just made from.
    fn trim(&mut self) {
        while self.bytes > self.cap && self.entries.len() > 1 {
            let Some(oldest) = self
                .entries
                .iter()
                .min_by_key(|(_, (_, _, used))| *used)
                .map(|(key, _)| key.clone())
            else {
                break;
            };
            if let Some((_, bytes, _)) = self.entries.remove(&oldest) {
                self.bytes -= bytes;
            }
        }
    }
}

/// The user's image: which file a setting names, and the prepared bitmaps
/// kept in the temporary directory between launches.
mod disk {
    use super::{hash_of, Base, Shown, DISK_ENTRIES, DISK_VERSION};
    use std::path::{Path, PathBuf};

    const EXTENSIONS: [&str; 5] = ["png", "jpg", "jpeg", "webp", "bmp"];

    fn dir() -> PathBuf {
        std::env::temp_dir().join("zaparoo-background")
    }

    /// The file a setting names. A folder picks one of its images at
    /// random, once per power cycle: the pick is written beside the
    /// prepared bitmaps, which the temporary directory loses on reboot.
    pub(super) fn resolve(setting: &str) -> Result<PathBuf, String> {
        let path = Path::new(setting);
        if !path.is_dir() {
            return if path.is_file() {
                Ok(path.to_path_buf())
            } else {
                Err(format!("{setting} not found"))
            };
        }
        let pick_file = dir().join(format!("pick-{:016x}", hash_of(&setting)));
        if let Ok(name) = std::fs::read_to_string(&pick_file) {
            let picked = path.join(name.trim());
            if picked.is_file() {
                return Ok(picked);
            }
        }
        let mut images: Vec<PathBuf> = std::fs::read_dir(path)
            .map_err(|e| e.to_string())?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|p| {
                p.is_file()
                    && p.extension().is_some_and(|ext| {
                        EXTENSIONS.contains(&ext.to_string_lossy().to_ascii_lowercase().as_str())
                    })
            })
            .collect();
        if images.is_empty() {
            return Err(format!("{setting} has no images"));
        }
        images.sort();
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.subsec_nanos() as usize ^ d.as_secs() as usize);
        let picked = images.swap_remove(seed % images.len());
        if let Some(name) = picked.file_name() {
            let _ = std::fs::create_dir_all(dir());
            let _ = std::fs::write(&pick_file, name.to_string_lossy().as_bytes());
        }
        Ok(picked)
    }

    /// The file a prepared bitmap is kept in: everything it depends on is
    /// in the name, so a changed file or setting never reads a stale one.
    fn entry(setting: &str, shown: &Shown) -> Option<PathBuf> {
        let path = resolve(setting).ok()?;
        let meta = std::fs::metadata(&path).ok()?;
        let modified = meta
            .modified()
            .ok()?
            .duration_since(std::time::UNIX_EPOCH)
            .ok()?
            .as_nanos();
        let key = hash_of(&(
            DISK_VERSION,
            path,
            meta.len(),
            modified,
            shown.width,
            shown.height,
            shown.blur,
        ));
        Some(dir().join(format!("base-{key:016x}.bin")))
    }

    pub(super) fn load(setting: &str, shown: &Shown) -> Option<Base> {
        let bytes = std::fs::read(entry(setting, shown)?).ok()?;
        let word = |at: usize| -> Option<u32> {
            Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
        };
        let (width, height, samples) = (word(0)?, word(4)?, word(8)? as usize);
        let rgb_len = width as usize * height as usize * 3;
        if (width, height) != (shown.width, shown.height)
            || bytes.len() != 12 + samples * 3 + rgb_len
        {
            return None;
        }
        let samples_end = 12 + samples * 3;
        Some(Base {
            width,
            height,
            samples: bytes[12..samples_end]
                .chunks_exact(3)
                .map(|px| [px[0], px[1], px[2]])
                .collect(),
            rgb: bytes[samples_end..].to_vec(),
        })
    }

    pub(super) fn store(setting: &str, shown: &Shown, base: &Base) {
        let Some(path) = entry(setting, shown) else {
            return;
        };
        let mut bytes = Vec::with_capacity(12 + base.samples.len() * 3 + base.rgb.len());
        bytes.extend_from_slice(&base.width.to_le_bytes());
        bytes.extend_from_slice(&base.height.to_le_bytes());
        bytes.extend_from_slice(&(base.samples.len() as u32).to_le_bytes());
        bytes.extend(base.samples.iter().flatten());
        bytes.extend_from_slice(&base.rgb);
        if std::fs::create_dir_all(dir()).is_err() {
            return;
        }
        let tmp = path.with_extension("tmp");
        if std::fs::write(&tmp, &bytes).is_err() || std::fs::rename(&tmp, &path).is_err() {
            let _ = std::fs::remove_file(&tmp);
            return;
        }
        prune(&path);
    }

    /// Keep the newest `DISK_ENTRIES` bitmaps.
    fn prune(keep: &Path) {
        let Ok(entries) = std::fs::read_dir(dir()) else {
            return;
        };
        let mut bases: Vec<(std::time::SystemTime, PathBuf)> = entries
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| {
                p != keep
                    && p.file_name()
                        .is_some_and(|n| n.to_string_lossy().starts_with("base-"))
            })
            .filter_map(|p| Some((std::fs::metadata(&p).ok()?.modified().ok()?, p)))
            .collect();
        bases.sort_by_key(|(modified, _)| std::cmp::Reverse(*modified));
        for (_, path) in bases.into_iter().skip(DISK_ENTRIES - 1) {
            let _ = std::fs::remove_file(path);
        }
    }
}

/// The name a background image setting shows in Settings: the file or
/// folder name.
pub(crate) fn setting_name(setting: &str) -> String {
    Path::new(setting)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// The folder the picker opens in: the current image's folder, the folder
/// itself, or the customization root.
pub(crate) fn picker_start(setting: &str, custom_root: Option<PathBuf>) -> Option<PathBuf> {
    let path = Path::new(setting);
    if path.is_dir() {
        return Some(path.to_path_buf());
    }
    path.parent()
        .filter(|p| p.is_dir())
        .map(Path::to_path_buf)
        .or(custom_root)
}

#[cfg(test)]
mod tests {
    use super::Lru;

    #[test]
    fn a_smaller_budget_evicts_the_least_recently_used() {
        let mut lru = Lru::new(30);
        lru.put(1, 'a', 10);
        lru.put(2, 'b', 10);
        lru.put(3, 'c', 10);
        assert_eq!(lru.get(&1), Some('a'));
        // The bitmaps on show grew: 2 is the oldest unused, then 3.
        lru.set_cap(20);
        assert_eq!(lru.get(&2), None);
        assert_eq!(lru.get(&1), Some('a'));
        lru.set_cap(5);
        // The most recently used entry stays, even alone over budget.
        assert_eq!(lru.entries.len(), 1);
    }
}
