// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// Parity gate for `zaparoo_app::sizing` against the QML it was ported from.
//
// `tests/fixtures/sizing_golden.txt` was dumped from `src/ui/theme/Sizing.qml`
// while that file still owned every formula, so this test compares the port
// against the shipped behaviour rather than against itself. 192 cases: 12
// scene sizes crossed with CRT path, bitmap type, interface profile and
// rotation. If a value here moves, the UI moved with it.
//
// The fixture is a flat `key=value` format on purpose. Parsing it with
// `split_whitespace` keeps `zaparoo-app` at zero dependencies, which is what
// makes `scripts/check-toolkit-free.sh` a one-line check.

#![allow(
    clippy::expect_used,
    clippy::unwrap_used,
    clippy::panic,
    reason = "tests should fail-fast on unexpected errors"
)]

use std::collections::BTreeMap;

use zaparoo_app::sizing::{
    declared_grid_shape, derive, detail_cover_source_size, games_grid_cover_box,
    games_grid_cover_source_size, games_grid_shape, max_expressible_cover_tier, snap_cover_tier,
    snap_logo_width, systems_grid_shape, GridKind, Inputs, InterfaceProfile,
};

const GOLDEN: &str = include_str!("../../../tests/fixtures/sizing_golden.txt");

fn fields(line: &str) -> BTreeMap<&str, &str> {
    line.split_whitespace()
        .skip(1)
        .map(|token| {
            token
                .split_once('=')
                .unwrap_or_else(|| panic!("malformed token {token:?}"))
        })
        .collect()
}

fn int(row: &BTreeMap<&str, &str>, key: &str) -> i32 {
    row.get(key)
        .unwrap_or_else(|| panic!("missing key {key:?}"))
        .parse()
        .unwrap_or_else(|_| panic!("key {key:?} is not an integer"))
}

fn flag(row: &BTreeMap<&str, &str>, key: &str) -> bool {
    int(row, key) != 0
}

fn inputs_of(row: &BTreeMap<&str, &str>) -> Inputs {
    Inputs {
        screen_width: f64::from(int(row, "screen_width")),
        screen_height: f64::from(int(row, "screen_height")),
        crt_native_path: flag(row, "crt"),
        bitmap_type: flag(row, "bitmap"),
        swap_percentage_axes: flag(row, "swap"),
        interface_profile: InterfaceProfile::from_name(
            row.get("profile").expect("missing profile"),
        ),
    }
}

fn label(row: &BTreeMap<&str, &str>) -> String {
    format!(
        "{}x{} crt={} bitmap={} swap={} profile={}",
        row["screen_width"],
        row["screen_height"],
        row["crt"],
        row["bitmap"],
        row["swap"],
        row["profile"]
    )
}

fn rows(marker: &str) -> impl Iterator<Item = BTreeMap<&'static str, &'static str>> + use<'_> {
    GOLDEN
        .lines()
        .filter(move |line| {
            line.split_whitespace()
                .next()
                .is_some_and(|first| first == marker)
        })
        .map(fields)
}

#[test]
fn every_derived_value_matches_the_qml() {
    let mut checked = 0usize;
    for row in rows("GOLDEN") {
        let inputs = inputs_of(&row);
        let d = derive(&inputs);
        let at = label(&row);

        let expect = |key: &str, actual: i32| {
            assert_eq!(actual, int(&row, key), "{key} at {at}");
        };

        expect("effectiveHeight", d.effective_height);
        expect("resolutionHeight", d.resolution_height);
        expect("radiusMd", d.radius_md);
        expect("radiusSm", d.radius_sm);
        expect("fontHero", d.font_hero);
        expect("fontTitle", d.font_title);
        expect("fontSection", d.font_section);
        expect("fontBody", d.font_body);
        expect("fontCaption", d.font_caption);
        expect("fontSmall", d.font_small);
        expect("cardBorderWidth", d.card_border_width);
        expect("focusBorderWidth", d.focus_border_width);
        expect("focusRingWidth", d.focus_ring_width);
        expect("pressEdgeHeight", d.press_edge_height);
        expect("helpBarHeight", d.help_bar_height);
        expect("helpBarClearance", d.help_bar_clearance);
        expect("visibleCovers", d.visible_covers);
        expect("hubGridColumns", d.hub_grid_columns);
        expect("hubGridRows", d.hub_grid_rows);
        expect("_hubActiveLabelHeight", d.hub_active_label_height);
        expect("_hubGridTopMargin", d.hub_grid_top_margin);
        expect("_hubGridHeightBudget", d.hub_grid_height_budget);
        expect("_hubGridSideInset", d.hub_grid_side_inset);
        expect("_hubGridColumnGap", d.hub_grid_column_gap);
        expect("_hubGridTopInset", d.hub_grid_top_inset);
        expect("_hubGridBottomInset", d.hub_grid_bottom_inset);
        expect("_hubGridRowGap", d.hub_grid_row_gap);
        expect("_hubGridWidthFit", d.hub_grid_width_fit);
        expect("_hubGridHeightFit", d.hub_grid_height_fit);
        expect("hubTileSize", d.hub_tile_size);
        expect("hubTileWidth", d.hub_tile_width);
        expect("hubTileHeight", d.hub_tile_height);
        expect("systemsGridColumns", d.systems_grid_columns);
        expect("systemsGridRows", d.systems_grid_rows);
        expect("gamesGridColumns", d.games_grid_columns);
        expect("gamesGridRows", d.games_grid_rows);
        expect("headerRowHeight", d.header_row_height);
        expect("headerStackGap", d.header_stack_gap);
        expect("headerTopMargin", d.header_top_margin);
        expect("headerSideMargin", d.header_side_margin);
        expect("headerHeight", d.header_height);
        expect("headerBottom", d.header_bottom);

        assert_eq!(d.tier.as_str(), row["tier"], "tier at {at}");
        assert_eq!(
            d.handheld_profile,
            flag(&row, "handheldProfile"),
            "handheldProfile at {at}"
        );
        assert_eq!(
            d.corner_antialiasing,
            flag(&row, "cornerAntialiasing"),
            "cornerAntialiasing at {at}"
        );

        checked += 1;
    }
    assert_eq!(checked, 192, "fixture case count changed");
}

#[test]
fn grid_shape_functions_match_the_qml() {
    for row in rows("GOLDEN") {
        let inputs = inputs_of(&row);
        let at = label(&row);
        let (w, h) = (inputs.screen_width, inputs.screen_height);
        // The dump probed both the full scene and the reduced viewport the
        // games grid actually gets after the header and footer strips.
        let reduced = f64::from((h * 0.68).round() as i32).max(1.0);

        let full = games_grid_shape(&inputs, w, h);
        assert_eq!(full.columns, int(&row, "gamesFullC"), "gamesFullC at {at}");
        assert_eq!(full.rows, int(&row, "gamesFullR"), "gamesFullR at {at}");

        let red = games_grid_shape(&inputs, w, reduced);
        assert_eq!(red.columns, int(&row, "gamesRedC"), "gamesRedC at {at}");
        assert_eq!(red.rows, int(&row, "gamesRedR"), "gamesRedR at {at}");

        let systems = systems_grid_shape(&inputs, w, h);
        assert_eq!(
            systems.columns,
            int(&row, "systemsFullC"),
            "systemsFullC at {at}"
        );
        assert_eq!(
            systems.rows,
            int(&row, "systemsFullR"),
            "systemsFullR at {at}"
        );

        for (kind, c_key, r_key) in [
            (GridKind::Games, "declaredGamesC", "declaredGamesR"),
            (GridKind::Systems, "declaredSystemsC", "declaredSystemsR"),
        ] {
            let declared = declared_grid_shape(&inputs, kind);
            let (c, r) = declared.map_or((-1, -1), |s| (s.columns, s.rows));
            assert_eq!(c, int(&row, c_key), "{c_key} at {at}");
            assert_eq!(r, int(&row, r_key), "{r_key} at {at}");
        }
    }
}

#[test]
fn cover_tier_functions_match_the_qml() {
    for row in rows("GOLDEN") {
        let inputs = inputs_of(&row);
        let at = label(&row);
        let (w, h) = (inputs.screen_width, inputs.screen_height);

        assert_eq!(
            games_grid_cover_box(&inputs, w, h),
            int(&row, "coverBox"),
            "coverBox at {at}"
        );
        assert_eq!(
            games_grid_cover_source_size(&inputs, w, h),
            int(&row, "gridCover"),
            "gridCover at {at}"
        );
        assert_eq!(
            detail_cover_source_size(&inputs, w, h),
            int(&row, "detailCover"),
            "detailCover at {at}"
        );
        assert_eq!(
            max_expressible_cover_tier(w),
            int(&row, "maxTier"),
            "maxTier at {at}"
        );
    }
}

#[test]
fn font_size_matches_the_qml() {
    for row in rows("GOLDEN") {
        let inputs = inputs_of(&row);
        let at = label(&row);
        for (percent, key) in [
            (2.0, "font20"),
            (2.6, "font26"),
            (4.0, "font40"),
            (10.0, "font100"),
        ] {
            assert_eq!(
                inputs.font_size(percent),
                int(&row, key),
                "fontSize({percent}) at {at}"
            );
        }
    }
}

#[test]
fn argument_only_ladders_match_the_qml() {
    let row = rows("GOLDENPURE").next().expect("GOLDENPURE line missing");
    let mut seen = 0usize;
    for (key, expected) in &row {
        let (name, arg) = key.split_once('_').expect("malformed ladder key");
        let px = f64::from(arg.parse::<i32>().expect("ladder arg is not an integer"));
        let expected: i32 = expected.parse().expect("ladder value is not an integer");
        let actual = match name {
            "coverTier" => snap_cover_tier(px),
            "logoWidth" => snap_logo_width(px),
            "maxTier" => max_expressible_cover_tier(px),
            other => panic!("unexpected ladder {other:?}"),
        };
        assert_eq!(actual, expected, "{name}({px})");
        seen += 1;
    }
    assert_eq!(seen, 48, "ladder probe count changed");
}

// The two argument pairs the surviving QML tests still call directly, so a
// change here shows up in both suites rather than only one.
#[test]
fn probe_pairs_match_the_qml() {
    let row = rows("GOLDENPROBE")
        .next()
        .expect("GOLDENPROBE line missing");

    let nonstandard = Inputs {
        screen_width: 1000.0,
        screen_height: 600.0,
        ..Inputs::default()
    };
    let shape = games_grid_shape(&nonstandard, 1000.0, 405.0);
    assert_eq!(shape.columns, int(&row, "nonstandard_1000x405_c"));
    assert_eq!(shape.rows, int(&row, "nonstandard_1000x405_r"));
    assert!(
        declared_grid_shape(&nonstandard, GridKind::Games).is_none(),
        "1000x600 is not a common digital scene, so it must be scored"
    );

    let half_1080 = Inputs {
        screen_width: 1280.0,
        screen_height: 720.0,
        ..Inputs::default()
    };
    let shape = games_grid_shape(&half_1080, 960.0, 365.0);
    assert_eq!(shape.columns, int(&row, "half1080_960x365_c"));
    assert_eq!(shape.rows, int(&row, "half1080_960x365_r"));
}
