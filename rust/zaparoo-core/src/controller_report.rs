// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// Controller-type autodetection. `Main_MiSTer`'s launcher integration writes
// `/tmp/zaparoo_launcher_input.json` (atomically, on every input event) with
// the connected controller's glyph profile and the physical face positions
// bound to confirm/cancel. We watch that file and project it into the small
// set of values the help bar needs: which icon style to show (the existing
// neutral `style_a`/`style_b`/`style_c`/`style_d` ids, plus `style_e` for
// keyboard when the active input source is the keyboard) and which face
// glyph stands for accept vs cancel.
//
// IO lives here (in core); the QML side (`models::controller_report`) is a
// late subscriber, so state is published over a `watch` channel that always
// hands out the current value (see AGENTS.md "watch vs broadcast"). The file
// is rewritten on *every* button press on MiSTer, so we only publish when the
// projected glyphs actually change -- the help bar must not churn per press.
//
// Deliberately polling, not inotify: a 300ms `stat()` on MiSTer's tmpfs is
// sub-microsecond, so this avoids a new Cargo dependency and the debounce
// machinery a push-based watcher would need to coalesce the per-press write
// storm. A hot-swapped controller or a mapping change takes effect within one
// poll tick.

use crate::platform_paths::launcher_input_report_path;
use crate::runtime;
use serde::Deserialize;
use std::path::Path;
use std::sync::OnceLock;
use std::time::{Duration, SystemTime};
use tokio::sync::watch;

/// Icon style resolved from `Main_MiSTer`'s `glyph_profile` field, mapped
/// onto this frontend's existing neutral `style_a`/`style_b`/`style_c`/
/// `style_d` ids (see `Browse.Settings.current_button_layout`) rather than
/// the connected controller's brand name -- deliberate, see AGENTS.md's
/// button-layout note. Does not include the keyboard style (`style_e`) --
/// that's driven by a separate axis (`active_source`), not `glyph_profile`;
/// see [`parse_report`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlyphProfile {
    A,
    B,
    C,
    D,
}

impl GlyphProfile {
    // Matches Main_MiSTer's own wire values verbatim -- its `glyph_profile`
    // field is a controller-maker string. That's purely to interoperate with
    // Main's existing report format; nowhere else in this codebase repeats
    // these strings, and the frontend's own ids stay the neutral letters.
    fn from_report_str(raw: &str) -> Self {
        match raw.trim().to_ascii_lowercase().as_str() {
            "nintendo" => Self::A,
            "xbox" => Self::B,
            "playstation" => Self::C,
            // Unknown/missing profiles fall back to the neutral style.
            _ => Self::D,
        }
    }

    /// The `resources/images/buttons/<id>/` directory this style uses.
    fn layout(self) -> &'static str {
        match self {
            Self::A => "style_a",
            Self::B => "style_b",
            Self::C => "style_c",
            Self::D => "style_d",
        }
    }
}

/// Map a physical face position to the positionally-named glyph file. The
/// glyph assets are named by physical position, not printed label:
/// `FaceEast`=east, `FaceSouth`=south, `FaceNorth`=north, `FaceWest`=west.
/// Main can resolve `ok`/`back` to a non-face slot too (a shoulder or
/// start/select, via its own remap) -- that returns `None` here, and the
/// caller falls back to the default face position rather than guess.
fn position_to_face(position: &str) -> Option<&'static str> {
    match position.trim().to_ascii_lowercase().as_str() {
        "east" => Some("FaceEast"),
        "south" => Some("FaceSouth"),
        "north" => Some("FaceNorth"),
        "west" => Some("FaceWest"),
        _ => None,
    }
}

/// Fully-projected glyph selection the help bar consumes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ControllerGlyphs {
    /// Style id directory under `resources/images/buttons/`
    /// (`style_a`/`style_b`/`style_c`/`style_d`, or `style_e` for keyboard).
    pub layout: &'static str,
    /// Positional glyph file for the confirm action (`FaceEast`/`FaceSouth`/...).
    pub accept_button: &'static str,
    /// Positional glyph file for the cancel action.
    pub cancel_button: &'static str,
}

#[derive(Deserialize)]
struct RawReport {
    controller: Option<RawController>,
    actions: Option<RawActions>,
    /// Which input device is currently driving the UI (`controller` or
    /// `keyboard`). Drives the keyboard style below.
    active_source: Option<String>,
}

#[derive(Deserialize)]
struct RawController {
    glyph_profile: Option<String>,
}

#[derive(Deserialize)]
struct RawActions {
    ok: Option<RawAction>,
    back: Option<RawAction>,
}

#[derive(Deserialize)]
struct RawAction {
    position: Option<String>,
}

/// True when the report says the keyboard is the active input source. The
/// keyboard style overrides the controller style in that case (the point of
/// the `active_source` field): a player navigating by keyboard sees keycaps,
/// not pad buttons.
fn is_keyboard_source(raw: &RawReport) -> bool {
    raw.active_source
        .as_deref()
        .is_some_and(|s| s.trim().eq_ignore_ascii_case("keyboard"))
}

/// Parse the subset of the report we care about. Tolerates missing/extra
/// fields: an unparseable file yields `None` (treated as "no report"), and a
/// report missing the `ok`/`back` positions falls back to the default
/// east=accept / south=cancel layout.
fn parse_report(bytes: &[u8]) -> Option<ControllerGlyphs> {
    let raw: RawReport = serde_json::from_slice(bytes).ok()?;
    let keyboard = is_keyboard_source(&raw);
    let layout = if keyboard {
        "style_e"
    } else {
        raw.controller
            .as_ref()
            .and_then(|c| c.glyph_profile.as_deref())
            .map_or(GlyphProfile::D, GlyphProfile::from_report_str)
            .layout()
    };
    // Keyboard keys are fixed (Enter = confirm, Escape = cancel) and must
    // never swap, so force the default face positions regardless of how the
    // connected controller maps ok/back. For a controller, the report's
    // positions drive which face glyph stands for accept vs cancel.
    let (accept_button, cancel_button) = if keyboard {
        ("FaceEast", "FaceSouth")
    } else {
        let position = |action: Option<&RawAction>| {
            action
                .and_then(|a| a.position.as_deref())
                .and_then(position_to_face)
        };
        (
            position(raw.actions.as_ref().and_then(|a| a.ok.as_ref())).unwrap_or("FaceEast"),
            position(raw.actions.as_ref().and_then(|a| a.back.as_ref())).unwrap_or("FaceSouth"),
        )
    };
    Some(ControllerGlyphs {
        layout,
        accept_button,
        cancel_button,
    })
}

fn read_and_parse(path: &Path) -> Option<ControllerGlyphs> {
    let bytes = std::fs::read(path).ok()?;
    parse_report(&bytes)
}

fn channel() -> &'static watch::Sender<Option<ControllerGlyphs>> {
    static CHANNEL: OnceLock<watch::Sender<Option<ControllerGlyphs>>> = OnceLock::new();
    CHANNEL.get_or_init(|| watch::channel(None).0)
}

/// Subscribe to controller-glyph updates. `None` until a report is read.
pub fn subscribe() -> watch::Receiver<Option<ControllerGlyphs>> {
    channel().subscribe()
}

/// Publish only when the projection actually changes, so the help bar's
/// bindings don't re-fire on every keypress-driven file rewrite.
fn publish_if_changed(next: Option<ControllerGlyphs>) {
    channel().send_if_modified(|current| {
        if *current == next {
            false
        } else {
            *current = next;
            true
        }
    });
}

const POLL_INTERVAL: Duration = Duration::from_millis(300);

/// Spawns a dedicated OS thread that polls Main's export file for mtime
/// changes and republishes the resolved glyphs to [`subscribe`] whenever it
/// changes. Seeds the current value synchronously first, so a cold start
/// that lands after the first report still shows the right glyphs.
///
/// The input report is a `MiSTer`-only integration: only `Main_MiSTer` writes
/// `/tmp/zaparoo_launcher_input.json`. Off `MiSTer` the watcher is skipped
/// entirely so desktop builds never poll a file that will never appear --
/// `Browse.ControllerReport` then stays at its no-report fallback. The
/// `ZAPAROO_INPUT_REPORT_FILE` override forces the watcher on regardless, so
/// the feature can be exercised on desktop against a fixture file.
pub fn spawn_watcher() {
    let forced = std::env::var_os("ZAPAROO_INPUT_REPORT_FILE").is_some_and(|v| !v.is_empty());
    if !runtime::current().is_mister() && !forced {
        return;
    }
    let path = launcher_input_report_path();
    publish_if_changed(read_and_parse(&path));
    std::thread::spawn(move || {
        let mut last_mtime: Option<SystemTime> = None;
        loop {
            if let Ok(meta) = std::fs::metadata(&path) {
                let mtime = meta.modified().ok();
                if mtime.is_some() && mtime != last_mtime {
                    last_mtime = mtime;
                    publish_if_changed(read_and_parse(&path));
                }
            }
            std::thread::sleep(POLL_INTERVAL);
        }
    });
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::unwrap_used,
        clippy::panic,
        reason = "tests should fail-fast on unexpected errors"
    )]

    use super::{parse_report, position_to_face, ControllerGlyphs, GlyphProfile};

    #[test]
    fn profile_maps_to_layout_directory() {
        assert_eq!(GlyphProfile::A.layout(), "style_a");
        assert_eq!(GlyphProfile::B.layout(), "style_b");
        assert_eq!(GlyphProfile::C.layout(), "style_c");
        assert_eq!(GlyphProfile::D.layout(), "style_d");
    }

    #[test]
    fn profile_parsing_is_case_insensitive_and_falls_back() {
        assert_eq!(GlyphProfile::from_report_str("XBOX"), GlyphProfile::B);
        assert_eq!(GlyphProfile::from_report_str(" nintendo "), GlyphProfile::A);
        assert_eq!(
            GlyphProfile::from_report_str("PlayStation"),
            GlyphProfile::C
        );
        assert_eq!(GlyphProfile::from_report_str("dreamcast"), GlyphProfile::D);
        assert_eq!(GlyphProfile::from_report_str(""), GlyphProfile::D);
    }

    #[test]
    fn position_maps_to_positional_glyph_file() {
        assert_eq!(position_to_face("east"), Some("FaceEast"));
        assert_eq!(position_to_face("south"), Some("FaceSouth"));
        assert_eq!(position_to_face("NORTH"), Some("FaceNorth"));
        assert_eq!(position_to_face("west"), Some("FaceWest"));
        // Main can resolve ok/back to a shoulder or start/select via its own
        // remap; that's not a face position, so it falls back upstream.
        assert_eq!(position_to_face("left_shoulder"), None);
    }

    // Test-only fixtures below quote Main_MiSTer's actual wire values
    // (its `glyph_profile` field) to exercise the parser against the real
    // report format -- this is not a UI/branding surface.
    fn report(glyph_profile: &str, ok_position: &str, back_position: &str) -> String {
        format!(
            r#"{{
                "version": 1,
                "updated_ms": 0,
                "active_player": 0,
                "active_source": "controller",
                "controller": {{ "glyph_profile": "{glyph_profile}" }},
                "actions": {{
                    "ok":   {{ "position": "{ok_position}" }},
                    "back": {{ "position": "{back_position}" }}
                }}
            }}"#
        )
    }

    #[test]
    fn parses_default_orientation() {
        let json = report("xbox", "east", "south");
        assert_eq!(
            parse_report(json.as_bytes()),
            Some(ControllerGlyphs {
                layout: "style_b",
                accept_button: "FaceEast",
                cancel_button: "FaceSouth",
            })
        );
    }

    #[test]
    fn parses_swapped_ok_cancel() {
        let json = report("nintendo", "south", "east");
        assert_eq!(
            parse_report(json.as_bytes()),
            Some(ControllerGlyphs {
                layout: "style_a",
                accept_button: "FaceSouth",
                cancel_button: "FaceEast",
            })
        );
    }

    #[test]
    fn keyboard_active_source_forces_keyboard_layout_and_fixed_positions() {
        // Even with a controller connected and its ok/back swapped, the
        // keyboard being the active source must win: Enter/Escape never
        // swap.
        let json = r#"{
            "active_source": "keyboard",
            "controller": { "glyph_profile": "xbox" },
            "actions": {
                "ok":   { "position": "south" },
                "back": { "position": "east" }
            }
        }"#;
        assert_eq!(
            parse_report(json.as_bytes()),
            Some(ControllerGlyphs {
                layout: "style_e",
                accept_button: "FaceEast",
                cancel_button: "FaceSouth",
            })
        );
    }

    #[test]
    fn missing_actions_fall_back_to_default_positions() {
        let json = r#"{ "controller": { "glyph_profile": "playstation" } }"#;
        assert_eq!(
            parse_report(json.as_bytes()),
            Some(ControllerGlyphs {
                layout: "style_c",
                accept_button: "FaceEast",
                cancel_button: "FaceSouth",
            })
        );
    }

    #[test]
    fn missing_profile_falls_back_to_neutral_layout() {
        let json = r#"{ "actions": { "ok": { "position": "east" } } }"#;
        assert_eq!(
            parse_report(json.as_bytes()),
            Some(ControllerGlyphs {
                layout: "style_d",
                accept_button: "FaceEast",
                cancel_button: "FaceSouth",
            })
        );
    }

    #[test]
    fn garbage_is_rejected() {
        assert_eq!(parse_report(b"not json"), None);
        assert_eq!(parse_report(b""), None);
    }
}
