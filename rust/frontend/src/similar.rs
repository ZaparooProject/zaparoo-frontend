// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// "Similar games": the games Core finds like this one (`media.similar`),
// as a page of the context menu. A row opens the game's details in the
// menu's place, saying what it shares with this one and offering Play;
// Back there brings the page back on the same row.

use zaparoo_app::suggestions::{self, Shared};
use zaparoo_core::media_types::{MediaSimilarParams, SuggestedMedia};

use crate::context_page::{Page, PageRow};
use crate::games::GameRow;
use crate::router::{lock, Ctx};
use crate::App;

/// How many games the page asks Core for.
const MAX_SIMILAR: u32 = 20;

/// The game a row of the page stands for.
#[derive(Debug, Clone)]
pub struct SimilarGame {
    pub game: GameRow,
    /// The strongest thing it shares that the interface can word.
    pub shared: Option<Shared>,
}

/// Menu row accepted: ask Core for games like the one the menu is for,
/// leaving the menu open. Core finds the game by its id when there is one,
/// else by its system and path.
pub fn begin(ctx: &Ctx, app: &App, media_id: Option<i64>, system: &str, path: &str) {
    let client = ctx.store.client();
    let params = MediaSimilarParams {
        media_id,
        system: if media_id.is_some() {
            String::new()
        } else {
            system.to_string()
        },
        path: if media_id.is_some() {
            String::new()
        } else {
            path.to_string()
        },
        max_results: Some(MAX_SIMILAR),
    };
    crate::context_page::begin(ctx, app, Page::Similar, async move {
        let result = client.media_similar(params).await.map_err(|e| e.message)?;
        Ok(rows(&result.results))
    });
}

/// A row of the page accepted: the game's details open in the menu's
/// place, on Play.
pub fn accept(ctx: &Ctx, app: &App, id: &str) {
    let similar = crate::context_page::row_index(id).and_then(|index| {
        let shared = lock(&ctx.shared);
        let row = shared.context_page.rows.get(index)?;
        Some((index, row.similar.clone()?))
    });
    let Some((index, similar)) = similar else {
        return;
    };
    crate::router::open_from_page(
        ctx,
        app,
        zaparoo_app::options_menu::Opens::Details,
        index,
        || {
            crate::game_info::open_as(
                ctx,
                app,
                &similar.game,
                crate::game_info::Opened {
                    play: true,
                    shared: similar.shared,
                },
            );
        },
    );
}

/// The games in Core's order, each one Core sent a way to launch.
fn rows(games: &[SuggestedMedia]) -> Vec<PageRow> {
    games
        .iter()
        .filter_map(|item| {
            let game = GameRow::from(item);
            let launch_text = game.launch_text()?;
            let shared = suggestions::strongest(item.reasons.iter().map(|r| r.code.as_str()));
            Some(PageRow {
                label_key: "",
                label: game.name.clone(),
                name: game.name.clone(),
                launch_text,
                similar: Some(Box::new(SimilarGame { game, shared })),
                deck: None,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used, reason = "fixed fixtures always parse")]

    use super::*;

    #[test]
    fn rows_carry_the_game_and_what_it_shares() {
        let games: Vec<SuggestedMedia> = serde_json::from_value(serde_json::json!([
            {
                "name": "Contra III",
                "path": "SNES/Contra III.sfc",
                "zapScript": "",
                "reasons": [{ "code": "mood" }, { "code": "series", "tags": [{ "type": "series", "tag": "contra" }] }],
            },
            {
                "name": "Gunstar Heroes",
                "path": "",
                "zapScript": "@Genesis/Gunstar Heroes",
                "reasons": [{ "code": "mood" }],
            },
            { "name": "Nothing to launch", "path": "", "zapScript": "", "reasons": [] },
        ]))
        .expect("similar");
        let rows = rows(&games);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].label, "Contra III");
        assert_eq!(rows[0].launch_text, "SNES/Contra III.sfc");
        let first = rows[0].similar.as_ref().expect("first game");
        assert_eq!(first.game.path, "SNES/Contra III.sfc");
        assert_eq!(first.shared, Some(Shared::Series));
        assert_eq!(rows[1].launch_text, "@Genesis/Gunstar Heroes");
        assert_eq!(rows[1].similar.as_ref().and_then(|s| s.shared), None);
    }
}
