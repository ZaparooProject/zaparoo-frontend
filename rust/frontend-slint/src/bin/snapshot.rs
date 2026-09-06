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
use generated::{App, CategoryTile, GameTile, GlyphSource, LetterBucket, MenuEntry, Sizing, Theme};

#[path = "../fonts.rs"]
mod fonts;
#[path = "../glyphs.rs"]
mod glyphs;
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
    let radius_pct: Option<f32> = args.get(5).and_then(|a| a.parse().ok());

    slint::platform::set_platform(Box::new(SnapshotPlatform)).unwrap();

    let app = App::new().unwrap();
    // Same font stack as the device build: the embedded script faces and
    // their fallback wiring.
    fonts::register_embedded_fonts();
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
    }
    app.global::<GlyphSource>().on_glyph(|key, px| {
        glyphs::render(key.as_str(), px.round().max(0.0) as u32).unwrap_or_default()
    });
    let crt = screen.starts_with("crt-") || screen == "calibration";
    let ccw = screen.contains("ccw");
    let tate = screen.contains("tate") || ccw;
    let list = screen.contains("list");
    app.global::<Theme>().set_crt(crt);
    app.global::<Sizing>().set_crt(crt);
    app.global::<Sizing>().set_swap_axes(tate);
    let shell = app.global::<generated::Shell>();
    shell.set_orientation(if ccw {
        "ccw".into()
    } else if tate {
        "cw".into()
    } else {
        "horizontal".into()
    });
    shell.set_browse_list_layout(list);
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

    // Representative real catalog: Core emits singular category names.
    let cats = [
        "Arcade", "Console", "Computer", "Handheld", "Media", "Other", "Software",
    ];
    let tiles: Vec<CategoryTile> = cats
        .iter()
        .map(|name| CategoryTile {
            name: (*name).into(),
            hidden: *name == "Software",
        })
        .collect();
    app.global::<generated::HubView>()
        .set_categories(slint::ModelRc::new(slint::VecModel::from(tiles)));
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
    let games: Vec<GameTile> = (1..=12)
        .map(|i| GameTile {
            name: if i18n {
                i18n_titles[i - 1].into()
            } else {
                format!("Example Game Title {i}").into()
            },
            path: format!("/g/{i}").into(),
            cover: slint::Image::default(),
            has_cover: false,
            is_favorite: i == 2 || i == 7,
            thumb: slint::Image::default(),
            has_thumb: false,
            tags: if i == 3 {
                "USA Rev A".into()
            } else {
                "".into()
            },
        })
        .collect();
    app.global::<generated::GamesView>()
        .set_games(slint::ModelRc::new(slint::VecModel::from(games)));
    app.global::<generated::GamesView>()
        .set_games_system("Atari Lynx".into());
    app.global::<generated::GamesView>().set_games_page(0);
    app.global::<generated::GamesView>()
        .set_games_has_more(true);
    // "context" renders the games screen with the context menu open.
    if screen == "context" {
        app.global::<generated::GamesView>().set_games_index(6);
        app.global::<generated::Overlays>()
            .set_context_entries(slint::ModelRc::new(slint::VecModel::from(vec![
                MenuEntry {
                    id: "toggle_favorite".into(),
                    label: "Add to favorites".into(),
                },
                MenuEntry {
                    id: "write_card".into(),
                    label: "Write to NFC token".into(),
                },
                MenuEntry {
                    id: "more_info".into(),
                    label: "Game info".into(),
                },
                MenuEntry {
                    id: "launch_game".into(),
                    label: "Launch game".into(),
                },
            ])));
        app.global::<generated::Overlays>().set_context_index(1);
        app.global::<generated::Overlays>().set_context_open(true);
    }
    // "letters" renders the games screen with the letter picker open.
    if screen == "letters" {
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
        app.global::<generated::Overlays>().set_letter_columns(9);
        app.global::<generated::Overlays>().set_letter_index(4);
        app.global::<generated::Overlays>().set_letter_open(true);
    }
    app.global::<generated::Shell>()
        .set_about_version_line("Version 1.2.2 (Slint demo)".into());
    // Systems fixtures: a paged category (page 2 of 3) with no logo
    // files on disk, so the name fallback renders.
    let sys_tiles: Vec<generated::SystemTile> = (13..=24)
        .map(|i| generated::SystemTile {
            id: format!("Sys{i}").into(),
            name: format!("Example System {i}").into(),
            logo: slint::Image::default(),
            logo_focus: slint::Image::default(),
            has_logo: false,
            // One hidden entry exercises the dim + badge rendering.
            hidden: i == 15,
        })
        .collect();
    let sysv = app.global::<generated::SystemsView>();
    sysv.set_systems(slint::ModelRc::new(slint::VecModel::from(sys_tiles)));
    sysv.set_systems_category("Console".into());
    sysv.set_systems_page(1);
    sysv.set_systems_total_pages(3);
    sysv.set_systems_index(2);
    app.global::<generated::Shell>()
        .set_status_keys(slint::ModelRc::new(slint::VecModel::from(vec![
            slint::SharedString::from("Bluetooth"),
            slint::SharedString::from("WiredNetwork"),
            slint::SharedString::from("WiFi"),
            slint::SharedString::from("NFC"),
        ])));
    // Settings fixtures: root category grid, or a field page with
    // every control kind when "settings-page" is requested.
    let setv = app.global::<generated::SettingsView>();
    if screen == "settings-page" {
        setv.set_settings_page("pageBrowsing".into());
        setv.set_settings_title("Browsing".into());
        setv.set_settings_index(3);
        let fields = vec![
            (
                "field",
                "browseLayout",
                "Browsing layout",
                "Grid view",
                "picker",
                false,
            ),
            (
                "field",
                "systemLogoStyle",
                "System logos",
                "Tinted",
                "picker",
                false,
            ),
            (
                "field",
                "mediaImageType",
                "Preferred artwork",
                "Auto",
                "picker",
                false,
            ),
            (
                "field",
                "showHidden",
                "Show hidden items",
                "",
                "toggle",
                true,
            ),
            (
                "field",
                "showOriginalFilenames",
                "Show original filenames",
                "",
                "toggle",
                false,
            ),
        ];
        let rows: Vec<generated::SettingsField> = fields
            .into_iter()
            .map(
                |(kind, id, label, value, control, checked)| generated::SettingsField {
                    kind: kind.into(),
                    id: id.into(),
                    label: label.into(),
                    value: value.into(),
                    control: control.into(),
                    checked,
                    enabled: true,
                },
            )
            .collect();
        setv.set_settings_fields(slint::ModelRc::new(slint::VecModel::from(rows)));
    } else {
        setv.set_settings_page("".into());
        setv.set_settings_title("Settings".into());
        setv.set_settings_index(1);
        let pages = [
            ("pageDisplayInterface", "Display"),
            ("pageBrowsing", "Browsing"),
            ("pageLanguage", "Language"),
            ("pageControlsInput", "Controls"),
            ("pageLibraryData", "Library"),
            ("pageSupportAbout", "Support"),
        ];
        let rows: Vec<generated::SettingsField> = pages
            .iter()
            .map(|(id, label)| generated::SettingsField {
                kind: "field".into(),
                id: (*id).into(),
                label: (*label).into(),
                value: (*label).into(),
                control: "action".into(),
                checked: false,
                enabled: true,
            })
            .collect();
        setv.set_settings_fields(slint::ModelRc::new(slint::VecModel::from(rows)));
    }
    if screen == "saver" {
        app.global::<generated::Shell>().set_saver_armed(true);
    }
    // Startup decision dialog fixture (the quit-confirm kind shows
    // the two-button row with default focus on "No").
    if screen == "dialog" {
        let ov = app.global::<generated::Overlays>();
        ov.set_dialog_kind("quit_confirm".into());
        ov.set_dialog_title("Quit Zaparoo Frontend?".into());
        ov.set_dialog_body("Are you sure you want to exit?".into());
        ov.set_dialog_buttons(slint::ModelRc::new(slint::VecModel::from(vec![
            slint::SharedString::from("Yes"),
            slint::SharedString::from("No"),
        ])));
        ov.set_dialog_focus(1);
        ov.set_dialog_open(true);
    }
    // Detailed-list layout fixtures: windowed rows + detail pane.
    if list && !screen.contains("systems") {
        let gv = app.global::<generated::GamesView>();
        gv.set_games_list_layout(true);
        let rows: Vec<GameTile> = (13..=22)
            .map(|i| GameTile {
                name: format!("Example Game {i}").into(),
                path: format!("/games/example-{i}.bin").into(),
                cover: slint::Image::default(),
                has_cover: false,
                is_favorite: i == 15,
                thumb: slint::Image::default(),
                has_thumb: false,
                tags: if i == 16 {
                    "Rev A · USA".into()
                } else {
                    "".into()
                },
            })
            .collect();
        gv.set_list_rows(slint::ModelRc::new(slint::VecModel::from(rows)));
        gv.set_list_sel(3);
        gv.set_list_view_top(12);
        gv.set_list_total(64);
        gv.set_list_visible(10);
        gv.set_detail_title("Example Game 16".into());
        gv.set_detail_path("/games/example-16.bin".into());
        gv.set_detail_has_cover(false);
        let meta = vec![
            ("Year", "1994"),
            ("Genre", "Platformer"),
            ("Players", "1-2"),
            ("Developer", "Example Corp"),
            ("Publisher", "Example Publishing"),
        ];
        let meta_rows: Vec<generated::DetailRow> = meta
            .into_iter()
            .map(|(label, value)| generated::DetailRow {
                label: label.into(),
                value: value.into(),
            })
            .collect();
        gv.set_detail_rows(slint::ModelRc::new(slint::VecModel::from(meta_rows)));
        gv.set_detail_description(
            "A demo description long enough to wrap across several lines in the \
             detail pane, proving the metadata table and body copy lay out the \
             way BrowseDetailPane does in the Qt frontend."
                .into(),
        );
        gv.set_games_system("Console".into());
    }
    if screen == "calibration" {
        let overlays = app.global::<generated::Overlays>();
        overlays.set_crt_h_offset(4);
        overlays.set_crt_v_offset(-2);
        overlays.set_crt_calibration_open(true);
    }
    let base = if screen.starts_with("route-") {
        "hub"
    } else if screen == "context"
        || screen == "letters"
        || (list && !screen.contains("systems"))
        || screen.contains("games")
    {
        "games"
    } else if screen == "settings-page" || screen == "crt-settings" {
        "settings"
    } else if screen == "saver" || screen == "dialog" || screen == "calibration" {
        "hub"
    } else if screen.contains("systems") {
        "systems"
    } else {
        screen.as_str()
    };
    app.global::<generated::Shell>()
        .set_active_screen(base.into());
    if screen == "route-forward" {
        let shell = app.global::<generated::Shell>();
        shell.set_route_slide_anim(false);
        shell.set_route_from_screen("hub".into());
        shell.set_route_to_screen("systems".into());
        shell.set_route_slide_dir(1);
        shell.set_route_transitioning(true);
        shell.set_route_page_slide(0.5);
    }
    if screen == "route-back" {
        let shell = app.global::<generated::Shell>();
        shell.set_active_screen("systems".into());
        shell.set_route_slide_anim(false);
        shell.set_route_from_screen("systems".into());
        shell.set_route_to_screen("hub".into());
        shell.set_route_slide_dir(-1);
        shell.set_route_transitioning(true);
        shell.set_route_page_slide(-0.5);
    }
    if screen == "route-cached" {
        let shell = app.global::<generated::Shell>();
        shell.set_active_screen("systems".into());
        shell.set_route_from_screen("hub".into());
        shell.set_route_to_screen("systems".into());
        shell.set_route_slide_dir(1);
        shell.set_route_transitioning(true);
        shell.set_route_cached_transition(true);
    }
    if screen.contains("cached") {
        app.global::<generated::SystemsView>()
            .set_systems_cached_transition(true);
        app.global::<generated::GamesView>()
            .set_games_cached_transition(true);
    }
    if let Some(pct) = radius_pct {
        app.global::<Sizing>().set_corner_radius_pct(pct);
    }

    let late = std::env::var("SNAPSHOT_LATE_SET").is_ok();
    let window = WINDOW.with(|w| w.borrow().clone()).unwrap();
    if late {
        // Regression probe for the globals refactor: first paint with
        // the empty defaults, then push state the way the runtime does
        // (after the first frame) and require a second dirty frame.
        let empty: Vec<CategoryTile> = Vec::new();
        app.global::<generated::HubView>()
            .set_categories(slint::ModelRc::new(slint::VecModel::from(empty)));
        let mut first = vec![PremultipliedRgbaColor::default(); width as usize * height as usize];
        let drew = window.draw_if_needed(|renderer| {
            renderer.set_rendering_rotation(rotation);
            renderer.render(first.as_mut_slice(), width as usize);
        });
        assert!(drew, "first frame had nothing to draw");
        let cats = ["Arcade", "Console", "Computer", "Handheld"];
        let tiles: Vec<CategoryTile> = cats
            .iter()
            .map(|name| CategoryTile {
                name: (*name).into(),
                hidden: false,
            })
            .collect();
        app.global::<generated::HubView>()
            .set_categories(slint::ModelRc::new(slint::VecModel::from(tiles)));
        let mut probe = vec![PremultipliedRgbaColor::default(); width as usize * height as usize];
        let dirty_model = window.draw_if_needed(|r| {
            r.render(probe.as_mut_slice(), width as usize);
        });
        println!("dirty after HubView.categories set: {dirty_model}");
        app.global::<generated::Shell>()
            .set_status_text("probe status".into());
        let dirty_text = window.draw_if_needed(|r| {
            r.render(probe.as_mut_slice(), width as usize);
        });
        println!("dirty after Shell.status-text set: {dirty_text}");
        app.global::<generated::HubView>().set_hub_category_index(2);
        let dirty_idx = window.draw_if_needed(|r| {
            r.render(probe.as_mut_slice(), width as usize);
        });
        println!("dirty after HubView.hub-category-index set: {dirty_idx}");
        // Games screen (alias-block pattern): does a late set reach it?
        app.global::<generated::Shell>()
            .set_active_screen("games".into());
        let _ = window.draw_if_needed(|r| {
            r.render(probe.as_mut_slice(), width as usize);
        });
        app.global::<generated::GamesView>().set_games_index(5);
        let dirty_games_idx = window.draw_if_needed(|r| {
            r.render(probe.as_mut_slice(), width as usize);
        });
        println!("dirty after GamesView.games-index set (alias pattern): {dirty_games_idx}");
        app.global::<generated::GamesView>()
            .set_games_system("Probe System".into());
        let dirty_games_sys = window.draw_if_needed(|r| {
            r.render(probe.as_mut_slice(), width as usize);
        });
        println!("dirty after GamesView.games-system set (alias pattern): {dirty_games_sys}");
        // Systems / Settings / About: every screen must react to a
        // late global write (the partially-applied-refactor class).
        app.global::<generated::Shell>()
            .set_active_screen("systems".into());
        let _ = window.draw_if_needed(|r| {
            r.render(probe.as_mut_slice(), width as usize);
        });
        app.global::<generated::SystemsView>()
            .set_systems_category("Probe Category".into());
        let dirty_sys = window.draw_if_needed(|r| {
            r.render(probe.as_mut_slice(), width as usize);
        });
        println!("dirty after SystemsView.systems-category set: {dirty_sys}");
        app.global::<generated::Shell>()
            .set_active_screen("settings".into());
        app.global::<generated::SettingsView>()
            .set_settings_page("pageBrowsing".into());
        let srows: Vec<generated::SettingsField> = ["A", "B"]
            .iter()
            .map(|l| generated::SettingsField {
                kind: "field".into(),
                id: (*l).into(),
                label: (*l).into(),
                value: "v".into(),
                control: "picker".into(),
                checked: false,
                enabled: true,
            })
            .collect();
        app.global::<generated::SettingsView>()
            .set_settings_fields(slint::ModelRc::new(slint::VecModel::from(srows)));
        let _ = window.draw_if_needed(|r| {
            r.render(probe.as_mut_slice(), width as usize);
        });
        // The startup fixture may already sit on index 1; move to 0 so
        // the write is a real change and must dirty a frame.
        app.global::<generated::SettingsView>()
            .set_settings_index(0);
        let dirty_set = window.draw_if_needed(|r| {
            r.render(probe.as_mut_slice(), width as usize);
        });
        println!("dirty after SettingsView.settings-index set: {dirty_set}");
        app.global::<generated::Shell>()
            .set_active_screen("about".into());
        let _ = window.draw_if_needed(|r| {
            r.render(probe.as_mut_slice(), width as usize);
        });
        app.global::<generated::Shell>()
            .set_about_version_line("Probe Version".into());
        let dirty_about = window.draw_if_needed(|r| {
            r.render(probe.as_mut_slice(), width as usize);
        });
        println!("dirty after Shell.about-version-line set: {dirty_about}");
        app.global::<generated::Shell>()
            .set_active_screen("hub".into());
        let _ = window.draw_if_needed(|r| {
            r.render(probe.as_mut_slice(), width as usize);
        });
        let tiles2: Vec<CategoryTile> = cats
            .iter()
            .map(|name| CategoryTile {
                name: (*name).into(),
                hidden: false,
            })
            .collect();
        app.global::<generated::HubView>()
            .set_categories(slint::ModelRc::new(slint::VecModel::from(tiles2)));
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
    img.save(&out).expect("write png");
    println!("wrote {out}");
}
