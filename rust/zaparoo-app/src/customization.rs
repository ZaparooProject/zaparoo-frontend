// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

//! What the user may drop into the customization folder and how it is
//! matched: the two artwork namespaces, the image types accepted, and
//! the forgiving key both the scan and the lookup use. Plus the
//! `[custom.system_names]` table's own normalization. Ported from
//! `image_overrides.rs` and `system_name_overrides.rs`; the folder walk
//! itself belongs to the shell.

use std::collections::HashMap;
use std::hash::BuildHasher;

/// Image types accepted as override artwork. SVG is rasterized; the
/// rest are decoded as bitmaps.
pub const ALLOWED_EXTENSIONS: [&str; 6] = ["png", "jpg", "jpeg", "webp", "bmp", "svg"];

/// Override namespaces, each a same-named subfolder of the root.
/// `systems` is keyed by Zaparoo system id, `hub` by category or
/// action id.
pub const NAMESPACES: [&str; 2] = ["systems", "hub"];

pub fn is_allowed_extension(extension: &str) -> bool {
    ALLOWED_EXTENSIONS
        .iter()
        .any(|allowed| allowed.eq_ignore_ascii_case(extension))
}

/// The lookup key for a file stem or an id. Matching is
/// case-insensitive, so `Arcade.png`, `arcade.png` and `ARCADE.png` all
/// answer the `Arcade` category; applied identically on both sides so
/// they cannot drift.
pub fn match_key(id: &str) -> String {
    id.to_ascii_lowercase()
}

/// Normalize the `[custom.system_names]` table into its lookup form:
/// keys through the same forgiving rule the bundled name table uses,
/// blank entries dropped.
pub fn normalize_system_names<S: BuildHasher>(
    raw: HashMap<String, String, S>,
) -> HashMap<String, String> {
    raw.into_iter()
        .filter_map(|(id, name)| {
            let key = crate::systems::normalize_key(&id);
            let name = name.trim().to_string();
            (!key.is_empty() && !name.is_empty()).then_some((key, name))
        })
        .collect()
}

/// A system's user display name, if the table names it.
pub fn system_name<'a, S: BuildHasher>(
    names: &'a HashMap<String, String, S>,
    system_id: &str,
) -> Option<&'a str> {
    names
        .get(&crate::systems::normalize_key(system_id))
        .map(String::as_str)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn map_of(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect()
    }

    #[test]
    fn match_key_lowercases() {
        assert_eq!(match_key("Arcade"), "arcade");
        assert_eq!(match_key("SNES"), "snes");
        assert_eq!(match_key("recents"), "recents");
    }

    #[test]
    fn only_image_types_are_accepted_and_case_does_not_matter() {
        assert!(is_allowed_extension("png"));
        assert!(is_allowed_extension("PNG"));
        assert!(is_allowed_extension("svg"));
        assert!(!is_allowed_extension("txt"));
        assert!(!is_allowed_extension(""));
        assert_eq!(NAMESPACES, ["systems", "hub"]);
    }

    #[test]
    fn the_name_table_normalizes_its_keys_and_drops_blanks() {
        let names = normalize_system_names(map_of(&[
            ("S.N.E.S", "Super Famicom"),
            ("Genesis", "  Mega Drive  "),
            ("", "Nothing"),
            ("NES", "   "),
        ]));
        assert_eq!(names.len(), 2);
        assert_eq!(system_name(&names, "snes"), Some("Super Famicom"));
        assert_eq!(system_name(&names, "SNES"), Some("Super Famicom"));
        assert_eq!(system_name(&names, "genesis"), Some("Mega Drive"));
        assert_eq!(system_name(&names, "NES"), None);
    }
}
