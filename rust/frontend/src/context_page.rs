// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// A page of the context menu: a list Core has to be asked for, offered in
// place of the menu's own rows. The menu stays open while Core answers,
// its rows become the list, and Back returns to the menu with focus on the
// row that opened the page. What each page lists is its own module's
// business (`alternates`, `discs`).

use std::future::Future;

use slint::{ComponentHandle, Model as _, ModelRc, VecModel};

use crate::router::{lock, Ctx};
use crate::App;

/// The pages the games context menu can turn to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Alternates,
    Discs,
}

impl Page {
    /// The menu row that opens the page.
    pub fn menu_id(self) -> &'static str {
        match self {
            Self::Alternates => "discover",
            Self::Discs => "choose_disc",
        }
    }

    /// The row's label while Core is being asked.
    fn waiting_key(self) -> &'static str {
        match self {
            Self::Alternates => "discover:searching",
            Self::Discs => "choose_disc:loading",
        }
    }

    /// The row's label when Core had nothing to list.
    fn empty_key(self) -> &'static str {
        match self {
            Self::Alternates => "discover:none",
            Self::Discs => "choose_disc:none",
        }
    }

    fn error_kind(self) -> &'static str {
        match self {
            Self::Alternates => "alternate_discovery",
            Self::Discs => "disc_list",
        }
    }

    /// The row as it reads once the wait or the empty answer replaced it.
    fn owns_label(self, key: &str) -> bool {
        key == self.waiting_key() || key == self.empty_key()
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
}

impl PageModel {
    /// Drop the page and orphan any answer still on its way.
    pub fn reset(&mut self) {
        self.seq += 1;
        self.showing = None;
        self.rows.clear();
    }
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
            crate::router::menu_row(page.menu_id())
        } else {
            entry
        }
    });
}

/// Replace the menu's rows with the page's.
fn present(ctx: &Ctx, app: &App, page: Page) {
    let entries: Vec<crate::MenuEntry> = {
        let mut shared = lock(&ctx.shared);
        shared.context_page.showing = Some(page);
        shared
            .context_page
            .rows
            .iter()
            .enumerate()
            .map(|(index, row)| {
                if row.label_key.is_empty() {
                    crate::router::menu_entry(&row_id(index), &row.label)
                } else {
                    crate::router::menu_row_keyed(&row_id(index), row.label_key, &row.label)
                }
            })
            .collect()
    };
    let overlays = app.global::<crate::Overlays>();
    overlays.set_context_entries(ModelRc::new(VecModel::from(entries)));
    overlays.set_context_index(0);
}

/// True when the menu is showing a page rather than its own entries.
pub fn showing(ctx: &Ctx) -> bool {
    lock(&ctx.shared).context_page.showing.is_some()
}

/// Back on a page returns to the menu it came from, on the row that
/// opened it, instead of closing the menu.
pub fn leave(ctx: &Ctx, app: &App) {
    let page = {
        let mut shared = lock(&ctx.shared);
        let page = shared.context_page.showing;
        shared.context_page.reset();
        page
    };
    crate::games::reopen_context_menu(ctx, app);
    let overlays = app.global::<crate::Overlays>();
    let opener = page.and_then(|page| {
        overlays
            .get_context_entries()
            .iter()
            .position(|entry| entry.id.as_str() == page.menu_id())
    });
    if let Some(index) = opener {
        overlays.set_context_index(i32::try_from(index).unwrap_or(0));
    }
}

/// A row on the page was accepted: run it and close.
pub fn accept(ctx: &Ctx, app: &App, id: &str) {
    let index = id
        .strip_prefix("page_row:")
        .and_then(|index| index.parse::<usize>().ok());
    let row = {
        let mut shared = lock(&ctx.shared);
        let row = index.and_then(|index| shared.context_page.rows.get(index).cloned());
        shared.context_page.reset();
        row
    };
    app.global::<crate::Overlays>().set_context_open(false);
    if let Some(row) = row {
        crate::router::launch(ctx, app, row.launch_text, &row.name);
    }
}
