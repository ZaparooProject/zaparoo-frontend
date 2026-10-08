// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

//! The header line's app cue: what the user's own action is waiting on
//! ("Loading more…", "Saving…", "Launching Tetris…"), or the brief
//! confirmation of one that changed something off screen ("Added to
//! Hub"). A wait follows `zaparoo_app::wait_cue`: nothing for the first
//! 300 ms, then at least 200 ms once shown. One slot, and the latest
//! caller owns it; the view decides when the line yields to Core link
//! trouble (`StatusLine` in `ui/chrome.slint`).

use std::time::Duration;

use slint::{ComponentHandle, SharedString};
use zaparoo_app::wait_cue::{Token, WaitCue, CUE_DELAY_MS, CUE_HOLD_MS};

use crate::router::{lock, Ctx};
use crate::{App, AppCue};

/// What a wait says: the cue and the values its sentence takes.
#[derive(Debug, Clone, PartialEq)]
struct Text {
    cue: AppCue,
    arg: String,
    arg2: String,
}

#[derive(Debug, Default)]
pub struct Model {
    rule: WaitCue,
    text: Option<Text>,
    /// Bumped by every new owner of the line, so a confirmation's timer
    /// only clears the confirmation it started.
    flash_seq: u64,
}

/// One wait in progress, handed back to `end` when its answer arrives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Wait(Token);

fn show(app: &App, text: Option<&Text>) {
    let shell = app.global::<crate::Shell>();
    if let Some(text) = text {
        shell.set_status_arg(SharedString::from(text.arg.as_str()));
        shell.set_status_arg2(SharedString::from(text.arg2.as_str()));
        shell.set_status_text(text.cue);
    } else {
        shell.set_status_text(AppCue::None);
        shell.set_status_arg(SharedString::default());
        shell.set_status_arg2(SharedString::default());
    }
}

/// A wait on Core starts. Its cue appears only if the wait outlasts the
/// delay. `arg` and `arg2` fill the cue's sentence (a name, a count).
pub fn begin(ctx: &Ctx, app: &App, cue: AppCue, arg: &str, arg2: &str) -> Wait {
    let token = {
        let mut shared = lock(&ctx.shared);
        let model = &mut shared.cue;
        model.flash_seq += 1;
        model.text = Some(Text {
            cue,
            arg: arg.to_string(),
            arg2: arg2.to_string(),
        });
        model.rule.begin()
    };
    // Whatever held the line belonged to an action this one replaces.
    show(app, None);
    let ctx = ctx.clone();
    let weak = app.as_weak();
    slint::Timer::single_shot(Duration::from_millis(CUE_DELAY_MS), move || {
        let Some(app) = weak.upgrade() else {
            return;
        };
        let text = {
            let mut shared = lock(&ctx.shared);
            if !shared.cue.rule.delay_elapsed(token) {
                return;
            }
            shared.cue.text.clone()
        };
        show(&app, text.as_ref());
        let weak = app.as_weak();
        slint::Timer::single_shot(Duration::from_millis(CUE_HOLD_MS), move || {
            let clear = lock(&ctx.shared).cue.rule.hold_elapsed(token);
            if clear {
                if let Some(app) = weak.upgrade() {
                    show(&app, None);
                }
            }
        });
    });
    Wait(token)
}

/// The wait has more to say (a count moved on). Shown at once if its cue
/// is up, otherwise kept for when it appears.
pub fn retext(ctx: &Ctx, app: &App, wait: Wait, cue: AppCue, arg: &str, arg2: &str) {
    let text = {
        let mut shared = lock(&ctx.shared);
        let model = &mut shared.cue;
        if !model.rule.is_waiting(wait.0) {
            return;
        }
        let text = Text {
            cue,
            arg: arg.to_string(),
            arg2: arg2.to_string(),
        };
        model.text = Some(text.clone());
        model.rule.is_shown(wait.0).then_some(text)
    };
    if let Some(text) = text {
        show(app, Some(&text));
    }
}

/// The wait is over. A cue that only just appeared stays out its hold.
pub fn end(ctx: &Ctx, app: &App, wait: Wait) {
    let clear = lock(&ctx.shared).cue.rule.end(wait.0);
    if clear {
        show(app, None);
    }
}

/// Confirm something that happened where the user cannot see it. Shown at
/// once, for `ms`; a wait or another confirmation replaces it.
pub fn flash(ctx: &Ctx, app: &App, cue: AppCue, ms: u64) {
    let seq = {
        let mut shared = lock(&ctx.shared);
        let model = &mut shared.cue;
        // Retire any wait: its timers find a newer owner.
        let retired = model.rule.begin();
        model.rule.end(retired);
        model.text = None;
        model.flash_seq += 1;
        model.flash_seq
    };
    show(
        app,
        Some(&Text {
            cue,
            arg: String::new(),
            arg2: String::new(),
        }),
    );
    let ctx = ctx.clone();
    let weak = app.as_weak();
    slint::Timer::single_shot(Duration::from_millis(ms), move || {
        if lock(&ctx.shared).cue.flash_seq != seq {
            return;
        }
        if let Some(app) = weak.upgrade() {
            show(&app, None);
        }
    });
}

/// A wait whose cue lives in the surface that is waiting (a relabelled
/// row, a line in a modal) rather than in the header. Same rule, same
/// numbers; the caller says how to show the cue and what to do when the
/// wait is over, and the second is put off while the hold keeps the cue up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocalWait(u64);

/// What a local wait does once it is over.
type Finish = Box<dyn FnOnce(&App)>;

struct Local {
    rule: WaitCue,
    token: Token,
    finish: Option<Finish>,
}

thread_local! {
    // Event-loop state: the finish closures hold view handles.
    static LOCALS: std::cell::RefCell<(u64, std::collections::HashMap<u64, Local>)> =
        std::cell::RefCell::new((0, std::collections::HashMap::new()));
}

/// Start a local wait. `show` runs if the wait outlasts the delay.
pub fn begin_local(app: &App, show: impl FnOnce(&App) + 'static) -> LocalWait {
    let id = LOCALS.with(|locals| {
        let mut locals = locals.borrow_mut();
        locals.0 += 1;
        let id = locals.0;
        let mut rule = WaitCue::default();
        let token = rule.begin();
        locals.1.insert(
            id,
            Local {
                rule,
                token,
                finish: None,
            },
        );
        id
    });
    let weak = app.as_weak();
    slint::Timer::single_shot(Duration::from_millis(CUE_DELAY_MS), move || {
        let shown = LOCALS.with(|locals| {
            locals
                .borrow_mut()
                .1
                .get_mut(&id)
                .is_some_and(|local| local.rule.delay_elapsed(local.token))
        });
        let Some(app) = weak.upgrade().filter(|_| shown) else {
            return;
        };
        show(&app);
        let weak = app.as_weak();
        slint::Timer::single_shot(Duration::from_millis(CUE_HOLD_MS), move || {
            let finish = LOCALS.with(|locals| {
                let mut locals = locals.borrow_mut();
                let over = locals
                    .1
                    .get_mut(&id)
                    .is_some_and(|local| local.rule.hold_elapsed(local.token));
                over.then(|| locals.1.remove(&id).and_then(|local| local.finish))
                    .flatten()
            });
            if let (Some(finish), Some(app)) = (finish, weak.upgrade()) {
                finish(&app);
            }
        });
    });
    LocalWait(id)
}

/// The local wait is over. `finish` clears the cue and applies the
/// answer: now, or once a cue that only just appeared has stayed its hold.
pub fn end_local(app: &App, wait: LocalWait, finish: impl FnOnce(&App) + 'static) {
    let now: Option<Finish> = LOCALS.with(|locals| {
        let mut locals = locals.borrow_mut();
        let Some(local) = locals.1.get_mut(&wait.0) else {
            return Some(Box::new(finish) as Finish);
        };
        let cleared = local.rule.end(local.token);
        if cleared || !local.rule.is_shown(local.token) {
            locals.1.remove(&wait.0);
            Some(Box::new(finish))
        } else {
            local.finish = Some(Box::new(finish));
            None
        }
    });
    if let Some(finish) = now {
        finish(app);
    }
}

/// Drop a local wait whose surface has gone away: nothing is shown and
/// nothing is finished.
pub fn abandon_local(wait: LocalWait) {
    LOCALS.with(|locals| {
        locals.borrow_mut().1.remove(&wait.0);
    });
}
