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

use zaparoo_app::layouts::{self, Body, ThemeId, View};
use zaparoo_app::sizing::{Inputs, InterfaceProfile};

const FIXTURE: &str = include_str!("../../../tests/fixtures/layout_golden.txt");

/// The container insets this port deliberately leaves the QML behind on.
/// `BrowseLayouts.qml` derives a card's sides from the width and its ends
/// from the height, which puts 26px beside 14px at 720p and gets worse the
/// wider the screen; a container's inset is one margin seen four times, so
/// it measures the same on both axes (`docs/style.md` -> "Surface
/// containment"). `default_card_insets_are_square` owns these keys instead,
/// so the divergence is pinned somewhere rather than silently tolerated.
///
/// Only the sides move: `pct_min(p) == pct_h(p)` at every geometry in the
/// fixture, because `swap_percentage_axes` already points `pct_h` at the
/// short axis in portrait. Top and bottom must still match the QML.
const SQUARE_INSET_KEYS: [&str; 4] = [
    "list.cardPaddingLeft",
    "list.cardPaddingRight",
    "detail.panePaddingLeft",
    "detail.panePaddingRight",
];

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
        // The CRT tables keep their hand-calibrated pixel insets, so the
        // divergence is the default theme's alone.
        let diverges = case.theme == ThemeId::Default;
        if actual != case.expected {
            let mut diff = Vec::new();
            for key in actual.keys().chain(case.expected.keys()) {
                if diverges && SQUARE_INSET_KEYS.contains(&key.as_str()) {
                    continue;
                }
                let a = actual.get(key);
                let e = case.expected.get(key);
                if a != e && !diff.iter().any(|d: &String| d.starts_with(key.as_str())) {
                    diff.push(format!("{key}: qml={e:?} rust={a:?}"));
                }
            }
            if diff.is_empty() {
                continue;
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

/// What `SQUARE_INSET_KEYS` gives up in `profiles_match_qml`. Every default
/// card and pane inset is the same number on all four sides, and it is the
/// number the height axis was already giving, so nothing grew.
#[test]
fn default_card_insets_are_square() {
    let mut checked = 0;
    for case in cases() {
        if case.theme != ThemeId::Default {
            continue;
        }
        let profile = layouts::profile(case.theme, case.view, &case.inputs);
        let Body::List { list, detail, .. } = profile.body else {
            continue;
        };
        let pad = case.inputs.pct_min(2.0);
        let at = format!(
            "{:?} at {}x{} swap={}",
            case.view,
            case.inputs.screen_width,
            case.inputs.screen_height,
            case.inputs.swap_percentage_axes
        );
        for (name, value) in [
            ("cardPaddingLeft", list.card_padding_left),
            ("cardPaddingRight", list.card_padding_right),
            ("cardPaddingTop", list.card_padding_top),
            ("cardPaddingBottom", list.card_padding_bottom),
        ] {
            assert_eq!(value, pad, "list.{name} at {at}");
        }
        // The TATE detail pane is deliberately anisotropic -- a short wide
        // strip under the list, where vertical padding costs a metadata
        // row -- so only the upright pane is square.
        if matches!(case.view, View::SystemsList | View::GamesList) {
            for (name, value) in [
                ("panePaddingLeft", detail.pane_padding_left),
                ("panePaddingRight", detail.pane_padding_right),
                ("panePaddingTop", detail.pane_padding_top),
                ("panePaddingBottom", detail.pane_padding_bottom),
            ] {
                assert_eq!(value, pad, "detail.{name} at {at}");
            }
        }
        checked += 1;
    }
    // Four list views at every geometry the fixture covers.
    assert_eq!(checked, 48 * 4);
}
