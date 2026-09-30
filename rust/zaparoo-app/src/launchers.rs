// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

//! The "Change launcher" picker, shared by the system default and the
//! per-media override: `Default` first, then the launchers Core offers
//! for that system, then the stored choice when it is no longer one of
//! them.

/// The sentinel row that clears the stored choice.
pub const DEFAULT_LAUNCHER_ID: &str = "__default__";

/// One launcher's readiness, as far as the picker cares: enough to decide
/// whether the row can currently be used, and roughly why not. Mirrors the
/// `available`/`detected` fields of Core's `launchers` result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LauncherReadiness {
    pub id: String,
    pub available: bool,
    /// `None` means the platform never checks (unknown, not "missing").
    pub detected: Option<bool>,
    /// Group names this launcher belongs to. `systemDefaults.launcher` may
    /// name a group instead of a launcher id, matched against any member's
    /// groups - a stored group default must not read as a retired launcher
    /// while a member of that group is still offered.
    pub groups: Vec<String>,
}

/// Why a picker row can't currently be used. Meaningless when the row is
/// available; kept as a separate field rather than folded into a single
/// enum so `available` stays the one thing callers branch on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickerReason {
    /// Available, or a row that speaks for itself (Default).
    None,
    /// Core looked and it isn't installed.
    NotInstalled,
    /// Unavailable for some other reason, or a stored choice Core no
    /// longer lists for this system at all.
    Unavailable,
}

/// One picker row: the id to store, plus the vocabulary key when the
/// row's text is ours rather than the launcher's own name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PickerRow {
    pub id: String,
    /// "launcher:default", "launcher:current", or empty for a launcher
    /// that speaks for itself.
    pub key: &'static str,
    pub available: bool,
    pub reason: PickerReason,
}

/// The rows for a launcher list and the currently stored choice
/// (`None` or the sentinel meaning "no override"). A row stays even when
/// unavailable now — Core's own selection is the actual gate, and
/// unavailable now doesn't mean unavailable once the user acts on it.
pub fn picker_rows(launchers: &[LauncherReadiness], current: Option<&str>) -> Vec<PickerRow> {
    let mut rows = vec![PickerRow {
        id: DEFAULT_LAUNCHER_ID.to_string(),
        key: "launcher:default",
        available: true,
        reason: PickerReason::None,
    }];
    for launcher in launchers {
        let reason = if launcher.available {
            PickerReason::None
        } else if launcher.detected == Some(false) {
            PickerReason::NotInstalled
        } else {
            PickerReason::Unavailable
        };
        rows.push(PickerRow {
            id: launcher.id.clone(),
            key: "",
            available: launcher.available,
            reason,
        });
    }
    // A launcher that was chosen and has since gone away still needs a
    // row, or the picker would silently show the wrong selection. `current`
    // may instead be a group name still matched by an offered launcher
    // (`systemDefaults.launcher` accepts either), which is not "gone".
    if let Some(current) = current.filter(|c| !c.is_empty() && *c != DEFAULT_LAUNCHER_ID) {
        let still_offered = launchers
            .iter()
            .any(|l| l.id == current || l.groups.iter().any(|g| g == current));
        if !still_offered {
            rows.push(PickerRow {
                id: current.to_string(),
                key: "launcher:current",
                available: false,
                reason: PickerReason::Unavailable,
            });
        }
    }
    rows
}

/// Where the cursor starts: the stored choice, else `Default`.
pub fn picker_index(rows: &[PickerRow], current: Option<&str>) -> usize {
    let current = current
        .filter(|c| !c.is_empty())
        .unwrap_or(DEFAULT_LAUNCHER_ID);
    rows.iter().position(|row| row.id == current).unwrap_or(0)
}

/// What to store for a picked row: the sentinel clears the override.
pub fn stored_value(picked_id: &str) -> Option<String> {
    if picked_id == DEFAULT_LAUNCHER_ID || picked_id.is_empty() {
        None
    } else {
        Some(picked_id.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ready(id: &str) -> LauncherReadiness {
        LauncherReadiness {
            id: id.to_string(),
            available: true,
            detected: None,
            groups: Vec::new(),
        }
    }

    fn readiness(values: &[&str]) -> Vec<LauncherReadiness> {
        values.iter().map(|v| ready(v)).collect()
    }

    #[test]
    fn the_list_leads_with_default_then_the_system_launchers() {
        let rows = picker_rows(&readiness(&["LauncherA", "LauncherB"]), None);
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].id, DEFAULT_LAUNCHER_ID);
        assert_eq!(rows[0].key, "launcher:default");
        assert!(rows[0].available);
        assert_eq!(rows[1].id, "LauncherA");
        assert_eq!(rows[1].key, "");
        assert!(rows[1].available);
        assert_eq!(rows[1].reason, PickerReason::None);
        assert_eq!(picker_index(&rows, None), 0);
    }

    #[test]
    fn a_stored_launcher_that_is_gone_keeps_its_own_row() {
        let rows = picker_rows(&readiness(&["LauncherA"]), Some("Retired"));
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[2].id, "Retired");
        assert_eq!(rows[2].key, "launcher:current");
        assert!(!rows[2].available);
        assert_eq!(rows[2].reason, PickerReason::Unavailable);
        assert_eq!(picker_index(&rows, Some("Retired")), 2);
        // One that is still offered gets no extra row.
        let rows = picker_rows(&readiness(&["LauncherA"]), Some("LauncherA"));
        assert_eq!(rows.len(), 2);
        assert_eq!(picker_index(&rows, Some("LauncherA")), 1);
    }

    #[test]
    fn the_sentinel_clears_the_stored_choice() {
        assert_eq!(stored_value(DEFAULT_LAUNCHER_ID), None);
        assert_eq!(stored_value(""), None);
        assert_eq!(stored_value("LauncherA"), Some("LauncherA".to_string()));
    }

    #[test]
    fn an_empty_current_reads_as_default() {
        let rows = picker_rows(&readiness(&["LauncherA"]), Some(""));
        assert_eq!(rows.len(), 2);
        assert_eq!(picker_index(&rows, Some("")), 0);
    }

    #[test]
    fn an_undetected_launcher_is_marked_not_installed() {
        let launchers = vec![LauncherReadiness {
            id: "LauncherA".to_string(),
            available: false,
            detected: Some(false),
            groups: Vec::new(),
        }];
        let rows = picker_rows(&launchers, None);
        assert!(!rows[1].available);
        assert_eq!(rows[1].reason, PickerReason::NotInstalled);
    }

    #[test]
    fn a_stored_group_default_with_an_available_member_is_not_flagged_gone() {
        // `systemDefaults.launcher` may name a group, matched against any
        // launcher's `groups`, not just a launcher id - a stored "libretro"
        // must not read as a retired launcher when a libretro launcher is
        // still offered and available.
        let launchers = vec![LauncherReadiness {
            id: "LauncherA".to_string(),
            available: true,
            detected: None,
            groups: vec!["libretro".to_string()],
        }];
        let rows = picker_rows(&launchers, Some("libretro"));
        assert_eq!(
            rows.len(),
            2,
            "no extra row for a group the launcher belongs to"
        );
    }

    #[test]
    fn an_unavailable_launcher_with_unknown_detection_is_generic() {
        for detected in [None, Some(true)] {
            let launchers = vec![LauncherReadiness {
                id: "LauncherA".to_string(),
                available: false,
                detected,
                groups: Vec::new(),
            }];
            let rows = picker_rows(&launchers, None);
            assert!(!rows[1].available);
            assert_eq!(
                rows[1].reason,
                PickerReason::Unavailable,
                "detected={detected:?}"
            );
        }
    }
}
