// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

//! Wire types for Core's profiles, paired clients and playtime limits.

use serde::{Deserialize, Serialize};

/// One profile (`profiles`). Limit fields left `None` inherit the global
/// limits; a `"0"` duration means unlimited.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Profile {
    pub profile_id: String,
    #[serde(default)]
    pub name: String,
    /// "admin" or "member".
    #[serde(default)]
    pub role: String,
    /// The bearer credential a switch card carries.
    #[serde(default)]
    pub switch_id: String,
    #[serde(default)]
    pub has_pin: bool,
    #[serde(default)]
    pub limits_enabled: Option<bool>,
    #[serde(default)]
    pub daily_limit: Option<String>,
    #[serde(default)]
    pub session_limit: Option<String>,
}

impl Profile {
    pub fn is_admin(&self) -> bool {
        self.role == "admin"
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ProfilesResult {
    #[serde(default)]
    pub profiles: Vec<Profile>,
}

/// The active profile (`profiles.active`, method and notification).
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ActiveProfile {
    pub profile_id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub role: String,
    #[serde(default)]
    pub has_pin: bool,
}

/// `profiles.active`: `None` is the shared profile.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct ActiveProfileResult {
    #[serde(default)]
    pub profile: Option<ActiveProfile>,
}

/// The kiosk lock (`kiosk`, and the `kiosk.state` notification), as far
/// as the frontend reads it.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct KioskState {
    /// `off`, `locked` or `unlocked`.
    #[serde(default)]
    pub state: String,
    /// Present whenever kiosk mode is on.
    #[serde(default)]
    pub settings: Option<KioskSettings>,
}

impl KioskState {
    /// Locked now. Off and a temporary unlock are not.
    pub fn is_locked(&self) -> bool {
        self.state == "locked"
    }

    /// The active profile may change between non-admin profiles while
    /// locked.
    pub fn profile_switching(&self) -> bool {
        self.settings.as_ref().is_some_and(|s| s.profile_switching)
    }
}

/// The kiosk settings the frontend reads.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct KioskSettings {
    #[serde(default)]
    pub profile_switching: bool,
}

/// `profiles.data`: how swapping a profile's data went.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProfileDataStatus {
    #[serde(default)]
    pub profile_id: String,
    /// "applied", "deferred", "failed" or "unavailable".
    #[serde(default)]
    pub status: String,
    #[serde(default)]
    pub reason: String,
}

/// `profiles.verify`: who a credential belongs to. Grants nothing.
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct VerifiedProfile {
    pub profile_id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub role: String,
    #[serde(default)]
    pub has_pin: bool,
}

/// `profiles.verify`: a profile and its PIN, or the next scanned switch card.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VerifyProfileParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pin: Option<String>,
    /// A switch card's ID, in place of a profile and its PIN.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub switch_id: Option<String>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub scan: bool,
}

/// `profiles.switch`: a profile (with its PIN when it has one), or neither
/// to leave for the shared profile.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SwitchProfileParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub profile_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pin: Option<String>,
    /// A switch card's ID, in place of the PIN.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub switch_id: Option<String>,
}

/// The `ZapScript` a profile's switch card carries.
pub fn switch_card_script(switch_id: &str) -> String {
    format!("**profile:{switch_id}")
}

/// `profiles.new`.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NewProfileParams {
    pub name: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pin: Option<String>,
}

/// `profiles.update`: only the fields set change.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
#[allow(clippy::struct_excessive_bools, reason = "Core's wire shape")]
pub struct UpdateProfileParams {
    pub profile_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pin: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limits_enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub daily_limit: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_limit: Option<String>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub clear_pin: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub clear_limits: bool,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub regenerate_switch_id: bool,
}

/// One paired device (`clients`).
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PairedClient {
    pub client_id: String,
    #[serde(default)]
    pub client_name: String,
    #[serde(default)]
    pub role: String,
    #[serde(default)]
    pub created_at: i64,
    #[serde(default)]
    pub last_seen_at: i64,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ClientsResult {
    #[serde(default)]
    pub clients: Vec<PairedClient>,
}

/// The global playtime limits (`settings.playtime.limits`). Durations are
/// Go duration strings ("1h30m").
#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PlaytimeLimits {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub daily: Option<String>,
    #[serde(default)]
    pub session: Option<String>,
    #[serde(default)]
    pub session_reset: Option<String>,
    #[serde(default)]
    pub warnings: Vec<String>,
}

/// `settings.playtime.limits.update`: only the fields set change.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdatePlaytimeLimitsParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub daily: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_reset: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn verify_by_scan_sends_only_scan() -> Result<(), serde_json::Error> {
        let v = serde_json::to_value(VerifyProfileParams {
            scan: true,
            ..Default::default()
        })?;
        assert_eq!(v, json!({"scan": true}));
        Ok(())
    }

    #[test]
    fn leaving_a_profile_sends_nothing() -> Result<(), serde_json::Error> {
        let v = serde_json::to_value(SwitchProfileParams::default())?;
        assert_eq!(v, json!({}));
        Ok(())
    }

    #[test]
    fn kiosk_reads_lock_and_switching() -> Result<(), serde_json::Error> {
        let k: KioskState = serde_json::from_value(json!({
            "state": "locked",
            "settings": {"preset": "family", "profileSwitching": true},
            "seal": {"sealed": false, "unsealed": []}
        }))?;
        assert!(k.is_locked());
        assert!(k.profile_switching());
        let off: KioskState = serde_json::from_value(json!({"state": "off", "seal": {}}))?;
        assert!(!off.is_locked());
        assert!(!off.profile_switching());
        Ok(())
    }

    #[test]
    fn shared_profile_reads_as_none() -> Result<(), serde_json::Error> {
        let r: ActiveProfileResult = serde_json::from_value(json!({"profile": null}))?;
        assert_eq!(r.profile, None);
        Ok(())
    }
}
