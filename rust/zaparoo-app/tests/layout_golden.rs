// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

//! Pins `zaparoo_app::layouts` to `BrowseLayouts.qml` at every geometry the
//! sizing fixture covers, for both themes and all six views.

#![allow(
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "tests should fail fast on a malformed fixture"
)]

use std::collections::BTreeMap;

use zaparoo_app::layouts::{self, ThemeId, View};
use zaparoo_app::sizing::{Inputs, InterfaceProfile};

const FIXTURE: &str = include_str!("../../../tests/fixtures/layout_golden.txt");

struct Case {
    theme: ThemeId,
    view: View,
    inputs: Inputs,
    tier: String,
    current_theme: ThemeId,
    expected: BTreeMap<String, String>,
}

fn parse(line: &str) -> Case {
    let mut fields = line.split(' ').skip(1);
    let mut head: BTreeMap<&str, &str> = BTreeMap::new();
    let mut expected = BTreeMap::new();
    for kv in fields.by_ref() {
        let (key, value) = kv.split_once('=').expect("key=value");
        if key.contains('.') {
            expected.insert(key.to_string(), value.to_string());
        } else {
            head.insert(key, value);
        }
    }
    let num = |key: &str| head[key].parse::<f64>().expect("number");
    let flag = |key: &str| head[key] == "1";
    Case {
        theme: ThemeId::from_name(head["theme"]).expect("theme"),
        view: View::from_name(head["view"]).expect("view"),
        inputs: Inputs {
            screen_width: num("screen_width"),
            screen_height: num("screen_height"),
            crt_native_path: flag("crt"),
            bitmap_type: false,
            swap_percentage_axes: flag("swap"),
            interface_profile: InterfaceProfile::Standard,
        },
        tier: head["tier"].to_string(),
        current_theme: ThemeId::from_name(head["currentTheme"]).expect("currentTheme"),
        expected,
    }
}

fn cases() -> Vec<Case> {
    FIXTURE
        .lines()
        .filter(|line| line.starts_with("GOLDEN "))
        .map(parse)
        .collect()
}

#[test]
fn fixture_covers_every_theme_and_view() {
    let cases = cases();
    assert_eq!(cases.len(), 48 * 2 * 6);
    for theme in [ThemeId::Default, ThemeId::Crt] {
        for view in [
            View::SystemsGrid,
            View::SystemsList,
            View::SystemsListTate,
            View::GamesGrid,
            View::GamesList,
            View::GamesListTate,
        ] {
            assert_eq!(
                cases
                    .iter()
                    .filter(|c| c.theme == theme && c.view == view)
                    .count(),
                48,
                "{theme:?}/{view:?}"
            );
        }
    }
}

#[test]
fn current_theme_matches_qml() {
    for case in cases() {
        assert_eq!(
            case.inputs.tier().as_str(),
            case.tier,
            "tier at {:?}",
            case.inputs
        );
        assert_eq!(
            ThemeId::current(&case.inputs),
            case.current_theme,
            "current theme at {:?}",
            case.inputs
        );
    }
}

#[test]
fn profiles_match_qml() {
    let mut failures = Vec::new();
    for case in cases() {
        let profile = layouts::profile(case.theme, case.view, &case.inputs);
        let actual: BTreeMap<String, String> = profile.flatten().into_iter().collect();
        if actual != case.expected {
            let mut diff = Vec::new();
            for key in actual.keys().chain(case.expected.keys()) {
                let a = actual.get(key);
                let e = case.expected.get(key);
                if a != e && !diff.iter().any(|d: &String| d.starts_with(key.as_str())) {
                    diff.push(format!("{key}: qml={e:?} rust={a:?}"));
                }
            }
            failures.push(format!(
                "{:?}/{:?} at {}x{} crt={} swap={}:\n  {}",
                case.theme,
                case.view,
                case.inputs.screen_width,
                case.inputs.screen_height,
                case.inputs.crt_native_path,
                case.inputs.swap_percentage_axes,
                diff.join("\n  ")
            ));
        }
        assert_eq!(
            profile.bottom_unsafe_height().to_string(),
            case.expected["footer.bottomUnsafeHeight"]
        );
    }
    assert!(
        failures.is_empty(),
        "{} profiles differ from the QML fixture:\n{}",
        failures.len(),
        failures.join("\n")
    );
}
