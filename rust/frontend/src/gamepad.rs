// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// Desktop gamepad source. gilrs normalizes every pad onto the kernel
// gamepad layout and applies the SDL mapping database, which is also how
// Steam's virtual pad and the `SDL_GAMECONTROLLERCONFIG` override Steam
// exports arrive correctly, so this module only has to map named buttons
// onto the action catalog and turn stick deflection into the same four
// directions.
//
// Events route through `input::gamepad_pressed` rather than synthetic key
// events. Keeping the two sources apart is the point: which device last
// drove the UI decides whether the help bar draws keycaps or pad glyphs,
// and whether the confirm/cancel and options/view swap settings apply at
// all. The reader publishes that decision through the same controller
// report channel `MiSTer` fills from Main's file, so the help bar has one
// consumer and no idea which producer spoke.
//
// `MiSTer` has its own reader (`mister::input`): Main forwards pad buttons
// as keyboard keys there, so everything below compiles to a no-op.

/// A real keyboard event arrived, so the help bar should show keycaps
/// until the pad is touched again. No-op unless the pad reader owns the
/// controller-report channel: on `MiSTer` the report comes from Main.
#[cfg(not(feature = "desktop"))]
pub fn note_keyboard_input() {}

#[cfg(feature = "desktop")]
pub use desktop::{note_keyboard_input, spawn};

#[cfg(feature = "desktop")]
mod desktop {
    use std::collections::HashMap;
    use std::hash::Hash;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, RwLock};
    use std::time::Duration;

    use gilrs::{Axis, Button, EventType, GamepadId, Gilrs};
    use slint::ComponentHandle;
    use zaparoo_app::buttons;
    use zaparoo_core::controller_report::{self, ControllerGlyphs};
    use zaparoo_core::input_actions::actions;

    use crate::router::Ctx;
    use crate::App;

    /// Which face accepts on a pad this reader drives. Every modern pad
    /// this runs on, the Steam Deck included, confirms with the south
    /// face; `swap_confirm_cancel` covers the ones that do not.
    const PAD_ACCEPT: &str = "FaceSouth";
    const PAD_CANCEL: &str = "FaceEast";

    /// Stick deflection is a latch with hysteresis: it takes a firm push
    /// to engage and a real return toward center to release, so a worn
    /// stick resting just past the line cannot chatter the grid.
    const AXIS_ENGAGE: f32 = 0.6;
    const AXIS_RELEASE: f32 = 0.4;

    /// Steam sets its session up around the moment a launched app starts,
    /// so a frontend that enumerates too early finds nothing and then
    /// never hears about the pad: the hotplug watch only reports devices
    /// that appear after it is armed, and one that was already there when
    /// the scan missed it raises no event. Re-enumerate while no pad has
    /// ever been seen, then stop, because hotplug covers everything after
    /// the first one.
    const RESCAN_INTERVAL: Duration = Duration::from_secs(2);
    const RESCAN_LIMIT: u8 = 30;

    /// True once the reader owns the controller-report channel. Guards
    /// `note_keyboard_input` so a build with no pad, or one where the
    /// `MiSTer` watcher is running, never overwrites the report.
    static READER_RUNNING: AtomicBool = AtomicBool::new(false);

    /// Style the connected pad resolved to, or None when none is
    /// connected. Held so a keyboard press can flip the bar to keycaps
    /// and the next pad press can flip it straight back.
    static PAD_STYLE: RwLock<Option<&'static str>> = RwLock::new(None);

    #[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
    enum AxisLatch {
        #[default]
        Center,
        Negative,
        Positive,
    }

    /// One axis sample against the current latch. A flick straight
    /// through center to the far side lands on the far side in one step
    /// rather than sticking at center for a sample.
    fn next_latch(current: AxisLatch, value: f32) -> AxisLatch {
        let held = match current {
            AxisLatch::Positive => value > AXIS_RELEASE,
            AxisLatch::Negative => value < -AXIS_RELEASE,
            AxisLatch::Center => false,
        };
        if held {
            return current;
        }
        if value >= AXIS_ENGAGE {
            AxisLatch::Positive
        } else if value <= -AXIS_ENGAGE {
            AxisLatch::Negative
        } else {
            AxisLatch::Center
        }
    }

    fn action_for_button(button: Button) -> Option<&'static str> {
        Some(match button {
            Button::DPadUp => actions::UP,
            Button::DPadDown => actions::DOWN,
            Button::DPadLeft => actions::LEFT,
            Button::DPadRight => actions::RIGHT,
            Button::South => actions::ACCEPT,
            Button::East => actions::CANCEL,
            Button::North => actions::CONTEXT_MENU,
            Button::West => actions::PAGE_MENU,
            Button::LeftTrigger => actions::PAGE_PREV,
            Button::RightTrigger => actions::PAGE_NEXT,
            _ => return None,
        })
    }

    /// The two directions an axis stands for, negative first. Hat axes
    /// arrive here too: a pad whose driver reports its d-pad as a hat
    /// produces `DPadX`/`DPadY` instead of the four buttons.
    fn actions_for_axis(axis: Axis) -> Option<(&'static str, &'static str)> {
        match axis {
            Axis::LeftStickX | Axis::DPadX => Some((actions::LEFT, actions::RIGHT)),
            // gilrs flips the Y axis on evdev, so up reads positive.
            Axis::LeftStickY | Axis::DPadY => Some((actions::DOWN, actions::UP)),
            _ => None,
        }
    }

    fn latch_action(
        latch: AxisLatch,
        negative: &'static str,
        positive: &'static str,
    ) -> Option<&'static str> {
        match latch {
            AxisLatch::Center => None,
            AxisLatch::Negative => Some(negative),
            AxisLatch::Positive => Some(positive),
        }
    }

    /// Hold identity for a pad action. Not a keyboard key: the duplicate
    /// guard and the hold only need a stable string, and a pad-owned
    /// namespace keeps a pad hold from cancelling a keyboard one. One
    /// identity per action, so the stick and the d-pad pushed the same
    /// way are one hold rather than two competing ones.
    fn hold_key(action: &str) -> String {
        format!("pad:{action}")
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    enum PadControl {
        Button(Button),
        Axis(Axis),
    }

    /// Physical pad controls currently contributing to each logical action.
    /// Multiple pads, or a stick and d-pad on one pad, share one `InputModel`
    /// hold and release it only when its final owner goes away.
    #[derive(Debug)]
    struct ActiveActions<Id> {
        sources: HashMap<(Id, PadControl), &'static str>,
        owners: HashMap<&'static str, usize>,
    }

    impl<Id> Default for ActiveActions<Id> {
        fn default() -> Self {
            Self {
                sources: HashMap::new(),
                owners: HashMap::new(),
            }
        }
    }

    impl<Id: Copy + Eq + Hash> ActiveActions<Id> {
        /// Change one physical control's action. Returns the logical release
        /// and press edges, in that order, after co-owners are accounted for.
        fn set(
            &mut self,
            id: Id,
            control: PadControl,
            next: Option<&'static str>,
        ) -> (Option<&'static str>, Option<&'static str>) {
            let key = (id, control);
            let current = match next {
                Some(action) => self.sources.insert(key, action),
                None => self.sources.remove(&key),
            };
            if current == next {
                return (None, None);
            }

            let released = current.and_then(|action| {
                let count = self.owners.get_mut(action)?;
                *count -= 1;
                if *count == 0 {
                    self.owners.remove(action);
                    Some(action)
                } else {
                    None
                }
            });
            let pressed = next.and_then(|action| {
                let count = self.owners.entry(action).or_default();
                let first = *count == 0;
                *count += 1;
                first.then_some(action)
            });
            (released, pressed)
        }

        /// Drop every control owned by one disconnected pad and return only
        /// actions whose final owner disappeared.
        fn disconnect(&mut self, id: Id) -> Vec<&'static str> {
            let controls: Vec<PadControl> = self
                .sources
                .keys()
                .filter_map(|(owner, control)| (*owner == id).then_some(*control))
                .collect();
            controls
                .into_iter()
                .filter_map(|control| self.set(id, control, None).0)
                .collect()
        }
    }

    fn dispatch_transition(
        ctx: &Arc<Ctx>,
        weak: &slint::Weak<App>,
        transition: (Option<&'static str>, Option<&'static str>),
    ) {
        if let Some(action) = transition.0 {
            release(ctx, weak, action);
        }
        if let Some(action) = transition.1 {
            press(ctx, weak, action);
        }
    }

    fn publish_pad(style: &'static str) {
        controller_report::publish(Some(ControllerGlyphs {
            layout: style,
            accept_button: PAD_ACCEPT,
            cancel_button: PAD_CANCEL,
        }));
    }

    pub fn note_keyboard_input() {
        if !READER_RUNNING.load(Ordering::Acquire) {
            return;
        }
        // Enter and Escape are fixed, so the keyboard entry always carries
        // the default faces; publishing is a compare against the current
        // value, which makes this cheap enough to call per keystroke.
        controller_report::publish(Some(ControllerGlyphs {
            layout: buttons::KEYBOARD_STYLE,
            accept_button: buttons::DEFAULT_ACCEPT,
            cancel_button: buttons::DEFAULT_CANCEL,
        }));
    }

    /// The pad is driving again, so put its glyphs back up.
    fn note_pad_input() {
        let style = *lock_read();
        if let Some(style) = style {
            publish_pad(style);
        }
    }

    fn lock_read() -> std::sync::RwLockReadGuard<'static, Option<&'static str>> {
        PAD_STYLE
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn set_pad_style(style: Option<&'static str>) {
        let mut guard = PAD_STYLE
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *guard = style;
        drop(guard);
        match style {
            Some(style) => publish_pad(style),
            None => controller_report::publish(None),
        }
    }

    /// Start the desktop pad reader on its own thread. Returns false when
    /// the `MiSTer` report watcher already owns the channel, or when the
    /// thread could not start; the frontend stays keyboard-only either
    /// way. Blocking reads mean an idle pad costs nothing, which matters
    /// while a launched game owns the screen.
    pub fn spawn(ctx: &Arc<Ctx>, app: &App, report_watcher_running: bool) -> bool {
        if report_watcher_running {
            tracing::debug!("gamepad: report watcher owns the channel, reader not started");
            return false;
        }
        let ctx = ctx.clone();
        let weak = app.as_weak();
        std::thread::Builder::new()
            .name("zaparoo-gamepad".into())
            .spawn(move || reader_loop(&ctx, &weak))
            .inspect_err(|e| tracing::warn!(error = %e, "gamepad: reader thread did not start"))
            .is_ok()
    }

    fn press(ctx: &Arc<Ctx>, weak: &slint::Weak<App>, action: &'static str) {
        note_pad_input();
        if dormant(ctx) {
            return;
        }
        let ctx = ctx.clone();
        let _ = weak.upgrade_in_event_loop(move |app| {
            crate::input::gamepad_pressed(&ctx, &app, action, &hold_key(action));
        });
    }

    fn release(ctx: &Arc<Ctx>, weak: &slint::Weak<App>, action: &'static str) {
        if dormant(ctx) {
            return;
        }
        let ctx = ctx.clone();
        let _ = weak.upgrade_in_event_loop(move |_app| {
            crate::input::gamepad_released(&ctx, &hold_key(action));
        });
    }

    /// A launched game owns the screen and the pad. Drop the event here
    /// rather than waking the event loop for a press the router will
    /// refuse anyway.
    fn dormant(ctx: &Ctx) -> bool {
        *ctx.dormant.borrow()
    }

    fn open_gilrs() -> Option<Gilrs> {
        Gilrs::new().ok()
    }

    /// Opening the backend can fail outright, not just come up empty,
    /// while Steam is still setting its session up around a launch. That
    /// used to end the reader thread and leave the frontend with no
    /// controller for the rest of the run, silently. Keep trying on the
    /// rescan cadence, and say so in the log either way: input that does
    /// not work with no explanation is the worst thing to be handed in a
    /// support report.
    fn open_with_retry() -> Option<Gilrs> {
        for attempt in 0..RESCAN_LIMIT {
            match Gilrs::new() {
                Ok(gilrs) => {
                    if attempt > 0 {
                        tracing::info!(attempt, "gamepad backend opened after retry");
                    }
                    return Some(gilrs);
                }
                Err(e) if attempt == 0 => {
                    tracing::info!(error = %e, "gamepad backend unavailable; retrying");
                }
                Err(_) => {}
            }
            std::thread::sleep(RESCAN_INTERVAL);
        }
        tracing::warn!("gamepad backend never opened; keyboard only");
        None
    }

    /// Adopt whatever is already plugged in. Returns whether anything was.
    fn seed_connected(gilrs: &Gilrs) -> bool {
        let Some((_, pad)) = gilrs.gamepads().next() else {
            return false;
        };
        let style = controller_report::style_for_device_name(pad.name());
        tracing::info!(pad = pad.name(), style, "gamepad connected");
        set_pad_style(Some(style));
        true
    }

    fn reader_loop(ctx: &Arc<Ctx>, weak: &slint::Weak<App>) {
        let Some(mut gilrs) = open_with_retry() else {
            return;
        };
        READER_RUNNING.store(true, Ordering::Release);
        let mut seen_a_pad = seed_connected(&gilrs);
        let mut rescans: u8 = 0;
        if !seen_a_pad {
            // Logged rather than left silent: a frontend with no working
            // controller and no explanation in the log is the single
            // hardest thing to triage from a support report.
            tracing::info!("no gamepad at startup; rescanning");
        }

        let mut latches: HashMap<(GamepadId, Axis), AxisLatch> = HashMap::new();
        let mut active = ActiveActions::default();
        loop {
            let Some(event) = gilrs.next_event_blocking(Some(RESCAN_INTERVAL)) else {
                if seen_a_pad || rescans >= RESCAN_LIMIT {
                    continue;
                }
                rescans += 1;
                let Some(reopened) = open_gilrs() else {
                    continue;
                };
                gilrs = reopened;
                seen_a_pad = seed_connected(&gilrs);
                if seen_a_pad {
                    tracing::info!(rescans, "gamepad found after rescan");
                } else if rescans >= RESCAN_LIMIT {
                    tracing::info!(rescans, "still no gamepad; hotplug only from here");
                }
                continue;
            };
            seen_a_pad = true;
            match event.event {
                EventType::Connected => {
                    let pad = gilrs.gamepad(event.id);
                    let style = controller_report::style_for_device_name(pad.name());
                    tracing::info!(pad = pad.name(), style, "gamepad connected");
                    set_pad_style(Some(style));
                }
                EventType::Disconnected => {
                    latches.retain(|(id, _), _| *id != event.id);
                    for action in active.disconnect(event.id) {
                        release(ctx, weak, action);
                    }
                    // Re-resolve from whatever is left rather than keeping
                    // the unplugged pad's glyphs on screen.
                    let remaining = gilrs
                        .gamepads()
                        .next()
                        .map(|(_, pad)| controller_report::style_for_device_name(pad.name()));
                    if remaining.is_none() {
                        tracing::info!("last gamepad disconnected");
                    }
                    set_pad_style(remaining);
                }
                EventType::ButtonPressed(button, _) => {
                    if let Some(action) = action_for_button(button) {
                        note_pad_input();
                        dispatch_transition(
                            ctx,
                            weak,
                            active.set(event.id, PadControl::Button(button), Some(action)),
                        );
                    }
                }
                EventType::ButtonReleased(button, _) => {
                    if action_for_button(button).is_some() {
                        dispatch_transition(
                            ctx,
                            weak,
                            active.set(event.id, PadControl::Button(button), None),
                        );
                    }
                }
                EventType::AxisChanged(axis, value, _) => {
                    let Some((negative, positive)) = actions_for_axis(axis) else {
                        continue;
                    };
                    let key = (event.id, axis);
                    let current = latches.get(&key).copied().unwrap_or_default();
                    let next = next_latch(current, value);
                    if next == current {
                        continue;
                    }
                    latches.insert(key, next);
                    let next_action = latch_action(next, negative, positive);
                    if next_action.is_some() {
                        note_pad_input();
                    }
                    dispatch_transition(
                        ctx,
                        weak,
                        active.set(event.id, PadControl::Axis(axis), next_action),
                    );
                }
                _ => {}
            }
        }
    }

    #[cfg(test)]
    mod tests {
        use super::{
            action_for_button, actions_for_axis, hold_key, latch_action, next_latch, ActiveActions,
            Axis, AxisLatch, Button, PadControl,
        };
        use zaparoo_core::input_actions::actions;

        #[test]
        fn face_buttons_follow_the_south_confirm_convention() {
            assert_eq!(action_for_button(Button::South), Some(actions::ACCEPT));
            assert_eq!(action_for_button(Button::East), Some(actions::CANCEL));
            assert_eq!(
                action_for_button(Button::North),
                Some(actions::CONTEXT_MENU)
            );
            assert_eq!(action_for_button(Button::West), Some(actions::PAGE_MENU));
            assert_eq!(
                action_for_button(Button::LeftTrigger),
                Some(actions::PAGE_PREV)
            );
            assert_eq!(
                action_for_button(Button::RightTrigger),
                Some(actions::PAGE_NEXT)
            );
            // Analog triggers, the sticks' own clicks and the menu pad
            // stay unbound rather than guessing an action for them.
            assert_eq!(action_for_button(Button::LeftTrigger2), None);
            assert_eq!(action_for_button(Button::Start), None);
            assert_eq!(action_for_button(Button::Unknown), None);
        }

        #[test]
        fn vertical_axes_read_positive_as_up() {
            assert_eq!(
                actions_for_axis(Axis::LeftStickY),
                Some((actions::DOWN, actions::UP))
            );
            assert_eq!(
                actions_for_axis(Axis::DPadY),
                Some((actions::DOWN, actions::UP))
            );
            assert_eq!(
                actions_for_axis(Axis::LeftStickX),
                Some((actions::LEFT, actions::RIGHT))
            );
            assert_eq!(actions_for_axis(Axis::RightStickX), None);
        }

        #[test]
        fn a_latched_axis_needs_a_real_return_to_release() {
            // A drifting stick short of the engage threshold does nothing.
            assert_eq!(next_latch(AxisLatch::Center, 0.55), AxisLatch::Center);
            let engaged = next_latch(AxisLatch::Center, 0.7);
            assert_eq!(engaged, AxisLatch::Positive);
            // Inside the hysteresis band the latch holds, so the grid does
            // not chatter on a stick resting near the line.
            assert_eq!(next_latch(engaged, 0.45), AxisLatch::Positive);
            assert_eq!(next_latch(engaged, 0.3), AxisLatch::Center);
            // A flick straight across lands on the far side in one step.
            assert_eq!(next_latch(engaged, -0.9), AxisLatch::Negative);
        }

        #[test]
        fn disconnected_pad_releases_only_its_unshared_actions() {
            let mut active = ActiveActions::default();
            assert_eq!(
                active.set(
                    1_u8,
                    PadControl::Button(Button::South),
                    Some(actions::ACCEPT)
                ),
                (None, Some(actions::ACCEPT))
            );
            assert_eq!(
                active.set(
                    2_u8,
                    PadControl::Button(Button::South),
                    Some(actions::ACCEPT)
                ),
                (None, None)
            );
            assert!(active.disconnect(1).is_empty());
            assert_eq!(active.disconnect(2), [actions::ACCEPT]);
        }

        #[test]
        fn stick_and_button_share_one_logical_hold() {
            let mut active = ActiveActions::default();
            assert_eq!(
                active.set(1_u8, PadControl::Button(Button::DPadUp), Some(actions::UP)),
                (None, Some(actions::UP))
            );
            assert_eq!(
                active.set(1_u8, PadControl::Axis(Axis::LeftStickY), Some(actions::UP)),
                (None, None)
            );
            assert_eq!(
                active.set(1_u8, PadControl::Button(Button::DPadUp), None),
                (None, None)
            );
            assert_eq!(
                active.set(1_u8, PadControl::Axis(Axis::LeftStickY), None),
                (Some(actions::UP), None)
            );
        }

        #[test]
        fn latch_directions_and_hold_identity_are_stable() {
            assert_eq!(
                latch_action(AxisLatch::Negative, actions::LEFT, actions::RIGHT),
                Some(actions::LEFT)
            );
            assert_eq!(
                latch_action(AxisLatch::Positive, actions::LEFT, actions::RIGHT),
                Some(actions::RIGHT)
            );
            assert_eq!(
                latch_action(AxisLatch::Center, actions::LEFT, actions::RIGHT),
                None
            );
            // One identity per action, so the stick and the d-pad pushed
            // the same way are one hold rather than two competing ones.
            assert_eq!(hold_key(actions::LEFT), "pad:left");
            assert_ne!(hold_key(actions::LEFT), hold_key(actions::RIGHT));
        }
    }
}
