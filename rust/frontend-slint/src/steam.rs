// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// Who Steam thinks we are. Steam exports the app id of whatever it
// launched, including a non-Steam shortcut, so a frontend installed as a
// Steam shortcut can recognize itself. Two things need that:
//
// - The compositor claim has to speak Steam's own id rather than the
//   placeholder an unlaunched process uses, or it desynchronizes Steam's
//   focus bookkeeping and gamescope shows nothing at all.
// - Core watches Steam and reports externally started Steam games as
//   active media. Our own shortcut is one of those, so without this the
//   frontend sees itself start and goes dormant behind its own launch.
//
// Empty everywhere Steam is not the parent, which includes `MiSTer` and
// every ordinary desktop run.

use std::sync::OnceLock;

/// A usable app id, or None for the values Steam writes when it has none:
/// unset, empty, or zero. Anything non-numeric is not an id either.
fn parse_app_id(raw: &str) -> Option<&str> {
    let id = raw.trim();
    (!id.is_empty() && id != "0" && id.bytes().all(|b| b.is_ascii_digit())).then_some(id)
}

/// Steam's own id for what it launched. `SteamAppId` is the 32-bit app id;
/// for a non-Steam shortcut it is the generated one in `shortcuts.vdf`.
fn app_id() -> Option<&'static str> {
    static CACHED: OnceLock<Option<String>> = OnceLock::new();
    CACHED
        .get_or_init(|| {
            let raw = std::env::var("SteamAppId").ok()?;
            parse_app_id(&raw).map(ToString::to_string)
        })
        .as_deref()
}

/// The value to write into the gamescope focus properties. Steam's id when
/// Steam launched us, so its bookkeeping and ours agree; otherwise the
/// placeholder an external process claims with, which cannot collide with
/// a real Steam app id. Only the desktop build has a compositor to claim.
#[cfg(feature = "desktop")]
pub fn focus_app_id() -> &'static str {
    app_id().unwrap_or(EXTERNAL_APP_ID)
}

/// The id an app Steam did not launch claims for itself.
#[cfg(feature = "desktop")]
const EXTERNAL_APP_ID: &str = "1";

/// True when Core's active-media path is this very process: our own Steam
/// shortcut, seen by Core's Steam watcher. Core reports these as
/// `steam://<app id>`.
pub fn is_self_media(media_path: &str) -> bool {
    app_id().is_some_and(|id| media_path_is(media_path, id))
}

fn media_path_is(media_path: &str, app_id: &str) -> bool {
    media_path
        .trim()
        .strip_prefix("steam://")
        .is_some_and(|path| path == app_id)
}

#[cfg(test)]
mod tests {
    use super::{is_self_media, media_path_is, parse_app_id};

    #[test]
    fn only_a_real_id_counts_as_a_steam_launch() {
        assert_eq!(parse_app_id("3574362381"), Some("3574362381"));
        assert_eq!(parse_app_id("  480 "), Some("480"));
        // Steam writes these when it has no id for what it launched.
        assert_eq!(parse_app_id(""), None);
        assert_eq!(parse_app_id("0"), None);
        assert_eq!(parse_app_id("not-an-id"), None);
    }

    #[test]
    fn self_media_is_our_own_shortcut_and_nothing_else() {
        assert!(media_path_is("steam://3574362381", "3574362381"));
        assert!(media_path_is(" steam://3574362381 ", "3574362381"));
        // A different Steam game launched from our shortcut still counts
        // as a game, which is the whole point of the filter.
        assert!(!media_path_is("steam://1942280", "3574362381"));
        // A prefix match is not a match.
        assert!(!media_path_is("steam://35743623810", "3574362381"));
        assert!(!media_path_is(
            "/home/deck/ROMs/Atari2600/Adventure.a26",
            "3574362381"
        ));
        assert!(!media_path_is("", "3574362381"));
    }

    #[test]
    fn without_a_steam_launch_nothing_is_self() {
        // The tests never run under Steam, so `SteamAppId` is unset and
        // every path has to belong to someone else.
        assert!(!is_self_media("steam://3574362381"));
    }
}
