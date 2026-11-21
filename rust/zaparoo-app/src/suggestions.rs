// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

//! Games Core suggests: "Similar games" (`media.similar`) and the
//! Recommended screen (`media.recommended`). Core sends why as codes, not
//! sentences; these are the codes the interface words. An unknown code
//! reads as no reason, since Core may add more.

/// What one similar game shares with the game it was suggested for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shared {
    Series,
    Genre,
    Perspective,
    Art,
    Based,
    Developer,
    Publisher,
    Players,
    Year,
    Decade,
}

impl Shared {
    pub fn from_code(code: &str) -> Option<Self> {
        Some(match code {
            "series" => Self::Series,
            "genre" => Self::Genre,
            "perspective" => Self::Perspective,
            "art" => Self::Art,
            "based" => Self::Based,
            "developer" => Self::Developer,
            "publisher" => Self::Publisher,
            "players" => Self::Players,
            "year" => Self::Year,
            "decade" => Self::Decade,
            _ => return None,
        })
    }

    /// The `Labels.menu` key that words it.
    pub fn key(self) -> &'static str {
        match self {
            Self::Series => "similar:series",
            Self::Genre => "similar:genre",
            Self::Perspective => "similar:perspective",
            Self::Art => "similar:art",
            Self::Based => "similar:based",
            Self::Developer => "similar:developer",
            Self::Publisher => "similar:publisher",
            Self::Players => "similar:players",
            Self::Year => "similar:year",
            Self::Decade => "similar:decade",
        }
    }
}

/// The reason a similar game's row shows: the strongest code the
/// interface knows. Core sends reasons strongest first.
pub fn strongest<'a>(codes: impl IntoIterator<Item = &'a str>) -> Option<Shared> {
    codes.into_iter().find_map(Shared::from_code)
}

/// Why a game is on the Recommended screen: the interest the profile
/// showed in its seed game, or the Play later list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Signal {
    Played,
    Favorite,
    Liked,
    Deck,
    PlayLater,
}

impl Signal {
    pub fn from_token(token: &str) -> Option<Self> {
        Some(match token {
            "played" => Self::Played,
            "favorite" => Self::Favorite,
            "liked" => Self::Liked,
            "deck" => Self::Deck,
            "playLater" => Self::PlayLater,
            _ => return None,
        })
    }
}

/// The "why" a recommended game's info line shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pick {
    pub signal: Signal,
    /// The game it is most like; empty for Play later.
    pub seed: String,
}

/// A recommended game's reason, when the interface can word it: a seed
/// signal needs the seed's name, Play later does not.
pub fn pick(signal: Option<&str>, seed: Option<&str>) -> Option<Pick> {
    let signal = Signal::from_token(signal?)?;
    let seed = seed.map(str::trim).unwrap_or_default();
    if signal != Signal::PlayLater && seed.is_empty() {
        return None;
    }
    Some(Pick {
        signal,
        seed: if signal == Signal::PlayLater {
            String::new()
        } else {
            seed.to_string()
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_strongest_known_code_names_the_row() {
        assert_eq!(strongest(["series", "genre"]), Some(Shared::Series));
        assert_eq!(strongest(["mood", "decade"]), Some(Shared::Decade));
        assert_eq!(strongest(["mood"]), None);
        assert_eq!(strongest([]), None);
        assert_eq!(Shared::Art.key(), "similar:art");
    }

    #[test]
    fn a_seed_signal_needs_its_seed() {
        assert_eq!(
            pick(Some("played"), Some("Contra")),
            Some(Pick {
                signal: Signal::Played,
                seed: "Contra".into()
            })
        );
        assert_eq!(pick(Some("liked"), None), None);
        assert_eq!(pick(Some("favorite"), Some("  ")), None);
        assert_eq!(
            pick(Some("playLater"), Some("Contra")),
            Some(Pick {
                signal: Signal::PlayLater,
                seed: String::new()
            })
        );
        assert_eq!(pick(Some("trending"), Some("Contra")), None);
        assert_eq!(pick(None, Some("Contra")), None);
    }
}
