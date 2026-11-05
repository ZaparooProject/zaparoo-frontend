// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// The Menu chip row's adapter. `zaparoo_app::page_bar` decides which chips a
// screen has and how focus walks them; this publishes each screen's chips
// with their current values, owns the row's focus, and runs a chip. The
// drivers publish their own screen's row as they render, so a screen that
// stays on show during a pending route keeps its own chips.

use std::cell::Cell;

use slint::{ComponentHandle, Model, ModelRc, VecModel};
use zaparoo_app::page_bar::{self as rules, Chip, Outcome, Surface};

use crate::router::{lock, menu_row, menu_row_keyed, Ctx, ListContext, Shared};
use crate::{App, GamesMode, PageBar, Screen, Shell};

thread_local! {
    /// A chip opened a picker: the screen and the chip to put focus back on
    /// if the picker is backed out of.
    static RESUME: Cell<Option<(Screen, usize)>> = const { Cell::new(None) };
    /// Games browses a live folder, which has no chips: Go to, Filter and
    /// Search here all need Core's index. Set by the games driver with the
    /// mode, read where only the screen or mode is at hand (geometry).
    static LIVE_BROWSE: Cell<bool> = const { Cell::new(false) };
}

/// The games driver entered or left a live folder.
pub(crate) fn set_live_browse(live: bool) {
    LIVE_BROWSE.with(|cell| cell.set(live));
}

/// The Games screen's surface: none for a live folder.
fn browse_surface() -> Surface {
    if LIVE_BROWSE.with(Cell::get) {
        Surface::Other
    } else {
        Surface::Games
    }
}

/// The screen on show, as far as its page actions go.
fn surface_of(screen: Screen) -> Surface {
    match screen {
        Screen::Hub => Surface::Hub,
        Screen::Systems => Surface::Systems,
        Screen::FavoriteSystems => Surface::FavoriteSystems,
        Screen::Games => browse_surface(),
        Screen::Favorites => Surface::Favorites,
        Screen::SearchResults => Surface::SearchResults,
        _ => Surface::Other,
    }
}

/// The games-style screens share one driver; its mode says which it is.
pub(crate) fn games_surface(mode: GamesMode) -> Surface {
    match mode {
        GamesMode::Browse => browse_surface(),
        GamesMode::Favorites => Surface::Favorites,
        GamesMode::Search => Surface::SearchResults,
        GamesMode::Recents => Surface::Other,
    }
}

/// The band a games-style screen's content gives up to the row: the row's
/// own band where the screen has one, nothing otherwise.
pub(crate) fn games_row_block(mode: GamesMode, derived: &zaparoo_app::sizing::Derived) -> i32 {
    if rules::has_row(games_surface(mode), derived.menu_row) {
        derived.menu_row_block
    } else {
        0
    }
}

/// The Systems screen's surface: a category's systems or the favorites'.
fn systems_surface(favorites: bool) -> Surface {
    if favorites {
        Surface::FavoriteSystems
    } else {
        Surface::Systems
    }
}

/// The same for the Systems screen.
pub(crate) fn systems_row_block(favorites: bool, derived: &zaparoo_app::sizing::Derived) -> i32 {
    if rules::has_row(systems_surface(favorites), derived.menu_row) {
        derived.menu_row_block
    } else {
        0
    }
}

fn active_surface(app: &App) -> Surface {
    surface_of(app.global::<Shell>().get_active_screen())
}

/// The scene has a row: above 240p, with the setting on, and not on the
/// HDMI head of a dual-head pair.
fn scene_row(app: &App) -> bool {
    app.global::<crate::Sizing>().get_menu_row()
}

/// The chips of the screen on show, as published.
fn active_chips(app: &App) -> ModelRc<crate::MenuEntry> {
    let bar = app.global::<PageBar>();
    match active_surface(app) {
        Surface::Hub => bar.get_hub_chips(),
        Surface::Systems | Surface::FavoriteSystems => bar.get_systems_chips(),
        Surface::Games | Surface::Favorites | Surface::SearchResults => bar.get_games_chips(),
        Surface::Other => ModelRc::default(),
    }
}

/// The vocabulary key for the favorites order, so a chip or a Menu row
/// can name it in the reader's language.
pub(crate) fn favorites_sort_label(shared: &Shared) -> &'static str {
    if shared.favorites_sort == "name" {
        "sort:name"
    } else {
        "sort:default"
    }
}

/// The same for the favorites grouping.
pub(crate) fn favorites_grouping_label(shared: &Shared) -> &'static str {
    if shared.persist.settings.favorites_grouping == "system" {
        "group:system"
    } else {
        "group:none"
    }
}

/// One chip, worded through `Labels.menu` with its current value. Go to
/// does not name the letter the selection is in: the picker opens on it,
/// and a label that changes as the selection moves flickers.
fn entry(chip: Chip, shared: &Shared) -> crate::MenuEntry {
    match chip {
        Chip::Filter => menu_row_keyed(
            chip.id(),
            "filter",
            &crate::browse_filter::summary(shared).unwrap_or_default(),
        ),
        Chip::Sort => menu_row_keyed(chip.id(), "favorites_sort", favorites_sort_label(shared)),
        Chip::GroupBy => menu_row_keyed(
            chip.id(),
            "favorites_grouping",
            favorites_grouping_label(shared),
        ),
        Chip::AddToHub => menu_row_keyed(
            chip.id(),
            if crate::search::on_hub(shared) {
                "hub:remove"
            } else {
                "save_to_hub"
            },
            "",
        ),
        Chip::GoTo
        | Chip::SearchHere
        | Chip::EditSearch
        | Chip::RandomFavorite
        | Chip::AddItem
        | Chip::More => menu_row(chip.id()),
    }
}

fn entries(surface: Surface, shared: &Shared) -> Vec<crate::MenuEntry> {
    rules::chips(surface)
        .iter()
        .map(|&chip| entry(chip, shared))
        .collect()
}

/// Replace a chip list only when it changed, so a move that leaves the
/// chips as they were repaints nothing.
fn set_if_changed(
    current: &ModelRc<crate::MenuEntry>,
    next: Vec<crate::MenuEntry>,
    set: impl FnOnce(ModelRc<crate::MenuEntry>),
) {
    let same = current.row_count() == next.len()
        && next
            .iter()
            .enumerate()
            .all(|(i, e)| current.row_data(i).as_ref() == Some(e));
    if !same {
        set(ModelRc::new(VecModel::from(next)));
    }
}

/// Publish the games-style screen's chips.
pub(crate) fn publish_games(app: &App, shared: &Shared, mode: GamesMode) {
    let bar = app.global::<PageBar>();
    let next = entries(games_surface(mode), shared);
    set_if_changed(&bar.get_games_chips(), next, |model| {
        bar.set_games_chips(model);
    });
    clamp_focus(app);
}

/// Publish the Systems screen's chips.
pub(crate) fn publish_systems(app: &App, shared: &Shared, favorites: bool) {
    let bar = app.global::<PageBar>();
    let next = entries(systems_surface(favorites), shared);
    set_if_changed(&bar.get_systems_chips(), next, |model| {
        bar.set_systems_chips(model);
    });
    clamp_focus(app);
}

/// Publish the Hub's chips.
pub(crate) fn publish_hub(app: &App, shared: &Shared) {
    let bar = app.global::<PageBar>();
    let next = entries(Surface::Hub, shared);
    set_if_changed(&bar.get_hub_chips(), next, |model| bar.set_hub_chips(model));
    clamp_focus(app);
}

/// Republish the games-style screen's chips after a chip changed what they
/// show, without a full render.
fn refresh_games(ctx: &Ctx, app: &App) {
    let shared = lock(&ctx.shared);
    publish_games(app, &shared, shared.games.mode);
}

fn clamp_focus(app: &App) {
    let bar = app.global::<PageBar>();
    if !bar.get_focused() {
        return;
    }
    let count = active_chips(app).row_count();
    if count == 0 {
        leave(app);
    } else if bar.get_index().max(0) as usize >= count {
        bar.set_index(i32::try_from(count - 1).unwrap_or(0));
    }
}

/// Whether the Menu button focuses the row on the screen on show, rather
/// than opening the Menu list.
fn row_takes_menu(app: &App) -> bool {
    rules::has_row(active_surface(app), scene_row(app)) && active_chips(app).row_count() > 0
}

/// The row has focus.
pub(crate) fn focused(app: &App) -> bool {
    app.global::<PageBar>().get_focused()
}

/// The Menu button on a screen: focus the row where it is on screen. False
/// when the screen keeps the Menu list, for the driver to open it.
pub(crate) fn enter(ctx: &Ctx, app: &App) -> bool {
    if !row_takes_menu(app) {
        return false;
    }
    if active_surface(app) == Surface::Games {
        // The two pickers behind the first chips fetch as the row opens,
        // so both are likely ready by the time a chip is pressed.
        crate::router::ensure_letter_index(ctx, app);
        crate::browse_filter::begin(ctx, app);
    }
    RESUME.with(|resume| resume.set(None));
    let bar = app.global::<PageBar>();
    bar.set_index(0);
    bar.set_focused(true);
    true
}

/// Focus goes back to the content, where it was.
pub(crate) fn leave(app: &App) {
    app.global::<PageBar>().set_focused(false);
}

/// A route committed: the row's focus and any pending return belong to
/// the screen that was left.
pub(crate) fn screen_changed(app: &App) {
    leave(app);
    RESUME.with(|resume| resume.set(None));
}

/// The scene changed: a scene with no row cannot keep its focus.
pub(crate) fn sync(app: &App) {
    if focused(app) && !row_takes_menu(app) {
        leave(app);
    }
}

/// An action while the row has focus.
pub(crate) fn handle_action(ctx: &Ctx, app: &App, action: &str) {
    let bar = app.global::<PageBar>();
    let count = active_chips(app).row_count();
    let index = bar.get_index().max(0) as usize;
    match rules::action(action, index, count) {
        Outcome::Focus(next) => bar.set_index(i32::try_from(next).unwrap_or(0)),
        Outcome::Activate(chip) => activate(ctx, app, chip),
        Outcome::Leave => leave(app),
        Outcome::Stay => {}
    }
}

/// A chip was clicked: run it as if focused and accepted.
pub(crate) fn clicked(ctx: &Ctx, app: &App, index: usize) {
    if !row_takes_menu(app) || index >= active_chips(app).row_count() {
        return;
    }
    let bar = app.global::<PageBar>();
    bar.set_index(i32::try_from(index).unwrap_or(0));
    bar.set_focused(true);
    activate(ctx, app, index);
}

fn activate(ctx: &Ctx, app: &App, index: usize) {
    let Some(id) = active_chips(app).row_data(index).map(|e| e.id.to_string()) else {
        return;
    };
    let screen = app.global::<Shell>().get_active_screen();
    let surface = surface_of(screen);
    // Focus leaves the row while what the chip opened is up.
    leave(app);
    match id.as_str() {
        "jump_letter" => crate::router::open_go_to(ctx, app),
        "filter" => crate::browse_filter::open(ctx, app),
        "search_here" => crate::search::enter_scoped(ctx, app),
        "edit_search" => crate::search::edit(ctx, app),
        "favorites_sort" => crate::router::open_favorites_sort_menu(ctx, app),
        "favorites_grouping" => crate::router::open_favorites_grouping_menu(ctx, app),
        "launch_random_favorite" => crate::router::favorites_page_menu_accept(ctx, app, &id),
        "add_to_hub" => {
            crate::search::toggle_hub(ctx, app);
            // The chip's own words change; focus stays on it to show it.
            refresh_games(ctx, app);
            app.global::<PageBar>().set_focused(true);
            return;
        }
        "hub_add" => crate::hub::page_menu_accept(ctx, app, &id),
        "more" => open_more(ctx, app, surface),
        _ => {}
    }
    // A picker is up: backing out of it comes back to this chip.
    if crate::input::modal_open(app) && app.global::<Shell>().get_active_screen() == screen {
        RESUME.with(|resume| resume.set(Some((screen, index))));
    }
}

/// The rare actions as the Menu list, titled More.
fn open_more(ctx: &Ctx, app: &App, surface: Surface) {
    let context = match surface {
        Surface::Hub => ListContext::HubPageMenu,
        Surface::Favorites => ListContext::FavoritesPageMenu,
        _ => return,
    };
    let rows = rules::more_rows(surface)
        .iter()
        .map(|id| menu_row(id))
        .collect();
    crate::router::present_list(ctx, app, context, "title:more", rows);
}

/// A picker, list or dialog may just have closed. Backing out of one a
/// chip opened returns to that chip; anything else leaves focus on the
/// content. Nothing happens while an overlay is still up.
pub(crate) fn overlay_settled(app: &App, action: &str) {
    if crate::input::modal_open(app) {
        return;
    }
    let Some((screen, index)) = RESUME.with(Cell::take) else {
        return;
    };
    let shell = app.global::<Shell>();
    if !rules::returns_to_row(action)
        || shell.get_active_screen() != screen
        || shell.get_transitioning()
        || !row_takes_menu(app)
    {
        return;
    }
    let count = active_chips(app).row_count();
    if index < count {
        let bar = app.global::<PageBar>();
        bar.set_index(i32::try_from(index).unwrap_or(0));
        bar.set_focused(true);
    }
}

/// Wire the chips' pointer input.
pub(crate) fn bind(ctx: &std::sync::Arc<Ctx>, app: &App) {
    let ctx = ctx.clone();
    let weak = app.as_weak();
    app.global::<crate::PageBarInput>()
        .on_chip_clicked(move |index| {
            let Some(app) = weak.upgrade() else {
                return;
            };
            crate::router::reset_idle(&ctx, &app);
            if let Ok(index) = usize::try_from(index) {
                clicked(&ctx, &app, index);
            }
        });
}

/// The chip ids of the screen on show.
#[cfg(all(test, feature = "mister"))]
pub(crate) fn chip_ids(app: &App) -> Vec<slint::SharedString> {
    active_chips(app).iter().map(|e| e.id).collect()
}
