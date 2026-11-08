// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

//! The Games browse filter: which of Core's tag types are offered, in what
//! order, and how a selection of values per category maps onto the
//! `type:value` tags `media.browse` takes.
//!
//! Values of one category are alternatives and categories are required
//! together: "this genre or that one, and this region". One value is
//! written plain (`genre:rpg`); two or more are written with Core's `~`
//! operator (`~genre:rpg`, `~genre:action`), which Core groups by tag type.
//! Plain repeats of one type (`genre:rpg` and `genre:action`, both
//! required) are not the picker's to show; a search keeps them as extras.

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

/// The built-in collections worth filtering on.
/// `hidden` is bookkeeping, not something to browse by. Decks join them
/// by name (`deck_id`).
const LIST_VALUES: [&str; 4] = ["favorite", "liked", "playlater", "disliked"];

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
        "disliked" => Some("Disliked"),
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

/// Core takes at most this many tags in one request.
pub const MAX_TAGS: usize = 50;

/// Core's operator for "any of these", grouped by tag type.
const ANY_OF: char = '~';

/// `tag`'s type and value, without a leading `~`: `~genre:rpg` and
/// `genre:rpg` are both `("genre", "rpg")`. A value keeps its own colons.
pub fn split_tag(tag: &str) -> Option<(&str, &str)> {
    tag.strip_prefix(ANY_OF).unwrap_or(tag).split_once(':')
}

/// The values a tag of `category` holds in `tags`, as written: the `~`
/// ones, and the plain ones.
fn written_values(tags: &[String], category: Category) -> (Vec<&str>, Vec<&str>) {
    let mut any: Vec<&str> = Vec::new();
    let mut plain: Vec<&str> = Vec::new();
    for tag in tags {
        let (list, rest) = match tag.strip_prefix(ANY_OF) {
            Some(rest) => (&mut any, rest),
            None => (&mut plain, tag.as_str()),
        };
        let Some((tag_type, value)) = rest.split_once(':') else {
            continue;
        };
        if tag_type == category.tag_type()
            && !value.is_empty()
            && category.accepts(value)
            && !list.contains(&value)
        {
            list.push(value);
        }
    }
    (any, plain)
}

/// The values the picker holds for `category`: two or more `~` tags of its
/// type, or else its one plain tag. Plain repeats (all required) and a lone
/// `~` tag are written by something else and are not the picker's.
pub fn selected_values(tags: &[String], category: Category) -> Vec<&str> {
    let (any, plain) = written_values(tags, category);
    if any.len() >= 2 {
        any
    } else if plain.len() == 1 {
        plain
    } else {
        Vec::new()
    }
}

/// One category's values as the picker holds them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pick {
    pub category: Category,
    pub values: Vec<String>,
}

/// The picker's selection in `tags`, in menu order; categories with no
/// value are left out.
pub fn picks(tags: &[String]) -> Vec<Pick> {
    Category::ALL
        .into_iter()
        .filter_map(|category| {
            let values = selected_values(tags, category);
            (!values.is_empty()).then(|| Pick {
                category,
                values: values.into_iter().map(str::to_string).collect(),
            })
        })
        .collect()
}

/// `picks` as tags: one value plain, two or more each with `~`.
pub fn write(picks: &[Pick]) -> Vec<String> {
    let mut tags = Vec::new();
    for pick in picks {
        let operator = if pick.values.len() >= 2 { "~" } else { "" };
        for value in &pick.values {
            tags.push(format!("{operator}{}:{value}", pick.category.tag_type()));
        }
    }
    tags
}

fn value_count(picks: &[Pick]) -> usize {
    picks.iter().map(|p| p.values.len()).sum()
}

/// `tags` with `value` ticked in `category` when it was not, or unticked
/// when it was, in menu order and the category's values in `order` (the
/// order the picker lists them). A value past `MAX_TAGS` is not added.
pub fn toggle(
    tags: &[String],
    category: Category,
    value: &str,
    order: &[FilterValue],
) -> Vec<String> {
    let value = value.trim();
    let mut picks = picks(tags);
    if value.is_empty() || !category.accepts(value) {
        return write(&picks);
    }
    let total = value_count(&picks);
    let index = picks.iter().position(|p| p.category == category);
    match index {
        Some(i) if picks[i].values.iter().any(|v| v == value) => {
            picks[i].values.retain(|v| v != value);
            if picks[i].values.is_empty() {
                picks.remove(i);
            }
        }
        _ if total >= MAX_TAGS => {}
        Some(i) => picks[i].values.push(value.to_string()),
        None => {
            picks.push(Pick {
                category,
                values: vec![value.to_string()],
            });
            picks.sort_by_key(|p| Category::ALL.iter().position(|c| *c == p.category));
        }
    }
    if let Some(pick) = picks.iter_mut().find(|p| p.category == category) {
        let rank = |v: &String| {
            order
                .iter()
                .position(|o| o.value == *v)
                .unwrap_or(usize::MAX)
        };
        pick.values.sort_by_key(rank);
    }
    write(&picks)
}

/// `tags` with nothing ticked in `category`.
pub fn clear_category(tags: &[String], category: Category) -> Vec<String> {
    let mut picks = picks(tags);
    picks.retain(|p| p.category != category);
    write(&picks)
}

/// Whether the picker can tick another value beside `tags`, every tag the
/// request carries.
pub fn has_room(tags: &[String]) -> bool {
    tags.len() < MAX_TAGS
}

/// Keep only tags the picker could have produced: known categories, each
/// value once, written plain or with `~` as `write` does, in menu order.
/// Guards a hand-edited or older state file.
pub fn normalize(tags: &[String]) -> Vec<String> {
    write(&picks(tags))
}

/// The one-line cue for a filter: the first selected value (menu order),
/// then `+N` for the N more values behind it, in any category ("Platformer
/// +1"). `label_of` resolves a plain `type:value` tag to Core's label when
/// the tag list is loaded; otherwise the raw value is made readable. None when nothing is selected.
pub fn summary(tags: &[String], label_of: impl Fn(&str) -> Option<String>) -> Option<String> {
    let tags = normalize(tags);
    let first = tags.first()?;
    let (tag_type, value) = split_tag(first)?;
    let label = label_of(&format!("{tag_type}:{value}"))
        .map_or_else(|| display_label("", value), |l| display_label(&l, value));
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
            tag("user", "disliked", "", 1),
        ];
        let groups = group(&tags);
        let labels: Vec<_> = groups[0].values.iter().map(|v| v.label.as_str()).collect();
        assert_eq!(
            labels,
            ["Disliked", "Favorites", "Liked games", "Play later"]
        );
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
        let order = &groups[0].values;
        let chosen = toggle(&[], Category::Lists, "deck:0k3v9x2rq7bm", order);
        assert_eq!(chosen, ["user:deck:0k3v9x2rq7bm"]);
        assert_eq!(
            selected_values(&chosen, Category::Lists),
            ["deck:0k3v9x2rq7bm"]
        );
        assert_eq!(
            toggle(&chosen, Category::Lists, "favorite", order),
            ["~user:favorite", "~user:deck:0k3v9x2rq7bm"]
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

    fn values(items: &[&str]) -> Vec<FilterValue> {
        items
            .iter()
            .map(|v| FilterValue {
                value: (*v).to_string(),
                label: (*v).to_string(),
                count: 1,
            })
            .collect()
    }

    #[test]
    fn categories_keep_menu_order_and_values_list_order() {
        let genres = values(&["action", "puzzle", "rpg"]);
        let regions = values(&["eu", "us"]);
        let mut tags = Vec::new();
        tags = toggle(&tags, Category::Region, "us", &regions);
        tags = toggle(&tags, Category::Genre, "rpg", &genres);
        assert_eq!(tags, strings(&["genre:rpg", "region:us"]));
        tags = toggle(&tags, Category::Genre, "action", &genres);
        assert_eq!(tags, strings(&["~genre:action", "~genre:rpg", "region:us"]));
        assert_eq!(selected_values(&tags, Category::Genre), ["action", "rpg"]);
        assert_eq!(selected_values(&tags, Category::Region), ["us"]);
        assert!(selected_values(&tags, Category::Year).is_empty());
        tags = toggle(&tags, Category::Region, "eu", &regions);
        assert_eq!(
            tags,
            strings(&["~genre:action", "~genre:rpg", "~region:eu", "~region:us"])
        );
    }

    #[test]
    fn unticking_down_to_one_value_writes_it_plain() {
        let genres = values(&["action", "rpg"]);
        let tags = strings(&["~genre:action", "~genre:rpg", "region:us"]);
        let tags = toggle(&tags, Category::Genre, "action", &genres);
        assert_eq!(tags, strings(&["genre:rpg", "region:us"]));
        let tags = toggle(&tags, Category::Genre, "rpg", &genres);
        assert_eq!(tags, strings(&["region:us"]));
    }

    #[test]
    fn clearing_a_category_leaves_the_others() {
        let tags = strings(&["~genre:action", "~genre:rpg", "region:us"]);
        assert_eq!(
            clear_category(&tags, Category::Genre),
            strings(&["region:us"])
        );
        assert_eq!(
            clear_category(&tags, Category::Region),
            strings(&["~genre:action", "~genre:rpg"])
        );
    }

    #[test]
    fn plain_repeats_and_a_lone_any_tag_are_not_the_pickers() {
        // Both genres required, as a Hub tile writes them.
        let tags = strings(&["genre:rpg", "genre:action", "region:us"]);
        assert!(selected_values(&tags, Category::Genre).is_empty());
        assert_eq!(normalize(&tags), strings(&["region:us"]));
        assert!(selected_values(&strings(&["~genre:rpg"]), Category::Genre).is_empty());
        // Alternatives beside a required value: the picker holds the
        // alternatives.
        let tags = strings(&["~genre:rpg", "~genre:action", "genre:puzzle"]);
        assert_eq!(selected_values(&tags, Category::Genre), ["rpg", "action"]);
    }

    #[test]
    fn values_keep_unknown_ones_after_the_listed_ones() {
        let genres = values(&["action", "rpg"]);
        let tags = toggle(&strings(&["genre:zzz"]), Category::Genre, "action", &genres);
        assert_eq!(tags, strings(&["~genre:action", "~genre:zzz"]));
    }

    #[test]
    fn ticking_stops_at_cores_tag_limit() {
        let years: Vec<String> = (0..MAX_TAGS).map(|n| format!("{}", 1950 + n)).collect();
        let order: Vec<FilterValue> = values(&years.iter().map(String::as_str).collect::<Vec<_>>());
        let mut tags = Vec::new();
        for year in &years {
            tags = toggle(&tags, Category::Year, year, &order);
        }
        assert_eq!(tags.len(), MAX_TAGS);
        assert!(!has_room(&tags));
        assert_eq!(toggle(&tags, Category::Genre, "rpg", &order), tags);
        let fewer = toggle(&tags, Category::Year, "1950", &order);
        assert_eq!(fewer.len(), MAX_TAGS - 1);
        assert!(has_room(&fewer));
    }

    #[test]
    fn genre_values_keep_their_own_colons() {
        let tags = toggle(&[], Category::Genre, "action:platformer", &[]);
        assert_eq!(tags, strings(&["genre:action:platformer"]));
        assert_eq!(
            selected_values(&tags, Category::Genre),
            ["action:platformer"]
        );
        assert_eq!(
            split_tag("~genre:action:platformer"),
            Some(("genre", "action:platformer"))
        );
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
            "~lang:en",
            "~lang:ja",
            "~lang:en",
            "-genre:rpg",
            "+year:1994",
        ]);
        assert_eq!(
            normalize(&tags),
            strings(&["~lang:en", "~lang:ja", "user:liked"])
        );
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
        let tags = strings(&["~genre:shmup", "~genre:action:platformer", "year:1994"]);
        assert_eq!(
            summary(&tags, |t| (t == "genre:shmup").then(|| "Shooter".into())).as_deref(),
            Some("Shooter +2")
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
