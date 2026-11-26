// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

//! Wire types for Core's readers, tokens and mappings methods.

use serde::{Deserialize, Serialize};

/// One manual reader connection (`readersConnect`).
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ReaderConnection {
    /// `None` and `Some(true)` both mean enabled.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    pub driver: String,
    #[serde(default)]
    pub path: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub id_source: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub scan_mode: String,
}

impl ReaderConnection {
    pub fn is_enabled(&self) -> bool {
        self.enabled.unwrap_or(true)
    }
}

/// One driver's entry in `readersDrivers`. A field left `None` runs on the
/// driver's default. An update replaces Core's whole list, so the fields
/// this frontend does not edit (scan mode, the optical drive's options) are
/// kept as Core sent them and go back unchanged.
#[derive(Debug, Clone, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DriverConfig {
    pub id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub auto_detect: Option<bool>,
    #[serde(flatten)]
    pub other: serde_json::Map<String, serde_json::Value>,
}

/// A driver ID as Core compares it: case folded, underscores dropped.
fn normalize_driver_id(id: &str) -> String {
    id.replace('_', "").to_lowercase()
}

/// The index of a driver's entry: one with its exact ID, else the first
/// whose ID Core reads as the same driver.
fn driver_index(list: &[DriverConfig], id: &str) -> Option<usize> {
    list.iter().position(|d| d.id == id).or_else(|| {
        let id = normalize_driver_id(id);
        list.iter().position(|d| normalize_driver_id(&d.id) == id)
    })
}

/// A driver's entry in a `readersDrivers` list.
pub fn find_driver<'a>(list: &'a [DriverConfig], id: &str) -> Option<&'a DriverConfig> {
    driver_index(list, id).map(|i| &list[i])
}

/// Sets a driver's `enabled` and `autoDetect` in a `readersDrivers` list,
/// keeping the entry's other fields and every other driver's entry, and
/// adding an entry when the driver has none.
pub fn set_driver(
    list: &mut Vec<DriverConfig>,
    id: &str,
    enabled: Option<bool>,
    auto_detect: Option<bool>,
) {
    let index = driver_index(list, id).unwrap_or_else(|| {
        list.push(DriverConfig {
            id: id.to_string(),
            ..DriverConfig::default()
        });
        list.len() - 1
    });
    list[index].enabled = enabled;
    list[index].auto_detect = auto_detect;
}

/// One supported driver (`readers.drivers`).
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[allow(
    clippy::struct_excessive_bools,
    reason = "independent facts Core reports about the driver"
)]
pub struct DriverInfo {
    pub id: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub default_enabled: bool,
    #[serde(default)]
    pub default_auto_detect: bool,
    /// Whether the driver really finds its readers by itself.
    #[serde(default)]
    pub can_auto_detect: bool,
    /// A display Core drives, not a reader.
    #[serde(default)]
    pub display: bool,
    /// "serial", "device", "text" or "none".
    #[serde(default)]
    pub path_kind: String,
    #[serde(default)]
    pub id_sources: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct DriversResult {
    #[serde(default)]
    pub drivers: Vec<DriverInfo>,
}

/// A candidate path for a driver (`readers.ports`).
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct PortInfo {
    pub path: String,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct PortsResult {
    #[serde(default)]
    pub ports: Vec<PortInfo>,
}

/// A token as Core reports it: text and data are redacted where they
/// carry a credential.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TokenInfo {
    #[serde(default)]
    pub scan_time: String,
    #[serde(default, rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub uid: String,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub data: String,
    #[serde(default)]
    pub reader_id: String,
}

/// The mapping a token matched.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct ResolvedMapping {
    pub id: String,
    /// "database" or "file".
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub label: String,
}

/// The game a token resolves to.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedMedia {
    #[serde(default)]
    pub system: String,
    #[serde(default)]
    pub system_name: String,
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub name: String,
}

/// `tokens.resolve`: what the last token read would do, without doing it.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct TokenResolution {
    pub token: TokenInfo,
    #[serde(default)]
    pub redacted: bool,
    #[serde(default)]
    pub script: String,
    #[serde(default)]
    pub mapping: Option<ResolvedMapping>,
    #[serde(default)]
    pub media: Option<ResolvedMedia>,
}

/// One token mapping (`mappings`).
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Mapping {
    pub id: String,
    #[serde(default)]
    pub added: String,
    #[serde(default)]
    pub label: String,
    #[serde(default, rename = "type")]
    pub kind: String,
    #[serde(default, rename = "match")]
    pub match_kind: String,
    #[serde(default)]
    pub pattern: String,
    #[serde(default)]
    pub r#override: String,
    /// "database" or "file".
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub read_only: bool,
    #[serde(default)]
    pub enabled: bool,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct MappingsResult {
    #[serde(default)]
    pub mappings: Vec<Mapping>,
}

/// `mappings.new`.
#[derive(Debug, Clone, Default, Serialize)]
pub struct NewMappingParams {
    pub label: String,
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(rename = "match")]
    pub match_kind: String,
    pub pattern: String,
    pub r#override: String,
    pub enabled: bool,
}

/// `mappings.update`: only the fields set change.
#[derive(Debug, Clone, Default, Serialize)]
pub struct UpdateMappingParams {
    pub id: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub r#override: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn resolution_reads_the_contract() -> Result<(), serde_json::Error> {
        let r: TokenResolution = serde_json::from_value(json!({
            "token": {"scanTime": "t", "type": "ntag", "uid": "04a1", "text": "", "data": "", "readerId": "r1"},
            "redacted": false,
            "script": "**launch:snes/smw.sfc",
            "mapping": {"id": "3", "source": "database", "label": "SMW"},
            "media": {"system": "SNES", "systemName": "Super Nintendo", "path": "snes/smw.sfc", "name": "Super Mario World"}
        }))?;
        assert_eq!(r.token.reader_id, "r1");
        assert_eq!(r.mapping.map(|m| m.id), Some("3".into()));
        assert_eq!(r.media.map(|m| m.name), Some("Super Mario World".into()));
        Ok(())
    }

    #[test]
    fn driver_overrides_omit_defaults() -> Result<(), serde_json::Error> {
        let v = serde_json::to_value(DriverConfig {
            id: "pn532".into(),
            enabled: Some(true),
            ..DriverConfig::default()
        })?;
        assert_eq!(v, json!({"id": "pn532", "enabled": true}));
        Ok(())
    }

    #[test]
    fn set_driver_keeps_every_other_field() -> Result<(), serde_json::Error> {
        let mut list: Vec<DriverConfig> = serde_json::from_value(json!([
            {"id": "optical", "keepReading": true, "launchOrder": "disc_first", "audioCdLauncher": "Mpv"},
            {"id": "PN532", "enabled": false, "scanMode": "hold"}
        ]))?;
        set_driver(&mut list, "pn532", Some(true), None);
        set_driver(&mut list, "ccid", Some(false), Some(false));
        assert_eq!(
            serde_json::to_value(&list)?,
            json!([
                {"id": "optical", "keepReading": true, "launchOrder": "disc_first", "audioCdLauncher": "Mpv"},
                {"id": "PN532", "enabled": true, "scanMode": "hold"},
                {"id": "ccid", "enabled": false, "autoDetect": false}
            ])
        );
        assert_eq!(
            find_driver(&list, "pn532").and_then(|d| d.enabled),
            Some(true)
        );
        assert!(find_driver(&list, "file").is_none());
        Ok(())
    }

    #[test]
    fn new_mapping_uses_core_names() -> Result<(), serde_json::Error> {
        let v = serde_json::to_value(NewMappingParams {
            label: "SMW".into(),
            kind: "id".into(),
            match_kind: "exact".into(),
            pattern: "04a1".into(),
            r#override: "**launch:x".into(),
            enabled: true,
        })?;
        assert_eq!(v["type"], "id");
        assert_eq!(v["match"], "exact");
        assert_eq!(v["override"], "**launch:x");
        Ok(())
    }
}
