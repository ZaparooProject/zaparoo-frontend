// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// The focused item's info line: what a grid's label under the tiles, a
// list's detail pane and a text list's focused row say about the one item
// in focus. A tile carries only its identity (cover, name, system label and
// the favorite heart); everything else about an item lives here, as typed
// fields in one fixed order. See `docs/style.md` -> "Tiles and the info
// line".

use crate::top_played::PlayTime;
use crate::user_flags::Flags;

/// One part of the info line, in the order it is laid out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    Name,
    /// The game's collections, as icons: heart, thumbs up, thumbs down,
    /// bookmark.
    Collections,
    /// A title's other versions, as a stack icon and a count.
    Versions,
    /// Total play time, as a clock icon and the time.
    PlayTime,
    /// A deck's owner and lock.
    Owner,
    /// Shown although hidden, as an eye-off icon.
    Hidden,
    /// Tags, dim; a deck's item count takes this place.
    Tags,
}

/// Everything the info line can say about one item. Each field is
/// optional; an empty or zero value is a field that does not show.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InfoLine {
    pub name: String,
    pub flags: Flags,
    /// Versions of the same title beyond this one's row, zero for none.
    pub versions: u32,
    pub play_time: Option<PlayTime>,
    /// Who a saved deck is from.
    pub owner: String,
    /// The deck cannot be changed here: a saved copy or a locked deck.
    pub locked: bool,
    pub hidden: bool,
    /// A deck's item count, shown dim where a game's tags go.
    pub item_count: Option<u32>,
    pub tags: String,
}

/// The order every view lays the fields out in.
pub const ORDER: [Field; 7] = [
    Field::Name,
    Field::Collections,
    Field::Versions,
    Field::PlayTime,
    Field::Owner,
    Field::Hidden,
    Field::Tags,
];

/// Which text gives way first when the line does not fit: the tags are
/// elided before the name, and the icons and short facts never are.
pub const SHRINK_ORDER: [Field; 2] = [Field::Tags, Field::Name];

impl InfoLine {
    /// Whether `field` has anything to show.
    pub fn shows(&self, field: Field) -> bool {
        match field {
            Field::Name => !self.name.is_empty(),
            Field::Collections => {
                self.flags.favorite
                    || self.flags.liked
                    || self.flags.disliked
                    || self.flags.play_later
            }
            Field::Versions => self.versions > 0,
            Field::PlayTime => self.play_time.is_some(),
            Field::Owner => !self.owner.is_empty() || self.locked,
            Field::Hidden => self.hidden,
            Field::Tags => !self.tags.is_empty() || self.item_count.is_some(),
        }
    }

    /// The fields that show, in layout order.
    pub fn fields(&self) -> Vec<Field> {
        ORDER.into_iter().filter(|&f| self.shows(f)).collect()
    }

    /// The facts between the name and the tags: what a text list's focused
    /// row adds at its right end, where the row already shows the name and
    /// the tags.
    pub fn facts(&self) -> Vec<Field> {
        self.fields()
            .into_iter()
            .filter(|f| !matches!(f, Field::Name | Field::Tags))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plain_game_shows_its_name_and_tags_only() {
        let line = InfoLine {
            name: "Zelda".into(),
            tags: "USA".into(),
            ..InfoLine::default()
        };
        assert_eq!(line.fields(), [Field::Name, Field::Tags]);
        assert!(line.facts().is_empty());
    }

    #[test]
    fn every_field_keeps_the_fixed_order() {
        let line = InfoLine {
            name: "Zelda".into(),
            flags: Flags {
                favorite: true,
                ..Flags::default()
            },
            versions: 3,
            play_time: Some(PlayTime::from_seconds(3600)),
            owner: "wizzo".into(),
            locked: true,
            hidden: true,
            item_count: None,
            tags: "USA".into(),
        };
        assert_eq!(line.fields(), ORDER);
        assert_eq!(
            line.facts(),
            [
                Field::Collections,
                Field::Versions,
                Field::PlayTime,
                Field::Owner,
                Field::Hidden
            ]
        );
    }

    #[test]
    fn a_locked_deck_shows_its_lock_without_an_owner_and_its_count_as_tags() {
        let line = InfoLine {
            name: "Club night".into(),
            locked: true,
            item_count: Some(4),
            ..InfoLine::default()
        };
        assert_eq!(line.fields(), [Field::Name, Field::Owner, Field::Tags]);
    }

    #[test]
    fn the_tags_give_way_before_the_name() {
        assert_eq!(SHRINK_ORDER, [Field::Tags, Field::Name]);
    }
}
