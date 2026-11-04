// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

//! One title's versions. A grouped search lists a title once, as the
//! version Core would launch, with how many versions match; the row's menu
//! lists them all as a page. Core does the grouping and picks the default;
//! these rules only word the versions so the page tells them apart. The
//! Core calls stay in the shell.

/// Most versions one page lists.
pub const MAX_VERSIONS: u32 = 100;

/// Whether a search result stands for more than one version, so its menu
/// offers them. Core counts them and names the title the page asks for.
pub fn has_versions(title_id: Option<i64>, version_count: u32) -> bool {
    title_id.is_some() && version_count > 1
}

/// One version as Core listed it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Version<'a> {
    pub name: &'a str,
    pub path: &'a str,
    /// The disambiguating tags as the tile suffix words them (`US RA`).
    pub tags: &'a str,
}

/// The label of each version, in order. A version reads as its tags; two
/// with the same tags add the folder that tells them apart (see
/// `folder_distinguishers`). Names lead when the versions do
/// not all share one. A version that still has nothing of its own, or
/// still reads like another, falls back to its file's name, which is unique
/// within a folder.
pub fn labels(versions: &[Version<'_>]) -> Vec<String> {
    let names_differ = versions
        .first()
        .is_some_and(|first| versions.iter().any(|v| v.name.trim() != first.name.trim()));
    let base: Vec<String> = versions
        .iter()
        .map(|version| {
            let name = if names_differ {
                version.name.trim()
            } else {
                ""
            };
            join(name, version.tags.trim())
        })
        .collect();
    let mut labels = base.clone();
    for (index, key) in base.iter().enumerate() {
        let alike: Vec<usize> = (0..base.len()).filter(|&i| base[i] == *key).collect();
        if alike.len() < 2 || alike[0] != index {
            continue;
        }
        let parents: Vec<&str> = alike.iter().map(|&i| versions[i].path).collect();
        for (&i, folder) in alike.iter().zip(folder_distinguishers(&parents)) {
            labels[i] = join(key, &folder);
        }
    }
    // Decide on the labels as they stand, so falling back on one does not
    // leave its twin looking unique.
    let fallback: Vec<bool> = labels
        .iter()
        .map(|label| label.is_empty() || labels.iter().filter(|other| *other == label).count() > 1)
        .collect();
    labels
        .into_iter()
        .zip(versions)
        .zip(fallback)
        .map(|((label, version), fallback)| {
            if fallback {
                crate::media_list::file_stem_or_name(version.path, version.name, false)
            } else {
                label
            }
        })
        .collect()
}

fn join(a: &str, b: &str) -> String {
    match (a.is_empty(), b.is_empty()) {
        (true, _) => b.to_string(),
        (_, true) => a.to_string(),
        _ => format!("{a} · {b}"),
    }
}

/// For files with the same label: the folder each sits in when no other
/// shares it, which names the copy (`Hacks`, `Platformers`). Copies whose
/// folders share a name are told apart by the first folder where their
/// paths part (`fat` and `usb0` for two drives), again within each set
/// that still shares one. Empty for files in one folder.
fn folder_distinguishers(paths: &[&str]) -> Vec<String> {
    let folders: Vec<Vec<&str>> = paths
        .iter()
        .map(|path| {
            let mut parts: Vec<&str> = path.split(['/', '\\']).filter(|s| !s.is_empty()).collect();
            parts.pop();
            parts
        })
        .collect();
    let mut out = vec![String::new(); paths.len()];
    let mut shared = Vec::new();
    for (index, parts) in folders.iter().enumerate() {
        let own = parts.last().copied();
        let unique = own.is_some()
            && folders
                .iter()
                .enumerate()
                .all(|(other, parts)| other == index || parts.last().copied() != own);
        match own {
            Some(own) if unique => out[index] = own.to_string(),
            _ => shared.push(index),
        }
    }
    split(&shared, &folders, 0, &mut out);
    out
}

/// Label `members` by the first folder at or past `from` where their paths
/// part, splitting again inside any set that still shares that folder.
fn split(members: &[usize], folders: &[Vec<&str>], from: usize, out: &mut [String]) {
    if members.len() < 2 {
        return;
    }
    let longest = members.iter().map(|&m| folders[m].len()).max().unwrap_or(0);
    let Some(at) = (from..longest).find(|&i| {
        let first = folders[members[0]].get(i);
        members.iter().any(|&m| folders[m].get(i) != first)
    }) else {
        return;
    };
    let mut groups: Vec<(Option<&str>, Vec<usize>)> = Vec::new();
    for &member in members {
        let part = folders[member].get(at).copied();
        match groups.iter_mut().find(|(key, _)| *key == part) {
            Some((_, group)) => group.push(member),
            None => groups.push((part, vec![member])),
        }
    }
    for (part, group) in groups {
        if let [only] = group[..] {
            out[only] = part.map_or_else(String::new, str::to_string);
        } else {
            split(&group, folders, at + 1, out);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn version<'a>(path: &'a str, tags: &'a str) -> Version<'a> {
        Version {
            name: "Super Mario World",
            path,
            tags,
        }
    }

    #[test]
    fn only_a_title_core_counted_more_than_one_version_of_offers_them() {
        assert!(has_versions(Some(3), 2));
        assert!(!has_versions(Some(3), 1));
        assert!(!has_versions(Some(3), 0));
        assert!(!has_versions(None, 4), "older Core sends no title");
    }

    #[test]
    fn versions_read_as_their_tags() {
        let labels = labels(&[
            version("/media/fat/games/SNES/Super Mario World (USA).sfc", "US"),
            version("/media/fat/games/SNES/Super Mario World (Europe).sfc", "EU"),
        ]);
        assert_eq!(labels, ["US", "EU"]);
    }

    #[test]
    fn copies_with_the_same_tags_add_the_folder_where_they_part() {
        let labels = labels(&[
            version("/media/fat/games/SNES/Super Mario World (USA).sfc", "US"),
            version("/media/usb0/games/SNES/Super Mario World (USA).sfc", "US"),
            version("/media/fat/games/SNES/Super Mario World (Japan).sfc", "JP"),
        ]);
        assert_eq!(labels, ["US · fat", "US · usb0", "JP"]);
    }

    #[test]
    fn an_untagged_copy_in_a_subfolder_reads_as_that_folder() {
        let labels = labels(&[
            version("/media/fat/games/SNES/Super Mario World.sfc", ""),
            version("/media/fat/games/SNES/Hacks/Super Mario World.sfc", ""),
        ]);
        assert_eq!(labels, ["SNES", "Hacks"]);
    }

    #[test]
    fn copies_deep_in_collections_read_as_their_own_folders() {
        let root = "/media/fat/games/GBA";
        let set =
            "@GBA - EverDrive GBA Game Series Collections Add-On.zip/4 Game Series Collections";
        let file = "Classic NES Series - Super Mario Bros. (USA, Europe).gba";
        let paths = [
            format!("{root}/@GBA - EverDrive GBA.zip/1 USA - A-C/{file}"),
            format!("{root}/{set}/Franchise/Mario/{file}"),
            format!("{root}/{set}/Genre/Platformers/Sort By - 2D Platformers/{file}"),
            format!("{root}/{set}/Miscellaneous/NES Ports & Remakes (Official)/{file}"),
        ];
        let versions: Vec<Version<'_>> = paths.iter().map(|path| version(path, "")).collect();
        assert_eq!(
            labels(&versions),
            [
                "1 USA - A-C",
                "Mario",
                "Sort By - 2D Platformers",
                "NES Ports & Remakes (Official)"
            ]
        );
    }

    #[test]
    fn copies_in_same_named_folders_part_where_their_paths_do() {
        let labels = labels(&[
            version("/media/fat/games/SNES/Super Mario World.sfc", "US"),
            version("/media/usb0/games/SNES/Super Mario World.sfc", "US"),
            version("/media/fat/games/SNES/Hacks/Super Mario World.sfc", "US"),
            version("/media/usb1/games/SNES/Hacks/Super Mario World.sfc", "US"),
        ]);
        assert_eq!(labels, ["US", "US · usb0", "US · Hacks", "US · usb1"]);
    }

    #[test]
    fn files_in_one_folder_with_the_same_tags_read_as_their_file_names() {
        let labels = labels(&[
            version("/g/SNES/Super Mario World (USA).sfc", "US"),
            version("/g/SNES/Super Mario World (USA) [b].sfc", "US"),
        ]);
        assert_eq!(
            labels,
            ["Super Mario World (USA)", "Super Mario World (USA) [b]"]
        );
    }

    #[test]
    fn names_lead_when_the_versions_are_named_apart() {
        let labels = labels(&[
            Version {
                name: "Street Fighter II",
                path: "/g/Arcade/sf2.mra",
                tags: "",
            },
            Version {
                name: "Street Fighter II'",
                path: "/g/Arcade/sf2ce.mra",
                tags: "US",
            },
        ]);
        assert_eq!(labels, ["Street Fighter II", "Street Fighter II' · US"]);
    }
}
