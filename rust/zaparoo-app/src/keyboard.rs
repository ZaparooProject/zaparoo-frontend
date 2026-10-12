// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

//! The on-screen keyboard: its key rows, how a direction moves between keys
//! of unequal width, and the text being edited.
//!
//! Every row is `COLUMNS` units wide. A key spans one or more units, so a
//! vertical move lands on whichever key sits under the middle of the one it
//! left.

/// Units across every row.
pub const COLUMNS: usize = 10;

/// What a key does when pressed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Char(char),
    Space,
    Backspace,
    Shift,
    Symbols,
    Submit,
}

/// Which characters the letter rows show.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Layer {
    #[default]
    Lower,
    Upper,
    Symbols,
}

/// The keys a use of the keyboard offers.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Layers {
    /// Lowercase letters and digits only: enough where case and punctuation
    /// do not matter.
    #[default]
    Basic,
    /// Adds the Shift and Symbols keys and their layers.
    Full,
}

/// One key and how many units it spans.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cell {
    pub key: Key,
    pub span: usize,
}

const fn cell(key: Key, span: usize) -> Cell {
    Cell { key, span }
}

fn chars(row: &str) -> Vec<Cell> {
    row.chars().map(|c| cell(Key::Char(c), 1)).collect()
}

/// The rows for a layer, top to bottom.
pub fn rows(layers: Layers, layer: Layer) -> Vec<Vec<Cell>> {
    let (digits, top, home, bottom) = match layer {
        Layer::Lower => ("1234567890", "qwertyuiop", "asdfghjkl", "zxcvbnm"),
        Layer::Upper => ("1234567890", "QWERTYUIOP", "ASDFGHJKL", "ZXCVBNM"),
        Layer::Symbols => ("!@#$%^&*()", "-_=+[]{}\\|", ";:'\"`~<>?", ",./€£¥°"),
    };
    let mut home_row = chars(home);
    home_row.push(cell(Key::Backspace, 1));
    let mut bottom_row = chars(bottom);
    let mut out = vec![chars(digits), chars(top), home_row];
    match layers {
        Layers::Basic => {
            bottom_row.push(cell(Key::Space, 1));
            bottom_row.push(cell(Key::Submit, 2));
            out.push(bottom_row);
        }
        Layers::Full => {
            bottom_row.extend(chars(match layer {
                Layer::Symbols => "§¿¡",
                Layer::Lower | Layer::Upper => ",.'",
            }));
            out.push(bottom_row);
            out.push(vec![
                cell(Key::Shift, 2),
                cell(Key::Symbols, 2),
                cell(Key::Space, 4),
                cell(Key::Submit, 2),
            ]);
        }
    }
    out
}

/// A key's place: row, then index within the row.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Position {
    pub row: usize,
    pub index: usize,
}

/// The edge a move ran off.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Edge {
    Up,
    Down,
    Left,
    Right,
}

/// The outcome of a directional press.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    To(Position),
    /// Nothing lies that way; the owner decides whether focus leaves.
    Exit(Edge),
}

fn start_unit(row: &[Cell], index: usize) -> usize {
    row.iter().take(index).map(|c| c.span).sum()
}

/// The key covering `unit`, or the last key when the row is short.
fn index_at(row: &[Cell], unit: usize) -> usize {
    let mut start = 0;
    for (index, cell) in row.iter().enumerate() {
        if unit < start + cell.span {
            return index;
        }
        start += cell.span;
    }
    row.len().saturating_sub(1)
}

/// Clamp a position onto the rows, for a layer change that reshapes them.
pub fn clamp(rows: &[Vec<Cell>], position: Position) -> Position {
    let row = position.row.min(rows.len().saturating_sub(1));
    let len = rows.get(row).map_or(0, Vec::len);
    Position {
        row,
        index: position.index.min(len.saturating_sub(1)),
    }
}

/// Move one key in a direction. Horizontal moves wrap within the row when
/// `wrap` is set and otherwise report the edge; vertical moves always report
/// the edge.
pub fn step(rows: &[Vec<Cell>], position: Position, action: &str, wrap: bool) -> Step {
    let position = clamp(rows, position);
    let Some(row) = rows.get(position.row) else {
        return Step::To(position);
    };
    match action {
        "left" => {
            if position.index > 0 {
                Step::To(Position {
                    index: position.index - 1,
                    ..position
                })
            } else if wrap {
                Step::To(Position {
                    index: row.len().saturating_sub(1),
                    ..position
                })
            } else {
                Step::Exit(Edge::Left)
            }
        }
        "right" => {
            if position.index + 1 < row.len() {
                Step::To(Position {
                    index: position.index + 1,
                    ..position
                })
            } else if wrap {
                Step::To(Position {
                    index: 0,
                    ..position
                })
            } else {
                Step::Exit(Edge::Right)
            }
        }
        "up" | "down" => {
            let target = if action == "up" {
                match position.row.checked_sub(1) {
                    Some(row) => row,
                    None => return Step::Exit(Edge::Up),
                }
            } else if position.row + 1 < rows.len() {
                position.row + 1
            } else {
                return Step::Exit(Edge::Down);
            };
            let start = start_unit(row, position.index);
            let span = row.get(position.index).map_or(1, |c| c.span);
            let middle = start + (span - 1) / 2;
            Step::To(Position {
                row: target,
                index: index_at(&rows[target], middle),
            })
        }
        _ => Step::To(position),
    }
}

/// Where a key sits, for the view: its first unit and its span.
pub fn placement(rows: &[Vec<Cell>], position: Position) -> (usize, usize) {
    let position = clamp(rows, position);
    rows.get(position.row).map_or((0, 1), |row| {
        (
            start_unit(row, position.index),
            row.get(position.index).map_or(1, |c| c.span),
        )
    })
}

/// The middle unit of the key at `position`.
pub fn middle_unit(rows: &[Vec<Cell>], position: Position) -> usize {
    let (start, span) = placement(rows, position);
    start + (span - 1) / 2
}

/// The key of `row` under the one at `position`, for focus arriving on a
/// row from outside the keyboard. A row past the end is the last one.
pub fn seat_row(rows: &[Vec<Cell>], position: Position, row: usize) -> Position {
    let row = row.min(rows.len().saturating_sub(1));
    let unit = middle_unit(rows, position);
    Position {
        row,
        index: rows.get(row).map_or(0, |cells| index_at(cells, unit)),
    }
}

/// The first key of a kind, for seating focus on it.
pub fn find(rows: &[Vec<Cell>], key: Key) -> Option<Position> {
    rows.iter().enumerate().find_map(|(row, cells)| {
        cells
            .iter()
            .position(|c| c.key == key)
            .map(|index| Position { row, index })
    })
}

/// The layer after a Shift or Symbols press: each toggles its own layer.
pub fn toggle(layer: Layer, key: Key) -> Layer {
    match (key, layer) {
        (Key::Shift, Layer::Upper) | (Key::Symbols, Layer::Symbols) => Layer::Lower,
        (Key::Shift, _) => Layer::Upper,
        (Key::Symbols, _) => Layer::Symbols,
        _ => layer,
    }
}

/// Whether a character from a physical keyboard is text rather than a
/// control code.
pub fn is_text(c: char) -> bool {
    !c.is_control() && !('\u{E000}'..='\u{F8FF}').contains(&c)
}

/// The text being edited, with a caret between characters.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TextBuffer {
    text: String,
    /// Caret position in characters, `0..=len`.
    caret: usize,
    max_chars: usize,
}

impl TextBuffer {
    pub fn new(max_chars: usize) -> Self {
        Self {
            max_chars,
            ..Self::default()
        }
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub fn caret(&self) -> usize {
        self.caret
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    fn len(&self) -> usize {
        self.text.chars().count()
    }

    fn byte_at(&self, chars: usize) -> usize {
        self.text
            .char_indices()
            .nth(chars)
            .map_or(self.text.len(), |(byte, _)| byte)
    }

    /// The text either side of the caret.
    pub fn split(&self) -> (&str, &str) {
        self.text.split_at(self.byte_at(self.caret))
    }

    /// Empty the field; false when it already was.
    pub fn clear(&mut self) -> bool {
        if self.text.is_empty() {
            return false;
        }
        self.text.clear();
        self.caret = 0;
        true
    }

    /// Replace the text, caret at the end.
    pub fn set(&mut self, text: &str) {
        self.text = text.chars().take(self.max_chars).collect();
        self.caret = self.len();
    }

    /// Insert at the caret. A leading space, a doubled space, or a
    /// character past the limit is refused.
    pub fn insert(&mut self, c: char) -> bool {
        if !is_text(c) || self.len() >= self.max_chars {
            return false;
        }
        if c == ' ' {
            let (before, after) = self.split();
            if before.is_empty() || before.ends_with(' ') || after.starts_with(' ') {
                return false;
            }
        }
        let at = self.byte_at(self.caret);
        self.text.insert(at, c);
        self.caret += 1;
        true
    }

    /// Delete the character before the caret.
    pub fn backspace(&mut self) -> bool {
        if self.caret == 0 {
            return false;
        }
        let at = self.byte_at(self.caret - 1);
        self.text.remove(at);
        self.caret -= 1;
        true
    }

    /// Move the caret one character; false at either end.
    pub fn move_caret(&mut self, forward: bool) -> bool {
        if forward {
            if self.caret >= self.len() {
                return false;
            }
            self.caret += 1;
        } else {
            if self.caret == 0 {
                return false;
            }
            self.caret -= 1;
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(row: usize, index: usize) -> Position {
        Position { row, index }
    }

    #[test]
    fn every_row_of_every_layer_is_ten_units_wide() {
        for layers in [Layers::Basic, Layers::Full] {
            for layer in [Layer::Lower, Layer::Upper, Layer::Symbols] {
                for row in rows(layers, layer) {
                    assert_eq!(
                        row.iter().map(|c| c.span).sum::<usize>(),
                        COLUMNS,
                        "{layers:?} {layer:?} {row:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn basic_rows_end_in_space_and_submit_and_offer_no_layer_keys() {
        let rows = rows(Layers::Basic, Layer::Lower);
        assert_eq!(rows.len(), 4);
        assert_eq!(find(&rows, Key::Space), Some(at(3, 7)));
        assert_eq!(find(&rows, Key::Submit), Some(at(3, 8)));
        assert_eq!(find(&rows, Key::Backspace), Some(at(2, 9)));
        assert_eq!(find(&rows, Key::Shift), None);
        assert_eq!(find(&rows, Key::Symbols), None);
    }

    #[test]
    fn horizontal_moves_wrap_or_report_the_edge() {
        let rows = rows(Layers::Basic, Layer::Lower);
        assert_eq!(step(&rows, at(1, 3), "right", false), Step::To(at(1, 4)));
        assert_eq!(step(&rows, at(1, 9), "right", true), Step::To(at(1, 0)));
        assert_eq!(
            step(&rows, at(1, 9), "right", false),
            Step::Exit(Edge::Right)
        );
        assert_eq!(step(&rows, at(3, 0), "left", true), Step::To(at(3, 8)));
        assert_eq!(step(&rows, at(3, 0), "left", false), Step::Exit(Edge::Left));
    }

    #[test]
    fn vertical_moves_follow_the_unit_under_the_key() {
        let rows = rows(Layers::Basic, Layer::Lower);
        // "p" (unit 9) drops onto Backspace, then onto the two-unit Submit.
        assert_eq!(step(&rows, at(1, 9), "down", false), Step::To(at(2, 9)));
        assert_eq!(step(&rows, at(2, 9), "down", false), Step::To(at(3, 8)));
        // Submit's first unit is 8, so Up returns to "l".
        assert_eq!(step(&rows, at(3, 8), "up", false), Step::To(at(2, 8)));
        assert_eq!(step(&rows, at(0, 4), "up", false), Step::Exit(Edge::Up));
        assert_eq!(step(&rows, at(3, 2), "down", false), Step::Exit(Edge::Down));
    }

    #[test]
    fn wide_keys_on_the_full_bottom_row_map_both_ways() {
        let rows = rows(Layers::Full, Layer::Lower);
        assert_eq!(rows.len(), 5);
        // Unit 5 on the letter row sits over the four-unit Space.
        assert_eq!(step(&rows, at(3, 5), "down", false), Step::To(at(4, 2)));
        // Space covers units 4..8; its middle is unit 5.
        assert_eq!(step(&rows, at(4, 2), "up", false), Step::To(at(3, 5)));
        assert_eq!(placement(&rows, at(4, 2)), (4, 4));
        assert_eq!(placement(&rows, at(4, 3)), (8, 2));
    }

    #[test]
    fn focus_arriving_on_a_row_lands_under_where_it_was() {
        let rows = rows(Layers::Basic, Layer::Lower);
        // "8" on the digit row sits over Space on the bottom row.
        assert_eq!(middle_unit(&rows, at(0, 7)), 7);
        assert_eq!(seat_row(&rows, at(0, 7), usize::MAX), at(3, 7));
        // Search spans units 8 and 9; its first unit leads back to "9".
        assert_eq!(middle_unit(&rows, at(3, 8)), 8);
        assert_eq!(seat_row(&rows, at(3, 8), 0), at(0, 8));
        assert_eq!(seat_row(&rows, at(2, 3), 2), at(2, 3));
    }

    #[test]
    fn out_of_range_positions_clamp() {
        let rows = rows(Layers::Basic, Layer::Lower);
        assert_eq!(clamp(&rows, at(9, 40)), at(3, 8));
        assert_eq!(step(&rows, at(9, 40), "left", false), Step::To(at(3, 7)));
        assert_eq!(step(&[], at(0, 0), "left", false), Step::To(at(0, 0)));
    }

    #[test]
    fn shift_and_symbols_each_toggle_their_own_layer() {
        assert_eq!(toggle(Layer::Lower, Key::Shift), Layer::Upper);
        assert_eq!(toggle(Layer::Upper, Key::Shift), Layer::Lower);
        assert_eq!(toggle(Layer::Symbols, Key::Shift), Layer::Upper);
        assert_eq!(toggle(Layer::Lower, Key::Symbols), Layer::Symbols);
        assert_eq!(toggle(Layer::Symbols, Key::Symbols), Layer::Lower);
        assert_eq!(toggle(Layer::Upper, Key::Space), Layer::Upper);
    }

    #[test]
    fn buffer_inserts_and_deletes_at_the_caret() {
        let mut buffer = TextBuffer::new(8);
        for c in "mrio".chars() {
            assert!(buffer.insert(c));
        }
        assert!(buffer.move_caret(false));
        assert!(buffer.move_caret(false));
        assert!(buffer.move_caret(false));
        assert!(buffer.insert('a'));
        assert_eq!(buffer.text(), "mario");
        assert_eq!(buffer.split(), ("ma", "rio"));
        assert!(buffer.backspace());
        assert_eq!(buffer.text(), "mrio");
        assert_eq!(buffer.caret(), 1);
    }

    #[test]
    fn buffer_refuses_stray_spaces_controls_and_overflow() {
        let mut buffer = TextBuffer::new(4);
        assert!(!buffer.insert(' '), "leading space");
        assert!(buffer.insert('a'));
        assert!(buffer.insert(' '));
        assert!(!buffer.insert(' '), "doubled space");
        assert!(!buffer.insert('\n'));
        assert!(buffer.insert('b'));
        assert!(buffer.insert('c'));
        assert!(!buffer.insert('d'), "past the limit");
        assert_eq!(buffer.text(), "a bc");
        assert!(buffer.move_caret(false));
        assert!(buffer.move_caret(false));
        assert!(!buffer.insert(' '), "space before a space");
    }

    #[test]
    fn clearing_empties_the_field_and_reports_whether_it_did() {
        let mut buffer = TextBuffer::new(8);
        assert!(!buffer.clear());
        buffer.set("mario");
        assert!(buffer.clear());
        assert_eq!((buffer.text(), buffer.caret()), ("", 0));
        assert!(buffer.insert('z'));
        assert_eq!(buffer.text(), "z");
    }

    #[test]
    fn buffer_ends_stop_the_caret_and_backspace() {
        let mut buffer = TextBuffer::new(8);
        assert!(!buffer.backspace());
        assert!(!buffer.move_caret(false));
        assert!(!buffer.move_caret(true));
        buffer.set("é日本");
        assert_eq!(buffer.caret(), 3);
        assert!(buffer.backspace());
        assert_eq!(buffer.text(), "é日");
        buffer.set("far too long for this");
        assert_eq!(buffer.text(), "far too ");
    }

    #[test]
    fn private_use_and_control_characters_are_not_text() {
        assert!(is_text('a'));
        assert!(is_text('日'));
        assert!(!is_text('\u{F700}'));
        assert!(!is_text('\u{8}'));
        assert!(!is_text('\u{1b}'));
    }
}
