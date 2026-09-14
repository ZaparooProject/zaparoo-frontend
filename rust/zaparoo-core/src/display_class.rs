// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// Viewing class: how far away is the person from the screen we are
// painting on right now?
//
// This is the input the layout density actually wants, and it is the one
// thing no API reports. Every platform that solves it solves it the same
// way: assume a distance per device class and bake it into the design
// unit. UWP's effective pixels fold density and an assumed distance into
// one unit so a control subtends a constant angle from a phone to a
// Surface Hub; Android TV assumes 3 m and ships a fixed 12-column grid at
// both 1080p and 4K; tvOS designs at 1920x1080 and renders 4K at 2x. None
// of them measure anything.
//
// So we assume too, and write the assumption down. The arithmetic behind
// the two classes, for the hardware this actually runs on:
//
//   Steam Deck panel, 151 mm wide, held at ~400 mm  -> ~21 degrees
//   52 inch TV, 1150 mm wide, seated at ~3000 mm    -> ~22 degrees
//   24 inch monitor, 530 mm wide, at ~600 mm        -> ~48 degrees
//
// Note what that says: a handheld at arm's length and a TV across the
// room subtend nearly the same angle, so angular size alone does not
// separate them, and a desk monitor is far wider than either. The classes
// below are therefore an ergonomic call, not a derivation: a small panel
// held in the hand, often in motion, earns bigger targets than a screen
// that sits still. Recording the numbers is what keeps that call
// falsifiable instead of folklore.
//
// The important half is that the class is a property of the *output*, not
// of the machine. A docked Steam Deck is a lean-back device and an
// undocked one is not, and it is the same binary either way, so this is
// re-read whenever the scene changes rather than cached at startup.
//
// Deliberately not derived from the reported display DPI: inside a
// gamescope session that number is fiction. gamescope hands the game's
// Xwayland a hardcoded 100 mm x 150 mm output whatever is really
// connected, which is exactly how a 1280x800 Deck ended up with a 2.17
// scale factor and a 591x369 logical scene. See `main.rs`'s
// `pin_logical_pixels_to_physical`.

use std::path::Path;

const DRM_CLASS_DIR: &str = "/sys/class/drm";
/// Connector name prefixes for a panel wired into the machine. Anything
/// else is a cable to something the user put there.
const INTERNAL_PREFIXES: [&str; 4] = ["eDP", "LVDS", "DSI", "DPI"];
/// Force a viewing class, for working on one target's layout from
/// another's hardware. Mirrors `ZAPAROO_RUNTIME_OVERRIDE`; an
/// unrecognized value is ignored so a stale export cannot silently pick
/// the wrong one.
const VIEWING_OVERRIDE: &str = "ZAPAROO_VIEWING_OVERRIDE";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Viewing {
    /// A panel held in the hand, roughly an arm's length away.
    Handheld,
    /// Anything the user sits back from: a TV across the room, a monitor
    /// on a desk, a `MiSTer` wired to whatever is in the lounge.
    Seated,
}

impl Viewing {
    pub fn is_handheld(self) -> bool {
        matches!(self, Self::Handheld)
    }
}

impl std::fmt::Display for Viewing {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Handheld => "handheld",
            Self::Seated => "seated",
        })
    }
}

fn from_token(token: &str) -> Option<Viewing> {
    match token.trim().to_ascii_lowercase().as_str() {
        "handheld" => Some(Viewing::Handheld),
        "seated" => Some(Viewing::Seated),
        _ => None,
    }
}

/// True for a connector wired to a panel inside the case.
fn is_internal(connector: &str) -> bool {
    INTERNAL_PREFIXES
        .iter()
        .any(|prefix| connector.starts_with(prefix))
}

/// A DRM connector's `status`/`enabled` pair, as the sysfs files spell
/// them. `enabled` is what separates a plugged-in display from one the
/// compositor is actually driving, which is the question here: a Deck on
/// a dock with the lid screen dark is a lean-back device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Connector<'a> {
    name: &'a str,
    connected: bool,
    enabled: bool,
}

/// Which class a set of connectors implies, for a machine that has a
/// handheld panel. An external display the compositor is driving wins,
/// because that is the screen the user is looking at.
fn class_for(connectors: &[Connector<'_>]) -> Viewing {
    let driving_external = connectors
        .iter()
        .any(|c| !is_internal(c.name) && c.connected && c.enabled);
    if driving_external {
        return Viewing::Seated;
    }
    let driving_internal = connectors
        .iter()
        .any(|c| is_internal(c.name) && c.connected && c.enabled);
    if driving_internal {
        Viewing::Handheld
    } else {
        // Nothing lit that we can see. A handheld with no readable DRM
        // state is still a handheld; guessing lean-back here would give
        // the denser layout to the smaller screen.
        Viewing::Handheld
    }
}

/// The connector name a `/sys/class/drm` entry describes: the directory
/// is `card<N>-<connector>`, and the connector half is what carries the
/// type prefix.
fn connector_name(entry: &str) -> Option<&str> {
    let rest = entry.strip_prefix("card")?;
    let (index, name) = rest.split_once('-')?;
    if index.is_empty() || !index.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    (!name.is_empty()).then_some(name)
}

fn read_flag(dir: &Path, file: &str, want: &str) -> bool {
    std::fs::read_to_string(dir.join(file)).is_ok_and(|value| value.trim() == want)
}

fn probe_drm(root: &Path) -> Vec<(String, bool, bool)> {
    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    let mut found = Vec::new();
    for entry in entries.flatten() {
        let file_name = entry.file_name();
        let Some(raw) = file_name.to_str() else {
            continue;
        };
        let Some(name) = connector_name(raw) else {
            continue;
        };
        let dir = entry.path();
        found.push((
            name.to_string(),
            read_flag(&dir, "status", "connected"),
            read_flag(&dir, "enabled", "enabled"),
        ));
    }
    found
}

/// The class for the screen being painted right now. Re-read rather than
/// cached: docking changes the answer without restarting anything.
pub fn current() -> Viewing {
    if let Ok(token) = std::env::var(VIEWING_OVERRIDE) {
        match from_token(&token) {
            Some(forced) => return forced,
            None if !token.trim().is_empty() => {
                tracing::warn!(token, "unrecognized {VIEWING_OVERRIDE}, detecting instead");
            }
            None => {}
        }
    }
    // Only a machine that *is* a handheld can be in the handheld class.
    // A laptop also drives an internal panel, and a 13 inch screen at
    // desk distance subtends more than twice the angle a Deck does, so
    // treating every built-in panel as handheld would give the roomier
    // layout to the screen that least needs it.
    if !crate::runtime::current().is_steam_os() {
        return Viewing::Seated;
    }
    let probed = probe_drm(Path::new(DRM_CLASS_DIR));
    let connectors: Vec<Connector<'_>> = probed
        .iter()
        .map(|(name, connected, enabled)| Connector {
            name,
            connected: *connected,
            enabled: *enabled,
        })
        .collect();
    let class = class_for(&connectors);
    tracing::debug!(%class, connectors = connectors.len(), "viewing class");
    class
}

#[cfg(test)]
mod tests {
    use super::{class_for, connector_name, from_token, is_internal, Connector, Viewing};

    fn c(name: &str, connected: bool, enabled: bool) -> Connector<'_> {
        Connector {
            name,
            connected,
            enabled,
        }
    }

    #[test]
    fn internal_prefixes_cover_the_panel_connectors() {
        for name in ["eDP-1", "LVDS-1", "DSI-1", "DPI-1"] {
            assert!(is_internal(name), "{name} should be internal");
        }
        for name in ["DP-2", "HDMI-A-1", "DP-1", "Writeback-1"] {
            assert!(!is_internal(name), "{name} should be external");
        }
    }

    #[test]
    fn a_driven_external_display_is_seated() {
        // The docked Steam Deck this was built against: the dock's
        // display is lit and the built-in panel is off.
        let docked = [
            c("DP-1", false, false),
            c("DP-2", true, true),
            c("eDP-1", true, false),
        ];
        assert_eq!(class_for(&docked), Viewing::Seated);
    }

    #[test]
    fn the_built_in_panel_alone_is_handheld() {
        let undocked = [
            c("DP-1", false, false),
            c("DP-2", false, false),
            c("eDP-1", true, true),
        ];
        assert_eq!(class_for(&undocked), Viewing::Handheld);
    }

    #[test]
    fn a_connected_but_unlit_external_does_not_count() {
        // A cable in the dock with the display asleep is not the screen
        // the user is looking at.
        let idle = [c("DP-2", true, false), c("eDP-1", true, true)];
        assert_eq!(class_for(&idle), Viewing::Handheld);
    }

    #[test]
    fn nothing_readable_stays_handheld() {
        assert_eq!(class_for(&[]), Viewing::Handheld);
    }

    #[test]
    fn connector_names_come_from_the_card_prefix() {
        assert_eq!(connector_name("card0-eDP-1"), Some("eDP-1"));
        assert_eq!(connector_name("card1-HDMI-A-2"), Some("HDMI-A-2"));
        assert_eq!(connector_name("card0-Writeback-1"), Some("Writeback-1"));
        // Not a connector directory.
        assert_eq!(connector_name("card0"), None);
        assert_eq!(connector_name("renderD128"), None);
        assert_eq!(connector_name("cardX-eDP-1"), None);
        assert_eq!(connector_name("card0-"), None);
    }

    #[test]
    fn the_override_round_trips_and_rejects_junk() {
        assert_eq!(from_token("handheld"), Some(Viewing::Handheld));
        assert_eq!(from_token(" Seated "), Some(Viewing::Seated));
        assert_eq!(from_token("docked"), None);
        assert_eq!(from_token(""), None);
        for class in [Viewing::Handheld, Viewing::Seated] {
            assert_eq!(from_token(&class.to_string()), Some(class));
        }
    }
}
