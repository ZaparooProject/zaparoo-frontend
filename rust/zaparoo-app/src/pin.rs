// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// The PIN panel's number pad: twelve keys in a phone's order, the cursor's
// moves over them, and what a key does to the digits typed so far.

use crate::settings::PIN_MAX;

/// One key of the pad.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Digit(u8),
    Delete,
    Ok,
}

/// Three columns, four rows: 1 to 9, then Delete, 0 and OK.
pub const COLUMNS: usize = 3;
pub const KEYS: [Key; 12] = [
    Key::Digit(1),
    Key::Digit(2),
    Key::Digit(3),
    Key::Digit(4),
    Key::Digit(5),
    Key::Digit(6),
    Key::Digit(7),
    Key::Digit(8),
    Key::Digit(9),
    Key::Delete,
    Key::Digit(0),
    Key::Ok,
];

/// Where the cursor starts: the 5 in the middle of the pad.
pub const START: usize = 4;

/// The key at a cursor position.
pub fn key_at(index: usize) -> Key {
    KEYS[index.min(KEYS.len() - 1)]
}

/// The cursor after one press: it stops at the pad's edges rather than
/// wrapping, so a held direction never loops past where it was going.
pub fn step(index: usize, dx: i64, dy: i64) -> usize {
    let rows = KEYS.len() / COLUMNS;
    let col = (index % COLUMNS) as i64;
    let row = (index / COLUMNS) as i64;
    let col = (col + dx).clamp(0, COLUMNS as i64 - 1);
    let row = (row + dy).clamp(0, rows as i64 - 1);
    (row * COLUMNS as i64 + col) as usize
}

/// What a key press did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Press {
    /// The digits changed (or stayed, at the limit).
    Edited,
    /// OK with a complete PIN: check it.
    Submit,
    /// OK too early: nothing to check yet.
    Incomplete,
}

/// Apply one key to the digits typed so far.
pub fn press(pin: &mut String, key: Key) -> Press {
    match key {
        Key::Digit(d) => {
            if pin.len() < PIN_MAX {
                pin.push(char::from(b'0' + d));
            }
            Press::Edited
        }
        Key::Delete => {
            pin.pop();
            Press::Edited
        }
        Key::Ok => {
            if crate::settings::pin_complete(pin) {
                Press::Submit
            } else {
                Press::Incomplete
            }
        }
    }
}

/// A typed character as a pad key: digits only.
pub fn key_for_char(c: char) -> Option<Key> {
    c.to_digit(10).map(|d| Key::Digit(d as u8))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cursor_stops_at_the_edges() {
        assert_eq!(step(0, -1, 0), 0);
        assert_eq!(step(0, 0, -1), 0);
        assert_eq!(step(START, 1, 0), 5);
        assert_eq!(step(START, 0, 2), 10);
        assert_eq!(step(11, 1, 1), 11);
        assert_eq!(key_at(10), Key::Digit(0));
    }

    #[test]
    fn digits_stop_at_the_limit_and_ok_needs_four() {
        let mut pin = String::new();
        assert_eq!(press(&mut pin, Key::Ok), Press::Incomplete);
        for _ in 0..10 {
            press(&mut pin, Key::Digit(7));
        }
        assert_eq!(pin.len(), PIN_MAX);
        assert_eq!(press(&mut pin, Key::Ok), Press::Submit);
        press(&mut pin, Key::Delete);
        assert_eq!(pin.len(), PIN_MAX - 1);
        let mut short = "12".to_string();
        assert_eq!(press(&mut short, Key::Ok), Press::Incomplete);
        assert_eq!(key_for_char('3'), Some(Key::Digit(3)));
        assert_eq!(key_for_char('a'), None);
    }
}
