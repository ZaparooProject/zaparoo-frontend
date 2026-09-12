// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

//! Motion and navigation checked with software-rendered frames on a stepped
//! clock. Assert coherent source/destination composition, local cursor motion,
//! visible pushes before dispatch, command ownership, and eventual quiescence.

use crate::{App, GridCell, HubView, Shell, Sizing, SystemsView};
use slint::platform::software_renderer::{MinimalSoftwareWindow, RepaintBufferType, Rgb565Pixel};
use slint::platform::{Platform, WindowAdapter};
use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;

thread_local! {
    static CLOCK: Cell<u64> = const { Cell::new(0) };
    static WINDOW: RefCell<Option<Rc<MinimalSoftwareWindow>>> = const { RefCell::new(None) };
}

/// A platform whose clock only moves when the test moves it, making each
/// animation the same number of frames on every machine.
pub(crate) struct ProbePlatform;

impl Platform for ProbePlatform {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, slint::PlatformError> {
        let window = MinimalSoftwareWindow::new(RepaintBufferType::NewBuffer);
        WINDOW.with(|slot| *slot.borrow_mut() = Some(window.clone()));
        Ok(window)
    }

    fn duration_since_start(&self) -> Duration {
        Duration::from_millis(CLOCK.with(Cell::get))
    }
}

const W: u32 = 320;
const H: u32 = 180;
const TICK_MS: u64 = 16;
/// Enough ticks for the retained 240 ms page slide to settle.
const SETTLE_TICKS: u32 = 20;

/// Render one frame and fingerprint it.
fn frame(window: &Rc<MinimalSoftwareWindow>) -> Option<u64> {
    let mut buf = vec![Rgb565Pixel(0); (W * H) as usize];
    let drew = window.draw_if_needed(|renderer| {
        renderer.render(&mut buf, W as usize);
    });
    if !drew {
        return None;
    }
    let mut hash = 0u64;
    for (i, pixel) in buf.iter().enumerate() {
        hash = hash
            .wrapping_mul(31)
            .wrapping_add(u64::from(pixel.0) ^ (i as u64));
    }
    Some(hash)
}

fn pixels(window: &Rc<MinimalSoftwareWindow>) -> Vec<Rgb565Pixel> {
    let mut buf = vec![Rgb565Pixel(0); (W * H) as usize];
    window.request_redraw();
    window.draw_if_needed(|renderer| {
        renderer.render(&mut buf, W as usize);
    });
    buf
}

fn assert_region_matches(
    actual: &[Rgb565Pixel],
    expected: &[Rgb565Pixel],
    x: std::ops::Range<usize>,
    y: std::ops::Range<usize>,
    message: &str,
) {
    let mismatches = y
        .flat_map(|row| x.clone().map(move |column| row * W as usize + column))
        .filter(|&index| actual[index] != expected[index])
        .count();
    if mismatches != 0 {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../output/slint-ui/motion-tests");
        let _ = std::fs::create_dir_all(&dir);
        for (name, pixels) in [("actual", actual), ("expected", expected)] {
            let image = image::RgbImage::from_fn(W, H, |x, y| {
                let word = pixels[(y * W + x) as usize].0;
                image::Rgb([
                    u8::try_from((word >> 11) * 255 / 31).unwrap_or(0),
                    u8::try_from(((word >> 5) & 63) * 255 / 63).unwrap_or(0),
                    u8::try_from((word & 31) * 255 / 31).unwrap_or(0),
                ])
            });
            let _ = image.save(dir.join(format!("{name}-{}.png", std::process::id())));
        }
    }
    assert_eq!(mismatches, 0, "{message}: {mismatches} mismatched pixels");
}

/// Step the clock `ticks` times and return how many of those frames
/// painted something different from the frame before. A slide shows up
/// as a long run of them; a cut is one.
fn distinct_frames(window: &Rc<MinimalSoftwareWindow>, ticks: u32) -> u32 {
    let mut previous = None;
    let mut moved = 0;
    for _ in 0..ticks {
        CLOCK.with(|clock| clock.set(clock.get() + TICK_MS));
        slint::platform::update_timers_and_animations();
        if let Some(hash) = frame(window) {
            if previous != Some(hash) {
                moved += 1;
            }
            previous = Some(hash);
        }
    }
    moved
}

/// Let everything in flight land, so the next case starts from a still
/// picture.
fn settle(window: &Rc<MinimalSoftwareWindow>) {
    distinct_frames(window, SETTLE_TICKS);
}

fn cells(count: usize, tag: &str) -> ModelRc<GridCell> {
    ModelRc::new(VecModel::from(
        (0..count)
            .map(|i| GridCell {
                name: SharedString::from(format!("{tag} {i}")),
                ..GridCell::default()
            })
            .collect::<Vec<_>>(),
    ))
}

struct TestStateDir(std::path::PathBuf);
impl Drop for TestStateDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// An app on a fixed-size software window with content on both of the
/// screens the cases travel between.
#[allow(
    clippy::expect_used,
    reason = "a probe that cannot build its own window has nothing to assert"
)]
fn boot() -> (App, Rc<MinimalSoftwareWindow>) {
    // nextest runs each platform-owning UI test in its own process. Never let
    // router persistence in these fixtures overwrite the user's frontend state.
    thread_local! {
        static STATE_DIR: TestStateDir = {
            let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).expect("clock").as_nanos();
            let path = std::env::temp_dir().join(format!("zaparoo-motion-{}-{stamp}", std::process::id()));
            std::fs::create_dir(&path).expect("isolated UI state directory");
            TestStateDir(path)
        };
    }
    STATE_DIR.with(|dir| std::env::set_var("ZAPAROO_STATE_FILE", dir.0.join("state.toml")));
    let app = App::new().expect("the app builds under the probe platform");
    let window = WINDOW
        .with(|slot| slot.borrow().clone())
        .expect("the platform handed out a window");
    window.set_size(slint::PhysicalSize::new(W, H));
    let sizing = app.global::<Sizing>();
    sizing.set_screen_width(W as f32);
    sizing.set_screen_height(H as f32);
    crate::router::refresh_layout(&app);

    let shell = app.global::<Shell>();
    shell.set_boot_complete(true);
    shell.set_active_screen(SharedString::from("hub"));

    let hub = app.global::<HubView>();
    hub.set_loaded(true);
    hub.set_focus_ready(true);
    hub.set_cells(cells(10, "Category"));
    app.global::<SystemsView>().set_cells(cells(10, "System"));
    (app, window)
}

#[test]
fn tile_press_survives_an_unchanged_model_publication() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    let hub = app.global::<HubView>();
    hub.set_cell_width(50.0);
    hub.set_cell_height(40.0);
    hub.set_grid_y(40.0);
    hub.set_grid_height(100.0);
    settle(&window);
    app.window().request_redraw();
    let resting = frame(&window);
    assert!(app.global::<Sizing>().get_press_edge_height() > 0.0);
    let hub = app.global::<HubView>();
    // Activation renders the current page again before publishing its pulse.
    crate::view_model::publish_cells(
        &hub.get_cells(),
        cells(10, "Category").iter().collect(),
        |rows| hub.set_cells(rows),
    );
    hub.set_activate_pulse(1);
    distinct_frames(&window, 4);
    app.window().request_redraw();
    let pressed = frame(&window);
    assert_ne!(resting, pressed, "the selected tile must push down");
    hub.set_release_pulse(1);
    settle(&window);
    app.window().request_redraw();
    assert_eq!(
        resting,
        frame(&window),
        "release must restore the raised face"
    );
}

#[allow(
    clippy::panic,
    reason = "missing commitment target is a broken test fixture"
)]
fn arm_feedback(app: &App, commits: &Rc<Cell<u32>>) {
    let Some(target) = crate::press_feedback::current(app) else {
        panic!("fixture has no commitment target");
    };
    let commits = commits.clone();
    crate::press_feedback::dispatch(app, &target, move |_| commits.set(commits.get() + 1));
}

#[test]
#[allow(
    clippy::expect_used,
    reason = "embedded logo is a required test fixture"
)]
fn system_logo_tile_feedback_preserves_images_and_settles() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    crate::sizing::apply_scene(
        &app,
        crate::sizing::Scene::of(&app, f64::from(W), f64::from(H), false),
    );
    app.global::<Shell>().set_active_screen("systems".into());
    let pixels = crate::system_logos::logo_for("SNES").expect("embedded SNES logo");
    let image = || {
        slint::Image::from_rgba8(
            slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(
                &pixels.rgba,
                pixels.width,
                pixels.height,
            ),
        )
    };
    let logo = image();
    let cell = GridCell {
        name: "Super Nintendo".into(),
        cover: logo.clone(),
        cover_focus: logo,
        has_cover: true,
        has_cover_focus: true,
        wordmark: true,
        ..Default::default()
    };
    let rebuilt = GridCell {
        cover: image(),
        cover_focus: image(),
        ..cell.clone()
    };
    assert!(
        !crate::view_model::same_cell(&cell, &rebuilt),
        "rebuilding equal pixels changes Slint image identity"
    );
    let page = ModelRc::new(VecModel::from(vec![cell]));
    let systems = app.global::<SystemsView>();
    systems.set_cells(page.clone());
    systems.set_count(1);
    systems.set_focus_ready(true);
    systems.set_cell_width(70.0);
    systems.set_cell_height(60.0);
    systems.set_grid_y(40.0);
    systems.set_grid_height(90.0);
    settle(&window);
    app.window().request_redraw();
    let resting = frame(&window);
    crate::systems::publish_press(&app, 1, 0);
    assert!(
        systems.get_cells() == page,
        "activation must not replace the logo page"
    );
    frame(&window);
    distinct_frames(&window, 2);
    app.window().request_redraw();
    assert_ne!(
        resting,
        frame(&window),
        "system tile must show its local press feedback"
    );
    crate::systems::publish_press(&app, 1, 1);
    assert!(
        systems.get_cells() == page,
        "release must preserve the same delegates"
    );
    settle(&window);
    app.window().request_redraw();
    assert_eq!(
        resting,
        frame(&window),
        "release must animate back to the raised face"
    );
}

#[allow(
    clippy::expect_used,
    reason = "offline router fixture must construct its runtime"
)]
fn offline_ctx() -> (tokio::runtime::Runtime, crate::router::Ctx) {
    use std::sync::{Arc, Mutex};
    // Never drive this current-thread runtime: no network tasks or writes run.
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    let handle = runtime.handle().clone();
    let client = zaparoo_core::client::Client::new("ws://127.0.0.1:1".into(), &handle);
    let ctx = crate::router::Ctx {
        store: zaparoo_core::store::Store::new(client, handle.clone()),
        handle,
        media: crate::media_cache::MediaCache::new().0,
        shared: Arc::new(Mutex::new(crate::router::Shared::new(
            zaparoo_core::persist::PersistedState::default(),
            false,
            vec![],
            vec![],
            String::new(),
            std::path::PathBuf::new(),
        ))),
        clock_twelve_hour: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        dormant: tokio::sync::watch::channel(false).0,
        status: crate::status::new("en"),
        config_path: std::path::PathBuf::new(),
        crt_enabled: false,
        is_mister: false,
        framebuffer_size: (W, H),
    };
    (runtime, ctx)
}

#[test]
fn launch_dormancy_is_local_only() {
    assert!(crate::local_lifecycle_enabled(
        "ws://127.0.0.1:7497/api/v0.1"
    ));
    assert!(crate::local_lifecycle_enabled(
        "ws://localhost:7497/api/v0.1"
    ));
    assert!(!crate::local_lifecycle_enabled(
        "ws://192.0.2.10:7497/api/v0.1"
    ));
}

#[test]
fn desktop_dormancy_stops_motion_and_screensaver_until_resume() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, _window) = boot();
    let (_runtime, ctx) = offline_ctx();
    let mut dormant = ctx.dormant.subscribe();
    app.global::<crate::Motion>().set_enabled(true);
    app.global::<Shell>().set_saver_armed(true);

    crate::set_dormant(&ctx, &app, true);
    assert!(app.global::<Shell>().get_dormant());
    assert!(!app.global::<Shell>().get_saver_armed());
    assert!(!app.global::<crate::Motion>().get_enabled());
    assert!(*dormant.borrow_and_update());

    crate::set_dormant(&ctx, &app, false);
    assert!(!app.global::<Shell>().get_dormant());
    assert!(app.global::<crate::Motion>().get_enabled());
    assert!(!*dormant.borrow_and_update());
}

#[test]
fn dormant_surface_is_static_and_opaque_over_live_ui() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    settle(&window);
    app.window().request_redraw();
    let active = frame(&window);

    app.global::<Shell>().set_dormant(true);
    let dormant = frame(&window);
    assert_ne!(active, dormant, "dormancy must replace the live screen");
    assert_eq!(
        distinct_frames(&window, 8),
        0,
        "the dormant face must not animate"
    );

    app.global::<Shell>()
        .set_active_screen(SharedString::from("systems"));
    app.window().request_redraw();
    assert_eq!(
        dormant,
        frame(&window),
        "live screen changes must not paint through the dormant face"
    );
}

#[test]
fn held_game_pages_cut_at_repeat_cadence_but_taps_keep_slides() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    let (_runtime, ctx) = offline_ctx();
    crate::sizing::apply_scene(
        &app,
        crate::sizing::Scene::of(&app, f64::from(W), f64::from(H), false),
    );
    app.global::<Shell>().set_active_screen("games".into());
    {
        let mut state = crate::router::lock(&ctx.shared);
        state.persist.settings.reduce_motion = false;
        state.games.loading = false;
        state.games.rows = (0..200)
            .map(|i| crate::games::GameRow {
                media_id: None,
                name: format!("Game {i}"),
                path: String::new(),
                entry_type: zaparoo_app::media_list::EntryType::Media,
                file_count: 0,
                system_id: String::new(),
                system_name: String::new(),
                zap_script: String::new(),
                tag_labels: vec![],
                has_cover: false,
                is_favorite: false,
                media_capable: false,
                root_distinguisher: String::new(),
                detail_rows: vec![],
                display: format!("Game {i}"),
                suffix: String::new(),
            })
            .collect();
        state.games.grid.set_item_count(200);
    }
    crate::games::render(&ctx, &app);
    crate::router::handle_action(&ctx, &app, "page_next");
    assert!(crate::router::lock(&ctx.shared).games.sliding);
    let view = app.global::<crate::GamesView>();
    let outgoing = view.get_cells().row_data(0).map(|cell| cell.name);
    let incoming = view.get_next_cells().row_data(0).map(|cell| cell.name);
    assert!(incoming.is_some());
    crate::games::render(&ctx, &app);
    assert_eq!(
        view.get_cells().row_data(0).map(|cell| cell.name),
        outgoing,
        "redraw must retain outgoing page"
    );
    assert_eq!(
        view.get_next_cells().row_data(0).map(|cell| cell.name),
        incoming,
        "redraw must not erase the incoming page"
    );
    let first = crate::router::lock(&ctx.shared).games.grid.current_page();
    crate::input::dispatch_repeat(&ctx, &app, "page_next", true);
    assert_eq!(
        crate::router::lock(&ctx.shared).games.grid.current_page(),
        first + 1,
        "new input must interrupt an in-flight page instead of waiting"
    );
    let first = first + 1;
    distinct_frames(&window, 18);
    assert!(!crate::router::lock(&ctx.shared).games.sliding);
    for expected in first + 1..=first + 5 {
        crate::input::dispatch_repeat(&ctx, &app, "page_next", true);
        let state = crate::router::lock(&ctx.shared);
        assert_eq!(state.games.grid.current_page(), expected);
        assert!(
            !state.games.sliding,
            "rapid pages must not arm a 260 ms gate"
        );
        drop(state);
        assert!(
            !crate::input::rapid_page(&ctx),
            "repeat context must not leak"
        );
        CLOCK.with(|clock| clock.set(clock.get() + 90));
        slint::platform::update_timers_and_animations();
        frame(&window);
    }
    app.global::<crate::Overlays>().set_list_open(true);
    crate::input::dispatch_repeat(&ctx, &app, "page_next", true);
    assert_eq!(
        crate::router::lock(&ctx.shared).games.grid.current_page(),
        first + 5,
        "modal-owned repeats must not page the background"
    );
    assert!(!crate::input::rapid_page(&ctx));
    app.global::<crate::Overlays>().set_list_open(false);
    crate::router::handle_action(&ctx, &app, "page_next");
    assert!(
        crate::router::lock(&ctx.shared).games.sliding,
        "ordinary taps retain motion"
    );
    distinct_frames(&window, 18);
    assert_eq!(
        crate::router::lock(&ctx.shared).games.grid.current_page(),
        first + 6
    );
}

#[test]
#[allow(
    clippy::too_many_lines,
    reason = "one platform-owned matrix inventories every root route and Settings category"
)]
fn every_screen_route_and_settings_category_commits_without_animation_delay() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    let (_runtime, ctx) = offline_ctx();
    crate::sizing::apply_scene(
        &app,
        crate::sizing::Scene::of(&app, f64::from(W), f64::from(H), false),
    );
    seat_folder(&ctx, &app, "Game", 0);
    let shell = app.global::<Shell>();
    for (from, to) in [
        ("hub", "systems"),
        ("systems", "games"),
        ("hub", "games"),
        ("hub", "favorite-systems"),
        ("favorite-systems", "favorites"),
        ("hub", "favorites"),
        ("hub", "recents"),
        ("hub", "settings"),
        ("settings", "about"),
    ] {
        shell.set_active_screen(from.into());
        app.global::<SystemsView>().set_mode(
            if from == "favorite-systems" || to == "favorite-systems" {
                "favorite-systems"
            } else {
                "systems"
            }
            .into(),
        );
        app.global::<crate::GamesView>().set_mode(
            if ["games", "favorites", "recents"].contains(&to) {
                to
            } else {
                "games"
            }
            .into(),
        );
        crate::settings::open_page(
            &ctx,
            &app,
            if to == "about" {
                "pageSupportAbout"
            } else {
                ""
            },
        );
        crate::router::refresh_layout(&app);
        settle(&window);
        crate::router::transition_to_screen(&app, to, 1);
        assert_eq!(
            shell.get_active_screen().as_str(),
            to,
            "ready route commits now"
        );
        assert!(!shell.get_transitioning());
        settle(&window);
        if to == "about" {
            crate::settings::show_about_return(&ctx, &app);
        } else {
            crate::router::transition_to_screen(&app, from, -1);
        }
        assert_eq!(shell.get_active_screen().as_str(), from, "Back commits now");
        settle(&window);
    }
    shell.set_active_screen("settings".into());
    crate::settings::open_page(&ctx, &app, "");
    let settings = app.global::<crate::SettingsView>();
    for (index, page) in zaparoo_app::settings::PAGES.iter().enumerate() {
        settings.set_index(index as i32);
        settle(&window);
        crate::settings::handle_action(&ctx, &app, "accept");
        assert_eq!(
            settings.get_page().as_str(),
            page.id,
            "Settings is synchronous"
        );
        assert!(!shell.get_transitioning());
        crate::settings::handle_action(&ctx, &app, "cancel");
        assert!(settings.get_page().is_empty());
        assert_eq!(
            settings.get_index(),
            index as i32,
            "Back restores the category tile"
        );
    }
    crate::settings::open_page(&ctx, &app, "pageLibraryData");
    assert!(settings.get_scroll().abs() < f32::EPSILON);
    for _ in 0..6 {
        crate::settings::handle_action(&ctx, &app, "down");
    }
    assert!(
        settings.get_scroll() > 0.0,
        "moving focus past the viewport must scroll the selected row into view"
    );

    shell.set_reduce_motion(true);
    crate::settings::navigate_page(&ctx, &app, "pageSupportAbout");
    assert!(!shell.get_transitioning());
    crate::settings::navigate_page(&ctx, &app, "");
    assert!(!shell.get_transitioning());
}

fn game_rows(name: &str, count: usize) -> Vec<crate::games::GameRow> {
    (0..count)
        .map(|i| {
            let mut row = crate::games::GameRow::from(&zaparoo_core::media_types::BrowseEntry {
                name: format!("{name} {i}"),
                entry_type: "media".into(),
                has_cover: false,
                ..Default::default()
            });
            row.display.clone_from(&row.name);
            row
        })
        .collect()
}

fn seat_folder(ctx: &crate::router::Ctx, app: &App, name: &str, index: usize) {
    let mut shared = crate::router::lock(&ctx.shared);
    shared.games.rows = game_rows(name, 8);
    shared.games.grid.set_item_count(8);
    shared.games.grid.set_current_index_immediate(index);
    drop(shared);
    crate::games::render(ctx, app);
}

#[test]
fn page_lookahead_is_bounded_and_delayed_partial_pages_wait_then_slide() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    let (_runtime, ctx) = offline_ctx();
    crate::sizing::apply_scene(
        &app,
        crate::sizing::Scene::of(&app, f64::from(W), f64::from(H), false),
    );
    app.global::<Shell>().set_active_screen("games".into());
    seat_folder(&ctx, &app, "First", 0);
    let size = crate::router::lock(&ctx.shared).games.grid.page_size();
    {
        let mut shared = crate::router::lock(&ctx.shared);
        let model = &mut shared.games;
        model.rows.truncate(size);
        model.grid.total_items_override = Some(size * 5);
        model.total_files = (size * 5) as u32;
        model.grid.set_item_count(size);
        model.grid.set_has_more_pages(true);
        model.next_cursor = Some("next".into());
    }
    crate::games::render(&ctx, &app);
    settle(&window);
    crate::games::drain_load_requests(&ctx, &app);
    assert!(
        crate::router::lock(&ctx.shared).games.loading_more,
        "lookahead starts before a keypress"
    );
    crate::games::on_append(
        &ctx,
        &app,
        0,
        Ok((game_rows("Second", size), Some("next".into()))),
    );
    assert!(crate::router::lock(&ctx.shared).games.loading_more);
    crate::games::on_append(
        &ctx,
        &app,
        0,
        Ok((game_rows("Third", size), Some("next".into()))),
    );
    assert!(
        !crate::router::lock(&ctx.shared).games.loading_more,
        "stop at two pages ahead"
    );
    let view = app.global::<crate::GamesView>();
    let outgoing = view.get_cells().row_data(0).map(|cell| cell.name);
    crate::games::handle_action(&ctx, &app, "page_prev");
    assert!(crate::router::lock(&ctx.shared)
        .games
        .grid
        .has_pending_target());
    assert!(!crate::router::lock(&ctx.shared).games.sliding);
    crate::games::on_append(&ctx, &app, 0, Err("offline".into()));
    assert_eq!(view.get_cells().row_data(0).map(|cell| cell.name), outgoing);
    crate::games::handle_action(&ctx, &app, "page_prev");
    crate::games::on_append(
        &ctx,
        &app,
        0,
        Ok((game_rows("Fourth", size), Some("next".into()))),
    );
    crate::games::on_append(
        &ctx,
        &app,
        0,
        Ok((game_rows("Last", 1), Some("next".into()))),
    );
    assert_eq!(view.get_cells().row_data(0).map(|cell| cell.name), outgoing);
    assert!(
        !crate::router::lock(&ctx.shared).games.sliding,
        "a partial nonterminal page must wait"
    );
    crate::games::on_append(&ctx, &app, 0, Ok((game_rows("Last", size - 1), None)));
    assert!(crate::router::lock(&ctx.shared).games.sliding);
    assert_eq!(view.get_cells().row_data(0).map(|cell| cell.name), outgoing);
    assert_eq!(view.get_next_cells().row_count(), size);
    assert!(distinct_frames(&window, 18) > 5);
    assert_eq!(view.get_page(), 4);
    assert!(!crate::router::lock(&ctx.shared).games.loading_more);
}

#[test]
fn rapid_letter_requires_a_qualified_hold_and_clears_on_taps_and_quiet() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    let (_runtime, ctx) = offline_ctx();
    crate::sizing::apply_scene(
        &app,
        crate::sizing::Scene::of(&app, f64::from(W), f64::from(H), false),
    );
    app.global::<Shell>().set_active_screen("games".into());
    seat_folder(&ctx, &app, "Game", 0);
    settle(&window);
    let view = app.global::<crate::GamesView>();
    for _ in 0..3 {
        crate::router::handle_action(&ctx, &app, "page_next");
        assert!(crate::router::lock(&ctx.shared).games.sliding);
        assert!(!view.get_rapid_active());
        assert!(
            view.get_rapid_letter().is_empty(),
            "ordinary taps must not show the rapid badge"
        );
        distinct_frames(&window, 18);
    }
    crate::input::dispatch_repeat(&ctx, &app, "page_next", true);
    assert!(view.get_rapid_active());
    assert!(!view.get_rapid_letter().is_empty());
    assert!(!crate::router::lock(&ctx.shared).games.sliding);
    crate::router::handle_action(&ctx, &app, "page_prev");
    assert!(!view.get_rapid_active());
    assert!(
        view.get_rapid_letter().is_empty(),
        "a fresh tap must retire the held badge"
    );
    assert!(crate::router::lock(&ctx.shared).games.sliding);
    distinct_frames(&window, 18);
    crate::input::dispatch_repeat(&ctx, &app, "page_next", true);
    assert!(!view.get_rapid_letter().is_empty());
    CLOCK.with(|clock| clock.set(clock.get() + zaparoo_app::input::RAPID_QUIET_MS));
    slint::platform::update_timers_and_animations();
    assert!(!view.get_rapid_active());
    assert!(
        view.get_rapid_letter().is_empty(),
        "quiet must clear the letter as well as rapid mode"
    );
    crate::router::handle_action(&ctx, &app, "page_next");
    settle(&window);
    assert!(
        view.get_rapid_letter().is_empty(),
        "later nonrapid flips cannot strand an old badge"
    );
}

#[test]
fn systems_redraw_retains_both_pages_during_a_slide() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    let (_runtime, ctx) = offline_ctx();
    crate::sizing::apply_scene(
        &app,
        crate::sizing::Scene::of(&app, f64::from(W), f64::from(H), false),
    );
    app.global::<Shell>().set_active_screen("systems".into());
    {
        let mut shared = crate::router::lock(&ctx.shared);
        shared.systems_model.rows = (0..40)
            .map(|i| zaparoo_app::systems::SystemRow {
                id: format!("System{i}"),
                name: format!("System {i}"),
                cover_key: String::new(),
                category: String::new(),
                hidden: false,
                zap_script: String::new(),
                release_date: String::new(),
                manufacturer: String::new(),
                media_count: None,
            })
            .collect();
        shared.systems_model.grid.set_item_count(40);
    }
    crate::systems::render(&ctx, &app);
    settle(&window);
    let view = app.global::<SystemsView>();
    for direction in [1, -1] {
        let from = crate::router::lock(&ctx.shared)
            .systems_model
            .grid
            .current_page();
        let outgoing = view.get_cells().row_data(0).map(|cell| cell.name);
        assert!(crate::router::lock(&ctx.shared)
            .systems_model
            .grid
            .page_by(direction));
        crate::systems::slide_to_current_page(&ctx, &app, from);
        let incoming = view.get_next_cells().row_data(0).map(|cell| cell.name);
        assert!(incoming.is_some());
        crate::systems::render(&ctx, &app);
        assert_eq!(view.get_cells().row_data(0).map(|cell| cell.name), outgoing);
        assert_eq!(
            view.get_next_cells().row_data(0).map(|cell| cell.name),
            incoming
        );
        assert!(distinct_frames(&window, 18) > 5);
        settle(&window);
    }
}

#[test]
fn folders_keep_the_source_until_ready_in_both_directions() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    let (_runtime, ctx) = offline_ctx();
    crate::sizing::apply_scene(
        &app,
        crate::sizing::Scene::of(&app, f64::from(W), f64::from(H), false),
    );
    app.global::<Shell>().set_active_screen("games".into());
    let view = app.global::<crate::GamesView>();
    seat_folder(&ctx, &app, "Parent", 3);
    settle(&window);

    for (direction, name, index) in [(1, "Child", 0), (-1, "Parent", 3)] {
        let outgoing = view.get_cells().row_data(0).map(|cell| cell.name);
        crate::folder_motion::capture(&app, direction);
        {
            let mut shared = crate::router::lock(&ctx.shared);
            shared.games.ticket += 1;
            shared.games.folder_direction = direction;
            shared.games.loading = true;
        }
        crate::games::render(&ctx, &app);
        assert_eq!(view.get_cells().row_data(0).map(|cell| cell.name), outgoing);
        assert!(app.global::<Shell>().get_transitioning());
        let source = pixels(&window);
        distinct_frames(&window, 9);
        assert_eq!(
            source,
            pixels(&window),
            "pending folder keeps the source intact"
        );

        crate::router::lock(&ctx.shared).games.loading = false;
        seat_folder(&ctx, &app, name, index);
        crate::folder_motion::start(&ctx, &app);
        assert_eq!(view.get_current_index(), index as i32);
        assert!(view
            .get_cells()
            .row_data(0)
            .is_some_and(|cell| cell.name.starts_with(name)));
        assert!(!app.global::<Shell>().get_transitioning());
        let destination = pixels(&window);
        settle(&window);
        assert_eq!(destination, pixels(&window), "no arrival motion");
        assert!(!view.get_folder_slide());
    }

    crate::folder_motion::capture(&app, 1);
    {
        let mut shared = crate::router::lock(&ctx.shared);
        shared.games.folder_direction = 1;
        shared.games.loading = true;
    }
    crate::games::render(&ctx, &app);
    distinct_frames(&window, 9);
    {
        let mut shared = crate::router::lock(&ctx.shared);
        shared.games.loading = false;
        shared.games.rows.clear();
        shared.games.grid.set_item_count(0);
    }
    crate::games::render(&ctx, &app);
    crate::folder_motion::start(&ctx, &app);
    distinct_frames(&window, 11);
    assert!(!app.global::<Shell>().get_transitioning());
    assert_eq!(view.get_count(), 0);

    crate::router::lock(&ctx.shared)
        .persist
        .settings
        .reduce_motion = true;
    app.global::<Shell>().set_reduce_motion(true);
    crate::folder_motion::capture(&app, 1);
    {
        let mut shared = crate::router::lock(&ctx.shared);
        shared.games.folder_direction = 1;
    }
    seat_folder(&ctx, &app, "Cut", 0);
    crate::folder_motion::start(&ctx, &app);
    assert!(!app.global::<Shell>().get_transitioning());
    assert!(view
        .get_cells()
        .row_data(0)
        .is_some_and(|cell| cell.name.starts_with("Cut")));
}

#[test]
fn launcher_save_keeps_picker_locked_delays_cue_and_retries_original_choice() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, _) = boot();
    let (_runtime, ctx) = offline_ctx();
    app.global::<crate::Motion>().set_enabled(false);
    let ov = app.global::<crate::Overlays>();
    ov.set_list_open(true);
    ov.set_list_entries(ModelRc::new(VecModel::from(vec![
        crate::router::menu_entry("default", "Default"),
        crate::router::menu_entry("alternate", "Alternate"),
    ])));
    ov.set_list_index(1);
    let rows = ov.get_list_entries();
    crate::router::lock(&ctx.shared)
        .persist
        .settings
        .mouse_enabled = true;
    crate::router::bind_context_input(&std::sync::Arc::new(ctx.clone()), &app);
    let ticket = crate::launchers::begin_save(&ctx, &app);
    ov.invoke_pointer_choice("list".into(), 0, true);
    crate::router::handle_action(&ctx, &app, "up");
    crate::router::handle_action(&ctx, &app, "cancel");
    assert_eq!(ov.get_list_index(), 1);
    assert!(ov.get_list_open());
    assert_eq!(ov.get_list_entries(), rows);
    assert!(crate::press_feedback::current(&app).is_none());
    CLOCK.with(|clock| clock.set(clock.get() + 299));
    slint::platform::update_timers_and_animations();
    assert!(!ov.get_launcher_saving_visible());
    CLOCK.with(|clock| clock.set(clock.get() + 1));
    slint::platform::update_timers_and_animations();
    assert!(ov.get_launcher_saving_visible());
    let payload = serde_json::json!(["system", "SNES", "", "alternate"]).to_string();
    crate::launchers::finish_save(&ctx, &app, ticket, Some(&payload));
    assert!(!ov.get_list_open());
    assert_eq!(ov.get_dialog_detail(), "launcher_save");
    crate::router::handle_action(&ctx, &app, "accept");
    assert!(ov.get_list_open() && ov.get_launcher_saving());
    let selected = ov.get_list_entries().row_data(ov.get_list_index() as usize);
    assert_eq!(selected.map(|entry| entry.id), Some("alternate".into()));
    let retry_ticket = crate::router::lock(&ctx.shared).launcher_save_seq;
    crate::launchers::finish_save(&ctx, &app, ticket, None);
    assert!(
        ov.get_launcher_saving(),
        "stale completion cannot close retry"
    );
    crate::launchers::finish_save(&ctx, &app, retry_ticket, None);
    CLOCK.with(|clock| clock.set(clock.get() + 300));
    slint::platform::update_timers_and_animations();
    assert!(
        !ov.get_launcher_saving_visible(),
        "fast completion retires delayed cue"
    );
    assert!(!ov.get_list_open());
}

#[test]
fn token_empty_retry_replaces_alert_and_cancel_drains_queue() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, _) = boot();
    let (_runtime, ctx) = offline_ctx();
    app.global::<crate::Motion>().set_enabled(false);
    crate::card_write::begin(&ctx, &app, String::new());
    let ov = app.global::<crate::Overlays>();
    assert!(!ov.get_card_write_open());
    assert!(ov.get_dialog_open());
    assert_eq!(ov.get_dialog_detail(), "card_write");
    assert_eq!(ov.get_dialog_buttons().row_data(0), Some("retry".into()));
    let first = ov.get_card_write_key();
    crate::router::handle_action(&ctx, &app, "accept");
    assert_ne!(ov.get_card_write_key(), first);
    assert!(
        ov.get_dialog_open(),
        "synchronous retry failure must not disappear through deduplication"
    );
    crate::router::report_action_error(&ctx, &app, "setting", "");
    crate::router::handle_action(&ctx, &app, "accept");
    assert_eq!(
        ov.get_dialog_detail(),
        "setting",
        "retry must not replace a queued alert"
    );
    crate::router::handle_action(&ctx, &app, "cancel");
    assert_eq!(
        ov.get_dialog_detail(),
        "card_write",
        "retry failure must survive behind the other alert"
    );
    crate::router::handle_action(&ctx, &app, "cancel");
    assert!(!ov.get_dialog_open());
    assert!(crate::router::lock(&ctx.shared).errors.showing().is_none());
}

#[test]
fn token_cancel_dispatches_once_and_feedback_cannot_cancel_a_reopened_write() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    crate::sizing::apply_scene(
        &app,
        crate::sizing::Scene::of(&app, f64::from(W), f64::from(H), false),
    );
    let ov = app.global::<crate::Overlays>();
    ov.set_card_write_key("1".into());
    ov.set_card_write_open(true);
    settle(&window);
    app.window().request_redraw();
    let resting = frame(&window);
    let commits = Rc::new(Cell::new(0));
    arm_feedback(&app, &commits);
    frame(&window);
    distinct_frames(&window, 2);
    app.window().request_redraw();
    assert_ne!(resting, frame(&window));
    assert_eq!(
        commits.get(),
        0,
        "the Cancel button must finish pushing before the write closes"
    );
    distinct_frames(&window, 4);
    assert_eq!(commits.get(), 1);
    arm_feedback(&app, &commits);
    ov.set_card_write_key("2".into());
    settle(&window);
    assert_eq!(
        commits.get(),
        1,
        "a reopened write must not receive the previous write's Cancel"
    );
    arm_feedback(&app, &commits);
    crate::press_feedback::cancel(&app);
    ov.set_card_write_open(false);
    settle(&window);
    assert_eq!(commits.get(), 1);
}

#[test]
fn dialog_pushes_before_dispatch_and_feedback_settles() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    crate::sizing::apply_scene(
        &app,
        crate::sizing::Scene::of(&app, f64::from(W), f64::from(H), false),
    );
    crate::router::open_quit_confirm(&app);
    settle(&window);
    app.window().request_redraw();
    let resting = frame(&window);
    let commits = Rc::new(Cell::new(0));
    arm_feedback(&app, &commits);
    frame(&window);
    distinct_frames(&window, 2);
    app.window().request_redraw();
    assert_ne!(
        resting,
        frame(&window),
        "dialog button must visibly depress"
    );
    assert_eq!(commits.get(), 0, "dialog must remain through the push");
    distinct_frames(&window, 4);
    assert_eq!(commits.get(), 1);
    assert!(!crate::press_feedback::pending(&app));
    settle(&window);
    app.window().request_redraw();
    assert_eq!(resting, frame(&window));
}

#[test]
fn feedback_completion_never_dispatches_to_a_later_target() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    crate::router::open_quit_confirm(&app);
    settle(&window);
    let commits = Rc::new(Cell::new(0));
    arm_feedback(&app, &commits);
    app.global::<crate::Overlays>().set_dialog_focus(1);
    settle(&window);
    assert_eq!(commits.get(), 0, "a later Yes must not inherit No's accept");
    arm_feedback(&app, &commits);
    crate::press_feedback::cancel(&app);
    settle(&window);
    assert_eq!(
        commits.get(),
        0,
        "canceling the push must discard its command"
    );
    assert!(!crate::press_feedback::pending(&app));
    app.global::<crate::Motion>().set_enabled(false);
    arm_feedback(&app, &commits);
    assert_eq!(commits.get(), 1, "Reduce motion dispatches synchronously");
    assert!(!crate::press_feedback::pending(&app));
}

#[test]
fn letter_and_log_buttons_paint_their_pending_press() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    crate::sizing::apply_scene(
        &app,
        crate::sizing::Scene::of(&app, f64::from(W), f64::from(H), false),
    );
    let ov = app.global::<crate::Overlays>();
    ov.set_letter_buckets(ModelRc::new(VecModel::from(vec![crate::LetterBucket {
        label: "A".into(),
        count: 2,
    }])));
    ov.set_letter_open(true);
    let commits = Rc::new(Cell::new(0));
    for owner in ["letter", "log"] {
        if owner == "log" {
            ov.set_letter_open(false);
            let log = app.global::<crate::LogUploadView>();
            log.set_open(true);
            log.set_phase("failed".into());
        }
        settle(&window);
        app.window().request_redraw();
        let resting = frame(&window);
        arm_feedback(&app, &commits);
        frame(&window);
        distinct_frames(&window, 2);
        app.window().request_redraw();
        assert_eq!(
            app.global::<crate::PressFeedback>().get_owner().as_str(),
            owner
        );
        assert_ne!(resting, frame(&window), "{owner} button must push down");
        settle(&window);
    }
    assert_eq!(commits.get(), 2);
}

#[test]
fn settings_category_and_picker_feedback_is_local_and_settles() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    crate::sizing::apply_scene(
        &app,
        crate::sizing::Scene::of(&app, f64::from(W), f64::from(H), false),
    );
    app.global::<Shell>().set_active_screen("settings".into());
    let settings = app.global::<crate::SettingsView>();
    settings.set_cells(cells(1, "Settings"));
    settings.set_cell_width(60.0);
    settings.set_cell_height(50.0);
    settings.set_grid_y(40.0);
    settings.set_grid_height(80.0);
    settings.set_rows(ModelRc::new(VecModel::from(vec![crate::SettingsRow {
        kind: "field".into(),
        control: "navigate".into(),
        id: "pageDisplayInterface".into(),
        enabled: true,
        ..Default::default()
    }])));
    let commits = Rc::new(Cell::new(0));
    for owner in ["settings", "list"] {
        if owner == "list" {
            let ov = app.global::<crate::Overlays>();
            ov.set_list_entries(ModelRc::new(VecModel::from(vec![crate::MenuEntry {
                id: "one".into(),
                label: "One".into(),
                ..Default::default()
            }])));
            ov.set_list_open(true);
        }
        settle(&window);
        app.window().request_redraw();
        let resting = frame(&window);
        arm_feedback(&app, &commits);
        frame(&window);
        distinct_frames(&window, 2);
        app.window().request_redraw();
        assert_eq!(
            app.global::<crate::PressFeedback>().get_owner().as_str(),
            owner
        );
        assert_ne!(
            resting,
            frame(&window),
            "{owner} must show local feedback before dispatch"
        );
        settle(&window);
        app.window().request_redraw();
        assert_eq!(
            frame(&window),
            resting,
            "{owner} feedback must fully release"
        );
    }
    assert_eq!(commits.get(), 2);
}

#[test]
#[allow(
    clippy::float_cmp,
    reason = "blocked or clamped scrolling must leave the stored value exactly unchanged"
)]
fn game_info_scrolls_long_content_but_not_short_or_loading_content() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    crate::sizing::apply_scene(
        &app,
        crate::sizing::Scene::of(&app, f64::from(W), f64::from(H), false),
    );
    app.global::<Shell>().set_active_screen("".into());
    let info = app.global::<crate::GameInfoView>();
    info.set_modal_open(true);
    info.set_modal_name("Details".into());
    info.set_rows(ModelRc::new(VecModel::from(vec![crate::DetailRow {
        key: "system".into(),
        value: "SNES".into(),
    }])));
    settle(&window);
    app.invoke_game_info_scroll("down".into());
    assert_eq!(info.get_scroll_position(), 0.0);
    info.set_modal_description(
        "Long description with enough words to fill several viewports. "
            .repeat(80)
            .into(),
    );
    settle(&window);
    app.window().request_redraw();
    let top = frame(&window);
    app.invoke_game_info_scroll("page_next".into());
    assert!(info.get_scroll_position() > 0.0);
    assert_ne!(top, frame(&window), "paging must move rendered content");
    for _ in 0..100 {
        app.invoke_game_info_scroll("page_next".into());
    }
    let bottom = info.get_scroll_position();
    app.invoke_game_info_scroll("down".into());
    assert_eq!(
        info.get_scroll_position(),
        bottom,
        "scroll must clamp at content end"
    );
    info.set_scroll_position(0.0);
    info.set_loading(true);
    app.invoke_game_info_scroll("down".into());
    assert_eq!(
        info.get_scroll_position(),
        0.0,
        "loading content must not scroll"
    );
}

#[test]
fn about_seeds_scroll_before_paint_and_scrolls_with_clamped_geometry() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    let (_runtime, ctx) = offline_ctx();
    crate::router::lock(&ctx.shared).persist.about_scroll_milli = 713;
    crate::about::bind(&std::sync::Arc::new(ctx), &app);
    let view = app.global::<crate::AboutView>();
    assert_eq!(
        view.get_scroll_milli(),
        713,
        "cold state must be seeded before first paint"
    );
    assert_eq!(view.get_commit(), zaparoo_build_info::COMMIT);
    assert_eq!(view.get_build_date(), zaparoo_build_info::BUILD_DATE);
    // Render/input probe replaces disk publication; persistence serialization
    // is covered by zaparoo-core's backward-compatible round-trip test.
    let weak = app.as_weak();
    view.on_scroll_requested(move |value| {
        if let Some(app) = weak.upgrade() {
            app.global::<crate::AboutView>().set_scroll_milli(value);
        }
    });
    crate::sizing::apply_scene(
        &app,
        crate::sizing::Scene::of(&app, f64::from(W), f64::from(H), false),
    );
    app.global::<Shell>().set_active_screen("about".into());
    view.set_scroll_milli(0);
    settle(&window);
    assert!(view.get_maximum_scroll_milli() > 0);
    app.window().request_redraw();
    let top = frame(&window);
    view.invoke_move(80);
    assert!(view.get_scroll_milli() > 0);
    assert_ne!(top, frame(&window));
    view.invoke_move(1_000_000);
    assert_eq!(view.get_scroll_milli(), 1000);
    view.set_scroll_milli(1_000_000);
    view.invoke_move(-80);
    assert!(
        view.get_scroll_milli() < 1000,
        "smaller viewport must not trap a restored oversized offset"
    );
}

#[test]
fn favorite_count_labels_use_real_plural_forms() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, _) = boot();
    let labels = app.global::<crate::Labels>();
    assert_eq!(labels.invoke_favorite_systems(1), "1 system with favorites");
    assert_eq!(
        labels.invoke_favorite_systems(2),
        "2 systems with favorites"
    );
    assert_eq!(labels.invoke_favorites(0), "0 favorites");
    assert_eq!(labels.invoke_favorites(1), "1 favorite");
    assert_eq!(labels.invoke_favorites(2), "2 favorites");
}

#[test]
fn picker_keeps_first_row_until_focus_leaves_viewport() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    crate::sizing::apply_scene(
        &app,
        crate::sizing::Scene::of(&app, f64::from(W), f64::from(H), false),
    );
    app.global::<Shell>().set_active_screen("".into());
    app.global::<crate::Theme>()
        .set_text_primary(slint::Color::from_rgb_u8(0, 255, 0));
    let ov = app.global::<crate::Overlays>();
    ov.set_list_entries(ModelRc::new(VecModel::from(
        (0..20)
            .map(|i| crate::MenuEntry {
                id: i.to_string().into(),
                label: if i == 0 { "Anchor".into() } else { "".into() },
                label_key: "".into(),
            })
            .collect::<Vec<_>>(),
    )));
    ov.set_list_open(true);
    for (index, visible) in [(1, true), (6, true), (19, false), (1, false)] {
        ov.set_list_index(index);
        settle(&window);
        app.window().request_redraw();
        let mut pixels = vec![Rgb565Pixel(0); (W * H) as usize];
        assert!(window.draw_if_needed(|renderer| {
            renderer.render(&mut pixels, W as usize);
        }));
        let ink = pixels[(35 * W) as usize..((H - 25) * W) as usize]
            .iter()
            .any(|pixel| {
                let red = (pixel.0 >> 11) & 31;
                let green = (pixel.0 >> 5) & 63;
                let blue = pixel.0 & 31;
                green > 20 && green > red * 2 && green > blue * 2
            });
        assert_eq!(ink, visible, "anchor visibility at selection {index}");
    }
}

#[test]
fn dialog_shells_respect_content_and_notice_caps() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    crate::sizing::apply_scene(
        &app,
        crate::sizing::Scene::of(&app, f64::from(W), f64::from(H), false),
    );
    app.global::<Shell>().set_active_screen("".into());
    app.global::<crate::Theme>()
        .set_bg_panel(slint::Color::from_rgb_u8(255, 0, 255));
    let ov = app.global::<crate::Overlays>();
    crate::router::open_quit_confirm(&app);
    let mut widths = Vec::new();
    for kind in ["quit_confirm", "action_error", "notice"] {
        ov.set_dialog_kind(kind.into());
        if kind == "action_error" {
            ov.set_dialog_kind("action_error".into());
            ov.set_dialog_detail("launch".into());
            ov.set_dialog_arg("A very long game name for a content-sized error".into());
        }
        settle(&window);
        app.window().request_redraw();
        let mut pixels = vec![Rgb565Pixel(0); (W * H) as usize];
        assert!(window.draw_if_needed(|renderer| {
            renderer.render(&mut pixels, W as usize);
        }));
        let (mut left, mut right) = (W, 0);
        for (i, pixel) in pixels.iter().enumerate() {
            if pixel.0 == 0xf81f {
                let x = i as u32 % W;
                left = left.min(x);
                right = right.max(x);
            }
        }
        assert!(right > left);
        widths.push(right - left + 1);
    }
    assert!(
        widths[0] <= widths[1],
        "confirmation must not exceed the longer error: {widths:?}"
    );
    assert!(
        widths[2] > widths[1],
        "notice must retain its wider shell: {widths:?}"
    );
}

#[test]
fn one_button_alert_sizes_action_to_its_label() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    crate::sizing::apply_scene(
        &app,
        crate::sizing::Scene::of(&app, f64::from(W), f64::from(H), false),
    );
    app.global::<Shell>().set_active_screen("".into());
    let theme = app.global::<crate::Theme>();
    theme.set_bg_panel(slint::Color::from_rgb_u8(255, 0, 255));
    theme.set_surface_card(slint::Color::from_rgb_u8(0, 255, 255));
    let ov = app.global::<crate::Overlays>();
    ov.set_dialog_kind("action_error".into());
    ov.set_dialog_detail("launch".into());
    ov.set_dialog_arg("Sonic the Hedgehog".into());
    ov.set_dialog_buttons(ModelRc::new(VecModel::from(vec!["ok".into()])));
    ov.set_dialog_open(true);
    settle(&window);
    app.window().request_redraw();
    let mut pixels = vec![Rgb565Pixel(0); (W * H) as usize];
    assert!(window.draw_if_needed(|renderer| {
        renderer.render(&mut pixels, W as usize);
    }));

    let bounds = |color| {
        let (mut left, mut right) = (W, 0);
        for (i, pixel) in pixels.iter().enumerate() {
            if pixel.0 == color {
                let x = i as u32 % W;
                left = left.min(x);
                right = right.max(x);
            }
        }
        assert!(right > left, "expected color {color:#06x} to paint");
        right - left + 1
    };
    let panel_width = bounds(0xf81f);
    let button_width = bounds(0x07ff);
    assert!(
        button_width < panel_width / 2,
        "single OK action must size around its label: button={button_width}, panel={panel_width}"
    );
}

#[test]
fn qr_shell_preserves_square_modules_and_documentation_url() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    crate::fonts::register_embedded_fonts();
    crate::sizing::apply_scene(
        &app,
        crate::sizing::Scene::of(&app, f64::from(W), f64::from(H), false),
    );
    app.global::<Shell>().set_active_screen("".into());
    let theme = app.global::<crate::Theme>();
    theme.set_text_primary(slint::Color::from_rgb_u8(0, 255, 0));
    theme.set_text_label(slint::Color::from_rgb_u8(255, 0, 255));
    crate::router::open_documentation_qr(&app);
    let overlays = app.global::<crate::Overlays>();
    assert!(overlays.get_qr_documentation());
    let modules = u32::try_from(overlays.get_qr_modules()).unwrap_or_default();
    assert!(modules > 0);
    for docs in [true, false] {
        overlays.set_qr_documentation(docs);
        settle(&window);
        app.window().request_redraw();
        let mut pixels = vec![Rgb565Pixel(0); (W * H) as usize];
        assert!(window.draw_if_needed(|renderer| {
            renderer.render(&mut pixels, W as usize);
        }));
        let (mut left, mut top, mut right, mut bottom) = (W, H, 0, 0);
        let mut url_ink = 0;
        for y in 35..H - 25 {
            for x in 0..W {
                let pixel = pixels[(y * W + x) as usize].0;
                if pixel == 0xffff {
                    left = left.min(x);
                    top = top.min(y);
                    right = right.max(x);
                    bottom = bottom.max(y);
                }
                let red = (pixel >> 11) & 31;
                let green = (pixel >> 5) & 63;
                let blue = pixel & 31;
                if red > 10 && blue > 10 && red > green && blue > green {
                    url_ink += 1;
                }
            }
        }
        assert!(right > left && bottom > top, "QR quiet zone must paint");
        assert_eq!(right - left, bottom - top, "matrix must remain square");
        assert_eq!(
            (right - left + 1) % modules,
            0,
            "module scale must be integral"
        );
        assert_eq!(url_ink > 0, docs, "only documentation shows a readable URL");
    }
}

#[test]
fn game_info_short_modal_keeps_close_help_at_desktop_size() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    crate::fonts::register_embedded_fonts();
    crate::theme::apply_palette(&app, "", "");
    let (width, height) = (960, 540);
    window.set_size(slint::PhysicalSize::new(width, height));
    crate::sizing::apply_scene(
        &app,
        crate::sizing::Scene::of(&app, f64::from(width), f64::from(height), false),
    );
    app.global::<Shell>().set_active_screen("".into());
    app.global::<crate::Theme>()
        .set_text_primary(slint::Color::from_rgb_u8(0, 255, 0));
    let info = app.global::<crate::GameInfoView>();
    info.set_modal_open(true);
    info.set_modal_name("Chrono Trigger".into());
    info.set_rows(ModelRc::new(VecModel::from(vec![
        crate::DetailRow {
            key: "system".into(),
            value: "Super Nintendo Entertainment System".into(),
        },
        crate::DetailRow {
            key: "year".into(),
            value: "1995".into(),
        },
    ])));
    let mut pixels = vec![Rgb565Pixel(0); (width * height) as usize];
    for _ in 0..SETTLE_TICKS {
        CLOCK.with(|clock| clock.set(clock.get() + TICK_MS));
        slint::platform::update_timers_and_animations();
        window.draw_if_needed(|renderer| {
            renderer.render(&mut pixels, width as usize);
        });
    }
    let help_ink = pixels[(width * (height - 30)) as usize..]
        .iter()
        .filter(|pixel| {
            let red = (pixel.0 >> 11) & 31;
            let green = (pixel.0 >> 5) & 63;
            let blue = pixel.0 & 31;
            green > 20 && green > red * 2 && green > blue * 2
        })
        .count();
    assert!(
        help_ink > 20,
        "short-modal layout must not invalidate Close help delegates"
    );
}

#[test]
fn game_info_metadata_labels_paint_left_of_values() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    crate::sizing::apply_scene(
        &app,
        crate::sizing::Scene::of(&app, f64::from(W), f64::from(H), false),
    );
    app.global::<Shell>().set_active_screen("".into());
    let theme = app.global::<crate::Theme>();
    theme.set_text_label(slint::Color::from_rgb_u8(255, 0, 255));
    theme.set_text_primary(slint::Color::from_rgb_u8(0, 255, 0));
    let info = app.global::<crate::GameInfoView>();
    info.set_modal_open(true);
    info.set_modal_name("Details".into());
    info.set_rows(ModelRc::new(VecModel::from(vec![
        crate::DetailRow {
            key: "system".into(),
            value: "Super Nintendo".into(),
        },
        crate::DetailRow {
            key: "year".into(),
            value: "1994".into(),
        },
    ])));
    settle(&window);
    app.window().request_redraw();
    let mut pixels = vec![Rgb565Pixel(0); (W * H) as usize];
    assert!(window.draw_if_needed(|renderer| {
        renderer.render(&mut pixels, W as usize);
    }));
    let mut label_x = W;
    let mut value_x = W;
    for y in H / 2..H * 3 / 4 {
        for x in 0..W {
            let pixel = pixels[(y * W + x) as usize].0;
            let red = (pixel >> 11) & 31;
            let green = (pixel >> 5) & 63;
            let blue = pixel & 31;
            // Include antialiased ink; small fonts need not contain a
            // fully opaque pixel of the requested foreground color.
            if red > 10 && blue > 10 && red > green && blue > green {
                label_x = label_x.min(x);
            }
            if green > 20 && green > red * 2 && green > blue * 2 {
                value_x = value_x.min(x);
            }
        }
    }
    assert!(
        label_x < value_x && value_x < W,
        "labels ({label_x}) must precede values ({value_x}), not overlap them"
    );
}

#[test]
fn picker_selected_text_uses_on_accent_and_palette_previews_paint() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    crate::theme::apply_palette(&app, "zaparoo-dark", "normal");
    app.global::<Shell>().set_active_screen("".into());
    let ov = app.global::<crate::Overlays>();
    ov.set_list_entries(ModelRc::new(VecModel::from(vec![crate::MenuEntry {
        id: "zaparoo-dark".into(),
        label: "Selected label".into(),
        label_key: "".into(),
    }])));
    ov.set_list_open(true);
    ov.set_list_index(0);
    settle(&window);
    app.window().request_redraw();
    let before = frame(&window);
    app.global::<crate::Theme>()
        .set_on_accent(slint::Color::from_rgb_u8(255, 0, 255));
    let after = frame(&window);
    assert!(before.is_some() && after.is_some());
    assert_ne!(
        before, after,
        "selected picker text must respond to on-accent"
    );

    ov.set_list_setting_id("colorScheme".into());
    let preview = frame(&window);
    app.global::<crate::Theme>()
        .on_preview_color(|_, _| slint::Color::from_rgb_u8(0, 255, 0));
    app.window().request_redraw();
    let changed = frame(&window);
    assert!(preview.is_some() && changed.is_some());
    assert_ne!(
        preview, changed,
        "palette swatches must paint authored preview colors"
    );
}

#[test]
fn mouse_setting_blocks_picker_clicks_but_not_enabled_selection() {
    use slint::platform::{PointerEventButton, WindowEvent};
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    app.global::<Shell>().set_active_screen("".into());
    let ov = app.global::<crate::Overlays>();
    ov.set_list_entries(ModelRc::new(VecModel::from(vec![crate::MenuEntry {
        id: "one".into(),
        label: "One".into(),
        label_key: "".into(),
    }])));
    ov.set_list_open(true);
    app.global::<crate::Theme>()
        .set_selection_fill(slint::Color::from_rgb_u8(255, 0, 255));
    settle(&window);
    let rendered = pixels(&window);
    let mut bounds = (W, H, 0, 0);
    for (index, pixel) in rendered.iter().enumerate() {
        if pixel.0 == 0xf81f {
            let x = index as u32 % W;
            let y = index as u32 / W;
            bounds = (
                bounds.0.min(x),
                bounds.1.min(y),
                bounds.2.max(x),
                bounds.3.max(y),
            );
        }
    }
    assert!(bounds.2 > bounds.0 && bounds.3 > bounds.1);
    let position = slint::LogicalPosition::new(
        (bounds.0 + bounds.2) as f32 / 2.0,
        (bounds.1 + bounds.3) as f32 / 2.0,
    );
    let clicks = Rc::new(Cell::new(0));
    let observed = clicks.clone();
    ov.on_pointer_choice(move |kind, _, accept| {
        if kind == "list" && accept {
            observed.set(observed.get() + 1);
        }
    });
    for enabled in [false, true] {
        app.global::<Shell>().set_mouse_enabled(enabled);
        frame(&window);
        app.window()
            .dispatch_event(WindowEvent::PointerMoved { position });
        app.window().dispatch_event(WindowEvent::PointerPressed {
            position,
            button: PointerEventButton::Left,
        });
        app.window().dispatch_event(WindowEvent::PointerReleased {
            position,
            button: PointerEventButton::Left,
        });
        assert_eq!(clicks.get(), i32::from(enabled));
    }
}

#[test]
fn letter_columns_follow_each_windows_geometry() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, _) = boot();
    let ov = app.global::<crate::Overlays>();
    ov.set_letter_buckets(ModelRc::new(VecModel::from(
        vec![crate::LetterBucket::default(); 28],
    )));
    let landscape = ov.get_letter_columns();
    let sizing = app.global::<Sizing>();
    sizing.set_screen_width(180.0);
    sizing.set_screen_height(320.0);
    let portrait = ov.get_letter_columns();
    assert!(
        landscape > portrait,
        "letter packing must reflow, not keep nine columns"
    );
}

#[test]
fn confirm_defaults_to_no_on_the_left() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, _) = boot();
    app.global::<Shell>().set_active_screen("".into());
    crate::router::open_quit_confirm(&app);
    let ov = app.global::<crate::Overlays>();
    assert_eq!(ov.get_dialog_focus(), 0);
    assert_eq!(ov.get_dialog_buttons().row_data(0), Some("no".into()));
    assert_eq!(ov.get_dialog_buttons().row_data(1), Some("yes".into()));
}

fn advance(ms: u64) {
    CLOCK.with(|clock| clock.set(clock.get() + ms));
    slint::platform::update_timers_and_animations();
}

fn ring_left(image: &[Rgb565Pixel]) -> usize {
    (42..75)
        .flat_map(|y| (0..W as usize).map(move |x| (x, y)))
        .filter(|&(x, y)| image[y * W as usize + x].0 == 0xf81f)
        .map(|(x, _)| x)
        .min()
        .unwrap_or(W as usize)
}

#[test]
fn held_window_down_key_enters_rapid_mode_without_direct_repeat_dispatch() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    let (_runtime, ctx) = offline_ctx();
    let ctx = std::sync::Arc::new(ctx);
    {
        let mut shared = crate::router::lock(&ctx.shared);
        shared.games.rows = game_rows("Game", 1_000);
        shared.games.grid.set_item_count(1_000);
        shared.games.focus_armed = true;
        shared.games.restore_done = true;
        shared.input.advance_test_clock(1);
    }
    app.global::<Shell>().set_active_screen("games".into());
    crate::router::refresh_layout(&app);
    crate::games::render(&ctx, &app);
    crate::input::bind(&ctx, &app, std::collections::HashMap::new());
    settle(&window);
    let key: SharedString = char::from(slint::platform::Key::DownArrow)
        .to_string()
        .into();
    app.window()
        .dispatch_event(slint::platform::WindowEvent::KeyPressed { text: key.clone() });
    for tick in 0..20 {
        crate::router::lock(&ctx.shared)
            .input
            .advance_test_clock(100);
        advance(100);
        if tick >= 4 {
            // Slint 1.17.1's Winit bridge forwards native repeats as ordinary
            // KeyPressed events with the repeat flag left at its default.
            app.window()
                .dispatch_event(slint::platform::WindowEvent::KeyPressed { text: key.clone() });
        }
        pixels(&window);
    }
    assert!(
        crate::input::rapid_navigation(&ctx),
        "a held window key must qualify rapid mode"
    );
    assert!(app.global::<crate::GamesView>().get_rapid_active());
    assert!(!app
        .global::<crate::GamesView>()
        .get_rapid_letter()
        .is_empty());
    app.window()
        .dispatch_event(slint::platform::WindowEvent::KeyReleased { text: key });
    crate::router::lock(&ctx.shared)
        .input
        .advance_test_clock(300);
    advance(300);
    assert!(!crate::input::rapid_navigation(&ctx));
    let before = crate::router::lock(&ctx.shared).games.grid.current_index();
    let key: SharedString = char::from(slint::platform::Key::DownArrow)
        .to_string()
        .into();
    app.window()
        .dispatch_event(slint::platform::WindowEvent::KeyPressed { text: key.clone() });
    assert_ne!(
        crate::router::lock(&ctx.shared).games.grid.current_index(),
        before,
        "release permits the next physical press"
    );
    app.invoke_input_lost();
    crate::router::lock(&ctx.shared)
        .input
        .advance_test_clock(100);
    advance(100);
    let before = crate::router::lock(&ctx.shared).games.grid.current_index();
    app.window()
        .dispatch_event(slint::platform::WindowEvent::KeyPressed { text: key.clone() });
    assert_ne!(
        crate::router::lock(&ctx.shared).games.grid.current_index(),
        before,
        "focus loss must clear pressed-key tracking"
    );
    app.window()
        .dispatch_event(slint::platform::WindowEvent::KeyReleased { text: key });
}

#[test]
fn grid_focus_glides_retargets_and_snaps_on_page_or_reduced_motion() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    app.global::<crate::Theme>()
        .set_accent(slint::Color::from_rgb_u8(255, 0, 255));
    let hub = app.global::<HubView>();
    hub.set_cells(cells(3, "Tile"));
    hub.set_columns(3);
    hub.set_cell_width(50.0);
    hub.set_cell_height(40.0);
    hub.set_grid_y(40.0);
    hub.set_grid_height(100.0);
    hub.set_selected_local(0);
    settle(&window);
    let first = ring_left(&pixels(&window));
    assert!(first < W as usize, "fixture must contain the focus ring");
    hub.set_selected_local(1);
    pixels(&window);
    advance(24);
    let middle = ring_left(&pixels(&window));
    advance(80);
    let second = ring_left(&pixels(&window));
    assert!(
        first < middle && middle < second,
        "adjacent focus must glide: {first}, {middle}, {second}"
    );
    hub.set_selected_local(0);
    pixels(&window);
    advance(16);
    let reversing = ring_left(&pixels(&window));
    hub.set_selected_local(1);
    let retargeted = ring_left(&pixels(&window));
    assert_eq!(
        retargeted, reversing,
        "retarget from current visual position"
    );
    advance(100);
    assert_eq!(ring_left(&pixels(&window)), second);
    hub.set_page(1);
    hub.set_selected_local(0);
    assert_eq!(ring_left(&pixels(&window)), first, "page changes snap");
    hub.set_selected_local(1);
    pixels(&window);
    advance(16);
    app.global::<crate::Motion>().set_enabled(false);
    assert_eq!(
        ring_left(&pixels(&window)),
        second,
        "live reduced motion snaps immediately"
    );
    advance(500);
    assert_eq!(ring_left(&pixels(&window)), second);
}

#[test]
fn browse_list_focus_and_scroll_are_local_and_restore_the_saved_viewport() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    let (_runtime, ctx) = offline_ctx();
    {
        let mut shared = crate::router::lock(&ctx.shared);
        shared.persist.settings.games_browse_layout = "list".into();
        shared.persist.games.list_top_at_level = vec![0];
        shared.games.rows = game_rows("Game", 40);
        shared.games.grid.set_item_count(40);
        shared.games.focus_armed = true;
    }
    app.global::<Shell>().set_active_screen("games".into());
    app.global::<Shell>().set_browse_list_layout(true);
    crate::router::refresh_layout(&app);
    crate::games::render(&ctx, &app);
    settle(&window);
    crate::games::handle_action(&ctx, &app, "down");
    let immediate = pixels(&window);
    advance(32);
    let middle = pixels(&window);
    advance(100);
    let final_frame = pixels(&window);
    assert_ne!(
        immediate, middle,
        "list ring must move, not just switch rows"
    );
    assert_ne!(middle, final_frame);
    let view = app.global::<crate::GamesView>();
    assert_eq!(
        view.get_list_scroll_top(),
        0,
        "do not recenter visible focus"
    );
    let visible = view.get_list_visible();
    for _ in 1..visible {
        crate::games::handle_action(&ctx, &app, "down");
    }
    assert_eq!(
        view.get_list_scroll_top(),
        1,
        "scroll only one row past the edge"
    );
    assert!(view.get_list_rows().row_count() <= visible as usize + 2);
    let edge = pixels(&window);
    advance(100);
    assert_eq!(
        pixels(&window),
        edge,
        "offscreen focus is revealed immediately, not after interpolation"
    );
    settle(&window);
    let saved = crate::router::lock(&ctx.shared)
        .persist
        .games
        .list_top_at_level
        .clone();
    assert_eq!(saved, vec![1]);
    view.set_list_scroll_top(0);
    crate::games::render(&ctx, &app);
    assert_eq!(
        view.get_list_scroll_top(),
        1,
        "viewport comes from saved state, not incidental widget state"
    );
}

#[test]
fn pending_folder_cancel_and_stale_reply_preserve_grid_and_list_sources() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    let (_runtime, ctx) = offline_ctx();
    for list in [false, true] {
        {
            let mut shared = crate::router::lock(&ctx.shared);
            shared.persist.active_screen = "games".into();
            shared.persist.settings.games_browse_layout = if list { "list" } else { "grid" }.into();
            shared.persist.games.path_stack = vec!["/parent".into()];
            shared.persist.games.selected_at_level = vec!["/parent/child".into()];
            shared.games.rows = game_rows("Parent", 8);
            shared.games.rows[0].entry_type = zaparoo_app::media_list::EntryType::Directory;
            shared.games.rows[0].path = "/parent/child".into();
            shared.games.grid.set_item_count(8);
            shared.games.grid.set_current_index_immediate(0);
            shared.games.focus_armed = true;
            shared.games.restore_done = true;
            shared.games.browse_path = "/parent".into();
        }
        app.global::<Shell>().set_active_screen("games".into());
        app.global::<Shell>().set_browse_list_layout(list);
        crate::router::refresh_layout(&app);
        crate::games::render(&ctx, &app);
        settle(&window);
        let title = app.global::<crate::GamesView>().get_title();
        let source = pixels(&window);
        crate::router::handle_action(&ctx, &app, "accept");
        if !list {
            assert!(crate::press_feedback::pending(&app));
            pixels(&window);
            advance(crate::press_feedback::PUSH_MS);
        }
        assert!(app.global::<Shell>().get_transitioning());
        let ticket = crate::router::lock(&ctx.shared).games.ticket;
        assert_eq!(app.global::<crate::GamesView>().get_title(), title);
        let saved = zaparoo_core::persist::load();
        assert_eq!(
            saved.games.path_stack,
            vec!["/parent"],
            "pending state must not replace durable source"
        );
        crate::router::handle_action(&ctx, &app, "accept");
        assert_eq!(
            crate::router::lock(&ctx.shared).games.ticket,
            ticket,
            "duplicate Accept is gated"
        );
        crate::router::handle_action(&ctx, &app, "cancel");
        assert!(!app.global::<Shell>().get_transitioning());
        settle(&window);
        assert_region_matches(
            &pixels(&window),
            &source,
            0..W as usize,
            55..H as usize,
            "Cancel restores source",
        );
        crate::games::apply_fill(&ctx, &app, ticket, game_rows("Stale", 4), None, None, false);
        assert_eq!(app.global::<crate::GamesView>().get_title(), title);
        assert_eq!(
            crate::router::lock(&ctx.shared).games.rows[0].name,
            "Parent 0"
        );
        crate::router::handle_action(&ctx, &app, "accept");
        if !list {
            pixels(&window);
            advance(crate::press_feedback::PUSH_MS);
        }
        let ticket = crate::router::lock(&ctx.shared).games.ticket;
        crate::games::apply_fill(
            &ctx,
            &app,
            ticket,
            game_rows("Child", 4),
            None,
            Some((4, 0)),
            false,
        );
        assert!(!app.global::<Shell>().get_transitioning());
        assert_eq!(
            crate::router::lock(&ctx.shared).games.rows[0].name,
            "Child 0"
        );
        assert_eq!(
            zaparoo_core::persist::load().games.path_stack,
            vec!["/parent", "/parent/child"]
        );
        settle(&window);
    }
}

#[test]
fn restored_list_waits_for_selection_and_its_saved_viewport() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    let (_runtime, ctx) = offline_ctx();
    settle(&window);
    let source = pixels(&window);
    crate::navigation::stage(&ctx, &app);
    crate::router::begin_pending(&app, "games");
    let mut rows = game_rows("Destination", 40);
    for (index, row) in rows.iter_mut().enumerate() {
        row.path = format!("/target/{index}");
    }
    let ticket = {
        let mut shared = crate::router::lock(&ctx.shared);
        shared.persist.settings.games_browse_layout = "list".into();
        shared.persist.games.path_stack = vec!["/target".into()];
        shared.persist.games.selected_at_level = vec![rows[8].path.clone()];
        shared.persist.games.list_top_at_level = vec![6];
        shared.games.ticket
    };
    crate::games::apply_fill(
        &ctx,
        &app,
        ticket,
        rows[..4].to_vec(),
        Some("next".into()),
        Some((40, 0)),
        true,
    );
    assert!(app.global::<Shell>().get_transitioning());
    assert_eq!(app.global::<Shell>().get_active_screen(), "hub");
    assert_region_matches(
        &pixels(&window),
        &source,
        0..W as usize,
        55..H as usize,
        "restore walk keeps source",
    );
    crate::games::on_append(
        &ctx,
        &app,
        ticket,
        Ok((rows[4..10].to_vec(), Some("last".into()))),
    );
    assert!(
        app.global::<Shell>().get_transitioning(),
        "finding focus alone must not truncate its saved viewport"
    );
    crate::games::on_append(&ctx, &app, ticket, Ok((rows[10..].to_vec(), None)));
    assert!(!app.global::<Shell>().get_transitioning());
    assert_eq!(app.global::<Shell>().get_active_screen(), "games");
    assert_eq!(app.global::<crate::GamesView>().get_current_index(), 8);
    assert_eq!(app.global::<crate::GamesView>().get_list_scroll_top(), 6);
    assert_eq!(
        zaparoo_core::persist::load().games.list_top_at_level,
        vec![6]
    );
}

#[test]
fn cold_and_cached_favorite_systems_restore_the_same_list_viewport() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    let (_runtime, ctx) = offline_ctx();
    let result = zaparoo_core::media_types::SystemsResult {
        systems: (0..20)
            .map(|index| zaparoo_core::media_types::SystemInfo {
                id: format!("System{index:02}"),
                name: format!("System {index:02}"),
                media_count: Some(1),
                ..Default::default()
            })
            .collect(),
    };
    for cached in [false, true] {
        app.global::<Shell>().set_active_screen("hub".into());
        {
            let mut shared = crate::router::lock(&ctx.shared);
            shared.persist.active_screen = "hub".into();
            shared.persist.settings.systems_browse_layout = "list".into();
            shared.persist.favorite_systems.selected_path = "System08".into();
            shared.persist.favorite_systems.list_top = Some(6);
        }
        settle(&window);
        crate::systems::enter_favorites(&ctx, &app);
        if !cached {
            assert!(app.global::<Shell>().get_transitioning());
            assert_eq!(
                crate::router::lock(&ctx.shared)
                    .persist
                    .favorite_systems
                    .list_top,
                Some(6)
            );
            crate::systems::apply_favorites(&ctx, &app, &result, 1, true);
        }
        assert!(!app.global::<Shell>().get_transitioning());
        assert_eq!(
            app.global::<Shell>().get_active_screen(),
            "favorite-systems"
        );
        assert_eq!(app.global::<SystemsView>().get_current_index(), 8);
        assert_eq!(app.global::<SystemsView>().get_list_scroll_top(), 6);
        assert_eq!(
            zaparoo_core::persist::load().favorite_systems.list_top,
            Some(6)
        );
    }
}

#[test]
fn failed_or_timed_out_navigation_restores_source_and_rejects_late_pages() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    let (_runtime, ctx) = offline_ctx();
    crate::router::lock(&ctx.shared).persist.active_screen = "hub".into();
    for timeout in [false, true] {
        crate::navigation::stage(&ctx, &app);
        crate::router::begin_pending(&app, "games");
        let ticket = crate::router::lock(&ctx.shared).games.ticket;
        if timeout {
            advance(15_001);
        } else {
            crate::games::on_append(&ctx, &app, ticket, Err("offline".into()));
        }
        assert!(!app.global::<Shell>().get_transitioning());
        assert_eq!(app.global::<Shell>().get_active_screen(), "hub");
        assert_eq!(zaparoo_core::persist::load().active_screen, "hub");
        assert!(app.global::<crate::Overlays>().get_dialog_open());
        crate::games::on_append(&ctx, &app, ticket, Ok((game_rows("Late", 4), None)));
        assert!(crate::router::lock(&ctx.shared).games.rows.is_empty());
        crate::router::handle_action(&ctx, &app, "cancel");
        settle(&window);
    }
}

#[test]
fn hub_swap_moves_only_held_tile_and_local_neighbor_then_stops() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    let hub = app.global::<HubView>();
    hub.set_cells(cells(3, "Tile"));
    hub.set_columns(3);
    hub.set_cell_width(50.0);
    hub.set_cell_height(40.0);
    hub.set_grid_y(40.0);
    hub.set_grid_height(100.0);
    hub.set_selected_local(0);
    hub.set_held_local(0);
    settle(&window);
    let original = pixels(&window);
    let mut swapped = cells(3, "Tile").iter().collect::<Vec<_>>();
    swapped.swap(0, 1);
    hub.set_cells(ModelRc::new(VecModel::from(swapped)));
    hub.set_move_origins(ModelRc::new(VecModel::from(vec![1, 0, 2])));
    hub.set_selected_local(1);
    hub.set_held_local(1);
    hub.set_move_pulse(1);
    let initial = pixels(&window);
    advance(2);
    pixels(&window);
    advance(24);
    let middle = pixels(&window);
    advance(100);
    let endpoint = pixels(&window);
    assert_ne!(middle, initial, "local swap must move");
    assert_ne!(middle, endpoint);
    assert_region_matches(
        &middle,
        &original,
        220..W as usize,
        0..H as usize,
        "unrelated third tile stays still",
    );
    advance(2_000);
    assert_eq!(
        pixels(&window),
        endpoint,
        "held selection must not keep blinking"
    );
    app.global::<crate::Motion>().set_enabled(false);
    hub.set_selected_local(0);
    hub.set_held_local(0);
    hub.set_move_origins(ModelRc::new(VecModel::from(vec![1, 0, 2])));
    hub.set_move_pulse(2);
    let snapped = pixels(&window);
    advance(100);
    assert_eq!(
        pixels(&window),
        snapped,
        "reduced motion snaps the whole swap"
    );
}

fn save_push_evidence(name: &str, buffer: &[Rgb565Pixel]) {
    let Some(directory) = std::env::var_os("ZAPAROO_PRESS_EVIDENCE") else {
        return;
    };
    let directory = std::path::PathBuf::from(directory);
    assert!(std::fs::create_dir_all(&directory).is_ok());
    let image = image::RgbImage::from_fn(W, H, |x, y| {
        let word = buffer[(y * W + x) as usize].0;
        image::Rgb([
            u8::try_from((word >> 11) * 255 / 31).unwrap_or(0),
            u8::try_from(((word >> 5) & 63) * 255 / 63).unwrap_or(0),
            u8::try_from((word & 31) * 255 / 31).unwrap_or(0),
        ])
    });
    assert!(image.save(directory.join(format!("{name}.png"))).is_ok());
}

#[test]
#[allow(
    clippy::too_many_lines,
    reason = "one software window checks the physical face and edge across every tile host"
)]
fn grid_push_lowers_face_art_and_ring_before_ready_navigation() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    crate::sizing::apply_scene(
        &app,
        crate::sizing::Scene::of(&app, f64::from(W), f64::from(H), false),
    );
    let shell = app.global::<Shell>();
    shell.set_systems_list_layout(false);
    shell.set_browse_list_layout(false);
    for owner in ["hub", "systems", "games", "settings"] {
        shell.set_active_screen(owner.into());
        let mut art = slint::SharedPixelBuffer::<slint::Rgb8Pixel>::new(8, 8);
        for (i, pixel) in art.make_mut_slice().iter_mut().enumerate() {
            *pixel = slint::Rgb8Pixel {
                r: if i < 32 { 255 } else { 0 },
                g: if i % 8 < 4 { 255 } else { 0 },
                b: 160,
            };
        }
        let image = slint::Image::from_rgb8(art);
        let page = ModelRc::new(VecModel::from(vec![GridCell {
            name: "Push target".into(),
            cover: image.clone(),
            cover_focus: image,
            has_cover: true,
            has_cover_focus: true,
            ..Default::default()
        }]));
        match owner {
            "hub" => {
                let view = app.global::<HubView>();
                view.set_cells(page);
                view.set_cell_width(70.0);
                view.set_cell_height(60.0);
                view.set_grid_y(40.0);
                view.set_grid_height(100.0);
            }
            "systems" => {
                let view = app.global::<SystemsView>();
                view.set_cells(page);
                view.set_count(1);
                view.set_focus_ready(true);
                view.set_cell_width(70.0);
                view.set_cell_height(60.0);
                view.set_grid_y(40.0);
                view.set_grid_height(100.0);
            }
            "games" => {
                let view = app.global::<crate::GamesView>();
                view.set_cells(page);
                view.set_count(1);
                view.set_total_items(1);
                view.set_focus_ready(true);
                view.set_cell_width(70.0);
                view.set_cell_height(60.0);
                view.set_grid_y(40.0);
                view.set_grid_height(100.0);
            }
            _ => {
                let view = app.global::<crate::SettingsView>();
                view.set_cells(page);
                view.set_cell_width(70.0);
                view.set_cell_height(60.0);
                view.set_grid_y(40.0);
                view.set_grid_height(100.0);
                view.set_rows(ModelRc::new(VecModel::from(vec![crate::SettingsRow {
                    kind: "field".into(),
                    control: "navigate".into(),
                    id: "pageAppearance".into(),
                    enabled: true,
                    ..Default::default()
                }])));
            }
        }
        settle(&window);
        let raised = pixels(&window);
        save_push_evidence(&format!("{owner}-0-raised"), &raised);
        let target = crate::press_feedback::current(&app);
        assert!(target.is_some(), "missing {owner} target");
        let Some(target) = target else {
            return;
        };
        crate::press_feedback::dispatch(&app, &target, |app| {
            crate::router::transition_to_screen(app, "about", 1);
        });
        pixels(&window);
        advance(16);
        save_push_evidence(&format!("{owner}-1-downstroke"), &pixels(&window));
        advance(32);
        let depressed = pixels(&window);
        save_push_evidence(&format!("{owner}-2-depressed"), &depressed);
        assert_eq!(
            shell.get_active_screen().as_str(),
            owner,
            "ready navigation must not hide the push"
        );
        let sizing = app.global::<Sizing>();
        let layout = app.global::<crate::Layout>();
        let left = if owner == "hub" {
            sizing.get_hub_grid_side_inset()
        } else {
            layout.get_grid_left_inset()
        } as usize;
        let top = 40
            + if owner == "hub" {
                sizing.get_hub_grid_top_inset()
            } else {
                layout.get_grid_top_inset()
            } as usize;
        let depth = sizing.get_press_edge_height() as usize;
        assert!(depth > 0);
        let colors: std::collections::HashSet<_> = (top + 6..top + 50)
            .flat_map(|y| (left + 10..left + 60).map(move |x| y * W as usize + x))
            .map(|i| raised[i].0)
            .collect();
        assert!(
            colors.len() > 3,
            "{owner}: translation assertion needs real artwork, not a blank face"
        );
        for y in top..top + 60 - depth {
            for x in left + 10..left + 60 {
                assert_eq!(
                    depressed[(y + depth) * W as usize + x],
                    raised[y * W as usize + x],
                    "{owner}: face contents must move down by the physical edge depth at {x},{y}"
                );
            }
        }
        let edge = |buffer: &[Rgb565Pixel]| buffer[(top + 59) * W as usize + left + 35];
        assert_ne!(
            edge(&raised),
            edge(&depressed),
            "{owner}: the front edge must collapse"
        );
        assert_ne!(
            raised, depressed,
            "{owner}: unchanged pixels are not push feedback"
        );
        advance(32);
        assert_eq!(
            shell.get_active_screen().as_str(),
            owner,
            "fully depressed source must remain visible"
        );
        advance(16);
        assert_eq!(shell.get_active_screen(), "about");
        assert!(!crate::press_feedback::pending(&app));
        save_push_evidence(&format!("{owner}-3-destination"), &pixels(&window));
    }
}

#[test]
fn window_and_pointer_accept_paint_settings_push_before_opening_page() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    let (_runtime, ctx) = offline_ctx();
    crate::sizing::apply_scene(
        &app,
        crate::sizing::Scene::of(&app, f64::from(W), f64::from(H), false),
    );
    app.global::<Shell>().set_active_screen("settings".into());
    let input_ctx = std::sync::Arc::new(ctx.clone());
    crate::input::bind(&input_ctx, &app, std::collections::HashMap::new());
    crate::settings::bind_input(&input_ctx, &app);
    let view = app.global::<crate::SettingsView>();
    for pointer in [false, true] {
        crate::settings::open_page(&ctx, &app, "");
        view.set_index(0);
        settle(&window);
        let raised = pixels(&window);
        if pointer {
            app.global::<crate::SettingsInput>().invoke_cell_clicked(0);
        } else {
            let key = char::from(slint::platform::Key::Return).to_string();
            app.window()
                .dispatch_event(slint::platform::WindowEvent::KeyPressed {
                    text: key.clone().into(),
                });
            app.window()
                .dispatch_event(slint::platform::WindowEvent::KeyReleased { text: key.into() });
        }
        assert!(crate::press_feedback::pending(&app));
        assert!(view.get_page().is_empty());
        app.global::<crate::SettingsInput>().invoke_cell_hovered(1);
        assert_eq!(
            view.get_index(),
            0,
            "pointer hover cannot steal a pending Accept"
        );
        pixels(&window);
        advance(48);
        assert_ne!(
            raised,
            pixels(&window),
            "real input must paint the physical press"
        );
        assert!(view.get_page().is_empty());
        advance(48);
        assert_eq!(view.get_page(), "pageAppearance");
        crate::router::handle_action(&ctx, &app, "cancel");
        settle(&window);
        assert_eq!(
            raised,
            pixels(&window),
            "return must restore a fully raised tile"
        );
    }
    crate::router::handle_action(&ctx, &app, "accept");
    crate::router::handle_action(&ctx, &app, "accept");
    crate::router::handle_action(&ctx, &app, "cancel");
    settle(&window);
    assert!(
        view.get_page().is_empty(),
        "Back cancels the pending Accept, not the source screen"
    );
    assert_eq!(app.global::<Shell>().get_active_screen(), "settings");
    crate::router::handle_action(&ctx, &app, "accept");
    crate::router::handle_action(&ctx, &app, "right");
    settle(&window);
    assert!(
        view.get_page().is_empty(),
        "new selection retires the old Accept"
    );
    assert_eq!(view.get_index(), 1);
    crate::router::handle_action(&ctx, &app, "accept");
    app.invoke_input_lost();
    settle(&window);
    assert!(
        view.get_page().is_empty(),
        "losing input ownership cancels the pending push"
    );
    assert!(!crate::press_feedback::pending(&app));
    app.global::<crate::Motion>().set_enabled(false);
    crate::router::handle_action(&ctx, &app, "accept");
    assert_eq!(
        view.get_page(),
        "pageLibraryData",
        "reduced motion does not wait"
    );
}

#[test]
fn accepting_during_page_motion_pushes_the_logical_destination_tile() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    let (_runtime, ctx) = offline_ctx();
    crate::sizing::apply_scene(
        &app,
        crate::sizing::Scene::of(&app, f64::from(W), f64::from(H), false),
    );
    app.global::<Shell>().set_active_screen("games".into());
    {
        let mut shared = crate::router::lock(&ctx.shared);
        shared.games.rows = game_rows("Folder", 200);
        for (i, row) in shared.games.rows.iter_mut().enumerate() {
            row.entry_type = zaparoo_app::media_list::EntryType::Directory;
            row.path = format!("/folder-{i}");
        }
        shared.games.grid.set_item_count(200);
        shared.games.focus_armed = true;
    }
    crate::games::render(&ctx, &app);
    settle(&window);
    crate::router::handle_action(&ctx, &app, "page_next");
    let index = {
        let shared = crate::router::lock(&ctx.shared);
        assert!(shared.games.sliding);
        shared.games.grid.current_index()
    };
    crate::router::handle_action(&ctx, &app, "accept");
    assert!(!crate::router::lock(&ctx.shared).games.sliding);
    assert!(crate::press_feedback::pending(&app));
    let view = app.global::<crate::GamesView>();
    assert_eq!(
        view.get_cells()
            .row_data(view.get_selected_local() as usize)
            .map(|row| row.name.to_string()),
        Some(format!("Folder {index}")),
        "push must target the new logical page, not its outgoing strip"
    );
    pixels(&window);
    advance(48);
    pixels(&window);
    assert!(!app.global::<Shell>().get_transitioning());
    advance(48);
    assert_eq!(
        crate::router::lock(&ctx.shared)
            .persist
            .games
            .path_stack
            .last(),
        Some(&format!("/folder-{index}"))
    );
    assert!(app.global::<Shell>().get_transitioning());
    crate::router::handle_action(&ctx, &app, "cancel");
}

#[test]
fn ready_routes_commit_without_advancing_the_clock() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    settle(&window);
    let time = CLOCK.with(Cell::get);
    for (target, direction) in [("systems", 1), ("hub", -1)] {
        crate::router::transition_to_screen(&app, target, direction);
        assert_eq!(app.global::<Shell>().get_active_screen().as_str(), target);
        assert_eq!(CLOCK.with(Cell::get), time);
        let immediate = pixels(&window);
        // No paint or timer is required before the coherent destination exists.
        assert_eq!(immediate, pixels(&window));
    }
}

#[test]
fn pending_navigation_preserves_source_pixels_outside_the_status_slot() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    settle(&window);
    let source = pixels(&window);
    crate::router::begin_pending(&app, "systems");
    for ticks in [1, 6, 20, 100] {
        distinct_frames(&window, ticks);
        assert_region_matches(
            &pixels(&window),
            &source,
            0..W as usize,
            55..H as usize,
            "source body stays intact while waiting",
        );
        assert_eq!(app.global::<Shell>().get_active_screen(), "hub");
    }
    crate::router::transition_to_screen(&app, "systems", 1);
    let destination = pixels(&window);
    settle(&window);
    assert_eq!(
        destination,
        pixels(&window),
        "first destination frame is final composition"
    );
}

#[test]
fn a_fast_fill_commits_without_flashing_loading_text() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    settle(&window);

    crate::router::begin_pending(&app, "systems");
    distinct_frames(&window, 3);
    assert!(!app.global::<Shell>().get_transition_cue());
    crate::router::transition_to_screen(&app, "systems", 1);
    assert_eq!(app.global::<Shell>().get_active_screen(), "systems");
    distinct_frames(&window, SETTLE_TICKS);
    assert!(!app.global::<Shell>().get_transition_cue());
}

#[test]
fn a_slow_fill_keeps_source_and_adds_static_delayed_feedback() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    settle(&window);

    crate::router::begin_pending(&app, "systems");
    assert_eq!(app.global::<Shell>().get_transition_target(), "systems");
    distinct_frames(&window, 24);
    let shell = app.global::<Shell>();
    assert!(shell.get_transition_cue());
    assert_eq!(shell.get_active_screen().as_str(), "hub");
    let held = pixels(&window);
    assert_eq!(
        distinct_frames(&window, 8),
        0,
        "pending wait must be static"
    );
    assert_eq!(held, pixels(&window));

    crate::router::transition_to_screen(&app, "systems", 1);
    assert_eq!(shell.get_active_screen().as_str(), "systems");
    distinct_frames(&window, SETTLE_TICKS);
    assert!(!shell.get_transitioning());
    assert!(!shell.get_transition_cue());
}
