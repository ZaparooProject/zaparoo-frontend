// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// The built-in lists' screens: Favorites, Liked, Play later and Disliked
// share the Favorites list screen and its systems screen, and this says
// which list is on them and where each keeps its state. Favorites keeps the
// sections and settings it had before the other lists (`persist.favorites`,
// `persist.favorite_systems`, `favorites_sort` in the config, the
// `favorites_grouping` setting); the others keep theirs in `persist.lists`.

use slint::ComponentHandle;
use zaparoo_app::user_flags::Flag;
use zaparoo_app::user_lists::{self as rules, ListSort};
use zaparoo_core::collections::UserList;
use zaparoo_core::persist::{PersistedState, UserListState};

use crate::router::Shared;
use crate::App;

/// The list the list screens show.
pub(crate) fn active(shared: &Shared) -> Flag {
    Flag::from_tag_value(shared.persist.lists.active.trim()).unwrap_or(Flag::Favorite)
}

/// Put `flag` on the list screens and name it to the views.
pub(crate) fn set_active(shared: &mut Shared, app: &App, flag: Flag) {
    flag.tag_value()
        .clone_into(&mut shared.persist.lists.active);
    publish(app, flag);
}

/// Name the list to the views, which title and word the screens by it.
pub(crate) fn publish(app: &App, flag: Flag) {
    app.global::<crate::Shell>()
        .set_active_list(crate::router::user_flag(flag));
}

/// The Core client's name for a list.
pub(crate) const fn user_list(flag: Flag) -> UserList {
    match flag {
        Flag::Favorite => UserList::Favorite,
        Flag::Like => UserList::Liked,
        Flag::Dislike => UserList::Disliked,
        Flag::PlayLater => UserList::PlayLater,
    }
}

fn state(persist: &PersistedState, flag: Flag) -> Option<&UserListState> {
    match flag {
        Flag::Favorite => None,
        Flag::Like => Some(&persist.lists.liked),
        Flag::Dislike => Some(&persist.lists.disliked),
        Flag::PlayLater => Some(&persist.lists.play_later),
    }
}

fn state_mut(persist: &mut PersistedState, flag: Flag) -> Option<&mut UserListState> {
    match flag {
        Flag::Favorite => None,
        Flag::Like => Some(&mut persist.lists.liked),
        Flag::Dislike => Some(&mut persist.lists.disliked),
        Flag::PlayLater => Some(&mut persist.lists.play_later),
    }
}

/// A list's order.
pub(crate) fn sort(shared: &Shared, flag: Flag) -> ListSort {
    state(&shared.persist, flag).map_or_else(
        || rules::sort_for(flag, &shared.favorites_sort),
        |list| rules::sort_for(flag, &list.sort),
    )
}

/// The active list's order as Core's sort argument.
pub(crate) fn core_sort(shared: &Shared) -> Option<String> {
    sort(shared, active(shared)).core_sort().map(str::to_string)
}

/// Keep a list's new order. Favorites' lives in the config, which the
/// caller writes; true when it changed.
pub(crate) fn set_sort(shared: &mut Shared, flag: Flag, sort: ListSort) -> bool {
    if self::sort(shared, flag) == sort {
        return false;
    }
    match state_mut(&mut shared.persist, flag) {
        Some(list) => sort.token().clone_into(&mut list.sort),
        // The config keeps Core's default as no value, as it always has.
        None => {
            shared.favorites_sort = match sort {
                ListSort::Default => String::new(),
                ListSort::Name | ListSort::Added => sort.token().to_string(),
            };
        }
    }
    true
}

/// Whether a list opens on the systems that hold it.
pub(crate) fn grouped(shared: &Shared, flag: Flag) -> bool {
    state(&shared.persist, flag).map_or_else(
        || rules::grouped(&shared.persist.settings.favorites_grouping),
        |list| rules::grouped(&list.grouping),
    )
}

/// Keep a list's new grouping; true when it changed.
pub(crate) fn set_grouped(shared: &mut Shared, flag: Flag, on: bool) -> bool {
    if grouped(shared, flag) == on {
        return false;
    }
    let value = if on { rules::GROUP_SYSTEM } else { "none" };
    match state_mut(&mut shared.persist, flag) {
        Some(list) => value.clone_into(&mut list.grouping),
        None => value.clone_into(&mut shared.persist.settings.favorites_grouping),
    }
    true
}

/// The game focused on a list, and the first row its list layout shows.
pub(crate) fn games_focus(persist: &PersistedState, flag: Flag) -> (String, Option<usize>) {
    state(persist, flag).map_or_else(
        || {
            (
                persist.favorites.selected_path.clone(),
                persist.favorites.list_top,
            )
        },
        |list| (list.selected_path.clone(), list.list_top),
    )
}

pub(crate) fn set_games_path(persist: &mut PersistedState, flag: Flag, path: String) {
    match state_mut(persist, flag) {
        Some(list) => list.selected_path = path,
        None => persist.favorites.selected_path = path,
    }
}

pub(crate) fn set_games_top(persist: &mut PersistedState, flag: Flag, top: Option<usize>) {
    match state_mut(persist, flag) {
        Some(list) => list.list_top = top,
        None => persist.favorites.list_top = top,
    }
}

/// Forget where a list's games were left: a fresh entry starts at the top.
pub(crate) fn reset_games(persist: &mut PersistedState, flag: Flag) {
    set_games_path(persist, flag, String::new());
    set_games_top(persist, flag, None);
}

/// The system focused on a list's systems screen, and its list's top row.
pub(crate) fn systems_focus(persist: &PersistedState, flag: Flag) -> (String, Option<usize>) {
    state(persist, flag).map_or_else(
        || {
            (
                persist.favorite_systems.selected_path.clone(),
                persist.favorite_systems.list_top,
            )
        },
        |list| (list.system_id.clone(), list.systems_list_top),
    )
}

pub(crate) fn set_systems_path(persist: &mut PersistedState, flag: Flag, system_id: String) {
    match state_mut(persist, flag) {
        Some(list) => list.system_id = system_id,
        None => persist.favorite_systems.selected_path = system_id,
    }
}

pub(crate) fn set_systems_top(persist: &mut PersistedState, flag: Flag, top: Option<usize>) {
    match state_mut(persist, flag) {
        Some(list) => list.systems_list_top = top,
        None => persist.favorite_systems.list_top = top,
    }
}

pub(crate) fn reset_systems(persist: &mut PersistedState, flag: Flag) {
    set_systems_path(persist, flag, String::new());
    set_systems_top(persist, flag, None);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn favorites_keep_their_old_sections_and_the_others_their_own() {
        let mut persist = PersistedState::default();
        set_games_path(&mut persist, Flag::Favorite, "/fav.nes".into());
        set_games_path(&mut persist, Flag::Like, "/liked.nes".into());
        set_systems_path(&mut persist, Flag::PlayLater, "SNES".into());
        set_systems_top(&mut persist, Flag::Favorite, Some(2));
        assert_eq!(persist.favorites.selected_path, "/fav.nes");
        assert_eq!(persist.lists.liked.selected_path, "/liked.nes");
        assert_eq!(persist.lists.play_later.system_id, "SNES");
        assert_eq!(persist.favorite_systems.list_top, Some(2));
        assert_eq!(
            games_focus(&persist, Flag::Like),
            ("/liked.nes".to_string(), None)
        );
        reset_games(&mut persist, Flag::Like);
        assert_eq!(games_focus(&persist, Flag::Like), (String::new(), None));
        assert_eq!(persist.lists.disliked, UserListState::default());
    }
}
