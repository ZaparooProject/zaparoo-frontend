// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// Host-local hardware/network status for the header HUD icons. NFC is
// NOT probed here: Core owns the reader, so the HUD's NFC state comes
// from Core's `readers` RPC (see the router's reader refresh).

#[cfg(not(feature = "hosted"))]
use std::fs;
#[cfg(not(feature = "hosted"))]
use std::net::{SocketAddr, TcpStream};
#[cfg(not(feature = "hosted"))]
use std::path::Path;
#[cfg(not(feature = "hosted"))]
use std::time::Duration;

#[cfg(not(feature = "hosted"))]
const INTERNET_TIMEOUT: Duration = Duration::from_millis(800);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "one flag per header HUD icon"
)]
pub struct LocalStatus {
    pub has_wifi_internet: bool,
    pub has_lan_internet: bool,
    /// No internet yet, with a Wi-Fi adapter switched on: the header shows
    /// a dimmed Wi-Fi icon until it connects.
    pub wifi_pending: bool,
    pub has_bluetooth: bool,
    /// A battery answered: the optional `MiSTer` HAT on the `SMBus`, or
    /// a system battery in the kernel's power-supply class off it.
    pub has_battery: bool,
    pub battery_percent: i32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg(not(feature = "hosted"))]
enum InterfaceKind {
    Wifi,
    Lan,
}

/// Probe the host. Blocking (reads /proc, /sys, and dials out with an
/// 800 ms cap) - run on a blocking-capable thread, never the UI loop.
#[cfg(not(feature = "hosted"))]
pub fn probe() -> LocalStatus {
    let network = default_network_kind()
        .filter(|_| internet_reachable())
        .map_or((false, false), |kind| match kind {
            InterfaceKind::Wifi => (true, false),
            InterfaceKind::Lan => (false, true),
        });
    // One HUD reading, two ways to get it: `MiSTer`'s optional fuel
    // gauge hangs off the `SMBus`, everything else exposes the kernel's
    // power-supply class.
    #[cfg(feature = "mister")]
    let battery = crate::mister_battery::read_capacity_percent();
    #[cfg(not(feature = "mister"))]
    let battery = crate::power_supply::read_capacity_percent();
    let offline = !network.0 && !network.1;
    LocalStatus {
        has_wifi_internet: network.0,
        has_lan_internet: network.1,
        wifi_pending: offline && wifi_adapter_up(),
        has_bluetooth: bluetooth_adapter_present(),
        has_battery: battery.is_some(),
        battery_percent: i32::from(battery.unwrap_or(0)),
    }
}

#[cfg(not(feature = "hosted"))]
fn default_network_kind() -> Option<InterfaceKind> {
    let routes = fs::read_to_string("/proc/net/route").ok()?;
    let iface = parse_default_route_interface(&routes)?;
    classify_interface(&iface)
}

#[cfg(any(test, not(feature = "hosted")))]
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

#[cfg(not(feature = "hosted"))]
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

#[cfg(not(feature = "hosted"))]
fn interface_is_up(iface: &str) -> bool {
    let path = Path::new("/sys/class/net").join(iface).join("operstate");
    fs::read_to_string(path).is_ok_and(|state| matches!(state.trim(), "up" | "unknown"))
}

#[cfg(not(feature = "hosted"))]
fn is_wireless_interface(iface: &str) -> bool {
    Path::new("/sys/class/net")
        .join(iface)
        .join("wireless")
        .exists()
        || iface.starts_with("wl")
}

/// A wireless interface the system has switched on, joined to a network
/// or not yet.
#[cfg(not(feature = "hosted"))]
fn wifi_adapter_up() -> bool {
    fs::read_dir("/sys/class/net")
        .ok()
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .any(|entry| {
            let iface = entry.file_name().to_string_lossy().into_owned();
            is_wireless_interface(&iface)
                && fs::read_to_string(entry.path().join("flags"))
                    .is_ok_and(|flags| interface_flags_up(&flags))
        })
}

/// Whether a `/sys/class/net/<iface>/flags` reading (`0x1003`) has
/// `IFF_UP` set.
#[cfg(any(test, not(feature = "hosted")))]
fn interface_flags_up(flags: &str) -> bool {
    let digits = flags.trim();
    let digits = digits
        .strip_prefix("0x")
        .or_else(|| digits.strip_prefix("0X"))
        .unwrap_or(digits);
    u32::from_str_radix(digits, 16).is_ok_and(|flags| flags & 0x1 != 0)
}

#[cfg(not(feature = "hosted"))]
fn internet_reachable() -> bool {
    [
        SocketAddr::from(([1, 1, 1, 1], 443)),
        SocketAddr::from(([8, 8, 8, 8], 443)),
    ]
    .iter()
    .any(|addr| TcpStream::connect_timeout(addr, INTERNET_TIMEOUT).is_ok())
}

#[cfg(not(feature = "hosted"))]
fn bluetooth_adapter_present() -> bool {
    fs::read_dir("/sys/class/bluetooth")
        .ok()
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .any(|entry| entry.file_name().to_string_lossy().starts_with("hci"))
}

/// The status an embedding host last reported. Framework status is
/// host-owned there: never probe Linux sysfs or public IPs.
#[cfg(feature = "hosted")]
static HOST_STATUS: std::sync::Mutex<LocalStatus> = std::sync::Mutex::new(LocalStatus {
    has_wifi_internet: false,
    has_lan_internet: false,
    wifi_pending: false,
    has_bluetooth: false,
    has_battery: false,
    battery_percent: 0,
});

/// Wakes the status loop when a host reports a change, so the header does
/// not wait for its next poll.
static CHANGED: tokio::sync::Notify = tokio::sync::Notify::const_new();

#[cfg(feature = "hosted")]
pub fn probe() -> LocalStatus {
    *HOST_STATUS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Record what the embedding host reports and refresh the header now.
#[cfg(feature = "hosted")]
pub fn set_host_status(status: LocalStatus) {
    *HOST_STATUS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = status;
    CHANGED.notify_one();
}

/// Resolves when a host reports new status. Pending forever where the
/// frontend probes the machine itself.
pub async fn changed() {
    CHANGED.notified().await;
}

#[cfg(test)]
mod tests {
    use super::{interface_flags_up, parse_default_route_interface};

    #[test]
    fn interface_flags_read_iff_up() {
        assert!(interface_flags_up("0x1003\n"));
        assert!(interface_flags_up("0x1"));
        assert!(!interface_flags_up("0x1002\n"));
        assert!(!interface_flags_up(""));
        assert!(!interface_flags_up("garbage"));
    }

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
