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
use generated::{App, GlyphSource, GridCell, LetterBucket, MenuEntry, Sizing, Theme};
#[allow(
    unused_imports,
    reason = "reached through crate:: paths from the shared sizing adapter"
)]
use generated::{GamesView, Layout, Shell, SystemsView};

#[path = "../fonts.rs"]
mod fonts;
#[path = "../glyphs.rs"]
mod glyphs;
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
    let shell = app.global::<Shell>();
    shell.set_orientation(if ccw {
        "ccw".into()
    } else if tate {
        "cw".into()
    } else {
        "horizontal".into()
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
    sizing::apply_scene(&app, sizing::Scene::of(&app, scene_w, scene_h, crt));
    // Chrome fixtures: a full HUD with a battery reading, and an
    // indexing run in the status line so the track and percent render.
    shell.set_status_icons_enabled(true);
    shell.set_has_battery(true);
    shell.set_battery_percent(62);
    shell.set_clock_text("10:32".into());
    let status = app.global::<generated::Status>();
    status.set_kind("indexing".into());
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
    let games_mode = if screen.contains("favorites") {
        "favorites"
    } else if screen.contains("recents") {
        "recents"
    } else {
        "games"
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
                        id: format!("alternate_version:{index}").into(),
                        label: (*name).into(),
                        label_key: "".into(),
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
                    id: "more_info".into(),
                    label: "".into(),
                    label_key: "more_info".into(),
                },
                MenuEntry {
                    id: "toggle_favorite".into(),
                    label: "".into(),
                    label_key: "favorite:add".into(),
                },
                MenuEntry {
                    id: "write_card".into(),
                    label: "".into(),
                    label_key: "write_card".into(),
                },
                MenuEntry {
                    id: "qr_code".into(),
                    label: "".into(),
                    label_key: "qr_code".into(),
                },
                MenuEntry {
                    id: "add_to_hub".into(),
                    label: "".into(),
                    label_key: "add_to_hub".into(),
                },
                MenuEntry {
                    id: "scrape_game".into(),
                    label: "".into(),
                    label_key: "scrape_game".into(),
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
    app.global::<Shell>()
        .set_about_version_line("Version 1.2.2 (Slint demo)".into());
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
    fixture_settings(
        &app,
        scene_w,
        scene_h,
        crt,
        screen.contains("settings-page"),
    );
    // "setup" renders the scrape setup form over Settings; "setup-picker"
    // renders its scope page.
    if screen.contains("setup") {
        let inputs = sizing::Scene::of(&app, scene_w, scene_h, crt).inputs();
        let row_h = inputs.pct_h(8.0);
        let sv = app.global::<generated::SetupModalView>();
        let rows: Vec<generated::SettingsRow> = [
            ("source", "picker", "source", "Screenscraper"),
            ("systems", "picker", "category", "Console"),
            ("rescrape", "toggle", "", ""),
            ("startImport", "action", "", ""),
        ]
        .iter()
        .enumerate()
        .map(|(i, (id, control, value, name))| generated::SettingsRow {
            kind: "field".into(),
            id: (*id).into(),
            control: (*control).into(),
            value: (*value).into(),
            value_name: (*name).into(),
            checked: *id == "rescrape",
            enabled: true,
            y_offset: (i as i32 * row_h) as f32,
            height: row_h as f32,
            ..Default::default()
        })
        .collect();
        sv.set_kind("scrape".into());
        sv.set_rows(slint::ModelRc::new(slint::VecModel::from(rows)));
        sv.set_index(1);
        if screen.contains("picker") {
            let entries = [
                ("all", ""),
                ("category", "Console"),
                ("category", "Handheld"),
                ("system", "Nintendo Entertainment System"),
                ("system", "Super Nintendo"),
                ("system", "Mega Drive"),
                ("system", "Neo Geo"),
            ];
            let picker: Vec<generated::SetupPickerRow> = entries
                .iter()
                .map(|(kind, name)| generated::SetupPickerRow {
                    kind: (*kind).into(),
                    name: (*name).into(),
                })
                .collect();
            sv.set_picker_page(true);
            sv.set_picker_title("systems".into());
            sv.set_picker_rows(slint::ModelRc::new(slint::VecModel::from(picker)));
            sv.set_picker_sel(1);
            sv.set_has_below(true);
        }
        sv.set_open(true);
    }
    // "picker" renders the shared list picker over a long option list,
    // to check its scrolling window.
    if screen == "picker" || screen == "crt-picker" {
        let ov = app.global::<generated::Overlays>();
        let entries: Vec<MenuEntry> = zaparoo_app::settings::LANGUAGES
            .iter()
            .map(|value| MenuEntry {
                id: (*value).into(),
                label: "".into(),
                label_key: "".into(),
            })
            .collect();
        ov.set_list_setting_id("language".into());
        ov.set_list_entries(slint::ModelRc::new(slint::VecModel::from(entries)));
        ov.set_list_index(6);
        ov.set_list_open(true);
    }
    // "log-upload" renders the uploader's finished state with its link.
    if screen.contains("log-upload") {
        let lv = app.global::<generated::LogUploadView>();
        lv.set_phase(
            if screen.contains("failed") {
                "failed"
            } else {
                "done"
            }
            .into(),
        );
        lv.set_url("https://logs.zaparoo.org/a1b2c3d4".into());
        if let Some((image, modules)) = qr::qr_image("https://logs.zaparoo.org/a1b2c3d4") {
            lv.set_qr(image);
            lv.set_qr_modules(i32::try_from(modules).unwrap_or(0));
        }
        lv.set_open(true);
    }
    // "dialog" renders the two-button decision dialog; "alert" renders
    // the one-button failure alert, both through the same vocabulary
    // the router drives.
    if screen == "dialog" || screen == "alert" || screen == "notice" {
        let overlays = app.global::<generated::Overlays>();
        if screen == "notice" {
            overlays.set_dialog_kind("notice".into());
            overlays.set_dialog_buttons(slint::ModelRc::new(slint::VecModel::from(vec![
                slint::SharedString::from("i_understand"),
            ])));
            overlays.set_dialog_focus(0);
        } else if screen == "alert" {
            overlays.set_dialog_kind("action_error".into());
            overlays.set_dialog_detail("launch".into());
            overlays.set_dialog_arg("Sonic the Hedgehog".into());
            overlays.set_dialog_buttons(slint::ModelRc::new(slint::VecModel::from(vec![
                slint::SharedString::from("ok"),
            ])));
            overlays.set_dialog_focus(0);
        } else {
            overlays.set_dialog_kind("quit_confirm".into());
            overlays.set_dialog_buttons(slint::ModelRc::new(slint::VecModel::from(vec![
                slint::SharedString::from("yes"),
                slint::SharedString::from("no"),
            ])));
            overlays.set_dialog_focus(1);
        }
        overlays.set_dialog_open(true);
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
        || screen == "context-alt"
        || screen == "letters"
        || (list && !screen.contains("systems"))
        || screen.contains("games")
    {
        "games"
    } else if screen.contains("favorite-systems") {
        "favorite-systems"
    } else if screen.contains("favorites") {
        "favorites"
    } else if screen.contains("recents") {
        "recents"
    } else if screen.contains("settings")
        || screen.contains("setup")
        || screen.contains("log-upload")
    {
        "settings"
    } else if screen == "saver"
        || screen == "dialog"
        || screen == "alert"
        || screen == "notice"
        || screen == "calibration"
    {
        "hub"
    } else if screen.contains("systems") {
        "systems"
    } else if screen.contains("hub") {
        "hub"
    } else {
        screen.as_str()
    };
    app.global::<Shell>().set_active_screen(base.into());
    if screen == "route-forward" {
        let shell = app.global::<Shell>();
        shell.set_route_slide_anim(false);
        shell.set_route_from_screen("hub".into());
        shell.set_route_to_screen("systems".into());
        shell.set_route_slide_dir(1);
        shell.set_route_transitioning(true);
        shell.set_route_page_slide(0.5);
    }
    if screen == "route-back" {
        let shell = app.global::<Shell>();
        shell.set_active_screen("systems".into());
        shell.set_route_slide_anim(false);
        shell.set_route_from_screen("systems".into());
        shell.set_route_to_screen("hub".into());
        shell.set_route_slide_dir(-1);
        shell.set_route_transitioning(true);
        shell.set_route_page_slide(-0.5);
    }
    if screen == "route-cached" {
        let shell = app.global::<Shell>();
        shell.set_active_screen("systems".into());
        shell.set_route_from_screen("hub".into());
        shell.set_route_to_screen("systems".into());
        shell.set_route_slide_dir(1);
        shell.set_route_transitioning(true);
        shell.set_route_cached_transition(true);
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
        app.global::<Shell>().set_status_text("probe status".into());
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
        app.global::<Shell>().set_active_screen("games".into());
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
        app.global::<Shell>().set_active_screen("systems".into());
        let _ = window.draw_if_needed(|r| {
            r.render(probe.as_mut_slice(), width as usize);
        });
        app.global::<SystemsView>()
            .set_category("Probe Category".into());
        let dirty_sys = window.draw_if_needed(|r| {
            r.render(probe.as_mut_slice(), width as usize);
        });
        println!("dirty after SystemsView.category set: {dirty_sys}");
        app.global::<Shell>().set_active_screen("settings".into());
        app.global::<generated::SettingsView>()
            .set_page("pageLanguage".into());
        let srows: Vec<generated::SettingsRow> = ["language", "region"]
            .iter()
            .enumerate()
            .map(|(i, id)| generated::SettingsRow {
                kind: "field".into(),
                id: (*id).into(),
                control: "picker".into(),
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
        app.global::<Shell>().set_active_screen("about".into());
        let _ = window.draw_if_needed(|r| {
            r.render(probe.as_mut_slice(), width as usize);
        });
        app.global::<Shell>()
            .set_about_version_line("Probe Version".into());
        let dirty_about = window.draw_if_needed(|r| {
            r.render(probe.as_mut_slice(), width as usize);
        });
        println!("dirty after Shell.about-version-line set: {dirty_about}");
        app.global::<Shell>().set_active_screen("hub".into());
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

/// Push a Hub page built from a representative layout through the same
/// rules the app uses (`zaparoo_app::hub`), at the scene's geometry.
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
        focused.reason.as_str().into()
    } else {
        "".into()
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
fn fixture_settings(app: &App, scene_w: f64, scene_h: f64, crt: bool, page: bool) {
    use zaparoo_app::layouts::{self, Body, ThemeId, View};
    use zaparoo_app::settings::{self as rules, Control, Row};
    let inputs = sizing::Scene::of(app, scene_w, scene_h, crt).inputs();
    let derived = zaparoo_app::sizing::derive(&inputs);
    let view = app.global::<generated::SettingsView>();
    let page_id = if page { "pageLibraryData" } else { "" };
    let registry = rules::Inputs {
        is_mister: crt,
        crt_enabled: crt,
        debug_build: false,
    };
    let row_h = inputs.pct_h(8.0);
    let header_h = inputs.pct_h(5.0);
    let band = inputs.pct_h(3.2);
    let mut offset = 0;
    let rows: Vec<generated::SettingsRow> = rules::page_rows(page_id, &registry)
        .into_iter()
        .filter(|row| row.id() != "uploadLog")
        .map(|row| {
            let mut out = generated::SettingsRow {
                kind: if row.is_field() { "field" } else { "header" }.into(),
                id: row.id().into(),
                enabled: true,
                y_offset: offset as f32,
                ..Default::default()
            };
            let height = match row {
                Row::Header(_) => header_h,
                Row::Field { id, control } => {
                    out.control = match control {
                        Control::Toggle => "toggle",
                        Control::Picker => "picker",
                        Control::Action => "action",
                        Control::Navigate => "navigate",
                    }
                    .into();
                    match control {
                        Control::Toggle => out.checked = id == "showHidden",
                        Control::Picker => {
                            out.value = match id {
                                "systemsLayout" => "grid",
                                "gamesLayout" => "list",
                                _ => "auto",
                            }
                            .into();
                        }
                        Control::Action => {
                            out.busy = id == "runScraper";
                            out.value = rules::action_label_key(id, out.busy).into();
                        }
                        Control::Navigate => {}
                    }
                    if out.busy {
                        row_h + band
                    } else {
                        row_h
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
    // The rows viewport, as the driver computes it.
    let t240 = derived.tier == zaparoo_app::sizing::Tier::T240;
    let card_y = derived.header_bottom
        + profile.status.top_margin
        + profile.status.strip_height
        + inputs.pct_h(2.0);
    let card_bottom = if t240 {
        derived.help_bar_height + inputs.pct_h(2.0)
    } else {
        inputs.pct_h(8.0)
    };
    let card_h = (inputs.screen_height as i32 - card_y - card_bottom).max(0);
    let hint = 2 * (f64::from(derived.font_body) * 1.362).ceil() as i32;
    let viewport = (card_h - 2 * inputs.pct_h(2.0) - hint - inputs.pct_h(0.5)).max(0);
    view.set_page(page_id.into());
    view.set_index(if page { 2 } else { 1 });
    view.set_rows_height(viewport as f32);
    view.set_scroll(0.0);
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
/// pages loaded behind it (GamesScreen.qml's footer profile).
#[allow(
    clippy::too_many_arguments,
    reason = "one knob per fixture facet the screen names select"
)]
fn fixture_games(
    app: &App,
    scene_w: f64,
    scene_h: f64,
    crt: bool,
    i18n_titles: &[&str],
    i18n: bool,
    mode: &str,
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
    let flat = mode != "games";
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
    view.set_detail_title("Example Game Title 3".into());
    view.set_detail_cover_absent(true);
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
    view.set_mode(mode.into());
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
        "favorite-systems".into()
    } else {
        "systems".into()
    });
    view.set_favorites_total(if favorites { 87 } else { -1 });
    view.set_label_count(if favorites { 12 } else { -1 });
    view.set_cells(slint::ModelRc::new(slint::VecModel::from(cells)));
    view.set_list_rows(slint::ModelRc::new(slint::VecModel::from(list_rows)));
    view.set_list_sel(2);
    view.set_list_view_top(12);
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
