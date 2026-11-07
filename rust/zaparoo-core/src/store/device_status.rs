// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// `DeviceResource`: the connected controllers, the Bluetooth adapter and
// its devices, as one `watch` channel. Seeded from `device.status` (and
// `device.bluetooth.devices` where Core can pair) on every connect, then
// kept current from `device.changed` and `device.bluetooth.changed`.
//
// A Core that predates `device.*` answers "method not found": that reads
// as no controllers and no Bluetooth, quietly, so Settings and the header
// simply show nothing.

use crate::client::{Client, ClientError, ConnectionState, Notification};
use crate::device::{
    BluetoothAdapter, BluetoothScanParams, BluetoothState, Capabilities, Controller, Controllers,
    DeviceStatus, Power,
};
use serde_json::Value;
use std::sync::Arc;
use tokio::runtime::Handle;
use tokio::sync::{broadcast, watch};
use tracing::{debug, warn};

/// What the frontend knows about the device Core runs on.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DeviceState {
    /// `device.status` has answered (or failed) since the last connect.
    pub seeded: bool,
    pub capabilities: Capabilities,
    pub power: Option<Power>,
    pub bluetooth: Option<BluetoothAdapter>,
    /// `None` when Core reports no controllers section at all.
    pub controllers: Option<Vec<Controller>>,
    /// `None` until `device.bluetooth.devices` answers, and on a Core that
    /// cannot pair.
    pub bluetooth_devices: Option<BluetoothState>,
}

impl DeviceState {
    fn apply_status(&mut self, status: DeviceStatus) {
        self.capabilities = status.capabilities;
        self.power = status.power;
        self.bluetooth = status.bluetooth;
        self.controllers = status.controllers.map(|c| c.items);
        self.seeded = true;
    }

    /// `device.changed` carries whole sections: one that is present
    /// replaces what is held, `null` included. Capabilities never ride on
    /// it and keep the seeded answer.
    fn apply_changed(&mut self, params: &Value) {
        let Some(sections) = params.as_object() else {
            return;
        };
        let section = |key: &str| sections.get(key).filter(|v| !v.is_null()).cloned();
        if sections.contains_key("power") {
            self.power = section("power").and_then(|v| decode("power", v));
        }
        if sections.contains_key("bluetooth") {
            self.bluetooth = section("bluetooth").and_then(|v| decode("bluetooth", v));
        }
        if sections.contains_key("controllers") {
            self.controllers = section("controllers")
                .and_then(|v| decode::<Controllers>("controllers", v))
                .map(|c| c.items);
        }
    }

    /// Whether this Core can list Bluetooth devices for this client:
    /// it named a Bluetooth action at all.
    fn lists_bluetooth(&self) -> bool {
        self.capabilities
            .actions
            .keys()
            .any(|action| action.starts_with("device.bluetooth."))
    }
}

fn decode<T: serde::de::DeserializeOwned>(what: &str, value: Value) -> Option<T> {
    serde_json::from_value(value)
        .map_err(|e| warn!("device: {what} section decode failed: {e}"))
        .ok()
}

/// Live device publisher. One per `Store`.
pub struct DeviceResource {
    state: Arc<watch::Sender<DeviceState>>,
    client: Arc<Client>,
}

impl std::fmt::Debug for DeviceResource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeviceResource").finish_non_exhaustive()
    }
}

impl DeviceResource {
    pub fn new(client: &Arc<Client>, runtime: &Handle) -> Arc<Self> {
        let (state_tx, _) = watch::channel(DeviceState::default());
        let state = Arc::new(state_tx);
        let resource = Arc::new(Self {
            state: state.clone(),
            client: client.clone(),
        });
        let connection_rx = client.connection.subscribe();
        let notifications_rx = client.subscribe_notifications();
        let client = client.clone();
        runtime.spawn(async move {
            run_task(client, connection_rx, notifications_rx, state).await;
        });
        resource
    }

    pub fn subscribe(&self) -> watch::Receiver<DeviceState> {
        self.state.subscribe()
    }

    /// The state as it stands now.
    pub fn current(&self) -> DeviceState {
        self.state.borrow().clone()
    }

    /// Look for gamepads for `timeout_seconds`. Core stops on its own
    /// after that; asked again while it is still searching, it extends the
    /// search from now and keeps what it has found.
    pub async fn scan_start(&self, timeout_seconds: u32) -> Result<(), ClientError> {
        let result = self
            .client
            .bluetooth_scan_start(BluetoothScanParams {
                all: false,
                timeout_seconds: Some(timeout_seconds),
            })
            .await?;
        self.apply_bluetooth(result);
        Ok(())
    }

    pub async fn scan_stop(&self) -> Result<(), ClientError> {
        let result = self.client.bluetooth_scan_stop().await?;
        self.apply_bluetooth(result);
        Ok(())
    }

    /// Ask Core to pair `id`. Answers the attempt's number; its outcome
    /// arrives with a later state.
    pub async fn pair(&self, id: &str) -> Result<u64, ClientError> {
        let result = self.client.bluetooth_pair(id).await?;
        let attempt = result.pairing.as_ref().map_or(0, |p| p.attempt);
        self.apply_bluetooth(result);
        Ok(attempt)
    }

    pub async fn forget(&self, id: &str) -> Result<(), ClientError> {
        let result = self.client.bluetooth_forget(id).await?;
        self.apply_bluetooth(result);
        Ok(())
    }

    /// Every `device.bluetooth.*` answer is the whole state: hold it at
    /// once rather than waiting for the notification that follows.
    fn apply_bluetooth(&self, result: BluetoothState) {
        self.state.send_if_modified(|s| hold_bluetooth(s, result));
    }
}

/// Hold a Bluetooth state from a reply or a notification. A reply can
/// arrive after the notification of its attempt's outcome: the outcome
/// stays rather than going back to `pairing`, and an older attempt never
/// replaces a newer one.
fn hold_bluetooth(s: &mut DeviceState, mut incoming: BluetoothState) -> bool {
    if let Some(held) = s
        .bluetooth_devices
        .as_ref()
        .and_then(|b| b.pairing.as_ref())
    {
        let stale = incoming.pairing.as_ref().is_none_or(|new| {
            new.attempt < held.attempt
                || new.attempt == held.attempt && new.state == "pairing" && held.state != "pairing"
        });
        if stale {
            incoming.pairing = Some(held.clone());
        }
    }
    replace(&mut s.bluetooth_devices, Some(incoming))
}

fn replace<T: PartialEq>(slot: &mut T, value: T) -> bool {
    if *slot == value {
        return false;
    }
    *slot = value;
    true
}

async fn run_task(
    client: Arc<Client>,
    mut connection_rx: watch::Receiver<ConnectionState>,
    mut notifications_rx: broadcast::Receiver<Notification>,
    state: Arc<watch::Sender<DeviceState>>,
) {
    loop {
        let current = connection_rx.borrow_and_update().clone();
        if matches!(current, ConnectionState::Connected) {
            seed(&client, &state).await;
            loop {
                tokio::select! {
                    notification = notifications_rx.recv() => {
                        match notification {
                            Ok(n) => {
                                match fold_notification(&n, &state) {
                                    Refresh::Nothing => {}
                                    Refresh::Devices => seed_bluetooth(&client, &state).await,
                                    Refresh::Status => seed(&client, &state).await,
                                }
                            }
                            Err(broadcast::error::RecvError::Lagged(skipped)) => {
                                warn!(skipped, "device: notifications lagged; re-seeding");
                                seed(&client, &state).await;
                            }
                            Err(broadcast::error::RecvError::Closed) => return,
                        }
                    }
                    changed = connection_rx.changed() => {
                        if changed.is_err() {
                            return;
                        }
                        break;
                    }
                }
            }
        } else {
            // Nothing read over a dead link is current: no controllers
            // until Core answers again.
            state.send_if_modified(|s| replace(s, DeviceState::default()));
            if connection_rx.changed().await.is_err() {
                return;
            }
        }
    }
}

async fn seed(client: &Arc<Client>, state: &Arc<watch::Sender<DeviceState>>) {
    match client.device_status().await {
        Ok(status) => {
            state.send_modify(|s| s.apply_status(status));
            debug!("device: seeded from device.status");
        }
        Err(e) => {
            // An older Core has no `device.*`: nothing to show, not an
            // error worth more than a debug line.
            debug!("device: device.status unavailable: {e}");
            state.send_modify(|s| {
                *s = DeviceState {
                    seeded: true,
                    ..DeviceState::default()
                };
            });
            return;
        }
    }
    seed_bluetooth(client, state).await;
}

/// Ask for the Bluetooth device list where this Core can give one.
async fn seed_bluetooth(client: &Arc<Client>, state: &Arc<watch::Sender<DeviceState>>) {
    let wanted = { state.borrow().lists_bluetooth() };
    if !wanted {
        return;
    }
    match client.bluetooth_devices().await {
        Ok(devices) => {
            state.send_if_modified(|s| hold_bluetooth(s, devices));
        }
        Err(e) => debug!("device: device.bluetooth.devices unavailable: {e}"),
    }
}

/// What a notification leaves to be read again from Core.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Refresh {
    Nothing,
    /// The Bluetooth device list: an adapter appeared that this Core can
    /// pair with, and no list is held yet.
    Devices,
    /// All of `device.status`: the adapter came, went or switched, and
    /// what Core offers for it (its capabilities) is not in the push.
    Status,
}

/// Fold one notification into the state and say what to read again.
fn fold_notification(
    notification: &Notification,
    state: &Arc<watch::Sender<DeviceState>>,
) -> Refresh {
    match notification.method.as_str() {
        "device.changed" => {
            let mut refresh = Refresh::Nothing;
            state.send_if_modified(|s| {
                let before = s.clone();
                s.apply_changed(&notification.params);
                if s.bluetooth != before.bluetooth {
                    refresh = Refresh::Status;
                } else if s.bluetooth_devices.is_none()
                    && s.lists_bluetooth()
                    && s.bluetooth.as_ref().is_some_and(|b| b.present)
                {
                    refresh = Refresh::Devices;
                }
                *s != before
            });
            refresh
        }
        "device.bluetooth.changed" => {
            match serde_json::from_value::<BluetoothState>(notification.params.clone()) {
                Ok(devices) => {
                    state.send_if_modified(|s| hold_bluetooth(s, devices));
                }
                Err(e) => warn!("device: device.bluetooth.changed decode failed: {e}"),
            }
            Refresh::Nothing
        }
        _ => Refresh::Nothing,
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "a decode failure is the test failing")]
mod tests {
    use super::*;
    use serde_json::json;

    fn notification(method: &str, params: Value) -> Notification {
        Notification {
            method: method.to_string(),
            params,
        }
    }

    fn seeded() -> Arc<watch::Sender<DeviceState>> {
        let status: DeviceStatus = serde_json::from_value(json!({
            "capabilities": { "actions": { "device.bluetooth.pair": "supported" } },
            "power": null,
            "bluetooth": { "present": true, "powered": true },
            "controllers": { "count": 1, "items": [
                { "id": "a", "name": "Pad", "connection": "bluetooth",
                  "battery": { "percent": 80, "level": "full", "chargeState": "charging" } }
            ] }
        }))
        .unwrap();
        let mut state = DeviceState::default();
        state.apply_status(status);
        Arc::new(watch::channel(state).0)
    }

    #[test]
    fn a_present_section_replaces_the_held_one_and_an_absent_one_stays() {
        let state = seeded();
        fold_notification(
            &notification(
                "device.changed",
                json!({ "controllers": { "count": 0, "items": [] } }),
            ),
            &state,
        );
        let s = state.borrow().clone();
        assert_eq!(s.controllers, Some(Vec::new()));
        assert_eq!(s.bluetooth.unwrap().powered, Some(true), "not in the push");
        assert!(s.capabilities.action("device.bluetooth.pair").is_some());
        fold_notification(
            &notification(
                "device.changed",
                json!({ "bluetooth": { "present": true, "powered": false }, "controllers": null }),
            ),
            &state,
        );
        let s = state.borrow().clone();
        assert_eq!(s.bluetooth.unwrap().powered, Some(false));
        assert_eq!(s.controllers, None, "a null section is no section");
    }

    #[test]
    fn a_bluetooth_push_replaces_the_device_list() {
        let state = seeded();
        let asks = fold_notification(
            &notification(
                "device.bluetooth.changed",
                json!({ "powered": true, "scanning": true, "devices": [
                    { "id": "AA", "name": "8BitDo", "paired": false, "connected": false,
                      "trusted": false, "gamepad": true, "controllerId": null }
                ], "pairing": null }),
            ),
            &state,
        );
        assert_eq!(asks, Refresh::Nothing);
        let devices = state.borrow().bluetooth_devices.clone().unwrap();
        assert!(devices.scanning);
        assert_eq!(devices.devices[0].id, "AA");
    }

    #[test]
    fn a_held_adapter_with_no_list_asks_for_the_list_once() {
        let state = seeded();
        let push = || {
            notification(
                "device.changed",
                json!({ "bluetooth": { "present": true, "powered": true } }),
            )
        };
        let asks = fold_notification(&push(), &state);
        assert_eq!(asks, Refresh::Devices, "a pairing Core with no list yet");
        state.send_modify(|s| s.bluetooth_devices = Some(BluetoothState::default()));
        assert_eq!(fold_notification(&push(), &state), Refresh::Nothing);
    }

    #[test]
    fn an_adapter_coming_back_reads_the_status_again() {
        // Seeded while the dongle was off the bus: no Bluetooth actions.
        let state = Arc::new(
            watch::channel(DeviceState {
                seeded: true,
                bluetooth: Some(BluetoothAdapter {
                    present: true,
                    powered: None,
                }),
                ..DeviceState::default()
            })
            .0,
        );
        let asks = fold_notification(
            &notification(
                "device.changed",
                json!({ "bluetooth": { "present": true, "powered": true } }),
            ),
            &state,
        );
        assert_eq!(
            asks,
            Refresh::Status,
            "its actions are only in device.status"
        );
    }

    #[test]
    fn unrelated_and_malformed_pushes_change_nothing() {
        let state = seeded();
        let before = state.borrow().clone();
        fold_notification(&notification("media.indexing", json!({})), &state);
        fold_notification(
            &notification("device.bluetooth.changed", json!({ "devices": 7 })),
            &state,
        );
        fold_notification(&notification("device.changed", json!([1, 2])), &state);
        assert_eq!(*state.borrow(), before);
    }

    fn bluetooth(attempt: u64, state: &str) -> BluetoothState {
        serde_json::from_value(json!({
            "powered": true, "scanning": false, "devices": [],
            "pairing": { "id": "AA", "state": state, "error": null, "attempt": attempt }
        }))
        .unwrap()
    }

    fn held(s: &DeviceState) -> (u64, String) {
        let pairing = s
            .bluetooth_devices
            .as_ref()
            .unwrap()
            .pairing
            .as_ref()
            .unwrap();
        (pairing.attempt, pairing.state.clone())
    }

    #[test]
    fn a_late_reply_does_not_undo_its_attempts_outcome() {
        let mut s = DeviceState::default();
        assert!(hold_bluetooth(&mut s, bluetooth(1, "failed")));
        hold_bluetooth(&mut s, bluetooth(2, "failed"));
        assert!(
            !hold_bluetooth(&mut s, bluetooth(2, "pairing")),
            "the reply is older news"
        );
        assert_eq!(held(&s), (2, "failed".to_string()));
        hold_bluetooth(&mut s, bluetooth(1, "pairing"));
        assert_eq!(
            held(&s),
            (2, "failed".to_string()),
            "an older attempt never wins"
        );
        hold_bluetooth(&mut s, bluetooth(3, "pairing"));
        assert_eq!(held(&s), (3, "pairing".to_string()));
        hold_bluetooth(&mut s, bluetooth(3, "paired"));
        assert_eq!(held(&s), (3, "paired".to_string()));
    }
}
