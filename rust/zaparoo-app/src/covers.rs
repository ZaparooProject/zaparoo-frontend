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

/// Core's own delivery mode name for a path response.
pub const DELIVERY_LOCAL_PATH: &str = "localPath";

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

/// Whether this request may ask for a path instead of the bytes.
/// `disabled` latches once a Core that does not know the mode rejects
/// it, so the whole session stops asking.
pub fn local_path_request_allowed(
    max_size: u32,
    frontend_is_mister: bool,
    core_is_local: bool,
    disabled: bool,
) -> bool {
    max_size > 0 && frontend_is_mister && core_is_local && !disabled
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
    fn the_path_fast_path_needs_a_size_a_mister_and_a_local_core() {
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
}
