// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// "Choose a game": a root screen that hands one game back to the surface
// that asked for it, or nothing on Back. The caller passes a purpose; the
// picker remembers the screen it was opened from and returns there, and
// every fill carries a ticket so a late answer for a source the user has
// left, or a picker that closed, is dropped. The rows come from the same
// endpoints and the same `GameRow` the list screens read, and move
// through `browse_motion` like every browse list. Rules live in
// `zaparoo_app::game_picker`.
//
// The picker is never restored after a kill: it is not written as the
// active screen, so a cold start lands on the screen that opened it.

use slint::{ComponentHandle, SharedString};
use zaparoo_app::game_picker::{self as rules, Back, Pane, Scene, Source};
use zaparoo_app::keyboard::{Edge, Key, Layers};
use zaparoo_app::media_list as list_rules;
use zaparoo_app::search as search_rules;
use zaparoo_core::endpoints::decks::DeckEndpoint;
use zaparoo_core::endpoints::media_history::{HistoryArgs, MediaHistoryEndpoint};
use zaparoo_core::endpoints::media_search::{MediaSearchEndpoint, SearchArgs};
use zaparoo_core::endpoints::media_user_list::{MediaUserListEndpoint, UserListArgs};
use zaparoo_core::input_actions::actions;

use crate::games::GameRow;
use crate::keyboard::{Flash, Keyboard, Press};
use crate::router::{lock, Ctx, Shared};
use crate::{
    App, ContentState, GamePickerInput, GamePickerView, GridCell, PickerGamesView,
    PickerSourcesView,
};

/// Why the picker is open, and so who gets the game.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Purpose {
    /// The blank-token panel, or the inspector's Write a game: write the
    /// game to the token on this reader.
    WriteToken { reader_id: Option<String> },
    /// The inspector's Map to a game: map this token ID to the game.
    MapToken { uid: String },
    /// The Mappings page's Change game.
    ChangeMapping { id: i64 },
}

/// The game a picker hands back.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PickedGame {
    pub name: String,
    pub system: String,
    pub path: String,
    /// The script that launches it: the row's own `ZapScript`, else its path.
    pub zapscript: String,
}

impl PickedGame {
    /// A list row as a game to hand back: folders and decks are not games.
    pub(crate) fn from_row(row: &GameRow) -> Option<Self> {
        if row.is_dir() || row.deck_entry.is_some() {
            return None;
        }
        // A deck item is its own script (launcher or options included),
        // the one the deck plays and "Write games to tokens" writes.
        let deck_script = row
            .deck_item
            .as_ref()
            .and(Some(row.zap_script.trim()))
            .filter(|script| !script.is_empty())
            .map(str::to_string);
        Some(Self {
            name: row.display.clone(),
            system: row.system_id.clone(),
            path: row.path.clone(),
            zapscript: deck_script.or_else(|| row.launch_text())?,
        })
    }
}

#[derive(Debug)]
pub struct Model {
    /// Set while the picker is open.
    purpose: Option<Purpose>,
    /// The screen it was opened from, which Back and a pick return to.
    caller: crate::Screen,
    sources: Vec<Source>,
    source_index: usize,
    sources_top: usize,
    pane: Pane,
    /// The source the games pane shows, once one was asked for.
    shown: Option<Source>,
    rows: Vec<GameRow>,
    state: ContentState,
    game_index: usize,
    games_top: usize,
    /// Bumped by every fill and on close: an answer carrying another is
    /// dropped.
    ticket: u64,
    task: Option<tokio::task::JoinHandle<()>>,
    keyboard: Keyboard,
    flash: Flash,
    /// The query the games pane's results answer, once Search ran one.
    searched: Option<String>,
}

impl Default for Model {
    fn default() -> Self {
        Self {
            purpose: None,
            caller: crate::Screen::Hub,
            sources: Vec::new(),
            source_index: 0,
            sources_top: 0,
            pane: Pane::Sources,
            shown: None,
            rows: Vec::new(),
            state: ContentState::Loading,
            game_index: 0,
            games_top: 0,
            ticket: 0,
            task: None,
            keyboard: Keyboard::new(Layers::Basic, search_rules::QUERY_MAX_CHARS),
            flash: Flash::default(),
            searched: None,
        }
    }
}

impl Model {
    fn source(&self) -> Option<&Source> {
        self.sources.get(self.source_index)
    }

    fn cancel_fill(&mut self) {
        self.ticket = self.ticket.wrapping_add(1);
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}

/// The picker is the active screen.
pub(crate) fn is_active(app: &App) -> bool {
    app.global::<crate::Shell>().get_active_screen() == crate::Screen::GamePicker
}

fn deck_sources(shared: &Shared) -> Vec<(String, String)> {
    shared
        .decks
        .list
        .as_deref()
        .unwrap_or_default()
        .iter()
        .map(|deck| (deck.deck_id.clone(), deck.name.clone()))
        .collect()
}

/// Open the picker for `purpose`, over the screen that is up now.
pub fn open(ctx: &Ctx, app: &App, purpose: Purpose) {
    let caller = app.global::<crate::Shell>().get_active_screen();
    let kind = match &purpose {
        Purpose::WriteToken { .. } => crate::PickerPurpose::WriteToken,
        Purpose::MapToken { .. } => crate::PickerPurpose::MapToken,
        Purpose::ChangeMapping { .. } => crate::PickerPurpose::ChangeMapping,
    };
    {
        let mut shared = lock(&ctx.shared);
        let decks = deck_sources(&shared);
        let model = &mut shared.game_picker;
        model.cancel_fill();
        *model = Model {
            ticket: model.ticket,
            ..Model::default()
        };
        model.purpose = Some(purpose);
        model.caller = if caller == crate::Screen::GamePicker {
            crate::Screen::Hub
        } else {
            caller
        };
        model.sources = rules::sources(decks);
        model.source_index = rules::step(&model.sources, 0, 0);
    }
    app.global::<GamePickerView>().set_purpose(kind);
    if two_panes(ctx, app) {
        show_source(ctx, app);
    }
    render(ctx, app);
    crate::router::transition_to_screen(app, crate::Screen::GamePicker, 1);
}

/// Close the picker with `game` (none on Back) and hand it to the caller.
fn finish(ctx: &Ctx, app: &App, game: Option<PickedGame>) {
    let (purpose, caller) = {
        let mut shared = lock(&ctx.shared);
        let model = &mut shared.game_picker;
        model.cancel_fill();
        model.rows.clear();
        model.shown = None;
        (model.purpose.take(), model.caller)
    };
    publish_games(ctx, app);
    crate::router::transition_to_screen(app, caller, -1);
    if let Some(purpose) = purpose {
        deliver(ctx, app, purpose, game);
    }
}

/// Hand the picker's answer to the surface that asked for it.
pub fn deliver(ctx: &Ctx, app: &App, purpose: Purpose, game: Option<PickedGame>) {
    match purpose {
        Purpose::WriteToken { reader_id } => {
            crate::blank_token::picked(ctx, app, reader_id, game);
        }
        Purpose::MapToken { uid } => crate::token_inspector::picked(ctx, app, &uid, game),
        Purpose::ChangeMapping { id } => crate::mappings::picked(ctx, app, id, game),
    }
}

// ---------- Layout ----------

fn rotated(shared: &Shared) -> bool {
    matches!(shared.persist.settings.orientation.as_str(), "cw" | "ccw")
}

fn scene(ctx: &Ctx, app: &App) -> Scene {
    let inputs = crate::router::output_scene(app).inputs();
    let derived = zaparoo_app::sizing::derive(&inputs);
    let shared = lock(&ctx.shared);
    Scene {
        tier_240: derived.tier == zaparoo_app::sizing::Tier::T240,
        crt: ctx.crt_enabled,
        rotated: rotated(&shared),
        text_only: shared.persist.settings.games_browse_layout == "list-only",
    }
}

fn two_panes(ctx: &Ctx, app: &App) -> bool {
    rules::two_panes(scene(ctx, app))
}

fn geometry(ctx: &Ctx, app: &App) -> list_rules::ListGeometry {
    let inputs = crate::router::output_scene(app).inputs();
    let rotated = rotated(&lock(&ctx.shared));
    crate::games::list_geometry_for(
        &inputs,
        rotated,
        0,
        list_rules::list_visible_rows(ctx.crt_enabled, rotated),
    )
}

fn visible(ctx: &Ctx, app: &App) -> usize {
    geometry(ctx, app).visible_rows.max(1)
}

// ---------- Fills ----------

/// Rows ready to show: their display names, as the list screens make them.
fn prepared(shared: &Shared, mut rows: Vec<GameRow>) -> Vec<GameRow> {
    let settings = &shared.persist.settings;
    let language = crate::effective_language(&settings.language);
    let count = rows.len();
    crate::games::refresh_displays(
        &mut rows,
        0..count,
        settings.show_tags,
        settings.show_original_filenames,
        &language,
    );
    rows
}

/// A fill landed: show it if its ticket is still the picker's.
fn landed(ctx: &Ctx, app: &App, ticket: u64, rows: Result<Vec<GameRow>, String>) {
    {
        let mut shared = lock(&ctx.shared);
        if shared.game_picker.ticket != ticket
            || shared.game_picker.purpose.is_none()
            || !is_active(app)
        {
            return;
        }
        let rows = match rows {
            Ok(rows) => {
                let rows = prepared(&shared, rows);
                rows.into_iter()
                    .filter(|row| PickedGame::from_row(row).is_some())
                    .collect::<Vec<_>>()
            }
            Err(error) => {
                tracing::warn!("game picker: fill failed: {error}");
                let model = &mut shared.game_picker;
                model.task = None;
                model.rows.clear();
                model.state = ContentState::Error;
                drop(shared);
                render(ctx, app);
                return;
            }
        };
        let model = &mut shared.game_picker;
        model.task = None;
        model.state = if rows.is_empty() {
            ContentState::Empty
        } else {
            ContentState::Ready
        };
        model.rows = rows;
        model.game_index = 0;
        model.games_top = 0;
    }
    render(ctx, app);
}

/// Fill the games pane from the focused source. Search with no query run
/// yet shows its keyboard instead.
fn show_source(ctx: &Ctx, app: &App) {
    let (ticket, source, query) = {
        let mut shared = lock(&ctx.shared);
        let model = &mut shared.game_picker;
        let Some(source) = model.source().cloned() else {
            return;
        };
        if model.shown.as_ref() == Some(&source) && model.state != ContentState::Error {
            return;
        }
        model.cancel_fill();
        model.shown = Some(source.clone());
        model.rows.clear();
        model.game_index = 0;
        model.games_top = 0;
        model.state = ContentState::Loading;
        (model.ticket, source, model.searched.clone())
    };
    let limit = rules::FILL_LIMIT;
    let search_rows = |result: &zaparoo_core::media_types::MediaSearchResult| {
        result.results.iter().map(GameRow::from).collect::<Vec<_>>()
    };
    let task = match source {
        Source::Recents => Some(crate::games::settled::<MediaHistoryEndpoint>(
            ctx,
            app,
            HistoryArgs::new(Vec::new(), limit),
            move |ctx, app, result| {
                let rows = result.map(|r| r.entries.iter().map(GameRow::from).collect());
                landed(ctx, app, ticket, rows);
            },
        )),
        Source::Favorites | Source::Liked | Source::PlayLater => {
            let flag = match source {
                Source::Favorites => zaparoo_app::user_flags::Flag::Favorite,
                Source::Liked => zaparoo_app::user_flags::Flag::Like,
                _ => zaparoo_app::user_flags::Flag::PlayLater,
            };
            Some(crate::games::settled::<MediaUserListEndpoint>(
                ctx,
                app,
                UserListArgs::new(crate::user_lists::user_list(flag), limit, None, Vec::new()),
                move |ctx, app, result| landed(ctx, app, ticket, result.map(|r| search_rows(&r))),
            ))
        }
        Source::Deck { id, .. } => Some(crate::games::settled::<DeckEndpoint>(
            ctx,
            app,
            id,
            move |ctx, app, result| {
                landed(
                    ctx,
                    app,
                    ticket,
                    result.map(|deck| crate::decks::rows(&deck)),
                );
            },
        )),
        Source::Search => query.map(|query| {
            let include_hidden = lock(&ctx.shared).show_hidden;
            crate::games::settled::<MediaSearchEndpoint>(
                ctx,
                app,
                SearchArgs {
                    query,
                    systems: Vec::new(),
                    tags: Vec::new(),
                    path_prefix: String::new(),
                    max_results: limit,
                    sort: search_rules::RESULT_SORT.to_string(),
                    include_hidden,
                },
                move |ctx, app, result| landed(ctx, app, ticket, result.map(|r| search_rows(&r))),
            )
        }),
        Source::DecksHeading => None,
    };
    lock(&ctx.shared).game_picker.task = task;
}

/// Run the typed search into the games pane.
fn run_search(ctx: &Ctx, app: &App) {
    {
        let mut shared = lock(&ctx.shared);
        let model = &mut shared.game_picker;
        let query = search_rules::query_text(model.keyboard.text()).to_string();
        if query.is_empty() {
            return;
        }
        model.searched = Some(query);
        model.shown = None;
        model.pane = Pane::Games;
    }
    show_source(ctx, app);
    render(ctx, app);
}

// ---------- Rendering ----------

fn source_cell(app: &App, source: &Source) -> GridCell {
    let labels = app.global::<crate::Labels>();
    let (key, name) = match source {
        Source::Recents => ("recents", ""),
        Source::Favorites => ("favorites", ""),
        Source::Liked => ("liked", ""),
        Source::PlayLater => ("play_later", ""),
        Source::DecksHeading => ("decks", ""),
        Source::Deck { name, .. } => ("", name.as_str()),
        Source::Search => ("search", ""),
    };
    GridCell {
        name: labels.invoke_picker_source(SharedString::from(key), SharedString::from(name)),
        disabled: !source.selectable(),
        heading: !source.selectable(),
        opens: source.selectable(),
        ..Default::default()
    }
}

fn publish_games(ctx: &Ctx, app: &App) {
    let visible = visible(ctx, app);
    let mut shared = lock(&ctx.shared);
    let view = app.global::<PickerGamesView>();
    let shared = &mut *shared;
    let model = &mut shared.game_picker;
    let count = model.rows.len();
    let current = model.game_index.min(count.saturating_sub(1));
    model.games_top = crate::browse_motion::publish_list(
        &view,
        &shared.input,
        current,
        count,
        visible,
        model.games_top,
        &model.rows,
        crate::games::text_cell,
    );
}

/// Paint the picker from its model.
pub(crate) fn render(ctx: &Ctx, app: &App) {
    let two = two_panes(ctx, app);
    let g = geometry(ctx, app);
    let visible = g.visible_rows.max(1);
    let view = app.global::<GamePickerView>();
    view.set_two_panes(two);
    view.set_card_x(g.card_x as f32);
    view.set_card_y(g.card_y as f32);
    view.set_card_width(g.card_width as f32);
    view.set_card_height(g.card_height as f32);
    view.set_row_height(g.row_height as f32);
    let gap = (g.detail_x - g.list_width).max(1);
    let (left, games_x, right) = rules::split(g.card_width, gap);
    view.set_sources_width(left as f32);
    view.set_games_x(games_x as f32);
    view.set_games_width(right as f32);
    {
        let mut shared = lock(&ctx.shared);
        let shared = &mut *shared;
        let model = &mut shared.game_picker;
        let pane = model.pane;
        view.set_pane(match pane {
            Pane::Sources => crate::PickerPane::Sources,
            Pane::Games => crate::PickerPane::Games,
            Pane::Keyboard => crate::PickerPane::Keyboard,
        });
        let search_focused = model.source() == Some(&Source::Search);
        let keyboard_shown = pane == Pane::Keyboard
            || (two && pane == Pane::Sources && search_focused && model.searched.is_none());
        view.set_keyboard_shown(keyboard_shown);
        view.set_games_state(model.state);
        view.set_source_name(
            model
                .source()
                .map(|source| source_cell(app, source).name)
                .unwrap_or_default(),
        );
        view.set_count(i32::try_from(model.rows.len()).unwrap_or(i32::MAX));
        view.set_current_index(i32::try_from(model.game_index).unwrap_or(0));
        // The keyboard.
        let (before, at, after) = model.keyboard.display();
        view.set_before(before);
        view.set_at(at);
        view.set_after(after);
        crate::view_model::publish(&view.get_keys(), model.keyboard.cells(), |rows| {
            view.set_keys(rows);
        });
        view.set_key_rows(i32::try_from(model.keyboard.row_count()).unwrap_or(4));
        view.set_key_index(i32::try_from(model.keyboard.index()).unwrap_or(0));
        view.set_key_kind(model.keyboard.key_kind());
        view.set_pressed_key(model.flash.index());
        view.set_can_search(!search_rules::query_text(model.keyboard.text()).is_empty());
        // The sources.
        let cells: Vec<GridCell> = model.sources.iter().map(|s| source_cell(app, s)).collect();
        let count = cells.len();
        let current = model.source_index.min(count.saturating_sub(1));
        let sources_view = app.global::<PickerSourcesView>();
        model.sources_top = crate::browse_motion::publish_list(
            &sources_view,
            &shared.input,
            current,
            count,
            visible,
            model.sources_top,
            &cells,
            Clone::clone,
        );
    }
    publish_games(ctx, app);
}

/// The scene changed size: refit and repaint, and fill the pane a switch
/// to two panes now shows.
pub(crate) fn relayout(ctx: &Ctx, app: &App) {
    if !is_active(app) {
        return;
    }
    let two = two_panes(ctx, app);
    if two && lock(&ctx.shared).game_picker.pane == Pane::Sources {
        show_source(ctx, app);
    }
    render(ctx, app);
}

// ---------- Input ----------

fn move_source(ctx: &Ctx, app: &App, delta: i64) {
    let moved = {
        let mut shared = lock(&ctx.shared);
        let model = &mut shared.game_picker;
        let next = rules::step(&model.sources, model.source_index, delta);
        let moved = next != model.source_index;
        model.source_index = next;
        moved
    };
    if !moved {
        return;
    }
    if two_panes(ctx, app) {
        show_source(ctx, app);
    }
    render(ctx, app);
}

fn move_game(ctx: &Ctx, app: &App, delta: i64) {
    let held = list_rules::is_page_move(delta) && crate::input::dispatching_repeat(ctx);
    let moved = {
        let mut shared = lock(&ctx.shared);
        let model = &mut shared.game_picker;
        let count = model.rows.len();
        match list_rules::linear_move(model.game_index, count, delta, false, held) {
            list_rules::LinearMove::To { index, .. } => {
                model.game_index = index;
                true
            }
            list_rules::LinearMove::WrapToTail => {
                model.game_index = count.saturating_sub(1);
                true
            }
            list_rules::LinearMove::FetchMore | list_rules::LinearMove::Stay { .. } => false,
        }
    };
    if moved {
        render(ctx, app);
    }
}

/// Into the focused source: its games, or Search's keyboard.
fn enter_source(ctx: &Ctx, app: &App) {
    {
        let mut shared = lock(&ctx.shared);
        let model = &mut shared.game_picker;
        let Some(source) = model.source().cloned() else {
            return;
        };
        model.pane = rules::enter(&source, model.searched.is_some());
    }
    show_source(ctx, app);
    render(ctx, app);
}

fn pick_focused(ctx: &Ctx, app: &App) {
    let game = {
        let shared = lock(&ctx.shared);
        let model = &shared.game_picker;
        if model.state != ContentState::Ready {
            return;
        }
        model
            .rows
            .get(model.game_index)
            .and_then(PickedGame::from_row)
    };
    if let Some(game) = game {
        finish(ctx, app, Some(game));
    }
}

fn back(ctx: &Ctx, app: &App) {
    let step = {
        let shared = lock(&ctx.shared);
        let model = &shared.game_picker;
        let source = model.source().cloned().unwrap_or(Source::Recents);
        rules::back(model.pane, &source)
    };
    match step {
        Back::Leave => finish(ctx, app, None),
        Back::To(pane) => {
            lock(&ctx.shared).game_picker.pane = pane;
            render(ctx, app);
        }
    }
}

fn apply_press(ctx: &Ctx, app: &App, press: Press) {
    match press {
        Press::Submit => run_search(ctx, app),
        Press::Edited | Press::Moved | Press::Layer | Press::None => render(ctx, app),
    }
}

fn flash_key(ctx: &Ctx, app: &App) {
    let index = lock(&ctx.shared).game_picker.keyboard.index();
    crate::input::flash_key(
        ctx,
        app,
        index,
        |shared| &mut shared.game_picker.flash,
        |app| app.global::<GamePickerView>().set_pressed_key(-1),
    );
}

/// A character from a physical keyboard, while the keyboard pane has the
/// cursor.
pub(crate) fn type_char(ctx: &Ctx, app: &App, c: char) {
    let press = {
        let mut shared = lock(&ctx.shared);
        let keyboard = &mut shared.game_picker.keyboard;
        let press = keyboard.insert(c);
        keyboard.focus(Key::Submit);
        press
    };
    apply_press(ctx, app, press);
}

/// Whether Accept on the focused key repeats while held.
pub(crate) fn key_repeats(ctx: &Ctx) -> bool {
    lock(&ctx.shared).game_picker.keyboard.key_repeats()
}

fn keyboard_action(ctx: &Ctx, app: &App, action: &str) {
    let two = two_panes(ctx, app);
    match action {
        actions::UP | actions::DOWN | actions::LEFT | actions::RIGHT => {
            let to_sources = {
                let mut shared = lock(&ctx.shared);
                let keyboard = &mut shared.game_picker.keyboard;
                match keyboard.step(action, !two) {
                    Some(Edge::Left) if two => true,
                    Some(Edge::Up) => {
                        keyboard.enter_row(true);
                        false
                    }
                    Some(Edge::Down) => {
                        keyboard.enter_row(false);
                        false
                    }
                    Some(Edge::Left | Edge::Right) | None => false,
                }
            };
            if to_sources {
                lock(&ctx.shared).game_picker.pane = Pane::Sources;
            }
            render(ctx, app);
        }
        actions::ACCEPT => {
            let press = lock(&ctx.shared).game_picker.keyboard.press();
            flash_key(ctx, app);
            apply_press(ctx, app, press);
        }
        actions::CANCEL => back(ctx, app),
        _ => {
            if action == zaparoo_app::input::TEXT_KEY {
                flash_key(ctx, app);
            }
            let held = crate::input::repeat_held_ms(ctx);
            let press = lock(&ctx.shared)
                .game_picker
                .keyboard
                .text_action(action, held);
            if let Some(press) = press {
                apply_press(ctx, app, press);
            }
        }
    }
}

pub(crate) fn handle_action(ctx: &Ctx, app: &App, action: &str) {
    let pane = {
        let shared = lock(&ctx.shared);
        if shared.game_picker.purpose.is_none() {
            drop(shared);
            if action == actions::CANCEL {
                finish(ctx, app, None);
            }
            return;
        }
        shared.game_picker.pane
    };
    if pane == Pane::Keyboard {
        keyboard_action(ctx, app, action);
        return;
    }
    let two = two_panes(ctx, app);
    let page = i64::try_from(visible(ctx, app)).unwrap_or(10);
    let dir: i64 = if matches!(action, actions::UP | actions::LEFT | actions::PAGE_PREV) {
        -1
    } else {
        1
    };
    let step = if crate::input::rapid_page(ctx) {
        page
    } else {
        1
    };
    match (pane, action) {
        (Pane::Sources, actions::UP | actions::DOWN) => move_source(ctx, app, dir * step),
        (Pane::Sources, actions::PAGE_PREV | actions::PAGE_NEXT) => {
            move_source(ctx, app, dir * page);
        }
        (Pane::Sources, actions::RIGHT) if two => enter_source(ctx, app),
        (Pane::Sources, actions::ACCEPT) => enter_source(ctx, app),
        (Pane::Games, actions::UP | actions::DOWN) => move_game(ctx, app, dir * step),
        (Pane::Games, actions::LEFT) if two => {
            lock(&ctx.shared).game_picker.pane = Pane::Sources;
            render(ctx, app);
        }
        (Pane::Games, actions::PAGE_PREV | actions::PAGE_NEXT | actions::LEFT | actions::RIGHT) => {
            move_game(ctx, app, dir * page);
        }
        (Pane::Games, actions::ACCEPT) => pick_focused(ctx, app),
        (_, actions::CANCEL) => back(ctx, app),
        _ => {}
    }
}

// ---------- Pointer ----------

fn pointer_allowed(ctx: &Ctx, app: &App) -> bool {
    let shell = app.global::<crate::Shell>();
    is_active(app)
        && !shell.get_transitioning()
        && !crate::input::modal_open(app)
        && lock(&ctx.shared).persist.settings.mouse_enabled
}

/// A row index from the view, in the list's own order.
fn row_of(view_top: i32, index: i32) -> Option<usize> {
    usize::try_from(view_top + index).ok()
}

/// Wire the view's pointer callbacks.
#[allow(
    clippy::too_many_lines,
    reason = "one closure per view callback, each short"
)]
pub fn bind(ctx: &Ctx, app: &App) {
    let input = app.global::<GamePickerInput>();
    let on = |f: fn(&Ctx, &App, i32)| {
        let ctx = ctx.clone();
        let weak = app.as_weak();
        move |index: i32| {
            if let Some(app) = weak.upgrade() {
                if pointer_allowed(&ctx, &app) {
                    f(&ctx, &app, index);
                }
            }
        }
    };
    input.on_source_hovered(on(|ctx, app, index| {
        let top = app.global::<PickerSourcesView>().get_list_view_top();
        let Some(row) = row_of(top, index) else {
            return;
        };
        let changed = {
            let mut shared = lock(&ctx.shared);
            let model = &mut shared.game_picker;
            let ok = model.sources.get(row).is_some_and(Source::selectable);
            let changed = ok && (model.source_index != row || model.pane != Pane::Sources);
            if changed {
                model.source_index = row;
                model.pane = Pane::Sources;
            }
            changed
        };
        if changed {
            if two_panes(ctx, app) {
                show_source(ctx, app);
            }
            render(ctx, app);
        }
    }));
    input.on_source_clicked(on(|ctx, app, index| {
        let top = app.global::<PickerSourcesView>().get_list_view_top();
        let Some(row) = row_of(top, index) else {
            return;
        };
        let ok = {
            let mut shared = lock(&ctx.shared);
            let model = &mut shared.game_picker;
            let ok = model.sources.get(row).is_some_and(Source::selectable);
            if ok {
                model.source_index = row;
                model.pane = Pane::Sources;
            }
            ok
        };
        if ok {
            enter_source(ctx, app);
        }
    }));
    input.on_game_hovered(on(|ctx, app, index| {
        let top = app.global::<PickerGamesView>().get_list_view_top();
        let Some(row) = row_of(top, index) else {
            return;
        };
        let changed = {
            let mut shared = lock(&ctx.shared);
            let model = &mut shared.game_picker;
            let changed =
                row < model.rows.len() && (model.game_index != row || model.pane != Pane::Games);
            if changed {
                model.game_index = row;
                model.pane = Pane::Games;
            }
            changed
        };
        if changed {
            render(ctx, app);
        }
    }));
    input.on_game_clicked(on(|ctx, app, index| {
        let top = app.global::<PickerGamesView>().get_list_view_top();
        let Some(row) = row_of(top, index) else {
            return;
        };
        let ok = {
            let mut shared = lock(&ctx.shared);
            let model = &mut shared.game_picker;
            let ok = row < model.rows.len();
            if ok {
                model.game_index = row;
                model.pane = Pane::Games;
            }
            ok
        };
        if ok {
            pick_focused(ctx, app);
        }
    }));
    input.on_key_hovered(on(|ctx, app, index| {
        let Ok(index) = usize::try_from(index) else {
            return;
        };
        let changed = {
            let mut shared = lock(&ctx.shared);
            let model = &mut shared.game_picker;
            model.pane = Pane::Keyboard;
            model.keyboard.set_index(index)
        };
        if changed {
            render(ctx, app);
        }
    }));
    input.on_key_clicked(on(|ctx, app, index| {
        let Ok(index) = usize::try_from(index) else {
            return;
        };
        {
            let mut shared = lock(&ctx.shared);
            let model = &mut shared.game_picker;
            model.pane = Pane::Keyboard;
            model.keyboard.set_index(index);
        }
        keyboard_action(ctx, app, actions::ACCEPT);
    }));
    input.on_wheel(on(|ctx, app, delta| {
        let pane = lock(&ctx.shared).game_picker.pane;
        let delta = i64::from(delta.signum());
        match pane {
            Pane::Sources => move_source(ctx, app, delta),
            Pane::Games => move_game(ctx, app, delta),
            Pane::Keyboard => {}
        }
    }));
}

#[cfg(test)]
#[allow(clippy::expect_used, reason = "tests should fail fast")]
mod tests {
    use super::*;

    #[test]
    fn a_folder_or_a_deck_is_not_a_game_to_hand_back() {
        use zaparoo_core::media_types::BrowseEntry;
        let game = BrowseEntry {
            name: "Super Metroid".into(),
            path: "SNES/Super Metroid.sfc".into(),
            entry_type: "media".into(),
            system_id: "SNES".into(),
            ..BrowseEntry::default()
        };
        let mut row = GameRow::from(&game);
        row.display = row.name.clone();
        let picked = PickedGame::from_row(&row).expect("a game");
        assert_eq!(picked.name, "Super Metroid");
        assert_eq!(picked.system, "SNES");
        assert!(!picked.zapscript.is_empty());

        let folder = BrowseEntry {
            entry_type: "directory".into(),
            ..game
        };
        assert_eq!(PickedGame::from_row(&GameRow::from(&folder)), None);
    }
}
