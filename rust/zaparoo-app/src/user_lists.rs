// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

//! The list screens of the built-in collections: Favorites, Liked, Play
//! later and Disliked. Each is a flag on a game in Core, listed with a
//! tag-filtered search, in one of three orders and, optionally, grouped by
//! system first.

use crate::user_flags::Flag;

/// A list's order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListSort {
    /// Core's own order.
    Default,
    /// A to Z.
    Name,
    /// Newest first, by when the game was put in the list.
    Added,
}

impl ListSort {
    /// The choices in the order the Sort picker lists them.
    pub const ALL: [Self; 3] = [Self::Default, Self::Name, Self::Added];

    /// The value a setting or the state file keeps.
    pub const fn token(self) -> &'static str {
        match self {
            Self::Default => "default",
            Self::Name => "name",
            Self::Added => "added",
        }
    }

    /// A kept value; `None` for an empty or unknown one, which reads as
    /// the list's default order.
    pub fn from_token(token: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|sort| sort.token() == token)
    }

    /// The `media.search` sort Core is sent.
    pub const fn core_sort(self) -> Option<&'static str> {
        match self {
            Self::Default => None,
            Self::Name => Some("name-asc"),
            Self::Added => Some("added-desc"),
        }
    }

    /// The `Labels.menu-value` key naming the order.
    pub const fn label_key(self) -> &'static str {
        match self {
            Self::Default => "sort:default",
            Self::Name => "sort:name",
            Self::Added => "sort:added",
        }
    }
}

/// The order a list opens in before the user picks one. Play later is a
/// queue, so what was put in last reads first.
pub const fn default_sort(flag: Flag) -> ListSort {
    match flag {
        Flag::PlayLater => ListSort::Added,
        Flag::Favorite | Flag::Like | Flag::Dislike => ListSort::Default,
    }
}

/// A list's order from what was kept for it.
pub fn sort_for(flag: Flag, kept: &str) -> ListSort {
    ListSort::from_token(kept.trim()).unwrap_or_else(|| default_sort(flag))
}

/// The kept grouping value that groups a list by system.
pub const GROUP_SYSTEM: &str = "system";

/// Whether a list opens on the systems that hold it.
pub fn grouped(kept: &str) -> bool {
    kept.trim() == GROUP_SYSTEM
}

/// The `ZapScript` that launches a random game from a list, from one
/// system's or from all of them.
pub fn random_launch_text(flag: Flag, system_id: &str) -> String {
    let scope = system_id.trim();
    if scope.is_empty() {
        return format!("**launch.random:all?tags={}", flag.tag());
    }
    let mut escaped = String::with_capacity(scope.len());
    for ch in scope.chars() {
        if matches!(ch, '^' | '?' | ',' | '&' | '|') {
            escaped.push('^');
        }
        escaped.push(ch);
    }
    format!("**launch.random:{escaped}?tags={}", flag.tag())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kept_orders_read_back_and_unknown_ones_fall_to_the_default() {
        for sort in ListSort::ALL {
            assert_eq!(ListSort::from_token(sort.token()), Some(sort));
        }
        assert_eq!(sort_for(Flag::Favorite, ""), ListSort::Default);
        assert_eq!(sort_for(Flag::PlayLater, ""), ListSort::Added);
        assert_eq!(sort_for(Flag::PlayLater, "default"), ListSort::Default);
        assert_eq!(sort_for(Flag::Like, "shuffle"), ListSort::Default);
        assert_eq!(sort_for(Flag::Dislike, " name "), ListSort::Name);
    }

    #[test]
    fn orders_map_to_core_sorts() {
        assert_eq!(ListSort::Default.core_sort(), None);
        assert_eq!(ListSort::Name.core_sort(), Some("name-asc"));
        assert_eq!(ListSort::Added.core_sort(), Some("added-desc"));
    }

    #[test]
    fn random_launch_scopes_to_the_list_and_escapes_the_system() {
        assert_eq!(
            random_launch_text(Flag::Like, ""),
            "**launch.random:all?tags=user:liked"
        );
        assert_eq!(
            random_launch_text(Flag::PlayLater, "Sega?CD"),
            "**launch.random:Sega^?CD?tags=user:playlater"
        );
        assert!(grouped("system") && !grouped("") && !grouped("none"));
    }
}
