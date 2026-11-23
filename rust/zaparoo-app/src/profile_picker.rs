// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// "Who is playing?": the profile picker a start can open on. Whether it
// shows at all, which rows it lists, and where the cursor starts.

/// The kiosk lock as the picker needs it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Kiosk {
    /// The device is locked now (`state` is `locked`). Off and a
    /// temporary unlock both read as not locked.
    pub locked: bool,
    /// The active profile may change between non-admin profiles while
    /// locked (`settings.profileSwitching`).
    pub profile_switching: bool,
}

/// One of Core's profiles, as the picker filters them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Entry<'a> {
    pub id: &'a str,
    pub admin: bool,
}

/// A row of the picker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Row {
    /// Core's profile at this index of the list it gave.
    Profile(usize),
    /// Leave every profile for the shared one.
    Shared,
}

/// Whether this start may open on the picker, before Core is asked who
/// there is: the setting is on and the start is a fresh one (a power-on on
/// `MiSTer`, every process start on desktop, a host's say-so when hosted).
pub fn may_show(setting_on: bool, fresh_start: bool) -> bool {
    setting_on && fresh_start
}

/// The picker's rows: Core's profiles in Core's order, then the shared
/// profile last. While locked, admins go (an admin is the way in, not a
/// player) and so does the shared profile. The shared profile also goes
/// when Core will not launch without a profile.
pub fn rows(profiles: &[Entry<'_>], require_for_launch: bool, kiosk: Kiosk) -> Vec<Row> {
    let mut rows: Vec<Row> = profiles
        .iter()
        .enumerate()
        .filter(|(_, p)| !(kiosk.locked && p.admin))
        .map(|(i, _)| Row::Profile(i))
        .collect();
    if !require_for_launch && !kiosk.locked {
        rows.push(Row::Shared);
    }
    rows
}

/// Whether the picker shows, once Core has said who there is. It never
/// shows with no profile to pick, and a lock that keeps the active profile
/// skips it for this start. One profile is still a choice: it or the
/// shared one, or a confirmation of who is about to play.
pub fn shows(rows: &[Row], kiosk: Kiosk) -> bool {
    if kiosk.locked && !kiosk.profile_switching {
        return false;
    }
    rows.iter().any(|r| matches!(r, Row::Profile(_)))
}

/// Where the cursor starts: on the active profile (`None` is the shared
/// profile), or the first row when that is not listed.
pub fn initial(rows: &[Row], profiles: &[Entry<'_>], active: Option<&str>) -> usize {
    rows.iter()
        .position(|row| match (row, active) {
            (Row::Profile(i), Some(id)) => profiles.get(*i).is_some_and(|p| p.id == id),
            (Row::Shared, None) => true,
            _ => false,
        })
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    const UNLOCKED: Kiosk = Kiosk {
        locked: false,
        profile_switching: false,
    };

    fn entries() -> Vec<Entry<'static>> {
        vec![
            Entry {
                id: "mum",
                admin: true,
            },
            Entry {
                id: "kid",
                admin: false,
            },
            Entry {
                id: "guest",
                admin: false,
            },
        ]
    }

    #[test]
    fn needs_the_setting_and_a_fresh_start() {
        assert!(may_show(true, true));
        assert!(!may_show(false, true));
        assert!(!may_show(true, false));
        assert!(!may_show(false, false));
    }

    #[test]
    fn lists_profiles_in_order_then_shared() {
        assert_eq!(
            rows(&entries(), false, UNLOCKED),
            vec![
                Row::Profile(0),
                Row::Profile(1),
                Row::Profile(2),
                Row::Shared
            ]
        );
    }

    #[test]
    fn require_for_launch_drops_shared() {
        assert_eq!(
            rows(&entries(), true, UNLOCKED),
            vec![Row::Profile(0), Row::Profile(1), Row::Profile(2)]
        );
    }

    #[test]
    fn locked_drops_admins_and_shared() {
        let kiosk = Kiosk {
            locked: true,
            profile_switching: true,
        };
        assert_eq!(
            rows(&entries(), false, kiosk),
            vec![Row::Profile(1), Row::Profile(2)]
        );
    }

    #[test]
    fn shows_with_one_profile() {
        let one = [Entry {
            id: "kid",
            admin: false,
        }];
        let rows = rows(&one, false, UNLOCKED);
        assert!(shows(&rows, UNLOCKED));
    }

    #[test]
    fn never_shows_without_a_profile() {
        let rows = rows(&[], false, UNLOCKED);
        assert_eq!(rows, vec![Row::Shared]);
        assert!(!shows(&rows, UNLOCKED));
    }

    #[test]
    fn locked_with_only_admins_skips() {
        let kiosk = Kiosk {
            locked: true,
            profile_switching: true,
        };
        let admins = [Entry {
            id: "mum",
            admin: true,
        }];
        let rows = rows(&admins, false, kiosk);
        assert!(!shows(&rows, kiosk));
    }

    #[test]
    fn locked_without_switching_skips() {
        let kiosk = Kiosk {
            locked: true,
            profile_switching: false,
        };
        let rows = rows(&entries(), false, kiosk);
        assert!(!shows(&rows, kiosk));
    }

    #[test]
    fn unlocked_ignores_switching() {
        let rows = rows(&entries(), false, UNLOCKED);
        assert!(shows(&rows, UNLOCKED));
    }

    #[test]
    fn starts_on_the_active_profile() {
        let profiles = entries();
        let list = rows(&profiles, false, UNLOCKED);
        assert_eq!(initial(&list, &profiles, Some("kid")), 1);
        assert_eq!(initial(&list, &profiles, None), 3);
        assert_eq!(initial(&list, &profiles, Some("gone")), 0);
    }

    #[test]
    fn active_profile_hidden_by_the_lock_starts_on_the_first_row() {
        let kiosk = Kiosk {
            locked: true,
            profile_switching: true,
        };
        let profiles = entries();
        let list = rows(&profiles, false, kiosk);
        assert_eq!(initial(&list, &profiles, Some("mum")), 0);
        assert_eq!(initial(&list, &profiles, None), 0);
    }
}
