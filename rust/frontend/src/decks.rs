// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// Decks: the Decks screen (the Systems grid in its `decks` mode), the Deck
// screen (the games list in its `deck` mode), a deck's menus on both and
// on its Hub tile, and what they change in Core.
//
// The deck list is watched for the whole session: pinned deck tiles take
// their names and covers from it, and the Decks screen opens on it. Only
// the user's own decks and the ones their account saved are listed; copies
// Core keeps of cards someone tapped are its cache, not a collection.

use slint::{ComponentHandle, SharedString};
use zaparoo_app::options_menu as menu;
use zaparoo_app::systems::SystemRow;
use zaparoo_core::collections::{
    Deck, DeckCover, DeckItemInput, DeckItemKind, DeckUpdateParams, DecksChanged, DecksParams,
};
use zaparoo_core::endpoints::decks::{
    deck_tags, DeckDeleteMutation, DeckUpdateMutation, DecksEndpoint,
};
use zaparoo_core::input_actions::actions;
use zaparoo_core::remote_resource::ResourceStatus;

use crate::games::{DeckItemRow, GameRow};
use crate::media_cache::MediaKey;
use crate::navigation::EntryMode;
use crate::router::{lock, menu_row, Ctx, ListContext, Shared};
use crate::{App, GridCell, SystemsMode, SystemsView};

/// A deck row's `cover_key` on the Decks screen: it has no system logo.
pub(crate) const ROW_COVER_KEY: &str = "deck";

#[derive(Debug, Default)]
pub struct Model {
    /// The decks the Decks screen lists, as Core last answered; `None`
    /// until it has.
    pub list: Option<Vec<Deck>>,
    watch: crate::scoped_task::ScopedTask,
    /// A Delete deck waiting on its dialog: the deck, its name and the
    /// screen it was asked on.
    pending_delete: Option<(String, String, crate::Screen)>,
}

fn find<'a>(shared: &'a Shared, deck_id: &str) -> Option<&'a Deck> {
    pick_deck(
        shared.games.deck.as_ref(),
        shared.decks.list.as_deref(),
        deck_id,
    )
}

/// The deck, from the open deck when it is that one (it has the newest
/// items) or the session's deck list. Once the list has loaded it decides
/// whether the deck exists: a deck deleted after it was opened stays the
/// open deck until another one is opened.
fn pick_deck<'a>(
    open: Option<&'a Deck>,
    list: Option<&'a [Deck]>,
    deck_id: &str,
) -> Option<&'a Deck> {
    let same = |deck: &&Deck| deck.deck_id.eq_ignore_ascii_case(deck_id);
    let listed = list.map(|list| list.iter().find(same));
    if let Some(None) = listed {
        return None;
    }
    open.filter(same).or(listed.flatten())
}

/// The deck's name, for menus and errors.
fn name_of(shared: &Shared, deck_id: &str) -> String {
    find(shared, deck_id)
        .map(|deck| deck.name.clone())
        .unwrap_or_default()
}

// ---------- The deck list ----------

/// Watch Core's deck list for the session. Every answer re-resolves the
/// Hub's deck tiles and the Decks screen when it is on show.
pub(crate) fn watch(ctx: &Ctx, app: &App) {
    let resource = ctx.store.subscribe::<DecksEndpoint>(DecksParams::default());
    let mut rx = resource.subscribe();
    let weak = app.as_weak();
    let ctx2 = ctx.clone();
    let task = ctx.handle.spawn(async move {
        // The handle keeps the list live, so every change refetches it.
        let _resource = resource;
        loop {
            let snapshot = rx.borrow_and_update().clone();
            if let ResourceStatus::Ready(result) = snapshot {
                let listed: Vec<Deck> = result.decks.into_iter().filter(Deck::listed).collect();
                let ctx3 = ctx2.clone();
                let _ = weak.upgrade_in_event_loop(move |app| list_landed(&ctx3, &app, listed));
            }
            if rx.changed().await.is_err() {
                return;
            }
        }
    });
    lock(&ctx.shared).decks.watch.replace(task);
}

fn list_landed(ctx: &Ctx, app: &App, listed: Vec<Deck>) {
    let showing = {
        let mut shared = lock(&ctx.shared);
        if shared.decks.list.as_ref() == Some(&listed) {
            return;
        }
        shared.decks.list = Some(listed);
        shared.systems_model.mode == SystemsMode::Decks
    };
    crate::hub::rebuild(ctx, app);
    if !showing {
        return;
    }
    let waiting = lock(&ctx.shared).systems_model.loading;
    if waiting {
        land_screen(ctx, app);
    } else if app.global::<crate::Shell>().get_active_screen() == crate::Screen::Decks {
        crate::systems::reproject(ctx, app);
    }
}

/// Ask Core for the deck list again.
pub(crate) fn refresh(ctx: &Ctx, _app: &App) {
    for tag in deck_tags(None) {
        ctx.store.invalidate(&tag);
    }
}

/// Core's `decks.changed`: the list and that deck are stale. A Deck screen
/// on that deck refills, or leaves when the deck is gone.
pub(crate) fn changed(ctx: &Ctx, app: &App, changed: &DecksChanged) {
    let id = (!changed.deck_id.is_empty()).then_some(changed.deck_id.as_str());
    for tag in deck_tags(id) {
        ctx.store.invalidate(&tag);
    }
    let open = {
        let shared = lock(&ctx.shared);
        shared.games.mode == crate::GamesMode::Deck
            && shared
                .persist
                .deck
                .deck_id
                .eq_ignore_ascii_case(&changed.deck_id)
    };
    let shell = app.global::<crate::Shell>();
    if !open || shell.get_active_screen() != crate::Screen::Deck || shell.get_transitioning() {
        return;
    }
    // An item held by Move keeps its place until it is dropped.
    if moving(ctx) {
        return;
    }
    if changed.action == "deleted" {
        leave_deck(ctx, app);
    } else {
        crate::games::refresh_deck(ctx, app);
    }
}

// ---------- The Decks screen ----------

/// The Decks screen as Systems rows.
pub(crate) fn system_rows(shared: &Shared) -> Vec<SystemRow> {
    shared
        .decks
        .list
        .iter()
        .flatten()
        .map(|deck| SystemRow {
            id: deck.deck_id.clone(),
            name: deck.name.clone(),
            cover_key: ROW_COVER_KEY.to_string(),
            media_count: Some(deck.item_count),
            ..SystemRow::default()
        })
        .collect()
}

/// Who a saved deck is from, and whether the focused deck is read-only.
pub(crate) fn note(shared: &Shared, deck_id: &str) -> (String, bool) {
    if shared.systems_model.mode != SystemsMode::Decks {
        return (String::new(), false);
    }
    find(shared, deck_id).map_or((String::new(), false), |deck| {
        (
            if deck.saved {
                deck.owned_by.clone()
            } else {
                String::new()
            },
            !deck.editable(),
        )
    })
}

/// A deck's cover key at `size`, the Hub's and the Decks grid's alike.
fn cover_key(deck: &Deck, size: u32, fit: zaparoo_app::covers::Fit) -> Option<MediaKey> {
    deck.has_image
        .then(|| MediaKey::deck_art(&deck.deck_id, None, deck.updated_at, size, fit))
}

/// One Decks tile: its cover, or its name as a wordmark.
pub(crate) fn cell(ctx: &Ctx, shared: &Shared, deck_id: &str, art_size: u32) -> GridCell {
    let Some(deck) = find(shared, deck_id) else {
        return GridCell::default();
    };
    let mut cell = GridCell {
        name: SharedString::from(deck.name.as_str()),
        wordmark: true,
        locked: !deck.editable(),
        ..Default::default()
    };
    if let Some(key) = cover_key(deck, art_size, ctx.media.hub_fit()) {
        if let Some(image) = ctx.media.get(&key) {
            cell.cover = slint::Image::from_rgba8(image.buffer);
            cell.has_cover = true;
            cell.fetched = true;
        } else if !ctx.media.is_negative(&key) {
            cell.fetched = true;
            ctx.media.enqueue(key);
        }
    }
    cell
}

/// A deck cover the Decks screen shows has landed.
pub(crate) fn covers_landed(ctx: &Ctx, app: &App, keys: &[MediaKey]) {
    if !keys.iter().any(MediaKey::is_deck_art) {
        return;
    }
    let shell = app.global::<crate::Shell>();
    if shell.get_active_screen() == crate::Screen::Decks && !shell.get_transitioning() {
        crate::systems::render(ctx, app);
    }
}

/// The Hub's Decks tile.
pub(crate) fn enter(ctx: &Ctx, app: &App, entry: EntryMode) {
    enter_with(ctx, app, entry, 1);
}

/// Back onto the Decks screen, on the deck it was left on.
pub(crate) fn return_to_decks(ctx: &Ctx, app: &App) {
    enter_with(ctx, app, EntryMode::Restore, -1);
}

fn enter_with(ctx: &Ctx, app: &App, entry: EntryMode, direction: i32) {
    crate::navigation::stage(ctx, app);
    let waiting = {
        let mut shared = lock(&ctx.shared);
        if entry == EntryMode::Fresh {
            shared.persist.decks = zaparoo_core::persist::DecksState::default();
        }
        shared.persist.active_screen = crate::Screen::Decks.token().to_string();
        let waiting = shared.decks.list.is_none();
        let model = &mut shared.systems_model;
        model.fill_task.cancel();
        model.mode = SystemsMode::Decks;
        model.loading = waiting;
        model.focus_armed = false;
        model.restore_done = false;
        model.page.sliding = false;
        model.transition_seq += 1;
        model.grid.prepare_for_model_replacement();
        seat(&mut shared);
        waiting
    };
    crate::router::save_persist(&ctx.shared);
    refresh(ctx, app);
    if waiting {
        PENDING_DIRECTION.with(|cell| cell.set(direction));
        crate::router::begin_pending_with_direction(app, crate::Screen::Decks, direction);
        wait_for_list(ctx, app);
        return;
    }
    show_screen(ctx, app, direction);
}

thread_local! {
    /// The direction a Decks route waiting on the list arrives from.
    static PENDING_DIRECTION: std::cell::Cell<i32> = const { std::cell::Cell::new(1) };
}

/// A first visit before Core answered: wait for the answer, or its error,
/// under the pending route. The session watch delivers a list; this only
/// catches the error the watch keeps quiet about.
fn wait_for_list(ctx: &Ctx, app: &App) {
    let ticket = lock(&ctx.shared).systems_model.transition_seq;
    let resource = ctx.store.subscribe::<DecksEndpoint>(DecksParams::default());
    let mut rx = resource.subscribe();
    let weak = app.as_weak();
    let ctx2 = ctx.clone();
    let task = ctx.handle.spawn(async move {
        loop {
            let snapshot = rx.borrow_and_update().clone();
            match snapshot {
                ResourceStatus::Ready(result) => {
                    let listed: Vec<Deck> = result.decks.into_iter().filter(Deck::listed).collect();
                    let _ = weak.upgrade_in_event_loop(move |app| {
                        if lock(&ctx2.shared).systems_model.transition_seq != ticket {
                            return;
                        }
                        lock(&ctx2.shared).decks.list = Some(listed);
                        crate::hub::rebuild(&ctx2, &app);
                        land_screen(&ctx2, &app);
                    });
                    return;
                }
                ResourceStatus::Errored { message, .. } => {
                    let _ = weak.upgrade_in_event_loop(move |app| {
                        if lock(&ctx2.shared).systems_model.transition_seq != ticket {
                            return;
                        }
                        if crate::navigation::fail(&ctx2, &app, &message) {
                            return;
                        }
                        lock(&ctx2.shared).systems_model.loading = false;
                        let view = app.global::<SystemsView>();
                        view.set_loading(false);
                        view.set_error(SharedString::from(message.as_str()));
                        crate::systems::render(&ctx2, &app);
                        let direction = PENDING_DIRECTION.with(std::cell::Cell::get);
                        crate::router::transition_to_screen(&app, crate::Screen::Decks, direction);
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

/// The list arrived for a Decks route waiting on it.
fn land_screen(ctx: &Ctx, app: &App) {
    {
        let mut shared = lock(&ctx.shared);
        if !shared.systems_model.loading || shared.systems_model.mode != SystemsMode::Decks {
            return;
        }
        shared.systems_model.loading = false;
        shared.systems_model.fill_task.cancel();
        seat(&mut shared);
    }
    show_screen(ctx, app, PENDING_DIRECTION.with(std::cell::Cell::get));
}

fn show_screen(ctx: &Ctx, app: &App, direction: i32) {
    crate::navigation::finish(app);
    let view = app.global::<SystemsView>();
    view.set_error(SharedString::default());
    view.set_loading(false);
    crate::systems::render(ctx, app);
    crate::router::save_persist(&ctx.shared);
    crate::router::transition_to_screen(app, crate::Screen::Decks, direction);
}

/// Seat the rows and the saved deck on the Decks screen.
fn seat(shared: &mut Shared) {
    let rows = system_rows(shared);
    let saved = shared.persist.decks.selected_deck.clone();
    let index = rows.iter().position(|row| row.id == saved).unwrap_or(0);
    let model = &mut shared.systems_model;
    model.rows = rows;
    model.grid.set_item_count(model.rows.len());
    model.grid.set_current_index_immediate(index);
    model.restore_done = true;
}

/// The Decks screen's Menu list (where the chip row is not on screen).
pub(crate) fn open_list_menu(ctx: &Ctx, app: &App) {
    crate::router::present_list(
        ctx,
        app,
        ListContext::DecksPageMenu,
        "title:menu",
        vec![menu_row("new_deck")],
    );
}

/// New deck… on the Decks screen: an empty deck, named on the keyboard.
pub(crate) fn new_deck(ctx: &Ctx, app: &App) {
    crate::rename::open_empty_deck(ctx, app, crate::Screen::Decks);
}

// ---------- The Deck screen ----------

/// Open a deck, from the Decks screen or its Hub tile.
pub(crate) fn open_deck(ctx: &Ctx, app: &App, deck_id: &str, from_hub: bool) {
    {
        let mut shared = lock(&ctx.shared);
        let deck = &mut shared.persist.deck;
        deck.deck_id = deck_id.to_string();
        deck.entered_from_hub = from_hub;
        shared.games.deck = None;
    }
    crate::games::enter_deck(ctx, app, EntryMode::Fresh);
}

/// The deck's items as list rows, in the deck's order.
pub(crate) fn rows(deck: &Deck) -> Vec<GameRow> {
    deck.items
        .iter()
        .map(|item| {
            let media = item.media.as_ref();
            let game = media.is_some_and(|media| media.available);
            let name = media
                .map(|media| media.name.as_str())
                .filter(|name| !name.trim().is_empty())
                .unwrap_or(item.name.as_str());
            let mut row = GameRow::from_script(name, &item.zapscript);
            if let Some(media) = media {
                row.system_id.clone_from(&media.system);
                row.path.clone_from(&media.path);
                // Core's flags for the file make the item a full game row:
                // the strip, Add to deck and Hide follow from them.
                if let Some(tags) = media.user_tags.as_deref().filter(|_| game) {
                    row.flags = crate::games::user_flags(tags);
                    row.is_hidden = tags
                        .iter()
                        .any(|tag| tag.tag_type == "user" && tag.tag == "hidden");
                    row.tags_known = true;
                }
            }
            row.media_capable = game;
            row.has_cover = game || (item.kind == DeckItemKind::Card && item.has_image);
            row.display = name.to_string();
            row.deck_item = Some(DeckItemRow {
                id: item.id,
                position: item.position,
                kind: item.kind,
                game,
                card_art: item.kind == DeckItemKind::Card && item.has_image,
                deck_id: deck.deck_id.clone(),
                revision: deck.updated_at,
            });
            row
        })
        .collect()
}

/// The deck's name in the title, once its fill landed.
pub(crate) fn publish_open(ctx: &Ctx, app: &App) {
    let name = lock(&ctx.shared)
        .games
        .deck
        .as_ref()
        .map(|deck| deck.name.clone())
        .unwrap_or_default();
    app.global::<crate::GamesView>()
        .set_title(SharedString::from(name.as_str()));
}

/// Back from the Deck screen when the deck is gone: where Back would go.
fn leave_deck(ctx: &Ctx, app: &App) {
    let from_hub = {
        let mut shared = lock(&ctx.shared);
        shared.games.deck = None;
        std::mem::take(&mut shared.persist.deck.entered_from_hub)
    };
    if from_hub {
        lock(&ctx.shared).persist.active_screen = crate::Screen::Hub.token().to_string();
        crate::router::save_persist(&ctx.shared);
        crate::router::return_to_hub(ctx, app);
    } else {
        return_to_decks(ctx, app);
    }
}

/// Play a deck as a playlist from its first item.
fn play(ctx: &Ctx, app: &App, deck_id: &str, name: &str) {
    crate::router::launch(ctx, app, format!("**playlist.play:deck://{deck_id}"), name);
}

/// Play the deck on the Deck screen from the item at `position`.
pub(crate) fn play_from(ctx: &Ctx, app: &App, position: u32, label: &str) {
    let deck_id = lock(&ctx.shared).persist.deck.deck_id.clone();
    if deck_id.is_empty() {
        return;
    }
    crate::router::launch(
        ctx,
        app,
        format!("**playlist.play:deck://{deck_id}?start={position}"),
        label,
    );
}

/// The Play deck chip on the Deck screen.
pub(crate) fn play_open(ctx: &Ctx, app: &App) {
    let (deck_id, name) = {
        let shared = lock(&ctx.shared);
        let id = shared.persist.deck.deck_id.clone();
        let name = name_of(&shared, &id);
        (id, name)
    };
    if !deck_id.is_empty() {
        play(ctx, app, &deck_id, &name);
    }
}

/// The deck's rows for its Menu: the page actions, by what the deck takes.
fn page_rows(shared: &Shared, deck_id: &str, with_play: bool) -> Vec<crate::MenuEntry> {
    let input = input(shared, deck_id, false);
    let mut rows = Vec::new();
    if with_play && input.has_items {
        rows.push(menu_row("play_deck"));
    }
    rows.push(crate::router::menu_row_keyed(
        "deck_hub",
        if input.on_hub {
            "hub:remove"
        } else {
            "add_to_hub"
        },
        "",
    ));
    if input.editable {
        rows.push(menu_row("deck_rename"));
        if input.can_choose_cover {
            rows.push(menu_row("deck_cover_choose"));
        }
        if input.has_cover {
            rows.push(menu_row("deck_cover_remove"));
        }
        rows.push(menu_row("deck_delete"));
    }
    rows
}

/// The Deck screen's Menu list (where the chip row is not on screen).
pub(crate) fn open_page_menu(ctx: &Ctx, app: &App) {
    let rows = {
        let shared = lock(&ctx.shared);
        page_rows(&shared, &shared.persist.deck.deck_id, true)
    };
    crate::router::present_list(ctx, app, ListContext::DeckPageMenu, "title:menu", rows);
}

/// The Deck screen's More chip: its Menu without Play deck, which has a
/// chip of its own.
pub(crate) fn open_more(ctx: &Ctx, app: &App) {
    let rows = {
        let shared = lock(&ctx.shared);
        page_rows(&shared, &shared.persist.deck.deck_id, false)
    };
    crate::router::present_list(ctx, app, ListContext::DeckPageMenu, "title:more", rows);
}

/// A row of the Decks or Deck screen's Menu list.
pub(crate) fn page_menu_accept(ctx: &Ctx, app: &App, id: &str) {
    if id == "new_deck" {
        new_deck(ctx, app);
        return;
    }
    let deck_id = lock(&ctx.shared).persist.deck.deck_id.clone();
    if id == "play_deck" {
        play_open(ctx, app);
        return;
    }
    deck_action(ctx, app, id, &deck_id, crate::Screen::Deck);
}

// ---------- A deck's menu ----------

fn input(shared: &Shared, deck_id: &str, tile: bool) -> menu::DeckInput {
    let on_hub = shared
        .hub
        .layout
        .target_position("collection", deck_id, "", "", "", "")
        .is_some();
    find(shared, deck_id).map_or(
        menu::DeckInput {
            on_hub,
            tile,
            ..menu::DeckInput::default()
        },
        |deck| menu::DeckInput {
            editable: deck.editable(),
            has_items: deck.item_count > 0,
            has_cover: deck.has_image && deck.editable(),
            can_choose_cover: crate::media_cache::should_request_local_path(256),
            on_hub,
            tile,
        },
    )
}

/// A deck's Options menu: on the Decks screen, or on its Hub tile.
pub(crate) fn menu(shared: &Shared, deck_id: &str, tile: bool) -> menu::Menu {
    menu::deck(&input(shared, deck_id, tile))
}

/// A deck menu row, from the Decks screen or a deck's Hub tile.
pub(crate) fn context_accept(ctx: &Ctx, app: &App, id: &str, deck_id: &str, source: crate::Screen) {
    if id == "play_deck" {
        let name = name_of(&lock(&ctx.shared), deck_id);
        play(ctx, app, deck_id, &name);
        return;
    }
    deck_action(ctx, app, id, deck_id, source);
}

/// What a deck's rows do, wherever they were picked.
fn deck_action(ctx: &Ctx, app: &App, id: &str, deck_id: &str, source: crate::Screen) {
    let name = name_of(&lock(&ctx.shared), deck_id);
    match id {
        "deck_hub" => {
            crate::hub::toggle_target(ctx, app, "collection", deck_id, "", "", "", "", "", "");
            if source == crate::Screen::Decks {
                crate::systems::render(ctx, app);
            }
        }
        "deck_rename" => crate::rename::open_deck_rename(ctx, app, deck_id, &name, source),
        "deck_cover_choose" => crate::file_browser::open_deck_cover(ctx, app, deck_id, source),
        "deck_cover_remove" => set_cover(ctx, app, deck_id, DeckCover::None),
        "deck_delete" => {
            lock(&ctx.shared).decks.pending_delete =
                Some((deck_id.to_string(), name.clone(), source));
            crate::router::open_dialog(
                app,
                crate::DialogKind::DeleteDeck,
                &name,
                "",
                &[crate::DialogButton::No, crate::DialogButton::Yes],
                0,
            );
        }
        _ => {}
    }
}

/// The Delete deck dialog closed.
pub(crate) fn delete_answered(ctx: &Ctx, app: &App, confirmed: bool) {
    let Some((deck_id, name, source)) = lock(&ctx.shared).decks.pending_delete.take() else {
        return;
    };
    if !confirmed {
        return;
    }
    run(ctx, app, &name, move |store| async move {
        store
            .run_mutation::<DeckDeleteMutation>(deck_id)
            .await
            .map(|()| None)
    });
    // The deck is gone from where it was asked on.
    if source == crate::Screen::Deck {
        leave_deck(ctx, app);
    }
}

/// Rename a deck (the rename screen's Save).
pub(crate) fn rename(ctx: &Ctx, app: &App, deck_id: &str, name: &str) {
    let params = DeckUpdateParams {
        deck_id: deck_id.to_string(),
        name: Some(name.to_string()),
        ..DeckUpdateParams::default()
    };
    update(ctx, app, name, params);
}

/// Set or remove a deck's cover.
pub(crate) fn set_cover(ctx: &Ctx, app: &App, deck_id: &str, cover: DeckCover) {
    let name = name_of(&lock(&ctx.shared), deck_id);
    let params = DeckUpdateParams {
        deck_id: deck_id.to_string(),
        cover: Some(cover),
        ..DeckUpdateParams::default()
    };
    update(ctx, app, &name, params);
}

fn update(ctx: &Ctx, app: &App, name: &str, params: DeckUpdateParams) {
    run(ctx, app, name, move |store| async move {
        store
            .run_mutation::<DeckUpdateMutation>(params)
            .await
            .map(Some)
    });
}

/// Send a deck change to Core under the Saving cue. The screens follow
/// Core's answer through `decks.changed`; a failure is an alert naming the
/// deck. A changed deck that is open on the Deck screen refills from the
/// answer at once.
fn run<F, Fut>(ctx: &Ctx, app: &App, name: &str, change: F)
where
    F: FnOnce(std::sync::Arc<zaparoo_core::store::Store>) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = Result<Option<Deck>, zaparoo_core::client::ClientError>>
        + Send
        + 'static,
{
    let store = ctx.store.clone();
    let ctx2 = ctx.clone();
    let weak = app.as_weak();
    let name = name.to_string();
    let wait = crate::cue::begin(ctx, app, crate::AppCue::Saving, "", "");
    ctx.handle.spawn(async move {
        let result = change(store).await;
        let _ = weak.upgrade_in_event_loop(move |app| {
            crate::cue::end(&ctx2, &app, wait);
            match result {
                Ok(Some(deck)) => deck_answered(&ctx2, &app, &deck),
                Ok(None) => {}
                Err(e) => {
                    tracing::warn!("deck change failed for {name}: {}", e.message);
                    crate::router::report_action_error(&ctx2, &app, "deck_update", &name);
                    // Whatever was shown ahead of Core's answer goes back.
                    let open = lock(&ctx2.shared).games.mode == crate::GamesMode::Deck;
                    if open
                        && app.global::<crate::Shell>().get_active_screen() == crate::Screen::Deck
                    {
                        crate::games::refresh_deck(&ctx2, &app);
                    }
                }
            }
        });
    });
}

/// Core's answer to a change of the deck open on the Deck screen.
fn deck_answered(ctx: &Ctx, app: &App, deck: &Deck) {
    let open = {
        let shared = lock(&ctx.shared);
        shared.games.mode == crate::GamesMode::Deck
            && shared
                .persist
                .deck
                .deck_id
                .eq_ignore_ascii_case(&deck.deck_id)
    };
    if open && app.global::<crate::Shell>().get_active_screen() == crate::Screen::Deck {
        crate::games::refresh_deck(ctx, app);
    }
}

// ---------- A deck item's rows ----------

/// Use as deck cover, Move and Remove from deck on an item of the deck.
pub(crate) fn item_action(ctx: &Ctx, app: &App, id: &str, row: &GameRow, system: &str) {
    let Some(item) = row.deck_item.clone() else {
        return;
    };
    let deck_id = lock(&ctx.shared).persist.deck.deck_id.clone();
    match id {
        "deck_use_cover" if item.game => set_cover(
            ctx,
            app,
            &deck_id,
            DeckCover::Media {
                system: system.to_string(),
                path: row.path.clone(),
            },
        ),
        "deck_move" => begin_move(ctx, app),
        "deck_remove" => remove_item(ctx, app, &deck_id, item.id),
        _ => {}
    }
}

/// Remove an item at once: the row leaves now, and comes back if Core
/// refuses.
fn remove_item(ctx: &Ctx, app: &App, deck_id: &str, item_id: i64) {
    let name = {
        let mut shared = lock(&ctx.shared);
        let name = name_of(&shared, deck_id);
        let model = &mut shared.games;
        if let Some(index) = model.rows.iter().position(|row| {
            row.deck_item
                .as_ref()
                .is_some_and(|item| item.id == item_id)
        }) {
            model.rows.remove(index);
            model.grid.set_item_count(model.rows.len());
            let current = model
                .grid
                .current_index()
                .min(model.rows.len().saturating_sub(1));
            model.grid.set_current_index_immediate(current);
        }
        name
    };
    crate::games::render(ctx, app);
    let params = DeckUpdateParams {
        deck_id: deck_id.to_string(),
        remove_item_ids: vec![item_id],
        ..DeckUpdateParams::default()
    };
    update(ctx, app, &name, params);
}

/// An item is picked up by Move.
pub(crate) fn moving(ctx: &Ctx) -> bool {
    lock(&ctx.shared).games.moving.is_some()
}

fn begin_move(ctx: &Ctx, app: &App) {
    {
        let mut shared = lock(&ctx.shared);
        let rows = shared.games.rows.clone();
        if rows.len() < 2 {
            return;
        }
        shared.games.moving = Some(rows);
    }
    app.global::<crate::GamesView>().set_moving(true);
    crate::games::render(ctx, app);
}

/// Input while an item is picked up: Up and Down carry it (Left and Right
/// too, on a grid), A puts it down and saves the order, Back puts it back.
pub(crate) fn move_action(ctx: &Ctx, app: &App, action: &str) {
    match action {
        actions::UP | actions::LEFT => step(ctx, app, -1),
        actions::DOWN | actions::RIGHT => step(ctx, app, 1),
        actions::ACCEPT => drop_item(ctx, app),
        actions::CANCEL => cancel_move(ctx, app),
        _ => {}
    }
}

fn step(ctx: &Ctx, app: &App, delta: i64) {
    {
        let mut shared = lock(&ctx.shared);
        let model = &mut shared.games;
        let from = model.grid.current_index();
        let Some(to) = from
            .checked_add_signed(isize::try_from(delta).unwrap_or(0))
            .filter(|to| *to < model.rows.len())
        else {
            return;
        };
        model.rows.swap(from, to);
        model.grid.set_current_index_immediate(to);
    }
    crate::games::render(ctx, app);
}

fn finish_move(ctx: &Ctx, app: &App) -> Option<Vec<GameRow>> {
    let original = lock(&ctx.shared).games.moving.take();
    app.global::<crate::GamesView>().set_moving(false);
    original
}

fn cancel_move(ctx: &Ctx, app: &App) {
    if let Some(original) = finish_move(ctx, app) {
        let mut shared = lock(&ctx.shared);
        let held = shared.games.current().and_then(|row| row.deck_item.clone());
        let index = held
            .and_then(|held| {
                original.iter().position(|row| {
                    row.deck_item
                        .as_ref()
                        .is_some_and(|item| item.id == held.id)
                })
            })
            .unwrap_or(0);
        shared.games.rows = original;
        shared.games.grid.set_current_index_immediate(index);
    }
    crate::games::render(ctx, app);
}

/// Put the item down: the new order goes to Core as the whole list of
/// item ids, unless nothing moved.
fn drop_item(ctx: &Ctx, app: &App) {
    let Some(original) = finish_move(ctx, app) else {
        return;
    };
    let (deck_id, name, order, changed) = {
        let shared = lock(&ctx.shared);
        let ids = |rows: &[GameRow]| -> Vec<i64> {
            rows.iter()
                .filter_map(|row| row.deck_item.as_ref().map(|item| item.id))
                .collect()
        };
        let order = ids(&shared.games.rows);
        let id = shared.persist.deck.deck_id.clone();
        let name = name_of(&shared, &id);
        let changed = order != ids(&original);
        (id, name, order, changed)
    };
    crate::games::render(ctx, app);
    if !changed {
        return;
    }
    let params = DeckUpdateParams {
        deck_id,
        items: Some(order.into_iter().map(DeckItemInput::existing).collect()),
        ..DeckUpdateParams::default()
    };
    update(ctx, app, &name, params);
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "test assertions")]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn deck_rows_keep_the_order_and_say_what_each_item_is() {
        let deck: Deck = serde_json::from_value(json!({
            "deckId": "abc", "name": "Couch", "owned": true, "itemCount": 4,
            "items": [
                {"id": 7, "position": 1, "kind": "script", "name": "Zelda",
                 "zapscript": "@NES/zelda", "media": {"system": "NES", "path": "/z.nes",
                 "name": "The Legend of Zelda", "available": true}},
                {"id": 3, "position": 2, "kind": "script", "name": "EarthBound",
                 "media": {"system": "SNES", "path": "/e.sfc", "name": "", "available": false}},
                {"id": 9, "position": 3, "kind": "card", "name": "Party", "hasImage": true},
                {"id": 4, "position": 4, "kind": "script", "name": "Random", "zapscript": "**launch.random:genesis"}
            ]
        }))
        .unwrap();
        let rows = rows(&deck);
        let ids: Vec<_> = rows
            .iter()
            .map(|row| row.deck_item.as_ref().unwrap().id)
            .collect();
        assert_eq!(ids, [7, 3, 9, 4]);
        assert_eq!(rows[0].display, "The Legend of Zelda");
        assert!(rows[0].media_capable && rows[0].deck_item.as_ref().unwrap().game);
        assert_eq!(
            (rows[0].system_id.as_str(), rows[0].path.as_str()),
            ("NES", "/z.nes")
        );
        // An item whose game is not here keeps its own name and does not
        // launch on its own.
        assert_eq!(rows[1].display, "EarthBound");
        assert!(!rows[1].media_capable && !rows[1].has_cover);
        assert!(rows[2].deck_item.as_ref().unwrap().card_art && rows[2].has_cover);
        assert!(!rows[3].has_cover && rows[3].system_id.is_empty());
        assert_eq!(rows[1].selection_key(), "#3");
        // Without Core's flags the item's flags stay unknown.
        assert!(rows.iter().all(|row| !row.tags_known));
    }

    #[test]
    fn a_deck_gone_from_the_list_is_gone_even_while_it_is_the_open_deck() {
        let deck = |id: &str, count: u32| Deck {
            deck_id: id.into(),
            item_count: count,
            ..Deck::default()
        };
        let open = deck("abc", 3);
        let others = [deck("xyz", 1)];
        assert!(pick_deck(Some(&open), Some(&others), "abc").is_none());
        // Before the list loads, the open deck answers for itself.
        assert_eq!(
            pick_deck(Some(&open), None, "ABC").map(|d| d.item_count),
            Some(3)
        );
        // While both have it, the open deck's newer copy wins.
        let listed = [deck("abc", 2)];
        assert_eq!(
            pick_deck(Some(&open), Some(&listed), "abc").map(|d| d.item_count),
            Some(3)
        );
        assert_eq!(
            pick_deck(None, Some(&listed), "abc").map(|d| d.item_count),
            Some(2)
        );
    }

    #[test]
    fn a_game_item_with_flags_is_a_full_game_row() {
        let deck: Deck = serde_json::from_value(json!({
            "deckId": "abc", "owned": true, "items": [
                {"id": 1, "position": 1, "kind": "script", "media": {"system": "NES",
                 "path": "/z.nes", "name": "Zelda", "available": true,
                 "userTags": [{"type": "user", "tag": "liked"}, {"type": "user", "tag": "hidden"}]}},
                {"id": 2, "position": 2, "kind": "script", "media": {"system": "SNES",
                 "path": "/e.sfc", "name": "EarthBound", "available": false, "userTags": []}}
            ]
        }))
        .unwrap();
        let rows = rows(&deck);
        assert!(rows[0].tags_known && rows[0].is_hidden);
        assert!(rows[0].flags.liked && !rows[0].flags.favorite);
        // A game this device no longer has is not one to flag.
        assert!(!rows[1].tags_known);
    }
}
