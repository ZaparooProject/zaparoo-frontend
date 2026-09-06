// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// Maps Slint key-event text onto the normalized action catalog from
// `zaparoo_core::input_actions`. Slint encodes non-printable keys as
// single private-use-area characters exposed through
// `slint::platform::Key`, so the mapping is a char comparison. The
// default bindings mirror `input_actions::default_bindings()`; the
// `[input.keyboard]` config override path is not wired in the demo.

use slint::platform::Key;
use std::collections::HashMap;
use zaparoo_core::input_actions::actions;

/// Qt key code for a Slint key event, covering the same subset as
/// `zaparoo_core::input_actions::qt_key_code`. This is the bridge that
/// lets the user's `[input.keyboard]` overrides (stored as Qt codes)
/// apply to Slint key events unchanged.
fn qt_code_for(ch: char) -> Option<i32> {
    let code = if ch == char::from(Key::LeftArrow) {
        0x0100_0012
    } else if ch == char::from(Key::RightArrow) {
        0x0100_0014
    } else if ch == char::from(Key::UpArrow) {
        0x0100_0013
    } else if ch == char::from(Key::DownArrow) {
        0x0100_0015
    } else if ch == char::from(Key::Return) {
        0x0100_0004
    } else if ch == char::from(Key::Escape) {
        0x0100_0000
    } else if ch == char::from(Key::Backspace) {
        0x0100_0003
    } else if ch == char::from(Key::Tab) {
        0x0100_0001
    } else if ch == char::from(Key::PageUp) {
        0x0100_0016
    } else if ch == char::from(Key::PageDown) {
        0x0100_0017
    } else if ch == ' ' {
        0x20
    } else {
        return None;
    };
    Some(code)
}

/// Resolve a Slint key event against the merged binding map from
/// `frontend.toml` (`config.key_to_action`: Qt key code -> action).
/// Falls back to the built-in defaults for events the map doesn't
/// cover, so a partial user override never bricks navigation.
pub fn action_for_key_with(map: &HashMap<i32, String>, text: &str) -> Option<String> {
    let ch = text.chars().next()?;
    if let Some(code) = qt_code_for(ch) {
        if let Some(action) = map.get(&code) {
            return Some(action.clone());
        }
    }
    action_for_key(text).map(str::to_string)
}

pub fn action_for_key(text: &str) -> Option<&'static str> {
    let ch = text.chars().next()?;
    let action = if ch == char::from(Key::LeftArrow) {
        actions::LEFT
    } else if ch == char::from(Key::RightArrow) {
        actions::RIGHT
    } else if ch == char::from(Key::UpArrow) {
        actions::UP
    } else if ch == char::from(Key::DownArrow) {
        actions::DOWN
    } else if ch == char::from(Key::Return) {
        actions::ACCEPT
    } else if ch == char::from(Key::Escape) || ch == char::from(Key::Backspace) {
        actions::CANCEL
    } else if ch == char::from(Key::Tab) {
        actions::CONTEXT_MENU
    } else if ch == char::from(Key::PageUp) {
        actions::PAGE_PREV
    } else if ch == char::from(Key::PageDown) {
        actions::PAGE_NEXT
    } else if ch == ' ' {
        actions::PAGE_MENU
    } else {
        return None;
    };
    Some(action)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arrows_map_to_directions() {
        let left: char = Key::LeftArrow.into();
        assert_eq!(action_for_key(&left.to_string()), Some(actions::LEFT));
        let down: char = Key::DownArrow.into();
        assert_eq!(action_for_key(&down.to_string()), Some(actions::DOWN));
    }

    #[test]
    fn return_accepts_escape_cancels() {
        let ret: char = Key::Return.into();
        assert_eq!(action_for_key(&ret.to_string()), Some(actions::ACCEPT));
        let esc: char = Key::Escape.into();
        assert_eq!(action_for_key(&esc.to_string()), Some(actions::CANCEL));
    }

    #[test]
    fn plain_text_maps_to_nothing() {
        assert_eq!(action_for_key("a"), None);
        assert_eq!(action_for_key(""), None);
    }
}
