// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// `Browse.Settings` — gamepad-accessible settings form. The model is the
// seam between the QML form and the persistence/runtime side: it owns
// curated picker lists, remembers what the user picked, and writes
// restart-applied settings back to config/state.
//
// Field design:
//   * `is_mister` — CONSTANT. Drives whether MiSTer-only fields render
//     in the form.
//   * `available_resolutions` — CONSTANT. Empty off MiSTer; on MiSTer,
//     the curated picker list. Order matters: it's the cycle order in
//     the UI's left/right cycler.
//   * `current_resolution` — READ + NOTIFY, persisted. Empty means "use
//     `[mister.video_*]` defaults from frontend.toml". The Settings
//     screen renders that empty value as `qsTr("Default")`.
//   * `available_languages` — CONSTANT. Curated language tags plus the
//     `auto` sentinel. The runtime translator is still startup-only, so
//     this setting applies on the next launch.
//   * `current_language` — READ + NOTIFY. Mirrors `[general].language`
//     from frontend.toml and is also recorded in persisted state so the
//     settings snapshot stays coherent.
//   * `available_orientations` — CONSTANT. Three display transforms:
//     horizontal (default), rotated clockwise, rotated counter-clockwise.
//   * `current_orientation` — READ + NOTIFY, persisted. Applied live by
//     the QML scene wrapper while also mirrored into frontend.toml so
//     MiSTer survives `/tmp` resets.
//   * `available_browse_layouts` — CONSTANT. The browsing layout picker
//     choices. "grid" is the existing layout; "list" is the detailed list
//     placeholder until the new browsing screen is built.
//   * `current_browse_layout` — READ + NOTIFY, persisted. Defaults to
//     "grid" so existing installs keep current behavior.
//   * `available_button_layouts` — CONSTANT. Single-letter ids used to
//     compose resources/images/buttons/<layout>/Button*.png. User-facing
//     labels are "Style A/B/C/D" (see
//     `SettingsScreen.qml::_buttonLayoutDisplay`) so the picker stays a
//     neutral aesthetic choice and avoids implying platform affiliation.
//   * `current_button_layout` — READ + NOTIFY, persisted. Defaults to
//     "a" — the new id for the previous "nintendo" asset directory.
//     `normalize_button_layout` migrates legacy persisted values
//     (`nintendo`/`xbox`/`sony`) to the new ids so users keep their
//     selection across the rename.
//   * `current_mouse_enabled` — READ + NOTIFY, persisted. Defaults to true
//     so existing installs keep the visible cursor and mouse hit targets.
//   * `current_debug_logging` — READ + NOTIFY, persisted. Defaults to false.
//     Toggling it writes `[logging] debug = …` into frontend.toml; the
//     tracing subscriber is built once at startup so the change only takes
//     effect on the next launch (mirrors how `language` works).
// Frontend-owned durable settings are mirrored into both `state.toml`
// and `frontend.toml`. `state.toml` keeps the in-process snapshot
// coherent; `frontend.toml` is the durable copy that survives MiSTer's
// `/tmp` lifecycle and is what startup `vmode` / translator install
// read on the next process launch. Button layout only changes the QML
// resource path used by help-bar icons, browse layout selects the game
// browsing presentation, mouse support drives the QML cursor/input blocker,
// discover-arcade-alternate-versions gates placeholder menu affordances for
// MiSTer arcade alternates, and language still takes effect on the next launch
// because Qt installs translators only at startup.

use crate::models::{with_persist_mut, with_persist_read};
use cxx_qt::{CxxQtType, Initialize};
use cxx_qt_lib::{QString, QStringList};
use std::pin::Pin;
use tracing::warn;
use zaparoo_core::config::{load_config, save_settings_mirror, Config, SettingsMirror};
use zaparoo_core::persist::{self, SettingsState};
use zaparoo_core::platform_paths::config_file_path;
use zaparoo_core::runtime;

/// Curated `MiSTer` resolution choices. Order is the left/right cycle
/// order in the form. Keep the list short — every entry is a literal
/// the user can crash a CRT scaler with if it doesn't suit their
/// monitor — and ASCII-only so the QML side never needs to translate
/// the strings (they're not user-facing labels, they're keys). The
/// empty leading entry is the "use `frontend.toml` defaults" sentinel;
/// the form renders it as `qsTr("Default")` so users can cycle back
/// to no-override after picking a custom value.
const MISTER_RESOLUTIONS: &[&str] = &[
    "",
    "1280x720",
    "1920x1080",
    "1920x1200",
    "1920x1440",
    "640x480",
    "2048x1536",
];
const LANGUAGES: &[&str] = &[
    "auto", "en", "en_US", "en_GB", "it_IT", "es", "es_ES", "eu", "eu_ES", "de", "de_DE", "el",
    "el_GR", "ja", "ja_JP", "ko", "ko_KR", "nl", "nl_NL", "ro", "ro_RO", "sk", "sk_SK", "uk",
    "uk_UA", "zh_CN", "zh_TW", "zh_HK", "he", "he_IL", "ar", "ar_SA", "hi", "hi_IN",
];
const DEFAULT_LANGUAGE: &str = "auto";
const ORIENTATIONS: &[&str] = &["horizontal", "cw", "ccw"];
const DEFAULT_ORIENTATION: &str = "horizontal";
const BROWSE_LAYOUTS: &[&str] = &["grid", "list"];
const DEFAULT_BROWSE_LAYOUT: &str = "grid";
const BUTTON_LAYOUTS: &[&str] = &["a", "b", "c", "d"];
const DEFAULT_BUTTON_LAYOUT: &str = "a";
// Screensaver idle-timeout choices. Values are seconds as ASCII
// strings, with the `"off"` sentinel meaning "never activate".
// Default of 5 minutes matches typical TV/console screensavers and
// is long enough that idle browsing does not trip it.
const SCREENSAVER_TIMEOUTS: &[&str] = &["off", "60", "120", "300", "600", "900", "1800"];
const DEFAULT_SCREENSAVER_TIMEOUT: &str = "300";
const MEDIA_IMAGE_TYPES: &[&str] = &[
    "auto",
    "image",
    "thumbnail",
    "boxart",
    "boxart3d",
    "screenshot",
    "wheel",
    "titleshot",
    "map",
    "marquee",
    "fanart",
    "boxartside",
    "boxartback",
];
const DEFAULT_MEDIA_IMAGE_TYPE: &str = "auto";

// Debug-only QA shortcut so the activation path can be exercised
// without waiting for the production timer. Only appears in debug
// builds; release builds drop both the picker entry and the
// normalization branch so a stray persisted "1" rounds back to the
// safe default.
#[cfg(debug_assertions)]
const SCREENSAVER_TIMEOUTS_DEBUG: &[&str] = &["1"];

#[allow(
    clippy::struct_excessive_bools,
    reason = "settings qobject is a persisted toggle bag exposed to QML"
)]
#[derive(Default)]
pub struct SettingsRust {
    is_mister: bool,
    available_resolutions: QStringList,
    current_resolution: QString,
    available_languages: QStringList,
    current_language: QString,
    available_orientations: QStringList,
    current_orientation: QString,
    available_browse_layouts: QStringList,
    current_browse_layout: QString,
    available_button_layouts: QStringList,
    current_button_layout: QString,
    current_mouse_enabled: bool,
    current_discover_arcade_alternate_versions: bool,
    current_debug_logging: bool,
    available_screensaver_timeouts: QStringList,
    current_screensaver_timeout: QString,
    available_media_image_types: QStringList,
    current_media_image_type: QString,
}

#[cxx_qt::bridge]
pub mod ffi {
    unsafe extern "C++" {
        include!("model_includes.h");

        type QString = cxx_qt_lib::QString;
        type QStringList = cxx_qt_lib::QStringList;
    }

    unsafe extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[qml_singleton]
        #[qproperty(bool, is_mister, READ, CONSTANT)]
        #[qproperty(QStringList, available_resolutions, READ, CONSTANT)]
        #[qproperty(QString, current_resolution, READ, WRITE = set_resolution, NOTIFY)]
        #[qproperty(QStringList, available_languages, READ, CONSTANT)]
        #[qproperty(QString, current_language, READ, WRITE = set_language, NOTIFY)]
        #[qproperty(QStringList, available_orientations, READ, CONSTANT)]
        #[qproperty(QString, current_orientation, READ, WRITE = set_orientation, NOTIFY)]
        #[qproperty(QStringList, available_browse_layouts, READ, CONSTANT)]
        #[qproperty(QString, current_browse_layout, READ, WRITE = set_browse_layout, NOTIFY)]
        #[qproperty(QStringList, available_button_layouts, READ, CONSTANT)]
        #[qproperty(QString, current_button_layout, READ, WRITE = set_button_layout, NOTIFY)]
        #[qproperty(bool, current_mouse_enabled, READ, WRITE = set_mouse_enabled, NOTIFY)]
        #[qproperty(bool, current_discover_arcade_alternate_versions, READ, WRITE = set_discover_arcade_alternate_versions, NOTIFY)]
        #[qproperty(bool, current_debug_logging, READ, WRITE = set_debug_logging, NOTIFY)]
        #[qproperty(QStringList, available_screensaver_timeouts, READ, CONSTANT)]
        #[qproperty(QString, current_screensaver_timeout, READ, WRITE = set_screensaver_timeout, NOTIFY)]
        #[qproperty(QStringList, available_media_image_types, READ, CONSTANT)]
        #[qproperty(QString, current_media_image_type, READ, WRITE = set_media_image_type, NOTIFY)]
        type Settings = super::SettingsRust;

        #[qinvokable]
        fn set_resolution(self: Pin<&mut Settings>, value: QString);

        #[qinvokable]
        fn set_language(self: Pin<&mut Settings>, value: QString);

        #[qinvokable]
        fn set_orientation(self: Pin<&mut Settings>, value: QString);

        #[qinvokable]
        fn set_browse_layout(self: Pin<&mut Settings>, value: QString);

        #[qinvokable]
        fn set_button_layout(self: Pin<&mut Settings>, value: QString);

        #[qinvokable]
        fn set_mouse_enabled(self: Pin<&mut Settings>, value: bool);

        #[qinvokable]
        fn set_discover_arcade_alternate_versions(self: Pin<&mut Settings>, value: bool);

        #[qinvokable]
        fn set_debug_logging(self: Pin<&mut Settings>, value: bool);

        #[qinvokable]
        fn set_screensaver_timeout(self: Pin<&mut Settings>, value: QString);

        #[qinvokable]
        fn set_media_image_type(self: Pin<&mut Settings>, value: QString);
    }

    impl cxx_qt::Initialize for Settings {}
}

impl Initialize for ffi::Settings {
    fn initialize(mut self: Pin<&mut Self>) {
        let started = std::time::Instant::now();
        crate::startup_trace("rust:model Settings init start");
        let snapshot: SettingsState = with_persist_read(|s| s.settings.clone());
        let config_path = config_file_path();
        let is_mister = runtime::current().is_mister();
        let config = load_config(&config_path);
        let merged = merge_settings(&snapshot, &config);
        persist_if_changed(&snapshot, &merged);
        mirror_settings_to_config(&config_path, &merged);
        self.as_mut().rust_mut().is_mister = is_mister;
        self.as_mut().rust_mut().available_resolutions = if is_mister {
            curated_resolutions()
        } else {
            QStringList::default()
        };
        self.as_mut().rust_mut().current_resolution = QString::from(merged.resolution.as_str());
        self.as_mut().rust_mut().available_languages = languages();
        self.as_mut().rust_mut().current_language = QString::from(merged.language.as_str());
        self.as_mut().rust_mut().available_orientations = orientations();
        self.as_mut().rust_mut().current_orientation = QString::from(merged.orientation.as_str());
        self.as_mut().rust_mut().available_browse_layouts = browse_layouts();
        self.as_mut().rust_mut().current_browse_layout =
            QString::from(merged.browse_layout.as_str());
        self.as_mut().rust_mut().available_button_layouts = button_layouts();
        self.as_mut().rust_mut().current_button_layout =
            QString::from(merged.button_layout.as_str());
        self.as_mut().rust_mut().current_mouse_enabled = merged.mouse_enabled;
        self.as_mut()
            .rust_mut()
            .current_discover_arcade_alternate_versions = merged.discover_arcade_alternate_versions;
        self.as_mut().rust_mut().current_debug_logging = merged.debug_logging;
        self.as_mut().rust_mut().available_screensaver_timeouts = screensaver_timeouts();
        self.as_mut().rust_mut().current_screensaver_timeout =
            QString::from(merged.screensaver_timeout.as_str());
        self.as_mut().rust_mut().available_media_image_types = media_image_types();
        self.as_mut().rust_mut().current_media_image_type =
            QString::from(merged.media_image_type.as_str());
        crate::startup_trace(format!(
            "rust:model Settings init end dur_ms={}",
            started.elapsed().as_millis()
        ));
    }
}

impl ffi::Settings {
    fn set_resolution(mut self: Pin<&mut Self>, value: QString) {
        if self.current_resolution == value {
            return;
        }
        let value_str = value.to_string();
        // Resolution is restart-applied, so this setter only updates the
        // durable state/config read by the next frontend process.
        let snapshot = persist_settings(|s| s.resolution.clone_from(&value_str));
        mirror_settings_to_config(&config_file_path(), &snapshot.settings);
        self.as_mut().rust_mut().current_resolution = value;
        self.as_mut().current_resolution_changed();
    }

    #[allow(
        clippy::needless_pass_by_value,
        reason = "cxx-qt qinvokable signature requires QString by value"
    )]
    fn set_language(mut self: Pin<&mut Self>, value: QString) {
        let value_str = normalize_language(&value.to_string()).to_string();
        if self.current_language.to_string() == value_str {
            return;
        }
        let snapshot = persist_settings(|s| s.language.clone_from(&value_str));
        mirror_settings_to_config(&config_file_path(), &snapshot.settings);
        self.as_mut().rust_mut().current_language = QString::from(value_str.as_str());
        self.as_mut().current_language_changed();
    }

    #[allow(
        clippy::needless_pass_by_value,
        reason = "cxx-qt qinvokable signature requires QString by value"
    )]
    fn set_orientation(mut self: Pin<&mut Self>, value: QString) {
        let value_str = normalize_orientation(&value.to_string()).to_string();
        if self.current_orientation.to_string() == value_str {
            return;
        }
        let snapshot = persist_settings(|s| s.orientation.clone_from(&value_str));
        mirror_settings_to_config(&config_file_path(), &snapshot.settings);
        self.as_mut().rust_mut().current_orientation = QString::from(value_str.as_str());
        self.as_mut().current_orientation_changed();
    }

    #[allow(
        clippy::needless_pass_by_value,
        reason = "cxx-qt qinvokable signature requires QString by value"
    )]
    fn set_browse_layout(mut self: Pin<&mut Self>, value: QString) {
        let value_str = normalize_browse_layout(&value.to_string()).to_string();
        if self.current_browse_layout.to_string() == value_str {
            return;
        }
        let snapshot = persist_settings(|s| s.browse_layout.clone_from(&value_str));
        mirror_settings_to_config(&config_file_path(), &snapshot.settings);
        self.as_mut().rust_mut().current_browse_layout = QString::from(value_str.as_str());
        self.as_mut().current_browse_layout_changed();
    }

    #[allow(
        clippy::needless_pass_by_value,
        reason = "cxx-qt qinvokable signature requires QString by value"
    )]
    fn set_button_layout(mut self: Pin<&mut Self>, value: QString) {
        let value_str = normalize_button_layout(&value.to_string()).to_string();
        if self.current_button_layout.to_string() == value_str {
            return;
        }
        let snapshot = persist_settings(|s| s.button_layout.clone_from(&value_str));
        mirror_settings_to_config(&config_file_path(), &snapshot.settings);
        self.as_mut().rust_mut().current_button_layout = QString::from(value_str.as_str());
        self.as_mut().current_button_layout_changed();
    }

    fn set_mouse_enabled(mut self: Pin<&mut Self>, value: bool) {
        if self.current_mouse_enabled == value {
            return;
        }
        let snapshot = persist_settings(|s| s.mouse_enabled = value);
        mirror_settings_to_config(&config_file_path(), &snapshot.settings);
        self.as_mut().rust_mut().current_mouse_enabled = value;
        self.as_mut().current_mouse_enabled_changed();
    }

    fn set_discover_arcade_alternate_versions(mut self: Pin<&mut Self>, value: bool) {
        if self.current_discover_arcade_alternate_versions == value {
            return;
        }
        let snapshot = persist_settings(|s| s.discover_arcade_alternate_versions = value);
        mirror_settings_to_config(&config_file_path(), &snapshot.settings);
        self.as_mut()
            .rust_mut()
            .current_discover_arcade_alternate_versions = value;
        self.as_mut()
            .current_discover_arcade_alternate_versions_changed();
    }

    fn set_debug_logging(mut self: Pin<&mut Self>, value: bool) {
        if self.current_debug_logging == value {
            return;
        }
        let snapshot = persist_settings(|s| s.debug_logging = value);
        mirror_settings_to_config(&config_file_path(), &snapshot.settings);
        self.as_mut().rust_mut().current_debug_logging = value;
        self.as_mut().current_debug_logging_changed();
    }

    #[allow(
        clippy::needless_pass_by_value,
        reason = "cxx-qt qinvokable signature requires QString by value"
    )]
    fn set_screensaver_timeout(mut self: Pin<&mut Self>, value: QString) {
        let value_str = normalize_screensaver_timeout(&value.to_string()).to_string();
        if self.current_screensaver_timeout.to_string() == value_str {
            return;
        }
        let snapshot = persist_settings(|s| s.screensaver_timeout.clone_from(&value_str));
        mirror_settings_to_config(&config_file_path(), &snapshot.settings);
        self.as_mut().rust_mut().current_screensaver_timeout = QString::from(value_str.as_str());
        self.as_mut().current_screensaver_timeout_changed();
    }

    #[allow(
        clippy::needless_pass_by_value,
        reason = "cxx-qt qinvokable signature requires QString by value"
    )]
    fn set_media_image_type(mut self: Pin<&mut Self>, value: QString) {
        let value_str = normalize_media_image_type(&value.to_string()).to_string();
        if self.current_media_image_type.to_string() == value_str {
            return;
        }
        let snapshot = persist_settings(|s| s.media_image_type.clone_from(&value_str));
        mirror_settings_to_config(&config_file_path(), &snapshot.settings);
        self.as_mut().rust_mut().current_media_image_type = QString::from(value_str.as_str());
        self.as_mut().current_media_image_type_changed();
    }
}

fn persist_settings<F: FnOnce(&mut SettingsState)>(mutator: F) -> persist::PersistedState {
    let snapshot = with_persist_mut(|s| {
        mutator(&mut s.settings);
        s.clone()
    });
    persist::save(&snapshot);
    snapshot
}

fn persist_if_changed(current: &SettingsState, merged: &SettingsState) {
    if current == merged {
        return;
    }
    let snapshot = with_persist_mut(|s| {
        s.settings = merged.clone();
        s.clone()
    });
    persist::save(&snapshot);
}

fn mirror_settings_to_config(config_path: &std::path::Path, settings: &SettingsState) {
    if let Err(e) = save_settings_mirror(
        config_path,
        SettingsMirror {
            resolution: settings.resolution.as_str(),
            language: settings.language.as_str(),
            orientation: settings.orientation.as_str(),
            browse_layout: settings.browse_layout.as_str(),
            button_layout: settings.button_layout.as_str(),
            mouse_enabled: settings.mouse_enabled,
            discover_arcade_alternate_versions: settings.discover_arcade_alternate_versions,
            debug_logging: settings.debug_logging,
            screensaver_timeout: settings.screensaver_timeout.as_str(),
            media_image_type: settings.media_image_type.as_str(),
        },
    ) {
        warn!(
            "could not save settings mirror to {}: {e}",
            config_path.display()
        );
    }
}

fn merge_settings(snapshot: &SettingsState, config: &Config) -> SettingsState {
    SettingsState {
        resolution: if config.video_explicit {
            format!("{}x{}", config.video_width, config.video_height)
        } else {
            String::new()
        },
        language: normalize_language(&config.language).to_string(),
        orientation: normalize_orientation(
            config
                .settings
                .orientation
                .as_deref()
                .unwrap_or(snapshot.orientation.as_str()),
        )
        .to_string(),
        browse_layout: normalize_browse_layout(
            config
                .settings
                .browse_layout
                .as_deref()
                .unwrap_or(snapshot.browse_layout.as_str()),
        )
        .to_string(),
        button_layout: normalize_button_layout(
            config
                .settings
                .button_layout
                .as_deref()
                .unwrap_or(snapshot.button_layout.as_str()),
        )
        .to_string(),
        mouse_enabled: config
            .settings
            .mouse_enabled
            .unwrap_or(snapshot.mouse_enabled),
        discover_arcade_alternate_versions: config
            .settings
            .discover_arcade_alternate_versions
            .unwrap_or(snapshot.discover_arcade_alternate_versions),
        // Config wins so frontend.toml is the durable source of truth on
        // MiSTer (state.toml lives on tmpfs).
        debug_logging: config.debug_logging,
        screensaver_timeout: normalize_screensaver_timeout(
            config
                .settings
                .screensaver_timeout
                .as_deref()
                .unwrap_or(snapshot.screensaver_timeout.as_str()),
        )
        .to_string(),
        media_image_type: normalize_media_image_type(
            config
                .settings
                .media_image_type
                .as_deref()
                .unwrap_or(snapshot.media_image_type.as_str()),
        )
        .to_string(),
    }
}

fn curated_resolutions() -> QStringList {
    let mut list = QStringList::default();
    for r in MISTER_RESOLUTIONS {
        list.append(QString::from(*r));
    }
    list
}

fn button_layouts() -> QStringList {
    let mut list = QStringList::default();
    for layout in BUTTON_LAYOUTS {
        list.append(QString::from(*layout));
    }
    list
}

fn browse_layouts() -> QStringList {
    let mut list = QStringList::default();
    for layout in BROWSE_LAYOUTS {
        list.append(QString::from(*layout));
    }
    list
}

fn orientations() -> QStringList {
    let mut list = QStringList::default();
    for orientation in ORIENTATIONS {
        list.append(QString::from(*orientation));
    }
    list
}

fn languages() -> QStringList {
    let mut list = QStringList::default();
    for language in LANGUAGES {
        list.append(QString::from(*language));
    }
    list
}

fn screensaver_timeouts() -> QStringList {
    let mut list = QStringList::default();
    #[cfg(debug_assertions)]
    for value in SCREENSAVER_TIMEOUTS_DEBUG {
        list.append(QString::from(*value));
    }
    for value in SCREENSAVER_TIMEOUTS {
        list.append(QString::from(*value));
    }
    list
}

fn media_image_types() -> QStringList {
    let mut list = QStringList::default();
    for value in MEDIA_IMAGE_TYPES {
        list.append(QString::from(*value));
    }
    list
}

fn normalize_language(value: &str) -> &str {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("auto") {
        return DEFAULT_LANGUAGE;
    }
    LANGUAGES
        .iter()
        .copied()
        .find(|language| *language == trimmed)
        .unwrap_or(DEFAULT_LANGUAGE)
}

fn normalize_orientation(value: &str) -> &'static str {
    let trimmed = value.trim();
    ORIENTATIONS
        .iter()
        .copied()
        .find(|orientation| *orientation == trimmed)
        .unwrap_or(DEFAULT_ORIENTATION)
}

fn normalize_browse_layout(value: &str) -> &'static str {
    let trimmed = value.trim();
    BROWSE_LAYOUTS
        .iter()
        .copied()
        .find(|layout| *layout == trimmed)
        .unwrap_or(DEFAULT_BROWSE_LAYOUT)
}

fn normalize_screensaver_timeout(value: &str) -> &'static str {
    let trimmed = value.trim();
    #[cfg(debug_assertions)]
    if let Some(found) = SCREENSAVER_TIMEOUTS_DEBUG
        .iter()
        .copied()
        .find(|v| *v == trimmed)
    {
        return found;
    }
    SCREENSAVER_TIMEOUTS
        .iter()
        .copied()
        .find(|v| *v == trimmed)
        .unwrap_or(DEFAULT_SCREENSAVER_TIMEOUT)
}

fn normalize_media_image_type(value: &str) -> &'static str {
    let trimmed = value.trim();
    MEDIA_IMAGE_TYPES
        .iter()
        .copied()
        .find(|v| *v == trimmed)
        .unwrap_or(DEFAULT_MEDIA_IMAGE_TYPE)
}

fn normalize_button_layout(value: &str) -> &'static str {
    let trimmed = value.trim();
    // Legacy alias map: state files written by builds before the
    // a/b/c rename hold "nintendo"/"xbox"/"sony"; preserve the user's
    // pick instead of silently snapping back to the default.
    let migrated = match trimmed {
        "nintendo" => "a",
        "xbox" => "b",
        "sony" => "c",
        other => other,
    };
    BUTTON_LAYOUTS
        .iter()
        .copied()
        .find(|layout| *layout == migrated)
        .unwrap_or(DEFAULT_BUTTON_LAYOUT)
}

#[cfg(test)]
mod tests {
    use super::{
        browse_layouts, button_layouts, curated_resolutions, languages, normalize_browse_layout,
        normalize_button_layout, normalize_language, normalize_orientation, orientations,
        BROWSE_LAYOUTS, BUTTON_LAYOUTS, DEFAULT_BROWSE_LAYOUT, DEFAULT_BUTTON_LAYOUT,
        DEFAULT_LANGUAGE, DEFAULT_ORIENTATION, LANGUAGES, MISTER_RESOLUTIONS, ORIENTATIONS,
    };

    #[test]
    fn curated_resolutions_preserves_order() {
        let list = curated_resolutions();
        let collected: Vec<String> = list.iter().map(String::from).collect();
        let expected: Vec<String> = MISTER_RESOLUTIONS
            .iter()
            .map(|s| (*s).to_string())
            .collect();
        assert_eq!(collected, expected);
    }

    #[test]
    fn curated_list_contains_720p_and_1080p() {
        // Mostly a sanity guard — if a future edit silently drops the
        // two most-likely-to-work resolutions, this test catches it.
        let collected: Vec<&str> = MISTER_RESOLUTIONS.to_vec();
        assert!(collected.contains(&"1280x720"));
        assert!(collected.contains(&"1920x1080"));
    }

    #[test]
    fn button_layouts_preserve_order() {
        let list = button_layouts();
        let collected: Vec<String> = list.iter().map(String::from).collect();
        let expected: Vec<String> = BUTTON_LAYOUTS.iter().map(|s| (*s).to_string()).collect();
        assert_eq!(collected, expected);
    }

    #[test]
    fn languages_preserve_order() {
        let list = languages();
        let collected: Vec<String> = list.iter().map(String::from).collect();
        let expected: Vec<String> = LANGUAGES.iter().map(|s| (*s).to_string()).collect();
        assert_eq!(collected, expected);
    }

    #[test]
    fn orientations_preserve_order() {
        let list = orientations();
        let collected: Vec<String> = list.iter().map(String::from).collect();
        let expected: Vec<String> = ORIENTATIONS.iter().map(|s| (*s).to_string()).collect();
        assert_eq!(collected, expected);
    }

    #[test]
    fn browse_layouts_preserve_order() {
        let list = browse_layouts();
        let collected: Vec<String> = list.iter().map(String::from).collect();
        let expected: Vec<String> = BROWSE_LAYOUTS.iter().map(|s| (*s).to_string()).collect();
        assert_eq!(collected, expected);
    }

    #[test]
    fn browse_layout_normalization_defaults_to_grid() {
        assert_eq!(normalize_browse_layout(""), DEFAULT_BROWSE_LAYOUT);
        assert_eq!(normalize_browse_layout("detail"), DEFAULT_BROWSE_LAYOUT);
        assert_eq!(normalize_browse_layout("grid"), "grid");
        assert_eq!(normalize_browse_layout("list"), "list");
    }

    #[test]
    fn orientation_normalization_defaults_to_horizontal() {
        assert_eq!(normalize_orientation(""), DEFAULT_ORIENTATION);
        assert_eq!(normalize_orientation("sideways"), DEFAULT_ORIENTATION);
        assert_eq!(normalize_orientation("horizontal"), "horizontal");
        assert_eq!(normalize_orientation("cw"), "cw");
        assert_eq!(normalize_orientation("ccw"), "ccw");
    }

    #[test]
    fn language_normalization_defaults_to_auto() {
        assert_eq!(normalize_language(""), DEFAULT_LANGUAGE);
        assert_eq!(normalize_language("auto"), DEFAULT_LANGUAGE);
        assert_eq!(normalize_language("AUTO"), DEFAULT_LANGUAGE);
        assert_eq!(normalize_language("fr"), DEFAULT_LANGUAGE);
        assert_eq!(normalize_language("it_IT"), "it_IT");
        assert_eq!(normalize_language("es_ES"), "es_ES");
        assert_eq!(normalize_language("eu_ES"), "eu_ES");
    }

    #[test]
    fn button_layout_values_are_lowercase() {
        for layout in BUTTON_LAYOUTS {
            assert_eq!(*layout, layout.to_ascii_lowercase());
        }
    }

    #[test]
    fn button_layout_normalization_defaults_to_a() {
        assert_eq!(normalize_button_layout(""), DEFAULT_BUTTON_LAYOUT);
        assert_eq!(
            normalize_button_layout("playstation"),
            DEFAULT_BUTTON_LAYOUT
        );
        assert_eq!(normalize_button_layout("b"), "b");
        assert_eq!(normalize_button_layout("d"), "d");
    }

    #[test]
    fn button_layout_migrates_legacy_vendor_ids() {
        assert_eq!(normalize_button_layout("nintendo"), "a");
        assert_eq!(normalize_button_layout("xbox"), "b");
        assert_eq!(normalize_button_layout("sony"), "c");
    }
}
