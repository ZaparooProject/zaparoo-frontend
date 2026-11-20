// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// The header's network, Bluetooth and battery glyphs and the Hub's
// internet gate, from Core's device state. Core may run on another
// machine, so the frontend never probes its own. The NFC glyph comes from
// Core's `readers` (`router::store_readers`). The rules live in
// `zaparoo_app::device_hud`; this is the Slint adapter.

use slint::{ComponentHandle, Model as _, ModelRc, SharedString, VecModel};
use zaparoo_app::device_hud::{self as rules, Link, NetworkGlyph, Reach, Reading};
use zaparoo_core::device::{Internet, NetworkType};
use zaparoo_core::store::DeviceState;

use crate::router::Ctx;
use crate::App;

fn reading(state: &DeviceState) -> Reading {
    Reading {
        network: state.network.as_ref().map(|network| {
            let link = match network.kind {
                NetworkType::Wifi => Link::Wifi,
                NetworkType::Wired => Link::Wired,
                NetworkType::Other | NetworkType::NoLink | NetworkType::Unknown => Link::Other,
            };
            let reach = match network.internet {
                Some(Internet::Full) => Reach::Full,
                Some(Internet::Portal | Internet::Offline | Internet::Unknown) | None => {
                    Reach::Limited
                }
            };
            (link, reach)
        }),
        bluetooth: state.bluetooth.as_ref().is_some_and(|b| b.present),
        battery_percent: state
            .power
            .as_ref()
            .filter(|power| power.present)
            .and_then(|power| power.percent),
    }
}

/// Publish what Core's device state says to the header and the Hub.
pub(crate) fn publish(ctx: &Ctx, app: &App, state: &DeviceState) {
    let hud = rules::hud(reading(state));
    let shell = app.global::<crate::Shell>();
    // NFC leads and is kept as the readers last left it.
    let mut keys: Vec<SharedString> = shell
        .get_status_keys()
        .iter()
        .filter(|k| k == "NFC")
        .collect();
    match hud.network {
        Some(NetworkGlyph::Wifi) => keys.push("WiFi".into()),
        Some(NetworkGlyph::Wired) => keys.push("WiredNetwork".into()),
        None => {}
    }
    if hud.bluetooth {
        keys.push("Bluetooth".into());
    }
    if !shell.get_status_keys().iter().eq(keys.iter().cloned()) {
        shell.set_status_keys(ModelRc::new(VecModel::from(keys)));
    }
    shell.set_has_battery(hud.battery_percent.is_some());
    shell.set_battery_percent(hud.battery_percent.unwrap_or(0));
    shell.set_status_icons_enabled(true);
    crate::hub::set_internet(ctx, app, hud.internet);
}

#[cfg(test)]
mod tests {
    use super::*;
    use zaparoo_core::device::{BluetoothAdapter, Network, Power};

    #[test]
    fn reads_core_sections_into_the_rule_input() {
        let state = DeviceState {
            network: Some(Network {
                kind: NetworkType::Wired,
                internet: Some(Internet::Portal),
            }),
            bluetooth: Some(BluetoothAdapter {
                present: true,
                powered: Some(false),
            }),
            power: Some(Power {
                present: true,
                percent: Some(40),
                ..Power::default()
            }),
            ..DeviceState::default()
        };
        assert_eq!(
            reading(&state),
            Reading {
                network: Some((Link::Wired, Reach::Limited)),
                bluetooth: true,
                battery_percent: Some(40),
            }
        );
    }

    #[test]
    fn no_battery_and_no_answer_read_as_nothing() {
        let state = DeviceState {
            network: Some(Network {
                kind: NetworkType::NoLink,
                internet: None,
            }),
            power: Some(Power {
                present: false,
                percent: Some(100),
                ..Power::default()
            }),
            ..DeviceState::default()
        };
        assert_eq!(
            reading(&state),
            Reading {
                network: Some((Link::Other, Reach::Limited)),
                bluetooth: false,
                battery_percent: None,
            }
        );
    }
}
