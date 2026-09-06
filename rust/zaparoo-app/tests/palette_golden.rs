// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// `tests/fixtures/palette_golden.txt` was dumped from `ColorSchemes.qml`
// before the palette moved to Rust. Every role of every preset at every
// intensity, plus the fallback and preview helpers, must match to the hex.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "tests should fail fast on a malformed fixture"
)]

use std::collections::HashMap;
use zaparoo_app::palette;

const GOLDEN: &str = include_str!("../../../tests/fixtures/palette_golden.txt");

fn fields(line: &str) -> (String, HashMap<String, String>) {
    let mut parts = line.split(' ');
    let kind = parts.next().expect("line kind").to_string();
    let map = parts
        .map(|field| {
            let (key, value) = field.split_once('=').expect("key=value field");
            (key.to_string(), value.to_string())
        })
        .collect();
    (kind, map)
}

fn cases(kind: &str) -> Vec<HashMap<String, String>> {
    GOLDEN
        .lines()
        .filter(|line| !line.starts_with('#') && !line.is_empty())
        .map(fields)
        .filter(|(k, _)| k == kind)
        .map(|(_, map)| map)
        .collect()
}

#[test]
fn catalog_matches_the_qml() {
    let catalog = cases("catalog");
    assert_eq!(catalog.len(), 1);
    let catalog = &catalog[0];
    assert_eq!(catalog["defaultId"], palette::DEFAULT_ID);
    assert_eq!(catalog["defaultIntensity"], palette::DEFAULT_INTENSITY);
    let ids: Vec<&str> = palette::ids().collect();
    assert_eq!(catalog["ids"], ids.join(","));
}

#[test]
fn fallback_and_preview_match_the_qml() {
    let effective = cases("effective");
    assert_eq!(effective.len(), 20);
    for case in effective {
        let id = case["id"].as_str();
        assert_eq!(
            palette::effective_id(id),
            case["value"],
            "effective id for {id}"
        );
        assert_eq!(
            palette::is_light_surface(id).to_string(),
            case["light"],
            "light surface for {id}"
        );
        let preview: Vec<String> = palette::preview_colors(id)
            .iter()
            .map(|color| color.hex())
            .collect();
        assert_eq!(
            preview.join(","),
            case["preview"],
            "preview colors for {id}"
        );
    }
}

#[test]
fn every_role_of_every_preset_matches_the_qml() {
    let palettes = cases("palette");
    assert_eq!(palettes.len(), 60);
    let mut mismatches = Vec::new();
    for case in &palettes {
        let id = case["id"].as_str();
        let intensity = case["intensity"].as_str();
        let computed = palette::palette(id, intensity);
        for (role, hex) in computed.roles() {
            let expected = &case[role];
            if &hex != expected {
                mismatches.push(format!(
                    "{id}/{intensity} {role}: qml {expected}, rust {hex}"
                ));
            }
        }
    }
    assert!(
        mismatches.is_empty(),
        "{} of {} role values differ:\n{}",
        mismatches.len(),
        palettes.len() * 27,
        mismatches.join("\n")
    );
}
