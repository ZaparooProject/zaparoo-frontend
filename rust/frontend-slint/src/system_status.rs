// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// Host-local hardware/network status for the header HUD icons, ported
// from the Qt `Browse.SystemStatus` singleton's probing half. NFC is
// NOT probed here - it projects from Core's `readers` endpoint (Core
// owns the reader), see the router's reader refresh.

use std::fs;
use std::net::{SocketAddr, TcpStream};
use std::path::Path;
use std::time::Duration;

const INTERNET_TIMEOUT: Duration = Duration::from_millis(800);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LocalStatus {
    pub has_wifi_internet: bool,
    pub has_lan_internet: bool,
    pub has_bluetooth: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum InterfaceKind {
    Wifi,
    Lan,
}

/// Probe the host. Blocking (reads /proc, /sys, and dials out with an
/// 800 ms cap) - run on a blocking-capable thread, never the UI loop.
pub fn probe() -> LocalStatus {
    let network = default_network_kind()
        .filter(|_| internet_reachable())
        .map_or((false, false), |kind| match kind {
            InterfaceKind::Wifi => (true, false),
            InterfaceKind::Lan => (false, true),
        });
    LocalStatus {
        has_wifi_internet: network.0,
        has_lan_internet: network.1,
        has_bluetooth: bluetooth_adapter_present(),
    }
}

fn default_network_kind() -> Option<InterfaceKind> {
    let routes = fs::read_to_string("/proc/net/route").ok()?;
    let iface = parse_default_route_interface(&routes)?;
    classify_interface(&iface)
}

fn parse_default_route_interface(routes: &str) -> Option<String> {
    routes
        .lines()
        .skip(1)
        .filter_map(|line| {
            let mut fields = line.split_whitespace();
            let iface = fields.next()?;
            let destination = fields.next()?;
            if destination != "00000000" {
                return None;
            }
            fields.next()?;
            let flags = u16::from_str_radix(fields.next()?, 16).ok()?;
            if flags & 0x1 == 0 {
                return None;
            }
            fields.next()?;
            fields.next()?;
            let metric = fields
                .next()
                .and_then(|field| field.parse::<u32>().ok())
                .unwrap_or(u32::MAX);
            Some((metric, iface.to_string()))
        })
        .min_by_key(|(metric, _)| *metric)
        .map(|(_, iface)| iface)
}

fn classify_interface(iface: &str) -> Option<InterfaceKind> {
    if iface == "lo" || !interface_is_up(iface) {
        return None;
    }
    if is_wireless_interface(iface) {
        Some(InterfaceKind::Wifi)
    } else {
        Some(InterfaceKind::Lan)
    }
}

fn interface_is_up(iface: &str) -> bool {
    let path = Path::new("/sys/class/net").join(iface).join("operstate");
    fs::read_to_string(path).is_ok_and(|state| matches!(state.trim(), "up" | "unknown"))
}

fn is_wireless_interface(iface: &str) -> bool {
    Path::new("/sys/class/net")
        .join(iface)
        .join("wireless")
        .exists()
        || iface.starts_with("wl")
}

fn internet_reachable() -> bool {
    [
        SocketAddr::from(([1, 1, 1, 1], 443)),
        SocketAddr::from(([8, 8, 8, 8], 443)),
    ]
    .iter()
    .any(|addr| TcpStream::connect_timeout(addr, INTERNET_TIMEOUT).is_ok())
}

fn bluetooth_adapter_present() -> bool {
    fs::read_dir("/sys/class/bluetooth")
        .ok()
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .any(|entry| entry.file_name().to_string_lossy().starts_with("hci"))
}

#[cfg(test)]
mod tests {
    use super::parse_default_route_interface;

    #[test]
    fn parse_default_route_selects_lowest_metric() {
        let routes = "\
Iface\tDestination\tGateway\tFlags\tRefCnt\tUse\tMetric\tMask\tMTU\tWindow\tIRTT\n\
eth0\t00000000\t0101A8C0\t0003\t0\t0\t200\t00000000\t0\t0\t0\n\
wlan0\t00000000\t0101A8C0\t0003\t0\t0\t100\t00000000\t0\t0\t0\n";
        assert_eq!(
            parse_default_route_interface(routes).as_deref(),
            Some("wlan0"),
        );
    }

    #[test]
    fn parse_default_route_ignores_non_default_routes() {
        let routes = "\
Iface\tDestination\tGateway\tFlags\tRefCnt\tUse\tMetric\tMask\tMTU\tWindow\tIRTT\n\
eth0\t00A8C0A8\t00000000\t0001\t0\t0\t0\t00FFFFFF\t0\t0\t0\n";
        assert_eq!(parse_default_route_interface(routes), None);
    }
}
