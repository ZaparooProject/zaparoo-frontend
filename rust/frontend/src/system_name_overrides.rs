// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// User-supplied system display-name overrides.
//
// Populated once at startup from the `[custom.system_names]` table in
// `frontend.toml` (a `system_id = "Display Name"` map). A lookup here takes
// priority over the bundled Names_MiSTer localized data and the Core catalog
// name (see `models::systems::rows_for_category`).
//
// Keys are normalized with `system_names::normalize_key` (strip
// non-alphanumerics, lowercase) on both store and lookup, so `SNES`, `snes`,
// and `S.N.E.S` all resolve to the same override — the same forgiving rule
// the bundled name table already uses.

use std::collections::HashMap;
use std::hash::BuildHasher;
use std::sync::OnceLock;

use crate::system_names::normalize_key;

static OVERRIDES: OnceLock<HashMap<String, String>> = OnceLock::new();

/// Normalize a raw `id -> name` map into the lookup form (normalized keys,
/// blank entries dropped). Pure so it is unit-testable without globals.
/// Generic over the input hasher so callers can pass any `HashMap`.
fn normalize_map<S: BuildHasher>(raw: HashMap<String, String, S>) -> HashMap<String, String> {
    raw.into_iter()
        .filter_map(|(k, v)| {
            let key = normalize_key(&k);
            let name = v.trim().to_string();
            (!key.is_empty() && !name.is_empty()).then_some((key, name))
        })
        .collect()
}

/// Install the override map. Call exactly once during `zaparoo_rust_init`;
/// later calls are silent no-ops (`OnceLock` semantics).
pub fn set<S: BuildHasher>(raw: HashMap<String, String, S>) {
    let _ = OVERRIDES.set(normalize_map(raw));
}

/// Return the user override display name for `system_id`, or `None` when no
/// override is configured. Callers fall back to the localized/Core name.
pub fn lookup(system_id: &str) -> Option<String> {
    OVERRIDES.get()?.get(&normalize_key(system_id)).cloned()
}

#[cfg(test)]
mod tests {
    use super::normalize_map;
    use std::collections::HashMap;

    fn map_of(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
            .collect()
    }

    #[test]
    fn normalize_map_lowercases_and_strips_key() {
        let m = normalize_map(map_of(&[("SNES", "Super Nintendo")]));
        assert_eq!(m.get("snes").map(String::as_str), Some("Super Nintendo"));
    }

    #[test]
    fn normalize_map_matches_id_casing_variants() {
        // Both a config key of "PSX" and a lookup of "psx" land on the same
        // normalized slot.
        let m = normalize_map(map_of(&[("PSX", "PlayStation")]));
        assert_eq!(
            m.get(&super::normalize_key("psx")).map(String::as_str),
            Some("PlayStation")
        );
    }

    #[test]
    fn normalize_map_drops_blank_entries() {
        let m = normalize_map(map_of(&[("SNES", "   "), ("", "Orphan")]));
        assert!(m.is_empty());
    }

    #[test]
    fn normalize_map_trims_value() {
        let m = normalize_map(map_of(&[("Genesis", "  Mega Drive  ")]));
        assert_eq!(m.get("genesis").map(String::as_str), Some("Mega Drive"));
    }
}
