// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

//! Writing a deck's games to tokens, one token per game.
//!
//! The batch holds only the items a token can carry: an online card is
//! already a token of its own, and an item with no script has nothing to
//! write. Each item is one step of the write panel. A written or skipped
//! token moves on; a failed one stays on the same item so the user can hold
//! the token again or skip it. The batch is done once every item has had
//! its turn.

/// One deck item, as the batch sees it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Item {
    pub name: String,
    pub system: String,
    pub path: String,
    /// What the token will run.
    pub zapscript: String,
    /// An online card: a token in its own right, never rewritten here.
    pub online_card: bool,
}

impl Item {
    fn writable(&self) -> bool {
        !self.online_card && !self.zapscript.trim().is_empty()
    }
}

/// How one token's turn ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Written,
    Skipped,
    Failed,
}

/// A batch in progress.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Batch {
    items: Vec<Item>,
    index: usize,
    failed: bool,
    written: usize,
}

impl Batch {
    /// The batch for these deck items, or `None` when no item can be
    /// written.
    #[must_use]
    pub fn new(items: impl IntoIterator<Item = Item>) -> Option<Self> {
        let items: Vec<Item> = items.into_iter().filter(Item::writable).collect();
        (!items.is_empty()).then_some(Self {
            items,
            ..Self::default()
        })
    }

    /// The item whose token is being written; `None` once done.
    #[must_use]
    pub fn current(&self) -> Option<&Item> {
        self.items.get(self.index)
    }

    /// The step shown as "Token N of M": 1-based.
    #[must_use]
    pub fn step(&self) -> usize {
        (self.index + 1).min(self.items.len())
    }

    #[must_use]
    pub fn count(&self) -> usize {
        self.items.len()
    }

    /// The last write of the current item failed.
    #[must_use]
    pub const fn failed(&self) -> bool {
        self.failed
    }

    /// How many tokens were written so far.
    #[must_use]
    pub const fn written(&self) -> usize {
        self.written
    }

    /// The current item is being written again after its write failed.
    pub fn clear_failure(&mut self) {
        self.failed = false;
    }

    #[must_use]
    pub fn done(&self) -> bool {
        self.index >= self.items.len()
    }

    /// Record the current item's outcome. Returns whether there is an item
    /// left to write.
    pub fn record(&mut self, outcome: Outcome) -> bool {
        if self.done() {
            return false;
        }
        match outcome {
            Outcome::Written => {
                self.written += 1;
                self.failed = false;
                self.index += 1;
            }
            Outcome::Skipped => {
                self.failed = false;
                self.index += 1;
            }
            Outcome::Failed => self.failed = true,
        }
        !self.done()
    }
}

/// A button of the write panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Button {
    /// Write the current item again after its write failed.
    Retry,
    /// Leave the current item's token unwritten and move on.
    Skip,
    Cancel,
}

/// The write panel's buttons, left to right: a single write only cancels;
/// a batch skips or cancels, and offers the item again once its write
/// failed.
#[must_use]
pub fn buttons(batch: Option<&Batch>) -> Vec<Button> {
    match batch {
        None => vec![Button::Cancel],
        Some(batch) if batch.failed() => vec![Button::Retry, Button::Skip, Button::Cancel],
        Some(_) => vec![Button::Skip, Button::Cancel],
    }
}

#[cfg(test)]
#[allow(clippy::expect_used, reason = "tests should fail fast")]
mod tests {
    use super::*;

    #[test]
    fn a_failed_item_offers_a_retry_first() {
        assert_eq!(buttons(None), [Button::Cancel]);
        let mut batch = Batch::new([game("a")]).expect("batch");
        assert_eq!(buttons(Some(&batch)), [Button::Skip, Button::Cancel]);
        batch.record(Outcome::Failed);
        assert_eq!(
            buttons(Some(&batch)),
            [Button::Retry, Button::Skip, Button::Cancel]
        );
    }

    fn game(name: &str) -> Item {
        Item {
            name: name.into(),
            zapscript: format!("**launch:{name}"),
            ..Item::default()
        }
    }

    #[test]
    fn online_cards_and_empty_scripts_are_left_out() {
        let card = Item {
            online_card: true,
            ..game("card")
        };
        let empty = Item {
            zapscript: "  ".into(),
            ..game("empty")
        };
        let batch = Batch::new([game("a"), card, empty, game("b")]).expect("two writable");
        assert_eq!(batch.count(), 2);
        assert_eq!(batch.current().map(|i| i.name.as_str()), Some("a"));
        assert!(Batch::new([Item {
            online_card: true,
            ..game("c")
        }])
        .is_none());
        assert!(Batch::new([]).is_none());
    }

    #[test]
    fn written_and_skipped_move_on_failed_stays() {
        let mut batch = Batch::new([game("a"), game("b"), game("c")]).expect("batch");
        assert_eq!(batch.step(), 1);
        assert!(batch.record(Outcome::Failed));
        assert!(batch.failed());
        assert_eq!(batch.step(), 1);
        assert!(batch.record(Outcome::Written));
        assert!(!batch.failed());
        assert_eq!(batch.step(), 2);
        assert!(batch.record(Outcome::Skipped));
        assert_eq!(batch.current().map(|i| i.name.as_str()), Some("c"));
        assert!(!batch.record(Outcome::Written));
        assert!(batch.done());
        assert_eq!(batch.written(), 2);
        assert!(batch.current().is_none());
        assert_eq!(batch.step(), 3);
        assert!(!batch.record(Outcome::Written));
        assert_eq!(batch.written(), 2);
    }
}
