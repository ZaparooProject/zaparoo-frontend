// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

//! The "Change launcher" picker, shared by the system default and the
//! per-media override: `Default` first, then the launchers Core offers
//! for that system, then the stored choice when it is no longer one of
//! them. Ported from `models/system_launchers.rs`.

/// The sentinel row that clears the stored choice.
pub const DEFAULT_LAUNCHER_ID: &str = "__default__";

/// One picker row: the id to store, plus the vocabulary key when the
/// row's text is ours rather than the launcher's own name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PickerRow {
    pub id: String,
    /// "launcher:default", "launcher:current", or empty for a launcher
    /// that speaks for itself.
    pub key: &'static str,
}

/// The rows for a launcher list and the currently stored choice
/// (`None` or the sentinel meaning "no override").
pub fn picker_rows(launcher_ids: &[String], current: Option<&str>) -> Vec<PickerRow> {
    let mut rows = vec![PickerRow {
        id: DEFAULT_LAUNCHER_ID.to_string(),
        key: "launcher:default",
    }];
    for id in launcher_ids {
        rows.push(PickerRow {
            id: id.clone(),
            key: "",
        });
    }
    // A launcher that was chosen and has since gone away still needs a
    // row, or the picker would silently show the wrong selection.
    if let Some(current) = current.filter(|c| !c.is_empty() && *c != DEFAULT_LAUNCHER_ID) {
        if !launcher_ids.iter().any(|id| id == current) {
            rows.push(PickerRow {
                id: current.to_string(),
                key: "launcher:current",
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

    fn ids(values: &[&str]) -> Vec<String> {
        values.iter().map(|v| (*v).to_string()).collect()
    }

    #[test]
    fn the_list_leads_with_default_then_the_system_launchers() {
        let rows = picker_rows(&ids(&["LauncherA", "LauncherB"]), None);
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].id, DEFAULT_LAUNCHER_ID);
        assert_eq!(rows[0].key, "launcher:default");
        assert_eq!(rows[1].id, "LauncherA");
        assert_eq!(rows[1].key, "");
        assert_eq!(picker_index(&rows, None), 0);
    }

    #[test]
    fn a_stored_launcher_that_is_gone_keeps_its_own_row() {
        let rows = picker_rows(&ids(&["LauncherA"]), Some("Retired"));
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[2].id, "Retired");
        assert_eq!(rows[2].key, "launcher:current");
        assert_eq!(picker_index(&rows, Some("Retired")), 2);
        // One that is still offered gets no extra row.
        let rows = picker_rows(&ids(&["LauncherA"]), Some("LauncherA"));
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
        let rows = picker_rows(&ids(&["LauncherA"]), Some(""));
        assert_eq!(rows.len(), 2);
        assert_eq!(picker_index(&rows, Some("")), 0);
    }
}
