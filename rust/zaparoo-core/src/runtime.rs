// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// Runtime: what device is the frontend binary running on?
//
// Independent of `platform` (which describes the Zaparoo Core server).
// See docs/architecture.md for the gating rules.

use std::sync::OnceLock;

/// The `MiSTer` rootfs mounts its SD card here; nothing else does.
const MISTER_MARKER: &str = "/media/fat";
const OS_RELEASE: &str = "/etc/os-release";
/// Force a runtime, for developing one target's behavior on another's
/// hardware. Mirrors `ZAPAROO_STATE_FILE`; an unrecognized value is
/// ignored so a stale shell export cannot silently pick the wrong one.
/// Deliberately not `ZAPAROO_RUNTIME`: `cmake/ZaparooRust.cmake` already
/// sets that at build time to pick the `zaparoo_runtime` cfg, and the two
/// answer different questions.
const RUNTIME_OVERRIDE: &str = "ZAPAROO_RUNTIME_OVERRIDE";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Runtime {
    Mister,
    /// A Steam Deck or another `SteamOS` install. Its paths are the desktop
    /// ones; the variant exists because it presents like a console rather
    /// than a desktop, which changes defaults rather than behavior.
    SteamOs,
    Desktop,
}

impl Runtime {
    pub fn is_mister(self) -> bool {
        matches!(self, Self::Mister)
    }

    /// True for every desktop-Linux runtime, `SteamOS` included: they share
    /// the XDG paths, a windowing system, and the desktop rendering
    /// backend. Ask [`Runtime::is_steam_os`] when the difference matters.
    pub fn is_desktop(self) -> bool {
        !self.is_mister()
    }

    pub fn is_steam_os(self) -> bool {
        matches!(self, Self::SteamOs)
    }
}

impl std::fmt::Display for Runtime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Mister => "mister",
            Self::SteamOs => "steamos",
            Self::Desktop => "desktop",
        })
    }
}

pub fn current() -> Runtime {
    static CACHED: OnceLock<Runtime> = OnceLock::new();
    *CACHED.get_or_init(detect)
}

/// The token spelling of each runtime, as [`Display`](std::fmt::Display)
/// writes it, for the override variable.
fn from_token(token: &str) -> Option<Runtime> {
    match token.trim().to_ascii_lowercase().as_str() {
        "mister" => Some(Runtime::Mister),
        "steamos" => Some(Runtime::SteamOs),
        "desktop" => Some(Runtime::Desktop),
        _ => None,
    }
}

/// `SteamOS` names itself in `/etc/os-release`. `ID_LIKE` counts too, so a
/// derivative that declares itself `SteamOS`-like picks the same defaults
/// as the hardware it targets. Values may be quoted and `ID_LIKE` is a
/// space-separated list.
fn os_release_is_steam_os(contents: &str) -> bool {
    contents.lines().any(|line| {
        let Some((key, value)) = line.split_once('=') else {
            return false;
        };
        let value = value.trim().trim_matches(['"', '\'']);
        match key.trim() {
            "ID" => value.eq_ignore_ascii_case("steamos"),
            "ID_LIKE" => value
                .split_whitespace()
                .any(|id| id.eq_ignore_ascii_case("steamos")),
            _ => false,
        }
    })
}

fn detect() -> Runtime {
    if let Ok(token) = std::env::var(RUNTIME_OVERRIDE) {
        match from_token(&token) {
            Some(forced) => {
                tracing::debug!(runtime = %forced, "runtime forced by override");
                return forced;
            }
            None if !token.trim().is_empty() => {
                tracing::warn!(token, "unrecognized {RUNTIME_OVERRIDE}, detecting instead");
            }
            None => {}
        }
    }
    if std::path::Path::new(MISTER_MARKER).exists() {
        return Runtime::Mister;
    }
    if std::fs::read_to_string(OS_RELEASE).is_ok_and(|c| os_release_is_steam_os(&c)) {
        return Runtime::SteamOs;
    }
    Runtime::Desktop
}

#[cfg(test)]
mod tests {
    use super::{current, from_token, os_release_is_steam_os, Runtime};

    const ALL: [Runtime; 3] = [Runtime::Mister, Runtime::SteamOs, Runtime::Desktop];

    #[test]
    fn current_is_stable_across_calls() {
        let a = current();
        let b = current();
        assert_eq!(a, b);
    }

    #[test]
    fn mister_and_desktop_partition_every_runtime() {
        // `SteamOS` is a desktop-Linux runtime: it shares the XDG paths and
        // the windowing system, so it answers the desktop side.
        for r in ALL {
            assert_ne!(r.is_mister(), r.is_desktop());
        }
        assert!(Runtime::SteamOs.is_desktop());
        assert!(Runtime::SteamOs.is_steam_os());
        assert!(!Runtime::Desktop.is_steam_os());
        assert!(!Runtime::Mister.is_steam_os());
    }

    #[test]
    fn display_round_trips_through_the_override_token() {
        assert_eq!(Runtime::Mister.to_string(), "mister");
        assert_eq!(Runtime::SteamOs.to_string(), "steamos");
        assert_eq!(Runtime::Desktop.to_string(), "desktop");
        for r in ALL {
            assert_eq!(from_token(&r.to_string()), Some(r));
        }
        assert_eq!(from_token(" SteamOS "), Some(Runtime::SteamOs));
        // An unrecognized value falls through to detection rather than
        // silently picking a runtime.
        assert_eq!(from_token("steam-deck"), None);
        assert_eq!(from_token(""), None);
    }

    #[test]
    fn os_release_recognizes_steamos_and_its_derivatives() {
        assert!(os_release_is_steam_os(
            "NAME=\"SteamOS\"\nID=steamos\nID_LIKE=arch\n"
        ));
        // A derivative that declares itself `SteamOS`-like counts.
        assert!(os_release_is_steam_os(
            "ID=holoiso\nID_LIKE=\"arch steamos\"\n"
        ));
        assert!(!os_release_is_steam_os("ID=fedora\nID_LIKE=\"\"\n"));
        assert!(!os_release_is_steam_os("ID=arch\n"));
        // A name mentioning Steam is not an ID.
        assert!(!os_release_is_steam_os(
            "NAME=\"Steam Deck Emulator\"\nID=ubuntu\n"
        ));
        assert!(!os_release_is_steam_os(""));
    }
}
