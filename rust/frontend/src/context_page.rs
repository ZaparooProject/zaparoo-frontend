// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// A page of the context menu, offered in place of the menu's own rows. Back
// returns to the menu with focus on the row that opened the page. A page is
// either part of the menu itself (Manage, Write to token: the rows are
// already known) or a list Core has to be asked for, in which case the menu
// stays open while Core answers and what the page lists is its own module's
// business (`alternates`, `discs`, `title_versions`, `add_to_deck`).

use std::future::Future;

use slint::{ComponentHandle, Model as _, ModelRc, VecModel};

use zaparoo_app::options_menu::Page as MenuPage;

use crate::router::{lock, Ctx};
use crate::App;

/// The pages a context menu can turn to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Alternates,
    Discs,
    Versions,
    /// The decks a game can be added to or removed from.
    AddToDeck,
    /// A page of the menu's own rows.
    Menu(MenuPage),
}

impl Page {
    /// The menu row that opens the page.
    pub fn menu_id(self) -> &'static str {
        match self {
            Self::Alternates => "discover",
            Self::Discs => "choose_disc",
            Self::Versions => "choose_version",
            Self::AddToDeck => "add_to_deck",
            Self::Menu(page) => page.menu_id(),
        }
    }

    /// The row's label while Core is being asked.
    fn waiting_key(self) -> &'static str {
        match self {
            Self::Alternates => "discover:searching",
            Self::Discs => "choose_disc:loading",
            Self::Versions => "choose_version:loading",
            Self::AddToDeck => "add_to_deck:loading",
            Self::Menu(_) => "",
        }
    }

    /// The row's label when Core had nothing to list.
    fn empty_key(self) -> &'static str {
        match self {
            Self::Alternates => "discover:none",
            Self::Discs => "choose_disc:none",
            Self::Versions => "choose_version:none",
            // Never empty: New deck is always there.
            Self::AddToDeck | Self::Menu(_) => "",
        }
    }

    fn error_kind(self) -> &'static str {
        match self {
            Self::Alternates => "alternate_discovery",
            Self::Discs => "disc_list",
            Self::Versions => "version_list",
            Self::AddToDeck => "deck_list",
            Self::Menu(_) => "",
        }
    }

    /// What the view titles the page with.
    fn view(self) -> crate::ContextPage {
        match self {
            Self::Alternates => crate::ContextPage::Alternates,
            Self::Discs => crate::ContextPage::Discs,
            Self::Versions => crate::ContextPage::Versions,
            Self::AddToDeck => crate::ContextPage::AddToDeck,
            Self::Menu(MenuPage::WriteToken) => crate::ContextPage::WriteToken,
            Self::Menu(MenuPage::ManageGame) => crate::ContextPage::ManageGame,
            Self::Menu(MenuPage::ManageSystem) => crate::ContextPage::ManageSystem,
            Self::Menu(MenuPage::ManageCategory) => crate::ContextPage::ManageCategory,
            Self::Menu(MenuPage::ManageDeck) => crate::ContextPage::ManageDeck,
        }
    }

    /// The row as it reads once the wait or the empty answer replaced it.
    fn owns_label(self, key: &str) -> bool {
        !key.is_empty() && (key == self.waiting_key() || key == self.empty_key())
    }
}

/// One row of a page: what to show, and what to run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageRow {
    /// A `Labels.menu` key that words `label`, or empty when `label` is
    /// the row's own text.
    pub label_key: &'static str,
    pub label: String,
    /// The name the launch cue shows.
    pub name: String,
    pub launch_text: String,
    /// On the Add to deck page, the deck the row toggles the game in, or
    /// New deck. `None` on a page whose rows launch.
    pub deck: Option<DeckChoice>,
}

/// What a row of the Add to deck page does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeckChoice {
    Deck {
        deck_id: String,
        /// The deck's items that are the game; empty when it is not in it.
        item_ids: Vec<i64>,
        /// The deck holds all it can, so the game cannot be added.
        full: bool,
    },
    New,
}

impl DeckChoice {
    fn entry(&self, id: &str, label: &str) -> crate::MenuEntry {
        match self {
            Self::Deck { item_ids, full, .. } => {
                let checked = !item_ids.is_empty();
                crate::MenuEntry {
                    role: if checked {
                        crate::MenuRole::CheckOn
                    } else {
                        crate::MenuRole::CheckOff
                    },
                    ..crate::router::menu_row_full(id, "", label, checked || !full, "")
                }
            }
            Self::New => crate::router::menu_row_keyed(id, "new_deck", ""),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct PageModel {
    pub rows: Vec<PageRow>,
    /// The page the menu is showing in place of its own entries.
    pub showing: Option<Page>,
    /// Bumped per run so a late answer cannot land on a menu the user
    /// has moved on from.
    pub seq: u64,
    /// The opening row's wait while Core has not answered.
    pub wait: Option<crate::cue::LocalWait>,
    /// The game the Add to deck page adds and removes.
    pub deck_game: Option<crate::add_to_deck::DeckGame>,
    /// Decks a change was sent for that Core has not answered yet. Their
    /// rows keep the state the user chose until it does.
    pub deck_pending: Vec<String>,
}

impl PageModel {
    /// Drop the page and orphan any answer still on its way.
    pub fn reset(&mut self) {
        self.seq += 1;
        self.showing = None;
        self.rows.clear();
        self.deck_game = None;
        self.deck_pending.clear();
    }
}

/// Whether the menu row `id` opens a page Core has to be asked for.
pub fn asks_core(id: &str) -> bool {
    [
        Page::Alternates,
        Page::Discs,
        Page::Versions,
        Page::AddToDeck,
    ]
    .iter()
    .any(|page| id == page.menu_id())
}

fn row_id(index: usize) -> String {
    format!("page_row:{index}")
}

/// The opening row was accepted: ask Core through `fetch`, leaving the
/// menu open with that row relabelled once the answer becomes a wait.
pub fn begin<F>(ctx: &Ctx, app: &App, page: Page, fetch: F)
where
    F: Future<Output = Result<Vec<PageRow>, String>> + Send + 'static,
{
    let (ticket, stale) = {
        let mut shared = lock(&ctx.shared);
        shared.context_page.reset();
        (shared.context_page.seq, shared.context_page.wait.take())
    };
    if let Some(stale) = stale {
        crate::cue::abandon_local(stale);
    }
    let shared = ctx.shared.clone();
    let wait = crate::cue::begin_local(app, move |app| {
        if lock(&shared).context_page.seq == ticket
            && app.global::<crate::Overlays>().get_context_open()
        {
            relabel(app, page, page.waiting_key());
        }
    });
    lock(&ctx.shared).context_page.wait = Some(wait);
    let ctx2 = ctx.clone();
    let weak = app.as_weak();
    ctx.handle.spawn(async move {
        let found = fetch.await;
        let _ = weak.upgrade_in_event_loop(move |app| landed(&ctx2, &app, page, ticket, found));
    });
}

/// The ticket of the page in flight, for a test to answer or outrun.
#[cfg(all(test, feature = "mister"))]
pub(crate) fn ticket(ctx: &Ctx) -> u64 {
    lock(&ctx.shared).context_page.seq
}

/// Core answered run `ticket`: show the page, or say why there is none.
pub(crate) fn landed(
    ctx: &Ctx,
    app: &App,
    page: Page,
    ticket: u64,
    found: Result<Vec<PageRow>, String>,
) {
    let wait = {
        let mut shared = lock(&ctx.shared);
        if shared.context_page.seq != ticket {
            return;
        }
        shared.context_page.wait.take()
    };
    let ctx2 = ctx.clone();
    let finish = move |app: &App| {
        // The menu the answer belongs to has to still be open.
        if lock(&ctx2.shared).context_page.seq != ticket
            || !app.global::<crate::Overlays>().get_context_open()
        {
            return;
        }
        match found {
            Ok(rows) if rows.is_empty() => relabel(app, page, page.empty_key()),
            Ok(rows) => {
                lock(&ctx2.shared).context_page.rows = rows;
                present(&ctx2, app, page);
            }
            Err(message) => {
                tracing::warn!(?page, "context menu page failed: {message}");
                // The wait is over: the row must not go on saying it is
                // still running.
                restore(app, page);
                crate::router::report_action_error(&ctx2, app, page.error_kind(), "");
            }
        }
    };
    match wait {
        Some(wait) => crate::cue::end_local(app, wait, finish),
        None => finish(app),
    }
}

fn map_entries(app: &App, map: impl Fn(crate::MenuEntry) -> crate::MenuEntry) {
    let overlays = app.global::<crate::Overlays>();
    let rows: Vec<crate::MenuEntry> = overlays.get_context_entries().iter().map(map).collect();
    overlays.set_context_entries(ModelRc::new(VecModel::from(rows)));
}

/// Swap the open menu's opening row for one that says what happened. The
/// replacement carries no id, so pressing it does nothing.
fn relabel(app: &App, page: Page, key: &str) {
    map_entries(app, |entry| {
        if entry.id.as_str() == page.menu_id() || page.owns_label(entry.label_key.as_str()) {
            crate::router::menu_row_keyed("", key, "")
        } else {
            entry
        }
    });
}

/// Put the opening row back as it was, pressable again.
fn restore(app: &App, page: Page) {
    map_entries(app, |entry| {
        if page.owns_label(entry.label_key.as_str()) {
            let mut row = crate::router::menu_row(page.menu_id());
            row.role = crate::MenuRole::Submenu;
            row
        } else {
            entry
        }
    });
}

/// Replace the menu's rows with the page's.
fn present(ctx: &Ctx, app: &App, page: Page) {
    present_at(ctx, app, page, 0);
}

/// The page's rows as the view takes them.
fn page_entries(rows: &[PageRow]) -> Vec<crate::MenuEntry> {
    rows.iter()
        .enumerate()
        .map(|(index, row)| {
            if let Some(deck) = &row.deck {
                deck.entry(&row_id(index), &row.label)
            } else if row.label_key.is_empty() {
                crate::router::menu_entry(&row_id(index), &row.label)
            } else {
                crate::router::menu_row_keyed(&row_id(index), row.label_key, &row.label)
            }
        })
        .collect()
}

/// Show the page's rows with focus on row `index`.
pub(crate) fn present_at(ctx: &Ctx, app: &App, page: Page, index: usize) {
    let entries = {
        let mut shared = lock(&ctx.shared);
        shared.context_page.showing = Some(page);
        page_entries(&shared.context_page.rows)
    };
    let last = entries.len().saturating_sub(1);
    let overlays = app.global::<crate::Overlays>();
    overlays.set_context_entries(ModelRc::new(VecModel::from(entries)));
    overlays.set_context_page(page.view());
    overlays.set_context_index(i32::try_from(index.min(last)).unwrap_or(0));
}

/// Redraw one row of the page in place, after its state changed.
pub(crate) fn refresh_row(ctx: &Ctx, app: &App, index: usize) {
    let entry = {
        let shared = lock(&ctx.shared);
        let Some(row) = shared.context_page.rows.get(index) else {
            return;
        };
        page_entries(std::slice::from_ref(row))
            .pop()
            .map(|mut entry| {
                entry.id = slint::SharedString::from(row_id(index));
                entry
            })
    };
    let Some(entry) = entry else {
        return;
    };
    let model = app.global::<crate::Overlays>().get_context_entries();
    if index < model.row_count() {
        model.set_row_data(index, entry);
    }
}

/// Turn the menu to one of its own pages: the rows are the menu's, so
/// there is nothing to wait for.
pub fn turn(ctx: &Ctx, app: &App, page: MenuPage) {
    let (entries, stale) = {
        let mut shared = lock(&ctx.shared);
        shared.context_page.reset();
        shared.context_page.showing = Some(Page::Menu(page));
        let entries: Vec<crate::MenuEntry> = shared
            .context_menu
            .rows(page)
            .iter()
            .map(|row| crate::router::menu_row_keyed(row.id, row.key, ""))
            .collect();
        (entries, shared.context_page.wait.take())
    };
    if let Some(stale) = stale {
        crate::cue::abandon_local(stale);
    }
    let overlays = app.global::<crate::Overlays>();
    overlays.set_context_entries(ModelRc::new(VecModel::from(entries)));
    overlays.set_context_page(Page::Menu(page).view());
    overlays.set_context_index(0);
}

/// True when the menu is showing a page rather than its own entries.
pub fn showing(ctx: &Ctx) -> bool {
    lock(&ctx.shared).context_page.showing.is_some()
}

/// Back on a page returns to the menu it came from, on the row that
/// opened it, instead of closing the menu.
pub fn leave(ctx: &Ctx, app: &App) {
    let (page, entries) = {
        let mut shared = lock(&ctx.shared);
        let page = shared.context_page.showing;
        shared.context_page.reset();
        (page, crate::router::root_entries(&shared.context_menu))
    };
    let opener = page.and_then(|page| {
        entries
            .iter()
            .position(|entry| entry.id.as_str() == page.menu_id())
    });
    let overlays = app.global::<crate::Overlays>();
    overlays.set_context_entries(ModelRc::new(VecModel::from(entries)));
    overlays.set_context_page(crate::ContextPage::Root);
    overlays.set_context_index(
        opener
            .and_then(|index| i32::try_from(index).ok())
            .unwrap_or(0),
    );
}

/// The page row an entry id names.
pub(crate) fn row_index(id: &str) -> Option<usize> {
    id.strip_prefix("page_row:")
        .and_then(|index| index.parse::<usize>().ok())
}

/// A row on the page was accepted: run it and close. The Add to deck page
/// stays open, its rows toggling in place.
pub fn accept(ctx: &Ctx, app: &App, id: &str) {
    let index = row_index(id);
    if lock(&ctx.shared).context_page.showing == Some(Page::AddToDeck) {
        if let Some(index) = index {
            crate::add_to_deck::accept(ctx, app, index);
        }
        return;
    }
    let row = {
        let mut shared = lock(&ctx.shared);
        let row = index.and_then(|index| shared.context_page.rows.get(index).cloned());
        shared.context_page.reset();
        row
    };
    app.global::<crate::Overlays>().set_context_open(false);
    app.global::<crate::Overlays>()
        .set_context_page(crate::ContextPage::Root);
    if let Some(row) = row {
        crate::router::launch(ctx, app, row.launch_text, &row.name);
    }
}
