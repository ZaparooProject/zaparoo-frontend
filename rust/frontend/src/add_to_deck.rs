// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// The Add to deck page of a game's Options menu: every deck the user can
// change, checked where it already holds the game, and New deck last.
// Accept adds the game to a deck or takes it out, in place; the page stays
// open so several decks can be changed in one visit. Core says which decks
// hold the game (`decks` with the game's system and path), so the checks
// are never worked out here.

use std::sync::Arc;

use slint::ComponentHandle;

use zaparoo_core::client::Client;
use zaparoo_core::collections::{Deck, DeckItemInput, DeckUpdateParams, DecksParams};
use zaparoo_core::endpoints::decks::DeckUpdateMutation;

use crate::context_page::{DeckChoice, Page, PageRow};
use crate::router::{lock, Ctx};
use crate::App;

/// The game the page adds and removes.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeckGame {
    pub system: String,
    pub path: String,
    /// What the game is called, for an alert.
    pub name: String,
}

impl DeckGame {
    fn params(&self) -> DecksParams {
        DecksParams {
            system: self.system.clone(),
            path: self.path.clone(),
        }
    }
}

/// The page's rows for Core's deck list: the decks the user can change,
/// in Core's order, then New deck.
pub(crate) fn rows(decks: &[Deck]) -> Vec<PageRow> {
    let mut rows: Vec<PageRow> = decks
        .iter()
        .filter(|deck| deck.listed() && deck.editable())
        .map(deck_row)
        .collect();
    rows.push(PageRow {
        label_key: "new_deck",
        label: String::new(),
        name: String::new(),
        launch_text: String::new(),
        deck: Some(DeckChoice::New),
    });
    rows
}

fn deck_row(deck: &Deck) -> PageRow {
    let name = deck.name.trim().to_string();
    PageRow {
        label_key: "",
        label: name.clone(),
        name,
        launch_text: String::new(),
        deck: Some(DeckChoice::Deck {
            deck_id: deck.deck_id.clone(),
            item_ids: deck.contains_item_ids.clone().unwrap_or_default(),
            full: deck.full(),
        }),
    }
}

fn deck_id(row: &PageRow) -> Option<&str> {
    match &row.deck {
        Some(DeckChoice::Deck { deck_id, .. }) => Some(deck_id),
        _ => None,
    }
}

/// Fold a fresh deck list into the rows on screen without moving them:
/// Core lists the deck changed last first, and a deck jumping under the
/// cursor the moment it is checked would be worse than a stale order. A
/// deck still waiting on Core keeps its row as the user left it; decks
/// that are new land before New deck, and decks that are gone leave.
pub(crate) fn merge(current: &[PageRow], fresh: Vec<PageRow>, pending: &[String]) -> Vec<PageRow> {
    let mut fresh_decks: Vec<PageRow> = fresh
        .into_iter()
        .filter(|row| deck_id(row).is_some())
        .collect();
    let mut merged = Vec::with_capacity(fresh_decks.len() + 1);
    for row in current {
        let Some(id) = deck_id(row) else {
            continue;
        };
        let Some(at) = fresh_decks.iter().position(|f| deck_id(f) == Some(id)) else {
            continue;
        };
        let fresh = fresh_decks.remove(at);
        merged.push(if pending.iter().any(|p| p == id) {
            row.clone()
        } else {
            fresh
        });
    }
    merged.extend(fresh_decks);
    merged.extend(rows(&[]));
    merged
}

async fn fetch(client: Arc<Client>, game: DeckGame) -> Result<Vec<PageRow>, String> {
    client
        .decks(game.params())
        .await
        .map(|result| rows(&result.decks))
        .map_err(|e| e.message)
}

/// Add to deck was accepted on a game: ask Core for the decks.
pub fn begin(ctx: &Ctx, app: &App, system: &str, path: &str, name: &str) {
    let game = DeckGame {
        system: system.to_string(),
        path: path.to_string(),
        name: name.to_string(),
    };
    crate::context_page::begin(
        ctx,
        app,
        Page::AddToDeck,
        fetch(ctx.store.client(), game.clone()),
    );
    lock(&ctx.shared).context_page.deck_game = Some(game);
}

/// Ask Core again and fold the answer into the page, if it is still up:
/// after a change made here, or one Core reported from anywhere else.
pub(crate) fn refresh(ctx: &Ctx, app: &App) {
    let (seq, game) = {
        let shared = lock(&ctx.shared);
        let page = &shared.context_page;
        if page.showing != Some(Page::AddToDeck) {
            return;
        }
        let Some(game) = page.deck_game.clone() else {
            return;
        };
        (page.seq, game)
    };
    let client = ctx.store.client();
    let ctx2 = ctx.clone();
    let weak = app.as_weak();
    ctx.handle.spawn(async move {
        let found = fetch(client, game).await;
        let _ = weak.upgrade_in_event_loop(move |app| {
            let Ok(fresh) = found else {
                // The page still shows what it last knew; the next change
                // or visit asks again.
                return;
            };
            let index = {
                let mut shared = lock(&ctx2.shared);
                let page = &mut shared.context_page;
                if page.seq != seq || page.showing != Some(Page::AddToDeck) {
                    return;
                }
                page.rows = merge(&page.rows, fresh, &page.deck_pending);
                usize::try_from(app.global::<crate::Overlays>().get_context_index()).unwrap_or(0)
            };
            crate::context_page::present_at(&ctx2, &app, Page::AddToDeck, index);
        });
    });
}

/// A row was accepted: toggle the game in that deck, or name a new one.
pub fn accept(ctx: &Ctx, app: &App, index: usize) {
    let (row, game, seq) = {
        let shared = lock(&ctx.shared);
        let page = &shared.context_page;
        let (Some(row), Some(game)) = (page.rows.get(index).cloned(), page.deck_game.clone())
        else {
            return;
        };
        (row, game, page.seq)
    };
    match row.deck {
        Some(DeckChoice::New) => {
            let source = app.global::<crate::Shell>().get_active_screen();
            crate::router::close_context_menu(ctx, app);
            crate::rename::open_new_deck(ctx, app, game, source);
        }
        Some(DeckChoice::Deck {
            deck_id,
            item_ids,
            full,
        }) => toggle(
            ctx, app, index, seq, &game, &row.name, deck_id, item_ids, full,
        ),
        None => {}
    }
}

#[allow(
    clippy::too_many_arguments,
    reason = "one row's whole state, taken apart once"
)]
fn toggle(
    ctx: &Ctx,
    app: &App,
    index: usize,
    seq: u64,
    game: &DeckGame,
    name: &str,
    deck_id: String,
    item_ids: Vec<i64>,
    full: bool,
) {
    let adding = item_ids.is_empty();
    // A full deck takes nothing more; its row is dimmed and does nothing.
    if adding && full {
        return;
    }
    {
        let mut shared = lock(&ctx.shared);
        let page = &mut shared.context_page;
        if page.deck_pending.contains(&deck_id) {
            return;
        }
        page.deck_pending.push(deck_id.clone());
        // Checked at once; Core's answer fills in the real item ids.
        if let Some(Some(DeckChoice::Deck {
            item_ids: shown, ..
        })) = page.rows.get_mut(index).map(|row| row.deck.as_mut())
        {
            *shown = if adding { vec![0] } else { Vec::new() };
        }
    }
    crate::context_page::refresh_row(ctx, app, index);
    let params = if adding {
        DeckUpdateParams {
            deck_id: deck_id.clone(),
            add_items: vec![DeckItemInput::game(&game.system, &game.path)],
            ..DeckUpdateParams::default()
        }
    } else {
        DeckUpdateParams {
            deck_id: deck_id.clone(),
            remove_item_ids: item_ids.clone(),
            ..DeckUpdateParams::default()
        }
    };
    let store = ctx.store.clone();
    let ctx2 = ctx.clone();
    let weak = app.as_weak();
    let name = name.to_string();
    let wait = crate::cue::begin(ctx, app, crate::AppCue::Saving, "", "");
    ctx.handle.spawn(async move {
        let result = store.run_mutation::<DeckUpdateMutation>(params).await;
        let _ = weak.upgrade_in_event_loop(move |app| {
            crate::cue::end(&ctx2, &app, wait);
            let current = {
                let mut shared = lock(&ctx2.shared);
                let page = &mut shared.context_page;
                page.deck_pending.retain(|id| *id != deck_id);
                page.seq == seq
            };
            if let Err(e) = result {
                tracing::warn!("deck update failed for {name}: {}", e.message);
                if current {
                    if let Some(Some(DeckChoice::Deck {
                        item_ids: shown, ..
                    })) = lock(&ctx2.shared)
                        .context_page
                        .rows
                        .get_mut(index)
                        .map(|row| row.deck.as_mut())
                    {
                        *shown = item_ids;
                    }
                    crate::context_page::refresh_row(&ctx2, &app, index);
                }
                crate::router::report_action_error(&ctx2, &app, "deck_update", &name);
                return;
            }
            if current {
                refresh(&ctx2, &app);
            }
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn deck(id: &str, owned: bool, saved: bool, locked: bool, holds: &[i64]) -> Deck {
        Deck {
            deck_id: id.into(),
            name: format!(" {id} "),
            owned,
            saved,
            locked,
            item_count: 3,
            contains_item_ids: Some(holds.to_vec()),
            ..Deck::default()
        }
    }

    fn ids(rows: &[PageRow]) -> Vec<String> {
        rows.iter()
            .map(|row| match &row.deck {
                Some(DeckChoice::Deck {
                    deck_id, item_ids, ..
                }) => {
                    format!("{deck_id}:{}", item_ids.len())
                }
                Some(DeckChoice::New) => "new".into(),
                None => "?".into(),
            })
            .collect()
    }

    #[test]
    fn lists_only_decks_the_user_can_change_then_new_deck() {
        let decks = [
            deck("own", true, false, false, &[7]),
            deck("locked", true, false, true, &[]),
            deck("saved", false, true, false, &[]),
            deck("cached", false, false, false, &[]),
            deck("empty", true, false, false, &[]),
        ];
        let rows = rows(&decks);
        assert_eq!(ids(&rows), ["own:1", "empty:0", "new"]);
        assert_eq!(rows[0].label, "own");
    }

    #[test]
    fn a_full_deck_is_marked_full() {
        let full = Deck {
            item_count: zaparoo_core::collections::MAX_DECK_ITEMS,
            ..deck("full", true, false, false, &[])
        };
        assert!(matches!(
            rows(&[full])[0].deck,
            Some(DeckChoice::Deck { full: true, .. })
        ));
    }

    #[test]
    fn merging_keeps_the_order_on_screen_and_the_rows_still_waiting() {
        let shown = rows(&[
            deck("a", true, false, false, &[]),
            deck("b", true, false, false, &[]),
            deck("c", true, false, false, &[]),
        ]);
        // Core moved b to the front after a change, dropped c, and has a
        // new deck d; a's change is still on its way.
        let mut waiting = shown.clone();
        if let Some(DeckChoice::Deck { item_ids, .. }) = waiting[0].deck.as_mut() {
            *item_ids = vec![0];
        }
        let fresh = rows(&[
            deck("b", true, false, false, &[9]),
            deck("d", true, false, false, &[]),
            deck("a", true, false, false, &[]),
        ]);
        let merged = merge(&waiting, fresh, &["a".to_string()]);
        assert_eq!(ids(&merged), ["a:1", "b:1", "d:0", "new"]);
    }
}
