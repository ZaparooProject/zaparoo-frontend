// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// Wire types for Core's `device.*` methods: the parts of `device.status`
// and `device.changed` the frontend reads (power, controllers, the
// Bluetooth adapter, capabilities), and the `device.bluetooth.*` state.
// Every field tolerates a missing key and an explicit `null`, so a Core
// that predates a field, or reports a section as unknown, still decodes.

use serde::{Deserialize, Deserializer, Serialize};
use std::collections::HashMap;

/// A field that may be absent or `null`, read as its default either way.
fn null_default<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Default + Deserialize<'de>,
{
    Ok(Option::<T>::deserialize(deserializer)?.unwrap_or_default())
}

/// The value an action in `capabilities.actions` has when this client may
/// call it.
pub const SUPPORTED: &str = "supported";
/// The action Bluetooth pairing is offered on.
pub const ACTION_BLUETOOTH_PAIR: &str = "device.bluetooth.pair";

/// `capabilities`: section and action names to `supported`,
/// `unsupported` or `notPermitted`. A missing key means unsupported.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct Capabilities {
    #[serde(default, deserialize_with = "null_default")]
    pub sections: HashMap<String, String>,
    #[serde(default, deserialize_with = "null_default")]
    pub actions: HashMap<String, String>,
}

impl Capabilities {
    /// What Core says about `action` for this client, if anything.
    pub fn action(&self, action: &str) -> Option<&str> {
        self.actions.get(action).map(String::as_str)
    }
}

/// `power`: the battery the device itself runs on.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Power {
    #[serde(default, deserialize_with = "null_default")]
    pub present: bool,
    #[serde(default)]
    pub percent: Option<i64>,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub charge_state: Option<String>,
}

/// `bluetooth`: the adapter, as Core reads it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct BluetoothAdapter {
    #[serde(default, deserialize_with = "null_default")]
    pub present: bool,
    /// `None` where the adapter's power state cannot be read.
    #[serde(default)]
    pub powered: Option<bool>,
}

/// A controller's own battery.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ControllerBattery {
    #[serde(default)]
    pub percent: Option<i64>,
    /// `empty`, `low`, `medium` or `full`.
    #[serde(default, deserialize_with = "null_default")]
    pub level: String,
    /// `charging`, `discharging`, `full` or `notCharging`; `None` when
    /// unknown, and on a Core that predates the field.
    #[serde(default)]
    pub charge_state: Option<String>,
}

/// One connected game controller.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Controller {
    /// Identifies the controller only while it stays connected.
    #[serde(default, deserialize_with = "null_default")]
    pub id: String,
    /// The name the device reports (the kernel's).
    #[serde(default)]
    pub name: Option<String>,
    /// Core's friendly model name, from its own vendor and product table;
    /// `None` for a model it does not know, and on an older Core.
    #[serde(default)]
    pub display_name: Option<String>,
    /// `usb`, `bluetooth` or `unknown`.
    #[serde(default, deserialize_with = "null_default")]
    pub connection: String,
    /// `None` for a controller with no battery of its own.
    #[serde(default)]
    pub battery: Option<ControllerBattery>,
}

/// `controllers`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct Controllers {
    #[serde(default, deserialize_with = "null_default")]
    pub items: Vec<Controller>,
}

/// `device.status`: the sections the frontend reads. Any section may be
/// `null` (unsupported, or no reading yet).
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct DeviceStatus {
    #[serde(default, deserialize_with = "null_default")]
    pub capabilities: Capabilities,
    #[serde(default)]
    pub power: Option<Power>,
    #[serde(default)]
    pub bluetooth: Option<BluetoothAdapter>,
    #[serde(default)]
    pub controllers: Option<Controllers>,
}

/// One Bluetooth device Core knows: paired, or found by the current or
/// last scan.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
#[allow(
    clippy::struct_excessive_bools,
    reason = "wire-faithful flags, one per BlueZ property"
)]
pub struct BluetoothDevice {
    /// The device's address.
    #[serde(default, deserialize_with = "null_default")]
    pub id: String,
    /// `BlueZ`'s alias or name.
    #[serde(default)]
    pub name: Option<String>,
    /// Core's friendly model name, as on `Controller`.
    #[serde(default)]
    pub display_name: Option<String>,
    /// From `BlueZ`'s `Modalias`: four lowercase hex digits each, or `None`.
    #[serde(default)]
    pub vendor_id: Option<String>,
    #[serde(default)]
    pub product_id: Option<String>,
    #[serde(default, deserialize_with = "null_default")]
    pub paired: bool,
    #[serde(default, deserialize_with = "null_default")]
    pub connected: bool,
    #[serde(default, deserialize_with = "null_default")]
    pub trusted: bool,
    #[serde(default, deserialize_with = "null_default")]
    pub gamepad: bool,
    /// The `device.status` controller this is, while it is connected.
    #[serde(default)]
    pub controller_id: Option<String>,
}

/// The latest pairing attempt. It stays at its last result until the next
/// attempt starts.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct BluetoothPairing {
    #[serde(default, deserialize_with = "null_default")]
    pub id: String,
    /// `pairing`, `paired` or `failed`.
    #[serde(default, deserialize_with = "null_default")]
    pub state: String,
    /// Why a failed attempt failed: `pinRequired`, `notPowered`,
    /// `timeout`, `rejected`, `notFound` or `failed`.
    #[serde(default)]
    pub error: Option<String>,
    /// Counts pair requests since Core started: a later attempt has a
    /// higher number.
    #[serde(default, deserialize_with = "null_default")]
    pub attempt: u64,
}

/// `device.bluetooth.devices`, the result of every other
/// `device.bluetooth.*` method, and the params of
/// `device.bluetooth.changed`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct BluetoothState {
    #[serde(default, deserialize_with = "null_default")]
    pub powered: bool,
    #[serde(default, deserialize_with = "null_default")]
    pub scanning: bool,
    #[serde(default, deserialize_with = "null_default")]
    pub devices: Vec<BluetoothDevice>,
    #[serde(default)]
    pub pairing: Option<BluetoothPairing>,
}

/// `device.bluetooth.scan.start`.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BluetoothScanParams {
    /// List every device found, not only gamepads.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub all: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_seconds: Option<u32>,
}

/// `device.bluetooth.pair` and `device.bluetooth.forget`.
#[derive(Debug, Clone, Serialize)]
pub struct BluetoothDeviceParams {
    pub id: String,
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "a decode failure is the test failing")]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn device_status_reads_the_sections_it_needs() {
        let status: DeviceStatus = serde_json::from_value(json!({
            "capabilities": {
                "sections": { "controllers": "supported", "bluetooth": "supported" },
                "actions": { "device.bluetooth.pair": "supported" }
            },
            "power": { "present": true, "percent": 96, "source": "external", "chargeState": "charging" },
            "network": { "type": "wifi" },
            "bluetooth": { "present": true, "powered": true },
            "controllers": {
                "count": 2,
                "items": [
                    {
                        "id": "input17",
                        "name": "DualSense Wireless Controller",
                        "displayName": "DualSense",
                        "vendorId": "054c",
                        "productId": "0ce6",
                        "connection": "bluetooth",
                        "battery": { "percent": 80, "level": "full", "chargeState": "charging" }
                    },
                    { "id": "input3", "name": null, "connection": "usb", "battery": null }
                ]
            },
            "time": { "clockReliable": true }
        }))
        .unwrap();
        assert_eq!(
            status.capabilities.action(ACTION_BLUETOOTH_PAIR),
            Some(SUPPORTED)
        );
        assert_eq!(status.power.unwrap().percent, Some(96));
        assert_eq!(status.bluetooth.unwrap().powered, Some(true));
        let items = status.controllers.unwrap().items;
        assert_eq!(items.len(), 2);
        let battery = items[0].battery.clone().unwrap();
        assert_eq!(battery.percent, Some(80));
        assert_eq!(battery.charge_state.as_deref(), Some("charging"));
        assert_eq!(items[0].display_name.as_deref(), Some("DualSense"));
        assert_eq!(items[1].name, None);
        assert_eq!(items[1].display_name, None);
        assert_eq!(items[1].battery, None);
    }

    #[test]
    fn null_sections_and_an_older_battery_shape_decode() {
        let status: DeviceStatus = serde_json::from_value(json!({
            "capabilities": { "sections": {}, "actions": null },
            "power": null,
            "bluetooth": null,
            "controllers": { "count": 1, "items": [
                { "id": "x", "connection": "unknown", "battery": { "percent": null, "level": "low" } }
            ] }
        }))
        .unwrap();
        assert!(status.capabilities.actions.is_empty());
        assert_eq!(status.bluetooth, None);
        let battery = status.controllers.unwrap().items[0]
            .battery
            .clone()
            .unwrap();
        assert_eq!(battery.level, "low");
        assert_eq!(battery.charge_state, None);
    }

    #[test]
    fn bluetooth_state_reads_devices_and_the_last_attempt() {
        let state: BluetoothState = serde_json::from_value(json!({
            "powered": true,
            "scanning": false,
            "devices": [{
                "id": "AA:BB:CC:DD:EE:FF",
                "name": null,
                "displayName": "DualShock 4",
                "vendorId": "054c",
                "productId": "09cc",
                "paired": true,
                "connected": true,
                "trusted": true,
                "gamepad": true,
                "controllerId": "input17"
            }],
            "pairing": { "id": "AA:BB:CC:DD:EE:FF", "state": "failed", "error": "pinRequired" }
        }))
        .unwrap();
        assert!(state.powered);
        assert_eq!(state.devices[0].controller_id.as_deref(), Some("input17"));
        assert_eq!(
            state.devices[0].display_name.as_deref(),
            Some("DualShock 4")
        );
        assert_eq!(state.devices[0].vendor_id.as_deref(), Some("054c"));
        assert_eq!(state.devices[0].product_id.as_deref(), Some("09cc"));
        let pairing = state.pairing.unwrap();
        assert_eq!(pairing.error.as_deref(), Some("pinRequired"));
        let empty: BluetoothState =
            serde_json::from_value(json!({ "devices": null, "pairing": null })).unwrap();
        assert_eq!(empty, BluetoothState::default());
    }

    #[test]
    fn scan_params_leave_out_what_is_not_set() {
        assert_eq!(
            serde_json::to_value(BluetoothScanParams::default()).unwrap(),
            json!({})
        );
        assert_eq!(
            serde_json::to_value(BluetoothScanParams {
                all: true,
                timeout_seconds: Some(60)
            })
            .unwrap(),
            json!({ "all": true, "timeoutSeconds": 60 })
        );
    }
}
