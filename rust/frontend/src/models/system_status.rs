// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// `Browse.SystemStatus` — host-local hardware/network status for the
// top-right HUD. Unsupported probes are intentionally quiet: failing to
// read a Linux/MiSTer sysfs/procfs path simply leaves the related icon
// hidden.

use cxx_qt::{Initialize, Threading};
use std::fs;
use std::net::{SocketAddr, TcpStream};
use std::path::Path;
use std::pin::Pin;
use std::thread;
use std::time::Duration;
use tracing::warn;
use zaparoo_core::endpoints::readers::ReadersEndpoint;
use zaparoo_core::media_types::{ReaderInfo, ReadersResult};
use zaparoo_core::remote_resource::ResourceStatus;

const LOCAL_PROBE_INTERVAL: Duration = Duration::from_secs(30);
const NFC_REFETCH_INTERVAL: Duration = Duration::from_secs(30);
const INTERNET_TIMEOUT: Duration = Duration::from_millis(800);

#[derive(Default)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "each field is an independent qproperty exposed to QML; folding them into a bitset would change the SystemStatus singleton's QML surface"
)]
pub struct SystemStatusRust {
    has_nfc: bool,
    has_wifi_internet: bool,
    has_lan_internet: bool,
    has_bluetooth: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct LocalStatus {
    has_wifi_internet: bool,
    has_lan_internet: bool,
    has_bluetooth: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum InterfaceKind {
    Wifi,
    Lan,
}

#[cxx_qt::bridge]
pub mod ffi {
    unsafe extern "C++" {
        include!("model_includes.h");
    }

    unsafe extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[qml_singleton]
        #[qproperty(bool, has_nfc)]
        #[qproperty(bool, has_wifi_internet)]
        #[qproperty(bool, has_lan_internet)]
        #[qproperty(bool, has_bluetooth)]
        type SystemStatus = super::SystemStatusRust;
    }

    impl cxx_qt::Threading for SystemStatus {}
    impl cxx_qt::Initialize for SystemStatus {}
}

impl Initialize for ffi::SystemStatus {
    fn initialize(mut self: Pin<&mut Self>) {
        apply_local_status(self.as_mut(), probe_local_status());

        let qt_thread = self.qt_thread();
        if let Err(e) = thread::Builder::new()
            .name("zaparoo-system-status".into())
            .spawn(move || loop {
                thread::sleep(LOCAL_PROBE_INTERVAL);
                let status = probe_local_status();
                let _ = qt_thread.queue(move |model| apply_local_status(model, status));
            })
        {
            warn!("failed to spawn system status probe thread: {e}");
        }

        if crate::models::core_is_local() {
            bind_local_readers(self.as_mut());
        }
    }
}

fn bind_local_readers(mut model: Pin<&mut ffi::SystemStatus>) {
    let store = crate::models::global_store();
    let resource = store.subscribe::<ReadersEndpoint>(());
    let mut rx = resource.subscribe();
    apply_has_nfc(model.as_mut(), project_readers(&rx.borrow_and_update()));

    let qt_thread = model.qt_thread();
    crate::models::global_handle().spawn(async move {
        while rx.changed().await.is_ok() {
            let has_nfc = project_readers(&rx.borrow_and_update());
            let _ = qt_thread.queue(move |m| apply_has_nfc(m, has_nfc));
        }
    });

    let mut notifications = store.subscribe_notifications();
    let resource_for_notifications = resource.clone();
    crate::models::global_handle().spawn(async move {
        loop {
            match notifications.recv().await {
                Ok(notification)
                    if matches!(
                        notification.method.as_str(),
                        "readers.added" | "readers.removed"
                    ) =>
                {
                    resource_for_notifications.refetch();
                }
                Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });

    let resource_for_poll = resource.clone();
    if let Err(e) = thread::Builder::new()
        .name("zaparoo-reader-status".into())
        .spawn(move || loop {
            thread::sleep(NFC_REFETCH_INTERVAL);
            resource_for_poll.refetch();
        })
    {
        warn!("failed to spawn reader status probe thread: {e}");
    }
}

fn project_readers(status: &ResourceStatus<ReadersResult>) -> bool {
    match status {
        ResourceStatus::Ready(result) => result.readers.iter().any(ReaderInfo::is_nfc_reader),
        ResourceStatus::Idle | ResourceStatus::Loading | ResourceStatus::Errored { .. } => false,
    }
}

fn apply_has_nfc(mut model: Pin<&mut ffi::SystemStatus>, has_nfc: bool) {
    if model.has_nfc != has_nfc {
        model.as_mut().set_has_nfc(has_nfc);
    }
}

fn apply_local_status(mut model: Pin<&mut ffi::SystemStatus>, status: LocalStatus) {
    if model.has_wifi_internet != status.has_wifi_internet {
        model
            .as_mut()
            .set_has_wifi_internet(status.has_wifi_internet);
    }
    if model.has_lan_internet != status.has_lan_internet {
        model.as_mut().set_has_lan_internet(status.has_lan_internet);
    }
    if model.has_bluetooth != status.has_bluetooth {
        model.as_mut().set_has_bluetooth(status.has_bluetooth);
    }
}

fn probe_local_status() -> LocalStatus {
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
    let iface = default_route_interface(Path::new("/proc/net/route"))?;
    classify_interface(&iface)
}

fn default_route_interface(path: &Path) -> Option<String> {
    let routes = fs::read_to_string(path).ok()?;
    parse_default_route_interface(&routes)
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
    use super::{parse_default_route_interface, project_readers};
    use zaparoo_core::media_types::{ReaderInfo, ReadersResult};
    use zaparoo_core::remote_resource::ResourceStatus;

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

    #[test]
    fn readers_ready_projects_to_has_nfc() {
        let result = ReadersResult {
            readers: vec![ReaderInfo {
                driver: "pn532".into(),
                connected: true,
                ..ReaderInfo::default()
            }],
        };
        assert!(project_readers(&ResourceStatus::Ready(result)));
    }

    #[test]
    fn readers_error_projects_to_hidden() {
        assert!(!project_readers(
            &ResourceStatus::<ReadersResult>::Errored {
                message: "unsupported".into(),
                retrying: true,
            }
        ));
    }
}
