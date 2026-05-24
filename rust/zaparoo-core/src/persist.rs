// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// Persistent UI state. MiSTer's parent process kills the frontend binary
// without notice; every user-visible navigation choice must round-trip
// through disk so the next boot resumes where the user was.
//
// Layout: a single TOML with one section per screen. Pre-release; if a
// schema needs to change, just change it and tell users to delete the
// state file. No version field, no migration code.
//
// Written synchronously on every mutation (write-through). Parent can
// SIGKILL between any two lines, so the loss window must be zero. File
// is tiny (<300 bytes) and lives on tmpfs on MiSTer; sync cost on the
// Qt thread is microseconds.

use crate::platform_paths::state_file_path;
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};
use tracing::warn;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PersistedState {
    pub active_screen: String,
    pub hub: HubState,
    pub systems: SystemsState,
    pub games: GamesState,
    pub favorites: FavoritesState,
    pub recents: RecentsState,
    pub settings: SettingsState,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct HubState {
    pub category: String,
    /// Which Hub row had focus on the last persisted move. 0 = top
    /// (categories), 1 = bottom (action tiles).
    pub selected_row: u32,
    /// The bottom-row action tile that last had focus. One of
    /// `"favorites"`, `"recents"` or `"settings"`. Empty defaults
    /// to the leftmost action when restored.
    pub selected_action: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SystemsState {
    pub system_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct GamesState {
    pub system_id: String,
    pub path_stack: Vec<String>,
    pub selected_at_level: Vec<String>,
}

impl Default for GamesState {
    fn default() -> Self {
        Self {
            system_id: String::new(),
            path_stack: vec![String::new()],
            selected_at_level: vec![String::new()],
        }
    }
}

/// Recently-played selection state. The list itself is owned by Core
/// (`media.history`); we just remember which entry the user was on so
/// a kill-resume keeps focus.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct RecentsState {
    pub selected_path: String,
}

/// Favorite-games selection state. The list itself is owned by Core
/// (`media.search` with `user:favorite`); we just remember focus.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct FavoritesState {
    pub selected_path: String,
}

/// Per-frontend Settings selections. `resolution` is `"WxH"` (e.g.
/// `"1920x1080"`); empty means "no Settings override" and the value
/// from `[mister.video_*]` in `frontend.toml` is left in place.
/// `language` mirrors `[general].language` in `frontend.toml` so the UI
/// settings snapshot stays coherent with the config-backed startup path.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SettingsState {
    pub resolution: String,
    pub language: String,
    #[serde(default = "default_browse_layout")]
    pub browse_layout: String,
    #[serde(default = "default_button_layout")]
    pub button_layout: String,
    #[serde(default = "default_mouse_enabled")]
    pub mouse_enabled: bool,
    #[serde(default)]
    pub discover_arcade_alternate_versions: bool,
    #[serde(default)]
    pub debug_logging: bool,
    #[serde(default = "default_screensaver_timeout")]
    pub screensaver_timeout: String,
}

impl Default for SettingsState {
    fn default() -> Self {
        Self {
            resolution: String::new(),
            language: String::new(),
            browse_layout: default_browse_layout(),
            button_layout: default_button_layout(),
            mouse_enabled: default_mouse_enabled(),
            discover_arcade_alternate_versions: false,
            debug_logging: false,
            screensaver_timeout: default_screensaver_timeout(),
        }
    }
}

fn default_browse_layout() -> String {
    "grid".into()
}

fn default_button_layout() -> String {
    // Style A — formerly "nintendo". `models::settings::normalize_button_layout`
    // migrates legacy persisted values, so this default only applies to
    // brand-new state files.
    "a".into()
}

fn default_mouse_enabled() -> bool {
    true
}

fn default_screensaver_timeout() -> String {
    "300".into()
}

pub fn load() -> PersistedState {
    load_from(&state_file_path())
}

pub fn save(state: &PersistedState) {
    save_to(&state_file_path(), state);
}

fn load_from(path: &Path) -> PersistedState {
    let Ok(src) = std::fs::read_to_string(path) else {
        return PersistedState::default();
    };
    match toml::from_str(&src) {
        Ok(s) => s,
        Err(e) => {
            warn!("persist state parse error in {}: {e}", path.display());
            PersistedState::default()
        }
    }
}

fn save_to(path: &Path, state: &PersistedState) {
    let serialized = match toml::to_string(state) {
        Ok(s) => s,
        Err(e) => {
            warn!("persist state serialisation failed: {e}");
            return;
        }
    };
    if let Some(parent) = path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            warn!("could not create {}: {e}", parent.display());
            return;
        }
    }
    // Atomic replace: write to a unique sibling temp file, then rename. A
    // crash mid-write leaves the temp behind but never a torn state.toml.
    let tmp = tmp_sibling(path);
    match std::fs::File::create(&tmp).and_then(|mut f| {
        f.write_all(serialized.as_bytes())?;
        f.sync_all()?;
        Ok(())
    }) {
        Ok(()) => {}
        Err(e) => {
            warn!("persist state write to {} failed: {e}", tmp.display());
            let _ = std::fs::remove_file(&tmp);
            return;
        }
    }
    if let Err(e) = std::fs::rename(&tmp, path) {
        warn!(
            "persist state rename {} → {} failed: {e}",
            tmp.display(),
            path.display()
        );
        let _ = std::fs::remove_file(&tmp);
    }
}

fn tmp_sibling(path: &Path) -> PathBuf {
    // Per-process-and-thread suffix keeps concurrent writers from
    // clobbering each other's temp file and then racing on rename. The
    // final rename is still atomic; only the last one wins, which is the
    // desired semantics.
    let pid = std::process::id();
    let tid = format!("{:?}", std::thread::current().id());
    let tid_clean: String = tid.chars().filter(char::is_ascii_alphanumeric).collect();
    let suffix = format!(".tmp.{pid}.{tid_clean}");
    let mut buf = path.as_os_str().to_owned();
    buf.push(&suffix);
    PathBuf::from(buf)
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::unwrap_used,
        clippy::panic,
        reason = "tests should fail-fast on unexpected errors"
    )]

    use super::{
        load_from, save_to, FavoritesState, GamesState, HubState, PersistedState, RecentsState,
        SettingsState, SystemsState,
    };
    use std::thread;

    #[test]
    fn load_returns_default_on_missing_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("missing.toml");
        let state = load_from(&path);
        assert_eq!(state, PersistedState::default());
    }

    #[test]
    fn save_then_load_round_trips_all_sections() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("state.toml");
        let original = PersistedState {
            active_screen: "games".into(),
            hub: HubState {
                category: "Consoles".into(),
                selected_row: 1,
                selected_action: "settings".into(),
            },
            systems: SystemsState {
                system_id: "NES".into(),
            },
            games: GamesState {
                system_id: "NES".into(),
                path_stack: vec![String::new(), "/roms/nes/mario".into()],
                selected_at_level: vec!["/roms/nes/mario".into(), "/roms/nes/mario/smb.nes".into()],
            },
            recents: RecentsState {
                selected_path: "/roms/nes/mario/smb.nes".into(),
            },
            favorites: FavoritesState {
                selected_path: "/roms/nes/zelda.nes".into(),
            },
            settings: SettingsState {
                resolution: "1920x1080".into(),
                language: "it_IT".into(),
                browse_layout: "list".into(),
                button_layout: "b".into(),
                mouse_enabled: false,
                discover_arcade_alternate_versions: true,
                debug_logging: true,
                screensaver_timeout: "300".into(),
            },
        };
        save_to(&path, &original);
        let loaded = load_from(&path);
        assert_eq!(loaded, original);
    }

    #[test]
    fn load_from_malformed_toml_returns_default() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("state.toml");
        std::fs::write(&path, "this is = not [ valid toml").expect("write");
        let state = load_from(&path);
        assert_eq!(state, PersistedState::default());
    }

    #[test]
    fn save_creates_parent_directories() {
        let dir = tempfile::tempdir().expect("tempdir");
        let nested = dir.path().join("deeper").join("sub").join("state.toml");
        save_to(&nested, &PersistedState::default());
        assert!(nested.exists(), "state file was not created at {nested:?}");
    }

    #[test]
    fn save_is_atomic_under_concurrent_writes() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("state.toml");
        let handles: Vec<_> = (0..8)
            .map(|i| {
                let path = path.clone();
                thread::spawn(move || {
                    for j in 0..20 {
                        let state = PersistedState {
                            active_screen: format!("screen-{i}"),
                            hub: HubState {
                                category: format!("cat-{i}-{j}"),
                                selected_row: 0,
                                selected_action: String::new(),
                            },
                            systems: SystemsState {
                                system_id: format!("sys-{i}-{j}"),
                            },
                            games: GamesState {
                                system_id: format!("sys-{i}-{j}"),
                                path_stack: vec![String::new()],
                                selected_at_level: vec![format!("/roms/{i}/{j}.rom")],
                            },
                            favorites: FavoritesState::default(),
                            recents: RecentsState::default(),
                            settings: SettingsState::default(),
                        };
                        save_to(&path, &state);
                    }
                })
            })
            .collect();
        for h in handles {
            h.join().expect("thread join");
        }
        let final_state = load_from(&path);
        assert!(final_state.active_screen.starts_with("screen-"));
        assert!(final_state.hub.category.starts_with("cat-"));
        assert!(final_state.systems.system_id.starts_with("sys-"));
    }

    #[test]
    fn empty_sections_deserialise_via_default() {
        // A file with only `active_screen` — every section should
        // populate from `Default`.
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("state.toml");
        std::fs::write(&path, "active_screen = \"hub\"\n").expect("write");
        let state = load_from(&path);
        assert_eq!(state.active_screen, "hub");
        assert_eq!(state.hub, HubState::default());
        assert_eq!(state.systems, SystemsState::default());
        assert_eq!(state.games, GamesState::default());
        assert_eq!(state.favorites, FavoritesState::default());
        assert_eq!(state.recents, RecentsState::default());
        assert_eq!(state.settings, SettingsState::default());
    }

    #[test]
    fn missing_settings_fields_default_to_current_behavior() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("state.toml");
        std::fs::write(&path, "[settings]\nresolution = \"1920x1080\"\n").expect("write");
        let state = load_from(&path);
        assert_eq!(state.settings.resolution, "1920x1080");
        assert_eq!(state.settings.language, "");
        assert_eq!(state.settings.browse_layout, "grid");
        assert_eq!(state.settings.button_layout, "a");
        assert!(state.settings.mouse_enabled);
        assert!(!state.settings.debug_logging);
    }

    #[test]
    fn unknown_field_in_section_is_ignored() {
        // Forward-compat: adding a new field in a future version then
        // downgrading should not wipe state. Serde default drops
        // unknown fields silently.
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("state.toml");
        let on_disk = r#"[hub]
category = "Arcade"
future_field = "ignored"

[systems]
system_id = "NES"
future_field = "ignored"

[games]
system_id = "NES"
path_stack = [""]
selected_at_level = ["/x.rom"]
future_field = "ignored"
"#;
        std::fs::write(&path, on_disk).expect("write");
        let state = load_from(&path);
        assert_eq!(state.hub.category, "Arcade");
        assert_eq!(state.systems.system_id, "NES");
        assert_eq!(state.games.path_stack, vec![""]);
        assert_eq!(state.games.selected_at_level, vec!["/x.rom"]);
    }
}
