// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

//! Preferred artwork per system and folder.
//!
//! A choice is keyed by a system id and Core's launcher-relative path
//! (`SNES/USA`), so it survives the library moving to another drive. An
//! empty path is the whole system. A game or folder takes the choice of
//! the deepest folder above it that has one, then its system's, then the
//! global setting. Choices are client-local: they live in `frontend.toml`
//! and only reorder the `imageTypes` a cover asks Core for.

use std::collections::BTreeMap;

use crate::media_list::COVER_LADDER;
use crate::settings::MEDIA_IMAGE_TYPES;

/// The global setting's token for "no preference".
const AUTO: &str = "auto";

/// The persisted key of a choice: the system id alone, or the system id,
/// a slash and the launcher-relative path. System ids never hold a slash.
pub fn entry_key(system: &str, relative: &str) -> String {
    let relative = normalize(relative);
    if relative.is_empty() {
        system.to_string()
    } else {
        format!("{system}/{relative}")
    }
}

fn normalize(relative: &str) -> &str {
    relative.trim_matches('/')
}

/// Whether `token` is an artwork type a folder or system can choose.
/// `auto` is not: the absence of a choice already means it.
pub fn is_choice(token: &str) -> bool {
    token != AUTO && MEDIA_IMAGE_TYPES.contains(&token)
}

/// The types a folder or system can choose, in the global setting's order.
pub fn choices() -> impl Iterator<Item = &'static str> {
    MEDIA_IMAGE_TYPES.iter().copied().filter(|t| *t != AUTO)
}

/// The type a cover leads with: a folder or system choice, else the
/// global setting.
pub fn effective<'a>(prefer: Option<&'a str>, global: &'a str) -> &'a str {
    prefer.unwrap_or(global)
}

/// The `imageTypes` a plain cover asks Core for: the cover ladder, with
/// the preferred type moved to the front. Core answers the first type the
/// game has, so a game without the preferred art still gets a cover.
pub fn ladder(preferred: &str) -> Vec<String> {
    let mut types: Vec<String> = COVER_LADDER.iter().map(ToString::to_string).collect();
    if !preferred.is_empty() && preferred != AUTO {
        types.retain(|kind| kind != preferred);
        types.insert(0, preferred.to_string());
    }
    types
}

/// Every folder and system choice, as read from and written to
/// `frontend.toml`. Entries whose value is not a known type are kept, so a
/// newer build's choice survives an older build's save, but never apply.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ArtworkPrefs {
    entries: BTreeMap<String, String>,
}

impl ArtworkPrefs {
    /// Build from the persisted table. Blank keys and values are dropped.
    pub fn from_entries<I, K, V>(entries: I) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: AsRef<str>,
        V: AsRef<str>,
    {
        Self {
            entries: entries
                .into_iter()
                .filter_map(|(key, value)| {
                    let key = key.as_ref().trim().trim_end_matches('/');
                    let value = value.as_ref().trim();
                    (!key.is_empty() && !value.is_empty())
                        .then(|| (key.to_string(), value.to_string()))
                })
                .collect(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The choice made on exactly this system or folder, if it applies.
    pub fn chosen(&self, system: &str, relative: &str) -> Option<&str> {
        if system.is_empty() {
            return None;
        }
        self.entries
            .get(&entry_key(system, relative))
            .map(String::as_str)
            .filter(|token| is_choice(token))
    }

    /// The choice that applies to a game or folder at `relative`: its own,
    /// then each folder above it, deepest first, then its system's.
    pub fn resolve(&self, system: &str, relative: &str) -> Option<&str> {
        if self.entries.is_empty() || system.is_empty() {
            return None;
        }
        let mut relative = normalize(relative);
        loop {
            if let Some(token) = self.chosen(system, relative) {
                return Some(token);
            }
            if relative.is_empty() {
                return None;
            }
            relative = relative.rsplit_once('/').map_or("", |(parent, _)| parent);
        }
    }

    /// What a folder or system shows with no choice of its own: the
    /// nearest folder above it, then its system. A system inherits only
    /// the global setting, which is `None` here.
    pub fn inherited(&self, system: &str, relative: &str) -> Option<&str> {
        let relative = normalize(relative);
        if relative.is_empty() {
            return None;
        }
        let parent = relative.rsplit_once('/').map_or("", |(parent, _)| parent);
        self.resolve(system, parent)
    }

    /// Make or clear (`None`) the choice on one system or folder. Returns
    /// whether anything changed.
    pub fn set(&mut self, system: &str, relative: &str, choice: Option<&str>) -> bool {
        let key = entry_key(system, relative);
        match choice {
            Some(token) => {
                if self.entries.get(&key).map(String::as_str) == Some(token) {
                    return false;
                }
                self.entries.insert(key, token.to_string());
                true
            }
            None => self.entries.remove(&key).is_some(),
        }
    }

    /// Whether a choice on `system`/`relative` changes the art of anything
    /// at `other_relative` in the same system: it is that folder or below it.
    pub fn covers(relative: &str, other_relative: &str) -> bool {
        let relative = normalize(relative);
        let other = normalize(other_relative);
        relative.is_empty()
            || other == relative
            || other
                .strip_prefix(relative)
                .is_some_and(|rest| rest.starts_with('/'))
    }

    /// The persisted key and value of a choice, for the config writer.
    pub fn entry(&self, system: &str, relative: &str) -> (String, Option<String>) {
        let key = entry_key(system, relative);
        let value = self.entries.get(&key).cloned();
        (key, value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prefs(entries: &[(&str, &str)]) -> ArtworkPrefs {
        ArtworkPrefs::from_entries(entries.iter().copied())
    }

    #[test]
    fn a_system_choice_applies_to_every_game_in_it() {
        let p = prefs(&[("Arcade", "titleshot")]);
        assert_eq!(p.resolve("Arcade", "_Arcade/pacman.mra"), Some("titleshot"));
        assert_eq!(p.resolve("Arcade", ""), Some("titleshot"));
        assert_eq!(p.resolve("SNES", "SNES/Game.sfc"), None);
    }

    #[test]
    fn the_deepest_folder_wins() {
        let p = prefs(&[
            ("SNES", "boxart"),
            ("SNES/SNES", "screenshot"),
            ("SNES/SNES/Hacks", "titleshot"),
        ]);
        assert_eq!(p.resolve("SNES", "SNES/Hacks/Game.sfc"), Some("titleshot"));
        assert_eq!(p.resolve("SNES", "SNES/USA/Game.sfc"), Some("screenshot"));
        assert_eq!(p.resolve("SNES", "Other/Game.sfc"), Some("boxart"));
    }

    #[test]
    fn a_folder_prefix_is_matched_on_whole_segments() {
        let p = prefs(&[("SNES/SNES/Hack", "titleshot")]);
        assert_eq!(p.resolve("SNES", "SNES/Hacks/Game.sfc"), None);
        assert!(!ArtworkPrefs::covers("SNES/Hack", "SNES/Hacks/Game.sfc"));
        assert!(ArtworkPrefs::covers("SNES/Hack", "SNES/Hack/Game.sfc"));
        assert!(ArtworkPrefs::covers("SNES/Hack", "SNES/Hack"));
        assert!(ArtworkPrefs::covers("", "anything"));
    }

    #[test]
    fn a_folder_resolves_its_own_choice() {
        let p = prefs(&[("NES/NES/Hacks", "screenshot")]);
        assert_eq!(p.resolve("NES", "NES/Hacks"), Some("screenshot"));
        assert_eq!(p.chosen("NES", "NES/Hacks/"), Some("screenshot"));
    }

    #[test]
    fn a_child_overrides_and_reset_inherits_again() {
        let mut p = prefs(&[("NES", "boxart")]);
        assert!(p.set("NES", "NES/Hacks", Some("screenshot")));
        assert_eq!(p.resolve("NES", "NES/Hacks/Game.nes"), Some("screenshot"));
        assert_eq!(p.inherited("NES", "NES/Hacks"), Some("boxart"));
        assert!(!p.set("NES", "NES/Hacks", Some("screenshot")));
        assert!(p.set("NES", "NES/Hacks", None));
        assert!(!p.set("NES", "NES/Hacks", None));
        assert_eq!(p.resolve("NES", "NES/Hacks/Game.nes"), Some("boxart"));
    }

    #[test]
    fn a_system_inherits_only_the_global_setting() {
        let p = prefs(&[("NES", "boxart")]);
        assert_eq!(p.inherited("NES", ""), None);
        assert_eq!(
            effective(p.inherited("NES", ""), "screenshot"),
            "screenshot"
        );
    }

    #[test]
    fn unknown_tokens_are_kept_but_never_apply() {
        let p = prefs(&[("NES", "hologram"), ("SNES", "auto"), ("GB", " ")]);
        assert_eq!(p.resolve("NES", "NES/Game.nes"), None);
        assert_eq!(p.resolve("SNES", ""), None);
        assert_eq!(p.entry("NES", "").1.as_deref(), Some("hologram"));
        assert_eq!(p.entry("GB", "").1, None);
    }

    #[test]
    fn a_row_without_a_system_resolves_nothing() {
        let p = prefs(&[("NES", "boxart")]);
        assert_eq!(p.resolve("", "NES/Game.nes"), None);
        assert_eq!(p.chosen("", ""), None);
    }

    #[test]
    fn entry_keys_join_system_and_path() {
        assert_eq!(entry_key("SNES", ""), "SNES");
        assert_eq!(entry_key("SNES", "/SNES/USA/"), "SNES/SNES/USA");
    }

    #[test]
    fn the_ladder_leads_with_the_preferred_type_and_keeps_the_fallbacks() {
        assert_eq!(ladder("auto"), COVER_LADDER);
        assert_eq!(ladder(""), COVER_LADDER);
        let shot = ladder("screenshot");
        assert_eq!(shot[0], "screenshot");
        assert_eq!(shot.len(), COVER_LADDER.len());
        let title = ladder("titleshot");
        assert_eq!(title[0], "titleshot");
        assert_eq!(&title[1..], COVER_LADDER);
    }

    #[test]
    fn choices_are_the_global_types_without_auto() {
        assert!(choices().all(is_choice));
        assert_eq!(choices().count(), MEDIA_IMAGE_TYPES.len() - 1);
        assert!(!is_choice("auto"));
    }
}
