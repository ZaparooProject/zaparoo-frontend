// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

//! Writing `ZapScript` the frontend sends to Core: escaping a value so the
//! parser reads it as one argument, and attributing Update All launches.

const UPDATE_ALL_PATH: &str = "/media/fat/Scripts/update_all.sh";

/// Update All reads `LAUNCH_ORIGIN_ID` to decide how to hand the console back
/// after a core load. Core only sets it through this `mister.script` option; a
/// launch by path through the Scripts launcher leaves it unset.
const UPDATE_ALL_LAUNCH: &str = "**mister.script:update_all.sh?launch_origin_id=zaparoo_frontend";

/// Whether `path` is the top-level Update All script.
pub fn is_update_all(path: &str) -> bool {
    path == UPDATE_ALL_PATH
}

/// Puts the attributed Update All launch ahead of `candidates` when one of
/// them is its path. The others stay as the fallback.
pub fn attribute_update_all(mut candidates: Vec<String>) -> Vec<String> {
    if candidates.iter().any(|text| is_update_all(text)) {
        candidates.insert(0, UPDATE_ALL_LAUNCH.to_string());
    }
    candidates
}

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

    fn strings(values: &[&str]) -> Vec<String> {
        values.iter().map(ToString::to_string).collect()
    }

    #[test]
    fn update_all_launches_attributed_first_with_the_rest_kept_in_order() {
        assert_eq!(
            attribute_update_all(strings(&[UPDATE_ALL_PATH])),
            strings(&[UPDATE_ALL_LAUNCH, UPDATE_ALL_PATH]),
        );
        // A Hub tile: relative path, title command, then the absolute path.
        assert_eq!(
            attribute_update_all(strings(&[
                "Scripts/update_all.sh",
                "@Script/update_all",
                UPDATE_ALL_PATH,
            ])),
            strings(&[
                UPDATE_ALL_LAUNCH,
                "Scripts/update_all.sh",
                "@Script/update_all",
                UPDATE_ALL_PATH,
            ]),
        );
    }

    #[test]
    fn anything_but_the_top_level_update_all_is_left_alone() {
        for other in [
            "/media/fat/Scripts/wifi.sh",
            "/media/fat/Scripts/sub/update_all.sh",
            "/media/fat/Scripts/UPDATE_ALL.SH",
            "@Script/update_all",
        ] {
            assert_eq!(attribute_update_all(strings(&[other])), strings(&[other]));
        }
        assert!(attribute_update_all(Vec::new()).is_empty());
    }

    #[test]
    fn separators_are_escaped_and_the_rest_left_alone() {
        assert_eq!(escape_arg("/music/A, B?.mp3"), "/music/A^, B^?.mp3");
        assert_eq!(escape_arg("x^y&z|w"), "x^^y^&z^|w");
        assert_eq!(escape_arg("Crises [Remaster]"), "Crises [Remaster]");
    }
}
