// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// Normalized UI action catalog and the defaulted key-bindings that map
// raw Qt key codes onto those actions. Screens handle actions, not keys,
// so gamepad / NFC reader sources can slot in beside the keyboard without
// touching the UI tree. Inspired by RetroArch's RetroPad abstraction.

use std::collections::HashMap;

pub mod actions {
    pub const UP: &str = "up";
    pub const DOWN: &str = "down";
    pub const LEFT: &str = "left";
    pub const RIGHT: &str = "right";
    pub const ACCEPT: &str = "accept";
    pub const CANCEL: &str = "cancel";
    pub const CONTEXT_MENU: &str = "context_menu";
    pub const DETAILS: &str = "details";
    pub const PAGE_PREV: &str = "page_prev";
    pub const PAGE_NEXT: &str = "page_next";
    pub const PAGE_MENU: &str = "page_menu";
    pub const QUIT: &str = "quit";
}

/// Resolves a `Qt::Key` name as found in `frontend.toml` (e.g. `"Left"`,
/// `"Return"`) to the numeric key code Qt emits at runtime. Returns None
/// for unknown names so the caller can warn and skip.
#[must_use]
pub fn qt_key_code(name: &str) -> Option<i32> {
    // Subset that covers every action in the default bindings plus a few
    // common aliases. Extend as new actions land. Values match Qt::Key.
    match name {
        "Left" => Some(0x0100_0012),
        "Right" => Some(0x0100_0014),
        "Up" => Some(0x0100_0013),
        "Down" => Some(0x0100_0015),
        "Return" => Some(0x0100_0004),
        "Enter" => Some(0x0100_0005),
        "Escape" => Some(0x0100_0000),
        "Backspace" => Some(0x0100_0003),
        "PageUp" => Some(0x0100_0016),
        "PageDown" => Some(0x0100_0017),
        "Space" => Some(0x20),
        "Tab" => Some(0x0100_0001),
        _ => None,
    }
}

/// Default action → Qt-key-name list. Merged with `[input.keyboard]`
/// overrides from `frontend.toml`: a user-provided list replaces the
/// default for that action (not merged), so emptying a list unbinds it.
#[must_use]
pub fn default_bindings() -> HashMap<String, Vec<String>> {
    let mut map: HashMap<String, Vec<String>> = HashMap::new();
    map.insert(actions::LEFT.into(), vec!["Left".into()]);
    map.insert(actions::RIGHT.into(), vec!["Right".into()]);
    map.insert(actions::UP.into(), vec!["Up".into()]);
    map.insert(actions::DOWN.into(), vec!["Down".into()]);
    map.insert(
        actions::ACCEPT.into(),
        vec!["Return".into(), "Enter".into()],
    );
    map.insert(
        actions::CANCEL.into(),
        vec!["Escape".into(), "Backspace".into()],
    );
    map.insert(actions::CONTEXT_MENU.into(), vec!["Tab".into()]);
    map.insert(actions::PAGE_PREV.into(), vec!["PageUp".into()]);
    map.insert(actions::PAGE_NEXT.into(), vec!["PageDown".into()]);
    map.insert(actions::PAGE_MENU.into(), vec!["Space".into()]);
    map
}

/// Inverts the bindings (action → keys) into the runtime lookup shape
/// ([`Qt::Key`] code → action). When two actions bind the same key the
/// alphabetically-later action wins — a deterministic, hand-authored
/// collision policy. We sort the entries before walking them because
/// `HashMap` iteration order is randomized per process, which would
/// otherwise let two runs disagree on which action owns a contested key.
#[must_use]
pub fn invert<S>(bindings: &HashMap<String, Vec<String>, S>) -> HashMap<i32, String>
where
    S: std::hash::BuildHasher,
{
    let mut out: HashMap<i32, String> = HashMap::new();
    let mut ordered: Vec<(&String, &Vec<String>)> = bindings.iter().collect();
    ordered.sort_unstable_by_key(|(a, _)| *a);
    for (action, keys) in ordered {
        for name in keys {
            if let Some(code) = qt_key_code(name) {
                out.insert(code, action.clone());
            } else {
                tracing::warn!("unknown Qt key name in input binding: {name}");
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::unwrap_used,
        reason = "tests should fail-fast on unexpected errors"
    )]

    use super::{actions, default_bindings, invert, qt_key_code};

    #[test]
    fn every_default_binding_is_non_empty() {
        // Iterating the whole map (rather than a hard-coded subset)
        // catches future drift: any action added to default_bindings()
        // must come with at least one key, or this test fails.
        let b = default_bindings();
        assert!(
            !b.is_empty(),
            "default_bindings() must define at least one action"
        );
        for (action, keys) in &b {
            assert!(!keys.is_empty(), "missing default binding for {action}");
        }
    }

    #[test]
    fn invert_produces_unique_key_to_action_map() {
        let map = invert(&default_bindings());
        assert_eq!(
            map.get(&qt_key_code("Left").unwrap()).map(String::as_str),
            Some(actions::LEFT),
        );
        assert_eq!(
            map.get(&qt_key_code("Return").unwrap()).map(String::as_str),
            Some(actions::ACCEPT),
        );
        assert_eq!(
            map.get(&qt_key_code("Escape").unwrap()).map(String::as_str),
            Some(actions::CANCEL),
        );
        assert_eq!(
            map.get(&qt_key_code("Tab").unwrap()).map(String::as_str),
            Some(actions::CONTEXT_MENU),
        );
        assert_eq!(
            map.get(&qt_key_code("Space").unwrap()).map(String::as_str),
            Some(actions::PAGE_MENU),
        );
    }

    #[test]
    fn collision_resolution_is_deterministic_alphabetical_winner() {
        // Two actions bind "Return" — `accept` (alphabetically earlier)
        // and `zzz_late` (alphabetically later). With sorted iteration
        // the later-walked entry overwrites, so `zzz_late` must always
        // win regardless of process-local hash seed.
        let mut b = std::collections::HashMap::new();
        b.insert(actions::ACCEPT.into(), vec!["Return".into()]);
        b.insert("zzz_late".to_string(), vec!["Return".into()]);
        let map = invert(&b);
        assert_eq!(
            map.get(&qt_key_code("Return").unwrap()).map(String::as_str),
            Some("zzz_late"),
        );
    }

    #[test]
    fn unknown_key_names_log_warning_and_are_skipped() {
        let mut b = default_bindings();
        b.insert("fictional".into(), vec!["NotAKey".into()]);
        // invert logs a warning and keeps going; the result holds no entry
        // for the fictional action.
        let map = invert(&b);
        assert!(!map.values().any(|v| v == "fictional"));
    }
}
