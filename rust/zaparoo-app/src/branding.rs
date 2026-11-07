// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

//! The header logo setting: the Zaparoo logo, the user's own image, or no
//! logo. Changing it needs an Online account with the `custom_branding`
//! feature (Zaparoo Pro or a Zaparoo Warp subscription), which Online
//! reports through Core's `online.status`.
//!
//! The gate is honor-system. The saved setting applies at boot as saved,
//! with nothing cached and nothing waited for. It goes back to the Zaparoo
//! logo when Core says the device is not linked to an Online account, or
//! when a check that reached Online says the feature is absent; an error,
//! being offline, a linked device that was never checked, or a Core too
//! old to answer changes nothing.

/// The Online feature that allows a custom header logo.
pub const FEATURE: &str = "custom_branding";

/// The setting's values, in the order the picker offers them.
pub const HEADER_LOGOS: &[&str] = &["zaparoo", "custom", "none"];

/// The persisted default.
pub const DEFAULT: &str = "zaparoo";

/// The header logo setting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeaderLogo {
    Zaparoo,
    Custom,
    None,
}

impl HeaderLogo {
    /// A persisted token; anything unknown is the Zaparoo logo.
    pub fn parse(token: &str) -> Self {
        match token.trim() {
            "custom" => Self::Custom,
            "none" => Self::None,
            _ => Self::Zaparoo,
        }
    }

    pub fn token(self) -> &'static str {
        match self {
            Self::Zaparoo => "zaparoo",
            Self::Custom => "custom",
            Self::None => "none",
        }
    }
}

/// What the header shows for the saved setting before any image is read:
/// Custom with no image chosen is the Zaparoo logo.
pub fn shown(setting: &str, image: &str) -> HeaderLogo {
    match HeaderLogo::parse(setting) {
        HeaderLogo::Custom if image.trim().is_empty() => HeaderLogo::Zaparoo,
        logo => logo,
    }
}

/// Whether this account may change the header logo, as far as is known.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Entitlement {
    /// Never checked, or Core cannot say.
    #[default]
    Unknown,
    Yes,
    No,
}

/// The entitlement a successful `online.status` answer gives. A device
/// with no linked account has nothing that could grant the feature, so it
/// is No. A linked one is No only after a check that reached Online
/// (`checked_at` set) without the feature; a linked device that was never
/// checked says nothing either way.
pub fn entitlement(linked: bool, features: &[String], checked_at: Option<&str>) -> Entitlement {
    if !linked {
        Entitlement::No
    } else if features.iter().any(|f| f == FEATURE) {
        Entitlement::Yes
    } else if checked_at.is_some_and(|at| !at.trim().is_empty()) {
        Entitlement::No
    } else {
        Entitlement::Unknown
    }
}

/// The saved setting has to go back to the Zaparoo logo: Online said this
/// account does not have the feature.
pub fn reverts(setting: &str, entitlement: Entitlement) -> bool {
    entitlement == Entitlement::No && HeaderLogo::parse(setting) != HeaderLogo::Zaparoo
}

/// Settings rows that change the header logo; locked when the entitlement
/// is known to be absent.
pub fn needs_entitlement(id: &str) -> bool {
    matches!(id, "headerLogo" | "headerLogoImage")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn features(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn unknown_tokens_read_as_the_zaparoo_logo() {
        for token in HEADER_LOGOS {
            assert_eq!(HeaderLogo::parse(token).token(), *token);
        }
        assert_eq!(HeaderLogo::parse(""), HeaderLogo::Zaparoo);
        assert_eq!(HeaderLogo::parse("hidden"), HeaderLogo::Zaparoo);
        assert_eq!(DEFAULT, HeaderLogo::Zaparoo.token());
    }

    #[test]
    fn custom_without_an_image_shows_the_zaparoo_logo() {
        assert_eq!(shown("custom", ""), HeaderLogo::Zaparoo);
        assert_eq!(shown("custom", "/a.png"), HeaderLogo::Custom);
        assert_eq!(shown("none", ""), HeaderLogo::None);
        assert_eq!(shown("zaparoo", "/a.png"), HeaderLogo::Zaparoo);
    }

    #[test]
    fn only_a_completed_check_without_the_feature_says_no() {
        let at = Some("2026-01-01T00:00:00Z");
        assert_eq!(
            entitlement(true, &features(&["cloud_backup", FEATURE]), at),
            Entitlement::Yes
        );
        assert_eq!(
            entitlement(true, &features(&[FEATURE]), None),
            Entitlement::Yes
        );
        assert_eq!(
            entitlement(true, &features(&["cloud_backup"]), at),
            Entitlement::No
        );
        assert_eq!(entitlement(true, &[], at), Entitlement::No);
        assert_eq!(entitlement(true, &[], None), Entitlement::Unknown);
        assert_eq!(entitlement(true, &[], Some("")), Entitlement::Unknown);
    }

    #[test]
    fn an_unlinked_device_is_not_entitled() {
        assert_eq!(entitlement(false, &[], None), Entitlement::No);
        assert_eq!(
            entitlement(false, &features(&[FEATURE]), Some("2026-01-01T00:00:00Z")),
            Entitlement::No
        );
    }

    #[test]
    fn only_a_known_no_reverts_a_changed_logo() {
        assert!(reverts("custom", Entitlement::No));
        assert!(reverts("none", Entitlement::No));
        assert!(!reverts("zaparoo", Entitlement::No));
        assert!(!reverts("custom", Entitlement::Unknown));
        assert!(!reverts("none", Entitlement::Yes));
    }

    #[test]
    fn the_logo_rows_are_the_gated_ones() {
        assert!(needs_entitlement("headerLogo"));
        assert!(needs_entitlement("headerLogoImage"));
        assert!(!needs_entitlement("backgroundImage"));
    }
}
