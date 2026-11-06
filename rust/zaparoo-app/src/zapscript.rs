// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

//! Writing `ZapScript` the frontend sends to Core: escaping a value so the
//! parser reads it as one argument.

/// Escape `ZapScript` separators while leaving path text readable.
pub fn escape_arg(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for ch in value.chars() {
        if matches!(ch, '^' | '?' | ',' | '&' | '|') {
            escaped.push('^');
        }
        escaped.push(ch);
    }
    escaped
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn separators_are_escaped_and_the_rest_left_alone() {
        assert_eq!(escape_arg("/music/A, B?.mp3"), "/music/A^, B^?.mp3");
        assert_eq!(escape_arg("x^y&z|w"), "x^^y^&z^|w");
        assert_eq!(escape_arg("Crises [Remaster]"), "Crises [Remaster]");
    }
}
