// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// QR code for the zaparoo.app write deep-link (QrCodeModal.qml's
// role). The matrix renders at module resolution - one pixel per
// module plus the standard 4-module quiet zone baked in white - and
// the UI scales it to the largest whole multiple with pixelated
// rendering, so modules stay square at every output size.

use qrcode::{Color, QrCode};

/// Quiet-zone width in modules on each side (QR spec minimum).
pub const QUIET_ZONE: u32 = 4;

/// The zaparoo.app deep-link the scanning device opens; the web app
/// hands the zapscript back to a Core/frontend pairing.
pub fn write_url(zapscript: &str) -> String {
    format!("https://zaparoo.app/write?v={}", percent_encode(zapscript))
}

/// Render `content` as a QR image at module resolution. Returns the
/// image and its side length in modules (matrix + both quiet zones).
pub fn qr_image(content: &str) -> Option<(slint::Image, u32)> {
    let code = match QrCode::new(content.as_bytes()) {
        Ok(code) => code,
        Err(e) => {
            tracing::warn!("failed to generate QR code: {e}");
            return None;
        }
    };
    let width = u32::try_from(code.width()).ok()?;
    let colors = code.to_colors();
    let total = width + 2 * QUIET_ZONE;
    let mut buf = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::new(total, total);
    let stride = total as usize;
    let pixels = buf.make_mut_slice();
    for px in pixels.iter_mut() {
        *px = slint::Rgba8Pixel {
            r: 255,
            g: 255,
            b: 255,
            a: 255,
        };
    }
    for y in 0..width as usize {
        for x in 0..width as usize {
            if colors[y * width as usize + x] == Color::Dark {
                let ty = y + QUIET_ZONE as usize;
                let tx = x + QUIET_ZONE as usize;
                pixels[ty * stride + tx] = slint::Rgba8Pixel {
                    r: 0,
                    g: 0,
                    b: 0,
                    a: 255,
                };
            }
        }
    }
    Some((slint::Image::from_rgba8(buf), total))
}

/// `encodeURIComponent` semantics: alphanumerics and `-_.!~*'()` pass
/// through, everything else is UTF-8 percent-encoded. The QML modal
/// built the payload with the JS builtin; the port must produce the
/// same URL for the same zapscript.
fn percent_encode(s: &str) -> String {
    use std::fmt::Write as _;
    let mut out = String::with_capacity(s.len() * 3);
    for byte in s.bytes() {
        match byte {
            b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'-'
            | b'_'
            | b'.'
            | b'!'
            | b'~'
            | b'*'
            | b'\''
            | b'('
            | b')' => out.push(byte as char),
            _ => {
                let _ = write!(out, "%{byte:02X}");
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_matches_encode_uri_component() {
        assert_eq!(percent_encode("abc-XYZ_1.2~*'()!"), "abc-XYZ_1.2~*'()!");
        assert_eq!(percent_encode("a b/c?d&e=f"), "a%20b%2Fc%3Fd%26e%3Df");
        assert_eq!(percent_encode("日"), "%E6%97%A5");
    }

    #[test]
    fn write_url_wraps_payload() {
        assert_eq!(
            write_url("**launch.system:snes"),
            "https://zaparoo.app/write?v=**launch.system%3Asnes"
        );
    }

    #[test]
    fn qr_image_has_quiet_zone_and_modules() {
        // Version-1 QR is 21 modules; any content fits in >= 21.
        // map_or(0, ..) makes a failed generation fail the assert too.
        let total = qr_image("https://zaparoo.app/write?v=test").map_or(0, |(_, t)| t);
        assert!(total >= 21 + 2 * QUIET_ZONE);
    }
}
