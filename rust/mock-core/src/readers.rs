// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// Readers, tokens and mappings: one connected PN532 reader, one manual
// connection that fails to open, the driver catalog, candidate ports, the
// last token read and an editable mapping table. Shapes follow Core's
// `readers`, `readers.drivers`, `readers.ports`, `tokens.resolve` and
// `mappings*` methods.

use serde_json::{json, Map, Value};
use std::sync::{Mutex, MutexGuard, PoisonError};

use crate::media_state::Notifier;

/// The mock reader every token "arrives" on.
pub const READER_ID: &str = "mock-reader-1";

struct State {
    mappings: Vec<Value>,
    next_mapping_id: i64,
    hold: bool,
    writing: bool,
}

static STATE: Mutex<State> = Mutex::new(State {
    mappings: Vec::new(),
    next_mapping_id: 2,
    hold: false,
    writing: false,
});

fn state() -> MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(PoisonError::into_inner)
}

pub fn route(method: &str, params: &Value, notifier: &Notifier) -> Option<Result<Value, String>> {
    match method {
        "readers" => Some(Ok(readers_response())),
        "readers.drivers" => Some(Ok(drivers_response())),
        "readers.ports" => Some(Ok(ports_response(params))),
        "readers.write" => Some(Ok(write(params))),
        "readers.write.cancel" => Some(write_cancel()),
        "tokens.resolve" => Some(Ok(resolve_response())),
        "settings.zapscript.hold" => {
            state().hold = true;
            Some(Ok(Value::Null))
        }
        "mappings" => Some(Ok(mappings_response())),
        "mappings.new" => Some(Ok(mappings_new(params, notifier))),
        "mappings.update" => Some(mappings_update(params)),
        "mappings.delete" => Some(mappings_delete(params)),
        _ => None,
    }
}

fn readers_response() -> Value {
    json!({
        "readers": [
            {
                "id": "pn532:/dev/ttyUSB0",
                "readerId": READER_ID,
                "driver": "pn532",
                "info": "PN532 v1.6",
                "path": "/dev/ttyUSB0",
                "source": "auto",
                "scanMode": "tap",
                "capabilities": ["write"],
                "connected": true,
            },
            {
                "id": "simpleserial:/dev/ttyACM0",
                "readerId": "",
                "driver": "simpleserial",
                "info": "",
                "path": "/dev/ttyACM0",
                "source": "connect",
                "scanMode": "",
                "capabilities": [],
                "connected": false,
                "error": "no such device",
            },
        ],
    })
}

fn drivers_response() -> Value {
    let driver = |id: &str, description: &str, enabled: bool, auto: bool, can: bool, kind: &str| {
        json!({
            "id": id,
            "description": description,
            "defaultEnabled": enabled,
            "defaultAutoDetect": auto,
            "canAutoDetect": can,
            "display": false,
            "pathKind": kind,
        })
    };
    let mut optical = driver(
        "opticaldrive",
        "Optical drive CD/DVD reader",
        true,
        true,
        true,
        "device",
    );
    optical["idSources"] = json!(["uuid", "label", "merged"]);
    let mut oled = driver(
        "tty2oled",
        "TTY2OLED serial display device",
        false,
        false,
        true,
        "serial",
    );
    oled["display"] = json!(true);
    json!({
        "drivers": [
            driver("pn532", "PN532 NFC reader (UART/I2C/SPI)", true, true, true, "serial"),
            driver("ccid", "USB CCID NFC readers", true, true, true, "none"),
            driver("simpleserial", "Simple serial protocol reader", false, false, false, "serial"),
            driver("file", "File-based token reader", false, false, false, "text"),
            driver("mqtt", "MQTT client reader", false, false, false, "text"),
            optical,
            oled,
        ],
    })
}

fn ports_response(params: &Value) -> Value {
    match params.get("driver").and_then(Value::as_str) {
        Some("opticaldrive") => json!({"ports": [{"path": "/dev/sr0", "description": "DVD-RW"}]}),
        Some("file" | "mqtt") | None => json!({"ports": []}),
        Some(_) => json!({
            "ports": [
                {"path": "/dev/ttyUSB0", "description": "CP2102 USB to UART"},
                {"path": "/dev/ttyACM0"},
            ],
        }),
    }
}

fn write(params: &Value) -> Value {
    let text = params.get("text").and_then(Value::as_str).unwrap_or("");
    tracing::info!(zap_script = %text, "readers.write");
    state().writing = true;
    Value::Null
}

fn write_cancel() -> Result<Value, String> {
    let mut s = state();
    if !s.writing {
        return Err("no write in progress".into());
    }
    s.writing = false;
    Ok(Value::Null)
}

/// The token the mock pretends was last read: the one `events.rs` sends.
pub fn last_token() -> Value {
    json!({
        "type": "ntag",
        "uid": "04a1b2c3d4e5f6",
        "text": "",
        "data": "",
        "scanTime": "2026-04-29T23:00:00Z",
        "readerId": READER_ID,
    })
}

fn resolve_response() -> Value {
    let s = state();
    let mapping = s
        .mappings
        .iter()
        .find(|m| m["pattern"] == "04a1b2c3d4e5f6" && m["enabled"] == true);
    let mut out = json!({
        "token": last_token(),
        "redacted": false,
        "script": "",
    });
    if let Some(m) = mapping {
        out["script"] = m["override"].clone();
        out["mapping"] = json!({"id": m["id"], "source": m["source"], "label": m["label"]});
        out["media"] = json!({
            "system": "SNES",
            "systemName": "Super Nintendo",
            "path": "/media/fat/games/SNES/Super Mario World.sfc",
            "name": "Super Mario World",
        });
    }
    out
}

fn mappings_response() -> Value {
    let s = state();
    let mut all = vec![json!({
        "id": "1",
        "added": "2026-04-01T10:00:00Z",
        "label": "Arcade card",
        "type": "uid",
        "match": "exact",
        "pattern": "0499aabbccdd",
        "override": "**launch.system:arcade",
        "source": "file",
        "readOnly": true,
        "enabled": true,
    })];
    all.extend(s.mappings.iter().cloned());
    json!({"mappings": all})
}

fn mappings_new(params: &Value, notifier: &Notifier) -> Value {
    let mut s = state();
    let id = s.next_mapping_id;
    s.next_mapping_id += 1;
    let field = |k: &str| {
        params
            .get(k)
            .cloned()
            .unwrap_or(Value::String(String::new()))
    };
    s.mappings.push(json!({
        "id": id.to_string(),
        "added": "2026-04-30T10:00:00Z",
        "label": field("label"),
        "type": field("type"),
        "match": field("match"),
        "pattern": field("pattern"),
        "override": field("override"),
        "source": "database",
        "readOnly": false,
        "enabled": params.get("enabled").and_then(Value::as_bool).unwrap_or(true),
    }));
    drop(s);
    notifier.send("mappings.changed", &json!({}));
    json!({"id": id.to_string()})
}

fn mapping_index(s: &State, params: &Value) -> Result<usize, String> {
    let id = params
        .get("id")
        .and_then(Value::as_i64)
        .ok_or("id is required")?
        .to_string();
    s.mappings
        .iter()
        .position(|m| m["id"] == Value::String(id.clone()))
        .ok_or_else(|| format!("mapping {id} not found"))
}

fn mappings_update(params: &Value) -> Result<Value, String> {
    let mut s = state();
    let index = mapping_index(&s, params)?;
    let entry = &mut s.mappings[index];
    for key in ["label", "enabled", "override", "type", "match", "pattern"] {
        if let Some(v) = params.get(key) {
            entry[key] = v.clone();
        }
    }
    Ok(Value::Null)
}

fn mappings_delete(params: &Value) -> Result<Value, String> {
    let mut s = state();
    let index = mapping_index(&s, params)?;
    s.mappings.remove(index);
    Ok(Value::Null)
}

/// The new reader and scanning settings keys, with Core's defaults.
pub fn settings_defaults() -> Map<String, Value> {
    let mut map = Map::new();
    map.insert("readersScanIgnoreOnConnect".into(), json!(false));
    map.insert("mediaDefaultRegions".into(), json!(["us", "eu"]));
    map.insert("mediaDefaultLangs".into(), json!(["en"]));
    map.insert("readersDrivers".into(), json!({}));
    map.insert("launchGuardEnabled".into(), json!(false));
    map.insert("launchGuardTimeout".into(), json!(15.0));
    map.insert("launchGuardDelay".into(), json!(0.0));
    map.insert("launchGuardRequireConfirm".into(), json!(false));
    map.insert("profilesRequireForLaunch".into(), json!(false));
    map.insert("encryption".into(), json!(false));
    map
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_mapping_resolves_the_last_token() {
        let notifier = Notifier::noop();
        let params = json!({
            "label": "SMW", "type": "uid", "match": "exact",
            "pattern": "04a1b2c3d4e5f6", "override": "**launch:smw", "enabled": true,
        });
        mappings_new(&params, &notifier);
        let resolved = resolve_response();
        assert_eq!(resolved["script"], "**launch:smw");
        assert_eq!(resolved["media"]["name"], "Super Mario World");
    }

    #[test]
    fn cancelling_with_no_write_is_an_error() {
        state().writing = false;
        assert!(write_cancel().is_err());
        write(&json!({"text": "x"}));
        assert!(write_cancel().is_ok());
    }
}
