// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// Number formatting for user-visible counts, ported from `Format.qml`'s
// `count()` (`Number.toLocaleString(locale, "f", 0)`). Without Qt's CLDR
// data the grouping rules are a table over the languages the app ships;
// unknown languages group by three with a comma.

/// Groups `n` for `language` (`de`, `fr_FR`, `ar`, ...).
pub fn count(n: i64, language: &str) -> String {
    let tag = language.split(['.', '@']).next().unwrap_or_default();
    let mut parts = tag.split(['_', '-']);
    let lang = parts.next().unwrap_or_default().to_ascii_lowercase();
    let region = parts.next().unwrap_or_default().to_ascii_uppercase();

    let (separator, indian, arabic_digits) = match lang.as_str() {
        "de" | "es" | "it" | "nl" | "ro" | "el" | "eu" | "id" | "tr" | "da" | "pt" => {
            (".", false, false)
        }
        "fr" => ("\u{202f}", false, false),
        "sk" | "uk" | "ru" | "pl" | "cs" | "sv" | "fi" | "nb" | "nn" | "hu" | "bg" => {
            ("\u{a0}", false, false)
        }
        "hi" => (",", true, false),
        "ar" => (
            "\u{66c}",
            false,
            region != "MA" && region != "DZ" && region != "TN",
        ),
        _ => (",", false, false),
    };

    let negative = n < 0;
    let digits = n.unsigned_abs().to_string();
    let mut grouped = group(&digits, separator, indian);
    if arabic_digits {
        grouped = grouped
            .chars()
            .map(|c| match c {
                '0'..='9' => char::from_u32(0x660 + (c as u32 - '0' as u32)).unwrap_or(c),
                other => other,
            })
            .collect();
    }
    if negative {
        format!("-{grouped}")
    } else {
        grouped
    }
}

/// Insert `separator` every three digits from the right, or after the
/// first three and then every two for the Indian grouping.
fn group(digits: &str, separator: &str, indian: bool) -> String {
    let chars: Vec<char> = digits.chars().collect();
    let len = chars.len();
    let mut out = String::new();
    for (i, c) in chars.iter().enumerate() {
        let remaining = len - i;
        if i > 0 {
            let boundary = if indian {
                remaining == 3 || (remaining > 3 && (remaining - 3).is_multiple_of(2))
            } else {
                remaining.is_multiple_of(3)
            };
            if boundary {
                out.push_str(separator);
            }
        }
        out.push(*c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn groups_by_language() {
        assert_eq!(count(1_234_567, "en"), "1,234,567");
        assert_eq!(count(999, "en_US"), "999");
        assert_eq!(count(1_234_567, "de"), "1.234.567");
        assert_eq!(count(1_234_567, "fr_FR"), "1\u{202f}234\u{202f}567");
        assert_eq!(count(1_234_567, "sk"), "1\u{a0}234\u{a0}567");
        assert_eq!(count(1_234_567, "ja"), "1,234,567");
        assert_eq!(count(-1000, "es"), "-1.000");
        assert_eq!(count(0, "uk"), "0");
    }

    #[test]
    fn indian_and_arabic_groupings() {
        assert_eq!(count(1_234_567, "hi"), "12,34,567");
        assert_eq!(count(123_456, "hi_IN"), "1,23,456");
        assert_eq!(count(1234, "hi"), "1,234");
        assert_eq!(count(1234, "ar"), "\u{661}\u{66c}\u{662}\u{663}\u{664}");
        assert_eq!(count(1234, "ar_MA"), "1\u{66c}234");
    }
}
