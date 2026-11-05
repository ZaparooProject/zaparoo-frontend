// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

//! Top played: the games Core ranks by total play time
//! (`media.history.top`), with each game's time shown under the focused
//! tile, and optionally grouped by system first.

/// A total play time as the footer shows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayTime {
    /// Under a minute.
    Under,
    /// Minutes alone, under an hour.
    Minutes(u32),
    /// Hours and the minutes past them.
    Hours(u32, u32),
}

impl PlayTime {
    pub fn from_seconds(seconds: u64) -> Self {
        let minutes = seconds / 60;
        let hours = minutes / 60;
        let rest = u32::try_from(minutes % 60).unwrap_or(0);
        if hours > 0 {
            Self::Hours(u32::try_from(hours).unwrap_or(u32::MAX), rest)
        } else if minutes > 0 {
            Self::Minutes(rest)
        } else {
            Self::Under
        }
    }

    /// The hours and minutes the view composes the text from: hours 0 and
    /// minutes 0 read as under a minute.
    pub fn parts(self) -> (u32, u32) {
        match self {
            Self::Under => (0, 0),
            Self::Minutes(minutes) => (0, minutes),
            Self::Hours(hours, minutes) => (hours, minutes),
        }
    }
}

/// The systems of a ranked list, in the order their best game ranks, each
/// with how many of the list's games it holds.
pub fn systems_by_rank<'a>(systems: impl IntoIterator<Item = &'a str>) -> Vec<(String, u32)> {
    let mut out: Vec<(String, u32)> = Vec::new();
    for system in systems {
        let system = system.trim();
        if system.is_empty() {
            continue;
        }
        if let Some((_, count)) = out.iter_mut().find(|(id, _)| id == system) {
            *count += 1;
        } else {
            out.push((system.to_string(), 1));
        }
    }
    out
}

/// The kept grouping value that opens Top played on its systems.
pub fn grouped(kept: &str) -> bool {
    crate::user_lists::grouped(kept)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn play_time_reads_as_hours_and_minutes_minutes_or_under_a_minute() {
        assert_eq!(PlayTime::from_seconds(0), PlayTime::Under);
        assert_eq!(PlayTime::from_seconds(59), PlayTime::Under);
        assert_eq!(PlayTime::from_seconds(60), PlayTime::Minutes(1));
        assert_eq!(PlayTime::from_seconds(45 * 60 + 30), PlayTime::Minutes(45));
        assert_eq!(PlayTime::from_seconds(3600), PlayTime::Hours(1, 0));
        assert_eq!(
            PlayTime::from_seconds(12 * 3600 + 5 * 60 + 9),
            PlayTime::Hours(12, 5)
        );
        assert_eq!(PlayTime::from_seconds(12 * 3600 + 5 * 60).parts(), (12, 5));
        assert_eq!(PlayTime::Under.parts(), (0, 0));
    }

    #[test]
    fn systems_follow_their_best_ranked_game_and_count_their_games() {
        let systems = systems_by_rank(["SNES", "NES", "SNES", " ", "Genesis", "NES", "SNES"]);
        assert_eq!(
            systems,
            vec![
                ("SNES".to_string(), 3),
                ("NES".to_string(), 2),
                ("Genesis".to_string(), 1)
            ]
        );
        assert!(grouped("system") && !grouped(""));
    }
}
