// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

//! Arcade alternate-version discovery: which search results count as
//! another build of the same game, where to find the rest,
//! and how a title is normalized so "Bubble Bobble (Japan)" and
//! "Bubble Bobble" compare equal. The Core calls stay in the shell.

/// The only system that ships alternate builds this way.
pub const ARCADE_SYSTEM_ID: &str = "Arcade";
/// Search cap for the seeding query.
pub const MAX_ALT_RESULTS: u32 = 64;
/// Browse cap for each alternate folder.
pub const MAX_FOLDER_RESULTS: u32 = 1000;
/// The path marker `MiSTer`'s own `_alternatives` folders carry
/// (`_alternatives`, `_Alternatives`, `_alternates`, ...), compared
/// case-insensitively.
const FOLDER_MARKER: &str = "_alternat";
/// The folder the arcadepatches kit installs into. Its subfolders are
/// categories (`_Enhanced Versions`, `_Restorations`, ...) holding many
/// games, so a hit under it is an alternate by itself and the folder is
/// never browsed.
const PATCHES_MARKER: &str = "_arcade patches";

/// Where an alternate build was found, and so how to collect it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AlternateSource {
    /// An `_alternatives/<Game>/` folder: browse it for the other builds.
    Folder(String),
    /// A category folder of unrelated games: take the search hit itself.
    Direct,
}

/// Classify a search hit. `None` when it is the selected row itself, has
/// no path, or sits outside every alternates location.
pub fn classify_alternate(
    path: &str,
    relative_path: &str,
    selected_path: &str,
) -> Option<AlternateSource> {
    if path.is_empty() || path == selected_path {
        return None;
    }
    if let Some(folder) = alternate_folder_path(path) {
        return Some(AlternateSource::Folder(folder));
    }
    let under_patches = |value: &str| value.to_lowercase().contains(PATCHES_MARKER);
    (under_patches(path) || under_patches(relative_path)).then_some(AlternateSource::Direct)
}

/// The folder to browse for an `_alternatives` hit: the alternates
/// directory plus the one level under it that names the game, when that
/// level is a folder. A file directly in the alternates directory keeps
/// the directory itself.
fn alternate_folder_path(path: &str) -> Option<String> {
    let trimmed = path.trim_matches('/');
    let parts: Vec<&str> = trimmed.split('/').filter(|part| !part.is_empty()).collect();
    let index = parts
        .iter()
        .position(|part| part.to_lowercase().contains(FOLDER_MARKER))?;
    let folder_end = if index + 2 < parts.len() {
        index + 2
    } else {
        index + 1
    };
    Some(format!("/{}", parts[..folder_end].join("/")))
}

/// Lower-cased alphanumerics only, so punctuation and spacing cannot
/// separate two spellings of one title.
pub fn normalize_name(value: &str) -> String {
    value
        .chars()
        .filter(char::is_ascii_alphanumeric)
        .flat_map(char::to_lowercase)
        .collect()
}

/// The same, after dropping the trailing `(...)` and `[...]` groups a
/// release name carries, so the region or dump tag does not make a
/// title look like a different game.
pub fn normalize_seed_title(value: &str) -> String {
    let mut trimmed = value.trim();
    loop {
        let candidate = trimmed.trim_end();
        if let Some(prefix) = strip_trailing_group(candidate, '(', ')') {
            trimmed = prefix;
            continue;
        }
        if let Some(prefix) = strip_trailing_group(candidate, '[', ']') {
            trimmed = prefix;
            continue;
        }
        break;
    }
    normalize_name(trimmed)
}

fn strip_trailing_group(value: &str, open: char, close: char) -> Option<&str> {
    let value = value.trim_end();
    if !value.ends_with(close) {
        return None;
    }
    let mut depth = 0usize;
    for (index, ch) in value.char_indices().rev() {
        if ch == close {
            depth += 1;
        } else if ch == open {
            depth = depth.saturating_sub(1);
            if depth == 0 {
                return Some(value[..index].trim_end());
            }
        }
    }
    None
}

/// Discovery only runs for Arcade, and only for a row that has both a
/// name to search on and a path to exclude.
pub fn can_discover(system_id: &str, name: &str, selected_path: &str) -> bool {
    system_id == ARCADE_SYSTEM_ID && !name.trim().is_empty() && !selected_path.trim().is_empty()
}

/// A browsed entry belongs in the results when it is a file the user is
/// not already on and has not been collected already.
pub fn is_discovered_entry(is_folder: bool, path: &str, selected_path: &str) -> bool {
    !is_folder && !path.is_empty() && path != selected_path
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_name_strips_non_alnum_and_case() {
        assert_eq!(normalize_name("Bubble Bobble!"), "bubblebobble");
        assert_eq!(normalize_name("Bubble-Bobble"), "bubblebobble");
    }

    #[test]
    fn normalize_seed_title_drops_trailing_variant_groups() {
        assert_eq!(normalize_seed_title("Bubble Bobble"), "bubblebobble");
        assert_eq!(
            normalize_seed_title("Bubble Bobble (Japan)"),
            "bubblebobble"
        );
        assert_eq!(
            normalize_seed_title("Bubble Bobble [Bootleg]"),
            "bubblebobble"
        );
        assert_eq!(
            normalize_seed_title("Bubble Bobble Lost Cave"),
            "bubblebobblelostcave"
        );
    }

    const SELECTED: &str = "/media/fat/_Arcade/Bubble Bobble.mra";

    #[test]
    fn folder_hit_names_the_game_folder_under_the_marker() {
        assert_eq!(
            classify_alternate(
                "/media/fat/_Arcade/_alternatives/Bubble Bobble/Bubble Bobble (Japan).mra",
                "_alternatives/Bubble Bobble/Bubble Bobble (Japan).mra",
                SELECTED,
            ),
            Some(AlternateSource::Folder(
                "/media/fat/_Arcade/_alternatives/Bubble Bobble".into()
            ))
        );
        // A file directly in the alternates folder browses that folder.
        assert_eq!(
            classify_alternate(
                "/media/fat/_Arcade/_alternatives/Bubble Bobble.mra",
                "",
                SELECTED,
            ),
            Some(AlternateSource::Folder(
                "/media/fat/_Arcade/_alternatives".into()
            ))
        );
    }

    #[test]
    fn folder_marker_is_case_insensitive() {
        assert_eq!(
            classify_alternate(
                "/media/fat/_Arcade/_Alternatives/Bubble Bobble/Bubble Bobble (Japan).mra",
                "",
                SELECTED,
            ),
            Some(AlternateSource::Folder(
                "/media/fat/_Arcade/_Alternatives/Bubble Bobble".into()
            ))
        );
    }

    #[test]
    fn arcade_patches_hits_are_taken_directly() {
        for path in [
            "/media/fat/_Arcade/_Arcade Patches/_Restorations/Street Fighter Alpha 2 (USA) [English Restoration].mra",
            "/media/fat/_Arcade/_arcade patches/_Enhanced Versions/Street Fighter Alpha 2 (USA).mra",
        ] {
            assert_eq!(
                classify_alternate(path, "", "/media/fat/_Arcade/Street Fighter Alpha 2.mra"),
                Some(AlternateSource::Direct),
                "{path}"
            );
        }
        // The marker can be on the relative path alone.
        assert_eq!(
            classify_alternate(
                "/x/Street Fighter Alpha 2 (USA).mra",
                "_Arcade Patches/_Restorations/Street Fighter Alpha 2 (USA).mra",
                SELECTED,
            ),
            Some(AlternateSource::Direct)
        );
    }

    #[test]
    fn selected_empty_and_unmarked_paths_are_not_alternates() {
        assert_eq!(
            classify_alternate(SELECTED, "Bubble Bobble.mra", SELECTED),
            None
        );
        assert_eq!(
            classify_alternate(
                "/media/fat/_Arcade/_alternatives/Bubble Bobble/Bubble Bobble.mra",
                "",
                "/media/fat/_Arcade/_alternatives/Bubble Bobble/Bubble Bobble.mra",
            ),
            None
        );
        assert_eq!(classify_alternate("", "", "/other"), None);
        assert_eq!(
            classify_alternate(
                "/media/fat/_Arcade/Bubble Bobble/Bubble Bobble.mra",
                "Bubble Bobble/Bubble Bobble.mra",
                SELECTED,
            ),
            None
        );
    }

    #[test]
    fn discovery_is_arcade_only_and_needs_a_row_to_seed_from() {
        assert!(can_discover("Arcade", "Bubble Bobble", "/x.mra"));
        assert!(!can_discover("NES", "Bubble Bobble", "/x.mra"));
        assert!(!can_discover("Arcade", "  ", "/x.mra"));
        assert!(!can_discover("Arcade", "Bubble Bobble", ""));
    }

    #[test]
    fn browsed_folders_and_the_selected_row_are_not_results() {
        assert!(is_discovered_entry(false, "/a.mra", "/b.mra"));
        assert!(!is_discovered_entry(true, "/a", "/b.mra"));
        assert!(!is_discovered_entry(false, "", "/b.mra"));
        assert!(!is_discovered_entry(false, "/b.mra", "/b.mra"));
    }
}
