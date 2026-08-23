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
//   * `available_resolutions` / `current_resolution` — output-aware MiSTer
//     interface render sizes. Empty means Automatic; concrete choices preserve
//     the active output aspect ratio and stop at 1080p.
//   * `available_languages` — CONSTANT. Curated language tags plus the
//     `auto` sentinel. The runtime translator is still startup-only, so
//     this setting applies on the next launch.
//   * `current_language` — READ + NOTIFY. Mirrors `[general].language`
//     from frontend.toml and is also recorded in persisted state so the
//     settings snapshot stays coherent.
//   * `available_clock_formats` — CONSTANT. Tri-state wall-clock format:
//     `auto` follows the effective UI locale, `12h` and `24h` force an
//     override.
//   * `current_clock_format` — READ + NOTIFY, persisted. Defaults to
//     `auto` so existing installs keep locale-driven behavior.
//   * `available_orientations` — CONSTANT. Three display transforms:
//     horizontal (default), rotated clockwise, rotated counter-clockwise.
//   * `current_orientation` — READ + NOTIFY, persisted. Applied live by
//     the QML scene wrapper while also mirrored into frontend.toml so
//     MiSTer survives `/tmp` resets.
//   * `available_browse_layouts` — CONSTANT. The browsing layout picker
//     choices, shared by both scoped properties below. "grid" is the
//     existing layout; "list" is the detailed list.
//   * `current_systems_browse_layout` / `current_games_browse_layout` —
//     READ + NOTIFY, persisted, each defaulting to "grid". Round 10 split
//     the single `current_browse_layout` into these two so Systems and
//     Games can run different layouts; FavoriteSystemsScreen follows the
//     systems value, Favorites/Recents follow the games value (see
//     MediaListScreen.qml's `layoutScope`). A pre-round-10 install's
//     single `browse_layout` seeds both once — see persist.rs's
//     `migrate_browse_layout` and config.rs's `settings_config_from_raw`.
//   * `current_favorites_grouping` — READ + NOTIFY, persisted. "none"
//     preserves the flat list; "system" groups favorites by system. A string
//     leaves room for additional grouping dimensions without a schema change.
//   * `available_system_logo_styles` — CONSTANT. "tinted" keeps the default
//     theme-colored SVGs; "color" opts into restored full-color logos.
//   * `current_system_logo_style` — READ + NOTIFY, persisted. Defaults to
//     "tinted" so existing installs keep current behavior.
//   * `available_color_schemes` / `current_color_scheme` — curated live color
//     presets. Missing and unknown values normalize to "zaparoo-dark"; the
//     setting is mirrored into state.toml and frontend.toml.
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
//   * `current_reduce_motion` — READ + NOTIFY, persisted. Defaults to false
//     (motion on). When true, all Behavior durations in the UI collapse to
//     0 via Motion.dur() so animations complete in one frame.
//   * `current_debug_logging` — READ + NOTIFY, persisted. Defaults to false.
//     Toggling it writes `[logging] debug = …` into frontend.toml; the
//     tracing subscriber is built once at startup so the change only takes
//     effect on the next launch (mirrors how `language` works).
// Frontend-owned durable settings are mirrored into both `state.toml`
// and `frontend.toml`. `state.toml` keeps the in-process snapshot
// coherent; `frontend.toml` is the durable copy that survives MiSTer's
// `/tmp` lifecycle and is what startup services / translator install
// read on the next process launch. Button layout only changes the QML
// resource path used by help-bar icons, browse layout selects the game
// browsing presentation, mouse support drives the QML cursor/input blocker,
// discover-arcade-alternate-versions gates placeholder menu affordances for
// MiSTer arcade alternates, and language still takes effect on the next launch
// because Qt installs translators only at startup.

use crate::models::action_error::report_action_error;
use crate::models::{with_persist_mut, with_persist_read};
use cxx_qt::{CxxQtType, Initialize, Threading};
use cxx_qt_lib::{QString, QStringList};
use std::pin::Pin;
use tracing::warn;
use zaparoo_core::config::{load_config, save_settings_mirror, Config, SettingsMirror};
use zaparoo_core::persist::{self, SettingsState};
use zaparoo_core::platform_paths::config_file_path;
use zaparoo_core::runtime;

// One picker row per user-visible language. Keep Auto first, then sort by
// the English display labels returned by SettingsScreen._languageDisplay.
// Region-specific tags are accepted below as aliases so old configs keep
// working without creating duplicate labels in the settings modal.
const LANGUAGES: &[&str] = &[
    "auto", "ar", "eu", "zh_CN", "zh_TW", "nl", "en", "fr", "de", "el", "he", "hi", "it_IT", "ja",
    "ko", "ro", "sk", "es", "uk",
];
const LANGUAGE_ALIASES: &[(&str, &str)] = &[
    ("en_US", "en"),
    ("en_GB", "en"),
    ("it", "it_IT"),
    ("es_ES", "es"),
    ("eu_ES", "eu"),
    ("de_DE", "de"),
    ("el_GR", "el"),
    ("ja_JP", "ja"),
    ("ko_KR", "ko"),
    ("nl_NL", "nl"),
    ("ro_RO", "ro"),
    ("sk_SK", "sk"),
    ("uk_UA", "uk"),
    ("zh_Hans", "zh_CN"),
    ("zh_Hans_CN", "zh_CN"),
    ("zh_Hant", "zh_TW"),
    ("zh_Hant_TW", "zh_TW"),
    ("zh_HK", "zh_TW"),
    ("zh_Hant_HK", "zh_TW"),
    ("he_IL", "he"),
    ("ar_SA", "ar"),
    ("hi_IN", "hi"),
    ("fr_FR", "fr"),
];
const DEFAULT_LANGUAGE: &str = "auto";
const CLOCK_FORMATS: &[&str] = &["auto", "12h", "24h"];
const DEFAULT_CLOCK_FORMAT: &str = "auto";
const REGIONS: &[&str] = &["auto", "us", "eu", "jp"];
const DEFAULT_REGION: &str = "auto";
const ORIENTATIONS: &[&str] = &["horizontal", "cw", "ccw"];
const DEFAULT_ORIENTATION: &str = "horizontal";
const BROWSE_LAYOUTS: &[&str] = &["grid", "list"];
const DEFAULT_BROWSE_LAYOUT: &str = "grid";
const FAVORITES_GROUPINGS: &[&str] = &["none", "system"];
const DEFAULT_FAVORITES_GROUPING: &str = "none";
const SYSTEM_LOGO_STYLES: &[&str] = &["tinted", "color"];
const DEFAULT_SYSTEM_LOGO_STYLE: &str = "tinted";
// Kept in the same order as `ColorSchemes.ids` in
// src/ui/theme/ColorSchemes.qml -- see that file's header comment for the
// round-6 prune (24 presets to 11), round-7 regrowth (11 to 19), and
// round-10 reorder into family blocks.
const COLOR_SCHEMES: &[&str] = &[
    "zaparoo-dark",
    "zaparoo-light",
    "classic-purple",
    "amber-phosphor",
    "game-boy",
    "green-phosphor",
    "neo-geo",
    "nes",
    "virtual-boy",
    "dracula",
    "everforest",
    "gruvbox",
    "nord",
    "oxocarbon",
    "rose-pine",
    "solarized-dark",
    "synthwave-84",
    "flexoki-paper",
    "solarized-light",
];
const DEFAULT_COLOR_SCHEME: &str = "zaparoo-dark";
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
    available_clock_formats: QStringList,
    current_clock_format: QString,
    available_orientations: QStringList,
    current_orientation: QString,
    available_browse_layouts: QStringList,
    current_systems_browse_layout: QString,
    current_games_browse_layout: QString,
    current_favorites_grouping: QString,
    available_system_logo_styles: QStringList,
    current_system_logo_style: QString,
    available_color_schemes: QStringList,
    current_color_scheme: QString,
    available_button_layouts: QStringList,
    current_button_layout: QString,
    current_mouse_enabled: bool,
    current_reduce_motion: bool,
    current_debug_logging: bool,
    current_show_hidden: bool,
    current_show_original_filenames: bool,
    available_screensaver_timeouts: QStringList,
    current_screensaver_timeout: QString,
    available_media_image_types: QStringList,
    current_media_image_type: QString,
    available_regions: QStringList,
    current_region: QString,
    /// Flips once, never resets — see `output_resolution_stale`'s
    /// qproperty doc comment.
    output_resolution_stale: bool,
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
        #[qproperty(QStringList, available_clock_formats, READ, CONSTANT)]
        #[qproperty(QString, current_clock_format, READ, WRITE = set_clock_format, NOTIFY)]
        #[qproperty(QStringList, available_orientations, READ, CONSTANT)]
        #[qproperty(QString, current_orientation, READ, WRITE = set_orientation, NOTIFY)]
        #[qproperty(QStringList, available_browse_layouts, READ, CONSTANT)]
        #[qproperty(QString, current_systems_browse_layout, READ, WRITE = set_systems_browse_layout, NOTIFY)]
        #[qproperty(QString, current_games_browse_layout, READ, WRITE = set_games_browse_layout, NOTIFY)]
        #[qproperty(QString, current_favorites_grouping, READ, WRITE = set_favorites_grouping, NOTIFY)]
        #[qproperty(QStringList, available_system_logo_styles, READ, CONSTANT)]
        #[qproperty(QString, current_system_logo_style, READ, WRITE = set_system_logo_style, NOTIFY)]
        #[qproperty(QStringList, available_color_schemes, READ, CONSTANT)]
        #[qproperty(QString, current_color_scheme, READ, WRITE = set_color_scheme, NOTIFY)]
        #[qproperty(QStringList, available_button_layouts, READ, CONSTANT)]
        #[qproperty(QString, current_button_layout, READ, WRITE = set_button_layout, NOTIFY)]
        #[qproperty(bool, current_mouse_enabled, READ, WRITE = set_mouse_enabled, NOTIFY)]
        #[qproperty(bool, current_reduce_motion, READ, WRITE = set_reduce_motion, NOTIFY)]
        #[qproperty(bool, current_debug_logging, READ, WRITE = set_debug_logging, NOTIFY)]
        #[qproperty(bool, current_show_hidden, READ, WRITE = set_show_hidden, NOTIFY)]
        #[qproperty(bool, current_show_original_filenames, READ, WRITE = set_show_original_filenames, NOTIFY)]
        #[qproperty(QStringList, available_screensaver_timeouts, READ, CONSTANT)]
        #[qproperty(QString, current_screensaver_timeout, READ, WRITE = set_screensaver_timeout, NOTIFY)]
        #[qproperty(QStringList, available_media_image_types, READ, CONSTANT)]
        #[qproperty(QString, current_media_image_type, READ, WRITE = set_media_image_type, NOTIFY)]
        #[qproperty(QStringList, available_regions, READ, CONSTANT)]
        #[qproperty(QString, current_region, READ, WRITE = set_region, NOTIFY)]
        // No WRITE — Rust-owned, QML-observed only. `false` for the whole
        // session until a background poll (see
        // `mister_runtime::watch_for_output_change`) confirms the live
        // output timing genuinely changed from what this process resolved
        // at boot (e.g. a TV that was off at launch came on later). Flips
        // to `true` exactly once and never resets; `Main.qml` watches for
        // the change and restarts the process, which re-runs the full
        // verified boot-time probe. `Main.qml`'s own "needs restart"
        // confirm modal (language/resolution/CRT standard) is a separate,
        // user-initiated flow and stays untouched — this is the frontend
        // correcting its own wrong guess, not a setting to approve.
        #[qproperty(bool, output_resolution_stale, READ, NOTIFY)]
        type Settings = super::SettingsRust;

        #[qinvokable]
        fn set_resolution(self: Pin<&mut Settings>, value: QString) -> bool;

        #[qinvokable]
        fn set_language(self: Pin<&mut Settings>, value: QString);

        #[qinvokable]
        fn set_clock_format(self: Pin<&mut Settings>, value: QString);

        #[qinvokable]
        fn set_orientation(self: Pin<&mut Settings>, value: QString);

        #[qinvokable]
        fn set_systems_browse_layout(self: Pin<&mut Settings>, value: QString);

        #[qinvokable]
        fn set_games_browse_layout(self: Pin<&mut Settings>, value: QString);

        #[qinvokable]
        fn set_favorites_grouping(self: Pin<&mut Settings>, value: QString);

        #[qinvokable]
        fn set_system_logo_style(self: Pin<&mut Settings>, value: QString);

        #[qinvokable]
        fn set_color_scheme(self: Pin<&mut Settings>, value: QString);

        #[qinvokable]
        fn set_button_layout(self: Pin<&mut Settings>, value: QString);

        #[qinvokable]
        fn set_mouse_enabled(self: Pin<&mut Settings>, value: bool);

        #[qinvokable]
        fn set_reduce_motion(self: Pin<&mut Settings>, value: bool);

        #[qinvokable]
        fn set_debug_logging(self: Pin<&mut Settings>, value: bool);

        #[qinvokable]
        fn set_screensaver_timeout(self: Pin<&mut Settings>, value: QString);

        #[qinvokable]
        fn set_media_image_type(self: Pin<&mut Settings>, value: QString);

        #[qinvokable]
        fn set_show_hidden(self: Pin<&mut Settings>, value: bool);

        #[qinvokable]
        fn set_show_original_filenames(self: Pin<&mut Settings>, value: bool);

        #[qinvokable]
        fn set_region(self: Pin<&mut Settings>, value: QString);
    }

    impl cxx_qt::Threading for Settings {}
    impl cxx_qt::Initialize for Settings {}
}

impl Initialize for ffi::Settings {
    fn initialize(mut self: Pin<&mut Self>) {
        let started = std::time::Instant::now();
        crate::startup_trace("rust:model Settings init start");
        let snapshot: SettingsState = with_persist_read(|s| s.settings.clone());
        let config_path = config_file_path();
        let is_mister = runtime::current().is_mister();
        let output_size = crate::mister_runtime::digital_output_size();
        let explicit_request_applied = crate::mister_runtime::explicit_request_applied();
        let config = load_config(&config_path);
        let merged = merge_settings(
            &snapshot,
            &config,
            is_mister,
            output_size,
            explicit_request_applied,
        );
        persist_if_changed(&snapshot, &merged);
        mirror_settings_to_config(&config_path, &merged);
        self.as_mut().rust_mut().is_mister = is_mister;
        self.as_mut().rust_mut().available_resolutions = if is_mister {
            curated_resolutions(output_size)
        } else {
            QStringList::default()
        };
        self.as_mut().rust_mut().current_resolution = QString::from(merged.resolution.as_str());
        self.as_mut().rust_mut().available_languages = languages();
        self.as_mut().rust_mut().current_language = QString::from(merged.language.as_str());
        self.as_mut().rust_mut().available_clock_formats = clock_formats();
        self.as_mut().rust_mut().current_clock_format = QString::from(merged.clock_format.as_str());
        self.as_mut().rust_mut().available_orientations = orientations();
        self.as_mut().rust_mut().current_orientation = QString::from(merged.orientation.as_str());
        self.as_mut().rust_mut().available_browse_layouts = browse_layouts();
        self.as_mut().rust_mut().current_systems_browse_layout =
            QString::from(merged.systems_browse_layout.as_str());
        self.as_mut().rust_mut().current_games_browse_layout =
            QString::from(merged.games_browse_layout.as_str());
        self.as_mut().rust_mut().current_favorites_grouping =
            QString::from(merged.favorites_grouping.as_str());
        self.as_mut().rust_mut().available_system_logo_styles = system_logo_styles();
        self.as_mut().rust_mut().current_system_logo_style =
            QString::from(merged.system_logo_style.as_str());
        self.as_mut().rust_mut().available_color_schemes = color_schemes();
        self.as_mut().rust_mut().current_color_scheme = QString::from(merged.color_scheme.as_str());
        self.as_mut().rust_mut().available_button_layouts = button_layouts();
        self.as_mut().rust_mut().current_button_layout =
            QString::from(merged.button_layout.as_str());
        self.as_mut().rust_mut().current_mouse_enabled = merged.mouse_enabled;
        self.as_mut().rust_mut().current_reduce_motion = merged.reduce_motion;
        self.as_mut().rust_mut().current_debug_logging = merged.debug_logging;
        self.as_mut().rust_mut().current_show_hidden = merged.show_hidden;
        self.as_mut().rust_mut().current_show_original_filenames = merged.show_original_filenames;
        self.as_mut().rust_mut().available_screensaver_timeouts = screensaver_timeouts();
        self.as_mut().rust_mut().current_screensaver_timeout =
            QString::from(merged.screensaver_timeout.as_str());
        self.as_mut().rust_mut().available_media_image_types = media_image_types();
        self.as_mut().rust_mut().current_media_image_type =
            QString::from(merged.media_image_type.as_str());
        self.as_mut().rust_mut().available_regions = regions();
        self.as_mut().rust_mut().current_region = QString::from(merged.region.as_str());
        // Explicit runtime guard, not just the async fn's own internal
        // cfg gate: keeps a desktop build from ever spawning this task at
        // all, regardless of what `watch_for_output_change`'s body does
        // off `MiSTer`.
        if is_mister {
            let qt_thread = self.qt_thread();
            crate::models::global_handle().spawn(async move {
                crate::mister_runtime::watch_for_output_change().await;
                let _ = qt_thread.queue(mark_output_resolution_stale);
            });
        }
        crate::startup_trace(format!(
            "rust:model Settings init end dur_ms={}",
            started.elapsed().as_millis()
        ));
    }
}

/// Qt-thread callback queued once `watch_for_output_change` confirms a
/// real, stable output-timing change. Idempotent (the watcher only ever
/// resolves once and this only ever sets `true`), matching every other
/// setter's guard-then-set-then-notify shape.
fn mark_output_resolution_stale(mut model: Pin<&mut ffi::Settings>) {
    if model.output_resolution_stale {
        return;
    }
    model.as_mut().rust_mut().output_resolution_stale = true;
    model.as_mut().output_resolution_stale_changed();
}

impl ffi::Settings {
    #[allow(
        clippy::needless_pass_by_value,
        reason = "cxx-qt qinvokable signature requires QString by value"
    )]
    fn set_resolution(mut self: Pin<&mut Self>, value: QString) -> bool {
        let requested = value.to_string();
        let value_str = if requested.is_empty()
            || self
                .available_resolutions
                .iter()
                .map(String::from)
                .any(|candidate| candidate == requested)
        {
            requested
        } else {
            String::new()
        };
        if self.current_resolution.to_string() == value_str {
            return true;
        }

        // frontend.toml is authoritative across MiSTer's /tmp lifecycle. Write
        // it before changing in-memory/state.toml data so QML can keep the
        // process open when durable persistence fails.
        let mut proposed = with_persist_read(|s| s.settings.clone());
        proposed.resolution.clone_from(&value_str);
        let config_path = config_file_path();
        if let Err(error) = save_settings_to_config(&config_path, &proposed) {
            report_config_mirror_error(&config_path, &error);
            return false;
        }

        persist_settings(|s| s.resolution.clone_from(&value_str));
        self.as_mut().rust_mut().current_resolution = QString::from(value_str.as_str());
        self.as_mut().current_resolution_changed();
        true
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
    fn set_clock_format(mut self: Pin<&mut Self>, value: QString) {
        let value_str = normalize_clock_format(&value.to_string()).to_string();
        if self.current_clock_format.to_string() == value_str {
            return;
        }
        let snapshot = persist_settings(|s| s.clock_format.clone_from(&value_str));
        mirror_settings_to_config(&config_file_path(), &snapshot.settings);
        self.as_mut().rust_mut().current_clock_format = QString::from(value_str.as_str());
        self.as_mut().current_clock_format_changed();
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
    fn set_systems_browse_layout(mut self: Pin<&mut Self>, value: QString) {
        let value_str = normalize_browse_layout(&value.to_string()).to_string();
        if self.current_systems_browse_layout.to_string() == value_str {
            return;
        }
        let snapshot = persist_settings(|s| s.systems_browse_layout.clone_from(&value_str));
        mirror_settings_to_config(&config_file_path(), &snapshot.settings);
        self.as_mut().rust_mut().current_systems_browse_layout = QString::from(value_str.as_str());
        self.as_mut().current_systems_browse_layout_changed();
    }

    #[allow(
        clippy::needless_pass_by_value,
        reason = "cxx-qt qinvokable signature requires QString by value"
    )]
    fn set_games_browse_layout(mut self: Pin<&mut Self>, value: QString) {
        let value_str = normalize_browse_layout(&value.to_string()).to_string();
        if self.current_games_browse_layout.to_string() == value_str {
            return;
        }
        let snapshot = persist_settings(|s| s.games_browse_layout.clone_from(&value_str));
        mirror_settings_to_config(&config_file_path(), &snapshot.settings);
        self.as_mut().rust_mut().current_games_browse_layout = QString::from(value_str.as_str());
        self.as_mut().current_games_browse_layout_changed();
    }

    #[allow(
        clippy::needless_pass_by_value,
        reason = "cxx-qt qinvokable signature requires QString by value"
    )]
    fn set_favorites_grouping(mut self: Pin<&mut Self>, value: QString) {
        let value_str = normalize_favorites_grouping(&value.to_string()).to_string();
        if self.current_favorites_grouping.to_string() == value_str {
            return;
        }
        let snapshot = persist_settings(|s| s.favorites_grouping.clone_from(&value_str));
        mirror_settings_to_config(&config_file_path(), &snapshot.settings);
        self.as_mut().rust_mut().current_favorites_grouping = QString::from(value_str.as_str());
        self.as_mut().current_favorites_grouping_changed();
    }

    #[allow(
        clippy::needless_pass_by_value,
        reason = "cxx-qt qinvokable signature requires QString by value"
    )]
    fn set_system_logo_style(mut self: Pin<&mut Self>, value: QString) {
        let value_str = normalize_system_logo_style(&value.to_string()).to_string();
        if self.current_system_logo_style.to_string() == value_str {
            return;
        }
        let snapshot = persist_settings(|s| s.system_logo_style.clone_from(&value_str));
        mirror_settings_to_config(&config_file_path(), &snapshot.settings);
        self.as_mut().rust_mut().current_system_logo_style = QString::from(value_str.as_str());
        self.as_mut().current_system_logo_style_changed();
    }

    #[allow(
        clippy::needless_pass_by_value,
        reason = "cxx-qt qinvokable signature requires QString by value"
    )]
    fn set_color_scheme(mut self: Pin<&mut Self>, value: QString) {
        let value_str = normalize_color_scheme(&value.to_string()).to_string();
        if self.current_color_scheme.to_string() == value_str {
            return;
        }
        let snapshot = persist_settings(|s| s.color_scheme.clone_from(&value_str));
        mirror_settings_to_config(&config_file_path(), &snapshot.settings);
        self.as_mut().rust_mut().current_color_scheme = QString::from(value_str.as_str());
        self.as_mut().current_color_scheme_changed();
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

    fn set_reduce_motion(mut self: Pin<&mut Self>, value: bool) {
        if self.current_reduce_motion == value {
            return;
        }
        let snapshot = persist_settings(|s| s.reduce_motion = value);
        mirror_settings_to_config(&config_file_path(), &snapshot.settings);
        self.as_mut().rust_mut().current_reduce_motion = value;
        self.as_mut().current_reduce_motion_changed();
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

    fn set_show_hidden(mut self: Pin<&mut Self>, value: bool) {
        if self.current_show_hidden == value {
            return;
        }
        let snapshot = persist_settings(|s| s.show_hidden = value);
        mirror_settings_to_config(&config_file_path(), &snapshot.settings);
        self.as_mut().rust_mut().current_show_hidden = value;
        self.as_mut().current_show_hidden_changed();
    }

    fn set_show_original_filenames(mut self: Pin<&mut Self>, value: bool) {
        if self.current_show_original_filenames == value {
            return;
        }
        let snapshot = persist_settings(|s| s.show_original_filenames = value);
        mirror_settings_to_config(&config_file_path(), &snapshot.settings);
        self.as_mut().rust_mut().current_show_original_filenames = value;
        self.as_mut().current_show_original_filenames_changed();
    }

    #[allow(
        clippy::needless_pass_by_value,
        reason = "cxx-qt qinvokable signature requires QString by value"
    )]
    fn set_region(mut self: Pin<&mut Self>, value: QString) {
        let value_str = normalize_region(&value.to_string()).to_string();
        if self.current_region.to_string() == value_str {
            return;
        }
        let snapshot = persist_settings(|s| s.region.clone_from(&value_str));
        mirror_settings_to_config(&config_file_path(), &snapshot.settings);
        self.as_mut().rust_mut().current_region = QString::from(value_str.as_str());
        self.as_mut().current_region_changed();
    }
}

pub(super) fn persist_settings<F: FnOnce(&mut SettingsState)>(
    mutator: F,
) -> persist::PersistedState {
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

fn save_settings_to_config(
    config_path: &std::path::Path,
    settings: &SettingsState,
) -> Result<(), String> {
    save_settings_mirror(
        config_path,
        SettingsMirror {
            resolution: settings.resolution.as_str(),
            language: settings.language.as_str(),
            orientation: settings.orientation.as_str(),
            clock_format: settings.clock_format.as_str(),
            systems_browse_layout: settings.systems_browse_layout.as_str(),
            games_browse_layout: settings.games_browse_layout.as_str(),
            system_logo_style: settings.system_logo_style.as_str(),
            color_scheme: settings.color_scheme.as_str(),
            button_layout: settings.button_layout.as_str(),
            mouse_enabled: settings.mouse_enabled,
            reduce_motion: settings.reduce_motion,
            debug_logging: settings.debug_logging,
            screensaver_timeout: settings.screensaver_timeout.as_str(),
            media_image_type: settings.media_image_type.as_str(),
            favorites_grouping: settings.favorites_grouping.as_str(),
            show_hidden: settings.show_hidden,
            show_original_filenames: settings.show_original_filenames,
            region: settings.region.as_str(),
            crt_video_standard: settings.crt_video_standard.as_str(),
            crt_h_offset: settings.crt_h_offset,
            crt_v_offset: settings.crt_v_offset,
        },
    )
}

fn report_config_mirror_error(config_path: &std::path::Path, error: &str) {
    warn!(
        "could not save settings mirror to {}: {error}",
        config_path.display()
    );
    report_action_error("setting", "");
}

pub(super) fn mirror_settings_to_config(config_path: &std::path::Path, settings: &SettingsState) {
    if let Err(error) = save_settings_to_config(config_path, settings) {
        report_config_mirror_error(config_path, &error);
    }
}

// Resolve the native-CRT settings (video standard plus clamped centering
// trims) from config-over-snapshot. Split out of `merge_settings` so that
// function stays within the clippy line budget.
fn merge_crt_settings(snapshot: &SettingsState, config: &Config) -> (String, i32, i32) {
    let (h_offset, v_offset) = zaparoo_core::config::clamp_crt_offsets(
        config
            .settings
            .crt_h_offset
            .unwrap_or(snapshot.crt_h_offset),
        config
            .settings
            .crt_v_offset
            .unwrap_or(snapshot.crt_v_offset),
    );
    let standard = zaparoo_core::config::normalize_crt_video_standard(
        config
            .settings
            .crt_video_standard
            .as_deref()
            .unwrap_or(snapshot.crt_video_standard.as_str()),
    )
    .to_string();
    (standard, h_offset, v_offset)
}

#[allow(
    clippy::too_many_lines,
    clippy::cognitive_complexity,
    reason = "merging all settings is inherently verbose"
)]
fn merge_settings(
    snapshot: &SettingsState,
    config: &Config,
    is_mister: bool,
    output_size: Option<(u32, u32)>,
    explicit_request_applied: Option<bool>,
) -> SettingsState {
    let (crt_video_standard, crt_h_offset, crt_v_offset) = merge_crt_settings(snapshot, config);
    let configured_resolution = if config.video_explicit {
        let requested = (config.video_width, config.video_height);
        if !is_mister
            || (explicit_request_applied != Some(false)
                && crate::mister_runtime::configured_render_size_supported(requested, output_size))
        {
            format!("{}x{}", requested.0, requested.1)
        } else {
            String::new()
        }
    } else {
        String::new()
    };
    SettingsState {
        resolution: configured_resolution,
        language: normalize_language(&config.language).to_string(),
        orientation: normalize_orientation(
            config
                .settings
                .orientation
                .as_deref()
                .unwrap_or(snapshot.orientation.as_str()),
        )
        .to_string(),
        clock_format: normalize_clock_format(
            config
                .settings
                .clock_format
                .as_deref()
                .unwrap_or(snapshot.clock_format.as_str()),
        )
        .to_string(),
        // Vestigial -- no reader or writer touches this anymore (see
        // persist.rs's `browse_layout` field doc comment). Carried
        // through unchanged rather than normalized/dropped so the
        // struct's `PartialEq` derive doesn't manufacture a spurious
        // "changed" result against an on-disk snapshot that still has
        // the legacy value from before round 10.
        browse_layout: snapshot.browse_layout.clone(),
        systems_browse_layout: normalize_browse_layout(
            config
                .settings
                .systems_browse_layout
                .as_deref()
                .unwrap_or(snapshot.systems_browse_layout.as_str()),
        )
        .to_string(),
        games_browse_layout: normalize_browse_layout(
            config
                .settings
                .games_browse_layout
                .as_deref()
                .unwrap_or(snapshot.games_browse_layout.as_str()),
        )
        .to_string(),
        favorites_grouping: normalize_favorites_grouping(
            config
                .settings
                .favorites_grouping
                .as_deref()
                .unwrap_or(snapshot.favorites_grouping.as_str()),
        )
        .to_string(),
        system_logo_style: normalize_system_logo_style(
            config
                .settings
                .system_logo_style
                .as_deref()
                .unwrap_or(snapshot.system_logo_style.as_str()),
        )
        .to_string(),
        color_scheme: normalize_color_scheme(
            config
                .settings
                .color_scheme
                .as_deref()
                .unwrap_or(snapshot.color_scheme.as_str()),
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
        reduce_motion: config
            .settings
            .reduce_motion
            .unwrap_or(snapshot.reduce_motion),
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
        show_hidden: config.settings.show_hidden.unwrap_or(snapshot.show_hidden),
        show_original_filenames: config
            .settings
            .show_original_filenames
            .unwrap_or(snapshot.show_original_filenames),
        region: normalize_region(
            config
                .settings
                .region
                .as_deref()
                .unwrap_or(snapshot.region.as_str()),
        )
        .to_string(),
        crt_video_standard,
        crt_h_offset,
        crt_v_offset,
    }
}

fn curated_resolutions(output_size: Option<(u32, u32)>) -> QStringList {
    let mut list = QStringList::default();
    list.append(QString::from(""));
    let sizes = output_size.map_or_else(Vec::new, crate::mister_runtime::selectable_render_sizes);
    for (width, height) in sizes {
        list.append(QString::from(format!("{width}x{height}").as_str()));
    }
    list
}

fn color_schemes() -> QStringList {
    let mut list = QStringList::default();
    for scheme in COLOR_SCHEMES {
        list.append(QString::from(*scheme));
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

fn system_logo_styles() -> QStringList {
    let mut list = QStringList::default();
    for style in SYSTEM_LOGO_STYLES {
        list.append(QString::from(*style));
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

fn clock_formats() -> QStringList {
    let mut list = QStringList::default();
    for format in CLOCK_FORMATS {
        list.append(QString::from(*format));
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

fn regions() -> QStringList {
    let mut list = QStringList::default();
    for region in REGIONS {
        list.append(QString::from(*region));
    }
    list
}

fn normalize_language(value: &str) -> &str {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("auto") {
        return DEFAULT_LANGUAGE;
    }
    if let Some(language) = LANGUAGES
        .iter()
        .copied()
        .find(|language| *language == trimmed)
    {
        return language;
    }
    LANGUAGE_ALIASES
        .iter()
        .find_map(|(alias, language)| (*alias == trimmed).then_some(*language))
        .unwrap_or(DEFAULT_LANGUAGE)
}

fn normalize_clock_format(value: &str) -> &'static str {
    let trimmed = value.trim();
    CLOCK_FORMATS
        .iter()
        .copied()
        .find(|format| *format == trimmed)
        .unwrap_or(DEFAULT_CLOCK_FORMAT)
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

fn normalize_favorites_grouping(value: &str) -> &'static str {
    let trimmed = value.trim();
    FAVORITES_GROUPINGS
        .iter()
        .copied()
        .find(|grouping| *grouping == trimmed)
        .unwrap_or(DEFAULT_FAVORITES_GROUPING)
}

fn normalize_system_logo_style(value: &str) -> &'static str {
    let trimmed = value.trim();
    SYSTEM_LOGO_STYLES
        .iter()
        .copied()
        .find(|style| *style == trimmed)
        .unwrap_or(DEFAULT_SYSTEM_LOGO_STYLE)
}

fn normalize_color_scheme(value: &str) -> &'static str {
    let trimmed = value.trim();
    COLOR_SCHEMES
        .iter()
        .copied()
        .find(|scheme| *scheme == trimmed)
        .unwrap_or(DEFAULT_COLOR_SCHEME)
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

fn normalize_region(value: &str) -> &'static str {
    let trimmed = value.trim();
    REGIONS
        .iter()
        .copied()
        .find(|r| *r == trimmed)
        .unwrap_or(DEFAULT_REGION)
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
        browse_layouts, button_layouts, clock_formats, color_schemes, curated_resolutions,
        languages, normalize_browse_layout, normalize_button_layout, normalize_clock_format,
        normalize_color_scheme, normalize_favorites_grouping, normalize_language,
        normalize_orientation, normalize_region, normalize_system_logo_style, orientations,
        regions, system_logo_styles, BROWSE_LAYOUTS, BUTTON_LAYOUTS, CLOCK_FORMATS, COLOR_SCHEMES,
        DEFAULT_BROWSE_LAYOUT, DEFAULT_BUTTON_LAYOUT, DEFAULT_CLOCK_FORMAT, DEFAULT_COLOR_SCHEME,
        DEFAULT_LANGUAGE, DEFAULT_ORIENTATION, DEFAULT_REGION, DEFAULT_SYSTEM_LOGO_STYLE,
        ORIENTATIONS, REGIONS, SYSTEM_LOGO_STYLES,
    };

    #[test]
    fn curated_resolutions_only_include_integer_fill_choices() {
        let list = curated_resolutions(Some((1920, 1080)));
        let collected: Vec<String> = list.iter().map(String::from).collect();
        assert_eq!(collected, ["", "1920x1080"]);

        let list = curated_resolutions(Some((3840, 2160)));
        let collected: Vec<String> = list.iter().map(String::from).collect();
        assert_eq!(collected, ["", "1280x720", "1920x1080"]);
    }

    #[test]
    fn curated_resolutions_include_native_720p_choice() {
        let list = curated_resolutions(Some((1280, 720)));
        let collected: Vec<String> = list.iter().map(String::from).collect();
        assert_eq!(collected, ["", "1280x720"]);
    }

    #[test]
    fn curated_resolutions_use_only_automatic_when_output_is_unknown() {
        let list = curated_resolutions(None);
        let collected: Vec<String> = list.iter().map(String::from).collect();
        assert_eq!(collected, [""]);
    }

    #[test]
    fn merge_resets_broken_mister_resolution_above_1080p() {
        let snapshot = zaparoo_core::persist::SettingsState::default();
        let mut config = zaparoo_core::config::Config {
            video_width: 2560,
            video_height: 1440,
            video_explicit: true,
            ..zaparoo_core::config::Config::default()
        };
        let merged = super::merge_settings(&snapshot, &config, true, Some((2560, 1440)), None);
        assert!(merged.resolution.is_empty());

        config.video_width = 1920;
        config.video_height = 1080;
        let merged =
            super::merge_settings(&snapshot, &config, true, Some((2560, 1440)), Some(true));
        assert!(merged.resolution.is_empty());

        let merged =
            super::merge_settings(&snapshot, &config, true, Some((3840, 2160)), Some(true));
        assert_eq!(merged.resolution, "1920x1080");

        let merged = super::merge_settings(&snapshot, &config, true, None, Some(true));
        assert!(merged.resolution.is_empty());
    }

    #[test]
    fn merge_resets_explicit_resolution_that_failed_to_apply() {
        let snapshot = zaparoo_core::persist::SettingsState::default();
        let config = zaparoo_core::config::Config {
            video_width: 1920,
            video_height: 1080,
            video_explicit: true,
            ..zaparoo_core::config::Config::default()
        };
        let merged =
            super::merge_settings(&snapshot, &config, true, Some((1920, 1080)), Some(false));
        assert!(merged.resolution.is_empty());
    }

    #[test]
    fn resolution_config_write_failure_is_reported_to_caller() {
        let dir = tempfile::tempdir().expect("temp dir");
        let settings = zaparoo_core::persist::SettingsState {
            resolution: "1920x1080".into(),
            ..zaparoo_core::persist::SettingsState::default()
        };
        let result = super::save_settings_to_config(dir.path(), &settings);
        assert!(result.is_err());
    }

    // Round 10: Systems and Games must resolve independently through the
    // merge -- a config override on one must never leak onto the other,
    // and each falls back to its own snapshot value when config is silent.
    #[test]
    fn merge_resolves_systems_and_games_browse_layout_independently() {
        let snapshot = zaparoo_core::persist::SettingsState {
            systems_browse_layout: "list".into(),
            games_browse_layout: "grid".into(),
            ..zaparoo_core::persist::SettingsState::default()
        };
        let config = zaparoo_core::config::Config {
            settings: zaparoo_core::config::SettingsConfig {
                games_browse_layout: Some("list".into()),
                ..zaparoo_core::config::SettingsConfig::default()
            },
            ..zaparoo_core::config::Config::default()
        };
        let merged = super::merge_settings(&snapshot, &config, false, None, None);
        // Systems has no config override -> falls back to the snapshot.
        assert_eq!(merged.systems_browse_layout, "list");
        // Games has a config override -> config wins over the snapshot's
        // "grid", proving the two fields aren't cross-wired.
        assert_eq!(merged.games_browse_layout, "list");
    }

    #[test]
    fn favorites_grouping_normalizes_known_and_unknown_values() {
        assert_eq!(normalize_favorites_grouping(" none "), "none");
        assert_eq!(normalize_favorites_grouping("system"), "system");
        assert_eq!(normalize_favorites_grouping("genre"), "none");
    }

    #[test]
    fn color_schemes_preserve_order_and_default_unknown_values() {
        let list = color_schemes();
        let collected: Vec<String> = list.iter().map(String::from).collect();
        let expected: Vec<String> = COLOR_SCHEMES.iter().map(|s| (*s).to_string()).collect();
        assert_eq!(collected, expected);
        assert_eq!(normalize_color_scheme("classic-purple"), "classic-purple");
        assert_eq!(normalize_color_scheme("zaparoo-light"), "zaparoo-light");
        assert_eq!(normalize_color_scheme("removed"), DEFAULT_COLOR_SCHEME);
        assert_eq!(normalize_color_scheme(""), "zaparoo-dark");
        // Round-6-pruned ids (still valid round-5 scheme names) must fall
        // back to the default like any other unknown value.
        assert_eq!(
            normalize_color_scheme("midnight-amber"),
            DEFAULT_COLOR_SCHEME
        );
        assert_eq!(
            normalize_color_scheme("zaparoo-black"),
            DEFAULT_COLOR_SCHEME
        );
        assert_eq!(
            normalize_color_scheme("catppuccin-mocha"),
            DEFAULT_COLOR_SCHEME
        );
    }

    #[test]
    fn button_layouts_preserve_order() {
        let list = button_layouts();
        let collected: Vec<String> = list.iter().map(String::from).collect();
        let expected: Vec<String> = BUTTON_LAYOUTS.iter().map(|s| (*s).to_string()).collect();
        assert_eq!(collected, expected);
    }

    #[test]
    fn languages_preserve_picker_order() {
        let list = languages();
        let collected: Vec<String> = list.iter().map(String::from).collect();
        let expected = vec![
            "auto", "ar", "eu", "zh_CN", "zh_TW", "nl", "en", "fr", "de", "el", "he", "hi",
            "it_IT", "ja", "ko", "ro", "sk", "es", "uk",
        ];
        assert_eq!(collected, expected);
    }

    #[test]
    fn clock_formats_preserve_order() {
        let list = clock_formats();
        let collected: Vec<String> = list.iter().map(String::from).collect();
        let expected: Vec<String> = CLOCK_FORMATS.iter().map(|s| (*s).to_string()).collect();
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
    fn system_logo_styles_preserve_order() {
        let list = system_logo_styles();
        let collected: Vec<String> = list.iter().map(String::from).collect();
        let expected: Vec<String> = SYSTEM_LOGO_STYLES
            .iter()
            .map(|s| (*s).to_string())
            .collect();
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
    fn system_logo_style_normalization_defaults_to_tinted() {
        assert_eq!(normalize_system_logo_style(""), DEFAULT_SYSTEM_LOGO_STYLE);
        assert_eq!(
            normalize_system_logo_style("sepia"),
            DEFAULT_SYSTEM_LOGO_STYLE
        );
        assert_eq!(normalize_system_logo_style("tinted"), "tinted");
        assert_eq!(normalize_system_logo_style("color"), "color");
    }

    #[test]
    fn clock_format_normalization_defaults_to_auto() {
        assert_eq!(normalize_clock_format(""), DEFAULT_CLOCK_FORMAT);
        assert_eq!(normalize_clock_format("system"), DEFAULT_CLOCK_FORMAT);
        assert_eq!(normalize_clock_format("auto"), "auto");
        assert_eq!(normalize_clock_format("12h"), "12h");
        assert_eq!(normalize_clock_format("24h"), "24h");
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
        assert_eq!(normalize_language("fr"), "fr");
        assert_eq!(normalize_language("it_IT"), "it_IT");
        assert_eq!(normalize_language("es"), "es");
        assert_eq!(normalize_language("eu"), "eu");
        assert_eq!(normalize_language("zh_TW"), "zh_TW");
    }

    #[test]
    fn language_normalization_migrates_region_aliases() {
        assert_eq!(normalize_language("en_US"), "en");
        assert_eq!(normalize_language("en_GB"), "en");
        assert_eq!(normalize_language("it"), "it_IT");
        assert_eq!(normalize_language("es_ES"), "es");
        assert_eq!(normalize_language("eu_ES"), "eu");
        assert_eq!(normalize_language("de_DE"), "de");
        assert_eq!(normalize_language("el_GR"), "el");
        assert_eq!(normalize_language("ja_JP"), "ja");
        assert_eq!(normalize_language("ko_KR"), "ko");
        assert_eq!(normalize_language("nl_NL"), "nl");
        assert_eq!(normalize_language("ro_RO"), "ro");
        assert_eq!(normalize_language("sk_SK"), "sk");
        assert_eq!(normalize_language("uk_UA"), "uk");
        assert_eq!(normalize_language("zh_Hans_CN"), "zh_CN");
        assert_eq!(normalize_language("zh_HK"), "zh_TW");
        assert_eq!(normalize_language("he_IL"), "he");
        assert_eq!(normalize_language("ar_SA"), "ar");
        assert_eq!(normalize_language("hi_IN"), "hi");
        assert_eq!(normalize_language("fr_FR"), "fr");
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

    #[test]
    fn regions_preserve_order() {
        let list = regions();
        let collected: Vec<String> = list.iter().map(String::from).collect();
        let expected: Vec<String> = REGIONS.iter().map(|s| (*s).to_string()).collect();
        assert_eq!(collected, expected);
    }

    #[test]
    fn region_normalization_defaults_to_auto() {
        assert_eq!(normalize_region(""), DEFAULT_REGION);
        assert_eq!(normalize_region("unknown"), DEFAULT_REGION);
        assert_eq!(normalize_region("  "), DEFAULT_REGION);
        assert_eq!(normalize_region("auto"), "auto");
        assert_eq!(normalize_region("us"), "us");
        assert_eq!(normalize_region("eu"), "eu");
        assert_eq!(normalize_region("jp"), "jp");
    }

    #[test]
    fn region_values_are_lowercase() {
        for region in REGIONS {
            assert_eq!(*region, region.to_ascii_lowercase());
        }
    }
}
