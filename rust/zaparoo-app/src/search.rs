// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

//! The Search screen: what makes a search worth running, how its match
//! count reads, and how focus moves between the filter rows, the keyboard
//! and the pane beside it.

use crate::browse_filter::{self, Group};
use crate::keyboard::Edge;

/// Characters a query may hold.
pub const QUERY_MAX_CHARS: usize = 48;

/// Quiet time after the last edit before the preview is fetched.
pub const PREVIEW_DEBOUNCE_MS: u64 = 250;

/// Matches the preview asks Core for; beyond it the count reads "100+".
pub const PREVIEW_LIMIT: u32 = 100;

/// Core has no relevance ranking, so results read alphabetically.
pub const RESULT_SORT: &str = "name-asc";

/// The query with the padding a caret edit can leave trimmed off.
pub fn query_text(query: &str) -> &str {
    query.trim()
}

/// Whether there is anything to search on. An empty query still searches
/// when a system, a tag or a folder narrows it.
pub fn can_search(query: &str, systems: &[String], tags: &[String], path_prefix: &str) -> bool {
    !query_text(query).is_empty()
        || !systems.is_empty()
        || !tags.is_empty()
        || !path_prefix.is_empty()
}

/// What "Search here" limited a search to.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Scope {
    /// Not limited: the Hub's Search, a recent search, a saved one.
    #[default]
    None,
    /// The system on a browse screen and, inside a folder, that folder.
    Browse,
    /// The systems of one category.
    Category,
    /// The favorites, all of them or one system's, from the Favorites list.
    Favorites,
    /// The favorites, from the systems that hold them.
    FavoriteSystems,
}

/// The tag Core keeps favorites under.
pub const FAVORITE_TAG: &str = "user:favorite";

impl Scope {
    /// The kind as the state file keeps it (`persist::SEARCH_SCOPE_*`).
    pub fn token(self) -> &'static str {
        match self {
            Self::None => "",
            Self::Browse => "browse",
            Self::Category => "category",
            Self::Favorites => "favorites",
            Self::FavoriteSystems => "favorite_systems",
        }
    }

    /// A kind read back from the state file; anything else is no scope.
    pub fn from_token(token: &str) -> Self {
        match token {
            "browse" => Self::Browse,
            "category" => Self::Category,
            "favorites" => Self::Favorites,
            "favorite_systems" => Self::FavoriteSystems,
            _ => Self::None,
        }
    }

    /// The tags Core is sent: the chosen ones, plus the one the scope
    /// itself stands for. The scope's tag is kept apart from the chosen
    /// ones so the Tags field cannot take it away.
    pub fn request_tags(self, chosen: Vec<String>) -> Vec<String> {
        let mut tags = chosen;
        let favorites = matches!(self, Self::Favorites | Self::FavoriteSystems);
        if favorites && !tags.iter().any(|t| t == FAVORITE_TAG) {
            tags.push(FAVORITE_TAG.to_string());
        }
        tags
    }
}

/// Whether Core should include hidden games: when the setting says so, or
/// when a chosen tag is a user tag (a List), which browse answers with
/// hidden games included. The favorites scope's own tag does not count,
/// as the Favorites list itself does not.
pub fn includes_hidden(show_hidden: bool, chosen: &[String]) -> bool {
    show_hidden || chosen.iter().any(|t| t.starts_with("user:"))
}

/// A saved search's systems or tags as its Hub tile wrote them: trimmed,
/// without blanks or repeats, in the tile's order. Unlike the picker's
/// `browse_filter::normalize`, no tag is dropped for its type or for
/// sharing one: Core decides what a tag means.
pub fn tile_list(values: &[String]) -> Vec<String> {
    let mut kept: Vec<String> = Vec::with_capacity(values.len());
    for value in values {
        let value = value.trim();
        if !value.is_empty() && !kept.iter().any(|seen| seen == value) {
            kept.push(value.to_string());
        }
    }
    kept
}

/// The search's written tags the tags picker cannot show, as written: a
/// type it has no category for (`series:...`, a revision), or a value its
/// category cannot hold (a second genre, `user:hidden`). Only a tag search
/// from Game Info or a Hub tile puts one there.
pub fn extra_tags(tags: &[String]) -> Vec<String> {
    let shown = browse_filter::normalize(tags);
    tile_list(tags)
        .into_iter()
        .filter(|tag| !shown.contains(tag))
        .collect()
}

/// The picker category a tag belongs to, if any.
fn category_of(tag: &str) -> Option<browse_filter::Category> {
    let tag_type = tag.split_once(':').map_or(tag, |(t, _)| t);
    browse_filter::Category::ALL
        .into_iter()
        .find(|category| category.tag_type() == tag_type)
}

/// `written`'s extras that still hold beside `picked`: one with no
/// category always does; one with a category only while that category's
/// choice is the one it was, so changing a category replaces all of it
/// and changing another leaves it alone.
fn kept_extras(written: &[String], picked: &[String]) -> Vec<String> {
    let before = browse_filter::normalize(written);
    extra_tags(written)
        .into_iter()
        .filter(|tag| {
            category_of(tag).is_none_or(|category| {
                browse_filter::selected(&before, category)
                    == browse_filter::selected(picked, category)
            })
        })
        .collect()
}

/// A written search's tags after the picker set `picked`: the picker's,
/// then the extras it did not touch (`kept_extras`).
pub fn with_picked(written: &[String], picked: Vec<String>) -> Vec<String> {
    let extras = kept_extras(written, &picked);
    [picked, extras].concat()
}

/// The search's tags: the picker's, one per category in menu order, then
/// the extras it cannot show (`extra_tags`).
pub fn search_tags(tags: &[String]) -> Vec<String> {
    let mut all = browse_filter::normalize(tags);
    all.extend(extra_tags(tags));
    all
}

/// How many games match, as far as one page can tell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Count {
    Exact(usize),
    /// Core reported more pages; it gives no total.
    AtLeast(usize),
}

pub fn count(results: usize, has_more: bool) -> Count {
    if has_more {
        Count::AtLeast(results)
    } else {
        Count::Exact(results)
    }
}

/// The matches the pane beside the keyboard lists, as indices into Core's
/// page: the first of each title on each system. Variants of one title read
/// alike there, so the pane names it once and the results tell them apart.
pub fn pane_matches<'a>(matches: impl IntoIterator<Item = (&'a str, &'a str)>) -> Vec<usize> {
    let mut seen = std::collections::HashSet::new();
    matches
        .into_iter()
        .enumerate()
        .filter_map(|(index, key)| seen.insert(key).then_some(index))
        .collect()
}

/// Core lists at most this many values of a long-tail tag type (developer,
/// publisher), so a list this long may be missing ones that exist.
pub const TAG_VALUE_CAP: usize = 100;

/// `tags` without the ones `groups` shows the new system lacks, for a
/// change of system. A tag is dropped when its category is not offered at
/// all, or is listed in full without it; a capped list proves nothing, so
/// its tag stays. Tags are kept as they are while the groups have not
/// loaded.
pub fn prune_tags(tags: &[String], groups: &[Group]) -> Vec<String> {
    let tags = browse_filter::normalize(tags);
    if groups.is_empty() {
        return tags;
    }
    tags.into_iter()
        .filter(|tag| {
            tag.split_once(':').is_some_and(|(tag_type, value)| {
                groups
                    .iter()
                    .find(|g| g.category.tag_type() == tag_type)
                    .is_some_and(|g| {
                        g.values.len() >= TAG_VALUE_CAP || g.values.iter().any(|v| v.value == value)
                    })
            })
        })
        .collect()
}

/// `prune_tags` for a search whose tags are kept as written (a Hub tile's,
/// a tag search from Game Info): the tags the picker cannot show stay, as
/// no tag list names them to rule them out.
pub fn prune_written_tags(tags: &[String], groups: &[Group]) -> Vec<String> {
    with_picked(tags, prune_tags(tags, groups))
}

/// The focusable regions: the two fields above the keyboard, the
/// keyboard, and the pane beside them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Zone {
    System,
    Filter,
    #[default]
    Keys,
    Pane,
}

/// Where focus is outside the keyboard's own key position.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Focus {
    pub zone: Zone,
    pub pane_index: usize,
    /// Where the pane was entered from, so Left goes back there.
    pub pane_from: Zone,
}

/// What the screen currently offers.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Shape {
    /// A folder search: the system field states the scope and takes no
    /// focus.
    pub scoped: bool,
    /// Stops in the pane; 0 when it is collapsed or empty.
    pub pane_rows: usize,
}

impl Shape {
    /// With no pane to step into, the keyboard rows wrap on themselves.
    /// With one, a row and the pane beside it wrap as a loop instead.
    pub fn keys_wrap(self) -> bool {
        self.pane_rows == 0
    }

    /// The first field of the row above the keyboard.
    fn first_field(self) -> Zone {
        if self.scoped {
            Zone::Filter
        } else {
            Zone::System
        }
    }

    /// The field above a keyboard unit: System over the left half, Filter
    /// over the right.
    fn field_above(self, unit: usize) -> Zone {
        if unit * 2 < crate::keyboard::COLUMNS {
            self.first_field()
        } else {
            Zone::Filter
        }
    }
}

/// Keep a focus valid after the fields or the pane changed under it.
pub fn settle(focus: Focus, shape: Shape) -> Focus {
    let mut focus = focus;
    focus.pane_index = focus.pane_index.min(shape.pane_rows.saturating_sub(1));
    if focus.zone == Zone::Pane && shape.pane_rows == 0 {
        focus.zone = Zone::Keys;
    }
    if focus.zone == Zone::System && shape.scoped {
        focus.zone = Zone::Filter;
    }
    focus
}

fn enter_pane(mut focus: Focus, from: Zone) -> Focus {
    focus.pane_from = from;
    focus.zone = Zone::Pane;
    focus
}

/// What a move out of the fields or the pane did to keyboard focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Landing {
    /// Focus is not newly on the keyboard, or returns to the key it left.
    Unchanged,
    /// Onto the top key row, under where focus was.
    TopRow,
    /// Onto the bottom key row, under where focus was.
    BottomRow,
    /// Onto the first key of the row focus left, having gone round.
    RowStart,
}

/// A directional press outside the keyboard. Every edge wraps: the fields
/// and the key rows are one vertical loop, a row and the pane beside it
/// one horizontal loop, and the pane's own rows loop top to bottom.
pub fn step(focus: Focus, action: &str, shape: Shape) -> (Focus, Landing) {
    let mut next = settle(focus, shape);
    let has_pane = shape.pane_rows > 0;
    let mut landing = Landing::Unchanged;
    match (next.zone, action) {
        (Zone::System, "left") if has_pane => next = enter_pane(next, Zone::System),
        (Zone::System, "left" | "right") => next.zone = Zone::Filter,
        (Zone::Filter, "left") if !shape.scoped => next.zone = Zone::System,
        (Zone::Filter, "left" | "right") if has_pane => next = enter_pane(next, Zone::Filter),
        (Zone::Filter, "right") => next.zone = shape.first_field(),
        (Zone::System | Zone::Filter, "down") => {
            next.zone = Zone::Keys;
            landing = Landing::TopRow;
        }
        (Zone::System | Zone::Filter, "up") => {
            next.zone = Zone::Keys;
            landing = Landing::BottomRow;
        }
        (Zone::Pane, "left") => next.zone = next.pane_from,
        (Zone::Pane, "right") => {
            if next.pane_from == Zone::Keys {
                next.zone = Zone::Keys;
                landing = Landing::RowStart;
            } else {
                next.zone = shape.first_field();
            }
        }
        (Zone::Pane, "up") => {
            next.pane_index = next
                .pane_index
                .checked_sub(1)
                .unwrap_or(shape.pane_rows.saturating_sub(1));
        }
        (Zone::Pane, "down") => {
            next.pane_index = if next.pane_index + 1 < shape.pane_rows {
                next.pane_index + 1
            } else {
                0
            };
        }
        _ => {}
    }
    (settle(next, shape), landing)
}

/// Focus after the keyboard reported an edge at `unit` along its row. Up
/// and Down both reach the fields, which sit between the bottom key row
/// and the top one in the loop; Left and Right reach the pane.
pub fn leave_keys(focus: Focus, edge: Edge, unit: usize, shape: Shape) -> Focus {
    let mut next = settle(focus, shape);
    match edge {
        Edge::Up | Edge::Down => next.zone = shape.field_above(unit),
        Edge::Left | Edge::Right if shape.pane_rows > 0 => next = enter_pane(next, Zone::Keys),
        Edge::Left | Edge::Right => {}
    }
    next
}

/// The first row of a list window `visible` rows tall that keeps `index`
/// on show, moving the current `top` only as far as it must.
pub fn window_top(top: usize, index: usize, len: usize, visible: usize) -> usize {
    let visible = visible.max(1);
    let top = if index < top {
        index
    } else if index >= top + visible {
        index + 1 - visible
    } else {
        top
    };
    top.min(len.saturating_sub(visible))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::browse_filter::{Category, FilterValue};

    fn tags(list: &[&str]) -> Vec<String> {
        list.iter().map(ToString::to_string).collect()
    }

    fn focus(zone: Zone, pane_index: usize) -> Focus {
        Focus {
            zone,
            pane_index,
            pane_from: Zone::Keys,
        }
    }

    fn go(focus: Focus, action: &str, shape: Shape) -> Focus {
        step(focus, action, shape).0
    }

    #[test]
    fn a_written_searchs_tags_stay_until_their_own_category_changes() {
        let written: Vec<String> = ["genre:rpg", "genre:action", "user:hidden", "year:1994"]
            .map(String::from)
            .to_vec();
        // The picker shows one genre and the year; the rest are extras.
        assert_eq!(extra_tags(&written), ["genre:action", "user:hidden"]);
        // A new year leaves both genres and the hidden tag alone.
        let picked = browse_filter::with_selection(
            &browse_filter::normalize(&written),
            Category::Year,
            Some("1995"),
        );
        assert_eq!(
            with_picked(&written, picked),
            ["genre:rpg", "year:1995", "genre:action", "user:hidden"]
        );
        // A new genre replaces both.
        let picked = browse_filter::with_selection(
            &browse_filter::normalize(&written),
            Category::Genre,
            Some("racing"),
        );
        assert_eq!(
            with_picked(&written, picked),
            ["genre:racing", "year:1994", "user:hidden"]
        );
    }

    #[test]
    fn tags_the_picker_cannot_show_survive_it_and_a_change_of_system() {
        let tags: Vec<String> = ["series:Resident Evil", "genre:horror", "user:hidden"]
            .map(String::from)
            .to_vec();
        // `user:hidden` is a Lists tag the picker cannot select: an extra too.
        assert_eq!(extra_tags(&tags), ["series:Resident Evil", "user:hidden"]);
        assert_eq!(
            search_tags(&tags),
            ["genre:horror", "series:Resident Evil", "user:hidden"]
        );
        let groups = vec![Group {
            category: Category::Players,
            values: Vec::new(),
        }];
        // The new system has no genre list: the genre goes, the rest stays.
        assert_eq!(
            prune_written_tags(&tags, &groups),
            ["series:Resident Evil", "user:hidden"]
        );
        assert_eq!(
            prune_written_tags(&tags, &[]),
            ["genre:horror", "series:Resident Evil", "user:hidden"]
        );
        assert!(prune_tags(&tags, &groups).is_empty());
    }

    #[test]
    fn a_search_needs_a_query_or_something_narrowing_it() {
        assert!(!can_search("", &[], &[], ""));
        assert!(!can_search("   ", &[], &[], ""));
        assert!(can_search(" mario ", &[], &[], ""));
        assert!(can_search("", &tags(&["SNES"]), &[], ""));
        assert!(can_search("", &[], &tags(&["genre:rpg"]), ""));
        assert!(can_search("", &[], &[], "/roms/SNES/RPG"));
        assert_eq!(query_text("  mario kart "), "mario kart");
    }

    #[test]
    fn a_scope_round_trips_its_token_and_drops_an_unknown_one() {
        for scope in [
            Scope::None,
            Scope::Browse,
            Scope::Category,
            Scope::Favorites,
            Scope::FavoriteSystems,
        ] {
            assert_eq!(Scope::from_token(scope.token()), scope);
        }
        assert_eq!(Scope::from_token("decks"), Scope::None);
    }

    #[test]
    fn the_favorites_scope_adds_its_tag_once() {
        assert_eq!(
            Scope::Favorites.request_tags(tags(&["genre:rpg"])),
            tags(&["genre:rpg", FAVORITE_TAG])
        );
        assert_eq!(
            Scope::Favorites.request_tags(tags(&[FAVORITE_TAG])),
            tags(&[FAVORITE_TAG])
        );
        assert_eq!(
            Scope::FavoriteSystems.request_tags(Vec::new()),
            tags(&[FAVORITE_TAG])
        );
        assert_eq!(
            Scope::Browse.request_tags(tags(&["genre:rpg"])),
            tags(&["genre:rpg"])
        );
        assert!(can_search(
            "",
            &[],
            &Scope::Favorites.request_tags(Vec::new()),
            ""
        ));
    }

    #[test]
    fn a_chosen_user_tag_includes_hidden_games() {
        assert!(!includes_hidden(false, &tags(&["genre:rpg"])));
        assert!(includes_hidden(false, &tags(&["user:deck:abc"])));
        assert!(includes_hidden(true, &[]));
    }

    #[test]
    fn a_tiles_lists_keep_everything_but_blanks_and_repeats() {
        assert_eq!(
            tile_list(&tags(&[
                " genre:rpg ",
                "genre:action",
                "",
                "-region:jp",
                "bogus:x",
                "genre:rpg",
            ])),
            tags(&["genre:rpg", "genre:action", "-region:jp", "bogus:x"])
        );
    }

    #[test]
    fn the_count_is_exact_until_core_reports_more() {
        assert_eq!(count(12, false), Count::Exact(12));
        assert_eq!(count(0, false), Count::Exact(0));
        assert_eq!(count(100, true), Count::AtLeast(100));
    }

    #[test]
    fn the_pane_names_each_title_once_per_system() {
        let page = [
            ("Chrono Cross", "PSX"),
            ("Chrono Cross", "PSX"),
            ("Chrono Trigger", "SNES"),
            ("Chrono Trigger", "PSX"),
            ("Chrono Cross", "PSX"),
            ("Chrono Trigger", "SNES"),
        ];
        assert_eq!(pane_matches(page), vec![0, 2, 3]);
        assert!(pane_matches([]).is_empty());
    }

    #[test]
    fn the_pane_lists_a_whole_page_of_distinct_titles() {
        let titles: Vec<String> = (0..PREVIEW_LIMIT).map(|n| format!("Game {n}")).collect();
        let page = titles.iter().map(|title| (title.as_str(), "SNES"));
        assert_eq!(pane_matches(page).len(), titles.len());
    }

    #[test]
    fn pruning_drops_tags_the_new_system_lacks() {
        let groups = vec![Group {
            category: Category::Genre,
            values: vec![FilterValue {
                value: "rpg".into(),
                label: "RPG".into(),
                count: 3,
            }],
        }];
        let chosen = tags(&["genre:rpg", "year:1994", "bogus:x"]);
        assert_eq!(prune_tags(&chosen, &groups), tags(&["genre:rpg"]));
        // Before the tag list lands nothing can be judged, so only the
        // shape is tidied.
        assert_eq!(prune_tags(&chosen, &[]), tags(&["genre:rpg", "year:1994"]));
        // A capped list may simply have left the value out.
        let capped = vec![Group {
            category: Category::Developer,
            values: (0..TAG_VALUE_CAP)
                .map(|n| FilterValue {
                    value: format!("studio{n}"),
                    label: String::new(),
                    count: 1,
                })
                .collect(),
        }];
        assert_eq!(
            prune_tags(&tags(&["developer:obscure"]), &capped),
            tags(&["developer:obscure"])
        );
    }

    #[test]
    fn the_fields_and_key_rows_are_one_vertical_loop() {
        let shape = Shape::default();
        // Either vertical edge of the keyboard reaches the field above
        // that half of it.
        for edge in [Edge::Up, Edge::Down] {
            assert_eq!(
                leave_keys(Focus::default(), edge, 2, shape).zone,
                Zone::System
            );
            assert_eq!(
                leave_keys(Focus::default(), edge, 4, shape).zone,
                Zone::System
            );
            assert_eq!(
                leave_keys(Focus::default(), edge, 5, shape).zone,
                Zone::Filter
            );
            assert_eq!(
                leave_keys(Focus::default(), edge, 9, shape).zone,
                Zone::Filter
            );
        }
        for zone in [Zone::System, Zone::Filter] {
            let (down, landing) = step(focus(zone, 0), "down", shape);
            assert_eq!((down.zone, landing), (Zone::Keys, Landing::TopRow));
            let (up, landing) = step(focus(zone, 0), "up", shape);
            assert_eq!((up.zone, landing), (Zone::Keys, Landing::BottomRow));
        }
    }

    #[test]
    fn the_fields_wrap_between_themselves_without_a_pane() {
        let shape = Shape::default();
        let system = focus(Zone::System, 0);
        let filter = focus(Zone::Filter, 0);
        assert_eq!(go(system, "right", shape).zone, Zone::Filter);
        assert_eq!(go(filter, "right", shape).zone, Zone::System);
        assert_eq!(go(system, "left", shape).zone, Zone::Filter);
        assert_eq!(go(filter, "left", shape).zone, Zone::System);
    }

    #[test]
    fn a_folder_search_gives_the_scope_field_no_focus() {
        let scoped = Shape {
            scoped: true,
            ..Shape::default()
        };
        let up = leave_keys(Focus::default(), Edge::Up, 1, scoped);
        assert_eq!(up.zone, Zone::Filter);
        assert_eq!(go(up, "left", scoped).zone, Zone::Filter);
        assert_eq!(go(up, "right", scoped).zone, Zone::Filter);
        assert_eq!(settle(focus(Zone::System, 0), scoped).zone, Zone::Filter);
        // With a pane, the one field loops through it either way.
        let with_pane = Shape {
            pane_rows: 2,
            ..scoped
        };
        let in_pane = go(up, "left", with_pane);
        assert_eq!(in_pane.zone, Zone::Pane);
        assert_eq!(go(in_pane, "right", with_pane).zone, Zone::Filter);
        assert_eq!(go(in_pane, "left", with_pane).zone, Zone::Filter);
    }

    #[test]
    fn a_key_row_and_the_pane_beside_it_are_one_horizontal_loop() {
        let empty = Shape::default();
        assert!(empty.keys_wrap());
        for edge in [Edge::Left, Edge::Right] {
            assert_eq!(
                leave_keys(Focus::default(), edge, 0, empty).zone,
                Zone::Keys
            );
        }

        let shape = Shape {
            pane_rows: 3,
            ..empty
        };
        assert!(!shape.keys_wrap());
        for edge in [Edge::Left, Edge::Right] {
            let pane = leave_keys(Focus::default(), edge, 9, shape);
            assert_eq!((pane.zone, pane.pane_from), (Zone::Pane, Zone::Keys));
            // Left goes back to the key it left; Right goes round to the
            // start of that row.
            assert_eq!(
                step(pane, "left", shape),
                (focus(Zone::Keys, 0), Landing::Unchanged)
            );
            assert_eq!(
                step(pane, "right", shape),
                (focus(Zone::Keys, 0), Landing::RowStart)
            );
        }
        // The preview emptied while focus sat in it.
        let pane = leave_keys(Focus::default(), Edge::Right, 9, shape);
        assert_eq!(settle(pane, empty).zone, Zone::Keys);
    }

    #[test]
    fn the_fields_loop_through_the_pane_and_it_remembers_where_from() {
        let shape = Shape {
            pane_rows: 3,
            ..Shape::default()
        };
        let from_filter = go(focus(Zone::Filter, 0), "right", shape);
        assert_eq!(
            (from_filter.zone, from_filter.pane_from),
            (Zone::Pane, Zone::Filter)
        );
        assert_eq!(go(from_filter, "left", shape).zone, Zone::Filter);
        assert_eq!(go(from_filter, "right", shape).zone, Zone::System);
        let from_system = go(focus(Zone::System, 0), "left", shape);
        assert_eq!(
            (from_system.zone, from_system.pane_from),
            (Zone::Pane, Zone::System)
        );
        assert_eq!(go(from_system, "left", shape).zone, Zone::System);
        assert_eq!(go(focus(Zone::Filter, 0), "left", shape).zone, Zone::System);
    }

    #[test]
    fn the_pane_rows_loop_top_to_bottom() {
        let shape = Shape {
            pane_rows: 3,
            ..Shape::default()
        };
        let mut f = focus(Zone::Pane, 0);
        for expected in [1, 2, 0, 1] {
            f = go(f, "down", shape);
            assert_eq!(f.pane_index, expected);
        }
        for expected in [0, 2, 1] {
            f = go(f, "up", shape);
            assert_eq!(f.pane_index, expected);
        }
    }

    #[test]
    fn the_list_window_moves_only_as_far_as_focus_needs() {
        assert_eq!(window_top(0, 3, 10, 5), 0);
        assert_eq!(window_top(0, 5, 10, 5), 1);
        assert_eq!(window_top(4, 2, 10, 5), 2);
        assert_eq!(window_top(7, 9, 10, 5), 5);
        // A list that shrank pulls the window back onto it.
        assert_eq!(window_top(6, 2, 4, 5), 0);
        assert_eq!(window_top(0, 0, 0, 0), 0);
    }
}
