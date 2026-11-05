// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

//! The file browser's rules: what a folder lists and in what order, the
//! starting points above every folder (Places), where a browse starts, and
//! where Back goes. Paths are plain `/`-separated strings; the adapter
//! reads the disk and hands the names in.

use crate::customization;

/// The most entries one folder lists. A larger folder ends in a row saying
/// how many were left out, so a huge directory cannot exhaust memory.
pub const LIST_CAP: usize = 2000;

/// Where a folder pick for a live folder tile starts: the `MiSTer` SD card,
/// home of the main menu's own folders.
pub const FOLDER_START: &str = "/media/fat";

/// What the browser offers to pick.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    /// Every file, as a list.
    #[default]
    Plain,
    /// Folders and image files, as a grid of previews.
    Images,
    /// Folders only, as a list, each with a row that picks the folder
    /// itself.
    Folders,
}

impl Mode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Plain => "plain",
            Self::Images => "images",
            Self::Folders => "folders",
        }
    }

    /// Shown as a list rather than a grid of previews.
    pub fn is_list(self) -> bool {
        self != Self::Images
    }

    pub fn parse(value: &str) -> Self {
        match value {
            "images" => Self::Images,
            "folders" => Self::Folders,
            _ => Self::Plain,
        }
    }
}

/// One name a folder read returned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawEntry {
    pub name: String,
    pub is_dir: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    Folder,
    File,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub kind: EntryKind,
    pub name: String,
}

/// A folder's rows, and how many matching entries the cap left out.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Listing {
    pub entries: Vec<Entry>,
    pub omitted: usize,
}

/// Hidden names are left out, the image mode keeps only folders and the
/// image types the artwork loader accepts, and the folder mode only folders. Folders come first, then
/// files, each A to Z whatever the case.
pub fn listing(raw: Vec<RawEntry>, mode: Mode, cap: usize) -> Listing {
    let mut entries: Vec<Entry> = raw
        .into_iter()
        .filter(|entry| !entry.name.starts_with('.') && !entry.name.is_empty())
        .filter(|entry| match mode {
            Mode::Plain => true,
            Mode::Images => entry.is_dir || customization::is_image_file(&entry.name),
            Mode::Folders => entry.is_dir,
        })
        .map(|entry| Entry {
            kind: if entry.is_dir {
                EntryKind::Folder
            } else {
                EntryKind::File
            },
            name: entry.name,
        })
        .collect();
    entries.sort_by(|a, b| {
        (a.kind != EntryKind::Folder)
            .cmp(&(b.kind != EntryKind::Folder))
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
            .then_with(|| a.name.cmp(&b.name))
    });
    let omitted = entries.len().saturating_sub(cap);
    entries.truncate(cap);
    Listing { entries, omitted }
}

/// A folder's letter buckets, keyed as Core keys its own ("A" to "Z",
/// "0-9", "#"), for the fast-scroll rail and a long hold's letter steps.
/// Folders lead the listing and sit before the buckets, as a browse
/// folder's directories do: `leading` counts them, and each bucket's offset
/// counts from the first file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Letters {
    pub leading: usize,
    pub labels: Vec<String>,
    pub offsets: Vec<u32>,
}

/// The bucket a name files under.
pub fn letter_key(name: &str) -> String {
    match name.chars().next() {
        Some(c) if c.is_ascii_alphabetic() => c.to_ascii_uppercase().to_string(),
        Some(c) if c.is_ascii_digit() => "0-9".to_string(),
        _ => "#".to_string(),
    }
}

pub fn letters(entries: &[Entry]) -> Letters {
    let leading = entries
        .iter()
        .take_while(|entry| entry.kind == EntryKind::Folder)
        .count();
    let mut out = Letters {
        leading,
        ..Letters::default()
    };
    for (offset, entry) in entries[leading..].iter().enumerate() {
        let key = letter_key(&entry.name);
        if out.labels.last() != Some(&key) {
            out.labels.push(key);
            out.offsets.push(u32::try_from(offset).unwrap_or(u32::MAX));
        }
    }
    out
}

/// Which machine's starting points to offer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    Mister,
    Linux,
    MacOs,
}

/// What a starting point is, which the view words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlaceKind {
    Custom,
    SdCard,
    /// A `MiSTer` USB drive by its number.
    Usb(u8),
    Network,
    Home,
    /// A removable drive by its volume name.
    Drive(String),
    Root,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Place {
    pub kind: PlaceKind,
    pub path: String,
}

/// The machine the browser lists Places for: the custom folder, plus each
/// candidate the adapter finds as a folder. `list_dir` names the folders
/// inside a mount root (`/media`, `/run/media/<user>`, `/Volumes`).
pub fn places(
    platform: Platform,
    custom_dir: &str,
    home: Option<&str>,
    user: Option<&str>,
    is_dir: &dyn Fn(&str) -> bool,
    list_dir: &dyn Fn(&str) -> Vec<String>,
) -> Vec<Place> {
    let mut out = Vec::new();
    let mut push = |kind: PlaceKind, path: String| {
        if !out.iter().any(|place: &Place| place.path == path) && is_dir(&path) {
            out.push(Place { kind, path });
        }
    };
    push(PlaceKind::Custom, trim_dir(custom_dir));
    match platform {
        Platform::Mister => {
            push(PlaceKind::SdCard, "/media/fat".into());
            for n in 0..8u8 {
                push(PlaceKind::Usb(n), format!("/media/usb{n}"));
            }
            push(PlaceKind::Network, "/media/network".into());
            push(PlaceKind::Network, "/media/fat/cifs".into());
        }
        Platform::Linux | Platform::MacOs => {
            if let Some(home) = home {
                push(PlaceKind::Home, trim_dir(home));
            }
            let mut roots = Vec::new();
            if platform == Platform::MacOs {
                roots.push("/Volumes".to_string());
            } else {
                if let Some(user) = user {
                    roots.push(format!("/run/media/{user}"));
                }
                roots.push("/media".to_string());
            }
            for root in roots {
                let mut names = list_dir(&root);
                names.retain(|name| !name.starts_with('.'));
                names.sort_by_key(|name| name.to_lowercase());
                for name in names {
                    push(PlaceKind::Drive(name.clone()), join(&root, &name));
                }
            }
        }
    }
    push(PlaceKind::Root, "/".into());
    out
}

/// The Places a folder picked for Core can sit under: the drives Core
/// browses, not the custom folder or the root of the system.
pub fn core_places(places: Vec<Place>) -> Vec<Place> {
    places
        .into_iter()
        .filter(|place| {
            matches!(
                place.kind,
                PlaceKind::SdCard | PlaceKind::Usb(_) | PlaceKind::Network
            )
        })
        .collect()
}

/// Where a browse starts: the folder of the current icon when it is a
/// path, else the custom `hub` folder, else the custom folder. `None`
/// starts on Places.
pub fn start_dir(
    current_icon: &str,
    custom_dir: &str,
    is_dir: &dyn Fn(&str) -> bool,
) -> Option<String> {
    let mut candidates = Vec::new();
    if customization::is_absolute_icon(current_icon) {
        candidates.push(parent_dir(current_icon));
    }
    let custom = trim_dir(custom_dir);
    if !custom.is_empty() {
        candidates.push(join(&custom, "hub"));
        candidates.push(custom);
    }
    candidates
        .into_iter()
        .find(|dir| !dir.is_empty() && is_dir(dir))
}

/// The Place a folder belongs to: the deepest one containing it. Back at
/// that Place's own folder shows Places.
pub fn place_for<'a>(places: &'a [Place], dir: &str) -> Option<&'a Place> {
    places
        .iter()
        .filter(|place| contains(&place.path, dir))
        .max_by_key(|place| place.path.len())
}

/// Where Back goes from `dir`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Up {
    Folder(String),
    Places,
}

pub fn up(dir: &str, place_root: &str) -> Up {
    let dir = trim_dir(dir);
    if dir == trim_dir(place_root) || dir == "/" || !dir.starts_with('/') {
        return Up::Places;
    }
    Up::Folder(parent_dir(&dir))
}

/// A folder's parent; `/` for a top-level folder.
pub fn parent_dir(path: &str) -> String {
    let path = trim_dir(path);
    match path.rfind('/') {
        Some(0) | None => "/".to_string(),
        Some(at) => path[..at].to_string(),
    }
}

/// The last segment of a path, which titles its folder.
pub fn base_name(path: &str) -> &str {
    let path = path.trim_end_matches('/');
    path.rsplit('/').next().unwrap_or(path)
}

pub fn join(dir: &str, name: &str) -> String {
    if dir.ends_with('/') {
        format!("{dir}{name}")
    } else {
        format!("{dir}/{name}")
    }
}

fn trim_dir(path: &str) -> String {
    let trimmed = path.trim_end_matches('/');
    if trimmed.is_empty() && path.starts_with('/') {
        "/".to_string()
    } else {
        trimmed.to_string()
    }
}

fn contains(root: &str, dir: &str) -> bool {
    let root = trim_dir(root);
    let dir = trim_dir(dir);
    root == "/" || dir == root || dir.starts_with(&format!("{root}/"))
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    reason = "tests should fail fast on a missing place"
)]
mod tests {
    use super::*;

    fn raw(name: &str, is_dir: bool) -> RawEntry {
        RawEntry {
            name: name.into(),
            is_dir,
        }
    }

    fn names(listing: &Listing) -> Vec<&str> {
        listing.entries.iter().map(|e| e.name.as_str()).collect()
    }

    #[test]
    fn the_folder_mode_lists_folders_only_and_round_trips() {
        let out = listing(
            vec![
                raw("a.mgl", false),
                raw("_Arcade", true),
                raw("b.rbf", false),
            ],
            Mode::Folders,
            LIST_CAP,
        );
        assert_eq!(names(&out), ["_Arcade"]);
        assert_eq!(Mode::parse(Mode::Folders.as_str()), Mode::Folders);
        assert!(Mode::Folders.is_list() && Mode::Plain.is_list() && !Mode::Images.is_list());
    }

    #[test]
    fn core_places_keep_only_the_drives_core_browses() {
        let all = places(
            Platform::Mister,
            "/media/fat/zaparoo/custom",
            None,
            None,
            &|_| true,
            &|_| Vec::new(),
        );
        let kept: Vec<String> = core_places(all).into_iter().map(|p| p.path).collect();
        assert_eq!(kept.first().map(String::as_str), Some("/media/fat"));
        assert!(!kept
            .iter()
            .any(|p| p == "/" || p == "/media/fat/zaparoo/custom"));
        assert!(kept.iter().any(|p| p == "/media/usb0"));
    }

    #[test]
    fn folders_lead_then_files_a_to_z_and_hidden_names_are_left_out() {
        let out = listing(
            vec![
                raw("b.png", false),
                raw(".cache", true),
                raw("Zelda", true),
                raw("A.png", false),
                raw("art", true),
                raw(".hidden.png", false),
                raw("notes.txt", false),
            ],
            Mode::Plain,
            LIST_CAP,
        );
        assert_eq!(names(&out), ["art", "Zelda", "A.png", "b.png", "notes.txt"]);
        assert_eq!(out.omitted, 0);
    }

    #[test]
    fn the_image_mode_lists_folders_and_images_only() {
        let out = listing(
            vec![raw("a.txt", false), raw("b.SVG", false), raw("dir", true)],
            Mode::Images,
            LIST_CAP,
        );
        assert_eq!(names(&out), ["dir", "b.SVG"]);
    }

    #[test]
    fn a_large_folder_is_capped_and_counts_what_it_left_out() {
        let raw_entries = (0..10).map(|i| raw(&format!("{i}.png"), false)).collect();
        let out = listing(raw_entries, Mode::Images, 4);
        assert_eq!(out.entries.len(), 4);
        assert_eq!(out.omitted, 6);
    }

    #[test]
    fn files_bucket_by_letter_after_the_leading_folders() {
        let out = listing(
            vec![
                raw("zelda", true),
                raw("1942.png", false),
                raw("Arcade.png", false),
                raw("atari.svg", false),
                raw("Batman.png", false),
                raw("_x.png", false),
            ],
            Mode::Images,
            LIST_CAP,
        );
        let letters = letters(&out.entries);
        assert_eq!(letters.leading, 1);
        // Names sort as they are cased down: `_` comes before the letters.
        assert_eq!(letters.labels, ["0-9", "#", "A", "B"]);
        assert_eq!(letters.offsets, [0, 1, 2, 4]);
        assert_eq!(letter_key("éclair"), "#");
    }

    #[test]
    fn mister_places_are_the_drives_that_exist() {
        let present = [
            "/media/fat/zaparoo/custom",
            "/media/fat",
            "/media/usb1",
            "/",
        ];
        let is_dir = |p: &str| present.contains(&p);
        let out = places(
            Platform::Mister,
            "/media/fat/zaparoo/custom/",
            None,
            None,
            &is_dir,
            &|_| Vec::new(),
        );
        let kinds: Vec<PlaceKind> = out.into_iter().map(|p| p.kind).collect();
        assert_eq!(
            kinds,
            [
                PlaceKind::Custom,
                PlaceKind::SdCard,
                PlaceKind::Usb(1),
                PlaceKind::Root
            ]
        );
    }

    #[test]
    fn desktop_places_list_home_and_each_removable_drive() {
        let is_dir = |p: &str| p != "/media/ghost";
        let list_dir = |root: &str| match root {
            "/run/media/deck" => vec!["SD".to_string(), ".trash".to_string()],
            "/media" => vec!["ghost".to_string()],
            _ => Vec::new(),
        };
        let out = places(
            Platform::Linux,
            "/home/deck/.config/zaparoo/custom",
            Some("/home/deck"),
            Some("deck"),
            &is_dir,
            &list_dir,
        );
        let paths: Vec<&str> = out.iter().map(|p| p.path.as_str()).collect();
        assert_eq!(
            paths,
            [
                "/home/deck/.config/zaparoo/custom",
                "/home/deck",
                "/run/media/deck/SD",
                "/"
            ]
        );
        assert_eq!(out[2].kind, PlaceKind::Drive("SD".into()));
    }

    #[test]
    fn a_browse_starts_at_the_icon_then_the_custom_folders() {
        let all = |_: &str| true;
        assert_eq!(
            start_dir("/media/usb0/art/zelda.png", "/c", &all).as_deref(),
            Some("/media/usb0/art")
        );
        assert_eq!(start_dir("rpgs", "/c/", &all).as_deref(), Some("/c/hub"));
        let no_hub = |p: &str| p == "/c";
        assert_eq!(start_dir("", "/c", &no_hub).as_deref(), Some("/c"));
        assert_eq!(start_dir("/gone/a.png", "/c", &|_| false), None);
    }

    #[test]
    fn back_climbs_to_the_place_then_shows_places() {
        let all_places = vec![
            Place {
                kind: PlaceKind::SdCard,
                path: "/media/fat".into(),
            },
            Place {
                kind: PlaceKind::Custom,
                path: "/media/fat/zaparoo/custom".into(),
            },
            Place {
                kind: PlaceKind::Root,
                path: "/".into(),
            },
        ];
        let place = place_for(&all_places, "/media/fat/zaparoo/custom/hub").expect("place");
        assert_eq!(place.kind, PlaceKind::Custom);
        assert_eq!(
            up("/media/fat/zaparoo/custom/hub", &place.path),
            Up::Folder("/media/fat/zaparoo/custom".into())
        );
        assert_eq!(up("/media/fat/zaparoo/custom", &place.path), Up::Places);
        assert_eq!(
            place_for(&all_places, "/etc").map(|p| &p.kind),
            Some(&PlaceKind::Root)
        );
        assert_eq!(up("/etc", "/"), Up::Folder("/".into()));
        assert_eq!(up("/", "/"), Up::Places);
    }

    #[test]
    fn paths_split_and_join_on_slashes() {
        assert_eq!(parent_dir("/a/b/"), "/a");
        assert_eq!(parent_dir("/a"), "/");
        assert_eq!(base_name("/a/b/"), "b");
        assert_eq!(join("/", "a"), "/a");
        assert_eq!(join("/a", "b"), "/a/b");
        assert_eq!(Mode::parse(Mode::Images.as_str()), Mode::Images);
        assert_eq!(Mode::parse("unknown"), Mode::Plain);
    }
}
