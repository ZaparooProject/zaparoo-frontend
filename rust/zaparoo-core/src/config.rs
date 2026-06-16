// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

use crate::input_actions;
use serde::Deserialize;
use std::collections::HashMap;
use std::io::Write;
use std::path::Path;
use tracing::warn;

#[derive(Debug, Clone)]
pub struct Config {
    pub core_endpoint: String,
    pub video_width: u32,
    pub video_height: u32,
    /// True when at least one of `[video] width` / `height` was present in
    /// the loaded `frontend.toml`. The desktop CRT preview uses this to
    /// distinguish "user wants the default 1920x1080" (in which case the
    /// preview canvas would be too large to upscale into a desktop window)
    /// from "user didn't write a [video] section at all" (in which case
    /// `--crt` overrides this to the 320x240 `native_video_writer` canvas).
    pub video_explicit: bool,
    pub debug_logging: bool,
    /// Language override for the UI, passed to `QTranslator` via the
    /// C++ entry point. Empty string means "follow `QLocale::system()`";
    /// any non-empty value is treated as a BCP-47 tag (e.g. `en_US`,
    /// `ja`, `de_DE`). Populated from `[general] language` in the config
    /// file; the literal `auto` is normalised to an empty string.
    pub language: String,
    /// Qt key code → action name. Built at load time by merging
    /// `[input.keyboard]` overrides onto `input_actions::default_bindings()`
    /// and inverting.
    pub key_to_action: HashMap<i32, String>,
    /// Durable mirror of frontend-owned settings. These stay in
    /// `frontend.toml` so they survive `MiSTer`'s `/tmp` lifecycle.
    pub settings: SettingsConfig,
    /// First-run notices the user has acknowledged. Stored in
    /// `frontend.toml` (not `state.toml`) because `MiSTer`'s `/tmp`
    /// state is wiped on reboot — using state would re-show the notice
    /// every cold boot.
    pub notice: NoticeConfig,
    /// Optional directory scanned at startup for user-supplied system
    /// artwork. Files whose stem matches a Zaparoo system id (case-exact)
    /// are served as-is — no tint pipeline — via the `system-image` image
    /// provider. Configured via `[images] system_dir` in `frontend.toml`.
    /// Absent/empty means the feature is off.
    pub system_image_dir: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SettingsConfig {
    pub orientation: Option<String>,
    pub clock_format: Option<String>,
    pub browse_layout: Option<String>,
    pub button_layout: Option<String>,
    pub mouse_enabled: Option<bool>,
    pub reduce_motion: Option<bool>,
    pub discover_arcade_alternate_versions: Option<bool>,
    pub screensaver_timeout: Option<String>,
    pub media_image_type: Option<String>,
    pub show_hidden: Option<bool>,
    pub show_original_filenames: Option<bool>,
    pub region: Option<String>,
}

#[allow(
    clippy::struct_excessive_bools,
    reason = "flat settings mirror; each bool is an independent user-visible toggle"
)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SettingsMirror<'a> {
    pub resolution: &'a str,
    pub language: &'a str,
    pub orientation: &'a str,
    pub clock_format: &'a str,
    pub browse_layout: &'a str,
    pub button_layout: &'a str,
    pub mouse_enabled: bool,
    pub reduce_motion: bool,
    pub discover_arcade_alternate_versions: bool,
    pub debug_logging: bool,
    pub screensaver_timeout: &'a str,
    pub media_image_type: &'a str,
    pub show_hidden: bool,
    pub show_original_filenames: bool,
    pub region: &'a str,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct NoticeConfig {
    pub commercial_ack: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            core_endpoint: "ws://localhost:7497/api/v0.1".into(),
            video_width: 1920,
            video_height: 1080,
            video_explicit: false,
            debug_logging: false,
            language: String::new(),
            key_to_action: input_actions::invert(&input_actions::default_bindings()),
            settings: SettingsConfig::default(),
            notice: NoticeConfig::default(),
            system_image_dir: None,
        }
    }
}

#[derive(Deserialize, Default)]
struct RawConfig {
    #[serde(default)]
    general: RawGeneral,
    #[serde(default)]
    core: RawCore,
    #[serde(default)]
    video: RawVideo,
    #[serde(default)]
    logging: RawLogging,
    #[serde(default)]
    input: RawInput,
    #[serde(default)]
    settings: RawSettings,
    #[serde(default)]
    notice: RawNotice,
    #[serde(default)]
    images: RawImages,
}

#[derive(Deserialize, Default)]
struct RawGeneral {
    language: Option<String>,
}

#[derive(Deserialize, Default)]
struct RawCore {
    endpoint: Option<String>,
}

#[derive(Deserialize, Default)]
struct RawVideo {
    width: Option<u32>,
    height: Option<u32>,
}

#[derive(Deserialize, Default)]
struct RawLogging {
    debug: Option<bool>,
}

#[derive(Deserialize, Default)]
struct RawInput {
    #[serde(default)]
    keyboard: HashMap<String, Vec<String>>,
}

#[derive(Deserialize, Default)]
struct RawSettings {
    orientation: Option<String>,
    clock_format: Option<String>,
    browse_layout: Option<String>,
    button_layout: Option<String>,
    mouse_enabled: Option<bool>,
    reduce_motion: Option<bool>,
    discover_arcade_alternate_versions: Option<bool>,
    screensaver_timeout: Option<String>,
    media_image_type: Option<String>,
    show_hidden: Option<bool>,
    show_original_filenames: Option<bool>,
    region: Option<String>,
}

#[derive(Deserialize, Default)]
struct RawNotice {
    commercial_ack: Option<bool>,
}

#[derive(Deserialize, Default)]
struct RawImages {
    system_dir: Option<String>,
}

pub fn load_config(path: &Path) -> Config {
    let mut cfg = Config::default();
    let raw: RawConfig = match std::fs::read_to_string(path) {
        Ok(src) => match toml::from_str(&src) {
            Ok(r) => r,
            Err(e) => {
                warn!("config parse error in {}: {e}", path.display());
                // Fall through with defaults so the env-var override
                // below still applies on a malformed file.
                RawConfig::default()
            }
        },
        // Missing file is the first-run case. Don't early-return — the
        // env-var override below must still apply, otherwise an invocation
        // like `ZAPAROO_CORE_ENDPOINT=… just run` with no frontend.toml
        // silently falls back to the localhost default and the frontend
        // sits in CONNECTING forever.
        Err(_) => RawConfig::default(),
    };
    if let Some(lang) = raw.general.language {
        // "auto" is the documented opt-in to system-locale detection; treat
        // it as an empty override so the C++ side just calls `QLocale::system()`.
        cfg.language = if lang.eq_ignore_ascii_case("auto") {
            String::new()
        } else {
            lang
        };
    }
    if let Some(ep) = raw.core.endpoint {
        cfg.core_endpoint = ep;
    }
    // Env-var override wins over both the built-in default and any
    // frontend.toml setting. Used by run-dev.sh to point the frontend at
    // mock-core (port 27497) without forcing the user to maintain a
    // throwaway frontend.toml.
    if let Ok(ep) = std::env::var("ZAPAROO_CORE_ENDPOINT") {
        if !ep.is_empty() {
            cfg.core_endpoint = ep;
        }
    }
    cfg.video_explicit = raw.video.width.is_some() || raw.video.height.is_some();
    if let Some(w) = raw.video.width {
        cfg.video_width = w;
    }
    if let Some(h) = raw.video.height {
        cfg.video_height = h;
    }
    if let Some(d) = raw.logging.debug {
        cfg.debug_logging = d;
    }
    if !raw.input.keyboard.is_empty() {
        let mut merged = input_actions::default_bindings();
        for (action, keys) in raw.input.keyboard {
            merged.insert(action, keys);
        }
        cfg.key_to_action = input_actions::invert(&merged);
    }
    cfg.settings = SettingsConfig {
        orientation: raw
            .settings
            .orientation
            .map(|value| value.trim().to_string()),
        clock_format: raw
            .settings
            .clock_format
            .map(|value| value.trim().to_string()),
        browse_layout: raw
            .settings
            .browse_layout
            .map(|value| value.trim().to_string()),
        button_layout: raw
            .settings
            .button_layout
            .map(|value| value.trim().to_string()),
        mouse_enabled: raw.settings.mouse_enabled,
        reduce_motion: raw.settings.reduce_motion,
        discover_arcade_alternate_versions: raw.settings.discover_arcade_alternate_versions,
        screensaver_timeout: raw
            .settings
            .screensaver_timeout
            .map(|value| value.trim().to_string()),
        media_image_type: raw
            .settings
            .media_image_type
            .map(|value| value.trim().to_string()),
        show_hidden: raw.settings.show_hidden,
        show_original_filenames: raw.settings.show_original_filenames,
        region: raw.settings.region.map(|value| value.trim().to_string()),
    };
    cfg.notice = NoticeConfig {
        commercial_ack: raw.notice.commercial_ack.unwrap_or(false),
    };
    cfg.system_image_dir = raw
        .images
        .system_dir
        .map(|value| value.trim().to_string())
        .filter(|s| !s.is_empty());
    cfg
}

/// Get a mutable reference to a TOML section table, creating it if absent.
fn section_mut<'a>(
    table: &'a mut toml::Table,
    key: &'static str,
    path: &Path,
) -> Result<&'a mut toml::Table, String> {
    let v = table
        .entry(key)
        .or_insert_with(|| toml::Value::Table(toml::Table::new()));
    v.as_table_mut()
        .ok_or_else(|| format!("config key [{key}] in {} is not a table", path.display()))
}

pub fn save_settings_mirror(path: &Path, mirror: SettingsMirror<'_>) -> Result<(), String> {
    let mut table = if path.exists() {
        let src = std::fs::read_to_string(path)
            .map_err(|e| format!("could not read {}: {e}", path.display()))?;
        toml::from_str::<toml::Table>(&src)
            .map_err(|e| format!("config parse error in {}: {e}", path.display()))?
    } else {
        toml::Table::new()
    };

    let general = section_mut(&mut table, "general", path)?;
    general.insert(
        "language".into(),
        toml::Value::String(normalize_language_override(mirror.language)),
    );

    let video = section_mut(&mut table, "video", path)?;
    video.remove("backend");
    if let Some((width, height)) = parse_resolution_override(mirror.resolution) {
        video.insert("width".into(), toml::Value::Integer(i64::from(width)));
        video.insert("height".into(), toml::Value::Integer(i64::from(height)));
    } else {
        video.remove("width");
        video.remove("height");
    }

    let settings = section_mut(&mut table, "settings", path)?;
    settings.insert(
        "orientation".into(),
        toml::Value::String(mirror.orientation.trim().to_string()),
    );
    settings.insert(
        "clock_format".into(),
        toml::Value::String(mirror.clock_format.trim().to_string()),
    );
    settings.insert(
        "browse_layout".into(),
        toml::Value::String(mirror.browse_layout.trim().to_string()),
    );
    settings.insert(
        "button_layout".into(),
        toml::Value::String(mirror.button_layout.trim().to_string()),
    );
    settings.insert(
        "mouse_enabled".into(),
        toml::Value::Boolean(mirror.mouse_enabled),
    );
    settings.insert(
        "reduce_motion".into(),
        toml::Value::Boolean(mirror.reduce_motion),
    );
    settings.insert(
        "discover_arcade_alternate_versions".into(),
        toml::Value::Boolean(mirror.discover_arcade_alternate_versions),
    );
    settings.insert(
        "screensaver_timeout".into(),
        toml::Value::String(mirror.screensaver_timeout.trim().to_string()),
    );
    settings.insert(
        "media_image_type".into(),
        toml::Value::String(mirror.media_image_type.trim().to_string()),
    );
    settings.insert(
        "show_hidden".into(),
        toml::Value::Boolean(mirror.show_hidden),
    );
    settings.insert(
        "show_original_filenames".into(),
        toml::Value::Boolean(mirror.show_original_filenames),
    );
    settings.insert(
        "region".into(),
        toml::Value::String(mirror.region.trim().to_string()),
    );

    let logging = section_mut(&mut table, "logging", path)?;
    logging.insert("debug".into(), toml::Value::Boolean(mirror.debug_logging));

    let serialized =
        toml::to_string(&table).map_err(|e| format!("config serialisation failed: {e}"))?;
    write_atomic(path, serialized.as_bytes())
        .map_err(|e| format!("could not write {}: {e}", path.display()))
}

/// Persist a first-run notice acknowledgement into `frontend.toml`.
/// Mirrors `save_settings_mirror`'s atomic-write + section-preserving
/// pattern so unrelated keys in the file (core endpoint, video, input
/// bindings) survive untouched.
pub fn save_notice_ack(path: &Path, commercial_ack: bool) -> Result<(), String> {
    let mut table = if path.exists() {
        let src = std::fs::read_to_string(path)
            .map_err(|e| format!("could not read {}: {e}", path.display()))?;
        toml::from_str::<toml::Table>(&src)
            .map_err(|e| format!("config parse error in {}: {e}", path.display()))?
    } else {
        toml::Table::new()
    };

    let notice_value = table
        .entry("notice")
        .or_insert_with(|| toml::Value::Table(toml::Table::new()));
    let Some(notice) = notice_value.as_table_mut() else {
        return Err(format!(
            "config key [notice] in {} is not a table",
            path.display()
        ));
    };
    notice.insert(
        "commercial_ack".into(),
        toml::Value::Boolean(commercial_ack),
    );

    let serialized =
        toml::to_string(&table).map_err(|e| format!("config serialisation failed: {e}"))?;
    write_atomic(path, serialized.as_bytes())
        .map_err(|e| format!("could not write {}: {e}", path.display()))
}

fn normalize_language_override(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("auto") {
        "auto".into()
    } else {
        trimmed.into()
    }
}

fn parse_resolution_override(value: &str) -> Option<(u32, u32)> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    let (width, height) = trimmed
        .split_once('x')
        .or_else(|| trimmed.split_once('X'))?;
    let width = width.trim().parse().ok()?;
    let height = height.trim().parse().ok()?;
    if width == 0 || height == 0 {
        return None;
    }
    Some((width, height))
}

fn write_atomic(path: &Path, contents: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let tmp = tmp_sibling(path);
    let write_result = std::fs::File::create(&tmp).and_then(|mut file| {
        file.write_all(contents)?;
        file.sync_all()?;
        Ok(())
    });
    if let Err(e) = write_result {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    if let Err(e) = std::fs::rename(&tmp, path) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    Ok(())
}

fn tmp_sibling(path: &Path) -> std::path::PathBuf {
    let pid = std::process::id();
    let tid = format!("{:?}", std::thread::current().id());
    let tid_clean: String = tid.chars().filter(char::is_ascii_alphanumeric).collect();
    let suffix = format!(".tmp.{pid}.{tid_clean}");
    let mut buf = path.as_os_str().to_owned();
    buf.push(&suffix);
    std::path::PathBuf::from(buf)
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::unwrap_used,
        clippy::panic,
        reason = "tests should fail-fast on unexpected errors"
    )]

    use super::{load_config, save_notice_ack, save_settings_mirror, Config, SettingsMirror};
    use std::io::Write;

    fn write_tmp(contents: &str) -> tempfile::NamedTempFile {
        let mut f = tempfile::NamedTempFile::new().expect("tempfile");
        f.write_all(contents.as_bytes()).expect("write");
        f
    }

    #[test]
    fn defaults_match_production_values() {
        let cfg = Config::default();
        assert_eq!(cfg.core_endpoint, "ws://localhost:7497/api/v0.1");
        assert_eq!(cfg.video_width, 1920);
        assert_eq!(cfg.video_height, 1080);
        assert!(!cfg.debug_logging);
        assert_eq!(cfg.language, "");
        assert_eq!(cfg.settings.orientation, None);
        assert_eq!(cfg.settings.clock_format, None);
        assert_eq!(cfg.settings.browse_layout, None);
        assert_eq!(cfg.settings.button_layout, None);
        assert_eq!(cfg.settings.mouse_enabled, None);
        assert_eq!(cfg.settings.discover_arcade_alternate_versions, None);
        assert_eq!(cfg.settings.region, None);
        assert!(cfg.system_image_dir.is_none());
        assert!(!cfg.notice.commercial_ack);
        // Default keyboard bindings populate the map.
        assert!(!cfg.key_to_action.is_empty());
    }

    #[test]
    fn system_image_dir_round_trips() {
        let f = write_tmp("[images]\nsystem_dir = \"/mnt/art/systems\"\n");
        let cfg = load_config(f.path());
        assert_eq!(cfg.system_image_dir.as_deref(), Some("/mnt/art/systems"));
    }

    #[test]
    fn system_image_dir_absent_is_none() {
        let f = write_tmp("[core]\nendpoint = \"ws://example.com/api\"\n");
        let cfg = load_config(f.path());
        assert!(cfg.system_image_dir.is_none());
    }

    #[test]
    fn region_setting_round_trips() {
        let f = write_tmp("[settings]\nregion = \"jp\"\n");
        let cfg = load_config(f.path());
        assert_eq!(cfg.settings.region.as_deref(), Some("jp"));
    }

    #[test]
    fn notice_commercial_ack_round_trips() {
        let f = write_tmp("[notice]\ncommercial_ack = true\n");
        let cfg = load_config(f.path());
        assert!(cfg.notice.commercial_ack);
    }

    #[test]
    fn save_notice_ack_creates_section_and_preserves_others() {
        let f = write_tmp(
            "[core]\nendpoint = \"ws://example.com/api\"\n[settings]\nbutton_layout = \"b\"\n",
        );
        save_notice_ack(f.path(), true).expect("save");
        let cfg = load_config(f.path());
        assert!(cfg.notice.commercial_ack);
        assert_eq!(cfg.core_endpoint, "ws://example.com/api");
        assert_eq!(cfg.settings.button_layout.as_deref(), Some("b"));
    }

    #[test]
    fn save_notice_ack_can_unset() {
        let f = write_tmp("[notice]\ncommercial_ack = true\n");
        save_notice_ack(f.path(), false).expect("save");
        let cfg = load_config(f.path());
        assert!(!cfg.notice.commercial_ack);
    }

    #[test]
    fn language_auto_is_normalised_to_empty() {
        let f = write_tmp("[general]\nlanguage = \"auto\"\n");
        let cfg = load_config(f.path());
        assert_eq!(cfg.language, "");
    }

    #[test]
    fn language_auto_is_case_insensitive() {
        let f = write_tmp("[general]\nlanguage = \"AUTO\"\n");
        let cfg = load_config(f.path());
        assert_eq!(cfg.language, "");
    }

    #[test]
    fn language_explicit_code_passes_through() {
        let f = write_tmp("[general]\nlanguage = \"ja_JP\"\n");
        let cfg = load_config(f.path());
        assert_eq!(cfg.language, "ja_JP");
    }

    #[test]
    fn keyboard_override_replaces_default_for_that_action() {
        use crate::input_actions::{actions, qt_key_code};

        let toml = r#"
            [input.keyboard]
            accept = ["Space"]
        "#;
        let f = write_tmp(toml);
        let cfg = load_config(f.path());
        let space = qt_key_code("Space").unwrap();
        let enter = qt_key_code("Enter").unwrap();
        assert_eq!(
            cfg.key_to_action.get(&space).map(String::as_str),
            Some(actions::ACCEPT)
        );
        // Enter is no longer bound to accept: the user's list replaced
        // the default for this action entirely.
        assert!(!cfg.key_to_action.contains_key(&enter));
        // Cancel defaults survive untouched.
        let escape = qt_key_code("Escape").unwrap();
        assert_eq!(
            cfg.key_to_action.get(&escape).map(String::as_str),
            Some(actions::CANCEL)
        );
    }

    #[test]
    fn missing_file_returns_defaults() {
        let cfg = load_config(std::path::Path::new("/definitely/does/not/exist.toml"));
        assert_eq!(cfg.video_width, 1920);
    }

    #[test]
    fn video_explicit_tracks_section_presence() {
        // No [video] in file: not explicit.
        let f = write_tmp("[core]\nendpoint = \"ws://x/y\"\n");
        let cfg = load_config(f.path());
        assert!(!cfg.video_explicit);

        // [video] with width set: explicit.
        let f = write_tmp("[video]\nwidth = 384\n");
        let cfg = load_config(f.path());
        assert!(cfg.video_explicit);

        // [video] with height only: still explicit.
        let f = write_tmp("[video]\nheight = 224\n");
        let cfg = load_config(f.path());
        assert!(cfg.video_explicit);
    }

    #[test]
    fn malformed_toml_returns_defaults() {
        let f = write_tmp("this is not = valid toml [[[");
        let cfg = load_config(f.path());
        assert_eq!(cfg.core_endpoint, Config::default().core_endpoint);
    }

    #[test]
    fn partial_config_merges_with_defaults() {
        let f = write_tmp("[video]\nwidth = 1280\n");
        let cfg = load_config(f.path());
        assert_eq!(cfg.video_width, 1280);
        assert_eq!(cfg.video_height, 1080); // default preserved
        assert_eq!(cfg.core_endpoint, Config::default().core_endpoint);
    }

    #[test]
    fn full_config_overrides_all_fields() {
        let toml = r#"
            [general]
            language = "it_IT"

            [core]
            endpoint = "ws://example.com/api"

            [video]
            width = 640
            height = 480

            [logging]
            debug = true

            [settings]
            orientation = "cw"
            clock_format = "12h"
            browse_layout = "list"
            button_layout = "c"
            mouse_enabled = false
        "#;
        let f = write_tmp(toml);
        let cfg = load_config(f.path());
        assert_eq!(cfg.language, "it_IT");
        assert_eq!(cfg.core_endpoint, "ws://example.com/api");
        assert_eq!(cfg.video_width, 640);
        assert_eq!(cfg.video_height, 480);
        assert!(cfg.debug_logging);
        assert_eq!(cfg.settings.orientation.as_deref(), Some("cw"));
        assert_eq!(cfg.settings.clock_format.as_deref(), Some("12h"));
        assert_eq!(cfg.settings.browse_layout.as_deref(), Some("list"));
        assert_eq!(cfg.settings.button_layout.as_deref(), Some("c"));
        assert_eq!(cfg.settings.mouse_enabled, Some(false));
    }

    #[test]
    fn empty_file_returns_defaults() {
        let f = write_tmp("");
        let cfg = load_config(f.path());
        assert_eq!(cfg.video_width, Config::default().video_width);
    }

    #[test]
    fn save_settings_mirror_creates_sections() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("frontend.toml");
        save_settings_mirror(
            &path,
            SettingsMirror {
                resolution: "1280x720",
                language: "it_IT",
                orientation: "cw",
                clock_format: "24h",
                browse_layout: "list",
                button_layout: "b",
                mouse_enabled: false,
                reduce_motion: true,
                discover_arcade_alternate_versions: true,
                debug_logging: true,
                screensaver_timeout: "300",
                media_image_type: "auto",
                show_hidden: true,
                show_original_filenames: true,
                region: "us",
            },
        )
        .expect("save");
        let cfg = load_config(&path);
        assert_eq!(cfg.language, "it_IT");
        assert_eq!(cfg.video_width, 1280);
        assert_eq!(cfg.video_height, 720);
        assert!(cfg.video_explicit);
        assert_eq!(cfg.settings.orientation.as_deref(), Some("cw"));
        assert_eq!(cfg.settings.clock_format.as_deref(), Some("24h"));
        assert_eq!(cfg.settings.browse_layout.as_deref(), Some("list"));
        assert_eq!(cfg.settings.button_layout.as_deref(), Some("b"));
        assert_eq!(cfg.settings.mouse_enabled, Some(false));
        assert_eq!(cfg.settings.reduce_motion, Some(true));
        assert_eq!(cfg.settings.discover_arcade_alternate_versions, Some(true));
        assert_eq!(cfg.settings.screensaver_timeout.as_deref(), Some("300"));
        assert_eq!(cfg.settings.show_hidden, Some(true));
        assert_eq!(cfg.settings.show_original_filenames, Some(true));
        assert_eq!(cfg.settings.region.as_deref(), Some("us"));
        assert!(cfg.debug_logging);
    }

    #[test]
    fn save_settings_mirror_preserves_other_sections() {
        let f = write_tmp(
            "[core]\nendpoint = \"ws://example.com/api\"\n[video]\nbackend = \"native-core-poc\"\nwidth = 1280\nheight = 720\n",
        );
        save_settings_mirror(
            f.path(),
            SettingsMirror {
                resolution: "1280x720",
                language: "en",
                orientation: "horizontal",
                clock_format: "auto",
                browse_layout: "grid",
                button_layout: "a",
                mouse_enabled: true,
                reduce_motion: false,
                discover_arcade_alternate_versions: false,
                debug_logging: false,
                screensaver_timeout: "60",
                media_image_type: "auto",
                show_hidden: false,
                show_original_filenames: false,
                region: "auto",
            },
        )
        .expect("save");
        let written = std::fs::read_to_string(f.path()).expect("read");
        assert!(!written.contains("backend"));
        let cfg = load_config(f.path());
        assert_eq!(cfg.language, "en");
        assert_eq!(cfg.core_endpoint, "ws://example.com/api");
        assert_eq!(cfg.video_width, 1280);
        assert_eq!(cfg.video_height, 720);
        assert_eq!(cfg.settings.orientation.as_deref(), Some("horizontal"));
        assert_eq!(cfg.settings.clock_format.as_deref(), Some("auto"));
        assert_eq!(cfg.settings.browse_layout.as_deref(), Some("grid"));
        assert_eq!(cfg.settings.button_layout.as_deref(), Some("a"));
        assert_eq!(cfg.settings.mouse_enabled, Some(true));
        assert_eq!(cfg.settings.reduce_motion, Some(false));
        assert_eq!(cfg.settings.discover_arcade_alternate_versions, Some(false));
        assert_eq!(cfg.settings.screensaver_timeout.as_deref(), Some("60"));
        assert!(!cfg.debug_logging);
    }

    #[test]
    fn save_settings_mirror_normalizes_auto() {
        let f = write_tmp("");
        save_settings_mirror(
            f.path(),
            SettingsMirror {
                resolution: "",
                language: "",
                orientation: "ccw",
                clock_format: "12h",
                browse_layout: "list",
                button_layout: "c",
                mouse_enabled: false,
                reduce_motion: false,
                discover_arcade_alternate_versions: true,
                debug_logging: true,
                screensaver_timeout: "off",
                media_image_type: "auto",
                show_hidden: false,
                show_original_filenames: false,
                region: "auto",
            },
        )
        .expect("save");
        let written = std::fs::read_to_string(f.path()).expect("read");
        assert!(written.contains("language = \"auto\""));
        assert!(written.contains("orientation = \"ccw\""));
        assert!(written.contains("clock_format = \"12h\""));
        assert!(written.contains("browse_layout = \"list\""));
        assert!(written.contains("button_layout = \"c\""));
        assert!(written.contains("mouse_enabled = false"));
        assert!(written.contains("discover_arcade_alternate_versions = true"));
        assert!(written.contains("screensaver_timeout = \"off\""));
        assert!(written.contains("debug = true"));
        let cfg = load_config(f.path());
        assert_eq!(cfg.language, "");
        assert!(!cfg.video_explicit);
        assert_eq!(cfg.settings.orientation.as_deref(), Some("ccw"));
        assert_eq!(cfg.settings.clock_format.as_deref(), Some("12h"));
        assert_eq!(cfg.settings.browse_layout.as_deref(), Some("list"));
        assert_eq!(cfg.settings.button_layout.as_deref(), Some("c"));
        assert_eq!(cfg.settings.mouse_enabled, Some(false));
        assert_eq!(cfg.settings.reduce_motion, Some(false));
        assert_eq!(cfg.settings.discover_arcade_alternate_versions, Some(true));
        assert_eq!(cfg.settings.screensaver_timeout.as_deref(), Some("off"));
        assert!(cfg.debug_logging);
    }

    // Single test because std::env is process-global; splitting into
    // separate #[test]s would race when nextest runs them in parallel.
    #[test]
    fn env_var_overrides_endpoint_and_empty_string_is_ignored() {
        const KEY: &str = "ZAPAROO_CORE_ENDPOINT";
        let prior = std::env::var(KEY).ok();
        let f = write_tmp("[core]\nendpoint = \"ws://example.com/api\"\n");

        std::env::set_var(KEY, "ws://localhost:27497/api/v0.1");
        assert_eq!(
            load_config(f.path()).core_endpoint,
            "ws://localhost:27497/api/v0.1"
        );

        // Empty value is treated as unset so accidentally exporting an
        // empty ZAPAROO_CORE_ENDPOINT in a shell rc file doesn't blank
        // out the user's frontend.toml.
        std::env::set_var(KEY, "");
        assert_eq!(load_config(f.path()).core_endpoint, "ws://example.com/api");

        // Regression: missing file used to early-return defaults before
        // the env-var override applied, so a first-run invocation like
        // `ZAPAROO_CORE_ENDPOINT=… just run` silently fell back to the
        // localhost default and the frontend sat in CONNECTING forever.
        std::env::set_var(KEY, "ws://10.0.0.115:7497/api/v0.1");
        assert_eq!(
            load_config(std::path::Path::new("/definitely/does/not/exist.toml")).core_endpoint,
            "ws://10.0.0.115:7497/api/v0.1"
        );

        // Same regression on a malformed file — fall through to the env
        // override rather than freezing on the localhost default.
        let bad = write_tmp("this is not = valid toml [[[");
        assert_eq!(
            load_config(bad.path()).core_endpoint,
            "ws://10.0.0.115:7497/api/v0.1"
        );

        match prior {
            Some(v) => std::env::set_var(KEY, v),
            None => std::env::remove_var(KEY),
        }
    }
}
