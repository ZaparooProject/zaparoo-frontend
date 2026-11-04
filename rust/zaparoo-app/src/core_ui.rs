// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// Rules for Core's UI events (`ui`, `ui.changed`, `ui.respond`): which
// state to believe, and what each kind of event puts on screen. The
// frontend adapter (`rust/frontend/src/core_ui.rs`) applies them.

/// Orders Core's UI state. Every `ui.changed` and every `ui` answer carries
/// the whole state with a revision that only grows while Core runs; an
/// older one arriving late (a query answered after a notification) is
/// stale. A new connection may be a restarted Core counting from zero, so
/// a connect starts the order over.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Revisions {
    newest: Option<u64>,
}

impl Revisions {
    /// Whether a state at `revision` is current. The same revision again
    /// is the same state, so it is accepted (applying it changes nothing).
    pub fn accept(&mut self, revision: u64) -> bool {
        if self.newest.is_some_and(|newest| revision < newest) {
            return false;
        }
        self.newest = Some(revision);
        true
    }

    /// A new connection: whatever Core says next is current.
    pub fn reset(&mut self) {
        self.newest = None;
    }
}

/// The one line a loader or notice shows: its message, or its title when
/// Core sent only that. Core's own renderer reads them the same way.
pub fn line_text<'a>(title: &'a str, message: &'a str) -> &'a str {
    let message = message.trim();
    if message.is_empty() {
        title.trim()
    } else {
        message
    }
}

/// Where focus starts in a picker: the choice Core preselected, else the
/// first.
pub fn initial_choice(ids: &[&str], selected: &str) -> usize {
    ids.iter().position(|id| *id == selected).unwrap_or(0)
}

/// One step through a picker's choices, wrapping at the ends like every
/// other list.
pub fn step(len: usize, index: usize, forward: bool) -> usize {
    if len == 0 {
        return 0;
    }
    let index = index.min(len - 1);
    if forward {
        (index + 1) % len
    } else {
        (index + len - 1) % len
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stale_states_are_dropped_and_a_connect_starts_over() {
        let mut revisions = Revisions::default();
        assert!(revisions.accept(5));
        assert!(revisions.accept(5));
        assert!(!revisions.accept(4));
        assert!(revisions.accept(9));
        revisions.reset();
        assert!(revisions.accept(1));
    }

    #[test]
    fn a_line_prefers_the_message() {
        assert_eq!(line_text("Title", " Body "), "Body");
        assert_eq!(line_text(" Title ", "  "), "Title");
        assert_eq!(line_text("", ""), "");
    }

    #[test]
    fn focus_starts_on_the_preselected_choice() {
        assert_eq!(initial_choice(&["a", "b", "c"], "b"), 1);
        assert_eq!(initial_choice(&["a", "b"], "z"), 0);
        assert_eq!(initial_choice(&[], "z"), 0);
    }

    #[test]
    fn steps_wrap() {
        assert_eq!(step(3, 2, true), 0);
        assert_eq!(step(3, 0, false), 2);
        assert_eq!(step(3, 7, true), 0);
        assert_eq!(step(0, 0, true), 0);
    }
}
