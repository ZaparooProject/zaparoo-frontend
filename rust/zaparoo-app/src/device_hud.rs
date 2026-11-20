// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

//! The header's device glyphs and the internet gate, read from what Core
//! reports about the device it runs on (`device.status` and
//! `device.changed`), never from the machine drawing the frontend: Core may
//! run somewhere else. Nothing Core leaves out is guessed at.

/// The kind of link carrying the device's default route.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Link {
    Wifi,
    Wired,
    /// Some other link, no link at all, or a kind this frontend does not
    /// know.
    Other,
}

/// How far beyond the local network the device reaches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reach {
    Full,
    /// A sign-in page, nothing, an unknown answer, or no answer yet.
    Limited,
}

/// What Core reported, each part `None` when Core gave no reading.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Reading {
    pub network: Option<(Link, Reach)>,
    /// The device has a Bluetooth adapter.
    pub bluetooth: bool,
    /// The device runs on a battery and Core read its charge.
    pub battery_percent: Option<i64>,
}

/// The network glyph the header shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NetworkGlyph {
    Wifi,
    Wired,
}

/// What the header and the internet gate show.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Hud {
    /// Shown only while the link reaches the internet.
    pub network: Option<NetworkGlyph>,
    /// Online features are offered.
    pub internet: bool,
    pub bluetooth: bool,
    /// `None` hides the gauge.
    pub battery_percent: Option<i32>,
}

pub fn hud(reading: Reading) -> Hud {
    let internet = matches!(reading.network, Some((_, Reach::Full)));
    let network = match reading.network {
        Some((Link::Wifi, Reach::Full)) => Some(NetworkGlyph::Wifi),
        Some((Link::Wired, Reach::Full)) => Some(NetworkGlyph::Wired),
        _ => None,
    };
    Hud {
        network,
        internet,
        bluetooth: reading.bluetooth,
        battery_percent: reading
            .battery_percent
            .map(|p| i32::try_from(p.clamp(0, 100)).unwrap_or(0)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_glyph_needs_the_internet() {
        let reach = |link, reach| {
            hud(Reading {
                network: Some((link, reach)),
                ..Reading::default()
            })
        };
        let wifi = reach(Link::Wifi, Reach::Full);
        assert_eq!(wifi.network, Some(NetworkGlyph::Wifi));
        assert!(wifi.internet);
        assert_eq!(
            reach(Link::Wired, Reach::Full).network,
            Some(NetworkGlyph::Wired)
        );
        let portal = reach(Link::Wifi, Reach::Limited);
        assert_eq!(portal.network, None);
        assert!(!portal.internet);
        let other = reach(Link::Other, Reach::Full);
        assert_eq!(other.network, None, "no glyph for the link");
        assert!(other.internet, "but it is online");
    }

    #[test]
    fn no_reading_shows_nothing() {
        assert_eq!(hud(Reading::default()), Hud::default());
    }

    #[test]
    fn the_battery_is_clamped() {
        let battery = |p| {
            hud(Reading {
                battery_percent: Some(p),
                ..Reading::default()
            })
            .battery_percent
        };
        assert_eq!(battery(62), Some(62));
        assert_eq!(battery(140), Some(100));
        assert_eq!(battery(-3), Some(0));
    }
}
