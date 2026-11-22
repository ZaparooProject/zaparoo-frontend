// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// Backups: the status Settings > Online reads, and three local backups to
// list, inspect and delete. The cloud side is unlinked, as a fresh Core's
// is, so nothing here needs an account.

use serde_json::{json, Value};
use std::sync::{LazyLock, Mutex, MutexGuard, PoisonError};

struct Local {
    name: &'static str,
    created_at: &'static str,
    size: i64,
    partial: bool,
}

static LOCAL: LazyLock<Mutex<Vec<Local>>> = LazyLock::new(|| {
    Mutex::new(vec![
        Local {
            name: "zaparoo-backup-20261019-210512.zip",
            created_at: "2026-10-19T21:05:12Z",
            size: 4_404_019,
            partial: false,
        },
        Local {
            name: "zaparoo-backup-20261012-093001.zip",
            created_at: "2026-10-12T09:30:01Z",
            size: 4_198_400,
            partial: true,
        },
        Local {
            name: "zaparoo-backup-20261003-180044.zip",
            created_at: "2026-10-03T18:00:44Z",
            size: 3_870_720,
            partial: false,
        },
    ])
});

fn local() -> MutexGuard<'static, Vec<Local>> {
    LOCAL.lock().unwrap_or_else(PoisonError::into_inner)
}

pub fn route(method: &str, params: &Value) -> Option<Result<Value, String>> {
    match method {
        "settings.backup.status" => Some(Ok(status())),
        "settings.backup.list" => Some(Ok(Value::Array(
            local()
                .iter()
                .map(|b| json!({"name": b.name, "createdAt": b.created_at, "size": b.size}))
                .collect(),
        ))),
        "settings.backup.inspect" => Some(inspect(params)),
        "settings.backup.delete" => Some(delete(params)),
        _ => None,
    }
}

fn status() -> Value {
    let newest = local().first().map(|b| (b.created_at, b.size));
    json!({
        "local": {
            "enabled": true,
            "lastStatus": if newest.is_some() { "success" } else { "never" },
            "lastBackupSize": newest.map_or(0, |(_, size)| size),
            "lastRunAt": newest.map(|(at, _)| at),
            "lastSuccessAt": newest.map(|(at, _)| at),
        },
        "remote": {
            "enabled": false,
            "lastStatus": "never",
            "lastBackupSize": 0,
            "schedule": "daily",
            "autoBackup": true,
            "linked": false,
            "availability": "unknown",
        },
    })
}

fn name(params: &Value) -> Result<&str, String> {
    params
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| "name is required".to_string())
}

fn inspect(params: &Value) -> Result<Value, String> {
    let name = name(params)?;
    let list = local();
    let backup = list
        .iter()
        .find(|b| b.name == name)
        .ok_or_else(|| format!("backup not found: {name}"))?;
    let category =
        |files: i64, bytes: i64| json!({"files": files, "bytes": bytes, "enabled": true});
    let warnings: Vec<Value> = if backup.partial {
        vec![
            json!({"path": "saves/PSX/memcard1.mcd", "reason": "file busy"}),
            json!({"path": "saves/SNES/Chrono.sav", "reason": "file busy"}),
        ]
    } else {
        Vec::new()
    };
    Ok(json!({
        "name": backup.name,
        "createdAt": backup.created_at,
        "size": backup.size,
        "status": if backup.partial { "partial" } else { "success" },
        "integrity": "valid",
        "categories": {
            "zaparoo": category(4, 1_310_720),
            "settings": category(3, 24_576),
            "inputs": category(12, 49_152),
            "saves": category(41, 2_936_012),
            "savestates": {"files": 0, "bytes": 0, "enabled": false},
        },
        "warnings": warnings,
    }))
}

fn delete(params: &Value) -> Result<Value, String> {
    let name = name(params)?;
    let mut list = local();
    let before = list.len();
    list.retain(|b| b.name != name);
    if list.len() == before {
        return Err(format!("backup not found: {name}"));
    }
    Ok(Value::Null)
}
