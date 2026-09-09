// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// The key path: raw key events -> duplicate guard -> swaps -> action ->
// router, plus the hold-repeat cadence and the press-and-hold rapid
// navigation flag. Mirrors Main.qml's own input section; the rules and
// their thresholds live in `zaparoo_app::input`, this file owns the
// timers and the wiring.

use std::sync::Arc;
use std::time::{Duration, Instant};

use slint::ComponentHandle;
use zaparoo_app::input as rules;

use crate::router::{lock, Ctx};
use crate::App;

/// The key path's state. Held in `Shared` because the repeat timers
/// fire on the event loop with nothing else to hang it from.
#[derive(Debug, Clone)]
pub struct InputModel {
    hold: rules::Hold,
    guard: rules::DuplicateGuard,
    rapid: rules::RapidNav,
    /// Origin for the monotonic millisecond clock the rules take.
    epoch: Instant,
    /// Bumped whenever the hold changes, so a repeat timer armed for an
    /// earlier hold fires as a no-op.
    repeat_seq: u64,
    /// The same ticket for the rapid navigation quiet tail.
    quiet_seq: u64,
    /// True only during a qualified held-repeat dispatch, never its async tail.
    rapid_dispatch: bool,
}

impl InputModel {
    pub fn new() -> Self {
        Self {
            hold: rules::Hold::new(),
            guard: rules::DuplicateGuard::new(),
            rapid: rules::RapidNav::new(),
            epoch: Instant::now(),
            repeat_seq: 0,
            quiet_seq: 0,
            rapid_dispatch: false,
        }
    }

    /// Retire the repeat ticket and report whether release needs a persist flush.
    fn release(&mut self, key: &str) -> bool {
        if !self.hold.release(key) {
            return false;
        }
        self.repeat_seq += 1;
        true
    }

    fn now_ms(&self) -> u64 {
        u64::try_from(self.epoch.elapsed().as_millis()).unwrap_or(u64::MAX)
    }
}

impl Default for InputModel {
    fn default() -> Self {
        Self::new()
    }
}

/// The keyboard is the live input source, so neither swap applies.
/// Derived from the controller report, exactly as the help bar's own
/// glyphs are.
fn keyboard_active() -> bool {
    zaparoo_core::controller_report::subscribe()
        .borrow()
        .as_ref()
        .is_some_and(|report| zaparoo_app::buttons::keyboard_active(report.layout))
}

/// The Slint `has-modal()` set, plus the CRT calibration overlay that
/// owns input above it. A repeat that lands while a modal is open still
/// routes to the modal, but it must not arm rapid navigation behind it:
/// the grid is still painted under the scrim.
fn modal_open(app: &App) -> bool {
    let overlays = app.global::<crate::Overlays>();
    app.global::<crate::LogUploadView>().get_open()
        || app.global::<crate::SetupModalView>().get_open()
        || app.global::<crate::GameInfoView>().get_modal_open()
        || overlays.get_card_write_open()
        || overlays.get_qr_open()
        || overlays.get_dialog_open()
        || overlays.get_list_open()
        || overlays.get_letter_open()
        || overlays.get_context_open()
        || overlays.get_crt_calibration_open()
}

/// A real press: guard against a double delivery, map the key to an
/// action, apply the swaps, route it, then arm the repeat.
fn key_pressed(ctx: &Ctx, app: &App, bindings: &std::collections::HashMap<i32, String>, key: &str) {
    let accepted = {
        let mut shared = lock(&ctx.shared);
        let now = shared.input.now_ms();
        shared.input.guard.accept(key, now)
    };
    if !accepted {
        return;
    }
    let Some(action) = crate::actions::action_for_key_with(bindings, key) else {
        return;
    };
    let (swap_cc, swap_ov) = {
        let shared = lock(&ctx.shared);
        let s = &shared.persist.settings;
        (s.swap_confirm_cancel, s.swap_options_view)
    };
    let action = rules::swap_actions(&action, swap_cc, swap_ov, keyboard_active()).to_string();
    // The screensaver eats the waking press whole, repeat included: a
    // held direction that only woke the screen must not start walking
    // the list behind it.
    let waking = app.global::<crate::Shell>().get_saver_armed();
    crate::router::handle_action(ctx, app, &action);
    if !waking {
        arm_repeat(ctx, app, &action, key);
    }
}

/// The key came up. Only the key that started the repeat cancels it; a
/// release of any other key in flight is ignored.
fn key_released(ctx: &Ctx, key: &str) {
    let stopped = lock(&ctx.shared).input.release(key);
    if stopped {
        // Hold::release already cleared the hold. Stopping it again would
        // report false and skip the final selection's synchronous flush.
        crate::games::flush_persist(ctx);
    }
}

/// Record the held key and start the initial-delay timer. No-op for
/// actions that do not repeat.
fn arm_repeat(ctx: &Ctx, app: &App, action: &str, key: &str) {
    let armed = {
        let mut shared = lock(&ctx.shared);
        let now = shared.input.now_ms();
        // Invalidate a timer still pending for the previous hold.
        shared.input.repeat_seq += 1;
        shared.input.hold.arm(action, key, now)
    };
    if armed {
        schedule_repeat(ctx, app, rules::REPEAT_INITIAL_MS);
    }
}

/// Stop the repeat and commit whatever cell the hold landed on. The
/// media list debounces its selection writes (one atomic disk write per
/// move would batter `MiSTer`'s SD card on a Down-hold through 20+
/// pages); the flush here lands the final selection so a kill during a
/// launch resumes on the right entry.
pub fn stop_repeat(ctx: &Ctx) {
    let held = {
        let mut shared = lock(&ctx.shared);
        shared.input.repeat_seq += 1;
        shared.input.hold.stop()
    };
    if held {
        crate::games::flush_persist(ctx);
    }
}

fn schedule_repeat(ctx: &Ctx, app: &App, delay_ms: u64) {
    let seq = lock(&ctx.shared).input.repeat_seq;
    let ctx = ctx.clone();
    let weak = app.as_weak();
    slint::Timer::single_shot(Duration::from_millis(delay_ms), move || {
        let Some(app) = weak.upgrade() else {
            return;
        };
        if lock(&ctx.shared).input.repeat_seq != seq {
            return;
        }
        repeat_fire(&ctx, &app);
    });
}

/// One repeat: dispatch the held action, then decide whether this hold
/// has run long enough to replace the live grid with rapid navigation.
/// Both go through `handle_action`, so the transition gate, modal
/// routing and screen dispatch all apply exactly as for a fresh press.
fn repeat_fire(ctx: &Ctx, app: &App) {
    let fired = {
        let shared = lock(&ctx.shared);
        let now = shared.input.now_ms();
        shared
            .input
            .hold
            .tick(now)
            .map(|(action, long_enough)| (action.to_string(), long_enough))
    };
    let Some((action, long_enough)) = fired else {
        return;
    };
    dispatch_repeat(ctx, app, &action, long_enough);
    schedule_repeat(ctx, app, rules::REPEAT_TICK_MS);
}

pub(crate) fn rapid_page(ctx: &Ctx) -> bool {
    lock(&ctx.shared).input.rapid_dispatch
}

/// Async page fills use the input state, never its mirrored render flag.
pub(crate) fn rapid_navigation(ctx: &Ctx) -> bool {
    lock(&ctx.shared).input.rapid.active()
}

/// Keep repeat identity through screen dispatch; rapid rendering state is
/// not suitable because ordinary actions reset it before navigating.
pub(crate) fn dispatch_repeat(ctx: &Ctx, app: &App, action: &str, long_enough: bool) {
    // Read before dispatch: a modal that owns input keeps this repeat
    // off the rapid flag even though the action still routes to it.
    let owns_input = !modal_open(app);
    lock(&ctx.shared).input.rapid_dispatch =
        owns_input && long_enough && rules::is_rapid_navigation_action(action);
    crate::router::handle_action(ctx, app, action);
    lock(&ctx.shared).input.rapid_dispatch = false;
    if owns_input {
        // Publish the held state after dispatch. Qualified repeats preserve
        // the previous rapid state; early repeats still behave like taps.
        note_rapid(ctx, app, action, long_enough);
    }
}

/// A navigation action was dispatched to a screen (`handle_action`'s
/// own call passes `held_long_enough: false`, so a fresh tap always
/// restores the live grid).
pub fn note_rapid(ctx: &Ctx, app: &App, action: &str, held_long_enough: bool) {
    let (applied, active) = {
        let mut shared = lock(&ctx.shared);
        let applied = shared.input.rapid.note(action, held_long_enough);
        if applied {
            shared.input.quiet_seq += 1;
        }
        (applied, shared.input.rapid.active())
    };
    if !applied {
        return;
    }
    push_rapid(ctx, app, active);
    // The tail only has work to do while the flag is up; a fresh tap
    // has already cleared it (and invalidated the armed timer) by
    // bumping the ticket above.
    if active {
        schedule_quiet(ctx, app);
    }
}

/// Rapid navigation ends a beat after the last navigation action.
fn schedule_quiet(ctx: &Ctx, app: &App) {
    let seq = lock(&ctx.shared).input.quiet_seq;
    let ctx = ctx.clone();
    let weak = app.as_weak();
    slint::Timer::single_shot(Duration::from_millis(rules::RAPID_QUIET_MS), move || {
        let Some(app) = weak.upgrade() else {
            return;
        };
        {
            let mut shared = lock(&ctx.shared);
            if shared.input.quiet_seq != seq {
                return;
            }
            shared.input.rapid.reset();
        }
        push_rapid(&ctx, &app, false);
    });
}

/// The flag only means anything on the media-list screens; the Qt
/// bindings gate it on the active screen the same way.
fn push_rapid(ctx: &Ctx, app: &App, active: bool) {
    let on_list = matches!(
        app.global::<crate::Shell>().get_active_screen().as_str(),
        "games" | "favorites" | "recents"
    );
    crate::games::set_rapid(ctx, app, active && on_list);
}

/// Key events -> normalized actions -> router, honoring the merged
/// `[input.keyboard]` bindings from frontend.toml.
pub fn bind(ctx: &Arc<Ctx>, app: &App, bindings: std::collections::HashMap<i32, String>) {
    let pressed_ctx = ctx.clone();
    let weak = app.as_weak();
    app.on_key_pressed(move |text| {
        if let Some(app) = weak.upgrade() {
            key_pressed(&pressed_ctx, &app, &bindings, &text);
        }
    });
    let released_ctx = ctx.clone();
    app.on_key_released(move |text| key_released(&released_ctx, &text));
    let lost_ctx = ctx.clone();
    app.on_input_lost(move || stop_repeat(&lost_ctx));
}

#[cfg(test)]
mod tests {
    use super::InputModel;

    #[test]
    fn held_key_release_retires_timer_and_requests_exactly_one_flush() {
        let mut model = InputModel::new();
        model.hold.arm("down", "ArrowDown", 0);
        let ticket = model.repeat_seq;
        assert!(!model.release("ArrowUp"));
        assert_eq!(model.repeat_seq, ticket);
        assert!(model.release("ArrowDown"));
        assert!(model.repeat_seq > ticket);
        assert!(model.hold.tick(1000).is_none());
        assert!(!model.release("ArrowDown"));
    }
}
