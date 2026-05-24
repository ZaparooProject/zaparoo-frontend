// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// Platform: what is the connected Zaparoo Core server running on?
//
// Populated by the `version` RPC after each successful connect. Independent
// of `runtime` (which describes the frontend binary's host).
// See docs/architecture.md for the gating rules.

use crate::client::{Client, ConnectionState};
use crate::media_types::VersionResult;
use std::sync::{Arc, OnceLock};
use tokio::sync::watch;
use tracing::{info, warn};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Platform {
    Mister,
    BatoceraLinux,
    SteamOs,
    Windows,
    Mac,
    Linux,
    Unknown(String),
}

impl Platform {
    pub fn from_api_string(raw: &str) -> Self {
        match raw.trim().to_lowercase().as_str() {
            "mister" => Self::Mister,
            "batocera" | "batocera-linux" | "batoceralinux" => Self::BatoceraLinux,
            "steamos" => Self::SteamOs,
            "windows" | "win" | "win32" => Self::Windows,
            "mac" | "macos" | "darwin" | "osx" => Self::Mac,
            "linux" => Self::Linux,
            "" => Self::Unknown(String::new()),
            _ => Self::Unknown(raw.to_string()),
        }
    }
}

impl std::fmt::Display for Platform {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Mister => f.write_str("mister"),
            Self::BatoceraLinux => f.write_str("batocera-linux"),
            Self::SteamOs => f.write_str("steamos"),
            Self::Windows => f.write_str("windows"),
            Self::Mac => f.write_str("mac"),
            Self::Linux => f.write_str("linux"),
            Self::Unknown(s) if s.is_empty() => f.write_str("unknown"),
            Self::Unknown(s) => write!(f, "unknown({s})"),
        }
    }
}

fn channel() -> &'static watch::Sender<Option<Platform>> {
    static CHANNEL: OnceLock<watch::Sender<Option<Platform>>> = OnceLock::new();
    CHANNEL.get_or_init(|| watch::channel(None).0)
}

/// Subscribe to platform updates. The initial value is `None` until the first
/// `version` RPC after a successful connect completes.
pub fn subscribe() -> watch::Receiver<Option<Platform>> {
    channel().subscribe()
}

/// Snapshot of the last-known platform, or `None` before the first RPC.
pub fn current() -> Option<Platform> {
    channel().borrow().clone()
}

/// Internal: set the platform. Keeps writes funnelled through one place.
fn publish(p: Platform) {
    channel().send_replace(Some(p));
}

/// Spawns a task on `runtime` that issues a `version` RPC each time the
/// client reports a successful connect, parses the response, and publishes
/// the result to [`subscribe`] listeners.
///
/// Failure of the RPC is logged at `warn` and the platform is left at its
/// prior value (or `None`). `systems` and other catalog fetches continue
/// regardless.
pub fn spawn_fetcher(client: Arc<Client>, runtime: &tokio::runtime::Handle) {
    let mut connection_rx = client.connection.subscribe();
    runtime.spawn(async move {
        let mut state = connection_rx.borrow_and_update().clone();
        loop {
            if matches!(state, ConnectionState::Connected) {
                match client.version().await {
                    Ok(VersionResult { version, platform }) => {
                        let parsed = Platform::from_api_string(&platform);
                        info!("core version: {version} platform: {parsed}");
                        publish(parsed);
                    }
                    Err(e) => warn!("version RPC failed: {}", e.message),
                }
            }
            if connection_rx.changed().await.is_err() {
                break;
            }
            state = connection_rx.borrow_and_update().clone();
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

    use super::Platform;

    #[test]
    fn from_api_string_known_values() {
        assert_eq!(Platform::from_api_string("mister"), Platform::Mister);
        assert_eq!(Platform::from_api_string("MiSTer"), Platform::Mister);
        assert_eq!(Platform::from_api_string("linux"), Platform::Linux);
        assert_eq!(Platform::from_api_string("Linux"), Platform::Linux);
        assert_eq!(Platform::from_api_string("windows"), Platform::Windows);
        assert_eq!(Platform::from_api_string("win32"), Platform::Windows);
        assert_eq!(Platform::from_api_string("mac"), Platform::Mac);
        assert_eq!(Platform::from_api_string("darwin"), Platform::Mac);
        assert_eq!(Platform::from_api_string("osx"), Platform::Mac);
        assert_eq!(Platform::from_api_string("steamos"), Platform::SteamOs);
        assert_eq!(
            Platform::from_api_string("batocera"),
            Platform::BatoceraLinux,
        );
        assert_eq!(
            Platform::from_api_string("batocera-linux"),
            Platform::BatoceraLinux,
        );
    }

    #[test]
    fn from_api_string_trims_and_lowercases() {
        assert_eq!(Platform::from_api_string("  LINUX  "), Platform::Linux);
    }

    #[test]
    fn from_api_string_unknown_preserves_original_casing() {
        let p = Platform::from_api_string("Playdate");
        match p {
            Platform::Unknown(s) => assert_eq!(s, "Playdate"),
            other => panic!("expected Unknown(...), got {other:?}"),
        }
    }

    #[test]
    fn from_api_string_empty_becomes_unknown_empty() {
        assert_eq!(
            Platform::from_api_string(""),
            Platform::Unknown(String::new()),
        );
    }

    #[test]
    fn display_formats_known_and_unknown() {
        assert_eq!(Platform::Mister.to_string(), "mister");
        assert_eq!(Platform::BatoceraLinux.to_string(), "batocera-linux");
        assert_eq!(Platform::Unknown(String::new()).to_string(), "unknown");
        assert_eq!(
            Platform::Unknown("custom".into()).to_string(),
            "unknown(custom)",
        );
    }
}
