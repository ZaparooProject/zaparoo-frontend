// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// Profiles, paired clients and the global playtime limits. Two profiles to
// start with: an admin with PIN 1234 and a member without one; nobody is
// active. A switch card for verify-by-scan is the admin's.

use serde_json::{json, Value};
use std::sync::{Mutex, MutexGuard, PoisonError};

use crate::media_state::Notifier;

struct Profile {
    id: String,
    name: String,
    role: String,
    pin: Option<String>,
    switch_id: String,
    limits_enabled: Option<bool>,
    daily: Option<String>,
    session: Option<String>,
}

struct State {
    profiles: Vec<Profile>,
    next_id: u32,
    active: Option<String>,
    clients: Vec<Value>,
    limits: Value,
}

static STATE: Mutex<Option<State>> = Mutex::new(None);

fn with<T>(f: impl FnOnce(&mut State) -> T) -> T {
    let mut guard: MutexGuard<'_, Option<State>> =
        STATE.lock().unwrap_or_else(PoisonError::into_inner);
    let state = guard.get_or_insert_with(seed);
    f(state)
}

fn seed() -> State {
    State {
        profiles: vec![
            Profile {
                id: "1".into(),
                name: "Parent".into(),
                role: "admin".into(),
                pin: Some("1234".into()),
                switch_id: "sw-parent".into(),
                limits_enabled: None,
                daily: None,
                session: None,
            },
            Profile {
                id: "2".into(),
                name: "Sam".into(),
                role: "member".into(),
                pin: None,
                switch_id: "sw-sam".into(),
                limits_enabled: Some(true),
                daily: Some("2h".into()),
                session: None,
            },
        ],
        next_id: 3,
        active: None,
        clients: vec![
            json!({"clientId": "c1", "clientName": "Zaparoo App (Pixel 8)", "role": "admin", "createdAt": 1_775_000_000, "lastSeenAt": 1_777_000_000}),
            json!({"clientId": "c2", "clientName": "Living room tablet", "role": "member", "createdAt": 1_776_000_000, "lastSeenAt": 1_776_500_000}),
        ],
        limits: json!({"enabled": false, "daily": "3h", "session": "1h", "sessionReset": "20m", "warnings": ["5m", "1m"]}),
    }
}

pub fn route(method: &str, params: &Value, notifier: &Notifier) -> Option<Result<Value, String>> {
    match method {
        "profiles" => Some(Ok(with(
            |s| json!({"profiles": s.profiles.iter().map(profile_json).collect::<Vec<_>>()}),
        ))),
        "profiles.active" => Some(Ok(with(active_json))),
        "profiles.switch" => Some(switch(params, notifier)),
        "profiles.verify" => Some(verify(params)),
        "profiles.verify.cancel" => Some(Ok(Value::Null)),
        "profiles.new" => Some(new(params)),
        "profiles.update" => Some(update(params)),
        "profiles.delete" => Some(delete(params, notifier)),
        "clients" => Some(Ok(with(|s| json!({"clients": s.clients})))),
        "clients.delete" => Some(clients_delete(params)),
        "settings.playtime.limits" => Some(Ok(with(|s| s.limits.clone()))),
        "settings.playtime.limits.update" => Some(Ok(with(|s| {
            if let (Some(limits), Some(update)) = (s.limits.as_object_mut(), params.as_object()) {
                for (k, v) in update {
                    limits.insert(k.clone(), v.clone());
                }
            }
            Value::Null
        }))),
        _ => None,
    }
}

fn profile_json(p: &Profile) -> Value {
    json!({
        "profileId": p.id,
        "name": p.name,
        "role": p.role,
        "switchId": p.switch_id,
        "hasPin": p.pin.is_some(),
        "limitsEnabled": p.limits_enabled,
        "dailyLimit": p.daily,
        "sessionLimit": p.session,
    })
}

fn identity(p: &Profile) -> Value {
    json!({"profileId": p.id, "name": p.name, "role": p.role, "hasPin": p.pin.is_some()})
}

fn active_json(s: &mut State) -> Value {
    s.active
        .as_ref()
        .and_then(|id| s.profiles.iter().find(|p| &p.id == id))
        .map_or(Value::Null, identity)
}

fn str_param<'a>(params: &'a Value, key: &str) -> Option<&'a str> {
    params.get(key).and_then(Value::as_str)
}

fn check_pin(p: &Profile, pin: Option<&str>) -> Result<(), String> {
    match (&p.pin, pin) {
        (None, _) => Ok(()),
        (Some(want), Some(got)) if want == got => Ok(()),
        _ => Err("incorrect PIN".into()),
    }
}

fn switch(params: &Value, notifier: &Notifier) -> Result<Value, String> {
    let active = with(|s| {
        match str_param(params, "profileId") {
            None => s.active = None,
            Some(id) => {
                let p = s
                    .profiles
                    .iter()
                    .find(|p| p.id == id)
                    .ok_or("profile not found")?;
                // The profile's own switch card stands in for its PIN.
                if str_param(params, "switchId") != Some(p.switch_id.as_str()) {
                    check_pin(p, str_param(params, "pin"))?;
                }
                s.active = Some(id.to_string());
            }
        }
        Ok::<_, String>(active_json(s))
    })?;
    notifier.send("profiles.active", &json!({"profile": active}));
    if !active.is_null() {
        notifier.send(
            "profiles.data",
            &json!({"profileId": active["profileId"], "status": "applied", "reason": ""}),
        );
    }
    Ok(Value::Null)
}

fn verify(params: &Value) -> Result<Value, String> {
    with(|s| {
        if params.get("scan").and_then(Value::as_bool) == Some(true) {
            // The next token scanned is the admin's switch card.
            return s
                .profiles
                .iter()
                .find(|p| p.switch_id == "sw-parent")
                .map(identity)
                .ok_or_else(|| "not a switch card".to_string());
        }
        let id = str_param(params, "profileId").ok_or("profileId is required")?;
        let p = s
            .profiles
            .iter()
            .find(|p| p.id == id)
            .ok_or("profile not found")?;
        check_pin(p, str_param(params, "pin"))?;
        Ok(identity(p))
    })
}

fn new(params: &Value) -> Result<Value, String> {
    let name = str_param(params, "name")
        .filter(|n| !n.is_empty())
        .ok_or("name is required")?;
    Ok(with(|s| {
        let id = s.next_id.to_string();
        s.next_id += 1;
        let p = Profile {
            id: id.clone(),
            name: name.to_string(),
            role: str_param(params, "role").unwrap_or("member").to_string(),
            pin: str_param(params, "pin").map(str::to_string),
            switch_id: format!("sw-{id}"),
            limits_enabled: None,
            daily: None,
            session: None,
        };
        let out = profile_json(&p);
        s.profiles.push(p);
        out
    }))
}

fn update(params: &Value) -> Result<Value, String> {
    let id = str_param(params, "profileId").ok_or("profileId is required")?;
    with(|s| {
        let p = s
            .profiles
            .iter_mut()
            .find(|p| p.id == id)
            .ok_or("profile not found")?;
        let flag = |k: &str| params.get(k).and_then(Value::as_bool) == Some(true);
        if flag("clearPin") {
            p.pin = None;
        }
        if flag("clearLimits") {
            p.limits_enabled = None;
            p.daily = None;
            p.session = None;
        }
        if flag("regenerateSwitchId") {
            p.switch_id = format!("{}-new", p.switch_id);
        }
        if let Some(v) = str_param(params, "name") {
            p.name = v.to_string();
        }
        if let Some(v) = str_param(params, "pin") {
            p.pin = Some(v.to_string());
        }
        if let Some(v) = str_param(params, "role") {
            p.role = v.to_string();
        }
        if let Some(v) = params.get("limitsEnabled").and_then(Value::as_bool) {
            p.limits_enabled = Some(v);
        }
        if let Some(v) = str_param(params, "dailyLimit") {
            p.daily = Some(v.to_string());
        }
        if let Some(v) = str_param(params, "sessionLimit") {
            p.session = Some(v.to_string());
        }
        Ok(Value::Null)
    })
}

fn delete(params: &Value, notifier: &Notifier) -> Result<Value, String> {
    let id = str_param(params, "profileId").ok_or("profileId is required")?;
    let was_active = with(|s| {
        let before = s.profiles.len();
        s.profiles.retain(|p| p.id != id);
        if s.profiles.len() == before {
            return Err("profile not found".to_string());
        }
        let was_active = s.active.as_deref() == Some(id);
        if was_active {
            s.active = None;
        }
        Ok(was_active)
    })?;
    if was_active {
        notifier.send("profiles.active", &json!({"profile": null}));
    }
    Ok(Value::Null)
}

fn clients_delete(params: &Value) -> Result<Value, String> {
    let id = str_param(params, "clientId").ok_or("clientId is required")?;
    with(|s| {
        let before = s.clients.len();
        s.clients.retain(|c| c["clientId"] != id);
        if s.clients.len() == before {
            return Err("client not found".to_string());
        }
        Ok(Value::Null)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verify_checks_the_pin_and_returns_the_role() {
        assert!(verify(&json!({"profileId": "1", "pin": "0000"})).is_err());
        let ok = verify(&json!({"profileId": "1", "pin": "1234"}));
        assert_eq!(ok.ok().map(|v| v["role"].clone()), Some(json!("admin")));
        let scan = verify(&json!({"scan": true}));
        assert_eq!(scan.ok().map(|v| v["profileId"].clone()), Some(json!("1")));
    }
}
