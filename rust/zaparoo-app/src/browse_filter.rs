// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

//! The Games browse filter: which of Core's tag types are offered, in what
//! order, and how a one-value-per-category selection maps onto the
//! `type:value` tags `media.browse` takes.
//!
//! Core joins every `~` (OR) tag into a single group, so "this genre or that
//! one, and this region or that one" cannot be expressed. A selection is
//! therefore at most one value per category, all required together.

/// A tag as `media.tags` lists it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FilterTag {
    pub tag_type: String,
    pub value: String,
    /// Core's display name; empty when it sends none.
    pub label: String,
    /// Games carrying the tag; 0 when Core sends no count.
    pub count: i64,
}

/// One pickable value inside a category.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilterValue {
    pub value: String,
    pub label: String,
    pub count: i64,
}

/// The filterable categories, in menu order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Category {
    Genre,
    Year,
    Players,
    Developer,
    Publisher,
    Region,
    Language,
    Perspective,
    Input,
    ArcadeBoard,
    Lists,
}

/// The collections worth filtering on. `hidden`, `disliked` and the deck
/// membership tags are bookkeeping, not something to browse by.
const LIST_VALUES: [&str; 3] = ["favorite", "liked", "playlater"];

/// Leads a deck membership tag's value; the deck's id follows.
const DECK_PREFIX: &str = "deck:";

impl Category {
    pub const ALL: [Category; 11] = [
        Category::Genre,
        Category::Year,
        Category::Players,
        Category::Developer,
        Category::Publisher,
        Category::Region,
        Category::Language,
        Category::Perspective,
        Category::Input,
        Category::ArcadeBoard,
        Category::Lists,
    ];

    /// Core's tag type for the category.
    pub fn tag_type(self) -> &'static str {
        match self {
            Category::Genre => "genre",
            Category::Year => "year",
            Category::Players => "players",
            Category::Developer => "developer",
            Category::Publisher => "publisher",
            Category::Region => "region",
            Category::Language => "lang",
            Category::Perspective => "perspective",
            Category::Input => "input",
            Category::ArcadeBoard => "arcadeboard",
            Category::Lists => "user",
        }
    }

    /// The stable id the menu rows route on.
    pub fn id(self) -> &'static str {
        match self {
            Category::Language => "language",
            Category::Lists => "lists",
            other => other.tag_type(),
        }
    }

    pub fn from_id(id: &str) -> Option<Category> {
        Category::ALL.into_iter().find(|c| c.id() == id)
    }

    fn from_tag_type(tag_type: &str) -> Option<Category> {
        let tag_type = tag_type.trim();
        Category::ALL
            .into_iter()
            .find(|c| c.tag_type().eq_ignore_ascii_case(tag_type))
    }

    fn accepts(self, value: &str) -> bool {
        self != Category::Lists || LIST_VALUES.contains(&value) || deck_id(value).is_some()
    }

    /// Numbers sort by magnitude, not alphabetically ("10" after "2").
    fn sorts_numerically(self) -> bool {
        matches!(self, Category::Year | Category::Players)
    }
}

/// One category and the values Core offers for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group {
    pub category: Category,
    pub values: Vec<FilterValue>,
}

/// Sort the tags `media.tags` returned into categories, in menu order.
/// Types outside the allowlist, empty values and duplicates are dropped, and
/// a category with no values is left out.
pub fn group(tags: &[FilterTag]) -> Vec<Group> {
    Category::ALL
        .into_iter()
        .filter_map(|category| {
            let mut values: Vec<FilterValue> = Vec::new();
            for tag in tags {
                let value = tag.value.trim();
                if value.is_empty()
                    || Category::from_tag_type(&tag.tag_type) != Some(category)
                    || !category.accepts(value)
                    || values.iter().any(|v| v.value == value)
                    // A deck is offered by its name, which Core's tag list
                    // does not carry; one the caller could not name is an
                    // id nobody would recognise.
                    || (deck_id(value).is_some() && tag.label.trim().is_empty())
                {
                    continue;
                }
                let label = match collection_name(category, value) {
                    Some(name) if tag.label.trim().is_empty() => name.to_string(),
                    _ => display_label(&tag.label, value),
                };
                values.push(FilterValue {
                    value: value.to_string(),
                    label,
                    count: tag.count.max(0),
                });
            }
            if values.is_empty() {
                return None;
            }
            values.sort_by_cached_key(|v| sort_key(category, v));
            Some(Group { category, values })
        })
        .collect()
}

fn sort_key(category: Category, value: &FilterValue) -> (u8, u64, String) {
    let text = value.label.to_lowercase();
    if category.sorts_numerically() {
        let digits: String = value
            .value
            .chars()
            .take_while(char::is_ascii_digit)
            .collect();
        if let Ok(n) = digits.parse::<u64>() {
            return (0, n, text);
        }
        return (1, 0, text);
    }
    // The built-in collections lead, then the decks by name.
    if category == Category::Lists && deck_id(&value.value).is_some() {
        return (1, 0, text);
    }
    (0, 0, text)
}

/// The deck a membership tag value names: `deck:<id>`.
pub fn deck_id(value: &str) -> Option<&str> {
    value.strip_prefix(DECK_PREFIX).filter(|id| !id.is_empty())
}

/// The name a collection goes by, where its tag value alone would not
/// read as one.
fn collection_name(category: Category, value: &str) -> Option<&'static str> {
    if category != Category::Lists {
        return None;
    }
    match value {
        "favorite" => Some("Favorites"),
        "liked" => Some("Liked"),
        "playlater" => Some("Play later"),
        _ => None,
    }
}

/// Core's label, or the tag's own value when it sends none: the last part
/// of a nested value (`action:platformer` is `platformer`), otherwise as
/// written. Values are acronyms, proper names and codes as often as words
/// (`us`, `snk`, `2`), so recasing them reads worse than leaving them.
pub fn display_label(label: &str, value: &str) -> String {
    let label = label.trim();
    if !label.is_empty() {
        return label.to_string();
    }
    value.rsplit(':').next().unwrap_or(value).trim().to_string()
}

/// The value selected for `category`, if any.
pub fn selected(tags: &[String], category: Category) -> Option<&str> {
    let prefix = category.tag_type();
    tags.iter().find_map(|t| {
        let (tag_type, value) = t.split_once(':')?;
        (tag_type == prefix && !value.is_empty() && category.accepts(value)).then_some(value)
    })
}

/// `tags` with `category` set to `value`, or cleared when `value` is `None`,
/// in menu order. The other categories are untouched.
pub fn with_selection(tags: &[String], category: Category, value: Option<&str>) -> Vec<String> {
    let mut next: Vec<String> = tags
        .iter()
        .filter(|t| {
            t.split_once(':')
                .is_none_or(|(tag_type, _)| tag_type != category.tag_type())
        })
        .cloned()
        .collect();
    if let Some(value) = value.map(str::trim).filter(|v| !v.is_empty()) {
        next.push(format!("{}:{value}", category.tag_type()));
    }
    normalize(&next)
}

/// Keep only tags the picker could have produced: a known category, one per
/// category, in menu order. Guards a hand-edited or older state file.
pub fn normalize(tags: &[String]) -> Vec<String> {
    Category::ALL
        .into_iter()
        .filter_map(|category| {
            selected(tags, category).map(|value| format!("{}:{value}", category.tag_type()))
        })
        .collect()
}

/// The one-line cue for a filter: the first selected value (menu order),
/// then `+N` for the N more behind it ("Platformer +1"). `label_of` resolves
/// a `type:value` tag to Core's label when the tag list is loaded; otherwise
/// the raw value is made readable. None when nothing is selected.
pub fn summary(tags: &[String], label_of: impl Fn(&str) -> Option<String>) -> Option<String> {
    let tags = normalize(tags);
    let first = tags.first()?;
    let value = first.split_once(':').map_or("", |(_, v)| v);
    let label =
        label_of(first).map_or_else(|| display_label("", value), |l| display_label(&l, value));
    Some(match tags.len() - 1 {
        0 => label,
        extra => format!("{label} +{extra}"),
    })
}

/// Move `index` by one page of `visible` rows toward the start (`forward`
/// false) or end, stopping at the edges rather than wrapping.
pub fn page_step(index: usize, len: usize, visible: usize, forward: bool) -> usize {
    if len == 0 {
        return 0;
    }
    let step = visible.max(1);
    let index = index.min(len - 1);
    if forward {
        (index + step).min(len - 1)
    } else {
        index.saturating_sub(step)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tag(tag_type: &str, value: &str, label: &str, count: i64) -> FilterTag {
        FilterTag {
            tag_type: tag_type.into(),
            value: value.into(),
            label: label.into(),
            count,
        }
    }

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn groups_follow_menu_order_and_drop_unlisted_types() {
        let tags = vec![
            tag("region", "us", "USA", 40),
            tag("rev", "1", "", 3),
            tag("genre", "action", "Action", 90),
            tag("scraper.gamelistxml", "x", "", 1),
            tag("user", "hidden", "", 2),
            tag("user", "favorite", "", 5),
            tag("user", "deck:abc", "", 1),
        ];
        let groups = group(&tags);
        let order: Vec<_> = groups.iter().map(|g| g.category).collect();
        assert_eq!(order, [Category::Genre, Category::Region, Category::Lists]);
        assert_eq!(groups[2].values.len(), 1);
        assert_eq!(groups[2].values[0].value, "favorite");
    }

    #[test]
    fn years_and_players_sort_by_magnitude() {
        let tags = vec![
            tag("year", "1994", "", 0),
            tag("year", "985", "", 0),
            tag("players", "10", "", 0),
            tag("players", "2", "", 0),
            tag("players", "vs", "Versus", 0),
            tag("players", "1", "", 0),
        ];
        let groups = group(&tags);
        let years: Vec<_> = groups[0].values.iter().map(|v| v.value.as_str()).collect();
        assert_eq!(years, ["985", "1994"]);
        let players: Vec<_> = groups[1].values.iter().map(|v| v.value.as_str()).collect();
        assert_eq!(players, ["1", "2", "10", "vs"]);
    }

    #[test]
    fn other_categories_sort_by_label_ignoring_case() {
        let tags = vec![
            tag("genre", "sports", "sports", 1),
            tag("genre", "action", "Action", 1),
            tag("genre", "puzzle", "Puzzle", 1),
        ];
        let labels: Vec<_> = group(&tags)[0]
            .values
            .iter()
            .map(|v| v.label.clone())
            .collect();
        assert_eq!(labels, ["Action", "Puzzle", "sports"]);
    }

    #[test]
    fn empty_and_duplicate_values_are_dropped() {
        let tags = vec![
            tag("genre", " ", "", 1),
            tag("genre", "action", "", 1),
            tag("genre", "action", "Action", 2),
            tag("Genre", "puzzle", "", -4),
        ];
        let values = &group(&tags)[0].values;
        assert_eq!(values.len(), 2);
        assert_eq!(values[1].count, 0);
    }

    #[test]
    fn list_tags_get_readable_names() {
        let tags = vec![
            tag("user", "playlater", "", 1),
            tag("user", "favorite", "", 1),
            tag("user", "liked", "Liked games", 1),
        ];
        let groups = group(&tags);
        let labels: Vec<_> = groups[0].values.iter().map(|v| v.label.as_str()).collect();
        assert_eq!(labels, ["Favorites", "Liked games", "Play later"]);
    }

    #[test]
    fn decks_join_the_collections_by_name_after_the_built_ins() {
        let tags = vec![
            tag("user", "deck:0k3v9x2rq7bm", "Couch co-op", 14),
            tag("user", "playlater", "", 1),
            tag("user", "deck:zz00unnamed0", "", 3),
            tag("user", "deck:aa11bb22cc33", "Arcade night", 9),
            tag("user", "favorite", "", 1),
            tag("user", "hidden", "", 2),
            tag("user", "deck:", "Broken", 2),
        ];
        let groups = group(&tags);
        let values: Vec<_> = groups[0]
            .values
            .iter()
            .map(|v| (v.value.as_str(), v.label.as_str()))
            .collect();
        assert_eq!(
            values,
            [
                ("favorite", "Favorites"),
                ("playlater", "Play later"),
                ("deck:aa11bb22cc33", "Arcade night"),
                ("deck:0k3v9x2rq7bm", "Couch co-op"),
            ]
        );
        // A deck is one choice among the collections, like any other.
        let chosen = with_selection(&[], Category::Lists, Some("deck:0k3v9x2rq7bm"));
        assert_eq!(chosen, ["user:deck:0k3v9x2rq7bm"]);
        assert_eq!(
            selected(&chosen, Category::Lists),
            Some("deck:0k3v9x2rq7bm")
        );
        assert_eq!(
            with_selection(&chosen, Category::Lists, Some("favorite")),
            ["user:favorite"]
        );
        assert_eq!(deck_id("deck:abc"), Some("abc"));
        assert_eq!(deck_id("deck:"), None);
        assert_eq!(deck_id("favorite"), None);
    }

    #[test]
    fn labels_fall_back_to_a_readable_value() {
        assert_eq!(
            display_label("Platformer", "action:platformer"),
            "Platformer"
        );
        assert_eq!(display_label("", "action:platformer"), "platformer");
        assert_eq!(display_label("", "run-and-gun"), "run-and-gun");
        assert_eq!(display_label("  ", "us"), "us");
        assert_eq!(display_label("", "SNK"), "SNK");
    }

    #[test]
    fn selection_holds_one_value_per_category_in_menu_order() {
        let mut tags = Vec::new();
        tags = with_selection(&tags, Category::Region, Some("us"));
        tags = with_selection(&tags, Category::Genre, Some("action"));
        assert_eq!(tags, strings(&["genre:action", "region:us"]));
        tags = with_selection(&tags, Category::Genre, Some("rpg"));
        assert_eq!(tags, strings(&["genre:rpg", "region:us"]));
        assert_eq!(selected(&tags, Category::Region), Some("us"));
        assert_eq!(selected(&tags, Category::Year), None);
        tags = with_selection(&tags, Category::Genre, None);
        assert_eq!(tags, strings(&["region:us"]));
    }

    #[test]
    fn genre_values_keep_their_own_colons() {
        let tags = with_selection(&[], Category::Genre, Some("action:platformer"));
        assert_eq!(tags, strings(&["genre:action:platformer"]));
        assert_eq!(selected(&tags, Category::Genre), Some("action:platformer"));
    }

    #[test]
    fn normalize_drops_what_the_picker_cannot_produce() {
        let tags = strings(&[
            "region:us",
            "region:eu",
            "bogus:thing",
            "user:hidden",
            "user:liked",
            "genre:",
            "plain",
        ]);
        assert_eq!(normalize(&tags), strings(&["region:us", "user:liked"]));
    }

    #[test]
    fn summary_names_the_first_value_and_counts_the_rest() {
        assert_eq!(summary(&[], |_| None), None);
        let tags = strings(&["region:us", "genre:action:platformer", "year:1994"]);
        assert_eq!(summary(&tags, |_| None).as_deref(), Some("platformer +2"));
        assert_eq!(
            summary(&tags, |t| (t == "genre:action:platformer")
                .then(|| "Platform".into()))
            .as_deref(),
            Some("Platform +2")
        );
        assert_eq!(
            summary(&strings(&["year:1994"]), |_| None).as_deref(),
            Some("1994")
        );
    }

    #[test]
    fn category_ids_round_trip() {
        for category in Category::ALL {
            assert_eq!(Category::from_id(category.id()), Some(category));
        }
        assert_eq!(Category::from_id("lang"), None);
        assert_eq!(Category::from_id("nope"), None);
    }

    #[test]
    fn paging_stops_at_the_edges() {
        assert_eq!(page_step(0, 0, 5, true), 0);
        assert_eq!(page_step(0, 20, 5, true), 5);
        assert_eq!(page_step(17, 20, 5, true), 19);
        assert_eq!(page_step(7, 20, 5, false), 2);
        assert_eq!(page_step(2, 20, 5, false), 0);
        assert_eq!(page_step(3, 20, 0, true), 4);
    }
}
