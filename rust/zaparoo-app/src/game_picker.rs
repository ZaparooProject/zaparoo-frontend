// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

//! "Choose a game": the sources it lists, where the cursor moves among
//! them, which pane a press lands on, and when the screen shows its two
//! panes side by side or one at a time.

/// How many games one source lists: its first page from Core.
pub const FILL_LIMIT: u32 = 100;

/// The share of the card the sources take beside the games, as
/// `(sources, games)`.
pub const SOURCES_SHARE: (i32, i32) = (1, 3);

/// One row of the source list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    Recents,
    Favorites,
    Liked,
    PlayLater,
    /// The heading over the user's decks. It takes no focus.
    DecksHeading,
    /// One deck, by Core's id, with its name.
    Deck {
        id: String,
        name: String,
    },
    Search,
}

impl Source {
    /// The cursor can rest on it.
    pub fn selectable(&self) -> bool {
        !matches!(self, Self::DecksHeading)
    }
}

/// The source list, in order: the user's lists, a Decks heading with each
/// deck under it (no heading without decks), then Search.
pub fn sources(decks: impl IntoIterator<Item = (String, String)>) -> Vec<Source> {
    let mut list = vec![
        Source::Recents,
        Source::Favorites,
        Source::Liked,
        Source::PlayLater,
    ];
    let decks: Vec<Source> = decks
        .into_iter()
        .map(|(id, name)| Source::Deck { id, name })
        .collect();
    if !decks.is_empty() {
        list.push(Source::DecksHeading);
        list.extend(decks);
    }
    list.push(Source::Search);
    list
}

/// Where a step of `delta` rows from `current` lands: it stops at either
/// end and passes over headings.
pub fn step(sources: &[Source], current: usize, delta: i64) -> usize {
    if sources.is_empty() {
        return 0;
    }
    let last = sources.len() - 1;
    let wanted = if delta < 0 {
        current.saturating_sub(usize::try_from(delta.unsigned_abs()).unwrap_or(usize::MAX))
    } else {
        current
            .saturating_add(usize::try_from(delta).unwrap_or(usize::MAX))
            .min(last)
    };
    if sources[wanted].selectable() {
        return wanted;
    }
    // A heading: on past it the way the step went, or back if that is the end.
    let forward = (wanted..=last).find(|&i| sources[i].selectable());
    let backward = (0..=wanted).rev().find(|&i| sources[i].selectable());
    if delta < 0 {
        backward.or(forward).unwrap_or(current)
    } else {
        forward.or(backward).unwrap_or(current)
    }
}

/// The row to seat on after the list changed: the same source if it is
/// still there, else the first.
pub fn seat(sources: &[Source], previous: &Source) -> usize {
    sources
        .iter()
        .position(|source| source == previous && source.selectable())
        .unwrap_or(0)
}

/// What the screen is showing, and where the cursor is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Pane {
    #[default]
    Sources,
    Games,
    /// Search's on-screen keyboard.
    Keyboard,
}

/// Where Accept (or Right, beside the sources) on a source goes: Search to
/// its keyboard until a search has results to show, every other source to
/// its games.
pub fn enter(source: &Source, searched: bool) -> Pane {
    match source {
        Source::Search if !searched => Pane::Keyboard,
        _ => Pane::Games,
    }
}

/// What Back does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Back {
    /// Move to this pane.
    To(Pane),
    /// Close the picker without a game.
    Leave,
}

/// Back from `pane`: a search's results go back to its keyboard, the
/// keyboard and the games to the sources, and the sources close the
/// picker.
pub fn back(pane: Pane, source: &Source) -> Back {
    match pane {
        Pane::Sources => Back::Leave,
        Pane::Games if *source == Source::Search => Back::To(Pane::Keyboard),
        Pane::Keyboard | Pane::Games => Back::To(Pane::Sources),
    }
}

/// The scene the picker lays itself out for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "independent facts about the output and the user's list setting"
)]
pub struct Scene {
    /// The 240p sizing tier.
    pub tier_240: bool,
    /// The analog CRT output is in use.
    pub crt: bool,
    /// The scene is turned for a TATE screen.
    pub rotated: bool,
    /// Browse lists are set to text only: no detail pane anywhere.
    pub text_only: bool,
}

/// Two panes side by side wherever a browse list shows its detail pane;
/// one pane at a time at 240p, on a CRT, turned for TATE, and where lists
/// are text only.
pub fn two_panes(scene: Scene) -> bool {
    !(scene.tier_240 || scene.crt || scene.rotated || scene.text_only)
}

/// The sources' and the games' cards across `width`, `gap` apart: the
/// sources' width, then the games' left edge and width.
pub fn split(width: i32, gap: i32) -> (i32, i32, i32) {
    let (sources, games) = SOURCES_SHARE;
    let usable = (width - gap).max(0);
    let left = usable * sources / (sources + games).max(1);
    let right = usable - left;
    (left, left + gap, right)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decks() -> Vec<(String, String)> {
        vec![
            ("d1".to_string(), "Party".to_string()),
            ("d2".to_string(), "Retro".to_string()),
        ]
    }

    #[test]
    fn sources_list_the_lists_then_each_deck_under_a_heading_then_search() {
        let list = sources(decks());
        assert_eq!(
            list,
            vec![
                Source::Recents,
                Source::Favorites,
                Source::Liked,
                Source::PlayLater,
                Source::DecksHeading,
                Source::Deck {
                    id: "d1".to_string(),
                    name: "Party".to_string()
                },
                Source::Deck {
                    id: "d2".to_string(),
                    name: "Retro".to_string()
                },
                Source::Search,
            ]
        );
    }

    #[test]
    fn no_decks_means_no_heading() {
        let list = sources(Vec::new());
        assert!(!list.contains(&Source::DecksHeading));
        assert_eq!(list.last(), Some(&Source::Search));
        assert_eq!(list.len(), 5);
    }

    #[test]
    fn a_step_passes_over_the_heading_both_ways() {
        let list = sources(decks());
        // Play later (3) down: over the heading (4) to the first deck.
        assert_eq!(step(&list, 3, 1), 5);
        // The first deck up: over the heading to Play later.
        assert_eq!(step(&list, 5, -1), 3);
    }

    #[test]
    fn a_step_stops_at_either_end() {
        let list = sources(decks());
        assert_eq!(step(&list, 0, -1), 0);
        assert_eq!(step(&list, 7, 1), 7);
        assert_eq!(step(&list, 0, 100), 7);
        assert_eq!(step(&list, 7, -100), 0);
        assert_eq!(step(&[], 0, 1), 0);
    }

    #[test]
    fn a_page_step_landing_on_the_heading_moves_on_past_it() {
        let list = sources(decks());
        assert_eq!(step(&list, 0, 4), 5);
        assert_eq!(step(&list, 7, -3), 3);
    }

    #[test]
    fn the_cursor_keeps_its_source_when_the_list_changes() {
        let list = sources(decks());
        let retro = Source::Deck {
            id: "d2".to_string(),
            name: "Retro".to_string(),
        };
        assert_eq!(seat(&list, &retro), 6);
        assert_eq!(seat(&list, &Source::Search), 7);
        let gone = Source::Deck {
            id: "d9".to_string(),
            name: "Gone".to_string(),
        };
        assert_eq!(seat(&list, &gone), 0);
        assert_eq!(seat(&list, &Source::DecksHeading), 0);
    }

    #[test]
    fn search_opens_its_keyboard_until_it_has_results() {
        assert_eq!(enter(&Source::Search, false), Pane::Keyboard);
        assert_eq!(enter(&Source::Search, true), Pane::Games);
        assert_eq!(enter(&Source::Recents, false), Pane::Games);
    }

    #[test]
    fn back_walks_out_one_pane_at_a_time() {
        assert_eq!(
            back(Pane::Games, &Source::Favorites),
            Back::To(Pane::Sources)
        );
        assert_eq!(back(Pane::Games, &Source::Search), Back::To(Pane::Keyboard));
        assert_eq!(
            back(Pane::Keyboard, &Source::Search),
            Back::To(Pane::Sources)
        );
        assert_eq!(back(Pane::Sources, &Source::Recents), Back::Leave);
    }

    #[test]
    fn two_panes_only_where_the_detail_pane_shows() {
        assert!(two_panes(Scene::default()));
        for scene in [
            Scene {
                tier_240: true,
                ..Scene::default()
            },
            Scene {
                crt: true,
                ..Scene::default()
            },
            Scene {
                rotated: true,
                ..Scene::default()
            },
            Scene {
                text_only: true,
                ..Scene::default()
            },
        ] {
            assert!(!two_panes(scene), "{scene:?}");
        }
    }

    #[test]
    fn the_split_gives_the_sources_a_quarter_and_keeps_the_gap() {
        let (left, games_x, right) = split(1000, 20);
        assert_eq!(left, 245);
        assert_eq!(games_x, 265);
        assert_eq!(right, 735);
        assert_eq!(left + 20 + right, 1000);
        assert_eq!(split(0, 20), (0, 20, 0));
    }
}
