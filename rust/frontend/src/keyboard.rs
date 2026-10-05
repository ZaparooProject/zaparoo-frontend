// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// The on-screen keyboard's state for one text field: the layer on show,
// the focused key and the text. The owning screen routes input to it and
// publishes `cells()` to an `OnScreenKeyboard`. Layout, navigation and
// editing rules live in `zaparoo_app::keyboard`.

use slint::SharedString;
use zaparoo_app::keyboard::{self as rules, Cell, Edge, Key, Layer, Layers, Position, TextBuffer};

/// What pressing a key did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Press {
    /// Nothing changed (a refused character, an empty backspace).
    None,
    Edited,
    /// The layer on show changed; the cells need republishing.
    Layer,
    Submit,
}

#[derive(Debug, Clone)]
pub struct Keyboard {
    layers: Layers,
    layer: Layer,
    position: Position,
    buffer: TextBuffer,
}

impl Keyboard {
    pub fn new(layers: Layers, max_chars: usize) -> Self {
        Self {
            layers,
            layer: Layer::default(),
            // The home row's first key: the shortest reach to most letters.
            position: Position { row: 2, index: 0 },
            buffer: TextBuffer::new(max_chars),
        }
    }

    fn rows(&self) -> Vec<Vec<Cell>> {
        rules::rows(self.layers, self.layer)
    }

    pub fn row_count(&self) -> usize {
        self.rows().len()
    }

    pub fn layer(&self) -> Layer {
        self.layer
    }

    pub fn text(&self) -> &str {
        self.buffer.text()
    }

    pub fn set_text(&mut self, text: &str) {
        self.buffer.set(text);
    }

    /// The text around the cursor, as the field draws it: what comes
    /// before it, the character it sits on (empty at the end) and the rest.
    /// A space beside the cursor becomes a no-break space so it keeps its
    /// width.
    pub fn display(&self) -> (SharedString, SharedString, SharedString) {
        const NBSP: &str = "\u{a0}";
        let (before, rest) = self.buffer.split();
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
                        if self.layer == Layer::Symbols {
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
            Some(key @ (Key::Shift | Key::Symbols)) => {
                self.layer = rules::toggle(self.layer, key);
                self.position = rules::clamp(&self.rows(), self.position);
                Press::Layer
            }
            Some(Key::Submit) => Press::Submit,
            None => Press::None,
        }
    }

    pub fn insert(&mut self, c: char) -> Press {
        if self.buffer.insert(c) {
            Press::Edited
        } else {
            Press::None
        }
    }

    pub fn backspace(&mut self) -> Press {
        if self.buffer.backspace() {
            Press::Edited
        } else {
            Press::None
        }
    }

    pub fn move_caret(&mut self, forward: bool) -> bool {
        self.buffer.move_caret(forward)
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
    fn the_full_keyboard_switches_layers_and_keeps_focus_in_range() {
        let mut keyboard = Keyboard::new(Layers::Full, 16);
        assert_eq!(keyboard.row_count(), 5);
        keyboard.focus(Key::Shift);
        assert_eq!(keyboard.press(), Press::Layer);
        assert_eq!(keyboard.layer(), Layer::Upper);
        keyboard.focus(Key::Char('Q'));
        assert_eq!(keyboard.press(), Press::Edited);
        assert_eq!(keyboard.text(), "Q");
        keyboard.focus(Key::Symbols);
        assert_eq!(keyboard.press(), Press::Layer);
        assert_eq!(keyboard.layer(), Layer::Symbols);
        assert!(keyboard.key().is_some());
    }
}
