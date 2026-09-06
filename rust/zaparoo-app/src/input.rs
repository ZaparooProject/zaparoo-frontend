// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

//! The input rules `Main.qml` owns between a key event and an action:
//! the duplicate-delivery guard, the confirm/cancel and options/view
//! swaps, the hold-repeat state machine, and the press-and-hold rapid
//! navigation flag. Toolkit-agnostic; the shell owns the timers these
//! rules ask for and supplies the clock.

/// A held repeatable action waits this long before it starts repeating.
pub const REPEAT_INITIAL_MS: u64 = 350;
/// Then it repeats at this cadence.
pub const REPEAT_TICK_MS: u64 = 90;
/// Rapid navigation ends this long after the last navigation action.
pub const RAPID_QUIET_MS: u64 = 260;
/// One uninterrupted hold has to last this long before it counts as
/// rapid navigation. Qt's own press-and-hold gesture uses the same 800
/// ms; holding it apart from the repeat handoff lets ordinary held
/// navigation start promptly without replacing the live grid until the
/// intent is unambiguous.
pub const RAPID_HOLD_MS: u64 = 800;
/// A second delivery of the same key inside this window is contact
/// bounce or an input stack double send, not a second press. Far below
/// the repeat handoff so it never touches hold-repeat, and below the
/// floor for a deliberate human re-tap.
pub const DUPLICATE_INPUT_WINDOW_MS: u64 = 40;

/// The directions that repeat while held.
pub fn is_repeatable_action(action: &str) -> bool {
    matches!(
        action,
        "up" | "down" | "left" | "right" | "page_prev" | "page_next"
    )
}

/// The subset of those that can run away into rapid navigation: the
/// ones that walk a list, not the ones that walk a row.
pub fn is_rapid_navigation_action(action: &str) -> bool {
    matches!(action, "up" | "down" | "page_prev" | "page_next")
}

/// Drop a second delivery of the same key while the guard window is
/// open. A different key, or the same key after the window closes, is
/// not a duplicate.
pub fn is_duplicate_input(key: &str, last_key: &str, within_window: bool) -> bool {
    within_window && key == last_key
}

/// Apply the two input swaps at the single seam every screen and modal
/// sees. Neither applies while the keyboard is the live input source:
/// Enter/Escape and Tab/Space are fixed keys, not something a
/// controller mapping mistake affects.
pub fn swap_actions(
    action: &str,
    swap_confirm_cancel: bool,
    swap_options_view: bool,
    keyboard_active: bool,
) -> &str {
    if keyboard_active {
        return action;
    }
    if swap_confirm_cancel {
        match action {
            "accept" => return "cancel",
            "cancel" => return "accept",
            _ => {}
        }
    }
    if swap_options_view {
        match action {
            "context_menu" => return "page_menu",
            "page_menu" => return "context_menu",
            _ => {}
        }
    }
    action
}

/// The hold-repeat state machine (`_armRepeat`, `handleKeyRelease`,
/// `_stopRepeat`). Qt's own auto-repeat is dropped because it bursts
/// unpredictably under load and is not tunable on the framebuffer
/// build; this drives the cadence itself.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Hold {
    action: String,
    key: String,
    started_ms: u64,
}

impl Hold {
    pub fn new() -> Self {
        Self::default()
    }

    /// The action currently held, if any.
    pub fn action(&self) -> Option<&str> {
        if self.action.is_empty() {
            None
        } else {
            Some(self.action.as_str())
        }
    }

    /// A fresh press landed. True when the shell must arm the initial
    /// delay timer; non-repeatable actions leave the machine alone.
    pub fn arm(&mut self, action: &str, key: &str, now_ms: u64) -> bool {
        if !is_repeatable_action(action) {
            return false;
        }
        self.action = action.to_string();
        self.key = key.to_string();
        self.started_ms = now_ms;
        true
    }

    /// A key came up. True when it was the key that started the repeat,
    /// so the shell stops its timers; a release of any other key in
    /// flight (a chord, an unrelated press mid-hold) is ignored.
    pub fn release(&mut self, key: &str) -> bool {
        if self.action.is_empty() || self.key != key {
            return false;
        }
        self.stop();
        true
    }

    /// Forget the hold. True when something was actually held.
    pub fn stop(&mut self) -> bool {
        let held = !self.action.is_empty();
        self.action.clear();
        self.key.clear();
        self.started_ms = 0;
        held
    }

    /// A repeat timer fired: the action to dispatch, plus whether this
    /// hold has lasted long enough to count as rapid navigation.
    pub fn tick(&self, now_ms: u64) -> Option<(&str, bool)> {
        if self.action.is_empty() {
            return None;
        }
        let long_enough = self.started_ms > 0 && now_ms - self.started_ms >= RAPID_HOLD_MS;
        Some((self.action.as_str(), long_enough))
    }
}

/// The duplicate-input guard. The window stays anchored to the first
/// accepted press so a bounce cannot extend it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DuplicateGuard {
    last_key: String,
    last_ms: u64,
    open: bool,
}

impl DuplicateGuard {
    pub fn new() -> Self {
        Self::default()
    }

    /// True when the press is a real one and should be routed.
    pub fn accept(&mut self, key: &str, now_ms: u64) -> bool {
        let within = self.open && now_ms - self.last_ms < DUPLICATE_INPUT_WINDOW_MS;
        if is_duplicate_input(key, &self.last_key, within) {
            return false;
        }
        self.last_key.clear();
        self.last_key.push_str(key);
        self.last_ms = now_ms;
        self.open = true;
        true
    }
}

/// Press-and-hold rapid navigation (`rapidNavigationActive`): the flag
/// the media list reads to pause covers and drop the detail pane while
/// a hold runs away through the list.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RapidNav {
    active: bool,
    action: String,
}

impl RapidNav {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn active(&self) -> bool {
        self.active
    }

    pub fn action(&self) -> &str {
        &self.action
    }

    /// A navigation action was dispatched. True when the rule applied
    /// and the shell must restart the quiet timer. Only one
    /// uninterrupted physical hold may replace the live grid: fresh
    /// taps always keep (or restore) it, even when several arrive
    /// inside the quiet tail.
    pub fn note(&mut self, action: &str, held_long_enough: bool) -> bool {
        if !is_rapid_navigation_action(action) {
            return false;
        }
        self.action.clear();
        self.action.push_str(action);
        self.active = held_long_enough;
        true
    }

    /// The quiet timer fired, or the hold was cancelled outright.
    pub fn reset(&mut self) {
        self.active = false;
        self.action.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_directions_repeat() {
        for action in ["up", "down", "left", "right", "page_prev", "page_next"] {
            assert!(is_repeatable_action(action), "{action}");
        }
        for action in ["accept", "cancel", "context_menu", "page_menu", ""] {
            assert!(!is_repeatable_action(action), "{action}");
        }
    }

    #[test]
    fn only_list_walking_actions_go_rapid() {
        for action in ["up", "down", "page_prev", "page_next"] {
            assert!(is_rapid_navigation_action(action), "{action}");
        }
        // Left/Right repeat but never replace the live grid.
        for action in ["left", "right", "accept"] {
            assert!(!is_rapid_navigation_action(action), "{action}");
        }
    }

    #[test]
    fn a_duplicate_is_the_same_key_inside_the_window() {
        assert!(is_duplicate_input("a", "a", true));
        assert!(!is_duplicate_input("a", "a", false));
        assert!(!is_duplicate_input("a", "b", true));
    }

    #[test]
    fn the_guard_drops_a_bounce_without_extending_its_window() {
        let mut guard = DuplicateGuard::new();
        assert!(guard.accept("down", 1_000));
        // The bounce is dropped.
        assert!(!guard.accept("down", 1_020));
        // The window still closes 40 ms after the first accepted press,
        // not after the bounce.
        assert!(guard.accept("down", 1_041));
        // A different key is never a duplicate.
        assert!(guard.accept("up", 1_042));
    }

    #[test]
    fn the_swaps_apply_only_off_the_keyboard() {
        assert_eq!(swap_actions("accept", true, false, false), "cancel");
        assert_eq!(swap_actions("cancel", true, false, false), "accept");
        assert_eq!(swap_actions("accept", false, false, false), "accept");
        assert_eq!(
            swap_actions("context_menu", false, true, false),
            "page_menu"
        );
        assert_eq!(
            swap_actions("page_menu", false, true, false),
            "context_menu"
        );
        assert_eq!(swap_actions("up", true, true, false), "up");
        // Enter/Escape and Tab/Space are fixed keys.
        assert_eq!(swap_actions("accept", true, true, true), "accept");
        assert_eq!(
            swap_actions("context_menu", true, true, true),
            "context_menu"
        );
    }

    #[test]
    fn a_hold_arms_only_for_repeatable_actions() {
        let mut hold = Hold::new();
        assert!(!hold.arm("accept", "Return", 0));
        assert_eq!(hold.action(), None);
        assert!(hold.arm("down", "Down", 100));
        assert_eq!(hold.action(), Some("down"));
    }

    #[test]
    fn only_the_key_that_started_the_repeat_stops_it() {
        let mut hold = Hold::new();
        hold.arm("down", "Down", 0);
        assert!(!hold.release("Up"));
        assert_eq!(hold.action(), Some("down"));
        assert!(hold.release("Down"));
        assert_eq!(hold.action(), None);
        // A release with nothing held is a no-op.
        assert!(!hold.release("Down"));
    }

    #[test]
    fn a_tick_reports_rapid_only_after_the_hold_threshold() {
        let mut hold = Hold::new();
        assert_eq!(hold.tick(5_000), None);
        hold.arm("down", "Down", 1_000);
        assert_eq!(hold.tick(1_350), Some(("down", false)));
        assert_eq!(hold.tick(1_000 + RAPID_HOLD_MS - 1), Some(("down", false)));
        assert_eq!(hold.tick(1_000 + RAPID_HOLD_MS), Some(("down", true)));
        assert!(hold.stop());
        assert!(!hold.stop());
    }

    #[test]
    fn rapid_navigation_needs_one_uninterrupted_hold() {
        let mut rapid = RapidNav::new();
        // A fresh tap records the direction but keeps the live grid.
        assert!(rapid.note("down", false));
        assert!(!rapid.active());
        assert_eq!(rapid.action(), "down");
        // The hold passes the threshold.
        assert!(rapid.note("down", true));
        assert!(rapid.active());
        // A tap inside the quiet tail restores the live grid.
        assert!(rapid.note("down", false));
        assert!(!rapid.active());
        // Actions that never go rapid leave the flag alone.
        rapid.note("down", true);
        assert!(!rapid.note("left", false));
        assert!(rapid.active());
        rapid.reset();
        assert!(!rapid.active());
        assert_eq!(rapid.action(), "");
    }
}
