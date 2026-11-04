// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// The on-screen keyboard's state for one text field: the layer on show,
// the focused key and the text. The owning screen routes input to it and
// publishes `cells()` to an `OnScreenKeyboard`. Layout, navigation and
// editing rules live in `zaparoo_app::keyboard`.

use slint::SharedString;
use zaparoo_app::keyboard::{
    self as rules, Case, Cell, Edge, Key, Layer, Layers, Mask, Position, Shift, TextBuffer,
};

/// What pressing a key did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Press {
    /// Nothing changed (a refused character, an empty backspace).
    None,
    Edited,
    /// The layer on show changed; the cells need republishing.
    Layer,
    /// The caret moved; the field needs repainting.
    Moved,
    Submit,
}

#[derive(Debug, Clone)]
pub struct Keyboard {
    layers: Layers,
    case: Case,
    position: Position,
    buffer: TextBuffer,
    /// A password field: every character drawn as a dot but the one just
    /// typed. `None` for a field shown as typed.
    mask: Option<Mask>,
}

impl Keyboard {
    pub fn new(layers: Layers, max_chars: usize) -> Self {
        Self {
            layers,
            case: Case::default(),
            // The home row's first key: the shortest reach to most letters.
            position: Position { row: 2, index: 0 },
            buffer: TextBuffer::new(max_chars),
            mask: None,
        }
    }

    /// A password field: spaces kept as typed, and drawn masked.
    pub fn masked(layers: Layers, max_chars: usize) -> Self {
        Self {
            buffer: TextBuffer::verbatim(max_chars),
            mask: Some(Mask::default()),
            ..Self::new(layers, max_chars)
        }
    }

    fn hide(&mut self) {
        if let Some(mask) = &mut self.mask {
            mask.hide();
        }
    }

    /// The ticket of the character on show, while a password field shows
    /// one; its timer hands it to `expire_reveal`.
    pub fn reveal_ticket(&self) -> Option<u64> {
        self.mask
            .filter(|mask| mask.revealed().is_some())
            .map(|mask| mask.ticket())
    }

    /// The reveal timer ran out: true when the field needs repainting.
    pub fn expire_reveal(&mut self, ticket: u64) -> bool {
        self.mask.as_mut().is_some_and(|mask| mask.expire(ticket))
    }

    fn rows(&self) -> Vec<Vec<Cell>> {
        rules::rows(self.layers, self.case.layer)
    }

    pub fn row_count(&self) -> usize {
        self.rows().len()
    }

    pub fn layer(&self) -> Layer {
        self.case.layer
    }

    pub fn shift(&self) -> Shift {
        self.case.shift
    }

    /// Shift held down: caps until it is tapped again.
    pub fn lock_shift(&mut self) {
        self.case.lock_shift();
        self.position = rules::clamp(&self.rows(), self.position);
    }

    pub fn text(&self) -> &str {
        self.buffer.text()
    }

    pub fn set_text(&mut self, text: &str) {
        self.buffer.set(text);
        self.hide();
    }

    pub fn caret(&self) -> usize {
        self.buffer.caret()
    }

    pub fn set_caret(&mut self, caret: usize) {
        self.buffer.set_caret(caret);
        self.hide();
    }

    /// The text around the cursor, as the field draws it: what comes
    /// before it, the character it sits on (empty at the end) and the rest.
    /// A space beside the cursor becomes a no-break space so it keeps its
    /// width.
    pub fn display(&self) -> (SharedString, SharedString, SharedString) {
        const NBSP: &str = "\u{a0}";
        let shown;
        let (before, rest) = match self.mask {
            Some(mask) => {
                shown = rules::masked(self.buffer.text(), mask.revealed());
                let at = shown
                    .char_indices()
                    .nth(self.buffer.caret())
                    .map_or(shown.len(), |(byte, _)| byte);
                shown.split_at(at)
            }
            None => self.buffer.split(),
        };
        let before = before
            .strip_suffix(' ')
            .map_or_else(|| before.to_string(), |head| format!("{head}{NBSP}"));
        let mut chars = rest.chars();
        let at = match chars.next() {
            Some(' ') => NBSP.to_string(),
            Some(c) => c.to_string(),
            None => String::new(),
        };
        (
            SharedString::from(before),
            SharedString::from(at),
            SharedString::from(chars.as_str()),
        )
    }

    /// Empty the field.
    pub fn clear(&mut self) -> Press {
        self.hide();
        if self.buffer.clear() {
            Press::Edited
        } else {
            Press::None
        }
    }

    /// The keys in row order, for the view.
    pub fn cells(&self) -> Vec<crate::KeyCell> {
        let mut out = Vec::new();
        for (row, cells) in self.rows().iter().enumerate() {
            let mut start = 0;
            for cell in cells {
                let (kind, text) = match cell.key {
                    Key::Char(c) => (crate::KeyKind::Char, c.to_string()),
                    Key::Space => (crate::KeyKind::Space, String::new()),
                    Key::Backspace => (crate::KeyKind::Backspace, String::new()),
                    Key::Shift => (crate::KeyKind::Shift, String::new()),
                    Key::Symbols => (
                        crate::KeyKind::Symbols,
                        if self.case.layer == Layer::Symbols {
                            "abc"
                        } else {
                            "#+="
                        }
                        .to_string(),
                    ),
                    Key::Submit => (crate::KeyKind::Submit, String::new()),
                };
                out.push(crate::KeyCell {
                    kind,
                    text: SharedString::from(text),
                    row: i32::try_from(row).unwrap_or(0),
                    start: i32::try_from(start).unwrap_or(0),
                    span: i32::try_from(cell.span).unwrap_or(1),
                });
                start += cell.span;
            }
        }
        out
    }

    /// The focused key's place in `cells()`.
    pub fn index(&self) -> usize {
        let rows = self.rows();
        let position = rules::clamp(&rows, self.position);
        rows.iter().take(position.row).map(Vec::len).sum::<usize>() + position.index
    }

    /// Focus the key at `index` in `cells()`; false when there is none.
    pub fn set_index(&mut self, index: usize) -> bool {
        let mut remaining = index;
        for (row, cells) in self.rows().iter().enumerate() {
            if remaining < cells.len() {
                self.position = Position {
                    row,
                    index: remaining,
                };
                return true;
            }
            remaining -= cells.len();
        }
        false
    }

    /// The focused key.
    pub fn key(&self) -> Option<Key> {
        let rows = self.rows();
        let position = rules::clamp(&rows, self.position);
        rows.get(position.row)
            .and_then(|row| row.get(position.index))
            .map(|cell| cell.key)
    }

    /// Seat focus on the first key of a kind.
    pub fn focus(&mut self, key: Key) {
        if let Some(position) = rules::find(&self.rows(), key) {
            self.position = position;
        }
    }

    /// The middle unit of the focused key along its row.
    pub fn unit(&self) -> usize {
        rules::middle_unit(&self.rows(), self.position)
    }

    /// Bring focus onto the top or bottom row, under where it was.
    pub fn enter_row(&mut self, bottom: bool) {
        let row = if bottom { usize::MAX } else { 0 };
        self.position = rules::seat_row(&self.rows(), self.position, row);
    }

    /// Bring focus to the first key of its row.
    pub fn row_start(&mut self) {
        self.position.index = 0;
    }

    /// Move focus one key; the edge when nothing lies that way.
    pub fn step(&mut self, action: &str, wrap: bool) -> Option<Edge> {
        match rules::step(&self.rows(), self.position, action, wrap) {
            rules::Step::To(position) => {
                self.position = position;
                None
            }
            rules::Step::Exit(edge) => Some(edge),
        }
    }

    /// Press the focused key.
    pub fn press(&mut self) -> Press {
        match self.key() {
            Some(Key::Char(c)) => self.insert(c),
            Some(Key::Space) => self.insert(' '),
            Some(Key::Backspace) => self.backspace(),
            Some(Key::Shift) => {
                self.case.tap_shift();
                self.position = rules::clamp(&self.rows(), self.position);
                Press::Layer
            }
            Some(Key::Symbols) => {
                self.case.tap_symbols();
                self.position = rules::clamp(&self.rows(), self.position);
                Press::Layer
            }
            Some(Key::Submit) => Press::Submit,
            None => Press::None,
        }
    }

    /// Type one character. A one-character Shift lets go once it has.
    pub fn insert(&mut self, c: char) -> Press {
        if !self.buffer.insert(c) {
            return Press::None;
        }
        let caret = self.buffer.caret();
        if let Some(mask) = &mut self.mask {
            mask.typed(caret - 1);
        }
        let layer = self.case.layer;
        self.case.typed();
        if self.case.layer != layer {
            self.position = rules::clamp(&self.rows(), self.position);
        }
        Press::Edited
    }

    pub fn backspace(&mut self) -> Press {
        self.hide();
        if self.buffer.backspace() {
            Press::Edited
        } else {
            Press::None
        }
    }

    pub fn move_caret(&mut self, forward: bool) -> bool {
        self.hide();
        self.buffer.move_caret(forward)
    }

    /// The focused key's kind, for the help bar.
    pub fn key_kind(&self) -> crate::KeyKind {
        match self.key() {
            Some(Key::Submit) => crate::KeyKind::Submit,
            Some(Key::Space) => crate::KeyKind::Space,
            Some(Key::Backspace) => crate::KeyKind::Backspace,
            Some(Key::Shift) => crate::KeyKind::Shift,
            Some(Key::Symbols) => crate::KeyKind::Symbols,
            Some(Key::Char(_)) | None => crate::KeyKind::Char,
        }
    }

    /// Whether Accept on the focused key repeats while held: a key that
    /// types or deletes, never submit or a layer key.
    pub fn key_repeats(&self) -> bool {
        matches!(self.key(), Some(Key::Char(_) | Key::Space | Key::Backspace))
    }

    /// Delete the character before the cursor. A delete held long enough
    /// stops stepping and empties the field.
    pub fn delete(&mut self, held_ms: u64) -> Press {
        if held_ms >= zaparoo_app::input::TEXT_CLEAR_HOLD_MS {
            self.clear()
        } else {
            self.backspace()
        }
    }

    /// The text-editing buttons every keyboard screen shares: a held Accept
    /// on a key, Delete (and Menu), Options as a space, and the shoulders
    /// moving the caret. `None` for any other action, which the screen
    /// handles itself. `held_ms` is how long the repeat in flight has
    /// been held.
    pub fn text_action(&mut self, action: &str, held_ms: u64) -> Option<Press> {
        use zaparoo_core::input_actions::actions;
        Some(match action {
            zaparoo_app::input::TEXT_KEY => {
                if self.key() == Some(Key::Backspace) {
                    self.delete(held_ms)
                } else {
                    self.press()
                }
            }
            zaparoo_app::input::TEXT_DELETE | actions::PAGE_MENU => self.delete(held_ms),
            actions::CONTEXT_MENU => self.insert(' '),
            actions::PAGE_PREV | actions::PAGE_NEXT => {
                if self.move_caret(action == actions::PAGE_NEXT) {
                    Press::Moved
                } else {
                    Press::None
                }
            }
            _ => return None,
        })
    }
}

/// One keyboard's press cue: the key held down and the ticket that lifts it.
#[derive(Debug, Clone, Copy, Default)]
pub struct Flash {
    pub pressed: Option<usize>,
    seq: u64,
}

impl Flash {
    /// Hold `index` down; the returned ticket lifts it.
    pub fn start(&mut self, index: usize) -> u64 {
        self.pressed = Some(index);
        self.seq = self.seq.wrapping_add(1);
        self.seq
    }

    /// Lift the key if `seq` is still the latest press.
    pub fn lift(&mut self, seq: u64) -> bool {
        if self.seq != seq {
            return false;
        }
        self.pressed = None;
        true
    }

    pub fn clear(&mut self) {
        self.pressed = None;
    }

    pub fn index(&self) -> i32 {
        self.pressed
            .and_then(|index| i32::try_from(index).ok())
            .unwrap_or(-1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn basic() -> Keyboard {
        Keyboard::new(Layers::Basic, 16)
    }

    #[test]
    fn cells_place_every_key_on_its_row_and_units() {
        let cells = basic().cells();
        assert_eq!(cells.len(), 10 + 10 + 10 + 9);
        let submit = &cells[38];
        assert_eq!(submit.kind, crate::KeyKind::Submit);
        assert_eq!((submit.row, submit.start, submit.span), (3, 8, 2));
        let backspace = &cells[29];
        assert_eq!(backspace.kind, crate::KeyKind::Backspace);
        assert_eq!((backspace.row, backspace.start), (2, 9));
    }

    #[test]
    fn index_and_position_agree_across_rows() {
        let mut keyboard = basic();
        assert_eq!(keyboard.index(), 20);
        assert_eq!(keyboard.key(), Some(Key::Char('a')));
        assert!(keyboard.set_index(38));
        assert_eq!(keyboard.key(), Some(Key::Submit));
        assert_eq!(keyboard.index(), 38);
        assert!(!keyboard.set_index(39));
        keyboard.focus(Key::Space);
        assert_eq!(keyboard.index(), 37);
    }

    #[test]
    fn focus_enters_a_row_under_where_it_was_and_can_go_to_its_start() {
        let mut keyboard = basic();
        keyboard.focus(Key::Char('8'));
        assert_eq!(keyboard.unit(), 7);
        keyboard.enter_row(true);
        assert_eq!(keyboard.key(), Some(Key::Space));
        keyboard.enter_row(false);
        assert_eq!(keyboard.key(), Some(Key::Char('8')));
        keyboard.focus(Key::Submit);
        keyboard.row_start();
        assert_eq!(keyboard.key(), Some(Key::Char('z')));
    }

    #[test]
    fn pressing_keys_types_and_reports_submit() {
        let mut keyboard = basic();
        assert_eq!(keyboard.press(), Press::Edited);
        assert_eq!(keyboard.step("right", false), None);
        assert_eq!(keyboard.press(), Press::Edited);
        assert_eq!(keyboard.text(), "as");
        keyboard.focus(Key::Backspace);
        assert_eq!(keyboard.press(), Press::Edited);
        assert_eq!(keyboard.press(), Press::Edited);
        assert_eq!(keyboard.press(), Press::None);
        keyboard.focus(Key::Submit);
        assert_eq!(keyboard.press(), Press::Submit);
        assert_eq!(keyboard.step("right", false), Some(Edge::Right));
        assert_eq!(keyboard.step("right", true), None);
        assert_eq!(keyboard.key(), Some(Key::Char('z')));
    }

    #[test]
    fn the_cursor_sits_on_a_character_and_spaces_beside_it_keep_their_width() {
        let mut keyboard = basic();
        keyboard.set_text("mario kart");
        let parts = |k: &Keyboard| {
            let (before, at, after) = k.display();
            (before.to_string(), at.to_string(), after.to_string())
        };
        assert_eq!(
            parts(&keyboard),
            ("mario kart".into(), String::new(), String::new())
        );
        for _ in 0..4 {
            keyboard.move_caret(false);
        }
        assert_eq!(
            parts(&keyboard),
            ("mario\u{a0}".into(), "k".into(), "art".into())
        );
        keyboard.move_caret(false);
        assert_eq!(
            parts(&keyboard),
            ("mario".into(), "\u{a0}".into(), "kart".into())
        );
        assert_eq!(keyboard.clear(), Press::Edited);
        assert_eq!(keyboard.clear(), Press::None);
        assert_eq!(
            parts(&keyboard),
            (String::new(), String::new(), String::new())
        );
    }

    #[test]
    fn the_shared_text_buttons_delete_space_and_move_the_caret() {
        use zaparoo_core::input_actions::actions;
        let mut keyboard = basic();
        keyboard.set_text("ab");
        assert_eq!(
            keyboard.text_action(actions::CONTEXT_MENU, 0),
            Some(Press::Edited)
        );
        assert_eq!(keyboard.text(), "ab ");
        assert_eq!(
            keyboard.text_action(zaparoo_app::input::TEXT_DELETE, 0),
            Some(Press::Edited)
        );
        assert_eq!(
            keyboard.text_action(actions::PAGE_PREV, 0),
            Some(Press::Moved)
        );
        assert_eq!(keyboard.caret(), 1);
        assert_eq!(
            keyboard.text_action(actions::PAGE_MENU, zaparoo_app::input::TEXT_CLEAR_HOLD_MS),
            Some(Press::Edited)
        );
        assert_eq!(keyboard.text(), "", "a long hold clears the field");
        assert_eq!(keyboard.text_action(actions::UP, 0), None);
        keyboard.focus(Key::Backspace);
        assert!(keyboard.key_repeats());
        keyboard.focus(Key::Submit);
        assert!(!keyboard.key_repeats());
        assert_eq!(keyboard.key_kind(), crate::KeyKind::Submit);
    }

    #[test]
    fn a_flash_lifts_only_for_its_own_press() {
        let mut flash = Flash::default();
        let first = flash.start(3);
        let second = flash.start(4);
        assert!(!flash.lift(first));
        assert_eq!(flash.index(), 4);
        assert!(flash.lift(second));
        assert_eq!(flash.index(), -1);
    }

    #[test]
    fn the_full_keyboard_switches_layers_and_keeps_focus_in_range() {
        let mut keyboard = Keyboard::new(Layers::Full, 16);
        assert_eq!(keyboard.row_count(), 5);
        keyboard.focus(Key::Shift);
        assert_eq!(keyboard.press(), Press::Layer);
        assert_eq!(keyboard.layer(), Layer::Upper);
        keyboard.focus(Key::Char('Q'));
        assert_eq!(keyboard.press(), Press::Edited);
        assert_eq!(keyboard.text(), "Q");
        assert_eq!(
            keyboard.layer(),
            Layer::Lower,
            "one character, then lowercase"
        );
        assert_eq!(keyboard.key(), Some(Key::Char('q')));
        keyboard.lock_shift();
        keyboard.focus(Key::Char('W'));
        keyboard.press();
        keyboard.press();
        assert_eq!(keyboard.text(), "QWW", "caps stay on");
        assert_eq!(keyboard.shift(), Shift::Locked);
        keyboard.focus(Key::Symbols);
        assert_eq!(keyboard.press(), Press::Layer);
        assert_eq!(keyboard.layer(), Layer::Symbols);
        assert!(keyboard.key().is_some());
    }

    #[test]
    #[allow(clippy::unwrap_used, reason = "a missing reveal is a test failure")]
    fn a_masked_field_shows_only_the_newest_character() {
        let mut keyboard = Keyboard::masked(Layers::Full, 16);
        keyboard.insert('a');
        assert_eq!(keyboard.display(), ("a".into(), "".into(), "".into()));
        let first = keyboard.reveal_ticket().unwrap();
        keyboard.insert('b');
        assert_eq!(
            keyboard.display(),
            ("\u{2022}b".into(), "".into(), "".into())
        );
        assert!(!keyboard.expire_reveal(first));
        let second = keyboard.reveal_ticket().unwrap();
        assert!(keyboard.expire_reveal(second));
        assert_eq!(keyboard.reveal_ticket(), None);
        assert_eq!(
            keyboard.display(),
            ("\u{2022}\u{2022}".into(), "".into(), "".into())
        );
        keyboard.insert('c');
        keyboard.move_caret(false);
        assert_eq!(keyboard.reveal_ticket(), None, "a caret move hides it");
        assert_eq!(
            keyboard.display(),
            ("\u{2022}\u{2022}".into(), "\u{2022}".into(), "".into())
        );
        assert_eq!(keyboard.text(), "abc");
        assert_eq!(keyboard.insert(' '), Press::Edited);
        assert_eq!(
            keyboard.insert(' '),
            Press::Edited,
            "a password keeps doubled spaces"
        );
        let mut plain = basic();
        plain.insert('a');
        assert_eq!(plain.reveal_ticket(), None);
        assert_eq!(plain.display(), ("a".into(), "".into(), "".into()));
    }
}
