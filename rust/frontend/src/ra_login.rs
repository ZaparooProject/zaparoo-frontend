// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// The RetroAchievements login screen: the username, then the password, on
// the full keyboard. The login is checked with RetroAchievements before
// anything is written, so a mistyped password shows here instead of
// failing silently once a game starts; only then do both go into
// `retroachievements.cfg` (`crate::retro_achievements`).
//
// The one screen that is not restored after a kill, on purpose: nothing
// typed here is ever written to the state file, and the persisted screen
// stays Settings while it is up, so a relaunch comes back to Settings.

use slint::ComponentHandle;
use zaparoo_app::keyboard::{Edge, Key, Layer, Layers, Shift, PASSWORD_REVEAL_MS};
use zaparoo_app::retro_achievements::{self as rules, LoginOutcome, FIELD_MAX_CHARS};
use zaparoo_core::input_actions::actions;

use crate::keyboard::{Flash, Keyboard, Press};
use crate::router::{lock, Ctx};
use crate::{App, RaLoginInput, RaLoginStatus, RaLoginStep, RaLoginView};

#[cfg(not(feature = "hosted"))]
const LOGIN_URL: &str = "https://retroachievements.org/dorequest.php";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
enum Step {
    #[default]
    Username,
    Password,
}

pub struct Model {
    step: Step,
    username: Keyboard,
    password: Keyboard,
    flash: Flash,
    status: RaLoginStatus,
    /// Retires a login answer the user has backed out of.
    ticket: u64,
}

impl Default for Model {
    fn default() -> Self {
        Self {
            step: Step::Username,
            username: Keyboard::new(Layers::Full, FIELD_MAX_CHARS),
            password: Keyboard::masked(Layers::Full, FIELD_MAX_CHARS),
            flash: Flash::default(),
            status: RaLoginStatus::Idle,
            ticket: 0,
        }
    }
}

impl std::fmt::Debug for Model {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Model")
            .field("step", &self.step)
            .field("status", &self.status)
            .finish_non_exhaustive()
    }
}

impl Model {
    fn keyboard(&self) -> &Keyboard {
        match self.step {
            Step::Username => &self.username,
            Step::Password => &self.password,
        }
    }

    fn keyboard_mut(&mut self) -> &mut Keyboard {
        match self.step {
            Step::Username => &mut self.username,
            Step::Password => &mut self.password,
        }
    }

    fn verifying(&self) -> bool {
        self.status == RaLoginStatus::Verifying
    }

    fn can_submit(&self) -> bool {
        !self.verifying() && !self.keyboard().text().trim().is_empty()
    }
}

/// Paint only the field: the password's reveal running out changes nothing
/// else, and on the software renderer the keys are the expensive part.
fn render_field(ctx: &Ctx, app: &App) {
    let shared = lock(&ctx.shared);
    let (before, at, after) = shared.ra_login.keyboard().display();
    let view = app.global::<RaLoginView>();
    view.set_before(before);
    view.set_at(at);
    view.set_after(after);
}

pub(crate) fn render(ctx: &Ctx, app: &App) {
    render_field(ctx, app);
    let shared = lock(&ctx.shared);
    let model = &shared.ra_login;
    let keyboard = model.keyboard();
    let view = app.global::<RaLoginView>();
    crate::view_model::publish(&view.get_keys(), keyboard.cells(), |rows| {
        view.set_keys(rows);
    });
    view.set_key_rows(i32::try_from(keyboard.row_count()).unwrap_or(5));
    view.set_shift_active(keyboard.layer() == Layer::Upper);
    view.set_shift_locked(keyboard.shift() == Shift::Locked);
    view.set_symbols_active(keyboard.layer() == Layer::Symbols);
    view.set_key_index(i32::try_from(keyboard.index()).unwrap_or(0));
    view.set_key_kind(keyboard.key_kind());
    view.set_pressed_key(model.flash.index());
    view.set_step(match model.step {
        Step::Username => RaLoginStep::Username,
        Step::Password => RaLoginStep::Password,
    });
    view.set_status(model.status);
    view.set_can_submit(model.can_submit());
}

/// Log in… on Settings > Library: start on the username, filled in with the
/// one already saved.
pub(crate) fn open(ctx: &Ctx, app: &App) {
    {
        let mut shared = lock(&ctx.shared);
        let username = shared.ra.cfg.username.clone();
        let mut model = Model::default();
        model.username.set_text(&username);
        model.ticket = shared.ra_login.ticket.wrapping_add(1);
        shared.ra_login = model;
    }
    render(ctx, app);
    crate::router::transition_to_screen(app, crate::Screen::RaLogin, 1);
}

/// Back to Settings on the row that fits: Log out after a login, Log in
/// otherwise. The password goes with the screen.
fn leave(ctx: &Ctx, app: &App) {
    let row = {
        let mut shared = lock(&ctx.shared);
        let ticket = shared.ra_login.ticket.wrapping_add(1);
        shared.ra_login = Model {
            ticket,
            ..Model::default()
        };
        if shared.ra.cfg.logged_in() {
            "raLogOut"
        } else {
            "raLogIn"
        }
    };
    crate::settings::show_return(ctx, app, crate::SettingsPage::Library, row);
}

fn submit(ctx: &Ctx, app: &App) {
    let step = {
        let shared = lock(&ctx.shared);
        if !shared.ra_login.can_submit() {
            drop(shared);
            render(ctx, app);
            return;
        }
        shared.ra_login.step
    };
    match step {
        Step::Username => {
            {
                let mut shared = lock(&ctx.shared);
                let model = &mut shared.ra_login;
                let trimmed = model.username.text().trim().to_string();
                model.username.set_text(&trimmed);
                model.step = Step::Password;
                model.status = RaLoginStatus::Idle;
                model.password.focus(Key::Char('a'));
            }
            render(ctx, app);
        }
        Step::Password => verify(ctx, app),
    }
}

fn verify(ctx: &Ctx, app: &App) {
    let (username, password, ticket) = {
        let mut shared = lock(&ctx.shared);
        let model = &mut shared.ra_login;
        let username = model.username.text().to_string();
        let password = model.password.text().to_string();
        if !rules::value_supported(&username) || !rules::value_supported(&password) {
            model.status = RaLoginStatus::Unsupported;
            drop(shared);
            render(ctx, app);
            return;
        }
        model.status = RaLoginStatus::Verifying;
        model.ticket = model.ticket.wrapping_add(1);
        (username, password, model.ticket)
    };
    render(ctx, app);
    let ctx2 = ctx.clone();
    let weak = app.as_weak();
    let form = rules::login_form(&username, &password);
    ctx.handle.spawn(async move {
        let outcome = tokio::task::spawn_blocking(move || check(&form))
            .await
            .unwrap_or(LoginOutcome::Unreachable);
        let _ = weak.upgrade_in_event_loop(move |app| {
            answered(&ctx2, &app, ticket, outcome, password);
        });
    });
}

/// Ask `RetroAchievements` whether the login is good. A refusal comes back
/// as JSON with an error status, so the body is read either way.
#[cfg(not(feature = "hosted"))]
fn check(form: &str) -> LoginOutcome {
    #[derive(serde::Deserialize)]
    struct Login2 {
        #[serde(rename = "Success")]
        success: Option<bool>,
        #[serde(rename = "User")]
        user: Option<String>,
        #[serde(rename = "Code")]
        code: Option<String>,
    }
    let agent = format!("ZaparooFrontend/{}", env!("CARGO_PKG_VERSION"));
    let body = crate::curl::post(&crate::curl::Post {
        url: LOGIN_URL,
        content_type: "application/x-www-form-urlencoded",
        body: form.as_bytes(),
        timeout: std::time::Duration::from_secs(20),
        fail_with_body: false,
        user_agent: Some(&agent),
    });
    match body {
        Ok(body) => match serde_json::from_str::<Login2>(&body) {
            Ok(answer) => rules::login_outcome(
                answer.success,
                answer.user.as_deref(),
                answer.code.as_deref(),
            ),
            Err(e) => {
                tracing::warn!("RetroAchievements login answer was not readable: {e}");
                LoginOutcome::Unreachable
            }
        },
        Err(e) => {
            tracing::warn!("RetroAchievements login request failed: {e}");
            LoginOutcome::Unreachable
        }
    }
}

#[cfg(feature = "hosted")]
fn check(_form: &str) -> LoginOutcome {
    LoginOutcome::Unreachable
}

/// The answer is for the login still waiting on it.
fn current(ctx: &Ctx, ticket: u64) -> bool {
    let shared = lock(&ctx.shared);
    shared.ra_login.ticket == ticket && shared.ra_login.verifying()
}

fn answered(ctx: &Ctx, app: &App, ticket: u64, outcome: LoginOutcome, password: String) {
    if !current(ctx, ticket)
        || app.global::<crate::Shell>().get_active_screen() != crate::Screen::RaLogin
    {
        return;
    }
    let status = match outcome {
        LoginOutcome::LoggedIn(username) => {
            crate::retro_achievements::save_login(
                ctx,
                app,
                username,
                password,
                move |ctx, app, saved| {
                    if !current(ctx, ticket) {
                        return;
                    }
                    if saved {
                        leave(ctx, app);
                    } else {
                        lock(&ctx.shared).ra_login.status = RaLoginStatus::SaveFailed;
                        render(ctx, app);
                    }
                },
            );
            return;
        }
        LoginOutcome::WrongCredentials => RaLoginStatus::WrongCredentials,
        LoginOutcome::Refused => RaLoginStatus::Refused,
        LoginOutcome::Unreachable => RaLoginStatus::Unreachable,
    };
    lock(&ctx.shared).ra_login.status = status;
    render(ctx, app);
}

fn back(ctx: &Ctx, app: &App) {
    let step = {
        let mut shared = lock(&ctx.shared);
        let model = &mut shared.ra_login;
        if model.verifying() {
            model.ticket = model.ticket.wrapping_add(1);
            model.status = RaLoginStatus::Idle;
            None
        } else {
            Some(model.step)
        }
    };
    match step {
        None => render(ctx, app),
        Some(Step::Password) => {
            {
                let mut shared = lock(&ctx.shared);
                let model = &mut shared.ra_login;
                model.password.clear();
                model.step = Step::Username;
                model.status = RaLoginStatus::Idle;
            }
            render(ctx, app);
        }
        Some(Step::Username) => leave(ctx, app),
    }
}

/// After an edit: a password that just showed a character hides it again
/// once the reveal time runs out.
fn arm_reveal(ctx: &Ctx, app: &App) {
    let Some(ticket) = lock(&ctx.shared).ra_login.keyboard().reveal_ticket() else {
        return;
    };
    let ctx = ctx.clone();
    let weak = app.as_weak();
    slint::Timer::single_shot(
        std::time::Duration::from_millis(PASSWORD_REVEAL_MS),
        move || {
            let Some(app) = weak.upgrade() else {
                return;
            };
            let hid = lock(&ctx.shared)
                .ra_login
                .keyboard_mut()
                .expire_reveal(ticket);
            if hid {
                render_field(&ctx, &app);
            }
        },
    );
}

fn apply_press(ctx: &Ctx, app: &App, press: Press) {
    match press {
        Press::Edited => {
            {
                let mut shared = lock(&ctx.shared);
                let model = &mut shared.ra_login;
                if !model.verifying() {
                    model.status = RaLoginStatus::Idle;
                }
            }
            render(ctx, app);
            arm_reveal(ctx, app);
        }
        Press::Moved | Press::Layer | Press::None => render(ctx, app),
        Press::Submit => submit(ctx, app),
    }
}

fn flash_key(ctx: &Ctx, app: &App) {
    let index = lock(&ctx.shared).ra_login.keyboard().index();
    crate::input::flash_key(
        ctx,
        app,
        index,
        |shared| &mut shared.ra_login.flash,
        |app| app.global::<RaLoginView>().set_pressed_key(-1),
    );
}

/// A character from a physical keyboard. Focus moves to the submit key so
/// Enter goes on.
pub(crate) fn type_char(ctx: &Ctx, app: &App, c: char) {
    let press = {
        let mut shared = lock(&ctx.shared);
        if shared.ra_login.verifying() {
            return;
        }
        let keyboard = shared.ra_login.keyboard_mut();
        let press = keyboard.insert(c);
        keyboard.focus(Key::Submit);
        press
    };
    apply_press(ctx, app, press);
}

pub(crate) fn on_shift(ctx: &Ctx) -> bool {
    lock(&ctx.shared).ra_login.keyboard().key() == Some(Key::Shift)
}

pub(crate) fn lock_shift(ctx: &Ctx, app: &App) {
    lock(&ctx.shared).ra_login.keyboard_mut().lock_shift();
    render(ctx, app);
}

pub(crate) fn key_repeats(ctx: &Ctx) -> bool {
    lock(&ctx.shared).ra_login.keyboard().key_repeats()
}

pub fn handle_action(ctx: &Ctx, app: &App, action: &str) {
    if action == actions::CANCEL {
        back(ctx, app);
        return;
    }
    // While RetroAchievements is answering, only Back does anything.
    if lock(&ctx.shared).ra_login.verifying() {
        return;
    }
    match action {
        actions::UP | actions::DOWN | actions::LEFT | actions::RIGHT => {
            {
                let mut shared = lock(&ctx.shared);
                let keyboard = shared.ra_login.keyboard_mut();
                match keyboard.step(action, true) {
                    Some(Edge::Up) => keyboard.enter_row(true),
                    Some(Edge::Down) => keyboard.enter_row(false),
                    Some(Edge::Left | Edge::Right) | None => {}
                }
            }
            render(ctx, app);
        }
        actions::ACCEPT => {
            let press = lock(&ctx.shared).ra_login.keyboard_mut().press();
            flash_key(ctx, app);
            apply_press(ctx, app, press);
        }
        _ => {
            if action == zaparoo_app::input::TEXT_KEY {
                flash_key(ctx, app);
            }
            let held = crate::input::repeat_held_ms(ctx);
            let press = lock(&ctx.shared)
                .ra_login
                .keyboard_mut()
                .text_action(action, held);
            if let Some(press) = press {
                apply_press(ctx, app, press);
            }
        }
    }
}

fn pointer_allowed(ctx: &Ctx, app: &App) -> bool {
    crate::input::text_owner(app) == Some(crate::input::TextOwner::RaLogin)
        && lock(&ctx.shared).persist.settings.mouse_enabled
}

/// Wire the view's pointer callbacks.
pub fn bind(ctx: &Ctx, app: &App) {
    let input = app.global::<RaLoginInput>();
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
            if pointer_allowed(&ctx, &app)
                && lock(&ctx.shared).ra_login.keyboard_mut().set_index(index)
            {
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
        if !pointer_allowed(&ctx, &app)
            || !lock(&ctx.shared).ra_login.keyboard_mut().set_index(index)
        {
            return;
        }
        handle_action(&ctx, &app, actions::ACCEPT);
    });
}
