// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// Offline layout checker: renders the App component with the software
// renderer (the exact pipeline the MiSTer build presents through) at a
// given resolution and writes a PNG. Drives the same window-resize
// path as the device, so Sizing/`changed` plumbing bugs reproduce here.
//
// Usage: snapshot <width> <height> <out.png> [screen]

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "offline dev tool; fail-fast is the right behavior"
)]
#![allow(
    clippy::print_stdout,
    reason = "snapshot CLI reports rendered files and probe results"
)]

use std::cell::RefCell;
use std::rc::Rc;
use std::time::Duration;

use slint::platform::software_renderer::{
    MinimalSoftwareWindow, PremultipliedRgbaColor, RenderingRotation, RepaintBufferType,
};
use slint::platform::{Platform, WindowAdapter};
use slint::ComponentHandle;

#[allow(
    unused_qualifications,
    missing_debug_implementations,
    clippy::pedantic,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::todo,
    clippy::unimplemented,
    clippy::undocumented_unsafe_blocks,
    reason = "slint-generated code"
)]
mod generated {
    slint::include_modules!();
}
use generated::{
    ActionStatus, AppCue, ControlKind, DialogButton, DialogKind, DisabledReason, ErrorKind,
    GamesMode, LogPhase, OnlineLinkPhase, Orientation, PairPhase, RowKind, ScopeKind, Screen,
    SettingsPage, SetupKind, SetupPicker, StatusKind, SystemsMode, VideoStandard,
};
use generated::{App, Brand, GlyphSource, GridCell, LetterBucket, MenuEntry, Sizing, Theme};
#[allow(
    unused_imports,
    reason = "reached through crate:: paths from the shared sizing adapter"
)]
use generated::{GamesView, Layout, Motion, Shell, SystemsView};
use generated::{KeyCell, KeyKind, SearchPane, SearchPaneRow, SearchView, SearchZone};
#[path = "../state_types.rs"]
#[allow(
    dead_code,
    reason = "shared enum boundary adapters; snapshots use only fixture state"
)]
mod state_types;

#[path = "../brand.rs"]
mod brand;
#[path = "../fonts.rs"]
mod fonts;
#[path = "../glyphs.rs"]
mod glyphs;
#[path = "../keyboard.rs"]
#[allow(
    dead_code,
    reason = "the app's keyboard state; the snapshot tool only lays keys out"
)]
mod keyboard;
#[path = "../qr.rs"]
#[allow(dead_code, reason = "the snapshot tool renders one fixture code")]
mod qr;
#[path = "../sizing.rs"]
#[allow(
    dead_code,
    reason = "the app's adapter; the snapshot tool uses its scene push only"
)]
mod sizing;
#[path = "../theme.rs"]
mod theme;

thread_local! {
    static WINDOW: RefCell<Option<Rc<MinimalSoftwareWindow>>> = const { RefCell::new(None) };
}

struct SnapshotPlatform;

impl Platform for SnapshotPlatform {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, slint::PlatformError> {
        let window = MinimalSoftwareWindow::new(RepaintBufferType::ReusedBuffer);
        WINDOW.with(|w| *w.borrow_mut() = Some(window.clone()));
        Ok(window)
    }

    fn duration_since_start(&self) -> Duration {
        Duration::ZERO
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let width: u32 = args.get(1).map_or(1920, |a| a.parse().unwrap());
    let height: u32 = args.get(2).map_or(1080, |a| a.parse().unwrap());
    let out = args
        .get(3)
        .cloned()
        .unwrap_or_else(|| "snapshot.png".to_string());
    let screen = args.get(4).cloned().unwrap_or_else(|| "hub".to_string());

    slint::platform::set_platform(Box::new(SnapshotPlatform)).unwrap();
    // Same font stack as the device build: the embedded script faces and
    // their fallback wiring.
    fonts::register_embedded_fonts();

    // A 240p help bar is one row until its entries wrap, and the app
    // re-solves every screen when the bar reports that. A still frame has
    // no second turn, so a scene whose entries turn out to wrap is
    // composed again with the two-row bar from the start.
    if !render(width, height, &out, &screen, false) {
        assert!(
            render(width, height, &out, &screen, true),
            "the help bar's wrap measurement depends on its height"
        );
    }
}

/// Compose and write one frame, solved for a help bar of the given row
/// count. Returns false, writing nothing, when the composed scene's help
/// entries measure the other way.
#[allow(
    clippy::too_many_lines,
    reason = "offline fixture dispatcher keeps scenario setup and capture in one place"
)]
fn render(width: u32, height: u32, out: &str, screen: &str, help_two_rows: bool) -> bool {
    let app = App::new().unwrap();
    // ZAPAROO_SNAPSHOT_SCHEME / ZAPAROO_SNAPSHOT_INTENSITY render any preset.
    theme::apply_palette(
        &app,
        &std::env::var("ZAPAROO_SNAPSHOT_SCHEME").unwrap_or_default(),
        &std::env::var("ZAPAROO_SNAPSHOT_INTENSITY").unwrap_or_default(),
    );
    // ZAPAROO_SNAPSHOT_LANG=de renders through the bundled German catalog
    // (any language directory under translations/).
    if let Ok(lang) = std::env::var("ZAPAROO_SNAPSHOT_LANG") {
        slint::select_bundled_translation(&lang).expect("bundled language");
        fonts::set_han_preference(&lang);
    }
    brand::register(&app);
    app.global::<GlyphSource>().on_glyph(|key, px, tint| {
        glyphs::render(key.as_str(), px.round().max(0.0) as u32, tint).unwrap_or_default()
    });
    let crt = screen.starts_with("crt-") || screen == "calibration";
    let ccw = screen.contains("ccw");
    let tate = screen.contains("tate") || ccw;
    let list = screen.contains("list");
    app.global::<Theme>().set_crt(crt);
    app.global::<Sizing>().set_crt(crt);
    // These frames come from the software renderer, which ignores
    // transforms: the focus zoom is drawn by size, as it is on MiSTer.
    app.global::<Motion>().set_zoom_by_size(true);
    app.global::<Sizing>().set_swap_axes(tate);
    app.global::<Sizing>().set_help_bar_two_rows(help_two_rows);
    let shell = app.global::<Shell>();
    shell.set_orientation(if ccw {
        Orientation::Ccw
    } else if tate {
        Orientation::Cw
    } else {
        Orientation::Horizontal
    });
    shell.set_browse_list_layout(list);
    shell.set_systems_list_layout(list);
    shell.set_crt_enabled(crt);
    shell.set_is_mister(crt);
    let rotation = if ccw {
        RenderingRotation::Rotate270
    } else if tate {
        RenderingRotation::Rotate90
    } else {
        RenderingRotation::NoRotation
    };
    let (logical_width, logical_height) = if tate {
        (height, width)
    } else {
        (width, height)
    };
    app.window()
        .set_size(slint::PhysicalSize::new(logical_width, logical_height));
    // The scene the App derives: the CRT path trims 5% from each edge.
    let inset = |v: u32| {
        if crt {
            2.0 * (f64::from(v) * 0.05).round()
        } else {
            0.0
        }
    };
    let scene_w = f64::from(logical_width) - inset(logical_width);
    let scene_h = f64::from(logical_height) - inset(logical_height);
    // ZAPAROO_SNAPSHOT_PROFILE renders an interface profile the host is
    // not: `handheld` is what a Steam Deck resolves `device` to, and it
    // changes page density, so the shapes only reproduce offline if the
    // profile is set before the scene is solved.
    app.global::<Sizing>().set_handheld(
        zaparoo_app::sizing::InterfaceProfile::resolve(
            &std::env::var("ZAPAROO_SNAPSHOT_PROFILE").unwrap_or_default(),
            false,
        ) == zaparoo_app::sizing::InterfaceProfile::Handheld,
    );
    sizing::apply_scene(&app, sizing::Scene::of(&app, scene_w, scene_h, crt));
    // Chrome fixtures: a full HUD with a battery reading, and an
    // indexing run in the status line so the track and percent render.
    shell.set_status_icons_enabled(true);
    shell.set_has_battery(true);
    shell.set_battery_percent(62);
    shell.set_clock_text("10:32".into());
    let status = app.global::<generated::Status>();
    status.set_kind(StatusKind::Indexing);
    status.set_arg("Super Nintendo".into());
    status.set_show_track(true);
    status.set_current_step(5);
    status.set_total_steps(12);
    status.set_percent(42);

    // Representative persisted Hub layout: Resume first, the detected
    // categories, then the built-in actions, with one hidden system
    // shortcut and one folder shortcut the user added.
    fixture_hub(&app, scene_w, scene_h, crt, 1);
    // Games fixtures so the grid, top strip, and page counter render.
    // `*-i18n` screens swap the fixture titles for one string per script
    // the catalogs ship, so shaping (Arabic, Devanagari), bidi (Hebrew,
    // Arabic) and CJK glyph coverage can be checked offline.
    let i18n = screen.contains("i18n");
    let i18n_titles = [
        "لعبة تجريبية واحد",
        "משחק לדוגמה שתיים",
        "उदाहरण खेल तीन",
        "サンプルゲーム 四",
        "예제 게임 다섯",
        "示例游戏 六",
        "Mixed عربي Latin 7",
        "Ñandú Ægir Ωmega 8",
        "Straße Ærø Łódź 9",
        "Ελληνικά παιχνίδι 10",
        "Українська гра 11",
        "Example Game Title 12",
    ];
    let games_mode = if screen.contains("search-results") {
        GamesMode::Search
    } else if screen.contains("favorites") {
        GamesMode::Favorites
    } else if screen.contains("recents") {
        GamesMode::Recents
    } else {
        GamesMode::Browse
    };
    fixture_games(
        &app,
        scene_w,
        scene_h,
        crt,
        &i18n_titles,
        i18n,
        games_mode,
        (screen == "context" || screen == "context-alt").then_some(6),
    );
    if screen.contains("search-results") {
        app.global::<GamesView>()
            .set_title("\u{201c}mario\u{201d} \u{b7} Super Nintendo".into());
    } else if screen.contains("search") || screen.ends_with("system-picker") {
        fixture_search(&app, screen);
    }
    // The browse filter: the header cue on the games screen, the picker's
    // categories page with a filter set, its values page with counts, and
    // the empty list a filter can leave.
    if screen.ends_with("games-filter")
        || screen.ends_with("games-list-filter")
        || screen.ends_with("games-filter-empty")
    {
        let view = app.global::<GamesView>();
        view.set_filter_text("Platformer +1".into());
        if screen.ends_with("empty") {
            view.set_count(0);
            view.set_total_files(0);
            view.set_cells(slint::ModelRc::default());
        }
    }
    if screen.ends_with("filter-picker")
        || screen.ends_with("filter-values")
        || screen.ends_with("system-picker")
    {
        use generated::MenuRole;
        let row =
            |id: &str, key: &str, name: &str, detail: &str, detail_key: &str, role: MenuRole| {
                MenuEntry {
                    role,
                    detail_key: detail_key.into(),
                    id: id.into(),
                    label: name.into(),
                    label_key: key.into(),
                    enabled: true,
                    reason_key: "".into(),
                    detail: detail.into(),
                }
            };
        let option = |id: &str, key: &str, name: &str, detail: &str| {
            row(id, key, name, detail, "", MenuRole::Option)
        };
        let ov = app.global::<generated::Overlays>();
        if !screen.ends_with("system-picker") {
            app.global::<GamesView>()
                .set_filter_text("Platformer +1".into());
        }
        let (title, rows, index) = if screen.ends_with("values") {
            let values = [
                ("Action", "412"),
                ("Adventure", "88"),
                ("Fighting", "61"),
                ("Platformer", "203"),
                ("Puzzle", "97"),
                ("Racing", "54"),
                ("Role-playing", "130"),
                ("Shooter", "176"),
                ("Sports", "72"),
            ];
            let mut rows = vec![option("any", "filter:any", "", "")];
            rows.extend(
                values
                    .iter()
                    .map(|(name, count)| option(&format!("v:{name}"), "", name, count)),
            );
            ("title:filter_cat:genre", rows, 4)
        } else if screen.ends_with("system-picker") {
            // All systems, the ones used lately, then every system under
            // its manufacturer, with its game count.
            let header = |key: &str, name: &str| row("", key, name, "", "", MenuRole::Header);
            let system = |name: &str, games: &str| option(&format!("sys:{name}"), "", name, games);
            (
                "title:search_system",
                vec![
                    option("all", "search:all_systems", "", ""),
                    header("section:recent", ""),
                    system("Super Nintendo", "812"),
                    system("Genesis", "764"),
                    header("", "Nintendo"),
                    system("Game Boy", "1045"),
                    system("Nintendo 64", "296"),
                    system("Super Nintendo", "812"),
                    header("", "Sega"),
                    system("Genesis", "764"),
                    system("Saturn", "118"),
                ],
                2,
            )
        } else {
            let any = |id: &str| {
                row(
                    &format!("cat:{id}"),
                    &format!("title:filter_cat:{id}"),
                    "",
                    "",
                    "filter:any",
                    MenuRole::Option,
                )
            };
            let chosen = |id: &str, value: &str| {
                option(
                    &format!("cat:{id}"),
                    &format!("title:filter_cat:{id}"),
                    "",
                    value,
                )
            };
            (
                "title:filter",
                vec![
                    chosen("genre", "Platformer"),
                    any("year"),
                    chosen("players", "2"),
                    any("region"),
                    row("filter_clear", "filter_clear", "", "", "", MenuRole::Action),
                ],
                0,
            )
        };
        ov.set_list_title(title.into());
        ov.set_list_form(true);
        ov.set_list_entries(slint::ModelRc::new(slint::VecModel::from(rows)));
        ov.set_list_index(index);
        ov.set_list_open(true);
    }
    // "context-alt" renders the same menu after discovery replaced its
    // rows with the alternate builds it found.
    if screen == "context-alt" {
        let rows = [
            "Bubble Bobble (Japan)",
            "Bubble Bobble (Bootleg)",
            "Bubble Bobble (Rev A)",
        ];
        app.global::<generated::Overlays>()
            .set_context_entries(slint::ModelRc::new(slint::VecModel::from(
                rows.iter()
                    .enumerate()
                    .map(|(index, name)| MenuEntry {
                        role: generated::MenuRole::default(),
                        detail_key: slint::SharedString::default(),
                        id: format!("page_row:{index}").into(),
                        label: (*name).into(),
                        label_key: "".into(),
                        enabled: true,
                        reason_key: "".into(),
                        detail: "".into(),
                    })
                    .collect::<Vec<_>>(),
            )));
        app.global::<generated::Overlays>().set_context_index(0);
        app.global::<generated::Overlays>().set_context_open(true);
    }
    // "context" renders the games screen with the context menu open.
    if screen == "context" {
        app.global::<generated::Overlays>()
            .set_context_entries(slint::ModelRc::new(slint::VecModel::from(vec![
                MenuEntry {
                    role: generated::MenuRole::default(),
                    detail_key: slint::SharedString::default(),
                    id: "more_info".into(),
                    label: "".into(),
                    label_key: "more_info".into(),
                    enabled: true,
                    reason_key: "".into(),
                    detail: "".into(),
                },
                MenuEntry {
                    role: generated::MenuRole::default(),
                    detail_key: slint::SharedString::default(),
                    id: "toggle_favorite".into(),
                    label: "".into(),
                    label_key: "favorite:add".into(),
                    enabled: true,
                    reason_key: "".into(),
                    detail: "".into(),
                },
                MenuEntry {
                    role: generated::MenuRole::default(),
                    detail_key: slint::SharedString::default(),
                    id: "write_card".into(),
                    label: "".into(),
                    label_key: "write_card".into(),
                    enabled: true,
                    reason_key: "".into(),
                    detail: "".into(),
                },
                MenuEntry {
                    role: generated::MenuRole::default(),
                    detail_key: slint::SharedString::default(),
                    id: "qr_code".into(),
                    label: "".into(),
                    label_key: "qr_code".into(),
                    enabled: true,
                    reason_key: "".into(),
                    detail: "".into(),
                },
                MenuEntry {
                    role: generated::MenuRole::default(),
                    detail_key: slint::SharedString::default(),
                    id: "add_to_hub".into(),
                    label: "".into(),
                    label_key: "add_to_hub".into(),
                    enabled: true,
                    reason_key: "".into(),
                    detail: "".into(),
                },
                MenuEntry {
                    role: generated::MenuRole::default(),
                    detail_key: slint::SharedString::default(),
                    id: "toggle_hidden".into(),
                    label: "".into(),
                    label_key: "hide:hide".into(),
                    enabled: true,
                    reason_key: "".into(),
                    detail: "".into(),
                },
                MenuEntry {
                    role: generated::MenuRole::default(),
                    detail_key: slint::SharedString::default(),
                    id: "scrape_game".into(),
                    label: "".into(),
                    label_key: "scrape_game".into(),
                    enabled: true,
                    reason_key: "".into(),
                    detail: "".into(),
                },
            ])));
        app.global::<generated::Overlays>().set_context_index(1);
        app.global::<generated::Overlays>().set_context_open(true);
    }
    // "fast-scroll" renders the games screen mid fast scroll: the rail up
    // with its letters, on the fourth one.
    if screen.contains("fast-scroll") {
        let labels = [
            "0-9", "A", "B", "C", "D", "F", "G", "K", "M", "P", "R", "S", "T", "W", "Z",
        ];
        let view = app.global::<GamesView>();
        view.set_rail_letters(slint::ModelRc::new(slint::VecModel::from(
            labels
                .iter()
                .map(|l| slint::SharedString::from(*l))
                .collect::<Vec<_>>(),
        )));
        view.set_rail_index(3);
        view.set_rail_letter("C".into());
        view.set_rail_visible(true);
        view.set_rapid_active(true);
    }
    // "letters" renders the games screen with the letter picker open.
    if screen.contains("letters") {
        let labels = [
            "#", "A", "B", "C", "D", "E", "F", "G", "H", "J", "K", "L", "M", "N", "P", "R", "S",
            "T", "V", "W", "Y", "Z",
        ];
        app.global::<generated::Overlays>()
            .set_letter_buckets(slint::ModelRc::new(slint::VecModel::from(
                labels
                    .iter()
                    .enumerate()
                    .map(|(i, l)| LetterBucket {
                        label: (*l).into(),
                        count: (3 + (i as i32 * 7) % 40),
                    })
                    .collect::<Vec<_>>(),
            )));
        app.global::<generated::Overlays>().set_letter_index(4);
        app.global::<generated::Overlays>().set_letter_open(true);
    }
    let about = app.global::<generated::AboutView>();
    about.set_version("1.3.0".into());
    about.set_commit("0123456".into());
    about.set_channel("dev".into());
    about.set_build_date("2026-09-07".into());
    if screen.contains("about-scrolled") {
        about.set_scroll_milli(1000);
    }
    // Systems fixtures: a paged category (page 2 of 3) with no logo
    // files on disk, so the wordmark fallback renders.
    fixture_systems(
        &app,
        scene_w,
        scene_h,
        crt,
        screen.contains("favorite-systems"),
    );
    app.global::<Shell>()
        .set_status_keys(slint::ModelRc::new(slint::VecModel::from(vec![
            slint::SharedString::from("NFC"),
            slint::SharedString::from("WiFi"),
            slint::SharedString::from("WiredNetwork"),
            slint::SharedString::from("Bluetooth"),
        ])));
    // Settings fixtures: the root category grid, or the Library page
    // with a header, both maintenance actions and the browsing rows.
    fixture_settings(&app, scene_w, scene_h, crt, screen);
    // "setup" renders the scrape setup form over Settings; "setup-picker"
    // renders its scope page.
    if screen.contains("setup") {
        let inputs = sizing::Scene::of(&app, scene_w, scene_h, crt).inputs();
        let row_h = inputs.pct_h(8.0);
        let sv = app.global::<generated::SetupModalView>();
        let rows: Vec<generated::SettingsRow> = [
            (
                "source",
                ControlKind::Picker,
                ScopeKind::Source,
                "Screenscraper",
            ),
            (
                "systems",
                ControlKind::Picker,
                ScopeKind::Category,
                "Console",
            ),
            ("rescrape", ControlKind::Toggle, ScopeKind::All, ""),
            ("startImport", ControlKind::Action, ScopeKind::All, ""),
        ]
        .iter()
        .enumerate()
        .map(|(i, (id, control, value, name))| generated::SettingsRow {
            kind: RowKind::Field,
            id: (*id).into(),
            control: *control,
            scope_kind: *value,
            value_name: (*name).into(),
            checked: *id == "rescrape",
            enabled: true,
            y_offset: (i as i32 * row_h) as f32,
            height: row_h as f32,
            ..Default::default()
        })
        .collect();
        sv.set_kind(SetupKind::Scrape);
        sv.set_rows(slint::ModelRc::new(slint::VecModel::from(rows)));
        sv.set_index(1);
        if screen.contains("picker") {
            // Two systems checked: they lead the list under their own
            // heading, and stay checked under their manufacturer.
            let entries = [
                (ScopeKind::All, "", false),
                (ScopeKind::Category, "Console", false),
                (ScopeKind::Selected, "", false),
                (ScopeKind::System, "Mega Drive", true),
                (ScopeKind::System, "Super Nintendo", true),
                (ScopeKind::Header, "Nintendo", false),
                (ScopeKind::System, "Nintendo Entertainment System", false),
            ];
            let picker: Vec<generated::SetupPickerRow> = entries
                .iter()
                .map(|(kind, name, checked)| generated::SetupPickerRow {
                    kind: *kind,
                    name: (*name).into(),
                    checked: *checked,
                })
                .collect();
            sv.set_picker_page(true);
            sv.set_picker_title(SetupPicker::Systems);
            sv.set_picker_visible(i32::try_from(picker.len()).unwrap_or(0));
            sv.set_picker_count(i32::try_from(picker.len()).unwrap_or(0) + 1);
            sv.set_picker_rows(slint::ModelRc::new(slint::VecModel::from(picker)));
            sv.set_picker_sel(3);
            sv.set_picker_checked(2);
            sv.set_picker_toggle(true);
            sv.set_has_below(true);
        }
        sv.set_open(true);
    }
    // "picker" renders the shared list picker over a long option list,
    // to check its scrolling window.
    if screen == "picker" || screen == "crt-picker" || screen.contains("palette-picker") {
        let ov = app.global::<generated::Overlays>();
        let palette = screen.contains("palette-picker");
        let values: Vec<&str> = if palette {
            zaparoo_app::palette::ids().collect()
        } else {
            zaparoo_app::settings::LANGUAGES.to_vec()
        };
        let entries: Vec<MenuEntry> = values
            .iter()
            .map(|value| MenuEntry {
                role: generated::MenuRole::default(),
                detail_key: slint::SharedString::default(),
                id: (*value).into(),
                label: "".into(),
                label_key: "".into(),
                enabled: true,
                reason_key: "".into(),
                detail: "".into(),
            })
            .collect();
        ov.set_list_setting_id(if palette { "colorScheme" } else { "language" }.into());
        ov.set_list_entries(slint::ModelRc::new(slint::VecModel::from(entries)));
        ov.set_list_index(6);
        ov.set_list_open(true);
    }
    // "launcher-picker" renders the "Change launcher" list with one row
    // Core reports as unavailable — muted, folded reason, still pickable.
    if screen.ends_with("launcher-picker") {
        let ov = app.global::<generated::Overlays>();
        ov.set_list_title("title:change_launcher".into());
        ov.set_list_entries(slint::ModelRc::new(slint::VecModel::from(vec![
            MenuEntry {
                role: generated::MenuRole::default(),
                detail_key: slint::SharedString::default(),
                id: "default".into(),
                label: "".into(),
                label_key: "launcher:default".into(),
                enabled: true,
                reason_key: "".into(),
                detail: "".into(),
            },
            MenuEntry {
                role: generated::MenuRole::default(),
                detail_key: slint::SharedString::default(),
                id: "RetroArch".into(),
                label: "RetroArch".into(),
                label_key: "".into(),
                enabled: true,
                reason_key: "".into(),
                detail: "".into(),
            },
            MenuEntry {
                role: generated::MenuRole::default(),
                detail_key: slint::SharedString::default(),
                id: "DuckStation".into(),
                label: "DuckStation".into(),
                label_key: "".into(),
                enabled: false,
                reason_key: "launcher:not_installed".into(),
                detail: "".into(),
            },
        ])));
        ov.set_list_index(1);
        ov.set_list_open(true);
    }
    if screen.ends_with("launcher-saving") {
        let ov = app.global::<generated::Overlays>();
        ov.set_list_title("title:change_launcher".into());
        ov.set_list_entries(slint::ModelRc::new(slint::VecModel::from(vec![
            MenuEntry {
                role: generated::MenuRole::default(),
                detail_key: slint::SharedString::default(),
                id: "default".into(),
                label: "Default".into(),
                label_key: "".into(),
                enabled: true,
                reason_key: "".into(),
                detail: "".into(),
            },
            MenuEntry {
                role: generated::MenuRole::default(),
                detail_key: slint::SharedString::default(),
                id: "alternate".into(),
                label: "Alternate launcher".into(),
                label_key: "".into(),
                enabled: true,
                reason_key: "".into(),
                detail: "".into(),
            },
        ])));
        ov.set_list_index(1);
        ov.set_list_open(true);
        ov.set_launcher_saving(true);
        ov.set_launcher_saving_visible(true);
    }
    if screen.ends_with("token-write") {
        app.global::<generated::Overlays>()
            .set_card_write_open(true);
    }
    if screen.ends_with("token-error") {
        let ov = app.global::<generated::Overlays>();
        ov.set_dialog_kind(DialogKind::ActionError);
        ov.set_dialog_error(ErrorKind::CardWrite);
        ov.set_dialog_buttons(slint::ModelRc::new(slint::VecModel::from(vec![
            DialogButton::Retry,
        ])));
        ov.set_dialog_open(true);
    }
    if screen.contains("qr-") {
        let docs = screen.ends_with("docs");
        let payload = if docs {
            "https://zaparoo.org/docs/frontend/".to_string()
        } else {
            qr::write_url("/games/SNES/Chrono Trigger.sfc")
        };
        let (image, modules) =
            qr::qr_image(&payload, qr::code_colors(&app)).expect("fixture QR encodes");
        let ov = app.global::<generated::Overlays>();
        ov.set_qr_image(image);
        ov.set_qr_modules(i32::try_from(modules).unwrap());
        ov.set_qr_documentation(docs);
        ov.set_qr_open(true);
    }
    if screen.contains("game-info") {
        fixture_game_info(&app, screen);
    }
    // "online-link" renders the Online link panel with a live code.
    if screen.contains("online-link") {
        let url = "https://online.zaparoo.com/link?code=ABCD1234";
        let ov = app.global::<generated::Overlays>();
        if let Some((image, modules)) = qr::qr_image(url, qr::code_colors(&app)) {
            ov.set_online_qr(image);
            ov.set_online_qr_modules(i32::try_from(modules).unwrap_or(0));
        }
        ov.set_online_phase(OnlineLinkPhase::Showing);
        ov.set_online_code("ABCD-1234".into());
        ov.set_online_url("https://online.zaparoo.com/link".into());
        ov.set_online_expires_in(597);
        ov.set_online_open(true);
    }
    // "…-online-backups" and "…-online-activity" render the Online page's
    // list modal over it: a list long enough to scroll, cursor mid-way.
    if screen.contains("online-backups") || screen.contains("online-activity") {
        let backups = screen.contains("online-backups");
        let ov = app.global::<generated::Overlays>();
        let row = |id: &str, label: &str, detail: &str, enabled: bool, this_device: bool| {
            generated::OnlineListRow {
                id: id.into(),
                label: label.into(),
                detail: detail.into(),
                enabled,
                this_device,
            }
        };
        let rows = if backups {
            let mut rows = vec![row("run", "", "", true, false)];
            rows.extend((1..=9).map(|n| {
                row(
                    &format!("s{n}"),
                    &format!("{} Oct 09:45", 12 - n),
                    if n % 3 == 0 {
                        "4.2 MB, MiSTer"
                    } else {
                        "4.2 MB"
                    },
                    n != 4,
                    n % 3 != 0,
                )
            }));
            rows
        } else {
            (1..=9)
                .map(|n| {
                    row(
                        "",
                        &format!("{} Oct 09:45  launch", 12 - n),
                        if n == 2 {
                            "account, failed: host_foreground_required"
                        } else {
                            "account, succeeded"
                        },
                        true,
                        false,
                    )
                })
                .collect()
        };
        ov.set_online_list_kind(if backups {
            generated::OnlineListKind::Backups
        } else {
            generated::OnlineListKind::Activity
        });
        ov.set_online_list_note(if backups { "38.1 MB / 1.0 GB" } else { "" }.into());
        ov.set_online_list_rows(slint::ModelRc::new(slint::VecModel::from(rows)));
        ov.set_online_list_index(if backups { 4 } else { 1 });
        ov.set_online_list_open(true);
    }
    // "log-upload" renders the uploader's finished state with its link.
    if screen.contains("log-upload") {
        let lv = app.global::<generated::LogUploadView>();
        lv.set_phase(if screen.contains("failed") {
            LogPhase::Failed
        } else {
            LogPhase::Done
        });
        lv.set_url("https://logs.zaparoo.org/a1b2c3d4".into());
        if let Some((image, modules)) =
            qr::qr_image("https://logs.zaparoo.org/a1b2c3d4", qr::code_colors(&app))
        {
            lv.set_qr(image);
            lv.set_qr_modules(i32::try_from(modules).unwrap_or(0));
        }
        lv.set_open(true);
    }
    // "dialog" renders the two-button decision dialog; "alert" renders
    // the one-button failure alert, both through the same vocabulary
    // the router drives.
    if screen.ends_with("dialog") || screen.ends_with("alert") || screen.ends_with("notice") {
        let overlays = app.global::<generated::Overlays>();
        if screen.ends_with("notice") {
            overlays.set_dialog_kind(DialogKind::Notice);
            overlays.set_dialog_buttons(slint::ModelRc::new(slint::VecModel::from(vec![
                DialogButton::IUnderstand,
            ])));
            overlays.set_dialog_focus(0);
        } else if screen.ends_with("alert") {
            overlays.set_dialog_kind(DialogKind::ActionError);
            // The config alert carries the longest unbreakable word any
            // alert shows: the MiSTer path of the file.
            if screen.ends_with("config-alert") {
                overlays.set_dialog_error(ErrorKind::ConfigFile);
                overlays.set_dialog_arg("/media/fat/zaparoo/frontend.toml".into());
            } else {
                overlays.set_dialog_error(ErrorKind::Launch);
                overlays.set_dialog_arg("Sonic the Hedgehog".into());
            }
            overlays.set_dialog_buttons(slint::ModelRc::new(slint::VecModel::from(vec![
                DialogButton::Ok,
            ])));
            overlays.set_dialog_focus(0);
        } else {
            overlays.set_dialog_kind(DialogKind::QuitConfirm);
            overlays.set_dialog_buttons(slint::ModelRc::new(slint::VecModel::from(vec![
                DialogButton::No,
                DialogButton::Yes,
            ])));
            overlays.set_dialog_focus(0);
        }
        overlays.set_dialog_open(true);
    }
    if screen == "calibration" {
        let overlays = app.global::<generated::Overlays>();
        overlays.set_crt_h_offset(4);
        overlays.set_crt_v_offset(-2);
        overlays.set_crt_calibration_open(true);
    }
    // "update-*" renders the Update screen from a seeded `UpdateView`, with
    // no engine behind it.
    if screen.contains("update") {
        fixture_update(&app, screen);
    }
    app.global::<Shell>()
        .set_active_screen(fixture_screen(screen));
    if screen == "route-forward" {
        app.global::<Shell>().set_active_screen(Screen::Systems);
    }
    if screen == "route-back" {
        app.global::<Shell>().set_active_screen(Screen::Hub);
    }
    if screen.contains("cached") {
        app.global::<SystemsView>().set_cached_transition(true);
        app.global::<GamesView>().set_cached_transition(true);
    }

    let late = std::env::var("SNAPSHOT_LATE_SET").is_ok();
    let window = WINDOW.with(|w| w.borrow().clone()).unwrap();
    if late {
        // Regression probe for the globals refactor: first paint with
        // the empty defaults, then push state the way the runtime does
        // (after the first frame) and require a second dirty frame.
        let empty: Vec<GridCell> = Vec::new();
        app.global::<generated::HubView>()
            .set_cells(slint::ModelRc::new(slint::VecModel::from(empty)));
        let mut first = vec![PremultipliedRgbaColor::default(); width as usize * height as usize];
        let drew = window.draw_if_needed(|renderer| {
            renderer.set_rendering_rotation(rotation);
            renderer.render(first.as_mut_slice(), width as usize);
        });
        assert!(drew, "first frame had nothing to draw");
        fixture_hub(&app, scene_w, scene_h, crt, 0);
        let mut probe = vec![PremultipliedRgbaColor::default(); width as usize * height as usize];
        let dirty_model = window.draw_if_needed(|r| {
            r.render(probe.as_mut_slice(), width as usize);
        });
        println!("dirty after HubView.cells set: {dirty_model}");
        app.global::<Shell>().set_status_text(AppCue::Launching);
        let dirty_text = window.draw_if_needed(|r| {
            r.render(probe.as_mut_slice(), width as usize);
        });
        println!("dirty after Shell.status-text set: {dirty_text}");
        app.global::<generated::HubView>().set_selected_local(2);
        let dirty_idx = window.draw_if_needed(|r| {
            r.render(probe.as_mut_slice(), width as usize);
        });
        println!("dirty after HubView.selected-local set: {dirty_idx}");
        // Games screen (alias-block pattern): does a late set reach it?
        app.global::<Shell>().set_active_screen(Screen::Games);
        let _ = window.draw_if_needed(|r| {
            r.render(probe.as_mut_slice(), width as usize);
        });
        app.global::<GamesView>().set_selected_local(5);
        let dirty_games_idx = window.draw_if_needed(|r| {
            r.render(probe.as_mut_slice(), width as usize);
        });
        println!("dirty after GamesView.games-index set (alias pattern): {dirty_games_idx}");
        app.global::<GamesView>().set_title("Probe System".into());
        let dirty_games_sys = window.draw_if_needed(|r| {
            r.render(probe.as_mut_slice(), width as usize);
        });
        println!("dirty after GamesView.games-system set (alias pattern): {dirty_games_sys}");
        // Systems / Settings / About: every screen must react to a
        // late global write (the partially-applied-refactor class).
        app.global::<Shell>().set_active_screen(Screen::Systems);
        let _ = window.draw_if_needed(|r| {
            r.render(probe.as_mut_slice(), width as usize);
        });
        app.global::<SystemsView>()
            .set_category("Probe Category".into());
        let dirty_sys = window.draw_if_needed(|r| {
            r.render(probe.as_mut_slice(), width as usize);
        });
        println!("dirty after SystemsView.category set: {dirty_sys}");
        app.global::<Shell>().set_active_screen(Screen::Settings);
        app.global::<generated::SettingsView>()
            .set_page(SettingsPage::Language);
        let srows: Vec<generated::SettingsRow> = ["language", "region"]
            .iter()
            .enumerate()
            .map(|(i, id)| generated::SettingsRow {
                kind: RowKind::Field,
                id: (*id).into(),
                control: ControlKind::Picker,
                value: "auto".into(),
                enabled: true,
                y_offset: (i as f32) * 40.0,
                height: 40.0,
                ..Default::default()
            })
            .collect();
        app.global::<generated::SettingsView>()
            .set_rows(slint::ModelRc::new(slint::VecModel::from(srows)));
        let _ = window.draw_if_needed(|r| {
            r.render(probe.as_mut_slice(), width as usize);
        });
        // The startup fixture may already sit on index 1; move to 0 so
        // the write is a real change and must dirty a frame.
        app.global::<generated::SettingsView>().set_index(0);
        let dirty_set = window.draw_if_needed(|r| {
            r.render(probe.as_mut_slice(), width as usize);
        });
        println!("dirty after SettingsView.index set: {dirty_set}");
        app.global::<Shell>().set_active_screen(Screen::About);
        let _ = window.draw_if_needed(|r| {
            r.render(probe.as_mut_slice(), width as usize);
        });
        app.global::<generated::AboutView>()
            .set_version("Probe Version".into());
        let dirty_about = window.draw_if_needed(|r| {
            r.render(probe.as_mut_slice(), width as usize);
        });
        println!("dirty after Shell.about-version-line set: {dirty_about}");
        app.global::<Shell>().set_active_screen(Screen::Hub);
        let _ = window.draw_if_needed(|r| {
            r.render(probe.as_mut_slice(), width as usize);
        });
        fixture_hub(&app, scene_w, scene_h, crt, 0);
        println!("late-set applied; drawing final frame");
    }
    slint::platform::update_timers_and_animations();

    let mut buf = vec![PremultipliedRgbaColor::default(); width as usize * height as usize];
    let rendered = window.draw_if_needed(|renderer| {
        renderer.set_rendering_rotation(rotation);
        renderer.render(buf.as_mut_slice(), width as usize);
    });
    assert!(
        rendered,
        "window had nothing to draw (late-set: second frame not dirty)"
    );
    // The bar measures its entries once they are instantiated, which the
    // first draw does, so the row count is only known here.
    if app.get_help_entries_wrap() != help_two_rows {
        return false;
    }

    // Diagnostic: dump what the Sizing global actually saw, so stale
    // screen-size plumbing is visible in the output, not just the PNG.
    let sizing = app.global::<Sizing>();
    println!(
        "window {}x{} sizing {}x{}",
        width,
        height,
        sizing.get_screen_width(),
        sizing.get_screen_height()
    );

    let mut rgba = Vec::with_capacity(buf.len() * 4);
    for px in &buf {
        rgba.extend_from_slice(&[px.red, px.green, px.blue, 255]);
    }
    let img: image::RgbaImage =
        image::ImageBuffer::from_raw(width, height, rgba).expect("buffer size mismatch");
    img.save(out).expect("write png");
    println!("wrote {out}");
    true
}

/// Push a Hub page built from a representative layout through the same
/// rules the app uses (`zaparoo_app::hub`), at the scene's geometry.
/// The Search screen: a typed query with live matches, the empty screen
/// with its recent searches, or a folder search with tags chosen.
fn fixture_search(app: &App, screen: &str) {
    let view = app.global::<SearchView>();
    let sizing = app.global::<Sizing>();
    let collapsed = sizing.get_tier_240()
        || sizing.get_swap_axes()
        || sizing.get_screen_width() < sizing.get_screen_height() * 1.3;
    let mut keys = keyboard::Keyboard::new(
        zaparoo_app::keyboard::Layers::Basic,
        zaparoo_app::search::QUERY_MAX_CHARS,
    );
    let row = |title: &str, detail: &str| SearchPaneRow {
        title: title.into(),
        detail: detail.into(),
    };
    let recents = screen.contains("recents");
    let rows = if recents {
        view.set_pane(SearchPane::Recents);
        view.set_pane_clear(!collapsed);
        view.set_zone(SearchZone::Pane);
        view.set_pane_index(1);
        vec![
            row("mario kart", ""),
            row("zelda", "Super Nintendo"),
            row("Role-playing \u{b7} 1994", ""),
        ]
    } else {
        keys.set_text("mario k");
        keys.focus(zaparoo_app::keyboard::Key::Char('a'));
        view.set_pane(SearchPane::Preview);
        view.set_count_known(true);
        view.set_count(12);
        view.set_can_search(true);
        vec![
            row("Mario Kart 64", "Nintendo 64"),
            row("Mario Kart DS", "Nintendo DS"),
            row("Mario Kart: Super Circuit", "Game Boy Advance"),
            row("Super Mario Kart", "Super Nintendo"),
            row("Mario Kart Wii", "Wii"),
            row("Mario Kart 8 Deluxe", "Switch"),
            row("Mario Kart 7", "Nintendo 3DS"),
            row("Mario Kart: Double Dash", "GameCube"),
            row("Mario Kart Arcade GP", "Arcade"),
            row("Mario Kart Tour", "Applications"),
        ]
    };
    if screen.contains("scoped") {
        // A folder search with a filter chosen, the cursor moved back into
        // the query and focus on the Filter field.
        view.set_scoped(true);
        view.set_system_name("Super Nintendo".into());
        view.set_scope_name("Racing".into());
        view.set_filter_text("Racing +1".into());
        view.set_zone(SearchZone::Filter);
        keys.move_caret(false);
        keys.move_caret(false);
    }
    let (before, at, after) = keys.display();
    view.set_before(before);
    view.set_at(at);
    view.set_after(after);
    view.set_keys(slint::ModelRc::new(slint::VecModel::from(keys.cells())));
    view.set_key_rows(i32::try_from(keys.row_count()).unwrap_or(4));
    view.set_key_index(i32::try_from(keys.index()).unwrap_or(0));
    view.set_key_kind(KeyKind::Char);
    view.set_pane_collapsed(collapsed);
    view.set_pane_rows(slint::ModelRc::new(slint::VecModel::from(if collapsed {
        Vec::new()
    } else {
        rows
    })));
}

fn fixture_screen(screen: &str) -> Screen {
    if screen.starts_with("route-") {
        Screen::Hub
    } else if screen.contains("search-results") {
        Screen::SearchResults
    } else if screen.contains("search") || screen.ends_with("system-picker") {
        Screen::Search
    } else if screen.contains("update") {
        Screen::Update
    } else if matches!(screen, "context" | "context-alt" | "letters")
        || (screen.contains("list") && !screen.contains("systems"))
        || screen.contains("games")
        || screen.contains("filter")
        || screen.contains("fast-scroll")
        || screen.contains("game-info")
    {
        Screen::Games
    } else if screen.contains("favorite-systems") {
        Screen::FavoriteSystems
    } else if screen.contains("favorites") {
        Screen::Favorites
    } else if screen.contains("recents") {
        Screen::Recents
    } else if screen.contains("settings")
        || screen.contains("setup")
        || screen.contains("log-upload")
        || screen.contains("online-link")
    {
        Screen::Settings
    } else if matches!(
        screen,
        "saver" | "dialog" | "alert" | "config-alert" | "notice" | "calibration"
    ) {
        Screen::Hub
    } else if screen.contains("about") {
        Screen::About
    } else if screen.contains("systems") {
        Screen::Systems
    } else if screen.contains("hub") {
        Screen::Hub
    } else {
        // Standalone overlay fixtures intentionally mount no root screen.
        Screen::None
    }
}

/// Seed the Update screen's view state for one `update-*` fixture (a `crt-`
/// prefix or a `-tate`/`-ccw` suffix only changes the scene, not the state).
#[allow(
    clippy::too_many_lines,
    reason = "one explicit inventory of the fixtures keeps their states reviewable"
)]
fn fixture_update(app: &App, screen: &str) {
    use generated::{
        UpdateButton, UpdateCounts, UpdateError, UpdateFilter, UpdateFolderKind, UpdateHelp,
        UpdateHelpLabel, UpdateLinuxPhase, UpdateMembership, UpdateOutcome, UpdatePage, UpdateRow,
        UpdateRowKind, UpdateRowStatus, UpdateStatusKind, UpdateView,
    };
    let name = screen
        .trim_start_matches("crt-")
        .trim_end_matches("-ccw")
        .trim_end_matches("-tate");
    let view = app.global::<UpdateView>();
    let set_buttons = |buttons: &[UpdateButton], focus: i32| {
        view.set_buttons(slint::ModelRc::new(slint::VecModel::from(buttons.to_vec())));
        view.set_button_focus(focus);
    };
    let set_help = |entries: &[(&str, UpdateHelpLabel)]| {
        view.set_help(slint::ModelRc::new(slint::VecModel::from(
            entries
                .iter()
                .map(|(button, label)| UpdateHelp {
                    button: (*button).into(),
                    label: *label,
                })
                .collect::<Vec<_>>(),
        )));
    };
    view.set_available(name != "update-unavailable");
    view.set_version("2.3".into());
    view.set_page(UpdatePage::Intro);
    view.set_allows_screensaver(true);
    set_help(&[
        ("Dpad", UpdateHelpLabel::Move),
        ("ButtonA", UpdateHelpLabel::Start),
        ("ButtonB", UpdateHelpLabel::Back),
    ]);
    set_buttons(&[UpdateButton::Back, UpdateButton::Start], 1);
    let finished = |outcome: UpdateOutcome, counts: UpdateCounts| {
        view.set_page(UpdatePage::Finished);
        view.set_outcome(outcome);
        view.set_counts(counts);
    };
    match name {
        "update-unavailable" => {
            set_buttons(&[UpdateButton::Back], 0);
            set_help(&[("ButtonB", UpdateHelpLabel::Back)]);
        }
        "update-running" | "update-progress" | "update-slow" | "update-file" => {
            view.set_page(UpdatePage::Running);
            view.set_progress_known(name != "update-running" && name != "update-slow");
            view.set_progress_bp(4250);
            view.set_progress_decimals(1);
            view.set_status_kind(match name {
                "update-running" => UpdateStatusKind::Starting,
                "update-slow" => UpdateStatusKind::Slow,
                _ => UpdateStatusKind::File,
            });
            view.set_status_arg(
                "_Console/Super Nintendo Entertainment System Enhanced Edition (Rev A) [Alternate].rbf"
                    .into(),
            );
            view.set_allows_screensaver(false);
            set_help(&[("ButtonB", UpdateHelpLabel::Cancel)]);
        }
        "update-running-error" => {
            view.set_page(UpdatePage::Running);
            view.set_progress_known(true);
            view.set_progress_bp(6100);
            view.set_status_kind(UpdateStatusKind::Tool);
            view.set_status_arg("Retrying download of _Arcade/cores/1942.rbf".into());
            view.set_error(UpdateError::Network);
            view.set_allows_screensaver(false);
            set_help(&[("ButtonB", UpdateHelpLabel::Cancel)]);
        }
        "update-transition" => {
            view.set_page(UpdatePage::Running);
            view.set_progress_known(true);
            view.set_progress_bp(10000);
            view.set_status_kind(UpdateStatusKind::Transition);
            view.set_status_arg("from_old_db_ids_to_new_db_ids".into());
            view.set_allows_screensaver(false);
            set_help(&[("ButtonB", UpdateHelpLabel::Cancel)]);
        }
        "update-stopping" => {
            view.set_page(UpdatePage::Stopping);
            view.set_status_kind(UpdateStatusKind::StoppingSlow);
            view.set_allows_screensaver(false);
            set_help(&[]);
        }
        "update-finished"
        | "update-finished-errors"
        | "update-membership"
        | "update-reboot"
        | "update-uptodate"
        | "update-failed" => {
            let counts = UpdateCounts {
                installed: 12,
                updated: 34,
                removed: 1,
                failed: if name == "update-finished-errors" {
                    3
                } else {
                    0
                },
                ..Default::default()
            };
            match name {
                "update-finished-errors" => {
                    finished(UpdateOutcome::CompleteWithErrors, counts);
                    view.set_error(UpdateError::SomeFiles);
                }
                "update-reboot" => finished(UpdateOutcome::CompleteRebootNeeded, counts),
                "update-uptodate" => finished(UpdateOutcome::UpToDate, UpdateCounts::default()),
                "update-failed" => {
                    finished(UpdateOutcome::Failed, UpdateCounts::default());
                    view.set_error(UpdateError::Network);
                }
                _ => finished(UpdateOutcome::Complete, counts),
            }
            if name == "update-membership" {
                view.set_membership(slint::ModelRc::new(slint::VecModel::from(vec![
                    UpdateMembership {
                        topic: "Thanks for supporting the project".into(),
                        message: "New database entries arrive every Friday. Check the Details page after each update to see what changed.".into(),
                        info: "".into(),
                    },
                    UpdateMembership {
                        topic: "".into(),
                        message: "A second message".into(),
                        info: "".into(),
                    },
                ])));
                view.set_membership_index(0);
                view.set_transition("from_old_db_ids_to_new_db_ids".into());
            }
            if matches!(
                name,
                "update-finished" | "update-finished-errors" | "update-membership"
            ) {
                set_buttons(
                    &[
                        UpdateButton::Details,
                        UpdateButton::Errors,
                        UpdateButton::Ok,
                    ],
                    2,
                );
            } else {
                set_buttons(&[UpdateButton::Ok], 0);
            }
            set_help(&[
                ("Dpad", UpdateHelpLabel::Move),
                ("ButtonA", UpdateHelpLabel::Select),
                ("ButtonB", UpdateHelpLabel::Back),
            ]);
        }
        "update-linux" | "update-linux-flash" | "update-linux-failed" => {
            view.set_page(UpdatePage::Linux);
            view.set_linux_phase(if name == "update-linux" {
                UpdateLinuxPhase::Extract
            } else {
                UpdateLinuxPhase::Flash
            });
            view.set_linux_current_version("2025-03-14".into());
            view.set_linux_new_version("2026-08-30".into());
            view.set_linux_failed(name == "update-linux-failed");
            if name == "update-linux-failed" {
                view.set_error(UpdateError::Network);
            }
            view.set_allows_screensaver(false);
            set_help(&[]);
        }
        "update-rebooting" => {
            view.set_page(UpdatePage::Rebooting);
            view.set_countdown_secs(12);
            view.set_allows_screensaver(false);
            set_help(&[]);
        }
        "update-details"
        | "update-details-second"
        | "update-info"
        | "update-info-duplicate"
        | "update-info-failed" => {
            let counts = |i: i32, u: i32, r: i32, f: i32| UpdateCounts {
                installed: i,
                updated: u,
                removed: r,
                failed: f,
                ..Default::default()
            };
            let row = |kind: UpdateRowKind,
                       status: UpdateRowStatus,
                       depth: i32,
                       label: &str,
                       reason: &str| UpdateRow {
                kind,
                status,
                depth,
                label: label.into(),
                reason: reason.into(),
                ..Default::default()
            };
            let mut rows = vec![
                UpdateRow {
                    kind: UpdateRowKind::Database,
                    database: "distribution_mister".into(),
                    file_count: 47,
                    counts: counts(12, 34, 1, 0),
                    ..Default::default()
                },
                UpdateRow {
                    kind: UpdateRowKind::Folder,
                    folder: UpdateFolderKind::Arcade,
                    depth: 1,
                    file_count: 20,
                    counts: counts(4, 16, 0, 0),
                    ..Default::default()
                },
                row(
                    UpdateRowKind::File,
                    UpdateRowStatus::Updated,
                    2,
                    "1942.mra",
                    "_Arcade/1942.mra",
                ),
                row(
                    UpdateRowKind::File,
                    UpdateRowStatus::Installed,
                    2,
                    "Bubble Bobble (Japan, Ver 0.1).mra",
                    "_Arcade/Bubble Bobble (Japan, Ver 0.1).mra",
                ),
                row(
                    UpdateRowKind::File,
                    UpdateRowStatus::Failed,
                    2,
                    "Galaga.mra",
                    "Connection reset by peer",
                ),
                UpdateRow {
                    kind: UpdateRowKind::Folder,
                    folder: UpdateFolderKind::Console,
                    depth: 1,
                    file_count: 3,
                    counts: counts(3, 0, 0, 0),
                    collapsed: true,
                    ..Default::default()
                },
                UpdateRow {
                    kind: UpdateRowKind::Folder,
                    folder: UpdateFolderKind::Plain,
                    depth: 1,
                    label: "docs".into(),
                    file_count: 2,
                    counts: counts(0, 1, 1, 0),
                    ..Default::default()
                },
                row(
                    UpdateRowKind::File,
                    UpdateRowStatus::Removed,
                    2,
                    "old-readme.txt",
                    "docs/old-readme.txt",
                ),
                UpdateRow {
                    kind: UpdateRowKind::Database,
                    database: "jotego/jtcores".into(),
                    file_count: 9,
                    counts: counts(0, 9, 0, 0),
                    collapsed: true,
                    ..Default::default()
                },
                UpdateRow {
                    kind: UpdateRowKind::DuplicateCategory,
                    file_count: 2,
                    ..Default::default()
                },
                UpdateRow {
                    kind: UpdateRowKind::DuplicateFile,
                    depth: 1,
                    label: "PSX.rbf".into(),
                    reason: "_Console/PSX.rbf".into(),
                    database: "distribution_mister".into(),
                    ..Default::default()
                },
                UpdateRow {
                    kind: UpdateRowKind::NotOverwrittenCategory,
                    file_count: 1,
                    ..Default::default()
                },
                UpdateRow {
                    kind: UpdateRowKind::NotOverwrittenFile,
                    depth: 1,
                    label: "user.ini".into(),
                    reason: "config/user.ini".into(),
                    database: "mikes11/yc_builds-mister".into(),
                    ..Default::default()
                },
                UpdateRow {
                    kind: UpdateRowKind::DatabaseError,
                    status: UpdateRowStatus::Failed,
                    database: "theypsilon/broken_db".into(),
                    ..Default::default()
                },
                row(
                    UpdateRowKind::DatabaseErrorReason,
                    UpdateRowStatus::Failed,
                    1,
                    "",
                    "Could not download the database file.",
                ),
            ];
            for n in 0..80 {
                rows.push(row(
                    UpdateRowKind::File,
                    UpdateRowStatus::Updated,
                    2,
                    &format!("Extra file {n:02}.rbf"),
                    &format!("_Console/Extra file {n:02}.rbf"),
                ));
            }
            let total = i32::try_from(rows.len()).unwrap();
            let info_row = match name {
                "update-info-duplicate" => rows[10].clone(),
                "update-info-failed" => rows[4].clone(),
                _ => rows[3].clone(),
            };
            view.set_rows(slint::ModelRc::new(slint::VecModel::from(rows)));
            view.set_page(UpdatePage::Details);
            view.set_filter_visible(true);
            view.set_filter(UpdateFilter::All);
            view.set_filter_focus(UpdateFilter::All);
            view.set_filter_focused(false);
            view.set_focused_row(if name == "update-details-second" {
                60
            } else {
                3
            });
            view.set_row_count(total);
            view.set_database_count(3);
            view.set_details_counts(UpdateCounts {
                installed: 12,
                updated: 43,
                removed: 1,
                failed: 3,
                ..Default::default()
            });
            set_help(&[
                ("Dpad", UpdateHelpLabel::Move),
                ("ButtonA", UpdateHelpLabel::Info),
                ("ButtonB", UpdateHelpLabel::Back),
            ]);
            if name.starts_with("update-info") {
                view.set_info_row(info_row);
                view.set_info_databases(slint::ModelRc::new(slint::VecModel::from(
                    [
                        "distribution_mister",
                        "theypsilon/alt_a",
                        "theypsilon/alt_b",
                    ]
                    .iter()
                    .map(|db| slint::SharedString::from(*db))
                    .collect::<Vec<_>>(),
                )));
                view.set_info_open(true);
                set_help(&[
                    ("ButtonA", UpdateHelpLabel::Close),
                    ("ButtonB", UpdateHelpLabel::Back),
                ]);
            }
        }
        _ => {}
    }
}

fn fixture_game_info(app: &App, screen: &str) {
    let view = app.global::<generated::GameInfoView>();
    view.set_modal_open(true);
    view.set_modal_name("Chrono Trigger".into());
    view.set_loading(screen.ends_with("loading"));
    view.set_failed(screen.ends_with("error"));
    if view.get_loading() || view.get_failed() {
        return;
    }
    let short = screen.ends_with("short");
    let rows: Vec<_> = [
        ("system", "Super Nintendo Entertainment System"),
        ("release_date", "1995"),
        ("genre", "Role-playing"),
        ("players", "1"),
        ("developer", "Square"),
        ("publisher", "Square"),
        ("filename", "Chrono Trigger (USA)"),
    ]
    .into_iter()
    .take(if short { 2 } else { 7 })
    .map(|(key, value)| generated::DetailRow {
        key: key.into(),
        value: value.into(),
    })
    .collect();
    view.set_rows(slint::ModelRc::new(slint::VecModel::from(rows)));
    if !short {
        view.set_media_missing(true);
        view.set_modal_description("A journey through time, from prehistoric lands to a distant future. Meet companions, explore unfamiliar places, and discover how each era connects to the next. Choices made along the way shape the adventure.\n\nThis long fixture checks that descriptions stay readable and can be scrolled rather than silently truncated.".into());
        view.set_image_count(2);
        let logo = slint::Image::load_from_path(std::path::Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/assets/systems/SNES.png"
        )))
        .expect("fixture logo");
        view.set_modal_cover(logo);
        view.set_modal_has_cover(true);
        if screen.ends_with("scrolled") {
            view.set_scroll_position(10.0);
        }
    }
}

fn fixture_hub(app: &App, scene_w: f64, scene_h: f64, crt: bool, selected: usize) {
    use zaparoo_app::hub::{self, LayoutItem, Live, Resolver};
    struct Names;
    impl Resolver for Names {
        fn system_name(&self, id: &str) -> String {
            id.to_string()
        }
        fn system_cover_key(&self, id: &str) -> String {
            format!("systems/{id}")
        }
        fn media_cover_key(&self, _system: &str, _path: &str) -> String {
            "icons/File".to_string()
        }
    }
    let item = |kind: &str, id: &str| LayoutItem {
        kind: kind.into(),
        id: id.into(),
        ..LayoutItem::default()
    };
    let items = vec![
        item("action", "resume"),
        item("category", "Arcade"),
        item("category", "Console"),
        item("category", "Computer"),
        item("category", "Handheld"),
        item("category", "Other"),
        item("action", "favorites"),
        item("action", "recents"),
        item("action", "update"),
        item("action", "settings"),
        LayoutItem {
            kind: "folder".into(),
            path: "/media/fat/games/SNES/Homebrew".into(),
            system: "SNES".into(),
            ..LayoutItem::default()
        },
    ];
    let confirmed: Vec<String> = ["Arcade", "Console", "Computer", "Handheld", "Other"]
        .iter()
        .map(|c| (*c).to_string())
        .collect();
    let live = Live {
        categories_loaded: true,
        confirmed_categories: &confirmed,
        resume_enabled: true,
        resume_name: "Super Metroid",
        resume_cover_key: "",
        resume_known_unavailable: false,
        update_enabled: false,
        internet_available: true,
    };
    let inputs = sizing::Scene::of(app, scene_w, scene_h, crt).inputs();
    let derived = zaparoo_app::sizing::derive(&inputs);
    let geometry = hub::geometry(&inputs, &derived);
    let page_size = (geometry.columns * geometry.rows).max(1) as usize;
    let entries = hub::entries(&items, false, &live, &Names, page_size, 0);
    let cells: Vec<GridCell> = entries
        .iter()
        .take(page_size)
        .map(|e| GridCell {
            label_key: e.label_key.as_str().into(),
            name: e.name.as_str().into(),
            glyph_key: if e.is_empty() || e.cover_key.starts_with("systems/") {
                "".into()
            } else {
                e.cover_key.as_str().into()
            },
            wordmark: e.cover_key.starts_with("systems/"),
            disabled: e.disabled,
            is_empty: e.is_empty(),
            ..Default::default()
        })
        .collect();
    let view = app.global::<generated::HubView>();
    view.set_cells(slint::ModelRc::new(slint::VecModel::from(cells)));
    view.set_selected_local(selected as i32);
    view.set_columns(geometry.columns);
    view.set_rows(geometry.rows);
    view.set_cell_width(geometry.fit.cell_width as f32);
    view.set_cell_height(geometry.fit.cell_height as f32);
    view.set_block_offset_x(geometry.fit.block_offset_x as f32);
    view.set_block_offset_y(geometry.fit.block_offset_y as f32);
    view.set_grid_y(geometry.grid_y as f32);
    view.set_grid_height(geometry.grid_height as f32);
    view.set_label_y(geometry.label_y as f32);
    view.set_label_height(geometry.label_height as f32);
    view.set_page(0);
    view.set_total_pages(entries.len().div_ceil(page_size).max(1) as i32);
    view.set_has_pages_below(entries.len() > page_size);
    view.set_focus_ready(true);
    view.set_loaded(true);
    view.set_options_available(true);
    let focused = &entries[selected.min(entries.len() - 1)];
    view.set_label_key(focused.label_key.as_str().into());
    view.set_label_name(focused.name.as_str().into());
    view.set_label_reason(if focused.disabled {
        focused.reason.into()
    } else {
        DisabledReason::None
    });
}

/// Push a Systems page through the same geometry rules the app uses.
/// The list row height the driver would push for `target_rows` (0 lets
/// the profile's default row height decide).
fn list_metrics(
    app: &App,
    scene_w: f64,
    scene_h: f64,
    crt: bool,
    target_rows: usize,
) -> (f32, usize) {
    use zaparoo_app::layouts::{self, Body, ThemeId, View};
    let inputs = sizing::Scene::of(app, scene_w, scene_h, crt).inputs();
    let derived = zaparoo_app::sizing::derive(&inputs);
    let systems = target_rows == 0;
    let view = match (systems, inputs.swap_percentage_axes) {
        (true, true) => View::SystemsListTate,
        (true, false) => View::SystemsList,
        (false, true) => View::GamesListTate,
        (false, false) => View::GamesList,
    };
    let profile = layouts::profile(ThemeId::current(&inputs), view, &inputs);
    let Body::List { list, .. } = profile.body else {
        return (0.0, 1);
    };
    let g = zaparoo_app::media_list::list_geometry(
        &list,
        &zaparoo_app::media_list::ListFrame {
            screen_width: inputs.screen_width as i32,
            screen_height: inputs.screen_height as i32,
            header_bottom: derived.header_bottom,
            status_top_margin: profile.status.top_margin,
            strip_height: profile.status.strip_height,
            help_bar_height: derived.help_bar_height,
            tier_240: derived.tier == zaparoo_app::sizing::Tier::T240,
            safe_bottom_gap: inputs.pct_h(6.0),
            target_rows,
            min_row_height: inputs.pct_h(3.0),
            default_row_height: inputs.pct_h(6.0),
        },
    );
    (g.row_height as f32, g.visible_rows.max(1))
}

/// The settings screen at its own geometry: the root tiles, or one page
/// of rows straight from the registry.
#[allow(
    clippy::too_many_lines,
    reason = "one fixture keeps settings rows and their geometry together"
)]
fn fixture_settings(app: &App, scene_w: f64, scene_h: f64, crt: bool, screen: &str) {
    use zaparoo_app::layouts::{self, Body, ThemeId, View};
    use zaparoo_app::settings::{self as rules, Control, Row};
    let page = screen.contains("settings-page");
    let playtime = screen.contains("settings-page-playtime");
    let online = screen.contains("settings-page-online");
    // "…-online-unlinked": the same page before an account is linked, with
    // the cursor on a row that waits for one.
    let unlinked = screen.contains("online-unlinked");
    let inputs = sizing::Scene::of(app, scene_w, scene_h, crt).inputs();
    let derived = zaparoo_app::sizing::derive(&inputs);
    let view = app.global::<generated::SettingsView>();
    let page_id = if online {
        SettingsPage::Online
    } else if page {
        SettingsPage::Library
    } else {
        SettingsPage::Root
    };
    let registry = rules::Inputs {
        is_mister: crt,
        crt_enabled: crt,
        debug_build: false,
        log_upload: false,
        can_pick_folder: false,
        can_scan_launchers: false,
        can_request_playtime_access: playtime,
        can_link_online: online,
        online_linked: online && !unlinked,
    };
    // The card and its row heights, from the rule the driver uses.
    let geometry = rules::page_geometry(&inputs);
    let mut offset = 0;
    let rows: Vec<generated::SettingsRow> = rules::page_rows(page_id.token(), &registry)
        .into_iter()
        .map(|row| {
            let mut out = generated::SettingsRow {
                kind: if row.is_field() {
                    RowKind::Field
                } else {
                    RowKind::Header
                },
                id: row.id().into(),
                enabled: true,
                y_offset: offset as f32,
                ..Default::default()
            };
            let height = match row {
                Row::Header(_) => geometry.header_height,
                Row::Field { id, control } => {
                    out.control = control.into();
                    out.enabled = !(unlinked && rules::needs_online_link(id));
                    match control {
                        Control::Toggle => {
                            out.checked = id == "showHidden" || (id == "playtimeSync" && !unlinked);
                        }
                        Control::Picker | Control::Info => {
                            out.value = match id {
                                "systemsLayout" => "grid",
                                "gamesLayout" => "list",
                                "onlineStatus" if unlinked => "unlinked",
                                "onlineStatus" => "linked",
                                "onlineWarp" => "active",
                                "onlineBackupSchedule" => "daily",
                                "onlineRemoteStatus" => "waiting",
                                _ => "auto",
                            }
                            .into();
                        }
                        Control::Action => {
                            out.busy = id == "runScraper";
                            out.status_key = if id == "playtimeAccess" {
                                ActionStatus::PlaytimeUnverified
                            } else if out.busy {
                                ActionStatus::Running
                            } else {
                                ActionStatus::None
                            };
                            out.value = rules::action_label_key(id, out.busy).into();
                        }
                        Control::Navigate => {}
                        Control::TriToggle => {
                            out.value = if unlinked { "off" } else { "mixed" }.into();
                        }
                    }
                    if out.status_key == ActionStatus::None {
                        geometry.row_height
                    } else {
                        geometry.row_height + geometry.action_band_height
                    }
                }
            };
            out.height = height as f32;
            offset += height;
            out
        })
        .collect();
    let profile = layouts::profile(ThemeId::current(&inputs), View::GamesGrid, &inputs);
    let Body::Grid { grid, footer } = profile.body else {
        return;
    };
    let viewport = geometry.rows_viewport;
    view.set_card_y(geometry.card_y as f32);
    view.set_card_height(geometry.card_height as f32);
    view.set_card_pad(geometry.pad as f32);
    view.set_hint_height(geometry.hint_height as f32);
    view.set_page(page_id);
    // The Online fixture seats the cursor on the account row so the band
    // scrolls to the group.
    let index = if online {
        rows.iter()
            .position(|row| {
                row.id
                    == if unlinked {
                        "onlineAllFeatures"
                    } else {
                        "onlineUnlinkAccount"
                    }
            })
            .and_then(|i| i32::try_from(i).ok())
            .unwrap_or(2)
    } else if page {
        2
    } else {
        1
    };
    view.set_index(index);
    let fields = |end: usize| {
        rows.iter()
            .take(end)
            .filter(|row| row.kind == RowKind::Field)
            .count() as i32
    };
    view.set_field_index(fields(index.max(0) as usize));
    view.set_field_count(fields(rows.len()));
    // Same scroll the driver applies, from the same rule, so the fixture
    // cannot quietly frame the band differently from the app.
    let spans: Vec<(f32, f32)> = rows.iter().map(|r| (r.y_offset, r.height)).collect();
    let (scroll, shown) = zaparoo_app::settings::band_extent(
        &spans,
        index.max(0) as usize,
        viewport as f32,
        geometry.pad as f32,
    );
    view.set_rows_height(viewport as f32);
    view.set_rows_clip_height(shown);
    view.set_scroll(scroll);
    view.set_rows(slint::ModelRc::new(slint::VecModel::from(rows)));

    if page {
        return;
    }
    let grid_y = derived.header_bottom + profile.status.top_margin + profile.status.strip_height;
    let bottom = if derived.tier == zaparoo_app::sizing::Tier::T240 {
        derived.help_bar_height
    } else {
        footer.grid_bottom_margin
    };
    let grid_height = (inputs.screen_height as i32 - grid_y - bottom).max(0);
    let insets = zaparoo_app::paged_grid::Insets {
        left: grid.left_inset,
        right: grid.right_inset,
        top: grid.top_inset,
        bottom: grid.bottom_inset,
        column_gap: grid.column_gap,
        row_gap: grid.row_gap,
    };
    let (columns, grid_rows) =
        rules::root_grid_shape(rules::PAGES.len(), inputs.swap_percentage_axes);
    let columns = i32::try_from(columns).unwrap_or(3);
    let grid_rows = i32::try_from(grid_rows).unwrap_or(2);
    let fit = zaparoo_app::paged_grid::fit(
        columns,
        grid_rows,
        inputs.screen_width as i32,
        grid_height,
        None,
        true,
        &insets,
    );
    let cells: Vec<GridCell> = rules::PAGES
        .iter()
        .map(|p| GridCell {
            label_key: p.id.into(),
            glyph_key: p.glyph.into(),
            ..Default::default()
        })
        .collect();
    view.set_cells(slint::ModelRc::new(slint::VecModel::from(cells)));
    view.set_columns(columns);
    view.set_rows_count(grid_rows);
    view.set_cell_width(fit.cell_width as f32);
    view.set_cell_height(fit.cell_height as f32);
    view.set_block_offset_x(fit.block_offset_x as f32);
    view.set_block_offset_y(fit.block_offset_y as f32);
    view.set_grid_y(grid_y as f32);
    view.set_grid_height(grid_height as f32);
}

/// The Games grid at its browse geometry: a page of captioned tiles with
/// a favorite heart, a tag suffix and a folder row, page 1 of 4 with more
/// pages loaded behind it, at the Games footer profile.
#[allow(
    clippy::too_many_arguments,
    reason = "one knob per fixture facet the screen names select"
)]
#[allow(
    clippy::too_many_lines,
    reason = "one fixture projects the same game rows into grid and list layouts"
)]
fn fixture_games(
    app: &App,
    scene_w: f64,
    scene_h: f64,
    crt: bool,
    i18n_titles: &[&str],
    i18n: bool,
    mode: GamesMode,
    anchor_index: Option<usize>,
) {
    use zaparoo_app::layouts::{self, Body, ThemeId, View};
    let inputs = sizing::Scene::of(app, scene_w, scene_h, crt).inputs();
    let derived = zaparoo_app::sizing::derive(&inputs);
    let profile = layouts::profile(ThemeId::current(&inputs), View::GamesGrid, &inputs);
    let Body::Grid { grid, footer } = profile.body else {
        return;
    };
    let t240 = derived.tier == zaparoo_app::sizing::Tier::T240;
    let flat = mode != GamesMode::Browse;
    let screen_h = inputs.screen_height as i32;
    let grid_y = derived.header_bottom + profile.status.top_margin + profile.status.strip_height;
    let label_height = if flat {
        inputs.pct_h(7.0)
    } else {
        footer.active_label_height
    };
    let bottom = if t240 {
        derived.help_bar_height + label_height
    } else if flat {
        inputs.pct_h(15.0)
    } else {
        footer.grid_bottom_margin
    };
    let grid_height = (screen_h - grid_y - bottom).max(0);
    let label_y = if flat {
        grid_y + grid_height
    } else {
        screen_h
            - if t240 {
                derived.help_bar_height
            } else {
                footer.active_label_bottom_margin
            }
            - label_height
    };
    let insets = zaparoo_app::paged_grid::Insets {
        left: grid.left_inset,
        right: grid.right_inset,
        top: grid.top_inset,
        bottom: grid.bottom_inset,
        column_gap: grid.column_gap,
        row_gap: grid.row_gap,
    };
    let (columns, rows) = (derived.games_grid_columns, derived.games_grid_rows);
    let fit = zaparoo_app::paged_grid::fit(
        columns,
        rows,
        inputs.screen_width as i32,
        grid_height,
        None,
        false,
        &insets,
    );
    let page_size = (columns * rows).max(1) as usize;
    let cells: Vec<GridCell> = (1..=page_size)
        .map(|i| GridCell {
            name: if i18n {
                i18n_titles[(i - 1) % i18n_titles.len()].into()
            } else if i == 1 && !flat {
                "Homebrew".into()
            } else {
                format!("Example Game Title {i}").into()
            },
            glyph_key: if i == 1 && !flat {
                "icons/Folder".into()
            } else {
                "".into()
            },
            top_label: if flat {
                ["Genesis", "Super Nintendo", "PlayStation"][i % 3].into()
            } else {
                "".into()
            },
            favorite: i == 2 || i == 7,
            tags: if i == 3 {
                "USA Rev A".into()
            } else if i == 1 && !flat {
                "42".into()
            } else {
                "".into()
            },
            // Covers still loading: Core's average colour stands in, and a
            // cover Core has not sized yet keeps the plain plate.
            placeholder: snapshot_cover_color(i),
            has_placeholder: (flat || i != 1) && i % 4 != 0,
            ..Default::default()
        })
        .collect();
    let selected = anchor_index.unwrap_or(0);
    let label = cells[selected.min(cells.len() - 1)].clone();
    if let Some(index) = anchor_index {
        let rect = zaparoo_app::paged_grid::cell_rect(
            &fit,
            &insets,
            (index / columns.max(1) as usize) as i32,
            (index % columns.max(1) as usize) as i32,
        );
        let overlays = app.global::<generated::Overlays>();
        overlays.set_context_anchor_x(rect.x as f32);
        overlays.set_context_anchor_y((grid_y + rect.y) as f32);
        overlays.set_context_anchor_w(rect.width as f32);
        overlays.set_context_anchor_h(rect.height as f32);
        overlays.set_context_anchor_radius(app.global::<Layout>().get_card_radius());
        overlays.set_context_anchor_zoomed(true);
    }
    // The list layout is a different view of the same model: rows in a
    // card beside the focused row's detail pane. Filled here too so the
    // `*-list` screens render what the driver would push, not an empty
    // card.
    let (list_row_height, list_visible) = list_metrics(app, scene_w, scene_h, crt, 0);
    let list_rows: Vec<GridCell> = (1..=list_visible)
        .map(|i| GridCell {
            name: if i == 1 && !flat {
                "Homebrew".into()
            } else {
                format!("Example Game Title {i}").into()
            },
            glyph_key: if i == 1 && !flat {
                "icons/Folder".into()
            } else {
                "".into()
            },
            tags: if i == 1 && !flat {
                "42".into()
            } else {
                "".into()
            },
            favorite: i == 2,
            ..Default::default()
        })
        .collect();
    let view = app.global::<GamesView>();
    view.set_list_rows(slint::ModelRc::new(slint::VecModel::from(list_rows)));
    view.set_list_sel(2);
    view.set_list_view_top(0);
    view.set_list_visible(i32::try_from(list_visible).unwrap_or(10));
    view.set_list_row_height(list_row_height);
    let paging = zaparoo_app::media_list::list_paging(2, 48, Some(48), !flat, list_visible, true);
    view.set_current_index(2);
    view.set_list_page(paging.current_page as i32);
    view.set_list_total_pages(paging.total_pages as i32);
    view.set_has_items_above(paging.has_items_above);
    view.set_has_items_below(paging.has_items_below);
    view.set_detail_title("Example Game Title 3".into());
    view.set_detail_placeholder(snapshot_cover_color(3));
    view.set_detail_has_placeholder(true);
    view.set_detail_rows(slint::ModelRc::new(slint::VecModel::from(vec![
        generated::DetailRow {
            key: "system".into(),
            value: "Atari Lynx".into(),
        },
        generated::DetailRow {
            key: "region".into(),
            value: "USA".into(),
        },
        generated::DetailRow {
            key: "players".into(),
            value: "2".into(),
        },
    ])));
    view.set_mode(mode);
    view.set_title("Atari Lynx".into());
    view.set_cells(slint::ModelRc::new(slint::VecModel::from(cells)));
    view.set_count(48);
    view.set_total_items(48);
    view.set_total_files(if flat { 0 } else { 47 });
    view.set_total_known(!flat);
    view.set_has_more(true);
    view.set_page(0);
    view.set_total_pages(4);
    view.set_has_pages_below(true);
    view.set_selected_local(i32::try_from(selected).unwrap_or(0));
    view.set_label_name(label.name);
    view.set_label_tags(label.tags);
    view.set_focus_ready(true);
    view.set_columns(columns);
    view.set_rows(rows);
    view.set_cell_width(fit.cell_width as f32);
    view.set_cell_height(fit.cell_height as f32);
    view.set_block_offset_x(fit.block_offset_x as f32);
    view.set_block_offset_y(fit.block_offset_y as f32);
    view.set_grid_y(grid_y as f32);
    view.set_grid_height(grid_height as f32);
    view.set_label_y(label_y as f32);
    view.set_label_height(label_height as f32);
}

#[allow(
    clippy::too_many_lines,
    reason = "one fixture projects the same systems into grid and list layouts"
)]
fn fixture_systems(app: &App, scene_w: f64, scene_h: f64, crt: bool, favorites: bool) {
    use zaparoo_app::layouts::{self, Body, ThemeId, View};
    let inputs = sizing::Scene::of(app, scene_w, scene_h, crt).inputs();
    let derived = zaparoo_app::sizing::derive(&inputs);
    let profile = layouts::profile(ThemeId::current(&inputs), View::SystemsGrid, &inputs);
    let Body::Grid { grid, footer } = profile.body else {
        return;
    };
    let grid_y = derived.header_bottom + profile.status.top_margin + profile.status.strip_height;
    let bottom = if derived.tier == zaparoo_app::sizing::Tier::T240 {
        derived.help_bar_height + footer.active_label_height
    } else {
        footer.grid_bottom_margin
    };
    let grid_height = (inputs.screen_height as i32 - grid_y - bottom).max(0);
    let insets = zaparoo_app::paged_grid::Insets {
        left: grid.left_inset,
        right: grid.right_inset,
        top: grid.top_inset,
        bottom: grid.bottom_inset,
        column_gap: grid.column_gap,
        row_gap: grid.row_gap,
    };
    let (columns, rows) = (derived.systems_grid_columns, derived.systems_grid_rows);
    let fit = zaparoo_app::paged_grid::fit(
        columns,
        rows,
        inputs.screen_width as i32,
        grid_height,
        None,
        false,
        &insets,
    );
    let page_size = (columns * rows).max(1) as usize;
    let cells: Vec<GridCell> = (13..13 + page_size)
        .map(|i| GridCell {
            name: format!("Example System {i}").into(),
            wordmark: true,
            hidden: i == 15,
            ..Default::default()
        })
        .collect();
    let (row_height, visible) = list_metrics(app, scene_w, scene_h, crt, 0);
    let list_rows: Vec<GridCell> = (13..13 + visible)
        .map(|i| GridCell {
            name: format!("Example System {i}").into(),
            wordmark: true,
            hidden: i == 15,
            ..Default::default()
        })
        .collect();
    let view = app.global::<SystemsView>();
    view.set_mode(if favorites {
        SystemsMode::Favorites
    } else {
        SystemsMode::Category
    });
    view.set_favorites_total(if favorites { 87 } else { -1 });
    view.set_label_count(if favorites { 12 } else { -1 });
    view.set_cells(slint::ModelRc::new(slint::VecModel::from(cells)));
    view.set_list_rows(slint::ModelRc::new(slint::VecModel::from(list_rows)));
    view.set_list_sel(2);
    view.set_list_view_top(12);
    view.set_list_scroll_top(12);
    view.set_list_visible(i32::try_from(visible).unwrap_or(10));
    view.set_list_row_height(row_height);
    view.set_current_index(14);
    view.set_list_page(1);
    view.set_list_total_pages(3);
    view.set_has_items_above(true);
    view.set_has_items_below(true);
    view.set_detail_title("Example System 15".into());
    view.set_detail_wordmark(true);
    view.set_detail_rows(slint::ModelRc::new(slint::VecModel::from(vec![
        generated::DetailRow {
            key: "category".into(),
            value: "Consoles".into(),
        },
        generated::DetailRow {
            key: "release_date".into(),
            value: "1990".into(),
        },
        generated::DetailRow {
            key: "manufacturer".into(),
            value: "Example Corp".into(),
        },
    ])));
    view.set_category("Console".into());
    view.set_count(30);
    view.set_page(1);
    view.set_total_pages(3);
    view.set_has_pages_above(true);
    view.set_has_pages_below(true);
    view.set_selected_local(2);
    view.set_label_name("Example System 15".into());
    view.set_label_hidden(true);
    view.set_focus_ready(true);
    view.set_columns(columns);
    view.set_rows(rows);
    view.set_cell_width(fit.cell_width as f32);
    view.set_cell_height(fit.cell_height as f32);
    view.set_block_offset_x(fit.block_offset_x as f32);
    view.set_block_offset_y(fit.block_offset_y as f32);
    view.set_grid_y(grid_y as f32);
    view.set_grid_height(grid_height as f32);
}

/// Stand-in cover colours, the kind Core averages from box art.
fn snapshot_cover_color(i: usize) -> slint::Color {
    const COLORS: [[u8; 3]; 6] = [
        [0x8a, 0x2f, 0x2a],
        [0x2c, 0x4f, 0x7c],
        [0x3d, 0x6b, 0x3a],
        [0x7a, 0x62, 0x2b],
        [0x5b, 0x3a, 0x6e],
        [0x2f, 0x6a, 0x6a],
    ];
    let [r, g, b] = COLORS[i % COLORS.len()];
    slint::Color::from_rgb_u8(r, g, b)
}

#[cfg(test)]
mod fixture_tests {
    use super::{fixture_screen, Screen};

    #[test]
    fn standalone_overlays_do_not_invent_screen_tokens() {
        for name in [
            "picker",
            "palette-picker",
            "crt-dialog",
            "token-error",
            "qr-docs",
        ] {
            assert_eq!(fixture_screen(name), Screen::None, "{name}");
        }
        for (name, screen) in [
            ("hub", Screen::Hub),
            ("crt-systems", Screen::Systems),
            ("favorite-systems", Screen::FavoriteSystems),
            ("favorites", Screen::Favorites),
            ("recents", Screen::Recents),
            ("search", Screen::Search),
            ("crt-search-scoped", Screen::Search),
            ("search-results", Screen::SearchResults),
            ("game-info", Screen::Games),
            ("settings-page", Screen::Settings),
            ("about", Screen::About),
            ("update-intro", Screen::Update),
            ("crt-update-details", Screen::Update),
        ] {
            assert_eq!(fixture_screen(name), screen, "{name}");
        }
    }
}
