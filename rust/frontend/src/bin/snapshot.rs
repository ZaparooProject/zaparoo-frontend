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
    ActionStatus, AppCue, ControlKind, DeviceValue, DialogButton, DialogKind, DisabledReason,
    ErrorKind, GamesMode, LogPhase, OnlineLinkPhase, Orientation, PairFailure, PairPhase, RowKind,
    ScopeKind, Screen, SettingsPage, SetupKind, SetupPicker, StatusKind, SystemsMode,
    VideoStandard,
};
use generated::{
    App, Backdrop, Brand, GlyphSource, GridCell, LetterBucket, MenuEntry, Sizing, Theme,
};
#[allow(
    unused_imports,
    reason = "reached through crate:: paths from the shared sizing adapter"
)]
use generated::{FileBrowserMode, FileBrowserView, GamesView, Layout, Motion, Shell, SystemsView};
use generated::{KeyCell, KeyKind, SearchPane, SearchPaneRow, SearchScope, SearchView, SearchZone};
use generated::{
    MusicButton, MusicControl, MusicPage, MusicUnavailable, MusicView, PlaybackState, RepeatMode,
};
#[path = "../music_layout.rs"]
mod music_layout;
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
    // `…-bg` renders the screen over a prepared background image: the one
    // `ZAPAROO_SNAPSHOT_BACKGROUND` names, or a generated one.
    let (screen, background) = match screen.strip_suffix("-bg") {
        Some(base) => (base, true),
        None => (
            screen,
            std::env::var_os("ZAPAROO_SNAPSHOT_BACKGROUND").is_some(),
        ),
    };
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
    app.global::<GlyphSource>()
        .on_raster(|key, px, tint, pixel_grid| {
            glyphs::render(key.as_str(), px.round().max(0.0) as u32, tint, pixel_grid)
                .unwrap_or_default()
        });
    let crt = screen.starts_with("crt-") || screen.starts_with("calibration");
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
    // `…-textlist` screens: the list with no detail pane.
    shell.set_browse_list_only(screen.contains("textlist"));
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
    // ZAPAROO_SNAPSHOT_MENU_ROW=off renders with the Menu chip row turned
    // off in Settings, which gives its band back to the content.
    app.global::<Sizing>()
        .set_menu_row_enabled(std::env::var("ZAPAROO_SNAPSHOT_MENU_ROW").as_deref() != Ok("off"));
    // ZAPAROO_SNAPSHOT_HELP_BAR=off renders with the help bar hidden in
    // Settings, which gives its height back to every screen.
    app.global::<Sizing>()
        .set_help_bar_enabled(std::env::var("ZAPAROO_SNAPSHOT_HELP_BAR").as_deref() != Ok("off"));
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
    // Background music plays on the Hub and the music screens, so the
    // header's music note renders there.
    shell.set_music_playing(screen.contains("music") || screen.contains("hub"));

    // Representative persisted Hub layout: Resume first, the detected
    // categories, then the built-in actions, with one hidden system
    // shortcut and one folder shortcut the user added.
    // "hub-deck" focuses the pinned deck.
    let hub_selected = if screen.ends_with("hub-deck") {
        fixture_hub_items()
            .iter()
            .position(|item| item.kind == "collection")
            .unwrap_or(1)
    } else {
        1
    };
    fixture_hub(&app, scene_w, scene_h, crt, hub_selected);
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
    } else if fixture_screen(screen) == Screen::Decks {
        GamesMode::Decks
    } else if fixture_screen(screen) == Screen::TopPlayed {
        GamesMode::TopPlayed
    } else if screen.contains("favorites")
        || (fixture_list(screen).is_some() && !screen.contains("-systems"))
    {
        GamesMode::Favorites
    } else if screen.contains("recents") {
        GamesMode::Recents
    } else if fixture_screen(screen) == Screen::Deck {
        GamesMode::Deck
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
        screen.contains("context").then_some(6),
        screen.contains("markers"),
    );
    // `…-list-images`: Left and Right step the pane's images.
    if screen.ends_with("list-images") {
        let view = app.global::<GamesView>();
        view.set_detail_image_index(1);
        view.set_detail_image_count(4);
        view.set_detail_image_kind("screenshot".into());
    }
    if games_mode == GamesMode::Deck {
        app.global::<GamesView>().set_title("Couch co-op".into());
    }
    if screen.contains("search-results") {
        app.global::<GamesView>()
            .set_title("\u{201c}mario\u{201d} \u{b7} Super Nintendo".into());
    } else if screen.contains("search") || screen.ends_with("system-picker") {
        fixture_search(&app, screen);
    } else if screen.contains("rename") {
        fixture_rename(&app, screen.contains("caps"));
    } else if screen.contains("ra-login") {
        fixture_ra_login(&app, screen.contains("error"));
    } else if screen.contains("file-browser") {
        fixture_file_browser(&app, scene_w, scene_h, crt, screen);
    } else if screen.contains("music") {
        fixture_music(&app, scene_w, scene_h, crt, screen);
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
    // The context menu over the games screen, its rows taken from the
    // product rules so the fixture cannot drift from the app. "context" is
    // an arcade game's own rows with a reader connected; the other screens
    // are the pages those rows open, the Manage page being the Resume
    // tile's, which is the tallest and has the longest labels.
    if screen.contains("context") || screen.contains("deck-item") {
        use zaparoo_app::options_menu::{self as menu, GameInput, Page, Tile};
        use zaparoo_app::user_flags::{Flag, Flags};
        let flags = Flags {
            favorite: true,
            liked: true,
            ..Flags::default()
        };
        let game = menu::game(&GameInput {
            flags: Some(flags),
            can_add_to_deck: true,
            has_payload: true,
            has_nfc: true,
            can_discover: true,
            has_launchers: true,
            pinnable: true,
            hideable: true,
            has_system: true,
            tile: screen.ends_with("context-manage").then_some(Tile::Resume),
            ..GameInput::default()
        });
        // An item of an editable deck: the game's rows, with its place in
        // the deck leading the Manage page.
        let game = if screen.contains("deck-item") {
            game.with_deck_item(menu::DeckItem { can_be_cover: true })
        } else {
            game
        };
        let entry = |id: &str, key: &str, label: &str, opens: bool| MenuEntry {
            role: if opens {
                generated::MenuRole::Submenu
            } else {
                generated::MenuRole::default()
            },
            detail_key: slint::SharedString::default(),
            id: id.into(),
            label: label.into(),
            label_key: key.into(),
            enabled: true,
            reason_key: "".into(),
            detail: "".into(),
        };
        let page_rows = |page: Page| -> Vec<MenuEntry> {
            game.rows(page)
                .iter()
                .map(|row| entry(row.id, row.key, "", false))
                .collect()
        };
        let check = |index: usize, label: &str, checked: bool, enabled: bool| MenuEntry {
            role: if checked {
                generated::MenuRole::CheckOn
            } else {
                generated::MenuRole::CheckOff
            },
            enabled,
            ..entry(&format!("page_row:{index}"), "", label, false)
        };
        let (page, rows, index) = if screen.ends_with("deck-item-manage") {
            (
                generated::ContextPage::ManageGame,
                page_rows(Page::ManageGame),
                0,
            )
        } else if screen.ends_with("context-deck") {
            (
                generated::ContextPage::AddToDeck,
                vec![
                    check(0, "Couch co-op", true, true),
                    check(1, "Weekend", false, true),
                    check(2, "Arcade night", false, false),
                    entry("page_row:3", "new_deck", "", false),
                ],
                1,
            )
        } else if screen.ends_with("context-alt") {
            let names = [
                "Bubble Bobble (Japan)",
                "Bubble Bobble (Bootleg)",
                "Bubble Bobble (Rev A)",
            ];
            (
                generated::ContextPage::Alternates,
                names
                    .iter()
                    .enumerate()
                    .map(|(index, name)| entry(&format!("page_row:{index}"), "", name, false))
                    .collect(),
                0,
            )
        } else if screen.ends_with("context-manage") {
            (
                generated::ContextPage::ManageGame,
                page_rows(Page::ManageGame),
                0,
            )
        } else if screen.ends_with("context-write") {
            (
                generated::ContextPage::WriteToken,
                page_rows(Page::WriteToken),
                0,
            )
        } else {
            (
                generated::ContextPage::Root,
                game.root
                    .iter()
                    .map(|row| {
                        let opens = game.page_for(row.id).is_some()
                            || row.id == "discover"
                            || row.id == "add_to_deck";
                        entry(row.id, row.key, "", opens)
                    })
                    .collect(),
                1,
            )
        };
        let overlays = app.global::<generated::Overlays>();
        overlays.set_context_entries(slint::ModelRc::new(slint::VecModel::from(rows)));
        overlays.set_context_page(page);
        overlays.set_context_index(index);
        // The collections strip above the rows; "context-strip" has the
        // focus on its Like button.
        let buttons: Vec<generated::FlagButton> = Flag::ALL
            .iter()
            .map(|&flag| generated::FlagButton {
                flag: match flag {
                    Flag::Favorite => generated::UserFlag::Favorite,
                    Flag::Like => generated::UserFlag::Like,
                    Flag::Dislike => generated::UserFlag::Dislike,
                    Flag::PlayLater => generated::UserFlag::PlayLater,
                },
                on: game.strip.is_some_and(|flags| flags.has(flag)),
            })
            .collect();
        overlays.set_context_strip(slint::ModelRc::new(slint::VecModel::from(buttons)));
        if screen.ends_with("context-strip") {
            overlays.set_context_index(-1);
            overlays.set_context_strip_index(1);
        }
        overlays.set_context_open(true);
        // The list layout anchors the menu on a row, not a tile.
        if screen.contains("list") {
            if let Some(g) = list_geometry(
                &app,
                scene_w,
                scene_h,
                crt,
                0,
                games_mode != GamesMode::Recents,
            ) {
                let layout = app.global::<Layout>();
                let row_h = g.row_height as f32;
                overlays.set_context_anchor_x(
                    (g.card_x + g.list_x) as f32 + layout.get_card_padding_left(),
                );
                overlays.set_context_anchor_y(
                    (g.card_y + g.list_y) as f32
                        + layout.get_card_padding_top()
                        + 2.0 * (row_h + layout.get_row_spacing()),
                );
                overlays.set_context_anchor_w(
                    g.list_width as f32
                        - layout.get_card_padding_left()
                        - layout.get_card_padding_right(),
                );
                overlays.set_context_anchor_h(row_h);
                overlays.set_context_anchor_radius(layout.get_row_radius());
                overlays.set_context_anchor_zoomed(false);
            }
        }
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
        // "letters-few": a list with only a handful of sections.
        let labels: &[&str] = if screen.contains("letters-few") {
            &["A", "D", "M", "S"]
        } else {
            &[
                "#", "A", "B", "C", "D", "E", "F", "G", "H", "J", "K", "L", "M", "N", "P", "R",
                "S", "T", "V", "W", "Y", "Z",
            ]
        };
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
        app.global::<generated::Overlays>()
            .set_letter_index(i32::try_from(labels.len().min(5) - 1).unwrap_or(0));
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
        if screen.contains("top-played-systems") {
            SystemsMode::TopPlayed
        } else if screen.contains("favorite-systems")
            || (fixture_list(screen).is_some() && screen.contains("-systems"))
        {
            SystemsMode::Favorites
        } else {
            SystemsMode::Category
        },
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
            .enumerate()
            .map(|(i, value)| MenuEntry {
                // The current choice carries a check.
                role: if i == 6 {
                    generated::MenuRole::Chosen
                } else {
                    generated::MenuRole::default()
                },
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
    // "list-toggles" renders the music player's Menu list: rows that open
    // something, a value on the right, a muted row with its reason, and the
    // two settings as toggle switches (one on, one off).
    if screen.ends_with("list-toggles") || screen.ends_with("list-slider") {
        let ov = app.global::<generated::Overlays>();
        let row = |id: &str,
                   key: &str,
                   name: &str,
                   role: generated::MenuRole,
                   enabled: bool,
                   reason: &str| MenuEntry {
            role,
            detail_key: slint::SharedString::default(),
            id: id.into(),
            label: name.into(),
            label_key: key.into(),
            enabled,
            reason_key: reason.into(),
            detail: "".into(),
        };
        let option = generated::MenuRole::Option;
        ov.set_list_title("title:menu".into());
        ov.set_list_entries(slint::ModelRc::new(slint::VecModel::from(vec![
            row(
                "music_playlist",
                "music_playlist",
                "",
                option,
                false,
                "music:no_playlist",
            ),
            row("music_choose", "music_choose", "", option, true, ""),
            row("music_stop", "music_stop", "", option, true, ""),
            MenuEntry {
                detail: "80".into(),
                ..row(
                    "music_volume",
                    "music_volume",
                    "",
                    generated::MenuRole::Slider,
                    true,
                    "",
                )
            },
            row(
                "music_autoplay",
                "music_autoplay",
                "",
                generated::MenuRole::ToggleOn,
                true,
                "",
            ),
            row(
                "music_pause_games",
                "music_pause_games",
                "",
                generated::MenuRole::ToggleOff,
                true,
                "",
            ),
        ])));
        // list-slider focuses the volume slider, list-toggles a toggle.
        ov.set_list_index(if screen.ends_with("list-slider") {
            3
        } else {
            4
        });
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
    // "…-launcher-prefs" renders the launcher preference list over the
    // Library page, an entry held mid-move; "…-launcher-prefs-add" its Add
    // page.
    if screen.contains("launcher-prefs") {
        use generated::{LauncherPrefsPage, LauncherPrefsRow, LauncherPrefsRowKind};
        let ov = app.global::<generated::Overlays>();
        let row =
            |kind: LauncherPrefsRowKind, id: &str, detail: &str, enabled: bool, held: bool| {
                LauncherPrefsRow {
                    kind,
                    id: id.into(),
                    detail: detail.into(),
                    enabled,
                    held,
                }
            };
        let add = screen.contains("launcher-prefs-add");
        let rows = if add {
            vec![
                row(LauncherPrefsRowKind::Group, "DB9", "", false, false),
                row(LauncherPrefsRowKind::Group, "DualRAM", "", true, false),
                row(LauncherPrefsRowKind::Group, "LLAPI", "", true, false),
                row(
                    LauncherPrefsRowKind::Launcher,
                    "NESdb9",
                    "Nintendo Entertainment System",
                    false,
                    false,
                ),
                row(
                    LauncherPrefsRowKind::Launcher,
                    "RAMegaDrive",
                    "Sega Genesis",
                    true,
                    false,
                ),
                row(
                    LauncherPrefsRowKind::Launcher,
                    "SNES",
                    "Super Nintendo",
                    true,
                    false,
                ),
                row(
                    LauncherPrefsRowKind::Launcher,
                    "SNES_dualsdram",
                    "Super Nintendo",
                    true,
                    false,
                ),
                row(
                    LauncherPrefsRowKind::Launcher,
                    "PSX",
                    "PlayStation",
                    true,
                    false,
                ),
            ]
        } else {
            vec![
                row(
                    LauncherPrefsRowKind::Group,
                    "RetroAchievements",
                    "",
                    true,
                    false,
                ),
                row(LauncherPrefsRowKind::Group, "Sinden", "", true, true),
                row(
                    LauncherPrefsRowKind::Launcher,
                    "SNES_dualsdram",
                    "Super Nintendo",
                    true,
                    false,
                ),
            ]
        };
        ov.set_launcher_prefs_page(if add {
            LauncherPrefsPage::Add
        } else {
            LauncherPrefsPage::List
        });
        ov.set_launcher_prefs_rows(slint::ModelRc::new(slint::VecModel::from(rows)));
        ov.set_launcher_prefs_index(if add { 2 } else { 1 });
        ov.set_launcher_prefs_moving(!add);
        ov.set_launcher_prefs_open(true);
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
    // Core's UI events: a deck's picker, with the focused game's cover
    // beside the list, and a confirmation in the decision dialog.
    if screen.ends_with("core-picker") {
        let view = app.global::<generated::CoreUiView>();
        let rows = [
            ("The Legend of Zelda", "NES"),
            ("Super Mario World", "SNES"),
            ("F-Zero", "SNES"),
            ("Gunstar Heroes", "Genesis"),
            ("GoldenEye 007", "Nintendo 64"),
            ("Pokemon Red", "Game Boy"),
            ("Pokemon Crystal", "Game Boy Color"),
            ("Shantae", "Game Boy Color"),
            ("EarthBound", ""),
            ("Party night", ""),
            ("Random Genesis game", ""),
        ]
        .into_iter()
        .map(|(label, detail)| generated::CorePickerRow {
            label: label.into(),
            detail: detail.into(),
        })
        .collect::<Vec<_>>();
        view.set_title("Couch co-op".into());
        view.set_rows(slint::ModelRc::new(slint::VecModel::from(rows)));
        view.set_index(1);
        view.set_dismissible(true);
        view.set_has_art(true);
        let cover = slint::Image::load_from_path(std::path::Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/assets/systems-color/SNES.png"
        )))
        .expect("fixture cover");
        view.set_cover(cover);
        view.set_has_cover(true);
        view.set_kind(generated::CoreUiKind::Picker);
    }
    if screen.ends_with("core-confirm") {
        let overlays = app.global::<generated::Overlays>();
        overlays.set_dialog_kind(DialogKind::CoreConfirm);
        overlays.set_dialog_detail("Change game?".into());
        overlays.set_dialog_arg("Super Mario World is running. Launch F-Zero instead?".into());
        overlays.set_dialog_buttons(slint::ModelRc::new(slint::VecModel::from(vec![
            DialogButton::Ok,
            DialogButton::Cancel,
        ])));
        overlays.set_dialog_focus(0);
        overlays.set_dialog_open(true);
    }
    if screen.starts_with("calibration") {
        let overlays = app.global::<generated::Overlays>();
        overlays.set_crt_h_offset(4);
        overlays.set_crt_v_offset(-2);
        // `calibration-width`: the mode the options button switches to.
        overlays.set_crt_size_mode(screen == "calibration-width");
        overlays.set_crt_calibration_open(true);
    }
    // "update-*" renders the Update screen from a seeded `UpdateView`, with
    // no engine behind it.
    if screen.contains("update") {
        fixture_update(&app, screen);
    }
    app.global::<Shell>()
        .set_active_screen(fixture_screen(screen));
    if let Some(list) = fixture_list(screen) {
        app.global::<Shell>().set_active_list(list);
    }
    // "*-bar": the Menu chip row with focus on its first chip.
    if screen.ends_with("-bar") {
        let bar = app.global::<generated::PageBar>();
        bar.set_index(0);
        bar.set_focused(true);
    }
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
    if background && rotation == RenderingRotation::NoRotation {
        apply_background(&app, &window, width, height);
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

    if crt && std::env::var_os("ZAPAROO_SNAPSHOT_TEXT_PROBE").is_some() {
        for line in off_grid_text(&app) {
            println!("off-grid text: {line}");
        }
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

/// Prepare a background through the real pipeline for this frame: a first
/// draw lets the screens report their main card and the help bar its top,
/// as they do to `background.rs`, then the composed image is set.
fn apply_background(app: &App, window: &Rc<MinimalSoftwareWindow>, width: u32, height: u32) {
    use zaparoo_app::background as rules;
    let card = Rc::new(std::cell::Cell::new(None));
    let bar = Rc::new(std::cell::Cell::new(None));
    let backdrop = app.global::<Backdrop>();
    {
        let card = card.clone();
        backdrop.on_card_moved(move |shown, x, y, w, h, radius| {
            card.set((shown && w > 0.0).then(|| {
                let rect = rules::Rect {
                    x: x.round() as u32,
                    y: y.round() as u32,
                    width: w.round() as u32,
                    height: h.round() as u32,
                };
                (rect, radius.round() as u32)
            }));
        });
        let bar = bar.clone();
        backdrop.on_bar_moved(move |top| bar.set(Some(top)));
    }
    let mut scratch = vec![PremultipliedRgbaColor::default(); width as usize * height as usize];
    let _ = window.draw_if_needed(|r| {
        r.render(scratch.as_mut_slice(), width as usize);
    });
    let source = std::env::var("ZAPAROO_SNAPSHOT_BACKGROUND")
        .ok()
        .and_then(|path| image::open(path).ok())
        .map_or_else(synthetic_background, image::DynamicImage::into_rgb8);
    let fitted = rules::fit(
        source.as_raw(),
        source.width(),
        source.height(),
        width,
        height,
    );
    let base = rules::prepare_base(&fitted, rules::DEFAULT_BLUR);
    let theme = app.global::<Theme>();
    let rgb = |c: slint::Color| [c.red(), c.green(), c.blue()];
    let (ground, text) = (rgb(theme.get_bg_deep()), rgb(theme.get_text_primary()));
    let regions = rules::Regions {
        help_bar: bar
            .get()
            .map(|top| top.round().max(0.0) as u32)
            .filter(|top| *top < height)
            .map(|top| rules::Rect {
                x: 0,
                y: top,
                width,
                height: height - top,
            }),
        card: card.get().map(|(rect, _)| rect),
        card_radius: card.get().map_or(0, |(_, radius)| radius),
    };
    let dim = rules::dim_for(rules::readable_dim(&base.samples, text, ground), 0);
    let composed = rules::compose(&base, ground, dim, &regions, rules::Format::Rgb888);
    let buffer =
        slint::SharedPixelBuffer::<slint::Rgb8Pixel>::clone_from_slice(&composed, width, height);
    backdrop.set_image(slint::Image::from_rgb8(buffer));
    backdrop.set_glass(regions.card.is_some());
    backdrop.set_on(true);
}

/// A bright, busy stand-in for a user's wallpaper: a sky, a sun, hills and
/// a band of fine stripes, the cases readability has to hold over.
fn synthetic_background() -> image::RgbImage {
    let (w, h) = (640u32, 360u32);
    image::RgbImage::from_fn(w, h, |x, y| {
        let (fx, fy) = (f64::from(x) / f64::from(w), f64::from(y) / f64::from(h));
        let sun = ((fx - 0.72).powi(2) + (fy - 0.25).powi(2) * 3.0).sqrt() < 0.09;
        let hill = fy > 0.62 + 0.06 * (fx * 9.0).sin() + 0.03 * (fx * 23.0).sin();
        let stripes = fy > 0.05 && fy < 0.18 && fx < 0.4 && (x / 3) % 2 == 0;
        if sun || stripes {
            image::Rgb([255, 250, 230])
        } else if hill {
            image::Rgb([40, (110.0 - 40.0 * fy) as u8, 55])
        } else {
            image::Rgb([
                (250.0 - 110.0 * fy) as u8,
                (190.0 - 40.0 * fy) as u8,
                (110.0 + 110.0 * fy) as u8,
            ])
        }
    })
}

/// Every visible CRT label whose glyphs would start off a whole pixel.
///
/// Slint's software renderer places glyphs at quarter-pixel positions and
/// bakes the remainder into their coverage, so the 6x8 bitmap face only
/// stays sharp when each glyph's pen lands on a whole pixel. Its advances
/// are 6 or 12 px, always even, so the pen of every glyph shares the
/// fraction of the line's start, and a line's start depends only on the
/// item's absolute x and its alignment: x, x + width / 2, or x + width.
fn off_grid_text(app: &App) -> Vec<String> {
    use i_slint_core::item_tree::ItemRc;
    use i_slint_core::items::{ComplexText, SimpleText, TextHorizontalAlignment};
    use i_slint_core::window::WindowInner;

    let root = ItemRc::new_root(WindowInner::from_pub(app.window()).component());
    let mut found = Vec::new();
    root.visit_descendants(|item| {
        let text = item
            .downcast::<ComplexText>()
            .map(|t| {
                let t = t.as_pin_ref();
                (t.text(), t.horizontal_alignment())
            })
            .or_else(|| {
                item.downcast::<SimpleText>().map(|t| {
                    let t = t.as_pin_ref();
                    (t.text(), t.horizontal_alignment())
                })
            });
        if let Some((text, align)) = text {
            if !text.trim().is_empty() && item.is_visible() {
                let geometry = item.geometry();
                let x = item.map_to_window(geometry.origin).x;
                let start = match align {
                    TextHorizontalAlignment::Center => x + geometry.width() / 2.0,
                    TextHorizontalAlignment::Right | TextHorizontalAlignment::End => {
                        x + geometry.width()
                    }
                    _ => x,
                };
                // The renderer's own quantization: quarter-pixel bins.
                if (start * 4.0).round() as i32 % 4 != 0 {
                    found.push(format!(
                        "{text:?} starts at x={start:.2} (x={x:.2} w={:.2}){}",
                        geometry.width(),
                        element_path(item)
                    ));
                }
            }
        }
        std::ops::ControlFlow::<()>::Continue(())
    });
    found
}

/// The element and its nearest named ancestors, when the build carries
/// Slint's debug info (`SLINT_EMIT_DEBUG_INFO=1`); empty otherwise.
fn element_path(item: &i_slint_core::item_tree::ItemRc) -> String {
    use i_slint_core::item_tree::ParentItemTraversalMode;
    let mut names = Vec::new();
    let mut current = Some(item.clone());
    while let Some(node) = current {
        if let Some(infos) = node.element_type_names_and_ids(0) {
            for (type_name, id) in infos {
                let name = if id.is_empty() {
                    type_name.to_string()
                } else {
                    format!("{type_name}#{id}")
                };
                if names.last() != Some(&name) {
                    names.push(name);
                }
            }
        }
        if names.len() >= 6 {
            break;
        }
        current = node.parent_item(ParentItemTraversalMode::StopAtPopups);
    }
    if names.is_empty() {
        String::new()
    } else {
        format!(" in {}", names.join(" < "))
    }
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
        view.set_scope(SearchScope::Folder);
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

/// The `RetroAchievements` login on its password, the character just typed
/// still showing; "…-error" after `RetroAchievements` turned it down.
fn fixture_ra_login(app: &App, error: bool) {
    let view = app.global::<generated::RaLoginView>();
    let mut keys = keyboard::Keyboard::masked(
        zaparoo_app::keyboard::Layers::Full,
        zaparoo_app::retro_achievements::FIELD_MAX_CHARS,
    );
    for c in "hunter2".chars() {
        keys.insert(c);
    }
    if error {
        if let Some(ticket) = keys.reveal_ticket() {
            keys.expire_reveal(ticket);
        }
    }
    keys.focus(zaparoo_app::keyboard::Key::Char('2'));
    let (before, at, after) = keys.display();
    view.set_before(before);
    view.set_at(at);
    view.set_after(after);
    view.set_keys(slint::ModelRc::new(slint::VecModel::from(keys.cells())));
    view.set_key_rows(i32::try_from(keys.row_count()).unwrap_or(5));
    view.set_key_index(i32::try_from(keys.index()).unwrap_or(0));
    view.set_key_kind(KeyKind::Char);
    view.set_step(generated::RaLoginStep::Password);
    view.set_can_submit(true);
    view.set_status(if error {
        generated::RaLoginStatus::WrongCredentials
    } else {
        generated::RaLoginStatus::Idle
    });
}

/// The rename screen: a tile's name being edited, the cursor inside it.
fn fixture_rename(app: &App, caps: bool) {
    let view = app.global::<generated::RenameView>();
    let mut keys = keyboard::Keyboard::new(
        zaparoo_app::keyboard::Layers::Full,
        zaparoo_app::text_entry::TILE_NAME_MAX_CHARS,
    );
    keys.set_text("Role-playing games");
    keys.move_caret(false);
    keys.move_caret(false);
    if caps {
        keys.lock_shift();
        keys.focus(zaparoo_app::keyboard::Key::Shift);
    } else {
        keys.focus(zaparoo_app::keyboard::Key::Char('e'));
    }
    let (before, at, after) = keys.display();
    view.set_before(before);
    view.set_at(at);
    view.set_after(after);
    view.set_keys(slint::ModelRc::new(slint::VecModel::from(keys.cells())));
    view.set_key_rows(i32::try_from(keys.row_count()).unwrap_or(5));
    view.set_key_index(i32::try_from(keys.index()).unwrap_or(0));
    view.set_key_kind(KeyKind::Char);
    view.set_shift_active(caps);
    view.set_shift_locked(caps);
}

/// The file browser: a folder of images as previews, the same folder as a
/// list, or Places.
#[allow(
    clippy::too_many_lines,
    reason = "one fixture projects the same folder into grid and list layouts"
)]
fn fixture_file_browser(app: &App, scene_w: f64, scene_h: f64, crt: bool, screen: &str) {
    use zaparoo_app::layouts::{self, Body, ThemeId, View};
    let view = app.global::<FileBrowserView>();
    let places = screen.contains("places");
    // The Custom folder… picker: folders only, under the row that picks
    // the folder on screen.
    let pick = screen.contains("folders");
    let list = screen.contains("list") || pick;
    let pick_label = app
        .global::<generated::Labels>()
        .invoke_menu("file_add_folder".into(), "".into())
        .to_string();
    let names: Vec<(&str, &str)> = if pick {
        vec![
            (pick_label.as_str(), ""),
            ("_Arcade", ""),
            ("_Computer", ""),
            ("_RA_Cores", ""),
            ("_Utility", ""),
        ]
    } else if places {
        vec![
            ("Custom folder", "/media/fat/zaparoo/custom"),
            ("SD card", "/media/fat"),
            ("USB 0", "/media/usb0"),
            ("Network", "/media/network"),
            ("Root", "/"),
        ]
    } else {
        vec![
            ("Consoles", ""),
            ("Handhelds", ""),
            ("arcade.png", ""),
            ("favorites.svg", ""),
            ("rpg-night.jpg", ""),
            ("search.png", ""),
            ("settings.webp", ""),
        ]
    };
    let folders = if places || pick { names.len() } else { 2 };
    let cell = |i: usize, (name, path): (&str, &str)| GridCell {
        name: if list && !places && i < folders && !(pick && i == 0) {
            format!("{name}/").into()
        } else {
            name.into()
        },
        glyph_key: if i < folders {
            "icons/Folder"
        } else {
            "icons/File"
        }
        .into(),
        tags: if list { path.into() } else { "".into() },
        ..Default::default()
    };
    let cells: Vec<GridCell> = names.iter().enumerate().map(|(i, n)| cell(i, *n)).collect();
    view.set_mode(if list {
        FileBrowserMode::Plain
    } else {
        FileBrowserMode::Images
    });
    view.set_places(places);
    view.set_title(
        if places {
            ""
        } else if pick {
            "fat"
        } else {
            "hub"
        }
        .into(),
    );
    view.set_focus_ready(true);
    let selected = if places || pick { 0 } else { 2 };
    view.set_focused_opens(selected < folders && !pick);
    view.set_focused_picks(selected >= folders || pick);
    view.set_has_menu(!pick);
    let inputs = sizing::Scene::of(app, scene_w, scene_h, crt).inputs();
    if list {
        if let Some(geometry) = list_geometry(app, scene_w, scene_h, crt, 10, false) {
            view.set_list_x(geometry.card_x as f32);
            view.set_list_y(geometry.card_y as f32);
            view.set_list_width(geometry.card_width as f32);
            view.set_list_height(geometry.card_height as f32);
            view.set_list_row_height(geometry.row_height as f32);
        }
        view.set_list_rows(slint::ModelRc::new(slint::VecModel::from(cells)));
        view.set_list_view_top(0);
        view.set_list_scroll_top(0);
        view.set_list_sel(selected as i32);
        view.set_count(names.len() as i32);
        view.set_current_index(selected as i32);
        return;
    }
    let derived = zaparoo_app::sizing::derive(&inputs);
    let profile = layouts::profile(ThemeId::current(&inputs), View::GamesGrid, &inputs);
    let Body::Grid { grid, .. } = profile.body else {
        return;
    };
    let t240 = derived.tier == zaparoo_app::sizing::Tier::T240;
    let grid_y = derived.header_bottom + profile.status.top_margin + profile.status.strip_height;
    let label_height = inputs.pct_h(7.0);
    let bottom = if t240 {
        derived.help_bar_height + label_height
    } else {
        zaparoo_app::sizing::less_hidden_bar(&inputs, &derived, inputs.pct_h(15.0))
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
    let page = (columns * rows).max(1) as usize;
    let label = names[selected].0;
    let cells: Vec<GridCell> = cells.into_iter().take(page).collect();
    view.set_cells(slint::ModelRc::new(slint::VecModel::from(cells)));
    view.set_columns(columns);
    view.set_rows(rows);
    view.set_cell_width(fit.cell_width as f32);
    view.set_cell_height(fit.cell_height as f32);
    view.set_block_offset_x(fit.block_offset_x as f32);
    view.set_block_offset_y(fit.block_offset_y as f32);
    view.set_grid_y(grid_y as f32);
    view.set_grid_height(grid_height as f32);
    view.set_label_y((grid_y + grid_height) as f32);
    view.set_label_height(label_height as f32);
    view.set_selected_local(selected as i32);
    view.set_label_name(label.into());
    view.set_count(names.len() as i32);
    view.set_current_index(selected as i32);
}

/// "liked", "play-later" and "disliked" (and their "-systems" screens)
/// render the Favorites screens showing that list.
fn fixture_list(screen: &str) -> Option<generated::UserFlag> {
    if screen.contains("liked") && !screen.contains("disliked") {
        Some(generated::UserFlag::Like)
    } else if screen.contains("play-later") {
        Some(generated::UserFlag::PlayLater)
    } else if screen.contains("disliked") {
        Some(generated::UserFlag::Dislike)
    } else {
        None
    }
}

/// The music player: `music` plays a tagged track with Play/Pause focused,
/// `music-paused` has it paused with Repeat focused, `music-idle` has
/// nothing on the slot, `music-playlist` is the open playlist's page, and
/// `music-library` lists a folder of the Audio index.
#[allow(
    clippy::too_many_lines,
    reason = "one fixture fills the player and both list pages"
)]
fn fixture_music(app: &App, scene_w: f64, scene_h: f64, crt: bool, screen: &str) {
    let view = app.global::<MusicView>();
    let inputs = sizing::Scene::of(app, scene_w, scene_h, crt).inputs();
    let library = screen.contains("library");
    let playlist = screen.contains("playlist");
    let idle = screen.contains("idle");
    let paused = screen.contains("paused");
    let layout = music_layout::layout_for(&inputs, false, !playlist);
    music_layout::push_layout(&view, &layout);
    // The page's Menu chips, as the driver publishes them.
    let chip = |id: &str, key: &str, name: &str| MenuEntry {
        role: generated::MenuRole::Option,
        detail_key: "".into(),
        id: id.into(),
        label: name.into(),
        label_key: key.into(),
        enabled: true,
        reason_key: "".into(),
        detail: "".into(),
    };
    let chips = if library {
        vec![
            chip("music_play_all", "music_play_all", ""),
            chip("music_shuffle_all", "music_shuffle_all", ""),
        ]
    } else if playlist {
        Vec::new()
    } else if idle {
        vec![
            chip("music_choose", "music_choose", ""),
            chip("music_volume", "music_volume", "80"),
            chip("more", "more", ""),
        ]
    } else {
        vec![
            chip("music_playlist", "music_playlist", ""),
            chip("music_choose", "music_choose", ""),
            chip("music_stop", "music_stop", ""),
            chip("music_volume", "music_volume", "80"),
            chip("more", "more", ""),
        ]
    };
    app.global::<generated::PageBar>()
        .set_music_chips(slint::ModelRc::new(slint::VecModel::from(chips)));
    view.set_page(if library {
        MusicPage::Library
    } else if playlist {
        MusicPage::Playlist
    } else {
        MusicPage::Player
    });
    view.set_focus_ready(true);
    view.set_has_track(!idle);
    view.set_title("Tunnel Vision".into());
    view.set_artist("Neon Harbor".into());
    view.set_album("Night Drive".into());
    view.set_up_next(if idle { "" } else { "Rearview" }.into());
    view.set_playlist_name(if idle { "" } else { "Night Drive" }.into());
    view.set_track_number(if idle { 0 } else { 3 });
    view.set_track_total(if idle { 0 } else { 5 });
    view.set_playback(if idle {
        PlaybackState::Stopped
    } else if paused {
        PlaybackState::Paused
    } else {
        PlaybackState::Playing
    });
    view.set_progress_milli(if idle { 0 } else { 470 });
    // Nothing playing has no time to show, as the driver leaves it.
    view.set_position_text(if idle { "" } else { "1:42" }.into());
    view.set_duration_text(if idle { "" } else { "3:37" }.into());
    view.set_shuffle(false);
    view.set_repeat(RepeatMode::All);
    let controls = [
        MusicControl::Shuffle,
        MusicControl::Previous,
        MusicControl::PlayPause,
        MusicControl::Next,
        MusicControl::Repeat,
    ];
    let buttons: Vec<MusicButton> = controls
        .into_iter()
        .map(|control| MusicButton {
            control,
            unavailable: if idle {
                MusicUnavailable::NothingPlaying
            } else {
                MusicUnavailable::None
            },
        })
        .collect();
    view.set_buttons(slint::ModelRc::new(slint::VecModel::from(buttons)));
    view.set_control_index(if paused { 4 } else { 2 });
    let rows: Vec<GridCell> = if library {
        ["Night Drive", "Harbor Lights (single)", "Live at the Pier"]
            .iter()
            .enumerate()
            .map(|(i, name)| GridCell {
                name: (*name).into(),
                opens: i != 1,
                ..Default::default()
            })
            .collect()
    } else if playlist {
        [
            "Departure",
            "Coastline",
            "Tunnel Vision",
            "Rearview",
            "Arrival",
        ]
        .iter()
        .enumerate()
        .map(|(i, name)| GridCell {
            name: (*name).into(),
            now_playing: i == 2,
            ..Default::default()
        })
        .collect()
    } else {
        Vec::new()
    };
    let selected = if library { 1 } else { 2 };
    view.set_count(rows.len() as i32);
    view.set_current_index(selected);
    view.set_list_rows(slint::ModelRc::new(slint::VecModel::from(rows)));
    view.set_list_view_top(0);
    view.set_list_scroll_top(0);
    view.set_list_sel(selected);
    view.set_list_total_pages(1);
    view.set_library_top(false);
    view.set_library_title("Neon Harbor".into());
    view.set_focused_opens(false);
}

fn fixture_screen(screen: &str) -> Screen {
    if screen.starts_with("route-")
        || screen.ends_with("hub-deck")
        || screen.ends_with("core-picker")
        || screen.ends_with("core-confirm")
    {
        Screen::Hub
    } else if screen.ends_with("decks") || screen.ends_with("decks-list") {
        Screen::Decks
    } else if (screen.ends_with("deck") && !screen.contains("context"))
        || screen.ends_with("deck-list")
        || screen.contains("deck-item")
    {
        Screen::Deck
    } else if screen.contains("top-played-systems") {
        Screen::TopPlayedSystems
    } else if screen.contains("top-played") {
        Screen::TopPlayed
    } else if screen.contains("music") {
        Screen::Music
    } else if screen.contains("rename") {
        Screen::Rename
    } else if screen.contains("ra-login") {
        Screen::RaLogin
    } else if screen.contains("file-browser") {
        Screen::FileBrowser
    } else if screen.contains("search-results") {
        Screen::SearchResults
    } else if screen.contains("search") || screen.ends_with("system-picker") {
        Screen::Search
    } else if screen.contains("update") {
        Screen::Update
    } else if screen.contains("context")
        || screen == "letters"
        || screen == "letters-few"
        || (screen.contains("list") && !screen.contains("systems"))
        || screen.contains("games")
        || screen.contains("filter")
        || screen.contains("fast-scroll")
        || screen.contains("game-info")
    {
        Screen::Games
    } else if screen.contains("favorite-systems")
        || (fixture_list(screen).is_some() && screen.contains("-systems"))
    {
        Screen::FavoriteSystems
    } else if screen.contains("favorites") || fixture_list(screen).is_some() {
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
        "saver"
            | "dialog"
            | "alert"
            | "config-alert"
            | "notice"
            | "calibration"
            | "calibration-width"
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
    let manual = screen.contains("manual");
    view.set_loading(!manual && screen.ends_with("loading"));
    view.set_failed(!manual && screen.ends_with("error"));
    if view.get_loading() || view.get_failed() {
        return;
    }
    let short = screen.ends_with("short");
    // The table `game_info_data::info_rows` lays out: the facts under
    // Details, the leftover tags, then Technical details. `stop` is the
    // focus index after the art (0), the manual (1) and the description (2).
    let header = |key: &str| generated::InfoRow {
        kind: RowKind::Header,
        key: key.into(),
        value: "".into(),
        stop: -1,
    };
    let row = |key: &str, value: &str, stop: i32| generated::InfoRow {
        kind: RowKind::Field,
        key: key.into(),
        value: value.into(),
        stop,
    };
    let rows: Vec<_> = if short {
        vec![
            header("details"),
            row("system", "Super Nintendo Entertainment System", -1),
            row("year", "1995", 3),
        ]
    } else {
        vec![
            header("details"),
            row("system", "Super Nintendo Entertainment System", -1),
            row("year", "1995", 3),
            row("players", "1 player", 4),
            row("developer", "Square", 5),
            row("publisher", "Square", 6),
            row("genre", "RPG, JRPG", 7),
            row("region", "United States", 8),
            row("tags", "custom:anniversary, user:hidden", 9),
            header("technical"),
            row("dump", "Verified", 10),
            row("path", "/media/fat/games/SNES/Chrono Trigger (USA).sfc", -1),
        ]
    };
    view.set_info_rows(slint::ModelRc::new(slint::VecModel::from(rows)));
    view.set_image_stop(if short { -1 } else { 0 });
    view.set_manual_stop(1);
    view.set_description_stop(if short { -1 } else { 2 });
    // `…-focus-row` puts focus on Genre; `…-values` lists its values.
    view.set_focus(
        if screen.contains("focus-row") || screen.ends_with("values") {
            7
        } else {
            i32::from(short)
        },
    );
    if screen.ends_with("values") {
        view.set_values_kind("genre".into());
        view.set_values(slint::ModelRc::new(slint::VecModel::from(vec![
            slint::SharedString::from("RPG"),
            slint::SharedString::from("JRPG"),
        ])));
        view.set_values_index(1);
        view.set_values_open(true);
    }
    if !short {
        view.set_media_missing(true);
        view.set_modal_description("A journey through time, from prehistoric lands to a distant future. Meet companions, explore unfamiliar places, and discover how each era connects to the next. Choices made along the way shape the adventure.\n\nThis long fixture checks that descriptions stay readable and can be scrolled rather than silently truncated.".into());
        view.set_image_count(2);
        view.set_image_kind("boxart".into());
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
    view.set_has_manual(true);
    // `game-info-manual` is the manual's whole page, `…-manual-width` fit
    // width; `…-manual-loading` and `…-manual-error` the states before a
    // page.
    if screen.contains("manual") {
        fixture_manual(&view, screen);
    }
    // `game-info-viewer`: the art full screen, named and counted.
    if screen.ends_with("viewer") {
        let art = slint::Image::load_from_path(std::path::Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/assets/systems/SNES.png"
        )))
        .expect("fixture logo");
        view.set_viewer_image(art);
        view.set_viewer_has_image(true);
        view.set_viewer_open(true);
    }
}

fn fixture_manual(view: &generated::GameInfoView, screen: &str) {
    use generated::{ManualFit, ManualState};
    if screen.ends_with("loading") {
        view.set_manual_state(ManualState::Loading);
        return;
    }
    if screen.ends_with("error") {
        view.set_manual_state(ManualState::TooLarge);
        return;
    }
    let pdf = hayro::hayro_syntax::Pdf::new(fixture_manual_pdf()).expect("fixture PDF");
    let page = &pdf.pages()[0];
    let (page_w, _) = page.render_dimensions();
    let scale = 1280.0 / page_w;
    let pixmap = hayro::render(
        page,
        &hayro::RenderCache::new(),
        &hayro::hayro_interpret::InterpreterSettings::default(),
        &hayro::RenderSettings::default(),
        &hayro::PixmapSettings {
            x_scale: scale,
            y_scale: scale,
            bg_color: hayro::vello_cpu::color::palette::css::WHITE,
        },
    );
    let buffer = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(
        pixmap.data_as_u8_slice(),
        u32::from(pixmap.width()),
        u32::from(pixmap.height()),
    );
    view.set_manual_state(ManualState::Ready);
    view.set_manual_image(slint::Image::from_rgba8(buffer));
    view.set_manual_has_image(true);
    view.set_manual_pages(12);
    view.set_manual_page(2);
    // A manual opens on the whole page; `…-manual-width` is fit width,
    // scrolled a little.
    if screen.ends_with("width") {
        view.set_manual_fit(ManualFit::Width);
        view.set_manual_scroll(0.1);
    }
}

/// One US Letter page of manual-like text, built as the mock Core builds
/// its manuals.
fn fixture_manual_pdf() -> Vec<u8> {
    use std::fmt::Write as _;
    let mut text = String::from(
        "BT /F1 28 Tf 72 700 Td (Chrono Trigger) Tj ET\nBT /F1 16 Tf 72 664 Td (Starting the game) Tj ET\n",
    );
    for line in 0..32 {
        let _ = writeln!(
            text,
            "BT /F1 12 Tf 72 {} Td (Press START on the title screen, then choose New Game or Continue. {}) Tj ET",
            630 - line * 18,
            line + 1
        );
    }
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R >>".to_string(),
        "<< /Type /Pages /Kids [4 0 R] /Count 1 >>".into(),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into(),
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Resources << /Font << /F1 3 0 R >> >> /Contents 5 0 R >>".into(),
        format!("<< /Length {} >>\nstream\n{text}endstream", text.len()),
    ];
    let mut out = b"%PDF-1.4\n".to_vec();
    let mut offsets = Vec::new();
    for (i, object) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n{object}\nendobj\n", i + 1).as_bytes());
    }
    let xref = out.len();
    let mut tail = format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1);
    for offset in offsets {
        let _ = writeln!(tail, "{offset:010} 00000 n ");
    }
    let _ = write!(
        tail,
        "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
        objects.len() + 1
    );
    out.extend_from_slice(tail.as_bytes());
    out
}

/// The Hub fixture's tiles: Resume, the five categories, the built-in
/// actions and a pinned folder.
fn fixture_hub_items() -> Vec<zaparoo_app::hub::LayoutItem> {
    use zaparoo_app::hub::LayoutItem;
    let item = |kind: &str, id: &str| LayoutItem {
        kind: kind.into(),
        id: id.into(),
        ..LayoutItem::default()
    };
    vec![
        item("action", "resume"),
        item("category", "Arcade"),
        item("category", "Console"),
        item("category", "Computer"),
        item("category", "Handheld"),
        item("category", "Other"),
        item("action", "favorites"),
        item("action", "liked"),
        item("action", "playlater"),
        item("action", "decks"),
        item("collection", "0k3v9x2rq7bm"),
        item("action", "recents"),
        item("action", "topplayed"),
        item("action", "update"),
        item("action", "settings"),
        LayoutItem {
            kind: "folder".into(),
            path: "/media/fat/games/SNES/Homebrew".into(),
            system: "SNES".into(),
            ..LayoutItem::default()
        },
        // A Settings tile pinned to the Hub.
        item("setting", "pageMusic"),
    ]
}

fn fixture_hub(app: &App, scene_w: f64, scene_h: f64, crt: bool, selected: usize) {
    use zaparoo_app::hub::{self, Live, Resolver};
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
        fn deck(&self, _id: &str) -> hub::DeckLookup {
            hub::DeckLookup::Found {
                name: "Couch co-op".into(),
                cover_key: String::new(),
            }
        }
    }
    let items = fixture_hub_items();
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
        pick_folders: false,
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
    menu_row: bool,
) -> (f32, usize) {
    list_geometry(app, scene_w, scene_h, crt, target_rows, menu_row)
        .map_or((0.0, 1), |g| (g.row_height as f32, g.visible_rows.max(1)))
}

/// The list card's geometry as the driver would solve it, or `None` on a
/// profile whose body is not a list.
fn list_geometry(
    app: &App,
    scene_w: f64,
    scene_h: f64,
    crt: bool,
    target_rows: usize,
    menu_row: bool,
) -> Option<zaparoo_app::media_list::ListGeometry> {
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
        return None;
    };
    Some(zaparoo_app::media_list::list_geometry(
        &list,
        &zaparoo_app::media_list::ListFrame {
            screen_width: inputs.screen_width as i32,
            screen_height: inputs.screen_height as i32,
            header_bottom: derived.header_bottom,
            status_top_margin: profile.status.top_margin,
            strip_height: profile.status.strip_height
                + if menu_row { derived.menu_row_block } else { 0 },
            help_bar_height: derived.help_bar_height,
            tier_240: derived.tier == zaparoo_app::sizing::Tier::T240,
            safe_bottom_gap: derived.help_bar_band,
            target_rows,
            min_row_height: inputs.pct_h(3.0),
            default_row_height: inputs.pct_h(6.0),
        },
    ))
}

/// The Controllers page's own view: its layout from the rule the driver
/// uses, two cards (or none), and the search under way.
fn fixture_controllers(app: &App, inputs: &zaparoo_app::sizing::Inputs, none_connected: bool) {
    use generated::{ControllerSlot, ControllersView, PairSearch};
    let l = zaparoo_app::controllers::page_layout(inputs);
    let view = app.global::<ControllersView>();
    view.set_compact(l.compact);
    view.set_slots_y(l.slots_y as f32);
    view.set_slot_columns(l.slot_columns);
    view.set_slot_width(l.slot_width as f32);
    view.set_slot_height(l.slot_height as f32);
    view.set_slot_gap(l.slot_gap as f32);
    view.set_status_y(l.status_y as f32);
    view.set_status_height(l.status_height as f32);
    view.set_list_y(l.list_y as f32);
    view.set_row_height(l.row_height as f32);
    view.set_visible_rows(l.visible_rows);
    view.set_paired_y(l.paired_y as f32);
    view.set_top(0);
    view.set_can_pair(true);
    view.set_search(PairSearch::Searching);
    let mut slots = vec![ControllerSlot::default(); zaparoo_app::controllers::SLOTS];
    if !none_connected {
        slots[0] = ControllerSlot {
            filled: true,
            name: "DualSense".into(),
            reading: DeviceValue::ChargingPercent,
            percent: 80,
            bar: 80,
            charging: true,
            ..Default::default()
        };
        slots[1] = ControllerSlot {
            filled: true,
            name: "8BitDo Pro 2".into(),
            reading: DeviceValue::Percent,
            percent: 10,
            bar: 10,
            low: true,
            ..Default::default()
        };
    }
    view.set_connected(if none_connected { 0 } else { 2 });
    view.set_overflow(0);
    view.set_slots(slint::ModelRc::new(slint::VecModel::from(slots)));
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
    // "…-settings-page-ra": the Library page's RetroAchievements section,
    // logged in with a system whose own default wins; "…-ra-new" before
    // anything is set up. "…-launcher-prefs" puts the preference list
    // over the Library page, "…-launcher-prefs-add" its Add page.
    let ra = screen.contains("settings-page-ra") || screen.contains("launcher-prefs");
    let ra_setup = screen.contains("ra-new");
    // "…-online-unlinked": the same page before an account is linked, with
    // the cursor on a row that waits for one.
    let unlinked = screen.contains("online-unlinked");
    // "…-settings-page-appearance": the Appearance page with a background
    // image set, so its Background section shows every row.
    let appearance = screen.contains("settings-page-appearance");
    // "…-settings-page-controls": the Controls page on its Controllers row,
    // two connected. "…-controllers" the page it opens, mid-search: two
    // cards (one charging, one running flat), and three finds (two of one
    // name, told apart by their address; one pairing; one that failed for
    // want of a PIN, under the cursor). "…-controllers-none" the same with
    // nothing connected and the cursor where the page opens it, on Paired
    // controllers. "…-controllers-empty" the page as it opens: the cards,
    // nothing found yet, and what to do. "…-paired" the page that row
    // opens.
    let controls = screen.contains("settings-page-controls");
    let controllers_page = screen.contains("settings-page-controllers");
    let none_connected = screen.contains("controllers-none");
    let nothing_found = screen.contains("controllers-empty");
    let paired_page = screen.contains("settings-page-paired");
    let found = [
        (
            "DualShock 4",
            "79:CA",
            DeviceValue::Found,
            PairFailure::None,
        ),
        (
            "DualShock 4",
            "11:0F",
            DeviceValue::Pairing,
            PairFailure::None,
        ),
        (
            "Arcade Stick",
            "",
            DeviceValue::Failed,
            PairFailure::PinRequired,
        ),
    ];
    let paired = [
        ("DualSense", "", DeviceValue::Connected),
        ("8BitDo Pro 2", "", DeviceValue::Connected),
        ("Xbox Wireless Controller", "", DeviceValue::NotConnected),
        ("", "", DeviceValue::NotConnected),
    ];
    let inputs = sizing::Scene::of(app, scene_w, scene_h, crt).inputs();
    let derived = zaparoo_app::sizing::derive(&inputs);
    let view = app.global::<generated::SettingsView>();
    let page_id = if online {
        SettingsPage::Online
    } else if appearance {
        SettingsPage::Appearance
    } else if controls {
        SettingsPage::Controls
    } else if controllers_page {
        SettingsPage::Controllers
    } else if paired_page {
        SettingsPage::PairedControllers
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
        launcher_preference: ra,
        retro_achievements: ra,
        ra_logged_in: ra && !ra_setup,
        ra_installed: ra && !ra_setup,
        ra_conflict: ra && !ra_setup,
        background_active: appearance,
        controllers_listed: controls || controllers_page,
        can_pair_controllers: controls || controllers_page || paired_page,
        bluetooth_found: if controllers_page && !nothing_found {
            found.len()
        } else {
            0
        },
        bluetooth_paired: if paired_page { paired.len() } else { 0 },
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
                Row::Item { id, index, control } => {
                    out.control = control.into();
                    out.item = i32::try_from(index).unwrap_or(0);
                    if id == "foundDevice" {
                        let (name, tail, value, error) = found[index];
                        out.value_name = name.into();
                        out.name_tail = tail.into();
                        out.device = value;
                        out.pair_error = error;
                    } else {
                        let (name, tail, value) = paired[index];
                        out.value_name = name.into();
                        out.name_tail = tail.into();
                        out.device = value;
                    }
                    geometry.row_height
                }
                Row::Field { id, control } => {
                    out.control = control.into();
                    let waits = (unlinked && rules::needs_online_link(id))
                        || (ra_setup && rules::needs_ra_install(id));
                    out.enabled = !waits;
                    match control {
                        Control::Toggle => {
                            out.checked = id == "showHidden"
                                || (id == "playtimeSync" && !unlinked)
                                || (!ra_setup
                                    && matches!(
                                        id,
                                        "raLaunchers"
                                            | "raProgressPopups"
                                            | "raChallengePopups"
                                            | "raLeaderboardPopups"
                                    ));
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
                                "raAccount" if ra_setup => "loggedOut",
                                "raAccount" => "loggedIn",
                                "raConflict" => "1",
                                "raPopupPosition" => "left",
                                "colorScheme" => "zaparoo-dark",
                                "colorIntensity" => "subtle",
                                "systemLogoStyle" => "tinted",
                                "screensaverTimeout" => "300",
                                "gameBackground" => "screenshot",
                                "backgroundListStyle" => "glass",
                                "noPairedControllers" => "",
                                _ => "auto",
                            }
                            .into();
                            out.value_name = match id {
                                "raAccount" => "Callan",
                                "raConflict" => "Super Nintendo",
                                _ => "",
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
                        Control::Navigate => {
                            if id == "backgroundImage" {
                                out.value = "/media/fat/Wallpapers".into();
                                out.value_name = "Wallpapers".into();
                            } else if id == "controllers" {
                                out.value = "2".into();
                            }
                        }
                        Control::Slider => {
                            let (value, cells) = match id {
                                "backgroundDim" => ("-1", zaparoo_app::background::dim_cells(-1)),
                                _ => ("60", zaparoo_app::background::blur_cells(60)),
                            };
                            out.value = value.into();
                            (out.cells, out.lit) = cells;
                        }
                        Control::Device => {}
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
    } else if ra {
        rows.iter()
            .position(|row| row.id == if ra_setup { "raLogIn" } else { "raLaunchers" })
            .and_then(|i| i32::try_from(i).ok())
            .unwrap_or(2)
    } else if appearance {
        rows.iter()
            .position(|row| row.id == "backgroundDim")
            .and_then(|i| i32::try_from(i).ok())
            .unwrap_or(2)
    } else if controls {
        rows.iter()
            .position(|row| row.id == "controllers")
            .and_then(|i| i32::try_from(i).ok())
            .unwrap_or(0)
    } else if controllers_page {
        if nothing_found {
            0
        } else if none_connected {
            i32::try_from(found.len()).unwrap_or(0)
        } else {
            2
        }
    } else if paired_page || page {
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
    if controllers_page {
        fixture_controllers(app, &inputs, none_connected);
    }
    if (controls || controllers_page || paired_page) && !none_connected {
        // One of the two is running flat: the header says so. The same
        // two pads as the cards: a charging DualSense and a flat 8BitDo.
        let shell = app.global::<Shell>();
        shell.set_controller_battery_low(true);
        shell.set_controller_batteries(slint::ModelRc::new(slint::VecModel::from(vec![
            generated::HeaderPad {
                fill: generated::PadFill::Full,
                charging: true,
                low: false,
                marks_before: 0,
            },
            generated::HeaderPad {
                fill: generated::PadFill::Empty,
                charging: false,
                low: true,
                marks_before: 1,
            },
        ])));
    }
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

/// The Hub's and a games-style screen's Menu chips, as their drivers
/// publish them.
fn fixture_chips(app: &App, mode: GamesMode) {
    let chip = |id: &str, key: &str, name: &str| MenuEntry {
        role: generated::MenuRole::Option,
        detail_key: "".into(),
        id: id.into(),
        label: name.into(),
        label_key: key.into(),
        enabled: true,
        reason_key: "".into(),
        detail: "".into(),
    };
    let games = match mode {
        GamesMode::Browse => vec![
            chip("jump_letter", "jump_letter", ""),
            chip("filter", "filter", ""),
            chip("search_here", "search_here", ""),
        ],
        GamesMode::Favorites => vec![
            chip("favorites_sort", "favorites_sort", "sort:name"),
            chip("favorites_grouping", "favorites_grouping", "group:none"),
            chip("launch_random_favorite", "launch_random_favorite", ""),
            chip("search_here", "search_here", ""),
            chip("more", "more", ""),
        ],
        GamesMode::Search => vec![
            chip("edit_search", "edit_search", ""),
            chip("add_to_hub", "save_to_hub", ""),
        ],
        GamesMode::Deck => vec![chip("play_deck", "play_deck", ""), chip("more", "more", "")],
        GamesMode::Decks => vec![chip("new_deck", "new_deck", "")],
        GamesMode::TopPlayed => vec![chip(
            "favorites_grouping",
            "favorites_grouping",
            "group:none",
        )],
        GamesMode::Recents => Vec::new(),
    };
    let bar = app.global::<generated::PageBar>();
    bar.set_games_chips(slint::ModelRc::new(slint::VecModel::from(games)));
    bar.set_hub_chips(slint::ModelRc::new(slint::VecModel::from(vec![
        chip("hub_add", "hub_add", ""),
        chip("more", "more", ""),
    ])));
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
    all_markers: bool,
) {
    use zaparoo_app::layouts::{self, Body, ThemeId, View};
    // Which collections a fixture row is in: (favorite, liked, disliked,
    // play later). `…-markers` screens walk every combination; the others
    // show a few.
    let markers = |i: usize| {
        let bits = if all_markers {
            i - 1
        } else {
            match i {
                1 => 0b1001,
                3 => 0b1011,
                5 => 0b0100,
                7 => 0b0001,
                _ => 0,
            }
        };
        (bits & 1 != 0, bits & 2 != 0, bits & 4 != 0, bits & 8 != 0)
    };
    let inputs = sizing::Scene::of(app, scene_w, scene_h, crt).inputs();
    let derived = zaparoo_app::sizing::derive(&inputs);
    let profile = layouts::profile(ThemeId::current(&inputs), View::GamesGrid, &inputs);
    let Body::Grid { grid, footer } = profile.body else {
        return;
    };
    let t240 = derived.tier == zaparoo_app::sizing::Tier::T240;
    let flat = mode != GamesMode::Browse;
    let screen_h = inputs.screen_height as i32;
    fixture_chips(app, mode);
    let grid_y = derived.header_bottom
        + profile.status.top_margin
        + profile.status.strip_height
        + if mode == GamesMode::Recents {
            0
        } else {
            derived.menu_row_block
        };
    let label_height = if flat {
        inputs.pct_h(7.0)
    } else {
        footer.active_label_height
    };
    let bottom = if t240 {
        derived.help_bar_height + label_height
    } else if flat {
        zaparoo_app::sizing::less_hidden_bar(&inputs, &derived, inputs.pct_h(15.0))
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
    let decks = mode == GamesMode::Decks;
    // On a deck, every third item is a card: no system, so no name band.
    let card = |i: usize| mode == GamesMode::Deck && i % 3 == 1;
    let labelled = flat && !decks;
    let cells: Vec<GridCell> = (1..=page_size)
        .map(|i| GridCell {
            name: if decks {
                DECKS[(i - 1) % DECKS.len()].into()
            } else if card(i) {
                format!("Card {i}").into()
            } else if i18n {
                i18n_titles[(i - 1) % i18n_titles.len()].into()
            } else if i == 1 && !flat {
                "Homebrew".into()
            } else {
                format!("Example Game Title {i}").into()
            },
            glyph_key: if i == 1 && !flat {
                "icons/Folder".into()
            } else if decks && i % 2 == 0 {
                "icons/BookMarked".into()
            } else if card(i) {
                "icons/File".into()
            } else {
                "".into()
            },
            top_label: if labelled && !card(i) {
                ["Genesis", "Super Nintendo", "PlayStation"][i % 3].into()
            } else {
                "".into()
            },
            favorite: !decks && !card(i) && (flat || i != 1) && markers(i).0,
            // A hidden game, dimmed rather than worded.
            hidden: !decks && !card(i) && i == 6,
            tags: if i == 3 && !decks {
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
    // The `…-markers` screens focus the hidden game.
    let selected = anchor_index.unwrap_or(if all_markers { 5 } else { 0 });
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
    let (list_row_height, list_visible) =
        list_metrics(app, scene_w, scene_h, crt, 0, mode != GamesMode::Recents);
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
            opens: i == 1 && !flat,
            favorite: (flat || i != 1) && markers(i).0,
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
    // The focused item's info line, as `games::info_line` would publish
    // it. Versions come from Core's title grouping; the fixture shows a
    // count on every game so the stack icon is in the snapshots.
    let focus_i = selected + 1;
    let game = !decks && !card(focus_i) && (flat || focus_i != 1);
    let (favorite, liked, disliked, play_later) = if game {
        markers(focus_i)
    } else {
        (false, false, false, false)
    };
    let mut info = generated::InfoLineData {
        name: label.name.clone(),
        favorite,
        liked,
        disliked,
        play_later,
        versions: if game { 3 } else { 0 },
        hidden: label.hidden,
        item_count: -1,
        tags: label.tags.clone(),
        ..Default::default()
    };
    if decks {
        // A saved copy of someone else's deck: the owner and the lock.
        info.item_count = 12;
        info.owner = "wizzo".into();
        info.locked = true;
        info.tags = "".into();
        view.set_count(i32::try_from(DECKS.len()).unwrap_or(0));
    } else if mode == GamesMode::TopPlayed {
        info.has_play_time = true;
        info.hours = 12;
        info.minutes = 5;
    }
    view.set_info(info);
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
/// The Decks fixture's deck names.
const DECKS: [&str; 6] = [
    "Couch co-op",
    "Weekend",
    "Club night",
    "Arcade classics",
    "Party games",
    "Retro RPGs",
];

#[allow(
    clippy::too_many_lines,
    reason = "one fixture for every Systems mode keeps their states side by side"
)]
fn fixture_systems(app: &App, scene_w: f64, scene_h: f64, crt: bool, mode: SystemsMode) {
    use zaparoo_app::layouts::{self, Body, ThemeId, View};
    let favorites = mode == SystemsMode::Favorites;
    let top = mode == SystemsMode::TopPlayed;
    let inputs = sizing::Scene::of(app, scene_w, scene_h, crt).inputs();
    let derived = zaparoo_app::sizing::derive(&inputs);
    let profile = layouts::profile(ThemeId::current(&inputs), View::SystemsGrid, &inputs);
    let Body::Grid { grid, footer } = profile.body else {
        return;
    };
    let chip = |id: &str, key: &str, name: &str| MenuEntry {
        role: generated::MenuRole::Option,
        detail_key: "".into(),
        id: id.into(),
        label: name.into(),
        label_key: key.into(),
        enabled: true,
        reason_key: "".into(),
        detail: "".into(),
    };
    app.global::<generated::PageBar>()
        .set_systems_chips(slint::ModelRc::new(slint::VecModel::from(if top {
            vec![chip(
                "favorites_grouping",
                "favorites_grouping",
                "group:system",
            )]
        } else if favorites {
            vec![
                chip("favorites_grouping", "favorites_grouping", "group:system"),
                chip("search_here", "search_here", ""),
            ]
        } else {
            vec![chip("search_here", "search_here", "")]
        })));
    let grid_y = derived.header_bottom
        + profile.status.top_margin
        + profile.status.strip_height
        + derived.menu_row_block;
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
            hidden: i == 15 && !top,
            ..Default::default()
        })
        .collect();
    let (row_height, visible) = list_metrics(app, scene_w, scene_h, crt, 0, favorites);
    let list_rows: Vec<GridCell> = (13..13 + visible)
        .map(|i| GridCell {
            name: format!("Example System {i}").into(),
            wordmark: true,
            hidden: i == 15,
            ..Default::default()
        })
        .collect();
    let view = app.global::<SystemsView>();
    view.set_mode(mode);
    view.set_favorites_total(if favorites { 87 } else { -1 });
    view.set_label_count(if favorites || top { 12 } else { -1 });
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
    view.set_label_hidden(!top);
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
            "list-toggles",
            "list-slider",
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
            ("liked", Screen::Favorites),
            ("crt-play-later", Screen::Favorites),
            ("liked-systems", Screen::FavoriteSystems),
            ("recents", Screen::Recents),
            ("search", Screen::Search),
            ("crt-search-scoped", Screen::Search),
            ("search-results", Screen::SearchResults),
            ("rename", Screen::Rename),
            ("crt-ra-login", Screen::RaLogin),
            ("settings-page-launcher-prefs", Screen::Settings),
            ("crt-file-browser-list", Screen::FileBrowser),
            ("game-info", Screen::Games),
            ("settings-page", Screen::Settings),
            ("about", Screen::About),
            ("update-intro", Screen::Update),
            ("crt-update-details", Screen::Update),
            ("decks", Screen::Decks),
            ("crt-decks", Screen::Decks),
            ("decks-list", Screen::Decks),
            ("deck-list", Screen::Deck),
            ("top-played", Screen::TopPlayed),
            ("crt-top-played-list", Screen::TopPlayed),
            ("top-played-systems", Screen::TopPlayedSystems),
            ("games-markers", Screen::Games),
            ("crt-games-list-markers", Screen::Games),
            ("crt-deck", Screen::Deck),
            ("context-deck", Screen::Games),
            ("crt-context-deck", Screen::Games),
            ("deck-item-manage", Screen::Deck),
            ("crt-hub-deck", Screen::Hub),
            ("core-picker", Screen::Hub),
            ("crt-core-confirm", Screen::Hub),
        ] {
            assert_eq!(fixture_screen(name), screen, "{name}");
        }
    }
}
