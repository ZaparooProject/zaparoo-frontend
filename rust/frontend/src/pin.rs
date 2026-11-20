// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// The PIN panel and the two flows that use it: the admin gate in front of
// the settings that change Core for everyone, and switching to a profile
// that has a PIN. Core checks every PIN (`profiles.verify`,
// `profiles.switch`); the frontend never holds one past the request.
// While a reader is connected, a switch card can stand in for the PIN
// (`profiles.verify` with `scan`), which Core ends on
// `profiles.verify.cancel` when the panel closes.

use std::cell::RefCell;
use std::sync::Arc;

use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use zaparoo_app::pin as rules;
use zaparoo_core::input_actions::actions;
use zaparoo_core::profile_types::{
    Profile, SwitchProfileParams, VerifiedProfile, VerifyProfileParams,
};

use crate::router::{lock, Ctx};
use crate::{App, PinError, PinPage, PinView};

/// What runs once the gate opens. It stays on the event loop thread.
type Continuation = Box<dyn FnOnce(&Ctx, &App)>;

/// What a correct PIN unlocks.
enum Purpose {
    /// The admin gate: run this once an admin's PIN checks out.
    Gate(Continuation),
    /// Switch to this profile.
    Switch(String),
    /// Give the profile being typed for a new PIN.
    SetPin,
    /// Create the first profile, which Core makes an admin and so needs
    /// a PIN. The name is in `Model::profile`, with no id yet.
    NewAdmin,
}

#[derive(Default)]
struct Model {
    purpose: Option<Purpose>,
    /// The profile whose PIN is being typed, and its name.
    profile: Option<(String, String)>,
    /// Admins to pick from: id and name.
    admins: Vec<(String, String)>,
    admin_index: usize,
    pin: String,
    focus: usize,
    /// Bumped on every open and close, so a late answer for a panel that
    /// has gone is dropped.
    ticket: u64,
    /// A PIN check is in flight: the pad waits for it.
    checking: bool,
    /// Bumped by every refusal, open and close, so a message's timeout
    /// clears only the message it was set for.
    message: u64,
    /// The scan wait, aborted when the panel closes.
    scan: Option<tokio::task::JoinHandle<()>>,
}

thread_local! {
    static MODEL: RefCell<Model> = RefCell::new(Model::default());
}

fn with<T>(f: impl FnOnce(&mut Model) -> T) -> T {
    MODEL.with(|m| f(&mut m.borrow_mut()))
}

/// The open panel's ticket, for tests that answer a request themselves.
#[cfg(all(test, feature = "mister"))]
pub(crate) fn ticket_for_test() -> u64 {
    with(|m| m.ticket)
}

pub(crate) fn is_open(app: &App) -> bool {
    app.global::<PinView>().get_open()
}

/// Run `then` behind the admin gate: at once when no admin PIN is needed
/// (no profiles, no admins, or already unlocked), otherwise once an
/// admin's PIN or switch card checks out. Core's profile list is fetched
/// first, so a profile made elsewhere a moment ago counts.
pub(crate) fn gate(ctx: &Ctx, app: &App, then: impl FnOnce(&Ctx, &App) + 'static) {
    if lock(&ctx.shared).core.unlocked {
        then(ctx, app);
        return;
    }
    let client = ctx.store.client();
    let ctx2 = ctx.clone();
    let weak = app.as_weak();
    // The continuation stays on this thread; only the answer crosses.
    let then: Continuation = Box::new(then);
    let ticket = with(|m| {
        m.ticket += 1;
        m.ticket
    });
    PENDING.with(|p| *p.borrow_mut() = Some((ticket, then)));
    ctx.handle.spawn(async move {
        let result = client.profiles().await;
        let _ = weak.upgrade_in_event_loop(move |app| {
            // Only this request's own continuation: a newer gate keeps
            // its own, and one called off has none left.
            let Some(then) = PENDING.with(|p| {
                let mut p = p.borrow_mut();
                match p.as_ref() {
                    Some((pending, _)) if *pending == ticket => p.take().map(|(_, then)| then),
                    _ => None,
                }
            }) else {
                return;
            };
            match result {
                Ok(list) => {
                    let profiles = list.profiles;
                    let admins: Vec<(String, String)> = profiles
                        .iter()
                        .filter(|p| p.is_admin())
                        .map(|p| (p.profile_id.clone(), p.name.clone()))
                        .collect();
                    let required = {
                        let mut shared = lock(&ctx2.shared);
                        shared.core.profiles = Some(profiles.clone());
                        zaparoo_app::settings::admin_required(
                            profiles.len(),
                            admins.len(),
                            shared.core.unlocked,
                        )
                    };
                    if required {
                        open_gate(&ctx2, &app, admins, then);
                    } else {
                        then(&ctx2, &app);
                    }
                }
                Err(error) => {
                    tracing::warn!("profiles failed: {}", error.message);
                    crate::router::report_action_error(&ctx2, &app, "setting", "");
                }
            }
        });
    });
}

/// Settings went away or back to its hub: a gate still waiting for
/// Core's profile list opens nothing and runs nothing when it answers.
pub(crate) fn cancel_pending() {
    PENDING.with(|p| p.borrow_mut().take());
}

thread_local! {
    /// The gate's continuation while Core's profile list is fetched.
    static PENDING: RefCell<Option<(u64, Continuation)>> = const { RefCell::new(None) };
}

fn open_gate(ctx: &Ctx, app: &App, admins: Vec<(String, String)>, then: Continuation) {
    let profile = (admins.len() == 1).then(|| admins[0].clone());
    with(|m| {
        m.purpose = Some(Purpose::Gate(then));
        m.admins = admins;
        m.admin_index = 0;
        m.profile = profile;
    });
    open(ctx, app);
}

/// Switch to a profile: at once when it has no PIN, otherwise once its
/// PIN or its own switch card checks out. Leaving a profile is `leave`.
pub(crate) fn switch_to(ctx: &Ctx, app: &App, profile: &Profile) {
    if !profile.has_pin {
        request_switch(ctx, app, &profile.profile_id, None, None);
        return;
    }
    with(|m| {
        m.purpose = Some(Purpose::Switch(profile.profile_id.clone()));
        m.admins.clear();
        m.admin_index = 0;
        m.profile = Some((profile.profile_id.clone(), profile.name.clone()));
    });
    open(ctx, app);
}

/// Type a new PIN for a profile. The gate already let the admin in; Core
/// keeps the PIN, and the panel closes once it has.
pub(crate) fn set_pin(ctx: &Ctx, app: &App, profile: &Profile) {
    with(|m| {
        m.purpose = Some(Purpose::SetPin);
        m.admins.clear();
        m.admin_index = 0;
        m.profile = Some((profile.profile_id.clone(), profile.name.clone()));
    });
    app.global::<PinView>().set_setting(true);
    open(ctx, app);
}

/// Type the PIN for the first profile, named `name`. Core makes the first
/// profile an admin, and an admin needs a PIN, so nothing is created until
/// it is typed; leaving the panel creates nothing.
pub(crate) fn new_admin(ctx: &Ctx, app: &App, name: String) {
    with(|m| {
        m.purpose = Some(Purpose::NewAdmin);
        m.admins.clear();
        m.admin_index = 0;
        m.profile = Some((String::new(), name));
    });
    let view = app.global::<PinView>();
    view.set_setting(true);
    view.set_first_admin(true);
    open(ctx, app);
}

fn setting(m: &Model) -> bool {
    matches!(m.purpose, Some(Purpose::SetPin | Purpose::NewAdmin))
}

/// Leave the active profile for the shared one. Never asks for a PIN.
pub(crate) fn leave(ctx: &Ctx, app: &App) {
    let client = ctx.store.client();
    let ctx2 = ctx.clone();
    let weak = app.as_weak();
    ctx.handle.spawn(async move {
        let result = client.profiles_switch(SwitchProfileParams::default()).await;
        if let Err(error) = result {
            tracing::warn!("leaving the profile failed: {}", error.message);
            let _ = weak.upgrade_in_event_loop(move |app| {
                crate::router::report_action_error(&ctx2, &app, "setting", "");
            });
        }
    });
}

fn request_switch(
    ctx: &Ctx,
    app: &App,
    profile_id: &str,
    pin: Option<String>,
    switch_id: Option<String>,
) {
    let client = ctx.store.client();
    let ctx2 = ctx.clone();
    let weak = app.as_weak();
    let ticket = with(|m| m.ticket);
    let params = SwitchProfileParams {
        profile_id: Some(profile_id.to_string()),
        pin,
        switch_id,
    };
    ctx.handle.spawn(async move {
        let result = client.profiles_switch(params).await;
        let _ = weak.upgrade_in_event_loop(move |app| match result {
            Ok(()) => {
                if is_open(&app) && with(|m| m.ticket) == ticket {
                    close(&ctx2, &app);
                }
            }
            Err(error) => {
                tracing::warn!("switching profile failed: {}", error.message);
                if is_open(&app) && with(|m| m.ticket) == ticket {
                    refused(&app, PinError::Failed, &error.message);
                } else {
                    crate::router::report_action_error(&ctx2, &app, "setting", "");
                }
            }
        });
    });
}

fn open(ctx: &Ctx, app: &App) {
    with(|m| {
        m.ticket += 1;
        m.message += 1;
        m.pin.clear();
        m.focus = rules::START;
        m.checking = false;
    });
    let setting = with(|m| setting(m));
    let can_scan = !setting && lock(&ctx.shared).readers.iter().any(|r| r.connected);
    let unlocking = with(|m| matches!(m.purpose, Some(Purpose::Gate(_))));
    let view = app.global::<PinView>();
    view.set_unlocking(unlocking);
    view.set_can_scan(can_scan);
    view.set_error(PinError::None);
    view.set_error_text(SharedString::default());
    render(app);
    view.set_open(true);
    if can_scan {
        start_scan(ctx, app);
    }
}

/// Wait for a switch card. Core answers with the profile it belongs to.
fn start_scan(ctx: &Ctx, app: &App) {
    let client = ctx.store.client();
    let ctx2 = ctx.clone();
    let weak = app.as_weak();
    let ticket = with(|m| m.ticket);
    let task = ctx.handle.spawn(async move {
        let result = client
            .profiles_verify(VerifyProfileParams {
                scan: true,
                ..VerifyProfileParams::default()
            })
            .await;
        let _ = weak.upgrade_in_event_loop(move |app| {
            if !is_open(&app) || with(|m| m.ticket) != ticket {
                return;
            }
            match result {
                Ok(verified) => scanned(&ctx2, &app, &verified),
                Err(error) => {
                    // A wait that ended without a card (a timeout, a card
                    // that is not a switch card) leaves the pad as it is.
                    tracing::debug!("switch card wait ended: {}", error.message);
                }
            }
        });
    });
    with(|m| m.scan = Some(task));
}

fn scanned(ctx: &Ctx, app: &App, verified: &VerifiedProfile) {
    let purpose = with(|m| match &m.purpose {
        Some(Purpose::Gate(_)) => Some(None),
        Some(Purpose::Switch(id)) => Some(Some(id.clone())),
        // A new PIN is typed, never scanned.
        Some(Purpose::SetPin | Purpose::NewAdmin) | None => None,
    });
    match purpose {
        Some(None) => {
            if zaparoo_app::settings::verified_admin(&verified.role) {
                unlocked(ctx, app);
            } else {
                refused(app, PinError::NotAdmin, "");
                start_scan(ctx, app);
            }
        }
        Some(Some(target)) => {
            if verified.profile_id == target {
                let switch_id = lock(&ctx.shared)
                    .core
                    .profiles
                    .as_ref()
                    .and_then(|list| list.iter().find(|p| p.profile_id == target))
                    .map(|p| p.switch_id.clone());
                request_switch(ctx, app, &target, None, switch_id);
            } else {
                refused(app, PinError::Failed, "");
                start_scan(ctx, app);
            }
        }
        None => {}
    }
}

fn unlocked(ctx: &Ctx, app: &App) {
    lock(&ctx.shared).core.unlocked = true;
    let then = with(|m| match m.purpose.take() {
        Some(Purpose::Gate(then)) => Some(then),
        other => {
            m.purpose = other;
            None
        }
    });
    close(ctx, app);
    if let Some(then) = then {
        then(ctx, app);
    }
}

/// Core kept the new PIN, or refused it.
pub(crate) fn pin_saved(ctx: &Ctx, app: &App, ticket: u64, error: Option<String>) {
    if !is_open(app) || with(|m| m.ticket) != ticket {
        return;
    }
    match error {
        None => close(ctx, app),
        Some(text) => refused(app, PinError::Failed, &text),
    }
}

fn refused(app: &App, error: PinError, text: &str) {
    let message = with(|m| {
        m.pin.clear();
        m.checking = false;
        m.message += 1;
        m.message
    });
    let view = app.global::<PinView>();
    view.set_error(error);
    view.set_error_text(SharedString::from(text));
    render(app);
    // The message gives the caption line back to the standing hint.
    let weak = app.as_weak();
    slint::Timer::single_shot(
        std::time::Duration::from_millis(rules::MESSAGE_MS),
        move || {
            let Some(app) = weak.upgrade() else { return };
            if with(|m| m.message) == message {
                clear_message(&app);
            }
        },
    );
}

fn clear_message(app: &App) {
    let view = app.global::<PinView>();
    view.set_error(PinError::None);
    view.set_error_text(SharedString::default());
}

/// Close the panel without unlocking anything; a scan wait is called off.
pub(crate) fn close(ctx: &Ctx, app: &App) {
    let scan = with(|m| {
        m.ticket += 1;
        m.message += 1;
        m.purpose = None;
        m.pin.clear();
        m.checking = false;
        m.scan.take()
    });
    if let Some(scan) = scan {
        scan.abort();
        let client = ctx.store.client();
        ctx.handle.spawn(async move {
            if let Err(error) = client.profiles_verify_cancel().await {
                tracing::debug!("verify cancel failed: {}", error.message);
            }
        });
    }
    let view = app.global::<PinView>();
    view.set_open(false);
    view.set_setting(false);
    view.set_first_admin(false);
}

fn render(app: &App) {
    let view = app.global::<PinView>();
    let (page, name, digits, focus, admins, admin_index) = with(|m| {
        let page = if m.profile.is_none() {
            PinPage::Admins
        } else {
            PinPage::Entry
        };
        (
            page,
            m.profile.as_ref().map(|p| p.1.clone()).unwrap_or_default(),
            m.pin.len(),
            m.focus,
            m.admins
                .iter()
                .map(|a| SharedString::from(a.1.as_str()))
                .collect::<Vec<_>>(),
            m.admin_index,
        )
    });
    view.set_page(page);
    view.set_name(SharedString::from(name));
    view.set_digits(i32::try_from(digits).unwrap_or(0));
    view.set_focus(i32::try_from(focus).unwrap_or(0));
    view.set_admins(ModelRc::new(VecModel::from(admins)));
    view.set_admin_index(i32::try_from(admin_index).unwrap_or(0));
}

pub(crate) fn handle_action(ctx: &Ctx, app: &App, action: &str) {
    let on_admins = with(|m| m.profile.is_none());
    if on_admins {
        match action {
            actions::UP | actions::DOWN => {
                with(|m| {
                    let last = m.admins.len().saturating_sub(1);
                    m.admin_index = if action == actions::UP {
                        m.admin_index.saturating_sub(1)
                    } else {
                        (m.admin_index + 1).min(last)
                    };
                });
                render(app);
            }
            actions::ACCEPT => pick_admin(app),
            actions::CANCEL => close(ctx, app),
            _ => {}
        }
        return;
    }
    match action {
        actions::LEFT | actions::RIGHT | actions::UP | actions::DOWN => {
            let (dx, dy) = match action {
                actions::LEFT => (-1, 0),
                actions::RIGHT => (1, 0),
                actions::UP => (0, -1),
                _ => (0, 1),
            };
            with(|m| m.focus = rules::step(m.focus, dx, dy));
            render(app);
        }
        actions::ACCEPT => {
            let key = with(|m| rules::key_at(m.focus));
            press(ctx, app, key);
        }
        zaparoo_app::input::TEXT_DELETE => press(ctx, app, rules::Key::Delete),
        actions::CANCEL => {
            // With several admins, Back returns to the list first.
            let to_list = with(|m| {
                let to_list = m.admins.len() > 1;
                if to_list {
                    m.profile = None;
                    m.pin.clear();
                }
                to_list
            });
            if to_list {
                app.global::<PinView>().set_error(PinError::None);
                render(app);
            } else {
                close(ctx, app);
            }
        }
        _ => {}
    }
}

fn pick_admin(app: &App) {
    with(|m| {
        if let Some(admin) = m.admins.get(m.admin_index) {
            m.profile = Some(admin.clone());
            m.pin.clear();
            m.focus = rules::START;
        }
    });
    render(app);
}

/// A digit typed on a keyboard.
pub(crate) fn type_char(ctx: &Ctx, app: &App, c: char) {
    if with(|m| m.profile.is_none()) {
        return;
    }
    if let Some(key) = rules::key_for_char(c) {
        press(ctx, app, key);
    }
}

fn press(ctx: &Ctx, app: &App, key: rules::Key) {
    let outcome = with(|m| {
        if m.checking {
            return None;
        }
        Some(rules::press(&mut m.pin, key))
    });
    match outcome {
        Some(rules::Press::Submit) => submit(ctx, app),
        Some(_) => {
            app.global::<PinView>().set_error(PinError::None);
            render(app);
        }
        None => {}
    }
}

fn submit(ctx: &Ctx, app: &App) {
    let Some((profile_id, pin, ticket, switch)) = with(|m| {
        let (id, _) = m.profile.clone()?;
        m.checking = true;
        let switch = matches!(m.purpose, Some(Purpose::Switch(_)));
        Some((id, m.pin.clone(), m.ticket, switch))
    }) else {
        return;
    };
    if with(|m| matches!(m.purpose, Some(Purpose::NewAdmin))) {
        let name = with(|m| m.profile.clone().map(|(_, name)| name).unwrap_or_default());
        crate::core_settings::create_profile(ctx, app, name, Some((pin, ticket)));
        return;
    }
    if with(|m| setting(m)) {
        crate::core_settings::save_pin(ctx, app, profile_id, pin, ticket);
        return;
    }
    if switch {
        request_switch(ctx, app, &profile_id, Some(pin), None);
        return;
    }
    let client = ctx.store.client();
    let ctx2 = ctx.clone();
    let weak = app.as_weak();
    ctx.handle.spawn(async move {
        let result = client
            .profiles_verify(VerifyProfileParams {
                profile_id: Some(profile_id),
                pin: Some(pin),
                ..VerifyProfileParams::default()
            })
            .await;
        let _ = weak.upgrade_in_event_loop(move |app| {
            if !is_open(&app) || with(|m| m.ticket) != ticket {
                return;
            }
            match result {
                Ok(verified) if zaparoo_app::settings::verified_admin(&verified.role) => {
                    unlocked(&ctx2, &app);
                }
                Ok(_) => refused(&app, PinError::NotAdmin, ""),
                Err(error) => refused(&app, PinError::Failed, &error.message),
            }
        });
    });
}

/// The pointer: a pad key or an admin row clicked.
pub(crate) fn bind(ctx: &Arc<Ctx>, app: &App) {
    let view = app.global::<PinView>();
    let weak = app.as_weak();
    let c = ctx.clone();
    view.on_pointer_key(move |index| {
        let Some(app) = weak.upgrade() else { return };
        let index = usize::try_from(index).unwrap_or(0);
        with(|m| m.focus = index.min(rules::KEYS.len() - 1));
        press(&c, &app, rules::key_at(index));
    });
    let weak = app.as_weak();
    view.on_pointer_admin(move |index| {
        let Some(app) = weak.upgrade() else { return };
        with(|m| m.admin_index = usize::try_from(index).unwrap_or(0));
        pick_admin(&app);
    });
}
