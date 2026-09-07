// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

//! Failed user actions, as `Main.qml` presents them: a stable kind plus
//! the one piece of context the copy needs, deduplicated while the same
//! failure is already on screen, and queued so a burst of failures is
//! read one alert at a time. The technical detail belongs in the log at
//! the call site; the alert only carries what a user can act on.
//! Ported from `models/action_error.rs` and `Main.qml`'s own queue.

use std::collections::VecDeque;

/// Every kind the alert vocabulary knows. An unknown kind still shows,
/// with the generic copy.
pub const KINDS: [&str; 12] = [
    "launch",
    "favorite",
    "add_to_hub",
    "media_index",
    "media_scrape",
    "media_scrapers",
    "media_cancel",
    "launcher",
    "alternate_discovery",
    "qr_code",
    "card_write",
    "setting",
];

/// A discovery that failed while the context menu still holds its
/// "Searching…" row has to close the menu first: an alert is the one
/// thing allowed above a modal, but at depth 2 Back would return to a
/// row that can never resolve.
pub fn closes_context_menu(kind: &str) -> bool {
    kind == "alternate_discovery"
}

/// One queued failure. The key is what deduplication compares, so the
/// same failure about a different item still gets its own alert.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub kind: String,
    pub context: String,
}

impl Entry {
    pub fn new(kind: &str, context: &str) -> Self {
        Self {
            kind: kind.to_string(),
            context: context.to_string(),
        }
    }

    pub fn key(&self) -> String {
        format!("{}:{}", self.kind, self.context)
    }
}

/// The alert queue: one failure on screen, the rest waiting.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ErrorQueue {
    showing: Option<Entry>,
    queued: VecDeque<Entry>,
}

impl ErrorQueue {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn showing(&self) -> Option<&Entry> {
        self.showing.as_ref()
    }

    /// Report a failure. `slot_free` says whether the alert surface is
    /// available right now: another dialog (the startup notice, the
    /// first-run gate) owns it otherwise, and the failure waits rather
    /// than shouldering that dialog aside. Answers the entry to put on
    /// screen now, or `None` when it was dropped as a duplicate or
    /// queued behind one already waiting.
    pub fn present(&mut self, kind: &str, context: &str, slot_free: bool) -> Option<Entry> {
        if kind.is_empty() {
            return None;
        }
        let entry = Entry::new(kind, context);
        let key = entry.key();
        if self.showing.as_ref().is_some_and(|e| e.key() == key)
            || self.queued.iter().any(|e| e.key() == key)
        {
            return None;
        }
        if !slot_free || self.showing.is_some() {
            self.queued.push_back(entry);
            return None;
        }
        self.showing = Some(entry.clone());
        Some(entry)
    }

    /// The alert was dismissed. Answers the next failure to show, if
    /// any queued up behind it.
    pub fn dismiss(&mut self) -> Option<Entry> {
        self.showing = None;
        self.take_next()
    }

    /// The alert surface came free again (the dialog that was holding
    /// it closed). Answers the next queued failure, if any.
    pub fn take_next(&mut self) -> Option<Entry> {
        if self.showing.is_some() {
            return None;
        }
        let next = self.queued.pop_front()?;
        self.showing = Some(next.clone());
        Some(next)
    }

    /// Forget everything, queued and shown (a screen the alerts belong
    /// to is going away).
    pub fn clear(&mut self) {
        self.showing = None;
        self.queued.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_failure_shows_and_the_next_one_queues() {
        let mut q = ErrorQueue::new();
        assert_eq!(
            q.present("launch", "Sonic", true),
            Some(Entry::new("launch", "Sonic"))
        );
        assert_eq!(q.present("favorite", "", true), None);
        assert_eq!(q.showing(), Some(&Entry::new("launch", "Sonic")));
        // Dismissing the first hands over the queued one.
        assert_eq!(q.dismiss(), Some(Entry::new("favorite", "")));
        assert_eq!(q.dismiss(), None);
        assert_eq!(q.showing(), None);
    }

    #[test]
    fn the_same_failure_twice_is_one_alert() {
        let mut q = ErrorQueue::new();
        q.present("launch", "Sonic", true);
        // Already on screen.
        assert_eq!(q.present("launch", "Sonic", true), None);
        // Already queued.
        q.present("setting", "", true);
        q.present("setting", "", true);
        assert_eq!(q.dismiss(), Some(Entry::new("setting", "")));
        assert_eq!(q.dismiss(), None);
        // The same kind about another item is its own failure.
        q.present("launch", "Sonic", true);
        assert_eq!(
            q.present("launch", "Altered Beast", true),
            None,
            "queued behind the first"
        );
        assert_eq!(q.dismiss(), Some(Entry::new("launch", "Altered Beast")));
    }

    #[test]
    fn queued_token_failure_keeps_retry_payload_and_can_fail_again() {
        let mut queue = ErrorQueue::new();
        queue.present("launch", "game", true);
        let payload = "**launch.system:SNES||/games/original.sfc";
        assert_eq!(queue.present("card_write", payload, false), None);
        assert_eq!(queue.dismiss(), Some(Entry::new("card_write", payload)));
        assert_eq!(queue.dismiss(), None);
        assert_eq!(
            queue.present("card_write", payload, true),
            Some(Entry::new("card_write", payload))
        );
    }

    #[test]
    fn an_empty_kind_is_not_an_alert() {
        let mut q = ErrorQueue::new();
        assert_eq!(q.present("", "context", true), None);
        assert_eq!(q.showing(), None);
    }

    #[test]
    fn a_failure_waits_while_another_dialog_holds_the_slot() {
        let mut q = ErrorQueue::new();
        // The startup notice is up: nothing shows over it.
        assert_eq!(q.present("media_index", "", false), None);
        assert_eq!(q.showing(), None);
        // Once it closes, the failure gets the surface.
        assert_eq!(q.take_next(), Some(Entry::new("media_index", "")));
        // And nothing else jumps in while it is up.
        assert_eq!(q.take_next(), None);
    }

    #[test]
    fn clearing_drops_the_backlog() {
        let mut q = ErrorQueue::new();
        q.present("launch", "Sonic", true);
        q.present("favorite", "", true);
        q.clear();
        assert_eq!(q.showing(), None);
        assert_eq!(q.dismiss(), None);
    }

    #[test]
    fn only_a_failed_discovery_closes_the_context_menu() {
        assert!(closes_context_menu("alternate_discovery"));
        assert!(!closes_context_menu("launch"));
        assert!(KINDS.contains(&"alternate_discovery"));
    }
}
