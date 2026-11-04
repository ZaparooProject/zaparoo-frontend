// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// A game's four built-in collections: Favorite, Like, Dislike and Play
// later. Core keeps each as a flag on the game and keeps Like and Dislike
// apart: disliking a game drops its Like and its Favorite, and liking or
// favoriting it drops its Dislike. Play later stands on its own. The Options
// menu's strip shows the four and changes one at a time; this mirrors
// Core's rule so the strip shows the result before Core answers.

/// One of the four collections, in the strip's order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Flag {
    Favorite,
    Like,
    Dislike,
    PlayLater,
}

impl Flag {
    /// The strip's order: Favorite, Like, Dislike, Play later.
    pub const ALL: [Self; 4] = [Self::Favorite, Self::Like, Self::Dislike, Self::PlayLater];

    /// The value of the game's `user` tag for this flag.
    pub const fn tag_value(self) -> &'static str {
        match self {
            Self::Favorite => "favorite",
            Self::Like => "liked",
            Self::Dislike => "disliked",
            Self::PlayLater => "playlater",
        }
    }

    /// The whole tag, as Core filters and changes it.
    pub const fn tag(self) -> &'static str {
        match self {
            Self::Favorite => "user:favorite",
            Self::Like => "user:liked",
            Self::Dislike => "user:disliked",
            Self::PlayLater => "user:playlater",
        }
    }

    /// The flag a tag value names; `None` for any other value.
    pub fn from_tag_value(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|flag| flag.tag_value() == value)
    }
}

/// Which of the four a game is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "one bool per collection, each the game's own flag"
)]
pub struct Flags {
    pub favorite: bool,
    pub liked: bool,
    pub disliked: bool,
    pub play_later: bool,
}

impl Flags {
    /// The flags a game's `user` tag values say it has.
    pub fn from_tag_values<'a>(values: impl IntoIterator<Item = &'a str>) -> Self {
        let mut flags = Self::default();
        for value in values {
            match value {
                "favorite" => flags.favorite = true,
                "liked" => flags.liked = true,
                "disliked" => flags.disliked = true,
                "playlater" => flags.play_later = true,
                _ => {}
            }
        }
        flags
    }

    pub const fn has(self, flag: Flag) -> bool {
        match flag {
            Flag::Favorite => self.favorite,
            Flag::Like => self.liked,
            Flag::Dislike => self.disliked,
            Flag::PlayLater => self.play_later,
        }
    }

    const fn with(mut self, flag: Flag, on: bool) -> Self {
        match flag {
            Flag::Favorite => self.favorite = on,
            Flag::Like => self.liked = on,
            Flag::Dislike => self.disliked = on,
            Flag::PlayLater => self.play_later = on,
        }
        self
    }

    /// The game's flags once `flag` is toggled, as Core will leave them.
    #[must_use]
    pub const fn toggled(self, flag: Flag) -> Self {
        let on = !self.has(flag);
        let next = self.with(flag, on);
        if !on {
            return next;
        }
        match flag {
            Flag::Dislike => next.with(Flag::Like, false).with(Flag::Favorite, false),
            Flag::Like | Flag::Favorite => next.with(Flag::Dislike, false),
            Flag::PlayLater => next,
        }
    }
}

/// The toggles still waiting on Core, per game. A game can be toggled
/// again before Core answers the first change; only the answer to the
/// newest one decides what the game shows, so an older refusal cannot take
/// back a later change.
#[derive(Debug, Default)]
pub struct Requests {
    latest: std::collections::HashMap<String, u64>,
    next: u64,
}

impl Requests {
    /// A change to the game `key` is on its way; its ticket.
    pub fn begin(&mut self, key: &str) -> u64 {
        self.next = self.next.wrapping_add(1);
        self.latest.insert(key.to_string(), self.next);
        self.next
    }

    /// Core answered the change with `ticket`. True when it is the newest
    /// for the game, whose answer is then what the game shows.
    pub fn settle(&mut self, key: &str, ticket: u64) -> bool {
        if self.latest.get(key) == Some(&ticket) {
            self.latest.remove(key);
            true
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dislike_drops_like_and_favorite() {
        let liked = Flags {
            favorite: true,
            liked: true,
            play_later: true,
            ..Flags::default()
        };
        assert_eq!(
            liked.toggled(Flag::Dislike),
            Flags {
                disliked: true,
                play_later: true,
                ..Flags::default()
            }
        );
    }

    #[test]
    fn like_or_favorite_drops_dislike() {
        let disliked = Flags {
            disliked: true,
            ..Flags::default()
        };
        assert_eq!(
            disliked.toggled(Flag::Like),
            Flags {
                liked: true,
                ..Flags::default()
            }
        );
        assert_eq!(
            disliked.toggled(Flag::Favorite),
            Flags {
                favorite: true,
                ..Flags::default()
            }
        );
    }

    #[test]
    fn clearing_a_flag_changes_nothing_else() {
        let all = Flags {
            favorite: true,
            liked: true,
            play_later: true,
            ..Flags::default()
        };
        assert_eq!(
            all.toggled(Flag::Favorite),
            Flags {
                liked: true,
                play_later: true,
                ..Flags::default()
            }
        );
        let later = Flags {
            disliked: true,
            ..Flags::default()
        };
        assert_eq!(
            later.toggled(Flag::PlayLater).toggled(Flag::PlayLater),
            later
        );
    }

    #[test]
    fn only_the_newest_toggle_of_a_game_settles_it() {
        let mut requests = Requests::default();
        let first = requests.begin("SNES/mario");
        let other = requests.begin("NES/zelda");
        let second = requests.begin("SNES/mario");
        // The first change's answer, refusal or not, is stale.
        assert!(!requests.settle("SNES/mario", first));
        assert!(requests.settle("NES/zelda", other));
        assert!(requests.settle("SNES/mario", second));
        // Settled once only.
        assert!(!requests.settle("SNES/mario", second));
    }

    #[test]
    fn tag_values_read_back_as_flags() {
        let flags = Flags::from_tag_values(["playlater", "liked", "hidden"]);
        assert!(flags.liked && flags.play_later && !flags.favorite && !flags.disliked);
        for flag in Flag::ALL {
            assert!(Flags::from_tag_values([flag.tag_value()]).has(flag));
        }
    }
}
