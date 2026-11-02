// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

//! The Menu chip row: what a screen offers as page actions, and how focus
//! moves along the row once the Menu button has put it there.
//!
//! Options acts on the focused item; Menu acts on the screen. Above the
//! 240p tier the screen's page actions sit on the page as a row of chips
//! the Menu button focuses, so they can be seen before they are needed.
//! At 240p there is no room for the row, and a user can turn it off
//! (Settings > Appearance); either way the same actions open as the Menu
//! list. The d-pad never walks into the row from the content: only
//! the Menu button enters it, so grid wrap and paging stay untouched.

/// A screen, as far as its page actions go.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Surface {
    Hub,
    FavoriteSystems,
    /// A system's games: the browse screen.
    Games,
    Favorites,
    SearchResults,
    /// Everything with no page actions: Systems, Recents, Settings, About,
    /// the Search screen itself.
    Other,
}

/// One chip on the row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Chip {
    /// The letter picker. Shows the letter the selection is in.
    GoTo,
    /// The tags picker. Shows the active tags, or that there are none.
    Filter,
    /// Search scoped to the system on screen.
    SearchHere,
    /// The favorites order. Shows the current order.
    Sort,
    /// The favorites grouping. Shows the current grouping.
    GroupBy,
    /// A random favorite.
    RandomFavorite,
    /// Pin the search on screen to the Hub, or take it off.
    AddToHub,
    /// The Hub's Add item picker.
    AddItem,
    /// The rare actions, as the Menu list titled "More".
    More,
}

impl Chip {
    /// The id the adapter routes on. Matches the Menu list row it stands
    /// for where there is one.
    pub fn id(self) -> &'static str {
        match self {
            Self::GoTo => "jump_letter",
            Self::Filter => "filter",
            Self::SearchHere => "search_here",
            Self::Sort => "favorites_sort",
            Self::GroupBy => "favorites_grouping",
            Self::RandomFavorite => "launch_random_favorite",
            Self::AddToHub => "add_to_hub",
            Self::AddItem => "hub_add",
            Self::More => "more",
        }
    }
}

/// The chips a screen shows, in order: the frequent actions first, More
/// last and only where there is something rare to put in it.
pub fn chips(surface: Surface) -> &'static [Chip] {
    match surface {
        Surface::Hub => &[Chip::AddItem, Chip::More],
        Surface::Games => &[Chip::GoTo, Chip::Filter, Chip::SearchHere],
        Surface::Favorites => &[Chip::Sort, Chip::GroupBy, Chip::RandomFavorite, Chip::More],
        Surface::FavoriteSystems => &[Chip::GroupBy],
        Surface::SearchResults => &[Chip::AddToHub],
        Surface::Other => &[],
    }
}

/// The rows behind a screen's More chip, as Menu list row ids.
pub fn more_rows(surface: Surface) -> &'static [&'static str] {
    match surface {
        Surface::Hub => &["hub_reset", "hub_settings", "hub_quit"],
        Surface::Favorites => &["back_to_hub"],
        _ => &[],
    }
}

/// Whether the screen reserves and draws the row: it has page actions and
/// the scene has a row (`sizing::Derived::menu_row`: above 240p, with the
/// setting on).
pub fn has_row(surface: Surface, scene_row: bool) -> bool {
    scene_row && !chips(surface).is_empty()
}

/// Whether the Menu button focuses the row rather than opening the Menu
/// list. The row has to be on screen, and in dual-head mode the CRT head
/// shows the 240p layout, where the row does not exist: the list is the
/// one surface both heads can show.
pub fn row_takes_menu(surface: Surface, scene_row: bool, dual_head: bool) -> bool {
    has_row(surface, scene_row) && !dual_head
}

/// Left and Right along the row. Wraps, like every other focus walk.
pub fn step(index: usize, count: usize, forward: bool) -> usize {
    if count == 0 {
        return 0;
    }
    let index = index.min(count - 1);
    if forward {
        (index + 1) % count
    } else {
        index.checked_sub(1).unwrap_or(count - 1)
    }
}

/// What an action does while the row has focus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Focus moves to this chip.
    Focus(usize),
    /// The focused chip runs.
    Activate(usize),
    /// Focus goes back to the content, where it was.
    Leave,
    /// Nothing happens.
    Stay,
}

/// One action on the row. Back, the Menu button and Down all return to
/// the content; Up has nowhere to go.
pub fn action(action: &str, index: usize, count: usize) -> Outcome {
    match action {
        "left" | "right" if count > 0 => Outcome::Focus(step(index, count, action == "right")),
        "accept" if index < count => Outcome::Activate(index),
        "cancel" | "page_menu" | "down" => Outcome::Leave,
        _ => Outcome::Stay,
    }
}

/// How a picker a chip opened was closed, to decide where focus goes.
/// Backing out returns to the chip, so a look that changed nothing costs
/// nothing; a pick returns to the content it changed.
pub fn returns_to_row(closing_action: &str) -> bool {
    matches!(closing_action, "cancel" | "page_menu")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_screen_with_page_actions_has_a_row_where_the_scene_does() {
        for surface in [
            Surface::Hub,
            Surface::FavoriteSystems,
            Surface::Games,
            Surface::Favorites,
            Surface::SearchResults,
        ] {
            assert!(has_row(surface, true), "{surface:?}");
            assert!(!has_row(surface, false), "{surface:?}");
        }
        assert!(!has_row(Surface::Other, true));
    }

    #[test]
    fn dual_head_keeps_the_menu_list() {
        assert!(row_takes_menu(Surface::Games, true, false));
        assert!(!row_takes_menu(Surface::Games, true, true));
        assert!(!row_takes_menu(Surface::Games, false, false));
    }

    #[test]
    fn more_is_last_and_only_where_it_has_rows() {
        for surface in [
            Surface::Hub,
            Surface::FavoriteSystems,
            Surface::Games,
            Surface::Favorites,
            Surface::SearchResults,
        ] {
            let chips = chips(surface);
            let has_more = chips.contains(&Chip::More);
            assert_eq!(has_more, !more_rows(surface).is_empty(), "{surface:?}");
            if has_more {
                assert_eq!(chips.last(), Some(&Chip::More), "{surface:?}");
            }
        }
    }

    #[test]
    fn go_to_leads_the_games_row_and_quit_sits_behind_more() {
        assert_eq!(chips(Surface::Games).first(), Some(&Chip::GoTo));
        assert_eq!(more_rows(Surface::Hub).last(), Some(&"hub_quit"));
    }

    #[test]
    fn left_and_right_wrap_along_the_row() {
        assert_eq!(step(0, 3, true), 1);
        assert_eq!(step(2, 3, true), 0);
        assert_eq!(step(0, 3, false), 2);
        assert_eq!(step(5, 3, false), 1);
        assert_eq!(step(0, 1, true), 0);
        assert_eq!(step(0, 0, true), 0);
    }

    #[test]
    fn back_menu_and_down_leave_and_up_stays() {
        assert_eq!(action("right", 0, 2), Outcome::Focus(1));
        assert_eq!(action("left", 0, 2), Outcome::Focus(1));
        assert_eq!(action("accept", 1, 2), Outcome::Activate(1));
        assert_eq!(action("accept", 2, 2), Outcome::Stay);
        assert_eq!(action("cancel", 0, 2), Outcome::Leave);
        assert_eq!(action("page_menu", 0, 2), Outcome::Leave);
        assert_eq!(action("down", 0, 2), Outcome::Leave);
        assert_eq!(action("up", 0, 2), Outcome::Stay);
        assert_eq!(action("page_next", 0, 2), Outcome::Stay);
    }

    #[test]
    fn backing_out_of_a_picker_returns_to_the_chip() {
        assert!(returns_to_row("cancel"));
        assert!(returns_to_row("page_menu"));
        assert!(!returns_to_row("accept"));
    }
}
