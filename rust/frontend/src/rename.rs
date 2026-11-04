// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// The rename screen driver: a Hub tile's name typed on the full keyboard.
// Save writes the name to the tile's `[[hub.items]]` entry and returns to
// the Hub on that tile; Back returns without saving. The draft is kept in
// `persist.rename`, so a kill mid-edit comes back to the same text. Rules
// live in `zaparoo_app::text_entry`.

use slint::ComponentHandle;
use zaparoo_app::keyboard::{Edge, Key, Layer, Layers, Shift};
use zaparoo_app::text_entry::{self as rules, TILE_NAME_MAX_CHARS};
use zaparoo_core::input_actions::actions;
use zaparoo_core::persist::{HubTarget, RenameState};

use crate::keyboard::{Flash, Keyboard, Press};
use crate::router::{lock, Ctx};
use crate::{App, RenameInput, RenameView};

#[derive(Debug)]
pub struct Model {
    keyboard: Keyboard,
    flash: Flash,
    /// The tile's own name, which an empty or unchanged field keeps.
    default_name: String,
    /// The tile's kind has a name of its own to fall back to; a script
    /// tile without one would show its script.
    has_default: bool,
}

impl Default for Model {
    fn default() -> Self {
        Self {
            keyboard: Keyboard::new(Layers::Full, TILE_NAME_MAX_CHARS),
            flash: Flash::default(),
            default_name: String::new(),
            has_default: true,
        }
    }
}

fn can_save(model: &Model) -> bool {
    model.has_default || !model.keyboard.text().trim().is_empty()
}

pub(crate) fn render(ctx: &Ctx, app: &App) {
    let shared = lock(&ctx.shared);
    let model = &shared.rename;
    let view = app.global::<RenameView>();
    let (before, at, after) = model.keyboard.display();
    view.set_before(before);
    view.set_at(at);
    view.set_after(after);
    crate::view_model::publish(&view.get_keys(), model.keyboard.cells(), |rows| {
        view.set_keys(rows);
    });
    view.set_key_rows(i32::try_from(model.keyboard.row_count()).unwrap_or(5));
    view.set_shift_active(model.keyboard.layer() == Layer::Upper);
    view.set_shift_locked(model.keyboard.shift() == Shift::Locked);
    view.set_symbols_active(model.keyboard.layer() == Layer::Symbols);
    view.set_key_index(i32::try_from(model.keyboard.index()).unwrap_or(0));
    view.set_key_kind(model.keyboard.key_kind());
    view.set_pressed_key(model.flash.index());
    view.set_can_save(can_save(model));
}

/// Keep the draft on disk, so a kill mid-edit restores it.
fn persist_draft(ctx: &Ctx) {
    {
        let mut shared = lock(&ctx.shared);
        let draft = shared.rename.keyboard.text().to_string();
        let caret = shared.rename.keyboard.caret();
        shared.persist.rename.draft = draft;
        shared.persist.rename.caret = caret;
    }
    crate::router::save_persist(&ctx.shared);
}

fn show(ctx: &Ctx, app: &App) {
    lock(&ctx.shared).persist.active_screen = crate::Screen::Rename.token().to_string();
    crate::router::save_persist(&ctx.shared);
    render(ctx, app);
    crate::router::transition_to_screen(app, crate::Screen::Rename, 1);
}

/// Rename tile… on a Hub tile's Options menu.
pub(crate) fn open(ctx: &Ctx, app: &App, hub_index: i32) {
    let Some(tile) = crate::hub::edit_target(ctx, app, hub_index) else {
        return;
    };
    {
        let mut shared = lock(&ctx.shared);
        let mut model = Model {
            default_name: tile.default_name.clone(),
            has_default: tile.has_default,
            ..Model::default()
        };
        let initial = if tile.custom_name.is_empty() {
            &tile.default_name
        } else {
            &tile.custom_name
        };
        model.keyboard.set_text(initial);
        shared.rename = model;
        shared.persist.rename = RenameState {
            target: tile.target,
            draft: shared.rename.keyboard.text().to_string(),
            caret: shared.rename.keyboard.caret(),
        };
    }
    show(ctx, app);
}

/// Cold start on the rename screen: the same tile and draft, or the Hub
/// when the tile is no longer where it was.
pub(crate) fn restore(ctx: &Ctx, app: &App) {
    let saved = lock(&ctx.shared).persist.rename.clone();
    let Some(tile) = crate::hub::edit_target_matching(ctx, app, &saved.target) else {
        lock(&ctx.shared).persist.active_screen = crate::Screen::Hub.token().to_string();
        crate::router::save_persist(&ctx.shared);
        return;
    };
    {
        let mut shared = lock(&ctx.shared);
        let mut model = Model {
            default_name: tile.default_name,
            has_default: tile.has_default,
            ..Model::default()
        };
        model.keyboard.set_text(&saved.draft);
        model.keyboard.set_caret(saved.caret);
        shared.rename = model;
    }
    show(ctx, app);
}

/// Back to the Hub on the tile, saved or not.
fn leave(ctx: &Ctx, app: &App) {
    let target = {
        let mut shared = lock(&ctx.shared);
        shared.persist.active_screen = crate::Screen::Hub.token().to_string();
        std::mem::take(&mut shared.persist.rename).target
    };
    crate::router::save_persist(&ctx.shared);
    crate::hub::return_to_tile(ctx, app, &target);
}

fn save(ctx: &Ctx, app: &App) {
    let (target, name) = {
        let shared = lock(&ctx.shared);
        if !can_save(&shared.rename) {
            drop(shared);
            render(ctx, app);
            return;
        }
        (
            shared.persist.rename.target.clone(),
            rules::tile_name(shared.rename.keyboard.text(), &shared.rename.default_name),
        )
    };
    crate::hub::set_tile_name(ctx, app, &target, &name);
    crate::router::forget_menu_return(ctx);
    leave(ctx, app);
}

fn apply_press(ctx: &Ctx, app: &App, press: Press) {
    match press {
        Press::Edited | Press::Moved => {
            persist_draft(ctx);
            render(ctx, app);
        }
        Press::Layer | Press::None => render(ctx, app),
        Press::Submit => save(ctx, app),
    }
}

fn flash_key(ctx: &Ctx, app: &App) {
    let index = lock(&ctx.shared).rename.keyboard.index();
    crate::input::flash_key(
        ctx,
        app,
        index,
        |shared| &mut shared.rename.flash,
        |app| app.global::<RenameView>().set_pressed_key(-1),
    );
}

/// A character from a physical keyboard. Focus moves to the Save key so
/// Enter saves.
pub(crate) fn type_char(ctx: &Ctx, app: &App, c: char) {
    let press = {
        let mut shared = lock(&ctx.shared);
        let press = shared.rename.keyboard.insert(c);
        shared.rename.keyboard.focus(Key::Submit);
        press
    };
    apply_press(ctx, app, press);
}

/// Focus is on the Shift key.
pub(crate) fn on_shift(ctx: &Ctx) -> bool {
    lock(&ctx.shared).rename.keyboard.key() == Some(Key::Shift)
}

/// Seat focus on a key, for route tests.
#[cfg(all(test, feature = "mister"))]
pub(crate) fn focus_key_for_test(ctx: &Ctx, key: Key) {
    lock(&ctx.shared).rename.keyboard.focus(key);
}

/// Shift held down: caps until it is tapped again.
pub(crate) fn lock_shift(ctx: &Ctx, app: &App) {
    lock(&ctx.shared).rename.keyboard.lock_shift();
    render(ctx, app);
}

/// Whether Accept on the focused key repeats while held.
pub(crate) fn key_repeats(ctx: &Ctx) -> bool {
    lock(&ctx.shared).rename.keyboard.key_repeats()
}

pub fn handle_action(ctx: &Ctx, app: &App, action: &str) {
    match action {
        actions::UP | actions::DOWN | actions::LEFT | actions::RIGHT => {
            {
                let mut shared = lock(&ctx.shared);
                let keyboard = &mut shared.rename.keyboard;
                // With nothing beside the keyboard, every edge wraps.
                match keyboard.step(action, true) {
                    Some(Edge::Up) => keyboard.enter_row(true),
                    Some(Edge::Down) => keyboard.enter_row(false),
                    Some(Edge::Left | Edge::Right) | None => {}
                }
            }
            render(ctx, app);
        }
        actions::ACCEPT => {
            let press = lock(&ctx.shared).rename.keyboard.press();
            flash_key(ctx, app);
            apply_press(ctx, app, press);
        }
        actions::CANCEL => {
            leave(ctx, app);
            crate::router::return_to_menu(ctx, app, zaparoo_app::options_menu::Opens::Rename);
        }
        _ => {
            if action == zaparoo_app::input::TEXT_KEY {
                flash_key(ctx, app);
            }
            let held = crate::input::repeat_held_ms(ctx);
            let press = lock(&ctx.shared).rename.keyboard.text_action(action, held);
            if let Some(press) = press {
                apply_press(ctx, app, press);
            }
        }
    }
}

fn pointer_allowed(ctx: &Ctx, app: &App) -> bool {
    crate::input::text_owner(app) == Some(crate::input::TextOwner::Rename)
        && lock(&ctx.shared).persist.settings.mouse_enabled
}

/// Wire the view's pointer callbacks.
pub fn bind(ctx: &Ctx, app: &App) {
    let input = app.global::<RenameInput>();
    {
        let ctx = ctx.clone();
        let weak = app.as_weak();
        input.on_hovered(move |index| {
            let Some(app) = weak.upgrade() else {
                return;
            };
            let Ok(index) = usize::try_from(index) else {
                return;
            };
            if pointer_allowed(&ctx, &app) && lock(&ctx.shared).rename.keyboard.set_index(index) {
                render(&ctx, &app);
            }
        });
    }
    let ctx = ctx.clone();
    let weak = app.as_weak();
    input.on_clicked(move |index| {
        let Some(app) = weak.upgrade() else {
            return;
        };
        let Ok(index) = usize::try_from(index) else {
            return;
        };
        if !pointer_allowed(&ctx, &app) || !lock(&ctx.shared).rename.keyboard.set_index(index) {
            return;
        }
        handle_action(&ctx, &app, actions::ACCEPT);
    });
}

/// The tile a rename acts on, as the persisted state records it.
pub(crate) fn target(hub_index: i32, identity: &str) -> HubTarget {
    HubTarget {
        hub_index: u32::try_from(hub_index).unwrap_or(0),
        identity: identity.to_string(),
    }
}
