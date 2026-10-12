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
/// Core's smallest thumbnail tier, used to prepare a real loading color
/// before requesting full-size art when browse metadata has no average.
pub const COLOR_PREVIEW_MAX_SIZE: u32 = 32;
/// The Hub can hold this many tiles at its widest shape; a sanity
/// backstop on the manifest, not a real limit.
pub const MAX_HUB_ENTRIES: usize = 21;
/// The browse covers the manifest keeps for a return from a launched
/// game: the page that was on screen, with room for its neighbors.
pub const MAX_BROWSE_ENTRIES: usize = 48;
/// The largest tier a browse cover is requested at (`sizing::snap_cover_tier`).
pub const MAX_BROWSE_COVER_SIZE: u32 = 768;

/// The painted box a cover is prepared for. Slint's software renderer
/// samples bitmaps nearest-neighbor, so a cover is resized to this box
/// once, off the event loop, instead of being shrunk at paint time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Fit {
    pub width: u32,
    pub height: u32,
    /// The surface enlarges a screenshot or title screen smaller than the
    /// box to fill it (`enlarged`). Off for tiles, which keep the decoded
    /// size.
    pub crisp: bool,
}

/// How a capture smaller than its box is enlarged: by `factor` with hard
/// pixel edges, then smoothed over what is left to reach `width` x
/// `height`. Every source pixel stays the same size and sharp at any box.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Enlarged {
    pub factor: u32,
    pub width: u32,
    pub height: u32,
}

impl Fit {
    /// Keep the decoded size: nothing paints this image in a known box.
    pub const SOURCE: Self = Self {
        width: 0,
        height: 0,
        crisp: false,
    };

    pub fn new(width: i32, height: i32) -> Self {
        Self {
            width: u32::try_from(width).unwrap_or(0),
            height: u32::try_from(height).unwrap_or(0),
            crisp: false,
        }
    }

    /// The same box on a surface that enlarges captures.
    #[must_use]
    pub const fn crisp(self) -> Self {
        Self {
            crisp: true,
            ..self
        }
    }

    /// The size a `width` x `height` image is resized to: the largest
    /// size inside the box that keeps its aspect, as `image-fit: contain`
    /// paints it. None when the image already fits (art is never
    /// upscaled here) or there is no box.
    pub fn size_for(self, width: u32, height: u32) -> Option<(u32, u32)> {
        if self.width == 0 || self.height == 0 || width == 0 || height == 0 {
            return None;
        }
        if width <= self.width && height <= self.height {
            return None;
        }
        Some(self.contain(width, height))
    }

    /// The enlargement of a `width` x `height` capture that is smaller
    /// than the box of a `crisp` surface. None on any other surface, when
    /// the image is larger than the box (`size_for` shrinks it) or when
    /// it fills the box already.
    pub fn enlarged(self, width: u32, height: u32) -> Option<Enlarged> {
        if !self.crisp || self.width == 0 || self.height == 0 || width == 0 || height == 0 {
            return None;
        }
        if width > self.width || height > self.height {
            return None;
        }
        let (fw, fh) = self.contain(width, height);
        if (fw, fh) == (width, height) {
            return None;
        }
        Some(Enlarged {
            factor: (fw / width).min(fh / height).max(1),
            width: fw,
            height: fh,
        })
    }

    /// The largest size inside a non-empty box that keeps the aspect of a
    /// non-empty image.
    fn contain(self, width: u32, height: u32) -> (u32, u32) {
        let (bw, bh, w, h) = (
            u64::from(self.width),
            u64::from(self.height),
            u64::from(width),
            u64::from(height),
        );
        // Width-bound when the image is wider than the box, aspect for aspect.
        let (fw, fh) = if w * bh >= h * bw {
            (bw, (h * bw + w / 2) / w)
        } else {
            ((w * bh + h / 2) / h, bh)
        };
        (
            u32::try_from(fw.max(1)).unwrap_or(self.width),
            u32::try_from(fh.max(1)).unwrap_or(self.height),
        )
    }
}

/// Whether an image type is a raw screen capture, shown with its pixels
/// kept sharp. Takes the type as the frontend names it (`screenshot`) or
/// as Core tags it (`property:image-screenshot`).
pub fn is_capture_type(kind: &str) -> bool {
    let kind = kind.strip_prefix("property:").unwrap_or(kind);
    let kind = kind.strip_prefix("image-").unwrap_or(kind);
    matches!(kind, "screenshot" | "titleshot")
}

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

/// Alpha-weighted RGB average of straight RGBA pixels. Sampling is bounded
/// to 64×64 points; tiny Core previews are sampled in full. Transparent art
/// has no color, and malformed buffers must never become a made-up fallback.
pub fn average_cover_color(rgba: &[u8], width: usize, height: usize) -> Option<[u8; 3]> {
    if width == 0 || height == 0 || width.checked_mul(height)?.checked_mul(4)? != rgba.len() {
        return None;
    }
    let mut channels = [0_u64; 3];
    let mut alpha = 0_u64;
    for y in (0..height).step_by(height.div_ceil(64)) {
        for x in (0..width).step_by(width.div_ceil(64)) {
            let at = (y * width + x) * 4;
            let weight = u64::from(rgba[at + 3]);
            alpha += weight;
            for channel in 0..3 {
                channels[channel] += u64::from(rgba[at + channel]) * weight;
            }
        }
    }
    (alpha > 0).then(|| channels.map(|sum| u8::try_from(sum / alpha).unwrap_or(u8::MAX)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_cover_is_fitted_inside_its_box_and_never_upscaled() {
        let fit = Fit::new(100, 120);
        // Portrait box art is bound by the height.
        assert_eq!(fit.size_for(256, 512), Some((60, 120)));
        // Landscape art is bound by the width.
        assert_eq!(fit.size_for(512, 256), Some((100, 50)));
        // The box's own aspect fills it exactly.
        assert_eq!(fit.size_for(200, 240), Some((100, 120)));
        // One side over the box still shrinks.
        assert_eq!(fit.size_for(100, 240), Some((50, 120)));
        assert_eq!(fit.size_for(100, 120), None);
        assert_eq!(fit.size_for(40, 60), None);
        assert_eq!(fit.size_for(2000, 1), Some((100, 1)));
    }

    #[test]
    fn a_capture_is_enlarged_only_on_a_crisp_surface() {
        // 256x224 in a 600x345 box: height-bound to 394x345, one whole
        // multiple fits, and the rest is the smoothed remainder.
        let fit = Fit::new(600, 345).crisp();
        assert_eq!(
            fit.enlarged(256, 224),
            Some(Enlarged {
                factor: 1,
                width: 394,
                height: 345
            })
        );
        // An exact multiple needs no smoothing at all.
        assert_eq!(
            Fit::new(512, 448).crisp().enlarged(256, 224),
            Some(Enlarged {
                factor: 2,
                width: 512,
                height: 448
            })
        );
        assert_eq!(
            Fit::new(900, 700).crisp().enlarged(256, 224),
            Some(Enlarged {
                factor: 3,
                width: 800,
                height: 700
            })
        );
        // A tile box, an image that already fills the box, one that has
        // to shrink and a missing box are all left alone.
        assert_eq!(Fit::new(600, 345).enlarged(256, 224), None);
        assert_eq!(Fit::new(256, 300).crisp().enlarged(256, 224), None);
        assert_eq!(Fit::new(100, 100).crisp().enlarged(256, 224), None);
        assert_eq!(Fit::SOURCE.crisp().enlarged(256, 224), None);
        // Shrinking is the same on either kind of surface.
        assert_eq!(
            Fit::new(100, 120).crisp().size_for(256, 512),
            Fit::new(100, 120).size_for(256, 512)
        );
    }

    #[test]
    fn captures_are_named_by_the_frontend_or_by_core() {
        assert!(is_capture_type("screenshot"));
        assert!(is_capture_type("titleshot"));
        assert!(is_capture_type("property:image-screenshot"));
        assert!(is_capture_type("property:image-titleshot"));
        assert!(!is_capture_type("boxart"));
        assert!(!is_capture_type("property:image-boxart"));
        assert!(!is_capture_type(""));
    }

    #[test]
    fn no_box_keeps_the_decoded_size() {
        assert_eq!(Fit::SOURCE.size_for(512, 512), None);
        assert_eq!(Fit::new(-4, 100).size_for(512, 512), None);
        assert_eq!(Fit::new(100, 100).size_for(0, 10), None);
    }

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
    fn average_colors_are_alpha_weighted_and_never_invented() {
        assert_eq!(
            average_cover_color(&[20, 40, 60, 255, 100, 80, 60, 255], 2, 1),
            Some([60, 60, 60])
        );
        assert_eq!(
            average_cover_color(&[255, 0, 0, 128, 0, 0, 255, 128, 0, 255, 0, 0], 3, 1),
            Some([127, 0, 127])
        );
        assert_eq!(average_cover_color(&[255, 0, 0, 0], 1, 1), None);
        assert_eq!(average_cover_color(&[], 0, 0), None);
        assert_eq!(average_cover_color(&[1, 2, 3], 1, 1), None);
        assert_eq!(average_cover_color(&[], usize::MAX, 2), None);
        let large = [12, 34, 56, 255].repeat(1024 * 768);
        assert_eq!(average_cover_color(&large, 1024, 768), Some([12, 34, 56]));
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
