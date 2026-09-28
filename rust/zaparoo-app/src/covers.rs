// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

//! When cover art can be read straight off the disk instead of over the
//! wire, and how big a file that is allowed to be. On a colocated
//! `MiSTer` the thumbnails Core would base64 back are already on the SD
//! card, so Core is asked for the path and the frontend opens it
//! itself. Ported from `media_image_cache.rs` and the cold-boot
//! manifest's own gate.

/// A thumbnail larger than this is not a thumbnail; refuse it rather
/// than read it into a 512 MB machine.
pub const MAX_LOCAL_IMAGE_BYTES: usize = 16 * 1024 * 1024;
/// The size a Hub tile decodes at, and so the largest thumbnail worth
/// asking Core for on that tile's behalf.
pub const HUB_TILE_MAX_SIZE: u32 = 256;
/// The Hub can hold this many tiles at its widest shape; a sanity
/// backstop on the manifest, not a real limit.
pub const MAX_HUB_ENTRIES: usize = 21;

/// The host of a `ws://host:port/path` endpoint, IPv6 brackets and
/// userinfo included.
pub fn endpoint_host(endpoint: &str) -> Option<&str> {
    let after_scheme = endpoint
        .split_once("://")
        .map_or(endpoint, |(_, rest)| rest);
    let authority = after_scheme.split('/').next()?.rsplit('@').next()?;
    if let Some(bracketed) = authority.strip_prefix('[') {
        return bracketed.split_once(']').map(|(host, _)| host);
    }
    authority.split(':').next().filter(|host| !host.is_empty())
}

/// Core is on this machine, so its file paths are ours to open.
pub fn endpoint_is_loopback(endpoint: &str) -> bool {
    let Some(host) = endpoint_host(endpoint) else {
        return false;
    };
    let host = host.trim().trim_matches('.').to_lowercase();
    if host == "localhost" {
        return true;
    }
    host.parse::<std::net::IpAddr>()
        .is_ok_and(|ip| ip.is_loopback())
}

/// Whether this request may ask for a path instead of the bytes: only
/// when this frontend can open the files Core names (a `MiSTer` sharing
/// its SD card, or an embedding host whose Core runs as the same app) and
/// Core is local. `disabled` latches once a Core that does not know the
/// mode rejects it, so the whole session stops asking.
pub fn local_path_request_allowed(
    max_size: u32,
    reads_core_files: bool,
    core_is_local: bool,
    disabled: bool,
) -> bool {
    max_size > 0 && reads_core_files && core_is_local && !disabled
}

/// Core rejected the delivery field itself, rather than failing the
/// request for an ordinary reason. Matched narrowly: a connection error
/// must not disable the fast path for the session.
pub fn is_unsupported_delivery_error(message: &str) -> bool {
    let message = message.to_ascii_lowercase();
    message.contains("delivery")
        && (message.contains("unknown")
            || message.contains("unsupported")
            || message.contains("invalid params"))
}

/// The local-path facts a frontend starts with, before it has a Core
/// link: whether it can open Core's files and whether Core is local. An
/// embedding host always runs Core as this same app over its private
/// socket, so both hold from the first frame, which lets the Hub's
/// cold-boot covers seed before the host hands over the transport.
/// Everywhere else only a colocated `MiSTer` reads Core's files, and
/// locality follows the configured endpoint.
pub fn startup_local_path(hosted: bool, mister: bool, client_is_local: bool) -> (bool, bool) {
    if hosted {
        (true, true)
    } else {
        (mister, client_is_local)
    }
}

/// Core's `coverColor` (`#rrggbb`) as RGB; anything else is no colour.
pub fn parse_cover_color(value: &str) -> Option<[u8; 3]> {
    let hex = value.trim().strip_prefix('#')?;
    if hex.len() != 6 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let channel = |at: usize| u8::from_str_radix(&hex[at..at + 2], 16).ok();
    Some([channel(0)?, channel(2)?, channel(4)?])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loopback_core_endpoints_are_local() {
        for endpoint in [
            "ws://127.0.0.1:7497/api/v0.1",
            "ws://127.12.0.2:7497/api/v0.1",
            "ws://localhost:7497/api/v0.1",
            "ws://[::1]:7497/api/v0.1",
        ] {
            assert!(endpoint_is_loopback(endpoint), "{endpoint}");
        }
    }

    #[test]
    fn remote_core_endpoints_are_not_local() {
        for endpoint in [
            "ws://10.0.0.50:7497/api/v0.1",
            "ws://mister.local:7497/api/v0.1",
            "ws://192.168.1.9:7497/api/v0.1",
            "",
        ] {
            assert!(!endpoint_is_loopback(endpoint), "{endpoint}");
        }
    }

    #[test]
    fn the_path_fast_path_needs_a_size_shared_files_and_a_local_core() {
        assert!(local_path_request_allowed(256, true, true, false));
        assert!(!local_path_request_allowed(0, true, true, false));
        assert!(!local_path_request_allowed(256, false, true, false));
        assert!(!local_path_request_allowed(256, true, false, false));
        assert!(!local_path_request_allowed(256, true, true, true));
    }

    #[test]
    fn unsupported_delivery_errors_are_detected_narrowly() {
        assert!(is_unsupported_delivery_error(
            "invalid params: json: unknown field delivery"
        ));
        assert!(is_unsupported_delivery_error(
            "media.image: unsupported delivery localPath"
        ));
        assert!(!is_unsupported_delivery_error("connection reset by peer"));
        assert!(!is_unsupported_delivery_error("stale media id"));
    }

    #[test]
    fn a_hosted_frontend_starts_with_the_local_path_open() {
        assert_eq!(startup_local_path(true, false, false), (true, true));
        assert_eq!(startup_local_path(false, true, true), (true, true));
        assert_eq!(startup_local_path(false, true, false), (true, false));
        assert_eq!(startup_local_path(false, false, true), (false, true));
    }

    #[test]
    fn cover_colors_parse_only_as_six_digit_hex() {
        assert_eq!(parse_cover_color("#a1B2c3"), Some([0xa1, 0xb2, 0xc3]));
        assert_eq!(parse_cover_color("#000000"), Some([0, 0, 0]));
        assert_eq!(parse_cover_color(" #ffffff "), Some([255, 255, 255]));
        for bad in [
            "",
            "#",
            "a1b2c3",
            "#a1b2c",
            "#a1b2c3d4",
            "#gg0000",
            "#+10000",
            "#é0000",
        ] {
            assert_eq!(parse_cover_color(bad), None, "{bad}");
        }
    }
}
