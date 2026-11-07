// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// The device Core runs on, simulated for the Controllers page and the
// header: `device.status` with two Bluetooth controllers (one charging at
// 80%, one at 10% and running down) and two more paired but switched off,
// and the `device.bluetooth.*` methods over a scripted radio. A search
// finds gamepads spread over its first twenty seconds: two that share a
// name, one that always wants a PIN, and one with no name at all. Asked to
// search again while it still is, the radio extends the search from now
// and keeps what it found, as Core does. Pairing takes about two seconds
// and connects the pad as a controller; Forget drops it again. Each change
// is pushed as Core pushes it (`device.bluetooth.changed`,
// `device.changed`).
//
// `MOCK_CORE_BLUETOOTH=off` reports the adapter switched off, and `none`
// answers like a Core that predates `device.*`.

use crate::media_state::Notifier;
use serde_json::{json, Value};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;
use tokio::time::Instant;

const PAIR_ACTIONS: [&str; 3] = [
    "device.bluetooth.scan.start",
    "device.bluetooth.pair",
    "device.bluetooth.forget",
];
const POWERED_OFF: &str = "bluetooth adapter is powered off";

/// Core's `error.data.category` for a refusal the mock words itself: an
/// adapter that is off is `unavailable`, as Core sends it.
pub fn error_category(message: &str) -> Option<&'static str> {
    (message == POWERED_OFF).then_some("unavailable")
}
const PAIR_TIME: Duration = Duration::from_secs(2);
const PIN_FAIL_TIME: Duration = Duration::from_millis(1500);
const DEFAULT_SCAN_SECONDS: u64 = 60;

/// One device a search turns up.
struct Find {
    /// When it shows, from the start of the search.
    at_ms: u64,
    id: &'static str,
    name: Option<&'static str>,
    display_name: Option<&'static str>,
    model: Option<(&'static str, &'static str)>,
    /// Wants a PIN, so pairing always fails.
    pin: bool,
}

/// What a search turns up, in order.
const FINDS: [Find; 5] = [
    Find {
        at_ms: 1500,
        id: "A4:AE:12:34:79:CA",
        name: Some("Wireless Controller"),
        display_name: Some("DualShock 4"),
        model: Some(("054c", "09cc")),
        pin: false,
    },
    Find {
        at_ms: 4000,
        id: "98:B6:E9:12:56:04",
        name: Some("Pro Controller"),
        display_name: Some("Switch Pro Controller"),
        model: Some(("057e", "2009")),
        pin: false,
    },
    Find {
        at_ms: 8000,
        id: "E4:17:D8:3C:90:05",
        name: Some("Arcade Stick"),
        display_name: None,
        model: None,
        pin: true,
    },
    Find {
        at_ms: 13000,
        id: "A4:AE:12:34:11:0F",
        name: Some("Wireless Controller"),
        display_name: Some("DualShock 4"),
        model: Some(("054c", "09cc")),
        pin: false,
    },
    Find {
        at_ms: 20000,
        id: "C8:3F:26:7A:00:07",
        name: None,
        display_name: None,
        model: None,
        pin: false,
    },
];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    On,
    Off,
    Absent,
}

fn mode() -> Mode {
    match std::env::var("MOCK_CORE_BLUETOOTH").as_deref() {
        Ok("off") => Mode::Off,
        Ok("none") => Mode::Absent,
        _ => Mode::On,
    }
}

#[derive(Clone)]
struct Device {
    id: String,
    name: Option<String>,
    display_name: Option<String>,
    model: Option<(&'static str, &'static str)>,
    paired: bool,
    connected: bool,
    /// Wants a PIN, so pairing always fails.
    pin: bool,
    controller: Option<Controller>,
}

#[derive(Clone)]
struct Controller {
    id: String,
    percent: i64,
    level: &'static str,
    charge: &'static str,
}

struct Pairing {
    id: String,
    attempt: u64,
    state: &'static str,
    error: Option<&'static str>,
}

/// The simulated adapter and what it knows.
struct Radio {
    powered: bool,
    scanning: bool,
    devices: Vec<Device>,
    pairing: Option<Pairing>,
    /// Numbers pair requests, like Core does.
    attempts: u64,
    /// Bumped by every search that starts afresh and every stop, so a
    /// stale script stops.
    scan_generation: u64,
    /// When the running search ends; a repeat start moves it.
    scan_deadline: Option<Instant>,
}

impl Radio {
    fn new(powered: bool) -> Self {
        let paired = |id: &str,
                      name: &str,
                      display_name: Option<&str>,
                      model: Option<(&'static str, &'static str)>,
                      controller: Option<Controller>| Device {
            id: id.into(),
            name: Some(name.into()),
            display_name: display_name.map(Into::into),
            model,
            paired: true,
            connected: controller.is_some(),
            pin: false,
            controller,
        };
        Self {
            powered,
            scanning: false,
            devices: vec![
                paired(
                    "B0:00:00:00:00:01",
                    "Sony Interactive Entertainment DualSense Wireless Controller",
                    Some("DualSense"),
                    Some(("054c", "0ce6")),
                    Some(Controller {
                        id: "input17".into(),
                        percent: 80,
                        level: "full",
                        charge: "charging",
                    }),
                ),
                paired(
                    "B0:00:00:00:00:02",
                    "8BitDo Pro 2",
                    None,
                    None,
                    Some(Controller {
                        id: "input21".into(),
                        percent: 10,
                        level: "low",
                        charge: "discharging",
                    }),
                ),
                paired(
                    "B0:00:00:00:00:08",
                    "Xbox Wireless Controller",
                    Some("Xbox Wireless Controller"),
                    Some(("045e", "0b13")),
                    None,
                ),
                paired(
                    "B0:00:00:00:00:09",
                    "Wireless Controller",
                    Some("DualShock 4"),
                    Some(("054c", "05c4")),
                    None,
                ),
            ],
            pairing: None,
            attempts: 0,
            scan_generation: 0,
            scan_deadline: None,
        }
    }

    fn controllers(&self) -> Value {
        let items: Vec<Value> = self
            .devices
            .iter()
            .filter(|d| d.connected)
            .filter_map(|d| {
                let c = d.controller.as_ref()?;
                Some(json!({
                    "id": c.id,
                    "name": d.name,
                    "displayName": d.display_name,
                    "vendorId": d.model.map(|m| m.0),
                    "productId": d.model.map(|m| m.1),
                    "connection": "bluetooth",
                    "battery": { "percent": c.percent, "level": c.level, "chargeState": c.charge },
                }))
            })
            .collect();
        json!({ "count": items.len(), "items": items })
    }

    fn adapter(&self) -> Value {
        json!({ "present": true, "powered": self.powered })
    }

    fn state(&self) -> Value {
        let devices: Vec<Value> = self
            .devices
            .iter()
            .map(|d| {
                json!({
                    "id": d.id,
                    "name": d.name,
                    "displayName": d.display_name,
                    "vendorId": d.model.map(|m| m.0),
                    "productId": d.model.map(|m| m.1),
                    "paired": d.paired,
                    "connected": d.connected,
                    "trusted": d.paired,
                    "gamepad": true,
                    "controllerId": d.controller.as_ref().filter(|_| d.connected).map(|c| c.id.clone()),
                })
            })
            .collect();
        json!({
            "powered": self.powered,
            "scanning": self.scanning,
            "devices": devices,
            "pairing": self.pairing.as_ref().map(|p| json!({
                "id": p.id, "attempt": p.attempt, "state": p.state, "error": p.error,
            })),
        })
    }

    /// Start searching until `deadline`. A search already running is
    /// extended to it and keeps its finds; only a fresh one starts the
    /// list over. Returns the search's generation and whether it is new.
    fn start_scan(&mut self, deadline: Instant) -> Result<(u64, bool), String> {
        if !self.powered {
            return Err(POWERED_OFF.into());
        }
        self.scan_deadline = Some(deadline);
        if self.scanning {
            return Ok((self.scan_generation, false));
        }
        // A new search lists only what it finds; paired devices stay.
        self.devices.retain(|d| d.paired);
        self.scanning = true;
        self.scan_generation += 1;
        Ok((self.scan_generation, true))
    }

    fn stop_scan(&mut self) {
        self.scanning = false;
        self.scan_deadline = None;
        self.scan_generation += 1;
    }

    /// One scripted find, if its scan is still the current one.
    fn find(&mut self, generation: u64, index: usize) -> bool {
        let Some(find) = FINDS.get(index) else {
            return false;
        };
        if generation != self.scan_generation || self.devices.iter().any(|d| d.id == find.id) {
            return false;
        }
        self.devices.push(Device {
            id: find.id.into(),
            name: find.name.map(Into::into),
            display_name: find.display_name.map(Into::into),
            model: find.model,
            paired: false,
            connected: false,
            pin: find.pin,
            controller: None,
        });
        true
    }

    /// Start pairing; returns whether the device wants a PIN.
    fn start_pair(&mut self, id: &str) -> Result<bool, String> {
        if !self.powered {
            return Err(POWERED_OFF.into());
        }
        let device = self
            .devices
            .iter()
            .find(|d| d.id == id)
            .ok_or_else(|| format!("unknown device: {id}"))?;
        let pin = device.pin;
        self.attempts += 1;
        self.pairing = Some(Pairing {
            id: id.into(),
            attempt: self.attempts,
            state: "pairing",
            error: None,
        });
        Ok(pin)
    }

    /// The attempt on `id` ends. Returns whether the controllers changed.
    fn finish_pair(&mut self, id: &str, pin: bool) -> bool {
        let Some(attempt) = self
            .pairing
            .as_ref()
            .filter(|p| p.id == id)
            .map(|p| p.attempt)
        else {
            return false;
        };
        if pin {
            self.pairing = Some(Pairing {
                id: id.into(),
                attempt,
                state: "failed",
                error: Some("pinRequired"),
            });
            return false;
        }
        let number = self.devices.len();
        let Some(device) = self.devices.iter_mut().find(|d| d.id == id) else {
            self.pairing = Some(Pairing {
                id: id.into(),
                attempt,
                state: "failed",
                error: Some("notFound"),
            });
            return false;
        };
        device.paired = true;
        device.connected = true;
        device.controller = Some(Controller {
            id: format!("input{}", 30 + number),
            percent: 55,
            level: "medium",
            charge: "discharging",
        });
        self.pairing = Some(Pairing {
            id: id.into(),
            attempt,
            state: "paired",
            error: None,
        });
        true
    }

    /// Returns whether a connected controller went with it.
    fn forget(&mut self, id: &str) -> Result<bool, String> {
        let at = self
            .devices
            .iter()
            .position(|d| d.id == id && d.paired)
            .ok_or_else(|| format!("not a paired device: {id}"))?;
        let removed = self.devices.remove(at);
        Ok(removed.connected && removed.controller.is_some())
    }
}

fn radio() -> &'static Mutex<Radio> {
    static RADIO: OnceLock<Mutex<Radio>> = OnceLock::new();
    RADIO.get_or_init(|| Mutex::new(Radio::new(mode() != Mode::Off)))
}

fn with<R>(f: impl FnOnce(&mut Radio) -> R) -> R {
    let mut guard = radio()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    f(&mut guard)
}

/// The methods the device answers. `None` leaves the request to the rest
/// of the mock, which answers "method not found" for an older Core.
pub fn route(method: &str, params: &Value, notifier: &Notifier) -> Option<Result<Value, String>> {
    if mode() == Mode::Absent {
        return None;
    }
    match method {
        "device.status" => Some(Ok(status())),
        "device.bluetooth.devices" => Some(Ok(with(|r| r.state()))),
        "device.bluetooth.scan.start" => Some(scan_start(params, notifier)),
        "device.bluetooth.scan.stop" => Some(Ok(with(|r| {
            r.stop_scan();
            r.state()
        }))),
        "device.bluetooth.pair" => Some(pair(params, notifier)),
        "device.bluetooth.forget" => Some(forget(params, notifier)),
        _ => None,
    }
}

fn status() -> Value {
    let actions: serde_json::Map<String, Value> = PAIR_ACTIONS
        .iter()
        .map(|a| ((*a).to_string(), json!("supported")))
        .collect();
    let (adapter, controllers) = with(|r| (r.adapter(), r.controllers()));
    json!({
        "capabilities": {
            "sections": {
                "power": "unsupported",
                "bluetooth": "supported",
                "controllers": "supported",
                "time": "supported",
            },
            "actions": actions,
        },
        "power": null,
        "network": null,
        "bluetooth": adapter,
        "storage": null,
        "display": null,
        "controllers": controllers,
        "time": { "clockReliable": true, "timezone": null, "utcOffset": 0 },
        "system": null,
    })
}

fn push_bluetooth(notifier: &Notifier) {
    notifier.send("device.bluetooth.changed", &with(|r| r.state()));
}

fn push_device(notifier: &Notifier) {
    let (adapter, controllers) = with(|r| (r.adapter(), r.controllers()));
    notifier.send(
        "device.changed",
        &json!({
            "power": null,
            "network": null,
            "bluetooth": adapter,
            "display": null,
            "controllers": controllers,
            "time": { "clockReliable": true },
        }),
    );
}

fn device_id(params: &Value) -> Result<String, String> {
    params
        .get("id")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())
        .map(str::to_string)
        .ok_or_else(|| "missing id".to_string())
}

fn scan_start(params: &Value, notifier: &Notifier) -> Result<Value, String> {
    let seconds = params
        .get("timeoutSeconds")
        .and_then(Value::as_u64)
        .unwrap_or(DEFAULT_SCAN_SECONDS)
        .clamp(1, 120);
    let deadline = Instant::now() + Duration::from_secs(seconds);
    let (generation, fresh) = with(|r| r.start_scan(deadline))?;
    if fresh {
        let finds = notifier.clone();
        tokio::spawn(async move {
            let mut elapsed = 0;
            for (index, find) in FINDS.iter().enumerate() {
                tokio::time::sleep(Duration::from_millis(find.at_ms - elapsed)).await;
                elapsed = find.at_ms;
                if with(|r| r.find(generation, index)) {
                    push_bluetooth(&finds);
                } else if with(|r| r.scan_generation != generation) {
                    return;
                }
            }
        });
        let stops = notifier.clone();
        tokio::spawn(async move {
            // Sleep to the deadline as it stands; a repeat start may have
            // moved it on in the meantime.
            while let Some(deadline) =
                with(|r| r.scan_deadline.filter(|_| r.scan_generation == generation))
            {
                if Instant::now() < deadline {
                    tokio::time::sleep_until(deadline).await;
                    continue;
                }
                with(Radio::stop_scan);
                push_bluetooth(&stops);
                return;
            }
        });
    }
    Ok(with(|r| r.state()))
}

fn pair(params: &Value, notifier: &Notifier) -> Result<Value, String> {
    let id = device_id(params)?;
    let pin = with(|r| r.start_pair(&id))?;
    let notifier = notifier.clone();
    let task_id = id.clone();
    tokio::spawn(async move {
        tokio::time::sleep(if pin { PIN_FAIL_TIME } else { PAIR_TIME }).await;
        let controllers_changed = with(|r| r.finish_pair(&task_id, pin));
        push_bluetooth(&notifier);
        if controllers_changed {
            push_device(&notifier);
        }
    });
    Ok(with(|r| r.state()))
}

fn forget(params: &Value, notifier: &Notifier) -> Result<Value, String> {
    let id = device_id(params)?;
    let controllers_changed = with(|r| r.forget(&id))?;
    push_bluetooth(notifier);
    if controllers_changed {
        push_device(notifier);
    }
    Ok(with(|r| r.state()))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, reason = "tests fail fast")]
mod tests {
    use super::*;

    #[test]
    fn status_lists_the_two_controllers_and_the_pairing_actions() {
        let radio = Radio::new(true);
        let controllers = radio.controllers();
        let items = controllers["items"].as_array().unwrap();
        assert_eq!(items.len(), 2);
        assert_eq!(items[0]["displayName"], "DualSense");
        assert_eq!(items[0]["vendorId"], "054c");
        assert!(items[1]["displayName"].is_null());
        assert_eq!(items[0]["battery"]["chargeState"], "charging");
        assert_eq!(items[0]["battery"]["percent"], 80);
        assert_eq!(items[1]["battery"]["level"], "low");
        assert_eq!(items[1]["battery"]["chargeState"], "discharging");
        let state = radio.state();
        assert_eq!(state["devices"][0]["controllerId"], "input17");
        assert_eq!(state["devices"][3]["displayName"], "DualShock 4");
        assert_eq!(state["devices"][3]["connected"], false);
        assert!(state["pairing"].is_null());
    }

    fn soon() -> Instant {
        Instant::now() + Duration::from_mins(1)
    }

    #[test]
    fn a_repeat_start_extends_the_search_and_keeps_its_finds() {
        let mut radio = Radio::new(true);
        let (generation, fresh) = radio.start_scan(soon()).unwrap();
        assert!(fresh);
        assert!(radio.find(generation, 0));
        let later = soon() + Duration::from_secs(30);
        let (again, fresh) = radio.start_scan(later).unwrap();
        assert!(!fresh);
        assert_eq!(again, generation, "the same search, so its script runs on");
        assert_eq!(radio.scan_deadline, Some(later));
        assert_eq!(radio.state()["devices"].as_array().unwrap().len(), 5);
        // Once it has stopped, a start begins a new list.
        radio.stop_scan();
        let (_, fresh) = radio.start_scan(soon()).unwrap();
        assert!(fresh);
        assert_eq!(radio.state()["devices"].as_array().unwrap().len(), 4);
    }

    #[test]
    fn a_search_finds_pairs_and_forgets() {
        let mut radio = Radio::new(true);
        let (generation, _) = radio.start_scan(soon()).unwrap();
        assert!(radio.scanning);
        assert!(radio.find(generation, 0));
        assert!(!radio.find(generation, 0), "found once");
        assert!(!radio.find(generation + 1, 1), "a stale scan finds nothing");
        let id = FINDS[0].id;
        assert!(!radio.start_pair(id).unwrap());
        assert_eq!(radio.state()["pairing"]["state"], "pairing");
        assert!(radio.finish_pair(id, false));
        assert_eq!(radio.state()["pairing"]["state"], "paired");
        assert_eq!(radio.controllers()["count"], 3);
        assert!(radio.forget(id).unwrap());
        assert_eq!(radio.controllers()["count"], 2);
        assert!(radio.forget(id).is_err());
        radio.stop_scan();
        assert!(!radio.scanning);
    }

    #[test]
    fn the_pin_device_always_fails() {
        let mut radio = Radio::new(true);
        let (generation, _) = radio.start_scan(soon()).unwrap();
        assert!(radio.find(generation, 2));
        let id = FINDS[2].id;
        assert!(radio.start_pair(id).unwrap());
        assert!(!radio.finish_pair(id, true));
        let state = radio.state();
        assert_eq!(state["pairing"]["state"], "failed");
        assert_eq!(state["pairing"]["error"], "pinRequired");
    }

    #[test]
    fn an_adapter_that_is_off_refuses_search_and_pairing() {
        let mut radio = Radio::new(false);
        assert_eq!(radio.start_scan(soon()).unwrap_err(), POWERED_OFF);
        assert_eq!(
            radio.start_pair("B0:00:00:00:00:01").unwrap_err(),
            POWERED_OFF
        );
    }
}
