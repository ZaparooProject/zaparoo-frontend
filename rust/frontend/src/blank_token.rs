// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// The blank-token panel: Core refused a token as blank, offer to write a
// game to it. It is the decision dialog with its own buttons: the focused
// game when there is one, Choose a game (the Game picker), and Cancel.

use std::cell::RefCell;

use slint::ComponentHandle;

use crate::game_picker::{PickedGame, Purpose};
use crate::router::{lock, Ctx};
use crate::{App, DialogButton, DialogKind};

/// The offer on screen: the reader the blank token is on, and the game
/// the user was on when it was read.
struct Offer {
    reader_id: String,
    focused: Option<PickedGame>,
}

thread_local! {
    static OFFER: RefCell<Option<Offer>> = const { RefCell::new(None) };
}

/// The reader of a `run.failed` that reports a blank token a reader read.
pub fn blank_read(params: &serde_json::Value) -> Option<String> {
    let field = |key: &str| params.get(key).and_then(|v| v.as_str()).unwrap_or_default();
    (field("category") == "blank" && zaparoo_app::status_line::is_reader_run(field("source")))
        .then(|| field("readerId").to_string())
}

/// Offer to write a game to the blank token on `reader_id`. A running game
/// owns the screen, and an open modal keeps it: the header line still says
/// the token is blank.
pub fn offer(ctx: &Ctx, app: &App, reader_id: String) {
    if *ctx.dormant.borrow() || crate::steam_host::hosting() {
        return;
    }
    // The token is a press: it wakes a screensaver like one.
    crate::screensaver::disarm(app);
    crate::router::reset_idle(ctx, app);
    if crate::input::modal_open(app) {
        return;
    }
    let focused = focused_game(ctx, app);
    let mut buttons = Vec::with_capacity(3);
    if focused.is_some() {
        buttons.push(DialogButton::WriteGame);
    }
    buttons.push(DialogButton::ChooseGame);
    buttons.push(DialogButton::Cancel);
    let name = focused.as_ref().map(|g| g.name.clone()).unwrap_or_default();
    OFFER.with(|offer| *offer.borrow_mut() = Some(Offer { reader_id, focused }));
    crate::router::open_dialog(app, DialogKind::BlankToken, &name, "", &buttons, 0);
}

/// The game tile in focus, on the Hub or a games screen.
fn focused_game(ctx: &Ctx, app: &App) -> Option<PickedGame> {
    let shared = lock(&ctx.shared);
    match app.global::<crate::Shell>().get_active_screen() {
        crate::Screen::Hub => crate::hub::focused_game(&shared),
        crate::Screen::Games
        | crate::Screen::Favorites
        | crate::Screen::Recents
        | crate::Screen::SearchResults
        | crate::Screen::TopPlayed
        | crate::Screen::Deck => PickedGame::from_row(shared.games.current()?),
        _ => None,
    }
}

/// The dialog's answer. Back and Cancel land here with `Cancel`.
pub fn answered(ctx: &Ctx, app: &App, button: DialogButton) {
    crate::router::close_dialog(app);
    let Some(offer) = OFFER.with(|offer| offer.borrow_mut().take()) else {
        return;
    };
    let reader = Some(offer.reader_id).filter(|id| !id.is_empty());
    match button {
        DialogButton::WriteGame => {
            if let Some(game) = offer.focused {
                crate::card_write::begin_on_reader(ctx, app, game.zapscript, reader);
            }
        }
        DialogButton::ChooseGame => {
            crate::game_picker::open(ctx, app, Purpose::WriteToken { reader_id: reader });
        }
        _ => {}
    }
}

/// The Game picker's answer for a token write: write it on that reader,
/// or nothing on Back.
pub fn picked(ctx: &Ctx, app: &App, reader_id: Option<String>, game: Option<PickedGame>) {
    if let Some(game) = game {
        crate::card_write::begin_on_reader(ctx, app, game.zapscript, reader_id);
    }
}

#[cfg(test)]
mod tests {
    use super::blank_read;
    use serde_json::json;

    #[test]
    fn only_a_blank_token_a_reader_read_is_offered() {
        let blank = json!({"source": "reader", "category": "blank", "readerId": "pn532-1"});
        assert_eq!(blank_read(&blank).as_deref(), Some("pn532-1"));
        let other = json!({"source": "reader", "category": "media_not_found", "readerId": "x"});
        assert_eq!(blank_read(&other), None);
        let api = json!({"source": "api", "category": "blank"});
        assert_eq!(blank_read(&api), None);
    }
}
