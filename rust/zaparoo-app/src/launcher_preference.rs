// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

//! Core's global launcher preference: an ordered list of launcher ids and
//! launcher groups, tried in order for every system. Core takes the first
//! entry that names an available launcher for the game's system, after a
//! per-game override and the system's own default. Core matches entries
//! case-insensitively, so every rule here does too.

/// The group Core gives every launcher that runs a `RetroAchievements` core.
pub const RETRO_ACHIEVEMENTS: &str = "RetroAchievements";

/// One launcher, as far as the preference cares. Mirrors Core's
/// `launchers` result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LauncherRef<'a> {
    pub id: &'a str,
    pub system_id: &'a str,
    pub system_name: &'a str,
    pub groups: &'a [String],
    pub available: bool,
}

fn same(a: &str, b: &str) -> bool {
    a.eq_ignore_ascii_case(b)
}

/// Where `entry` sits in the list, in any case.
pub fn position(list: &[String], entry: &str) -> Option<usize> {
    list.iter().position(|e| same(e, entry))
}

/// The list with `entry` appended; unchanged when it is already there.
pub fn add(list: &[String], entry: &str) -> Vec<String> {
    let mut out = list.to_vec();
    if position(list, entry).is_none() {
        out.push(entry.to_string());
    }
    out
}

/// The list without the entry at `index`.
pub fn remove(list: &[String], index: usize) -> Vec<String> {
    let mut out = list.to_vec();
    if index < out.len() {
        out.remove(index);
    }
    out
}

/// The list as it was when a move began, with the held entry taken from
/// `origin` and placed at `to`. Always rebuilt from `base`, so a move that
/// comes back to where it started reproduces it exactly.
pub fn reseat(base: &[String], origin: usize, to: usize) -> Vec<String> {
    let mut out = base.to_vec();
    if origin >= out.len() {
        return out;
    }
    let held = out.remove(origin);
    out.insert(to.min(out.len()), held);
    out
}

/// One step of the held entry, or `None` at either end: a move never wraps.
pub fn step(index: usize, len: usize, down: bool) -> Option<usize> {
    if down {
        (index + 1 < len).then_some(index + 1)
    } else {
        index.checked_sub(1)
    }
}

/// Whether `group` is anywhere in the list.
pub fn contains(list: &[String], entry: &str) -> bool {
    position(list, entry).is_some()
}

/// Turn one entry on or off. On puts it first, unless it is already in the
/// list wherever the user put it; off removes every copy and keeps the rest
/// in order.
pub fn set(list: &[String], entry: &str, on: bool) -> Vec<String> {
    if on {
        if contains(list, entry) {
            return list.to_vec();
        }
        let mut out = Vec::with_capacity(list.len() + 1);
        out.push(entry.to_string());
        out.extend(list.iter().cloned());
        out
    } else {
        list.iter().filter(|e| !same(e, entry)).cloned().collect()
    }
}

/// What a list entry names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntryKind {
    Group,
    /// A launcher, with the name of the system it runs.
    Launcher(String),
    /// Nothing Core lists here; Core still keeps it.
    Unknown,
}

pub fn entry_kind(entry: &str, launchers: &[LauncherRef<'_>]) -> EntryKind {
    if let Some(launcher) = launchers.iter().find(|l| same(l.id, entry)) {
        return EntryKind::Launcher(launcher.system_name.to_string());
    }
    if launchers
        .iter()
        .any(|l| l.groups.iter().any(|g| same(g, entry)))
    {
        return EntryKind::Group;
    }
    EntryKind::Unknown
}

/// One choice on the Add page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Choice {
    pub id: String,
    /// The system name for a launcher; empty for a group.
    pub system_name: String,
    pub group: bool,
    /// A group is available when any of its launchers is.
    pub available: bool,
}

/// What can be added: every group first, then every launcher by system
/// name, leaving out what the list already has. Unavailable ones stay,
/// since Core keeps an entry that is not installed yet.
pub fn choices(launchers: &[LauncherRef<'_>], current: &[String]) -> Vec<Choice> {
    let mut groups: Vec<Choice> = Vec::new();
    for launcher in launchers {
        for group in launcher.groups {
            match groups.iter_mut().find(|c| same(&c.id, group)) {
                Some(choice) => choice.available |= launcher.available,
                None => groups.push(Choice {
                    id: group.clone(),
                    system_name: String::new(),
                    group: true,
                    available: launcher.available,
                }),
            }
        }
    }
    groups.sort_by_key(|c| c.id.to_lowercase());
    let mut singles: Vec<Choice> = launchers
        .iter()
        .map(|l| Choice {
            id: l.id.to_string(),
            system_name: l.system_name.to_string(),
            group: false,
            available: l.available,
        })
        .collect();
    singles.sort_by(|a, b| {
        a.system_name
            .to_lowercase()
            .cmp(&b.system_name.to_lowercase())
            .then_with(|| a.id.to_lowercase().cmp(&b.id.to_lowercase()))
    });
    singles.dedup_by(|a, b| same(&a.id, &b.id));
    groups
        .into_iter()
        .chain(singles)
        .filter(|c| !contains(current, &c.id))
        .collect()
}

fn in_group(launcher: &LauncherRef<'_>, group: &str) -> bool {
    launcher.groups.iter().any(|g| same(g, group))
}

/// Any `RetroAchievements` launcher is installed.
pub fn ra_installed(launchers: &[LauncherRef<'_>]) -> bool {
    launchers
        .iter()
        .any(|l| l.available && in_group(l, RETRO_ACHIEVEMENTS))
}

/// The systems a `RetroAchievements` preference would not reach: an
/// installed `RetroAchievements` launcher runs them, but their own default
/// launcher (which Core takes before the preference) is something else.
/// `defaults` is each system id and its default launcher or group.
pub fn ra_conflicts(launchers: &[LauncherRef<'_>], defaults: &[(&str, &str)]) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    for (system, default) in defaults {
        if default.is_empty() || same(default, RETRO_ACHIEVEMENTS) {
            continue;
        }
        let ra_here = launchers
            .iter()
            .find(|l| same(l.system_id, system) && l.available && in_group(l, RETRO_ACHIEVEMENTS));
        let Some(ra) = ra_here else {
            continue;
        };
        let default_is_ra = launchers
            .iter()
            .any(|l| same(l.id, default) && in_group(l, RETRO_ACHIEVEMENTS));
        if default_is_ra {
            continue;
        }
        let name = if ra.system_name.is_empty() {
            ra.system_id
        } else {
            ra.system_name
        };
        if !names.iter().any(|n| n == name) {
            names.push(name.to_string());
        }
    }
    names.sort_by_key(|n| n.to_lowercase());
    names
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list(entries: &[&str]) -> Vec<String> {
        entries.iter().map(|e| (*e).to_string()).collect()
    }

    fn groups(names: &[&str]) -> Vec<String> {
        list(names)
    }

    #[test]
    fn add_and_position_ignore_case() {
        let l = list(&["RetroAchievements"]);
        assert_eq!(position(&l, "retroachievements"), Some(0));
        assert_eq!(add(&l, "RETROACHIEVEMENTS"), l);
        assert_eq!(add(&l, "LLAPI"), list(&["RetroAchievements", "LLAPI"]));
        assert_eq!(remove(&l, 0), Vec::<String>::new());
        assert_eq!(remove(&l, 5), l);
    }

    #[test]
    fn set_puts_a_new_entry_first_and_keeps_a_placed_one() {
        let l = list(&["LLAPI", "DB9"]);
        assert_eq!(
            set(&l, RETRO_ACHIEVEMENTS, true),
            list(&["RetroAchievements", "LLAPI", "DB9"])
        );
        let placed = list(&["LLAPI", "retroachievements", "DB9"]);
        assert_eq!(set(&placed, RETRO_ACHIEVEMENTS, true), placed);
        assert_eq!(
            set(&placed, RETRO_ACHIEVEMENTS, false),
            list(&["LLAPI", "DB9"])
        );
        assert!(contains(&placed, RETRO_ACHIEVEMENTS));
        assert!(!contains(&l, RETRO_ACHIEVEMENTS));
    }

    #[test]
    fn reseat_rebuilds_from_the_snapshot() {
        let base = list(&["a", "b", "c", "d"]);
        assert_eq!(reseat(&base, 1, 2), list(&["a", "c", "b", "d"]));
        assert_eq!(reseat(&base, 1, 3), list(&["a", "c", "d", "b"]));
        assert_eq!(reseat(&base, 2, 0), list(&["c", "a", "b", "d"]));
        assert_eq!(reseat(&base, 1, 1), base);
        assert_eq!(reseat(&base, 9, 0), base);
    }

    #[test]
    fn a_move_stops_at_either_end() {
        assert_eq!(step(0, 3, false), None);
        assert_eq!(step(0, 3, true), Some(1));
        assert_eq!(step(2, 3, true), None);
        assert_eq!(step(2, 3, false), Some(1));
        assert_eq!(step(0, 0, true), None);
    }

    #[test]
    fn choices_list_groups_first_then_launchers_by_system() {
        let ra = groups(&["RetroAchievements"]);
        let db9 = groups(&["DB9"]);
        let none: Vec<String> = Vec::new();
        let launchers = [
            LauncherRef {
                id: "SNES",
                system_id: "SNES",
                system_name: "Super Nintendo",
                groups: &none,
                available: true,
            },
            LauncherRef {
                id: "RASNES",
                system_id: "SNES",
                system_name: "Super Nintendo",
                groups: &ra,
                available: false,
            },
            LauncherRef {
                id: "NESdb9",
                system_id: "NES",
                system_name: "NES",
                groups: &db9,
                available: true,
            },
            LauncherRef {
                id: "RANES",
                system_id: "NES",
                system_name: "NES",
                groups: &ra,
                available: true,
            },
        ];
        let all = choices(&launchers, &[]);
        let ids: Vec<&str> = all.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(
            ids,
            vec![
                "DB9",
                "RetroAchievements",
                "NESdb9",
                "RANES",
                "RASNES",
                "SNES"
            ]
        );
        assert!(
            all[1].group && all[1].available,
            "one installed member is enough"
        );
        assert!(!all[4].available);
        assert_eq!(all[2].system_name, "NES");

        let left = choices(&launchers, &list(&["retroachievements", "SNES"]));
        assert!(!left
            .iter()
            .any(|c| c.id == "RetroAchievements" || c.id == "SNES"));

        assert_eq!(
            entry_kind("retroachievements", &launchers),
            EntryKind::Group
        );
        assert_eq!(
            entry_kind("rasnes", &launchers),
            EntryKind::Launcher("Super Nintendo".to_string())
        );
        assert_eq!(entry_kind("Kodi", &launchers), EntryKind::Unknown);
        assert!(ra_installed(&launchers));
        assert!(!ra_installed(&launchers[..3]));
    }

    #[test]
    fn conflicts_are_systems_whose_own_default_beats_an_installed_ra_launcher() {
        let ra = groups(&["RetroAchievements"]);
        let none: Vec<String> = Vec::new();
        let launchers = [
            LauncherRef {
                id: "RASNES",
                system_id: "SNES",
                system_name: "Super Nintendo",
                groups: &ra,
                available: true,
            },
            LauncherRef {
                id: "RANES",
                system_id: "NES",
                system_name: "NES",
                groups: &ra,
                available: true,
            },
            LauncherRef {
                id: "RAPSX",
                system_id: "PSX",
                system_name: "PlayStation",
                groups: &ra,
                available: false,
            },
            LauncherRef {
                id: "SNES",
                system_id: "SNES",
                system_name: "Super Nintendo",
                groups: &none,
                available: true,
            },
        ];
        let defaults = [
            ("SNES", "SNES"),
            ("NES", "RANES"),
            ("PSX", "PSX"),
            ("Genesis", "Genesis"),
            ("GBA", ""),
        ];
        assert_eq!(ra_conflicts(&launchers, &defaults), vec!["Super Nintendo"]);
        assert!(ra_conflicts(&launchers, &[("SNES", "retroachievements")]).is_empty());
        assert!(ra_conflicts(&launchers, &[]).is_empty());
    }
}
