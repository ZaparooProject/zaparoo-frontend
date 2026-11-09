// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

//! Motion and navigation checked with software-rendered frames on a stepped
//! clock. Assert coherent source/destination composition, local cursor motion,
//! visible pushes before dispatch, command ownership, and eventual quiescence.

use crate::navigation::EntryMode;
use zaparoo_app::user_flags::Flag;

use crate::{App, GridCell, HubView, Shell, Sizing, SystemsView};
use crate::{
    ControlKind, DialogButton, DialogKind, ErrorKind, GamesMode, LogPhase, PairPhase, PressOwner,
    RowKind, Screen, SettingsPage, SystemsMode,
};
use slint::platform::software_renderer::{MinimalSoftwareWindow, RepaintBufferType, Rgb565Pixel};
use slint::platform::{Platform, WindowAdapter};
use slint::{ComponentHandle, Model, ModelRc, SharedString, VecModel};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::time::Duration;
use zaparoo_app::input::HoldTier;

thread_local! {
    static CLOCK: Cell<u64> = const { Cell::new(0) };
    static WINDOW: RefCell<Option<Rc<MinimalSoftwareWindow>>> = const { RefCell::new(None) };
    static REUSE_BUFFER: Cell<bool> = const { Cell::new(false) };
    static PIXELS: RefCell<Vec<Rgb565Pixel>> = const { RefCell::new(Vec::new()) };
}

/// A platform whose clock only moves when the test moves it, making each
/// animation the same number of frames on every machine.
pub(crate) struct ProbePlatform;

impl Platform for ProbePlatform {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, slint::PlatformError> {
        let mode = if REUSE_BUFFER.with(Cell::get) {
            RepaintBufferType::ReusedBuffer
        } else {
            RepaintBufferType::NewBuffer
        };
        let window = MinimalSoftwareWindow::new(mode);
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

fn render_pixels(window: &Rc<MinimalSoftwareWindow>) -> (bool, Vec<Rgb565Pixel>) {
    PIXELS.with(|pixels| {
        let mut buf = pixels.borrow_mut();
        buf.resize((W * H) as usize, Rgb565Pixel(0));
        let drew = window.draw_if_needed(|renderer| {
            renderer.render(&mut buf, W as usize);
        });
        (drew, buf.clone())
    })
}

/// Render one frame and fingerprint it.
fn frame(window: &Rc<MinimalSoftwareWindow>) -> Option<u64> {
    let (drew, buf) = render_pixels(window);
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
    window.request_redraw();
    let (drew, buf) = render_pixels(window);
    assert!(drew, "a requested redraw must produce a frame");
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
    crate::brand::register(&app);
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
    shell.set_active_screen(Screen::Hub);

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
    let commits = Rc::new(Cell::new(0));
    arm_feedback(&app, &commits);
    distinct_frames(&window, 4);
    app.window().request_redraw();
    let pressed = frame(&window);
    assert_ne!(resting, pressed, "the selected tile must push down");
    crate::press_feedback::cancel(&app);
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
    crate::press_feedback::dispatch(app, &target, move |app| {
        commits.set(commits.get() + 1);
        crate::press_feedback::keep_held(app);
    });
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
    app.global::<Shell>().set_active_screen(Screen::Systems);
    let bytes = include_bytes!("../assets/systems/SNES.png");
    let pixels = image::load_from_memory(bytes)
        .expect("embedded SNES logo")
        .to_rgba8();
    let image = || {
        slint::Image::from_rgba8(
            slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(
                &pixels,
                pixels.width(),
                pixels.height(),
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
    let commits = Rc::new(Cell::new(0));
    arm_feedback(&app, &commits);
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
        "system tile must show its shared press feedback"
    );
    crate::press_feedback::cancel(&app);
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
        folders: crate::folder_picker::Model::default(),
        images: crate::image_picker::Model::default(),
        launcher_scan: crate::launcher_scan::Model::default(),
        playtime_access: crate::playtime_access::Model::default(),
        open_url: crate::open_url::Model::default(),
        store: zaparoo_core::store::Store::new(client, handle.clone()),
        handle,
        media: crate::media_cache::MediaCache::new(),
        logos: crate::system_logos::Logos::new(),
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
        log_uploader: None,
        framebuffer_size: (W, H),
    };
    (runtime, ctx)
}

#[test]
fn startup_hub_seeding_uses_the_restored_page_and_skips_custom_icons() {
    let (_runtime, ctx) = offline_ctx();
    {
        let mut shared = crate::router::lock(&ctx.shared);
        let hub = &mut shared.hub;
        hub.layout.items = (0..7)
            .map(|n| zaparoo_core::hub_layout::HubItem {
                kind_raw: "zapscript".into(),
                system: "NES".into(),
                path: format!("/g/{n}"),
                script: format!("/g/{n}"),
                ..Default::default()
            })
            .collect();
        hub.entries = (0..7)
            .map(|n| zaparoo_app::hub::Entry {
                kind: Some(zaparoo_app::hub::Kind::ZapScript),
                hub_index: n,
                ..Default::default()
            })
            .collect();
        hub.entries[5].kind = Some(zaparoo_app::hub::Kind::Action);
        hub.entries[5].id = "resume".into();
        hub.entries[6].cover_key = "custom:own-icon".into();
        hub.grid.set_shape(2, 2);
        hub.grid.set_item_count(7);
        hub.grid.set_current_index_immediate(4);
    }
    assert_eq!(
        crate::hub::visible_cover_targets(&ctx),
        (vec![("NES".into(), "/g/4".into())], true)
    );
    assert_eq!(crate::router::lock(&ctx.shared).hub.grid.current_index(), 4);
}

#[test]
fn list_artwork_requests_only_detail_neighbors_and_retires_old_focus() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, _window) = boot();
    let (_runtime, ctx) = offline_ctx();
    app.global::<Shell>().set_active_screen(Screen::Games);
    app.global::<Shell>().set_browse_list_layout(true);
    {
        let mut shared = crate::router::lock(&ctx.shared);
        shared.persist.settings.games_browse_layout = "list".into();
        let model = &mut shared.games;
        model.rows = (0..40)
            .map(|n| {
                crate::games::GameRow::from(&zaparoo_core::media_types::BrowseEntry {
                    name: format!("Game {n}"),
                    path: format!("/g/{n}"),
                    system_id: "NES".into(),
                    entry_type: "media".into(),
                    ..Default::default()
                })
            })
            .collect();
        model.grid.set_item_count(model.rows.len());
        model.grid.set_current_index_immediate(10);
    }
    let hub = crate::media_cache::MediaKey {
        media_id: None,
        system: "NES".into(),
        path: "/hub/game".into(),
        max_size: 256,
        fit: ctx.media.hub_fit(),
        folder: false,
        image_type: None,
        prefer: None,
    };
    ctx.media.enqueue(hub.clone());
    crate::games::render(&ctx, &app);
    let size = crate::sizing::detail_cover_source_size(crate::router::output_scene(&app));
    let pending = ctx.media.pending_keys();
    assert_eq!(pending.len(), 4);
    assert_eq!(
        pending[..3]
            .iter()
            .map(|k| k.path.as_str())
            .collect::<Vec<_>>(),
        ["/g/10", "/g/11", "/g/9"]
    );
    assert!(pending[..3].iter().all(|k| k.max_size == size));
    assert!(app
        .global::<crate::GamesView>()
        .get_list_rows()
        .iter()
        .all(|r| !r.has_cover));

    crate::games::RENDERS.with(|n| n.set(0));
    crate::games::covers_landed(&ctx, &app, &pending[1..2]);
    assert_eq!(
        crate::games::RENDERS.with(Cell::get),
        0,
        "prefetch does not repaint"
    );
    crate::games::covers_landed(&ctx, &app, &[pending[0].color_preview()]);
    assert_eq!(
        crate::games::RENDERS.with(Cell::get),
        0,
        "tile-color previews do not repaint a text-only list"
    );
    crate::games::covers_landed(&ctx, &app, &pending[..1]);
    assert_eq!(crate::games::RENDERS.with(Cell::get), 1);

    crate::router::lock(&ctx.shared)
        .games
        .grid
        .set_current_index_immediate(20);
    crate::games::render(&ctx, &app);
    assert_eq!(
        ctx.media.pending_keys(),
        vec![hub.clone()],
        "a move retires the old focus and asks for nothing until it rests"
    );
    advance(zaparoo_app::media_list::DETAIL_DEBOUNCE_MS - 1);
    assert_eq!(ctx.media.pending_keys(), vec![hub.clone()]);
    advance(1);
    let pending = ctx.media.pending_keys();
    assert_eq!(pending.len(), 4);
    assert_eq!(
        pending[..3]
            .iter()
            .map(|k| k.path.as_str())
            .collect::<Vec<_>>(),
        ["/g/20", "/g/21", "/g/19"]
    );
    crate::router::lock(&ctx.shared).games.rapid.active = true;
    crate::games::render(&ctx, &app);
    assert_eq!(ctx.media.pending_keys(), vec![hub]);
}

#[test]
fn controller_report_seed_reaches_help_bar_before_event_loop() {
    use zaparoo_core::controller_report::{self, ControllerGlyphs};

    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, _window) = boot();
    let (_runtime, ctx) = offline_ctx();
    let ctx = std::sync::Arc::new(ctx);
    let report = ControllerGlyphs {
        layout: "style_c",
        accept_button: "FaceSouth",
        cancel_button: "FaceEast",
    };
    controller_report::publish(Some(report.clone()));
    let rx = controller_report::subscribe();

    // Main can report the pad before the frontend starts. The idle runtime
    // proves binding consumes that seed synchronously, without a later press.
    crate::bind_controller_report(&ctx, &app);
    let buttons = app.global::<crate::Buttons>();
    assert_eq!(buttons.get_style(), "style_c");
    assert_eq!(buttons.get_confirm(), "FaceSouth");
    assert_eq!(buttons.get_cancel(), "FaceEast");

    // Repeated presses rewrite Main's file but do not change its projection.
    controller_report::publish(Some(report));
    assert!(matches!(rx.has_changed(), Ok(false)));
    assert_eq!(buttons.get_style(), "style_c");

    // A manual style pins the artwork, not the controller's face positions.
    {
        let mut shared = crate::router::lock(&ctx.shared);
        shared.persist.settings.button_layout = "style_b".into();
        shared.persist.settings.swap_confirm_cancel = true;
        shared.persist.settings.swap_options_view = true;
    }
    crate::apply_buttons(&ctx, &app);
    assert_eq!(buttons.get_style(), "style_b");
    assert_eq!(buttons.get_confirm(), "FaceEast");
    assert_eq!(buttons.get_cancel(), "FaceSouth");
    assert_eq!(buttons.get_options(), "FaceWest");
    assert_eq!(buttons.get_menu(), "FaceNorth");
}

#[test]
fn keyboard_report_seed_keeps_fixed_bindings_despite_controller_swaps() {
    use zaparoo_core::controller_report::{self, ControllerGlyphs};

    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, _window) = boot();
    let (_runtime, ctx) = offline_ctx();
    let ctx = std::sync::Arc::new(ctx);
    {
        let mut shared = crate::router::lock(&ctx.shared);
        shared.persist.settings.swap_confirm_cancel = true;
        shared.persist.settings.swap_options_view = true;
    }
    controller_report::publish(Some(ControllerGlyphs {
        layout: "style_e",
        accept_button: "FaceEast",
        cancel_button: "FaceSouth",
    }));
    crate::bind_controller_report(&ctx, &app);
    let buttons = app.global::<crate::Buttons>();
    assert_eq!(buttons.get_style(), "style_e");
    assert_eq!(buttons.get_confirm(), "FaceEast");
    assert_eq!(buttons.get_cancel(), "FaceSouth");
    assert_eq!(buttons.get_options(), "FaceNorth");
    assert_eq!(buttons.get_menu(), "FaceWest");
}

#[test]
fn folder_setting_dispatches_host_once_until_completion() {
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, _window) = boot();
    let (_runtime, ctx) = offline_ctx();
    let requests = Arc::new(AtomicUsize::new(0));
    let counter = requests.clone();
    ctx.folders.configure(Arc::new(move || {
        counter.fetch_add(1, Ordering::SeqCst);
        true
    }));
    app.global::<Shell>().set_active_screen(Screen::Settings);
    crate::settings::open_page(&ctx, &app, SettingsPage::Library);
    crate::settings::handle_action(&ctx, &app, "accept");
    crate::settings::handle_action(&ctx, &app, "accept");
    assert_eq!(requests.load(Ordering::SeqCst), 1);
    assert_eq!(
        ctx.folders.status(),
        (crate::ActionStatus::FolderOpening, 0, true)
    );
    assert!(ctx
        .folders
        .update(1, zaparoo_app::folder_picker::State::Cancelled, 2));
    assert!(!ctx
        .folders
        .update(1, zaparoo_app::folder_picker::State::Failed, 0));
    crate::settings::handle_action(&ctx, &app, "accept");
    assert_eq!(requests.load(Ordering::SeqCst), 2);
}

#[test]
fn launcher_scan_setting_dispatches_host_once_until_completion() {
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, _window) = boot();
    let (_runtime, ctx) = offline_ctx();
    let requests = Arc::new(AtomicUsize::new(0));
    let counter = requests.clone();
    ctx.launcher_scan.configure(Arc::new(move || {
        counter.fetch_add(1, Ordering::SeqCst);
        true
    }));
    app.global::<Shell>().set_active_screen(Screen::Settings);
    crate::settings::open_page(&ctx, &app, SettingsPage::Library);
    crate::settings::reseat_to(&ctx, &app, "detectLaunchers");
    crate::settings::handle_action(&ctx, &app, "accept");
    crate::settings::handle_action(&ctx, &app, "accept");
    assert_eq!(requests.load(Ordering::SeqCst), 1);
    assert_eq!(
        ctx.launcher_scan.status(),
        (crate::ActionStatus::LauncherOpening, 0, true)
    );
    assert!(ctx
        .launcher_scan
        .update(1, zaparoo_app::launcher_scan::State::Cancelled, 0));
    assert!(!ctx
        .launcher_scan
        .update(1, zaparoo_app::launcher_scan::State::Failed, 0));
    crate::settings::handle_action(&ctx, &app, "accept");
    assert_eq!(requests.load(Ordering::SeqCst), 2);
}

/// Cover art that lands on a tile already on screen fades in; art a tile
/// already has when it appears paints at once, so paging never flashes.
#[test]
fn cover_art_fades_in_only_when_it_lands() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    crate::sizing::apply_scene(
        &app,
        crate::sizing::Scene::of(&app, f64::from(W), f64::from(H), false),
    );
    app.global::<crate::Motion>().set_enabled(true);
    app.global::<Shell>().set_active_screen(Screen::Hub);
    let mut buffer = slint::SharedPixelBuffer::<slint::Rgb8Pixel>::new(8, 8);
    for pixel in buffer.make_mut_slice() {
        *pixel = slint::Rgb8Pixel { r: 255, g: 0, b: 0 };
    }
    let red = slint::Image::from_rgb8(buffer);
    let cell = |has_cover: bool| GridCell {
        name: "Cover".into(),
        cover: red.clone(),
        cover_focus: red.clone(),
        has_cover,
        has_cover_focus: has_cover,
        fetched: true,
        ..Default::default()
    };
    let view = app.global::<HubView>();
    view.set_cell_width(70.0);
    view.set_cell_height(60.0);
    view.set_grid_y(40.0);
    view.set_grid_height(100.0);
    let art = |window: &Rc<MinimalSoftwareWindow>| {
        pixels(window).iter().filter(|p| p.0 == 0xF800).count()
    };

    // Art the tile has from the start is whole on the very first frame.
    view.set_cells(ModelRc::new(VecModel::from(vec![cell(true)])));
    let shown = art(&window);
    assert!(shown > 0, "cached art paints on the first frame");

    // Art that lands later starts transparent and ends whole.
    let cells = Rc::new(VecModel::from(vec![cell(false)]));
    view.set_cells(ModelRc::from(cells.clone()));
    settle(&window);
    assert_eq!(art(&window), 0);
    cells.set_row_data(0, cell(true));
    assert!(art(&window) < shown, "landed art does not pop in");
    settle(&window);
    assert_eq!(art(&window), shown, "the fade ends at full strength");

    // A built-in logo is not fetched art: it cuts in.
    let logo = |has_cover: bool| GridCell {
        fetched: false,
        ..cell(has_cover)
    };
    let cells = Rc::new(VecModel::from(vec![logo(false)]));
    view.set_cells(ModelRc::from(cells.clone()));
    settle(&window);
    cells.set_row_data(0, logo(true));
    assert_eq!(art(&window), shown, "a logo lands whole");
}

/// The pairing panel's one promise: Core is never left holding a PIN the
/// user has walked away from. Every way out of the panel is checked here,
/// against the same `cancels` count the driver sends its calls off.
#[test]
fn pairing_row_shows_a_pin_and_every_exit_calls_the_pairing_off() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    let (_runtime, ctx) = offline_ctx();
    crate::sizing::apply_scene(
        &app,
        crate::sizing::Scene::of(&app, f64::from(W), f64::from(H), false),
    );
    app.global::<crate::Motion>().set_enabled(false);
    app.global::<Shell>().set_active_screen(Screen::Settings);
    crate::settings::open_page(&ctx, &app, SettingsPage::About);
    let rows = app.global::<crate::SettingsView>().get_rows();
    assert_eq!(
        rows.row_data(0).map(|row| row.id.to_string()),
        Some("pairDevice".to_string()),
        "the About page leads with the pairing row"
    );

    // The row starts a pairing and puts the panel up before Core answers.
    let ov = app.global::<crate::Overlays>();
    settle(&window);
    app.window().request_redraw();
    let settings_page = frame(&window);
    crate::settings::handle_action(&ctx, &app, "accept");
    assert!(ov.get_pair_open());
    assert_eq!(ov.get_pair_phase(), PairPhase::Starting);

    // Core's PIN, with the seconds the panel counts down.
    let ticket = crate::pairing::open(&ctx, &app);
    let deadline = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_secs() as i64)
        + 30;
    crate::pairing::observe_started(&ctx, &app, ticket, "482913", deadline);
    assert_eq!(ov.get_pair_phase(), PairPhase::Showing);
    assert_eq!(ov.get_pair_pin(), "482913");
    assert_eq!(ov.get_pair_expires_in(), 30);
    settle(&window);
    app.window().request_redraw();
    assert_ne!(
        settings_page,
        frame(&window),
        "the PIN has to be on screen, not just in the model"
    );

    // Walking away calls it off and takes the PIN off screen.
    crate::router::handle_action(&ctx, &app, "cancel");
    assert!(!ov.get_pair_open());
    assert_eq!(ov.get_pair_pin(), "");
    assert_eq!(crate::router::lock(&ctx.shared).pairing.cancels, 1);

    // A device that pairs ends the flow by name, with nothing owed.
    let ticket = crate::pairing::open(&ctx, &app);
    crate::pairing::observe_started(&ctx, &app, ticket, "551104", deadline);
    crate::pairing::observe_paired(&ctx, &app, "Wizzo's phone");
    assert_eq!(ov.get_pair_phase(), PairPhase::Paired);
    assert_eq!(ov.get_pair_client(), "Wizzo's phone");
    assert_eq!(
        ov.get_pair_pin(),
        "",
        "a PIN that has been used is not left on screen"
    );
    crate::router::handle_action(&ctx, &app, "accept");
    assert!(!ov.get_pair_open());
    assert_eq!(
        crate::router::lock(&ctx.shared).pairing.cancels,
        1,
        "a completed pairing has nothing to call off"
    );

    // Running out of time retires the PIN and calls it off too.
    let ticket = crate::pairing::open(&ctx, &app);
    crate::pairing::observe_started(&ctx, &app, ticket, "730025", deadline);
    for _ in 0..30 {
        crate::pairing::observe_tick(&ctx, &app, ticket);
    }
    assert_eq!(ov.get_pair_phase(), PairPhase::Expired);
    assert_eq!(ov.get_pair_pin(), "");
    assert_eq!(crate::router::lock(&ctx.shared).pairing.cancels, 2);
    crate::router::handle_action(&ctx, &app, "cancel");
    assert!(!ov.get_pair_open());
    assert_eq!(crate::router::lock(&ctx.shared).pairing.cancels, 2);

    // A PIN Core minted for a panel the user already left is called off
    // as well: the start may only have reached Core after they walked.
    let ticket = crate::pairing::open(&ctx, &app);
    crate::router::handle_action(&ctx, &app, "cancel");
    assert_eq!(
        crate::router::lock(&ctx.shared).pairing.cancels,
        2,
        "nothing is owed while the start is still in flight"
    );
    crate::pairing::observe_started(&ctx, &app, ticket, "918820", deadline);
    assert!(!ov.get_pair_open());
    assert_eq!(crate::router::lock(&ctx.shared).pairing.cancels, 3);
}

/// One settings row by id, or an empty row the caller's asserts reject.
fn settings_row(app: &App, id: &str) -> crate::SettingsRow {
    app.global::<crate::SettingsView>()
        .get_rows()
        .iter()
        .find(|row| row.id == id)
        .unwrap_or_default()
}

fn online_settings(ctx: &crate::router::Ctx, app: &App, linked: bool) {
    {
        let mut shared = crate::router::lock(&ctx.shared);
        shared.online.available = true;
        shared.online.linked = linked;
        shared.online.features = zaparoo_app::online_settings::OnlineFeatures::default();
    }
    crate::settings::open_page(ctx, app, SettingsPage::Online);
}

/// The Online page's rows appear only when Core offers consent to this
/// client, the account row follows the link, and unlinking asks first.
#[test]
fn online_rows_follow_core_and_unlinking_asks_first() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, _window) = boot();
    let (_runtime, ctx) = offline_ctx();
    app.global::<crate::Motion>().set_enabled(false);
    app.global::<Shell>().set_active_screen(Screen::Settings);

    crate::settings::open_page(&ctx, &app, SettingsPage::Online);
    assert_eq!(
        settings_row(&app, "onlineLinkAccount").id,
        "",
        "no consent capability from Core, no Online rows"
    );
    assert_eq!(
        settings_row(&app, "onlineUnavailable").id,
        "onlineUnavailable"
    );
    assert_eq!(
        settings_row(&app, "playtimeSync").id,
        "",
        "play history sync lives on the Online page, not Library"
    );

    online_settings(&ctx, &app, false);
    let account = settings_row(&app, "onlineLinkAccount");
    assert_eq!(account.id, "onlineLinkAccount");
    assert_eq!(account.value, "link");
    assert_eq!(settings_row(&app, "onlineUnlinkAccount").id, "");
    assert_eq!(
        settings_row(&app, "onlineWarp").id,
        "",
        "Warp needs a linked account"
    );
    assert_eq!(settings_row(&app, "onlineStatus").value, "unlinked");
    let sync = settings_row(&app, "playtimeSync");
    assert_eq!(sync.id, "playtimeSync");
    assert!(!sync.checked, "upload consent defaults off");
    assert!(
        !sync.enabled,
        "a row that needs an account waits, dimmed, until one is linked"
    );
    assert!(!settings_row(&app, "onlineManageBackups").enabled);
    assert!(settings_row(&app, "onlineLinkAccount").enabled);
    // No input changes a row the page shows as unavailable: not Accept,
    // and not Left or Right on a toggle.
    {
        use slint::Model as _;
        let view = app.global::<crate::SettingsView>();
        let index = view
            .get_rows()
            .iter()
            .position(|row| row.id == "playtimeSync")
            .and_then(|index| i32::try_from(index).ok())
            .unwrap_or(-1);
        assert!(index >= 0, "the play history row is on the page");
        view.set_index(index);
    }
    for action in ["accept", "left", "right"] {
        crate::settings::handle_action(&ctx, &app, action);
        assert!(
            !crate::router::lock(&ctx.shared)
                .online
                .features
                .play_history,
            "{action} must not change a row that waits for an account"
        );
    }
    let all = settings_row(&app, "onlineAllFeatures");
    assert_eq!(all.control, ControlKind::TriToggle);
    assert_eq!(all.value, "off");

    online_settings(&ctx, &app, true);
    let account = settings_row(&app, "onlineUnlinkAccount");
    assert_eq!(account.id, "onlineUnlinkAccount");
    assert_eq!(account.value, "unlink");
    assert_eq!(settings_row(&app, "onlineLinkAccount").id, "");
    assert_eq!(settings_row(&app, "onlineStatus").value, "linked");
    assert_eq!(settings_row(&app, "onlineWarp").value, "checking");
    assert!(settings_row(&app, "playtimeSync").enabled);
    assert!(settings_row(&app, "onlineManageBackups").enabled);
    let ov = app.global::<crate::Overlays>();
    crate::online::accept_account(&ctx, &app);
    assert!(ov.get_dialog_open());
    assert_eq!(ov.get_dialog_kind(), DialogKind::UnlinkOnline);
    assert_eq!(ov.get_dialog_buttons().row_data(0), Some(DialogButton::No));
    assert_eq!(ov.get_dialog_focus(), 0);
    crate::router::handle_action(&ctx, &app, "cancel");
    assert!(!ov.get_dialog_open());
    assert!(crate::router::lock(&ctx.shared).online.linked);
}

/// The header logo setting applies as saved whatever is known; only a
/// completed Online check without the feature locks its rows and puts the
/// Zaparoo logo back, saving that.
#[test]
#[allow(clippy::expect_used, reason = "scratch config fixture")]
fn a_known_missing_entitlement_locks_and_reverts_the_header_logo() {
    use zaparoo_app::branding::Entitlement;
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, _window) = boot();
    let (_runtime, mut ctx) = offline_ctx();
    let dir = std::env::temp_dir().join(format!("zaparoo-branding-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch dir");
    ctx.config_path = dir.join("frontend.toml");
    app.global::<crate::Motion>().set_enabled(false);
    app.global::<Shell>().set_active_screen(Screen::Settings);
    crate::router::lock(&ctx.shared)
        .persist
        .settings
        .header_logo = "none".into();
    crate::settings::apply_header_logo(&ctx, &app);
    let brand = app.global::<crate::Brand>();
    assert_eq!(
        brand.get_mode(),
        crate::HeaderLogo::None,
        "applies as saved"
    );

    for entitlement in [Entitlement::Unknown, Entitlement::Yes] {
        crate::router::lock(&ctx.shared).branding = entitlement;
        crate::settings::enforce_branding(&ctx, &app);
        crate::settings::open_page(&ctx, &app, SettingsPage::Appearance);
        assert_eq!(
            crate::router::lock(&ctx.shared)
                .persist
                .settings
                .header_logo,
            "none"
        );
        assert_eq!(brand.get_mode(), crate::HeaderLogo::None);
        assert!(settings_row(&app, "headerLogo").enabled);
        assert!(!settings_row(&app, "headerLogo").premium);
    }

    crate::router::lock(&ctx.shared).branding = Entitlement::No;
    crate::settings::enforce_branding(&ctx, &app);
    crate::settings::open_page(&ctx, &app, SettingsPage::Appearance);
    assert_eq!(
        crate::router::lock(&ctx.shared)
            .persist
            .settings
            .header_logo,
        "zaparoo"
    );
    assert_eq!(brand.get_mode(), crate::HeaderLogo::Zaparoo);
    let saved = std::fs::read_to_string(&ctx.config_path).expect("mirrored");
    assert!(saved.contains("header_logo = \"zaparoo\""));
    let row = settings_row(&app, "headerLogo");
    assert!(row.enabled, "a locked row stays usable");
    assert!(row.premium, "and carries the Pro badge");
    assert_eq!(row.value, "zaparoo");
    assert_eq!(settings_row(&app, "headerLogoImage").id, "");
    // Accept on a locked row explains instead of opening the picker.
    focus_settings_row(&app, "headerLogo");
    crate::settings::handle_action(&ctx, &app, "accept");
    let overlays = app.global::<crate::Overlays>();
    assert!(!overlays.get_list_open());
    assert!(overlays.get_dialog_open());
    assert_eq!(overlays.get_dialog_kind(), DialogKind::LogoSupport);
    // Unlinked: Link account sits beside OK, focused, and opens the link
    // panel once the dialog closes.
    let buttons: Vec<DialogButton> = overlays.get_dialog_buttons().iter().collect();
    assert_eq!(buttons, [DialogButton::Ok, DialogButton::LinkAccount]);
    assert_eq!(overlays.get_dialog_focus(), 1);
    assert_eq!(overlays.get_dialog_arg(), "");
    crate::router::close_dialog(&app);
    // Linked: the copy says so, and OK is the only button.
    crate::router::lock(&ctx.shared).online.linked = true;
    crate::settings::handle_action(&ctx, &app, "accept");
    let buttons: Vec<DialogButton> = overlays.get_dialog_buttons().iter().collect();
    assert_eq!(buttons, [DialogButton::Ok]);
    assert_eq!(overlays.get_dialog_arg(), "linked");
    let _ = std::fs::remove_dir_all(&dir);
}

/// A custom logo is a paint-sized raster inside the logo box; a file that
/// cannot be read shows the Zaparoo logo; no logo paints nothing.
#[test]
#[allow(clippy::expect_used, reason = "scratch image fixture")]
fn a_custom_header_logo_fits_the_box_and_falls_back_to_zaparoo() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    crate::theme::apply_palette(&app, "zaparoo-dark", "normal");
    let (width, height) = (960, 540);
    window.set_size(slint::PhysicalSize::new(width, height));
    let sizing = app.global::<Sizing>();
    sizing.set_screen_width(width as f32);
    sizing.set_screen_height(height as f32);
    crate::sizing::apply_scene(
        &app,
        crate::sizing::Scene::of(&app, f64::from(width), f64::from(height), false),
    );
    let dir = std::env::temp_dir().join(format!("zaparoo-custom-logo-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch dir");
    let square = dir.join("square.png");
    image::RgbaImage::from_pixel(64, 64, image::Rgba([255, 0, 0, 255]))
        .save(&square)
        .expect("PNG");
    crate::brand::apply_now(&app, &square.to_string_lossy());
    let brand = app.global::<crate::Brand>();
    assert_eq!(brand.get_mode(), crate::HeaderLogo::Custom);
    let box_h = sizing.get_header_height();
    let box_w = box_h * brand.get_aspect();
    // A square image fills the box's height, not its width.
    assert!((brand.invoke_custom_width(box_w, box_h) - box_h.round()).abs() < f32::EPSILON);
    let image = brand.invoke_custom_image(box_w, box_h);
    assert_eq!(
        (image.size().width, image.size().height),
        (box_h.round() as u32, box_h.round() as u32)
    );
    let capture = || {
        let mut pixels = vec![Rgb565Pixel(0); (width * height) as usize];
        window.request_redraw();
        assert!(window.draw_if_needed(|renderer| {
            renderer.render(&mut pixels, width as usize);
        }));
        pixels
    };
    let custom = capture();
    let red = Rgb565Pixel(0xF800);
    assert!(custom.iter().filter(|p| **p == red).count() > 100);
    brand.set_mode(crate::HeaderLogo::None);
    let none = capture();
    assert!(none.iter().all(|p| *p != red));

    let broken = dir.join("broken.png");
    std::fs::write(&broken, b"not an image").expect("file");
    crate::brand::apply_now(&app, &broken.to_string_lossy());
    assert_eq!(brand.get_mode(), crate::HeaderLogo::Zaparoo);
    let _ = std::fs::remove_dir_all(&dir);
}

fn launcher(
    id: &str,
    system: &str,
    groups: &[&str],
    available: bool,
) -> zaparoo_core::media_types::LauncherInfo {
    zaparoo_core::media_types::LauncherInfo {
        id: id.into(),
        system_id: system.into(),
        system_name: system.into(),
        groups: groups.iter().map(|g| (*g).to_string()).collect(),
        available,
        ..Default::default()
    }
}

/// Put the Settings cursor on a row by id.
fn focus_settings_row(app: &App, id: &str) {
    let view = app.global::<crate::SettingsView>();
    let index = view
        .get_rows()
        .iter()
        .position(|row| row.id == id)
        .and_then(|i| i32::try_from(i).ok())
        .unwrap_or(-1);
    assert!(index >= 0, "{id} is on the page");
    view.set_index(index);
}

fn launcher_prefs_ids(app: &App) -> Vec<String> {
    app.global::<crate::Overlays>()
        .get_launcher_prefs_rows()
        .iter()
        .map(|row| row.id.to_string())
        .collect()
}

/// The launcher preference row only shows for a Core that has the setting;
/// its list adds at the end and holds the new entry, a move walks it and
/// Back puts it back, and Remove takes an entry out. Every change is the
/// list the page shows straight away.
#[test]
fn the_launcher_preference_list_adds_moves_and_removes_in_its_own_panel() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, _window) = boot();
    let (_runtime, ctx) = offline_ctx();
    app.global::<crate::Motion>().set_enabled(false);
    app.global::<Shell>().set_active_screen(Screen::Settings);

    crate::settings::open_page(&ctx, &app, SettingsPage::Library);
    assert_eq!(
        settings_row(&app, "launcherPreference").id,
        "",
        "an older Core has no list"
    );

    {
        let mut shared = crate::router::lock(&ctx.shared);
        shared.launchers = vec![
            launcher("RANES", "NES", &["RetroAchievements"], true),
            launcher("NESdb9", "NES", &["DB9"], false),
            launcher("SNES", "SNES", &[], true),
        ];
        shared.launcher_preference = Some(vec!["SNES".into()]);
        shared.launcher_pref_confirmed = Some(vec!["SNES".into()]);
    }
    crate::settings::open_page(&ctx, &app, SettingsPage::Library);
    let row = settings_row(&app, "launcherPreference");
    assert_eq!(row.control, ControlKind::Navigate);
    focus_settings_row(&app, "launcherPreference");
    crate::router::handle_action(&ctx, &app, "accept");
    let ov = app.global::<crate::Overlays>();
    assert!(ov.get_launcher_prefs_open());
    assert_eq!(
        launcher_prefs_ids(&app),
        vec!["SNES", ""],
        "the list, then Add…"
    );

    // Add…: groups first, then launchers, without what is already listed.
    crate::router::handle_action(&ctx, &app, "down");
    crate::router::handle_action(&ctx, &app, "accept");
    assert_eq!(ov.get_launcher_prefs_page(), crate::LauncherPrefsPage::Add);
    assert_eq!(
        launcher_prefs_ids(&app),
        vec!["DB9", "RetroAchievements", "NESdb9", "RANES"]
    );
    crate::router::handle_action(&ctx, &app, "down");
    crate::router::handle_action(&ctx, &app, "accept");
    assert_eq!(ov.get_launcher_prefs_page(), crate::LauncherPrefsPage::List);
    assert!(
        ov.get_launcher_prefs_moving(),
        "a new entry is held where it landed"
    );
    assert_eq!(
        crate::launcher_prefs::current(&ctx),
        Some(vec!["SNES".to_string(), "RetroAchievements".to_string()])
    );

    // Up takes it first; Back puts it back where the move began.
    crate::router::handle_action(&ctx, &app, "up");
    assert_eq!(
        crate::launcher_prefs::shown_for_test(&ctx),
        vec!["RetroAchievements", "SNES"]
    );
    crate::router::handle_action(&ctx, &app, "up");
    assert_eq!(ov.get_launcher_prefs_index(), 0, "a move stops at the top");
    crate::router::handle_action(&ctx, &app, "cancel");
    assert!(!ov.get_launcher_prefs_moving());
    assert!(
        ov.get_launcher_prefs_open(),
        "Back ends the move, not the list"
    );
    assert_eq!(
        crate::launcher_prefs::shown_for_test(&ctx),
        vec!["SNES", "RetroAchievements"]
    );

    // Move then Done writes the new order.
    crate::router::handle_action(&ctx, &app, "accept");
    assert_eq!(
        ov.get_launcher_prefs_page(),
        crate::LauncherPrefsPage::Entry
    );
    crate::router::handle_action(&ctx, &app, "accept");
    crate::router::handle_action(&ctx, &app, "up");
    crate::router::handle_action(&ctx, &app, "accept");
    assert_eq!(
        crate::launcher_prefs::current(&ctx),
        Some(vec!["RetroAchievements".to_string(), "SNES".to_string()])
    );

    // Remove.
    crate::router::handle_action(&ctx, &app, "down");
    crate::router::handle_action(&ctx, &app, "accept");
    crate::router::handle_action(&ctx, &app, "down");
    crate::router::handle_action(&ctx, &app, "accept");
    assert_eq!(
        crate::launcher_prefs::current(&ctx),
        Some(vec!["RetroAchievements".to_string()])
    );

    // Back closes the panel onto the page under it.
    crate::router::handle_action(&ctx, &app, "cancel");
    assert!(!ov.get_launcher_prefs_open());
    assert_eq!(app.global::<Shell>().get_active_screen(), Screen::Settings);
    assert_eq!(
        app.global::<crate::SettingsView>().get_page(),
        SettingsPage::Library
    );
}

/// The login screen keeps the password masked but for the character just
/// typed, never takes the persisted screen from Settings, and Back steps
/// from the password to the username and then out to Settings.
#[test]
fn the_retroachievements_login_masks_the_password_and_is_never_restored() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, _window) = boot();
    let (_runtime, ctx) = offline_ctx();
    app.global::<crate::Motion>().set_enabled(false);
    crate::settings::enter(&ctx, &app, EntryMode::Fresh);
    crate::settings::open_page(&ctx, &app, SettingsPage::RetroAchievements);
    crate::router::lock(&ctx.shared).ra.cfg.username = "Callan".into();

    crate::ra_login::open(&ctx, &app);
    assert_eq!(app.global::<Shell>().get_active_screen(), Screen::RaLogin);
    let view = app.global::<crate::RaLoginView>();
    assert_eq!(view.get_step(), crate::RaLoginStep::Username);
    assert_eq!(view.get_before(), "Callan", "the saved username comes back");
    assert_eq!(
        crate::router::lock(&ctx.shared).persist.active_screen,
        "settings"
    );

    crate::router::handle_action(&ctx, &app, "accept");
    for c in "pw".chars() {
        crate::ra_login::type_char(&ctx, &app, c);
    }
    // Typing moved focus to the submit key; Accept there goes on.
    crate::router::handle_action(&ctx, &app, "accept");
    assert_eq!(view.get_step(), crate::RaLoginStep::Password);
    for c in "ab".chars() {
        crate::ra_login::type_char(&ctx, &app, c);
    }
    assert_eq!(
        view.get_before(),
        "\u{2022}b",
        "only the newest character shows"
    );
    assert!(view.get_can_submit());
    assert_eq!(
        crate::router::lock(&ctx.shared).persist.active_screen,
        "settings"
    );

    crate::router::handle_action(&ctx, &app, "cancel");
    assert_eq!(view.get_step(), crate::RaLoginStep::Username);
    crate::router::handle_action(&ctx, &app, "cancel");
    assert_eq!(app.global::<Shell>().get_active_screen(), Screen::Settings);
    assert_eq!(
        app.global::<crate::SettingsView>().get_page(),
        SettingsPage::RetroAchievements
    );
    assert_eq!(
        crate::router::lock(&ctx.shared).persist.active_screen,
        "settings"
    );
}

/// The link panel shows Core's code until Core decides, and every way out
/// takes the code off screen.
#[test]
fn online_link_panel_shows_the_code_until_core_decides() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    let (_runtime, ctx) = offline_ctx();
    crate::sizing::apply_scene(
        &app,
        crate::sizing::Scene::of(&app, f64::from(W), f64::from(H), false),
    );
    app.global::<crate::Motion>().set_enabled(false);
    app.global::<Shell>().set_active_screen(Screen::Settings);
    online_settings(&ctx, &app, false);

    // Linking puts the panel up before Core answers, then shows the code.
    let ov = app.global::<crate::Overlays>();
    settle(&window);
    app.window().request_redraw();
    let settings_page = frame(&window);
    let ticket = crate::online::open(&ctx, &app);
    assert!(ov.get_online_open());
    assert_eq!(ov.get_online_phase(), crate::OnlineLinkPhase::Starting);
    crate::online::observe_started(
        &ctx,
        &app,
        ticket,
        "ABCD-1234",
        "https://online.example/link",
        "https://online.example/link?code=ABCD1234",
        600,
    );
    assert_eq!(ov.get_online_phase(), crate::OnlineLinkPhase::Showing);
    assert_eq!(ov.get_online_code(), "ABCD-1234");
    assert_eq!(ov.get_online_url(), "https://online.example/link");
    assert!(ov.get_online_qr_modules() > 0, "the code is scannable");
    settle(&window);
    app.window().request_redraw();
    assert_ne!(
        settings_page,
        frame(&window),
        "the code has to be on screen"
    );

    // Accept does nothing while Core waits; approval ends it.
    crate::router::handle_action(&ctx, &app, "accept");
    assert!(ov.get_online_open());
    crate::online::observe_status(&ctx, &app, "approved");
    assert_eq!(ov.get_online_phase(), crate::OnlineLinkPhase::Linked);
    assert_eq!(
        ov.get_online_code(),
        "",
        "an approved code leaves the screen"
    );
    crate::router::handle_action(&ctx, &app, "accept");
    assert!(!ov.get_online_open());

    // Back walks away from a live code.
    let ticket = crate::online::open(&ctx, &app);
    crate::online::observe_started(&ctx, &app, ticket, "WXYZ", "u", "", 600);
    crate::router::handle_action(&ctx, &app, "cancel");
    assert!(!ov.get_online_open());
    assert_eq!(ov.get_online_code(), "");

    // A refused start stays up with its failure until dismissed.
    let ticket = crate::online::open(&ctx, &app);
    crate::online::observe_failed(&ctx, &app, ticket);
    assert_eq!(ov.get_online_phase(), crate::OnlineLinkPhase::Failed);
    crate::router::handle_action(&ctx, &app, "accept");
    assert!(!ov.get_online_open());
}

#[test]
fn a_pairing_core_refuses_closes_the_panel_and_takes_the_shared_alert() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, _window) = boot();
    let (_runtime, ctx) = offline_ctx();
    app.global::<crate::Motion>().set_enabled(false);
    app.global::<Shell>().set_active_screen(Screen::Settings);
    crate::settings::open_page(&ctx, &app, SettingsPage::About);

    let ticket = crate::pairing::open(&ctx, &app);
    crate::pairing::observe_failed(&ctx, &app, ticket);
    let ov = app.global::<crate::Overlays>();
    assert!(!ov.get_pair_open());
    assert!(ov.get_dialog_open());
    assert_eq!(ov.get_dialog_kind(), DialogKind::ActionError);
    assert_eq!(ov.get_dialog_error(), ErrorKind::Pairing);
    assert_eq!(
        ov.get_dialog_buttons().row_data(0),
        Some(DialogButton::Ok),
        "a refused pairing is read and dismissed, not retried in place"
    );
    assert_eq!(
        crate::router::lock(&ctx.shared).pairing.cancels,
        0,
        "a start that never happened leaves nothing to call off"
    );
    crate::router::handle_action(&ctx, &app, "accept");
    assert!(!ov.get_dialog_open());
}

#[test]
fn launch_dormancy_is_local_only() {
    let local =
        |endpoint: &str| zaparoo_core::transport::Transport::tcp(endpoint.into(), None).is_local();
    assert!(local("ws://127.0.0.1:7497/api/v0.1"));
    assert!(local("ws://localhost:7497/api/v0.1"));
    assert!(!local("ws://192.0.2.10:7497/api/v0.1"));
    #[cfg(unix)]
    assert!(zaparoo_core::transport::Transport::unix(
        "/private/api.sock".into(),
        "test-key".into(),
        1
    )
    .is_local());
}

#[test]
fn input_storm_keeps_one_idle_countdown_and_off_cancels_it() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, _window) = boot();
    let (_runtime, ctx) = offline_ctx();
    let shell = app.global::<Shell>();
    shell.set_boot_complete(true);
    crate::router::lock(&ctx.shared)
        .persist
        .settings
        .screensaver_timeout = "1".into();
    let before = crate::router::idle_firings();
    for _ in 0..1500 {
        CLOCK.with(|clock| clock.set(clock.get() + 1));
        crate::router::reset_idle(&ctx, &app);
    }
    advance(999);
    assert!(!shell.get_saver_armed());
    assert_eq!(crate::router::idle_firings(), before);
    advance(1);
    assert!(shell.get_saver_armed());
    assert_eq!(crate::router::idle_firings(), before + 1);
    crate::screensaver::disarm(&app);
    crate::router::reset_idle(&ctx, &app);
    crate::router::lock(&ctx.shared)
        .persist
        .settings
        .screensaver_timeout = "off".into();
    crate::router::reset_idle(&ctx, &app);
    advance(2000);
    assert!(!shell.get_saver_armed());
    assert_eq!(crate::router::idle_firings(), before + 1);
}

#[test]
fn idle_countdown_preserves_busy_gates_and_shutdown_cancellation() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, _window) = boot();
    let (_runtime, ctx) = offline_ctx();
    let shell = app.global::<Shell>();
    crate::router::lock(&ctx.shared)
        .persist
        .settings
        .screensaver_timeout = "1".into();
    for (boot_complete, dormant, transitioning, update) in [
        (false, false, false, false),
        (true, true, false, false),
        (true, false, true, false),
        (true, false, false, true),
    ] {
        shell.set_boot_complete(boot_complete);
        shell.set_dormant(dormant);
        shell.set_transitioning(transitioning);
        shell.set_active_screen(if update { Screen::Update } else { Screen::Hub });
        app.global::<crate::UpdateView>()
            .set_allows_screensaver(false);
        crate::router::reset_idle(&ctx, &app);
        advance(1000);
        assert!(!shell.get_saver_armed());
    }
    shell.set_active_screen(Screen::Hub);
    crate::router::reset_idle(&ctx, &app);
    crate::router::lock(&ctx.shared)
        .persist
        .settings
        .screensaver_timeout = "2".into();
    crate::router::reset_idle(&ctx, &app);
    advance(1000);
    assert!(!shell.get_saver_armed());
    advance(1000);
    assert!(shell.get_saver_armed());
    crate::screensaver::disarm(&app);
    crate::router::reset_idle(&ctx, &app);
    crate::router::stop_idle();
    advance(3000);
    assert!(!shell.get_saver_armed());
}

#[test]
fn app_activation_dismisses_saver_and_restarts_idle_from_foreground_return() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, _window) = boot();
    let (_runtime, ctx) = offline_ctx();
    crate::router::lock(&ctx.shared)
        .persist
        .settings
        .screensaver_timeout = "1".into();
    let shell = app.global::<Shell>();
    shell.set_boot_complete(true);

    crate::router::reset_idle(&ctx, &app);
    CLOCK.with(|clock| clock.set(clock.get() + 800));
    slint::platform::update_timers_and_animations();
    shell.set_saver_armed(true); // It may have armed while another app held focus.
    crate::router::on_app_activated(&ctx, &app);
    assert!(
        !shell.get_saver_armed(),
        "foreground return must reveal the UI"
    );

    CLOCK.with(|clock| clock.set(clock.get() + 300));
    slint::platform::update_timers_and_animations();
    assert!(!shell.get_saver_armed(), "old idle timer must not re-arm");
    CLOCK.with(|clock| clock.set(clock.get() + 800));
    slint::platform::update_timers_and_animations();
    assert!(
        shell.get_saver_armed(),
        "new countdown starts on activation"
    );
}

#[test]
fn every_screensaver_style_arms_steps_and_tears_down() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, _window) = boot();
    let (_runtime, ctx) = offline_ctx();
    let shell = app.global::<Shell>();
    let saver = app.global::<crate::Saver>();
    shell.set_boot_complete(true);
    for token in zaparoo_app::screensaver::STYLES {
        {
            let mut shared = crate::router::lock(&ctx.shared);
            shared.persist.settings.screensaver_timeout = "1".into();
            shared.persist.settings.screensaver_style = (*token).into();
            shared.persist.settings.screensaver_bouncing = "logo".into();
        }
        crate::router::reset_idle(&ctx, &app);
        advance(1000);
        assert!(shell.get_saver_armed(), "{token} arms");
        assert!(crate::screensaver::running(), "{token} holds its run");
        // A few seconds of steps: every style's tick runs.
        for _ in 0..30 {
            advance(100);
        }
        crate::screensaver::disarm(&app);
        assert!(!shell.get_saver_armed(), "{token} disarms");
        assert!(!crate::screensaver::running(), "{token} drops its run");
        assert_eq!(saver.get_sprites().row_count(), 0, "{token} sprites");
        assert_eq!(saver.get_frame().size().width, 0, "{token} frame");
    }
    crate::router::stop_idle();
}

#[test]
fn random_screensaver_never_plays_the_same_style_twice_running() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, _window) = boot();
    let (_runtime, ctx) = offline_ctx();
    app.global::<Shell>().set_boot_complete(true);
    crate::router::lock(&ctx.shared)
        .persist
        .settings
        .screensaver_style = "random".into();
    let mut last = None;
    for _ in 0..8 {
        crate::screensaver::arm(&ctx, &app);
        let style = app.global::<crate::Saver>().get_style();
        assert_ne!(Some(style), last, "Random repeated a style");
        last = Some(style);
        crate::screensaver::disarm(&app);
    }
}

#[test]
fn hiding_the_header_logo_switches_a_bouncing_logo_to_the_clock() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, _window) = boot();
    let (_runtime, ctx) = offline_ctx();
    let bouncing = |ctx: &crate::router::Ctx| {
        crate::router::lock(&ctx.shared)
            .persist
            .settings
            .screensaver_bouncing
            .clone()
    };
    crate::router::lock(&ctx.shared)
        .persist
        .settings
        .screensaver_bouncing = "logo".into();
    crate::settings::picker_selected(&ctx, &app, "headerLogo", "none");
    assert_eq!(bouncing(&ctx), "clock");
    // Showing the logo again leaves the clock alone.
    crate::settings::picker_selected(&ctx, &app, "headerLogo", "zaparoo");
    assert_eq!(bouncing(&ctx), "clock");
    // A Recent game choice is not the logo: hiding it changes nothing.
    crate::settings::picker_selected(&ctx, &app, "screensaverBouncing", "recent-game");
    crate::settings::picker_selected(&ctx, &app, "headerLogo", "none");
    assert_eq!(bouncing(&ctx), "recent-game");
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

    app.global::<Shell>().set_active_screen(Screen::Systems);
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
    app.global::<Shell>().set_active_screen(Screen::Games);
    {
        let mut state = crate::router::lock(&ctx.shared);
        state.persist.settings.reduce_motion = false;
        state.games.loading = false;
        state.games.rows = game_rows("Game", 200).into();
        state.games.grid.set_item_count(200);
    }
    crate::games::render(&ctx, &app);
    crate::router::handle_action(&ctx, &app, "page_next");
    assert!(crate::router::lock(&ctx.shared).games.page.sliding);
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
    crate::input::dispatch_repeat(&ctx, &app, "page_next", HoldTier::Page);
    assert_eq!(
        crate::router::lock(&ctx.shared).games.grid.current_page(),
        first + 1,
        "new input must interrupt an in-flight page instead of waiting"
    );
    let first = first + 1;
    distinct_frames(&window, 18);
    assert!(!crate::router::lock(&ctx.shared).games.page.sliding);
    for expected in first + 1..=first + 5 {
        crate::input::dispatch_repeat(&ctx, &app, "page_next", HoldTier::Page);
        let state = crate::router::lock(&ctx.shared);
        assert_eq!(state.games.grid.current_page(), expected);
        assert!(
            !state.games.page.sliding,
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
    crate::input::dispatch_repeat(&ctx, &app, "page_next", HoldTier::Page);
    assert_eq!(
        crate::router::lock(&ctx.shared).games.grid.current_page(),
        first + 5,
        "modal-owned repeats must not page the background"
    );
    assert!(!crate::input::rapid_page(&ctx));
    app.global::<crate::Overlays>().set_list_open(false);
    crate::router::handle_action(&ctx, &app, "page_next");
    assert!(
        crate::router::lock(&ctx.shared).games.page.sliding,
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
        (Screen::Hub, Screen::Systems),
        (Screen::Systems, Screen::Games),
        (Screen::Hub, Screen::Games),
        (Screen::Hub, Screen::FavoriteSystems),
        (Screen::FavoriteSystems, Screen::Favorites),
        (Screen::Hub, Screen::Favorites),
        (Screen::Hub, Screen::Recents),
        (Screen::Hub, Screen::Settings),
        (Screen::Settings, Screen::About),
    ] {
        shell.set_active_screen(from);
        app.global::<SystemsView>().set_mode(
            if from == Screen::FavoriteSystems || to == Screen::FavoriteSystems {
                SystemsMode::Favorites
            } else {
                SystemsMode::Category
            },
        );
        app.global::<crate::GamesView>().set_mode(match to {
            Screen::Favorites => GamesMode::Favorites,
            Screen::Recents => GamesMode::Recents,
            _ => GamesMode::Browse,
        });
        crate::settings::open_page(
            &ctx,
            &app,
            if to == Screen::About {
                SettingsPage::About
            } else {
                SettingsPage::Root
            },
        );
        crate::router::refresh_layout(&app);
        settle(&window);
        crate::router::transition_to_screen(&app, to, 1);
        assert_eq!(shell.get_active_screen(), to, "ready route commits now");
        assert!(!shell.get_transitioning());
        settle(&window);
        if to == Screen::About {
            crate::settings::show_about_return(&ctx, &app);
        } else {
            crate::router::transition_to_screen(&app, from, -1);
        }
        assert_eq!(shell.get_active_screen(), from, "Back commits now");
        settle(&window);
    }
    shell.set_active_screen(Screen::Settings);
    crate::settings::open_page(&ctx, &app, SettingsPage::Root);
    let settings = app.global::<crate::SettingsView>();
    for (index, page) in crate::settings::root_pages().into_iter().enumerate() {
        settings.set_index(index as i32);
        settle(&window);
        crate::settings::handle_action(&ctx, &app, "accept");
        assert!(!shell.get_transitioning());
        if page.id == zaparoo_app::settings::MUSIC_PAGE {
            // Music is a screen of its own, opened as synchronously.
            assert_eq!(shell.get_active_screen(), Screen::Music);
            crate::music::handle_action(&ctx, &app, "cancel");
            assert_eq!(shell.get_active_screen(), Screen::Settings);
        } else {
            assert_eq!(
                Ok(settings.get_page()),
                SettingsPage::try_from(page.id),
                "Settings is synchronous"
            );
            crate::settings::handle_action(&ctx, &app, "cancel");
        }
        assert_eq!(settings.get_page(), SettingsPage::Root);
        assert_eq!(
            settings.get_index(),
            index as i32,
            "Back restores the category tile"
        );
    }
    crate::settings::open_page(&ctx, &app, SettingsPage::Appearance);
    assert!(settings.get_scroll().abs() < f32::EPSILON);
    for _ in 0..6 {
        crate::settings::handle_action(&ctx, &app, "down");
    }
    assert!(
        settings.get_scroll() > 0.0,
        "moving focus past the viewport must scroll the selected row into view"
    );

    shell.set_reduce_motion(true);
    crate::settings::navigate_page(&ctx, &app, SettingsPage::About);
    assert!(!shell.get_transitioning());
    crate::settings::navigate_page(&ctx, &app, SettingsPage::Root);
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

fn seat_visibility_list(
    ctx: &crate::router::Ctx,
    app: &App,
    mode: GamesMode,
) -> crate::games::GameRow {
    crate::sizing::apply_scene(
        app,
        crate::sizing::Scene::of(app, f64::from(W), f64::from(H), false),
    );
    app.global::<Shell>().set_active_screen(mode.screen());
    let mut shared = crate::router::lock(&ctx.shared);
    shared.games.mode = mode;
    shared.games.system_id = "NES".into();
    shared.games.rows = game_rows("Game", 3).into();
    for (index, row) in shared.games.rows.iter_mut().enumerate() {
        row.media_id = Some(index as i64 + 42);
        row.path = format!("/g/Game {index}.nes");
        row.system_id = "NES".into();
    }
    shared.games.grid.set_item_count(3);
    shared.games.grid.set_has_more_pages(true);
    shared.games.next_cursor = Some("old-visibility-cursor".into());
    let target = shared.games.rows[0].clone();
    shared.persist.games.path_stack = vec![String::new()];
    shared.persist.games.selected_at_level = vec![target.path.clone()];
    shared
        .persist
        .favorites
        .selected_path
        .clone_from(&target.path);
    shared
        .persist
        .recents
        .selected_path
        .clone_from(&target.path);
    shared.letter_scope = Some(("NES".into(), String::new()));
    shared.letter_buckets = vec![zaparoo_core::media_types::BrowseIndexGroup {
        label: "G".into(),
        count: 3,
        ..Default::default()
    }];
    target
}

fn visibility_reply(hidden: bool) -> zaparoo_core::media_types::MediaTagsUpdateResult {
    use zaparoo_core::media_types::TagInfo;
    let mut tags = vec![TagInfo {
        tag: "favorite".into(),
        tag_type: "user".into(),
        ..Default::default()
    }];
    if hidden {
        tags.push(TagInfo {
            tag: "hidden".into(),
            tag_type: "user".into(),
            ..Default::default()
        });
    }
    zaparoo_core::media_types::MediaTagsUpdateResult { tags }
}

fn disc_rows() -> Vec<crate::context_page::PageRow> {
    (1..=2)
        .map(|disc| crate::context_page::PageRow {
            label_key: "disc",
            label: disc.to_string(),
            name: "Game 0".into(),
            launch_text: format!("/g/Game 0/Game 0 (Disc {disc}).chd"),
            deck: None,
        })
        .collect()
}

fn context_ids(app: &App) -> Vec<String> {
    app.global::<crate::Overlays>()
        .get_context_entries()
        .iter()
        .map(|entry| entry.id.to_string())
        .collect()
}

/// Put the open menu's focus on row `id`, returning where it sits.
/// The id of the menu row that has focus.
fn context_row_id(app: &App) -> Option<String> {
    let index = usize::try_from(app.global::<crate::Overlays>().get_context_index()).ok()?;
    context_ids(app).get(index).cloned()
}

fn focus_context_row(app: &App, id: &str) -> i32 {
    let index = context_ids(app).iter().position(|row| row == id);
    assert!(index.is_some(), "the menu has no {id} row");
    let index = index
        .and_then(|index| i32::try_from(index).ok())
        .unwrap_or_default();
    app.global::<crate::Overlays>().set_context_index(index);
    index
}

#[test]
fn choose_disc_is_a_page_of_the_menu_that_back_returns_from() {
    use crate::context_page::Page;
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    let (_runtime, ctx) = offline_ctx();
    seat_visibility_list(&ctx, &app, GamesMode::Browse);
    {
        let mut shared = crate::router::lock(&ctx.shared);
        let row = &mut shared.games.rows[0];
        row.entry_type = zaparoo_app::media_list::EntryType::Directory;
        row.path = "/g/Game 0".into();
        row.zap_script = "@NES/Game 0 (disc:1)".into();
        row.multi_disc = true;
    }
    let overlays = app.global::<crate::Overlays>();
    crate::router::handle_action(&ctx, &app, "context_menu");
    let menu = context_ids(&app);

    // Accepting the row keeps the menu open while Core is asked.
    let opener = focus_context_row(&app, "choose_disc");
    crate::router::handle_action(&ctx, &app, "accept");
    settle(&window);
    assert!(overlays.get_context_open());
    let ticket = crate::context_page::ticket(&ctx);
    assert_ne!(ticket, 0, "the page was asked for");

    // An answer to a run the user moved on from fills nothing.
    crate::context_page::landed(
        &ctx,
        &app,
        Page::Discs,
        ticket.wrapping_sub(1),
        Ok(disc_rows()),
    );
    assert!(!crate::context_page::showing(&ctx));

    crate::context_page::landed(&ctx, &app, Page::Discs, ticket, Ok(disc_rows()));
    assert!(crate::context_page::showing(&ctx));
    let entries = overlays.get_context_entries();
    assert_eq!(entries.row_count(), 2);
    let second = entries
        .row_data(1)
        .map(|entry| (entry.label_key.to_string(), entry.label.to_string()));
    assert_eq!(second, Some(("disc".to_string(), "2".to_string())));
    assert_eq!(overlays.get_context_index(), 0);

    // Back returns to the menu, on the row that opened the page.
    crate::router::handle_action(&ctx, &app, "cancel");
    assert!(overlays.get_context_open() && !crate::context_page::showing(&ctx));
    assert_eq!(context_ids(&app), menu);
    assert_eq!(overlays.get_context_index(), opener);

    // A chosen disc closes the menu and launches.
    crate::router::handle_action(&ctx, &app, "accept");
    settle(&window);
    let ticket = crate::context_page::ticket(&ctx);
    crate::context_page::landed(&ctx, &app, Page::Discs, ticket, Ok(disc_rows()));
    crate::router::handle_action(&ctx, &app, "down");
    crate::router::handle_action(&ctx, &app, "accept");
    settle(&window);
    assert!(!overlays.get_context_open() && !crate::context_page::showing(&ctx));
}

#[test]
fn a_disc_list_that_fails_or_is_empty_never_strands_the_menu() {
    use crate::context_page::Page;
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    let (_runtime, ctx) = offline_ctx();
    seat_visibility_list(&ctx, &app, GamesMode::Browse);
    {
        let mut shared = crate::router::lock(&ctx.shared);
        let row = &mut shared.games.rows[0];
        row.entry_type = zaparoo_app::media_list::EntryType::Directory;
        row.zap_script = "@NES/Game 0 (disc:1)".into();
        row.multi_disc = true;
    }
    let overlays = app.global::<crate::Overlays>();
    let open_page = || {
        crate::router::handle_action(&ctx, &app, "context_menu");
        focus_context_row(&app, "choose_disc");
        crate::router::handle_action(&ctx, &app, "accept");
        settle(&window);
        crate::context_page::ticket(&ctx)
    };

    // Nothing to list: the row says so and can no longer be pressed.
    let ticket = open_page();
    crate::context_page::landed(&ctx, &app, Page::Discs, ticket, Ok(Vec::new()));
    assert!(overlays.get_context_open() && !crate::context_page::showing(&ctx));
    let keys: Vec<String> = overlays
        .get_context_entries()
        .iter()
        .map(|entry| entry.label_key.to_string())
        .collect();
    assert!(keys.iter().any(|key| key == "choose_disc:none"));
    assert!(!context_ids(&app).iter().any(|id| id == "choose_disc"));
    crate::router::handle_action(&ctx, &app, "cancel");
    assert!(!overlays.get_context_open());

    // A failure closes the menu under its alert.
    let ticket = open_page();
    crate::context_page::landed(&ctx, &app, Page::Discs, ticket, Err("offline".into()));
    assert!(!overlays.get_context_open());
    assert!(overlays.get_dialog_open());
    assert_eq!(overlays.get_dialog_error(), ErrorKind::DiscList);
}

fn version_rows() -> Vec<crate::context_page::PageRow> {
    ["US", "EU"]
        .into_iter()
        .map(|region| crate::context_page::PageRow {
            label_key: "",
            label: region.into(),
            name: "Game 0".into(),
            launch_text: format!("/g/Game 0 ({region}).nes"),
            deck: None,
        })
        .collect()
}

#[test]
fn a_search_result_with_versions_counts_them_and_lists_them_as_a_page() {
    use crate::context_page::Page;
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    let (_runtime, ctx) = offline_ctx();
    seat_visibility_list(&ctx, &app, GamesMode::Search);
    {
        let mut shared = crate::router::lock(&ctx.shared);
        let row = &mut shared.games.rows[0];
        row.title_id = Some(9);
        row.version_count = 3;
        // One version counts as none: the info line and the menu stay plain.
        let single = &mut shared.games.rows[1];
        single.title_id = Some(10);
        single.version_count = 1;
    }
    let view = app.global::<crate::GamesView>();
    let overlays = app.global::<crate::Overlays>();
    let focus = |index: usize| {
        crate::router::lock(&ctx.shared)
            .games
            .grid
            .set_current_index_immediate(index);
        crate::games::render(&ctx, &app);
    };

    // A title Core found once offers no versions.
    focus(1);
    assert_eq!(view.get_info().versions, 0);
    crate::router::handle_action(&ctx, &app, "context_menu");
    assert!(overlays.get_context_open());
    assert!(!context_ids(&app).iter().any(|id| id == "choose_version"));
    crate::router::handle_action(&ctx, &app, "cancel");
    assert!(!overlays.get_context_open());

    focus(0);
    assert_eq!(view.get_info().versions, 3);
    crate::router::handle_action(&ctx, &app, "context_menu");
    let menu = context_ids(&app);

    // Accept launches Core's pick; the menu lists every version.
    let opener = focus_context_row(&app, "choose_version");
    crate::router::handle_action(&ctx, &app, "accept");
    settle(&window);
    assert!(overlays.get_context_open());
    let ticket = crate::context_page::ticket(&ctx);
    crate::context_page::landed(&ctx, &app, Page::Versions, ticket, Ok(version_rows()));
    assert!(crate::context_page::showing(&ctx));
    assert_eq!(overlays.get_context_page(), crate::ContextPage::Versions);
    let labels: Vec<String> = overlays
        .get_context_entries()
        .iter()
        .map(|entry| entry.label.to_string())
        .collect();
    assert_eq!(labels, ["US", "EU"]);

    crate::router::handle_action(&ctx, &app, "cancel");
    assert!(overlays.get_context_open() && !crate::context_page::showing(&ctx));
    assert_eq!(context_ids(&app), menu);
    assert_eq!(overlays.get_context_index(), opener);

    // A chosen version closes the menu and launches.
    crate::router::handle_action(&ctx, &app, "accept");
    settle(&window);
    let ticket = crate::context_page::ticket(&ctx);
    crate::context_page::landed(&ctx, &app, Page::Versions, ticket, Ok(version_rows()));
    crate::router::handle_action(&ctx, &app, "accept");
    settle(&window);
    assert!(!overlays.get_context_open() && !crate::context_page::showing(&ctx));

    // A list Core cannot give closes the menu under its alert.
    crate::router::handle_action(&ctx, &app, "context_menu");
    focus_context_row(&app, "choose_version");
    crate::router::handle_action(&ctx, &app, "accept");
    settle(&window);
    let ticket = crate::context_page::ticket(&ctx);
    crate::context_page::landed(&ctx, &app, Page::Versions, ticket, Err("offline".into()));
    assert!(!overlays.get_context_open());
    assert!(overlays.get_dialog_open());
    assert_eq!(overlays.get_dialog_error(), ErrorKind::VersionList);
    crate::router::handle_action(&ctx, &app, "cancel");
    settle(&window);
    assert!(!overlays.get_dialog_open());
}

/// Which of the strip's four buttons are on, in its order.
fn strip_states(app: &App) -> Vec<bool> {
    app.global::<crate::Overlays>()
        .get_context_strip()
        .iter()
        .map(|button| button.on)
        .collect()
}

fn context_keys(app: &App) -> Vec<String> {
    app.global::<crate::Overlays>()
        .get_context_entries()
        .iter()
        .map(|entry| entry.label_key.to_string())
        .collect()
}

#[test]
fn manage_and_write_to_token_are_pages_that_back_returns_from() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    let (_runtime, ctx) = offline_ctx();
    seat_visibility_list(&ctx, &app, GamesMode::Browse);
    let overlays = app.global::<crate::Overlays>();

    // No reader: one way to write, so the row runs it instead of
    // offering a page with a single choice.
    crate::router::handle_action(&ctx, &app, "context_menu");
    assert_eq!(
        context_ids(&app),
        ["more_info", "add_to_deck", "write_token", "manage_game"]
    );
    assert_eq!(overlays.get_context_page(), crate::ContextPage::Root);
    let write = overlays.get_context_entries().row_data(2);
    assert_eq!(
        write.map(|entry| entry.role),
        Some(crate::MenuRole::Option),
        "a row that opens no page draws no chevron"
    );
    focus_context_row(&app, "write_token");
    crate::router::handle_action(&ctx, &app, "accept");
    settle(&window);
    assert!(!overlays.get_context_open());
    assert!(overlays.get_qr_open(), "the App workflow opened directly");
    overlays.set_qr_open(false);

    // A reader makes it a choice, on a page of the same menu.
    crate::router::lock(&ctx.shared).has_nfc = true;
    crate::router::handle_action(&ctx, &app, "context_menu");
    let root = context_ids(&app);
    let opener = focus_context_row(&app, "write_token");
    let write = overlays.get_context_entries().row_data(2);
    assert_eq!(
        write.map(|entry| entry.role),
        Some(crate::MenuRole::Submenu)
    );
    crate::router::handle_action(&ctx, &app, "accept");
    settle(&window);
    assert!(overlays.get_context_open() && crate::context_page::showing(&ctx));
    assert_eq!(overlays.get_context_page(), crate::ContextPage::WriteToken);
    assert_eq!(context_ids(&app), ["write_card", "qr_code"]);
    assert_eq!(overlays.get_context_index(), 0);
    crate::router::handle_action(&ctx, &app, "cancel");
    assert!(overlays.get_context_open() && !crate::context_page::showing(&ctx));
    assert_eq!(overlays.get_context_page(), crate::ContextPage::Root);
    assert_eq!(context_ids(&app), root);
    assert_eq!(overlays.get_context_index(), opener);

    // Manage holds the rest, and its rows run and close like any other.
    let opener = focus_context_row(&app, "manage_game");
    crate::router::handle_action(&ctx, &app, "accept");
    settle(&window);
    assert_eq!(overlays.get_context_page(), crate::ContextPage::ManageGame);
    assert_eq!(
        context_ids(&app),
        ["add_to_hub", "toggle_hidden", "scrape_game"]
    );
    crate::router::handle_action(&ctx, &app, "cancel");
    assert_eq!(overlays.get_context_index(), opener);
    crate::router::handle_action(&ctx, &app, "accept");
    settle(&window);
    focus_context_row(&app, "scrape_game");
    crate::router::handle_action(&ctx, &app, "accept");
    settle(&window);
    assert!(!overlays.get_context_open() && !crate::context_page::showing(&ctx));
    assert_eq!(overlays.get_context_page(), crate::ContextPage::Root);
    assert!(app.global::<crate::SetupModalView>().get_open());
}

#[test]
fn a_game_reads_the_same_on_every_list() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    let (_runtime, ctx) = offline_ctx();
    let mut menus = Vec::new();
    for mode in [
        GamesMode::Browse,
        GamesMode::Favorites,
        GamesMode::Recents,
        GamesMode::Search,
    ] {
        seat_visibility_list(&ctx, &app, mode);
        {
            let mut shared = crate::router::lock(&ctx.shared);
            shared.launchers = vec![zaparoo_core::media_types::LauncherInfo {
                id: "nes".into(),
                system_id: "NES".into(),
                ..Default::default()
            }];
        }
        crate::router::handle_action(&ctx, &app, "context_menu");
        let root = context_keys(&app);
        focus_context_row(&app, "manage_game");
        crate::router::handle_action(&ctx, &app, "accept");
        settle(&window);
        menus.push((mode, root, context_keys(&app)));
        crate::router::close_context_menu(&ctx, &app);
    }
    let (_, root, manage) = menus[0].clone();
    assert_eq!(
        root,
        ["more_info", "add_to_deck", "write_token", "manage_game"]
    );
    assert_eq!(
        manage,
        ["change_launcher", "add_to_hub", "hide:game", "scrape_game"]
    );
    for (mode, other_root, other_manage) in &menus[1..] {
        assert_eq!(other_root, &root, "{mode:?}");
        assert_eq!(other_manage, &manage, "{mode:?}");
    }
}

#[test]
fn the_collections_strip_sits_above_the_rows_and_toggles_in_place() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, _window) = boot();
    let (_runtime, ctx) = offline_ctx();
    seat_visibility_list(&ctx, &app, GamesMode::Browse);
    crate::router::lock(&ctx.shared).games.rows[0].flags = zaparoo_app::user_flags::Flags {
        liked: true,
        ..Default::default()
    };
    let overlays = app.global::<crate::Overlays>();
    crate::router::handle_action(&ctx, &app, "context_menu");
    assert_eq!(strip_states(&app), [false, true, false, false]);
    // The menu opens on its first row, not on the strip.
    assert_eq!(overlays.get_context_index(), 0);
    assert_eq!(overlays.get_context_strip_index(), -1);

    // Up from the first row reaches the strip; Left stops at its end.
    crate::router::handle_action(&ctx, &app, "up");
    assert_eq!(overlays.get_context_strip_index(), 0);
    assert_eq!(overlays.get_context_index(), -1);
    crate::router::handle_action(&ctx, &app, "left");
    assert_eq!(overlays.get_context_strip_index(), 0);
    crate::router::handle_action(&ctx, &app, "right");
    crate::router::handle_action(&ctx, &app, "right");
    assert_eq!(overlays.get_context_strip_index(), 2);

    // Dislike drops the Like at once, on the strip and on the row, and
    // the menu stays open on the same button.
    crate::router::handle_action(&ctx, &app, "accept");
    assert!(overlays.get_context_open());
    assert_eq!(overlays.get_context_strip_index(), 2);
    assert_eq!(strip_states(&app), [false, false, true, false]);
    assert!(
        crate::router::lock(&ctx.shared).games.rows[0]
            .flags
            .disliked
    );

    // Down returns to the first row; Up from the strip wraps to the last.
    crate::router::handle_action(&ctx, &app, "down");
    assert_eq!(overlays.get_context_strip_index(), -1);
    assert_eq!(overlays.get_context_index(), 0);
    crate::router::handle_action(&ctx, &app, "up");
    crate::router::handle_action(&ctx, &app, "up");
    assert_eq!(overlays.get_context_strip_index(), -1);
    assert_eq!(
        overlays.get_context_index(),
        i32::try_from(context_ids(&app).len() - 1).unwrap_or_default()
    );
    // Down from the last row comes back around to the strip, on the
    // button it left.
    crate::router::handle_action(&ctx, &app, "down");
    assert_eq!(overlays.get_context_strip_index(), 2);

    // A page hides the strip and keeps focus on its rows.
    crate::router::handle_action(&ctx, &app, "down");
    focus_context_row(&app, "manage_game");
    crate::router::handle_action(&ctx, &app, "accept");
    crate::router::handle_action(&ctx, &app, "up");
    assert_eq!(overlays.get_context_strip_index(), -1);
    crate::router::close_context_menu(&ctx, &app);
    assert_eq!(overlays.get_context_strip().row_count(), 0);
}

#[test]
fn add_to_deck_checks_in_place_and_new_deck_names_one_on_the_keyboard() {
    use crate::context_page::Page;
    use zaparoo_core::collections::Deck;
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    let (_runtime, ctx) = offline_ctx();
    seat_visibility_list(&ctx, &app, GamesMode::Favorites);
    let overlays = app.global::<crate::Overlays>();
    crate::router::handle_action(&ctx, &app, "context_menu");
    focus_context_row(&app, "add_to_deck");
    crate::router::handle_action(&ctx, &app, "accept");
    settle(&window);
    let deck = |id: &str, owned: bool, holds: &[i64], count: u32| Deck {
        deck_id: id.into(),
        name: id.into(),
        owned,
        item_count: count,
        contains_item_ids: Some(holds.to_vec()),
        ..Deck::default()
    };
    let decks = [
        deck("Couch", true, &[5], 3),
        deck("Copy", false, &[], 3),
        deck("Full", true, &[], zaparoo_core::collections::MAX_DECK_ITEMS),
        deck("Weekend", true, &[], 0),
    ];
    let ticket = crate::context_page::ticket(&ctx);
    crate::context_page::landed(
        &ctx,
        &app,
        Page::AddToDeck,
        ticket,
        Ok(crate::add_to_deck::rows(&decks)),
    );
    settle(&window);
    assert_eq!(overlays.get_context_page(), crate::ContextPage::AddToDeck);
    let roles: Vec<_> = overlays
        .get_context_entries()
        .iter()
        .map(|entry| (entry.label.to_string(), entry.role, entry.enabled))
        .collect();
    assert_eq!(
        roles,
        [
            ("Couch".into(), crate::MenuRole::CheckOn, true),
            ("Full".into(), crate::MenuRole::CheckOff, false),
            ("Weekend".into(), crate::MenuRole::CheckOff, true),
            (String::new(), crate::MenuRole::Option, true),
        ]
    );
    // Accept checks the deck at once and the page stays up.
    overlays.set_context_index(2);
    crate::router::handle_action(&ctx, &app, "accept");
    settle(&window);
    assert!(overlays.get_context_open());
    assert_eq!(
        overlays.get_context_entries().row_data(2).map(|e| e.role),
        Some(crate::MenuRole::CheckOn)
    );
    // A full deck does nothing.
    overlays.set_context_index(1);
    crate::router::handle_action(&ctx, &app, "accept");
    settle(&window);
    assert_eq!(
        overlays.get_context_entries().row_data(1).map(|e| e.role),
        Some(crate::MenuRole::CheckOff)
    );

    // New deck names the deck on the keyboard, and Back returns to the
    // list the menu was opened on.
    overlays.set_context_index(3);
    crate::router::handle_action(&ctx, &app, "accept");
    settle(&window);
    assert!(!overlays.get_context_open());
    assert_eq!(app.global::<Shell>().get_active_screen(), Screen::Rename);
    assert_eq!(
        app.global::<crate::RenameView>().get_purpose(),
        crate::RenamePurpose::NewDeck
    );
    assert!(!app.global::<crate::RenameView>().get_can_save());
    // The name and the game it is for are kept, so a kill while naming
    // comes back to them.
    {
        let shared = crate::router::lock(&ctx.shared);
        assert_eq!(shared.persist.active_screen, Screen::Rename.token());
        assert_eq!(shared.persist.rename.purpose, "new_deck");
        assert!(!shared.persist.rename.deck_path.is_empty());
    }
    crate::router::handle_action(&ctx, &app, "cancel");
    assert_eq!(app.global::<Shell>().get_active_screen(), Screen::Favorites);
    {
        let shared = crate::router::lock(&ctx.shared);
        assert_eq!(shared.persist.active_screen, Screen::Favorites.token());
        assert_eq!(shared.persist.rename.purpose, "");
    }
}

#[test]
fn a_new_deck_name_comes_back_after_a_kill() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    let (_runtime, ctx) = offline_ctx();
    {
        let mut shared = crate::router::lock(&ctx.shared);
        shared.persist.active_screen = Screen::Rename.token().into();
        shared.persist.rename = zaparoo_core::persist::RenameState {
            purpose: "new_deck".into(),
            deck_system: "SNES".into(),
            deck_path: "/snes/mario.sfc".into(),
            draft: "Couch".into(),
            caret: 5,
            ..zaparoo_core::persist::RenameState::default()
        };
    }
    crate::rename::restore(&ctx, &app);
    settle(&window);
    assert_eq!(app.global::<Shell>().get_active_screen(), Screen::Rename);
    let view = app.global::<crate::RenameView>();
    assert_eq!(view.get_purpose(), crate::RenamePurpose::NewDeck);
    assert_eq!(view.get_before(), "Couch");
    assert!(view.get_can_save());
    // Without the screen it was opened on behind it, Back goes to the Hub.
    crate::router::handle_action(&ctx, &app, "cancel");
    settle(&window);
    assert_eq!(app.global::<Shell>().get_active_screen(), Screen::Hub);
    assert_eq!(crate::router::lock(&ctx.shared).persist.rename.purpose, "");
}

#[test]
fn history_core_cannot_resolve_offers_no_state_it_does_not_know() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    let (_runtime, ctx) = offline_ctx();
    seat_visibility_list(&ctx, &app, GamesMode::Recents);
    {
        let mut shared = crate::router::lock(&ctx.shared);
        let row = &mut shared.games.rows[0];
        row.media_id = None;
        row.tags_known = false;
    }
    crate::router::handle_action(&ctx, &app, "context_menu");
    assert_eq!(
        context_ids(&app),
        ["more_info", "write_token", "manage_game"]
    );
    focus_context_row(&app, "manage_game");
    crate::router::handle_action(&ctx, &app, "accept");
    settle(&window);
    assert_eq!(context_ids(&app), ["add_to_hub", "scrape_game"]);
}

fn hub_item(kind: &str, id: &str) -> zaparoo_core::hub_layout::HubItem {
    zaparoo_core::hub_layout::HubItem {
        kind_raw: kind.into(),
        id: id.into(),
        ..Default::default()
    }
}

/// A Hub holding one of each tile the Options menu tells apart: Resume, a
/// pinned game, a hand-written script, a system, a folder and a search.
fn seed_hub_tiles(ctx: &crate::router::Ctx, app: &App) {
    use zaparoo_core::hub_layout::HubItem;
    seed_hub_pages(ctx, app);
    {
        let mut shared = crate::router::lock(&ctx.shared);
        shared.systems = navigation_catalog();
        shared.hub.layout.items = vec![
            hub_item("action", "resume"),
            HubItem {
                path: "/g/Pinned.nes".into(),
                script: "@NES/Pinned".into(),
                name: "Pinned".into(),
                system: "NES".into(),
                ..hub_item("zapscript", "")
            },
            HubItem {
                script: "**input.keyboard:{f12}".into(),
                name: "Menu".into(),
                ..hub_item("zapscript", "")
            },
            hub_item("system", "System08"),
            HubItem {
                path: "/g/NES/Homebrew".into(),
                system: "NES".into(),
                ..hub_item("folder", "")
            },
            HubItem {
                name: "Mario".into(),
                query: "mario".into(),
                ..hub_item("search", "")
            },
        ];
        shared.hub.resume = crate::hub::Resume {
            requested: true,
            loading: false,
            entry: Some(zaparoo_core::media_types::MediaHistoryLatestEntry {
                system_id: "NES".into(),
                media_name: "Game".into(),
                media_path: "/g/Game.nes".into(),
                ..Default::default()
            }),
        };
    }
    crate::hub::rebuild(ctx, app);
}

fn favorite_meta(path: &str) -> zaparoo_core::media_types::MediaMeta {
    zaparoo_core::media_types::MediaMeta {
        path: path.into(),
        zap_script: "@NES/Game".into(),
        tags: vec![zaparoo_core::media_types::TagInfo {
            tag: "favorite".into(),
            tag_type: "user".into(),
            ..Default::default()
        }],
        ..Default::default()
    }
}

#[test]
fn the_resume_tile_shows_its_games_menu_once_core_says_what_it_is() {
    use zaparoo_app::options_menu::Tile;
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    let (_runtime, ctx) = offline_ctx();
    seed_hub_tiles(&ctx, &app);
    settle(&window);
    let overlays = app.global::<crate::Overlays>();
    let resume = || crate::hub::pending_game_request(&ctx, Tile::Resume, "NES", "/g/Game.nes");
    let meta = favorite_meta("/g/Game.nes");

    // The tile holds no state of the game's: nothing opens until Core
    // answers, and an answer the user moved on from opens nothing.
    crate::router::handle_action(&ctx, &app, "context_menu");
    assert!(!overlays.get_context_open());
    let stale = resume();
    crate::router::handle_action(&ctx, &app, "right");
    crate::router::handle_action(&ctx, &app, "left");
    crate::hub::game_answered(&ctx, &app, &stale, Some(&meta));
    assert!(!overlays.get_context_open());

    crate::router::handle_action(&ctx, &app, "context_menu");
    crate::hub::game_answered(&ctx, &app, &resume(), Some(&meta));
    assert!(overlays.get_context_open());
    assert_eq!(
        context_keys(&app),
        [
            "more_info",
            "add_to_deck",
            "write_token",
            "edit_tile",
            "manage_game"
        ]
    );
    assert!(strip_states(&app)[0], "the strip shows the game's favorite");
    focus_context_row(&app, "manage_game");
    crate::router::handle_action(&ctx, &app, "accept");
    settle(&window);
    assert_eq!(
        context_keys(&app),
        ["add_to_hub", "hide:game", "scrape_game"]
    );
    crate::router::handle_action(&ctx, &app, "cancel");
    focus_context_row(&app, "edit_tile");
    crate::router::handle_action(&ctx, &app, "accept");
    settle(&window);
    assert_eq!(overlays.get_context_page(), crate::ContextPage::EditTile);
    assert_eq!(
        context_keys(&app),
        [
            "hub_rename:tile",
            "hub_icon:tile",
            "hub_move:tile",
            "hub:remove"
        ]
    );

    // A newer game arriving in history cannot redirect the open menu.
    let mut newer = crate::router::lock(&ctx.shared).hub.resume.clone();
    if let Some(entry) = newer.entry.as_mut() {
        entry.media_path = "/g/Other.nes".into();
    }
    crate::hub::set_resume(&ctx, &app, newer);
    let captured = match &crate::router::lock(&ctx.shared).hub.menu_target {
        crate::hub::MenuTarget::Game { row, .. } => row.path.clone(),
        _ => String::new(),
    };
    assert_eq!(captured, "/g/Game.nes");

    // The tile's own actions are still there, on Edit tile.
    focus_context_row(&app, "hub_move");
    crate::router::handle_action(&ctx, &app, "accept");
    settle(&window);
    assert!(!overlays.get_context_open());
    assert!(crate::router::lock(&ctx.shared).hub.move_armed());
    crate::router::handle_action(&ctx, &app, "cancel");
    settle(&window);
    // Back from the move brings the menu back on its row; two more leave it.
    assert!(overlays.get_context_open());
    assert_eq!(context_row_id(&app).as_deref(), Some("hub_move"));
    crate::router::handle_action(&ctx, &app, "cancel");
    crate::router::handle_action(&ctx, &app, "cancel");
    assert!(!overlays.get_context_open());

    // A game Core no longer has leaves the tile its layout actions.
    crate::router::handle_action(&ctx, &app, "context_menu");
    let request = crate::hub::pending_game_request(&ctx, Tile::Resume, "NES", "/g/Other.nes");
    crate::hub::game_answered(&ctx, &app, &request, None);
    assert_eq!(
        context_keys(&app),
        [
            "hub_rename:tile",
            "hub_icon:tile",
            "hub_move:tile",
            "hub:remove"
        ]
    );
}

#[test]
#[allow(
    clippy::too_many_lines,
    reason = "one walk over every pinned tile kind and its menu"
)]
fn pinned_tiles_show_what_they_stand_for_or_their_layout_actions() {
    use zaparoo_app::options_menu::Tile;
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    let (_runtime, ctx) = offline_ctx();
    seed_hub_tiles(&ctx, &app);
    settle(&window);
    let overlays = app.global::<crate::Overlays>();
    let tile = Cell::new(0);
    let next_tile = || {
        crate::router::close_context_menu(&ctx, &app);
        tile.set(tile.get() + 1);
        crate::router::lock(&ctx.shared)
            .hub
            .grid
            .set_current_index_immediate(tile.get());
        crate::router::handle_action(&ctx, &app, "context_menu");
    };

    // A pinned game: its own menu, with the shortcut's actions on Edit
    // tile.
    next_tile();
    assert!(!overlays.get_context_open());
    let request = crate::hub::pending_game_request(&ctx, Tile::Pinned, "NES", "/g/Pinned.nes");
    crate::hub::game_answered(&ctx, &app, &request, Some(&favorite_meta("/g/Pinned.nes")));
    assert_eq!(
        context_keys(&app),
        [
            "more_info",
            "add_to_deck",
            "write_token",
            "edit_tile",
            "manage_game"
        ]
    );
    assert!(strip_states(&app)[0], "the strip shows the game's favorite");
    focus_context_row(&app, "edit_tile");
    crate::router::handle_action(&ctx, &app, "accept");
    settle(&window);
    assert_eq!(
        context_keys(&app),
        [
            "hub_rename:tile",
            "hub_icon:tile",
            "hub_move:tile",
            "hub:remove"
        ]
    );
    crate::router::handle_action(&ctx, &app, "cancel");
    focus_context_row(&app, "manage_game");
    crate::router::handle_action(&ctx, &app, "accept");
    settle(&window);
    assert_eq!(context_keys(&app), ["hide:game", "scrape_game"]);

    // A script that is not one game: it can be written as it is.
    next_tile();
    assert_eq!(
        context_keys(&app),
        [
            "write_token",
            "hub_rename:tile",
            "hub_icon:tile",
            "hub_move:tile",
            "hub:remove"
        ]
    );
    focus_context_row(&app, "write_token");
    crate::router::handle_action(&ctx, &app, "accept");
    settle(&window);
    assert!(overlays.get_qr_open());
    overlays.set_qr_open(false);

    // A pinned system: the system's menu, the tile's rows on Edit tile.
    next_tile();
    assert_eq!(
        context_keys(&app),
        [
            "launch_system",
            "launch_random_system",
            "edit_tile",
            "manage_system"
        ]
    );
    focus_context_row(&app, "edit_tile");
    crate::router::handle_action(&ctx, &app, "accept");
    settle(&window);
    assert_eq!(overlays.get_context_page(), crate::ContextPage::EditTile);
    assert_eq!(
        context_keys(&app),
        [
            "hub_rename:tile",
            "hub_icon:tile",
            "hub_move:tile",
            "hub:remove"
        ]
    );
    crate::router::handle_action(&ctx, &app, "cancel");
    assert_eq!(context_row_id(&app).as_deref(), Some("edit_tile"));
    focus_context_row(&app, "manage_system");
    crate::router::handle_action(&ctx, &app, "accept");
    settle(&window);
    assert_eq!(
        overlays.get_context_page(),
        crate::ContextPage::ManageSystem
    );
    assert_eq!(
        context_keys(&app),
        ["artwork", "hide:system", "index_system", "scrape_system"]
    );

    // A folder's visibility is not something the tile knows.
    next_tile();
    assert_eq!(
        context_keys(&app),
        [
            "hub_rename:tile",
            "hub_icon:tile",
            "hub_move:tile",
            "hub:remove"
        ]
    );

    // A saved search is neither a game nor a script to write.
    next_tile();
    assert_eq!(
        context_keys(&app),
        [
            "hub_rename:tile",
            "hub_icon:tile",
            "hub_move:tile",
            "hub:remove"
        ]
    );

    // Removing acts on the tile the menu opened on.
    focus_context_row(&app, "hub_remove");
    crate::router::handle_action(&ctx, &app, "accept");
    settle(&window);
    let shared = crate::router::lock(&ctx.shared);
    assert!(shared
        .hub
        .layout
        .visible()
        .all(|item| item.kind_raw != "search"));
}

#[test]
fn systems_and_favorite_systems_show_the_same_system_the_same_way() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    let (_runtime, ctx) = offline_ctx();
    {
        let mut shared = crate::router::lock(&ctx.shared);
        let mut catalog = navigation_catalog();
        catalog.truncate(1);
        catalog.push(zaparoo_core::media_types::SystemInfo {
            id: "dvd".into(),
            name: "DVD Player".into(),
            category: "Console".into(),
            zap_script: "zaparoo://launch/dvd".into(),
            ..Default::default()
        });
        shared.systems = catalog;
        shared.categories = vec!["Console".into()];
        shared.hub.layout.items.clear();
    }
    crate::systems::enter(&ctx, &app, "Console", EntryMode::Fresh, false);

    // Rows sort by name: the launch-only system first. Accept launches
    // it, so its menu neither repeats that nor needs a page.
    crate::router::handle_action(&ctx, &app, "context_menu");
    assert_eq!(context_keys(&app), ["add_to_hub", "hide:system"]);
    crate::router::close_context_menu(&ctx, &app);

    crate::router::handle_action(&ctx, &app, "right");
    crate::router::handle_action(&ctx, &app, "context_menu");
    assert_eq!(
        context_keys(&app),
        ["launch_system", "launch_random_system", "manage_system"]
    );
    focus_context_row(&app, "manage_system");
    crate::router::handle_action(&ctx, &app, "accept");
    settle(&window);
    let manage = context_keys(&app);
    assert_eq!(
        manage,
        [
            "artwork",
            "add_to_hub",
            "hide:system",
            "index_system",
            "scrape_system"
        ]
    );
    crate::router::close_context_menu(&ctx, &app);

    // The same system among the favorite systems: random play stays in
    // its favorites, everything else is the system's own.
    crate::router::lock(&ctx.shared).systems_model.mode = SystemsMode::Favorites;
    crate::router::handle_action(&ctx, &app, "context_menu");
    assert_eq!(
        context_keys(&app),
        ["launch_system", "random_favorite", "manage_system"]
    );
    focus_context_row(&app, "manage_system");
    crate::router::handle_action(&ctx, &app, "accept");
    settle(&window);
    assert_eq!(context_keys(&app), manage);
}

#[test]
fn hiding_restarts_browse_and_letters_without_losing_favorites_or_neighbor_focus() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, _window) = boot();
    let (_runtime, ctx) = offline_ctx();
    let target = seat_visibility_list(&ctx, &app, GamesMode::Browse);
    crate::games::on_hidden_updated(&ctx, &app, 0, &target, "NES", Ok(visibility_reply(true)));
    let shared = crate::router::lock(&ctx.shared);
    assert!(shared.games.rows[0].is_hidden);
    assert!(shared.games.rows[0].flags.favorite);
    assert_eq!(shared.persist.games.selected_at_level[0], "/g/Game 1.nes");
    assert!(shared.games.loading);
    assert_eq!(shared.games.ticket, 1);
    assert!(shared.games.next_cursor.is_none());
    assert!(shared.letter_scope.is_none());
    assert!(shared.letter_buckets.is_empty());
    assert_eq!(shared.letter_seq, 1);
    assert_eq!(app.global::<Shell>().get_active_screen(), Screen::Games);
}

#[test]
fn hidden_management_keeps_favorites_recents_and_recovery_selection() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, _window) = boot();
    let (_runtime, ctx) = offline_ctx();
    for mode in [GamesMode::Browse, GamesMode::Favorites, GamesMode::Recents] {
        let target = seat_visibility_list(&ctx, &app, mode);
        {
            let mut shared = crate::router::lock(&ctx.shared);
            shared.show_hidden = mode == GamesMode::Browse;
        }
        let ticket = crate::router::lock(&ctx.shared).games.ticket;
        crate::games::on_hidden_updated(
            &ctx,
            &app,
            ticket,
            &target,
            "NES",
            Ok(visibility_reply(true)),
        );
        let shared = crate::router::lock(&ctx.shared);
        assert!(shared.games.rows[0].is_hidden);
        let selected = match mode {
            GamesMode::Browse => &shared.persist.games.selected_at_level[0],
            GamesMode::Favorites => &shared.persist.favorites.selected_path,
            GamesMode::Recents => &shared.persist.recents.selected_path,
            GamesMode::Search => &shared.persist.search.selected_path,
            GamesMode::Deck => &shared.persist.deck.selected_item,
            GamesMode::Decks => &shared.persist.decks.selected_deck,
            GamesMode::TopPlayed => &shared.persist.top_played.selected_path,
        };
        assert_eq!(selected, &target.path);
        let ticket = shared.games.ticket;
        drop(shared);
        crate::games::on_hidden_updated(
            &ctx,
            &app,
            ticket,
            &target,
            "NES",
            Ok(visibility_reply(false)),
        );
        assert!(!crate::router::lock(&ctx.shared).games.rows[0].is_hidden);
    }
}

#[test]
fn visibility_errors_and_stale_completions_do_not_replace_the_visible_list() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, _window) = boot();
    let (runtime, ctx) = offline_ctx();
    let target = seat_visibility_list(&ctx, &app, GamesMode::Browse);
    let failure = runtime.block_on(
        ctx.store
            .client()
            .media_tags_update(zaparoo_core::media_types::MediaTagsUpdateParams::default()),
    );
    assert!(failure.is_err());
    crate::games::on_hidden_updated(&ctx, &app, 0, &target, "NES", failure);
    assert_eq!(
        app.global::<crate::Overlays>().get_dialog_error(),
        ErrorKind::MediaVisibility
    );
    assert!(!crate::router::lock(&ctx.shared).games.rows[0].is_hidden);
    assert_eq!(
        crate::router::lock(&ctx.shared)
            .games
            .next_cursor
            .as_deref(),
        Some("old-visibility-cursor")
    );
    crate::router::lock(&ctx.shared).games.ticket = 1;
    crate::games::on_hidden_updated(&ctx, &app, 0, &target, "NES", Ok(visibility_reply(true)));
    assert!(!crate::router::lock(&ctx.shared).games.rows[0].is_hidden);
    app.global::<Shell>().set_active_screen(Screen::Settings);
    crate::games::on_hidden_updated(&ctx, &app, 1, &target, "NES", Ok(visibility_reply(true)));
    assert_eq!(app.global::<Shell>().get_active_screen(), Screen::Settings);
    assert!(!crate::router::lock(&ctx.shared).games.loading);
    assert!(!crate::router::lock(&ctx.shared).games.rows[0].is_hidden);
}

#[test]
#[allow(clippy::expect_used, reason = "isolated settings fixture")]
fn show_hidden_setting_retires_letter_scope_without_routing_out_of_settings() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, _window) = boot();
    let (_runtime, mut ctx) = offline_ctx();
    seat_visibility_list(&ctx, &app, GamesMode::Browse);
    ctx.config_path =
        std::path::PathBuf::from(std::env::var("ZAPAROO_STATE_FILE").expect("isolated state"))
            .with_file_name("frontend.toml");
    app.global::<Shell>().set_active_screen(Screen::Settings);
    let view = app.global::<crate::SettingsView>();
    view.set_page(SettingsPage::Browsing);
    let rows = zaparoo_app::settings::page_rows(
        SettingsPage::Browsing.token(),
        &crate::settings::inputs(&ctx),
    );
    view.set_index(
        rows.iter()
            .position(|row| row.id() == "showHidden")
            .expect("show hidden row") as i32,
    );
    crate::settings::handle_action(&ctx, &app, "accept");
    let shared = crate::router::lock(&ctx.shared);
    assert!(shared.show_hidden);
    assert!(shared.persist.settings.show_hidden);
    assert!(shared.letter_scope.is_none());
    assert!(shared.letter_buckets.is_empty());
    assert_eq!(shared.games.ticket, 0);
    assert_eq!(app.global::<Shell>().get_active_screen(), Screen::Settings);
}

fn seat_folder(ctx: &crate::router::Ctx, app: &App, name: &str, index: usize) {
    let mut shared = crate::router::lock(&ctx.shared);
    shared.games.rows = game_rows(name, 8).into();
    shared.games.grid.set_item_count(8);
    shared.games.grid.set_current_index_immediate(index);
    drop(shared);
    crate::games::render(ctx, app);
}

#[test]
fn missing_browse_colors_use_real_previews_on_every_page_before_full_art() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    let (_runtime, ctx) = offline_ctx();
    crate::theme::apply_palette(&app, "zaparoo-dark", "normal");
    crate::sizing::apply_scene(
        &app,
        crate::sizing::Scene::of(&app, f64::from(W), f64::from(H), false),
    );
    app.global::<Shell>().set_active_screen(Screen::Games);
    let rows: Vec<_> = (500..560)
        .map(|index| {
            crate::games::GameRow::from(&zaparoo_core::media_types::BrowseEntry {
                media_id: Some(index),
                name: format!("Game {index}"),
                path: format!("/SNES/{index}"),
                system_id: "SNES".into(),
                entry_type: "media".into(),
                has_cover: true,
                ..Default::default()
            })
        })
        .collect();
    let ticket = crate::router::lock(&ctx.shared).games.ticket;
    crate::games::apply_fill(&ctx, &app, ticket, rows.clone(), None, Some((60, 0)), false);
    let view = app.global::<crate::GamesView>();
    let original = view.get_cells();
    assert!(original
        .iter()
        .all(|cell| !cell.has_placeholder && !cell.has_cover));
    let page_size = crate::router::lock(&ctx.shared).games.grid.page_size();
    let pending = ctx.media.pending_keys();
    assert!(pending[..page_size]
        .iter()
        .all(|key| key.max_size == zaparoo_app::covers::COLOR_PREVIEW_MAX_SIZE));
    assert!(pending[page_size..page_size * 2]
        .iter()
        .all(|key| key.max_size > zaparoo_app::covers::COLOR_PREVIEW_MAX_SIZE));
    let keys: Vec<_> = rows
        .iter()
        .map(|row| crate::media_cache::MediaKey {
            media_id: row.media_id,
            system: row.system_id.clone(),
            path: row.path.clone(),
            max_size: zaparoo_app::covers::COLOR_PREVIEW_MAX_SIZE,
            fit: zaparoo_app::covers::Fit::SOURCE,
            folder: false,
            image_type: None,
            prefer: None,
        })
        .collect();
    for key in &keys {
        ctx.media.seed(
            key.clone(),
            crate::media_cache::DecodedImage {
                buffer: slint::SharedPixelBuffer::clone_from_slice(&[255_u8, 0, 255, 255], 1, 1),
            },
        );
    }
    crate::deliver_covers(&ctx, &app, &keys);
    assert_eq!(
        view.get_cells(),
        original,
        "preview colors patch retained delegates"
    );
    for page in 0..3 {
        if page > 0 {
            crate::router::handle_action(&ctx, &app, "page_next");
        }
        settle(&window);
        assert_eq!(view.get_page(), page);
        assert!(view
            .get_cells()
            .iter()
            .all(|cell| cell.has_placeholder && !cell.has_cover));
        assert!(
            pixels(&window)
                .iter()
                .filter(|pixel| pixel.0 == 0xf81f)
                .count()
                > 20
        );
    }
    crate::router::lock(&ctx.shared).games.rapid.active = true;
    crate::games::render(&ctx, &app);
    assert!(
        ctx.media.pending_keys().is_empty(),
        "rapid navigation starts no artwork work"
    );
    assert!(
        view.get_cells().iter().all(|cell| cell.has_placeholder),
        "prepared colors survive rapid navigation"
    );
    assert!(
        crate::router::lock(&ctx.shared)
            .games
            .rows
            .iter()
            .all(|row| row.cover_color.is_none()),
        "browse metadata and persisted state are not rewritten"
    );
}

#[test]
fn cover_colors_survive_appended_pages_and_warm_to_cold_tile_replacement() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    let (_runtime, ctx) = offline_ctx();
    crate::theme::apply_palette(&app, "zaparoo-dark", "normal");
    crate::sizing::apply_scene(
        &app,
        crate::sizing::Scene::of(&app, f64::from(W), f64::from(H), false),
    );
    app.global::<Shell>().set_active_screen(Screen::Games);
    let rows: Vec<_> = (0..60)
        .map(|index| {
            crate::games::GameRow::from(&zaparoo_core::media_types::BrowseEntry {
                media_id: Some(index),
                name: format!("Game {index}"),
                path: format!("/SNES/{index}"),
                system_id: "SNES".into(),
                entry_type: "media".into(),
                has_cover: true,
                cover_color: Some("#ff00ff".into()),
                ..Default::default()
            })
        })
        .collect();
    let ticket = crate::router::lock(&ctx.shared).games.ticket;
    crate::games::apply_fill(
        &ctx,
        &app,
        ticket,
        rows[..30].to_vec(),
        Some("next".into()),
        Some((60, 0)),
        false,
    );
    crate::games::on_append(&ctx, &app, ticket, Ok((rows[30..].to_vec(), None)));
    let view = app.global::<crate::GamesView>();
    let colored = || {
        pixels(&window)
            .iter()
            .filter(|pixel| pixel.0 == 0xf81f)
            .count()
    };
    settle(&window);
    assert!(
        colored() > 20,
        "first page must paint supplied colors before art"
    );
    let page_size = crate::router::lock(&ctx.shared).games.grid.page_size();
    let tier = crate::sizing::games_grid_cover_source_size(crate::router::output_scene(&app));
    for row in &rows[..page_size] {
        ctx.media.seed(
            crate::media_cache::MediaKey {
                media_id: row.media_id,
                system: row.system_id.clone(),
                path: row.path.clone(),
                max_size: tier,
                fit: crate::games::grid_cover_fit(&app, GamesMode::Browse),
                folder: false,
                image_type: None,
                prefer: None,
            },
            crate::media_cache::DecodedImage {
                buffer: slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(
                    &[255u8; 16],
                    2,
                    2,
                ),
            },
        );
    }
    crate::games::render(&ctx, &app);
    settle(&window);
    assert_eq!(colored(), 0, "placeholder must yield to loaded art");
    for page in 1..=30 / page_size + 1 {
        crate::router::handle_action(&ctx, &app, "page_next");
        settle(&window);
        assert_eq!(view.get_page(), page as i32);
        assert!(view
            .get_cells()
            .iter()
            .all(|cell| cell.has_placeholder && !cell.has_cover));
        assert!(
            colored() > 20,
            "page {page} must paint colors, including appended API rows"
        );
    }
    {
        let mut shared = crate::router::lock(&ctx.shared);
        let index = shared.games.grid.current_index();
        shared.games.rows[index].cover_color = None;
    }
    crate::games::render(&ctx, &app);
    assert!(
        !view
            .get_cells()
            .row_data(0)
            .is_some_and(|cell| cell.has_placeholder),
        "absent Core colors must not be invented"
    );
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
    app.global::<Shell>().set_active_screen(Screen::Games);
    seat_folder(&ctx, &app, "First", 0);
    let size = crate::router::lock(&ctx.shared).games.grid.page_size();
    {
        let mut shared = crate::router::lock(&ctx.shared);
        let model = &mut shared.games;
        model.rows.dense_mut().truncate(size);
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
    assert!(!crate::router::lock(&ctx.shared).games.page.sliding);
    crate::games::on_append(
        &ctx,
        &app,
        0,
        Err(crate::games::PageError::transport("offline")),
    );
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
        !crate::router::lock(&ctx.shared).games.page.sliding,
        "a partial nonterminal page must wait"
    );
    crate::games::on_append(&ctx, &app, 0, Ok((game_rows("Last", size - 1), None)));
    assert!(crate::router::lock(&ctx.shared).games.page.sliding);
    assert_eq!(view.get_cells().row_data(0).map(|cell| cell.name), outgoing);
    assert_eq!(view.get_next_cells().row_count(), size);
    assert_eq!(
        view.get_slide_dir(),
        -1,
        "a deferred wrap keeps Previous direction"
    );
    assert!(distinct_frames(&window, 18) > 5);
    assert_eq!(view.get_page(), 4);
    assert!(!crate::router::lock(&ctx.shared).games.loading_more);
}

#[test]
fn rail_needs_a_qualified_hold_and_lingers_after_the_scroll_stops() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    let (_runtime, ctx) = offline_ctx();
    crate::sizing::apply_scene(
        &app,
        crate::sizing::Scene::of(&app, f64::from(W), f64::from(H), false),
    );
    app.global::<Shell>().set_active_screen(Screen::Games);
    seat_folder(&ctx, &app, "Game", 0);
    settle(&window);
    let view = app.global::<crate::GamesView>();
    for _ in 0..3 {
        crate::router::handle_action(&ctx, &app, "page_next");
        assert!(crate::router::lock(&ctx.shared).games.page.sliding);
        assert!(!view.get_rapid_active());
        assert!(
            !view.get_rail_visible(),
            "ordinary taps must not raise the rail"
        );
        distinct_frames(&window, 18);
    }
    crate::input::dispatch_repeat(&ctx, &app, "page_next", HoldTier::Page);
    assert!(view.get_rapid_active());
    assert!(view.get_rail_visible());
    assert!(!crate::router::lock(&ctx.shared).games.page.sliding);
    // A fresh tap ends the fast scroll; the rail lingers, then goes.
    crate::router::handle_action(&ctx, &app, "page_prev");
    assert!(!view.get_rapid_active());
    assert!(view.get_rail_visible(), "the rail lingers after the scroll");
    CLOCK.with(|clock| clock.set(clock.get() + zaparoo_app::media_list::RAIL_LINGER_MS));
    slint::platform::update_timers_and_animations();
    assert!(!view.get_rail_visible());
    distinct_frames(&window, 18);
    // Quiet ends the scroll too, and the linger restarts from there.
    crate::input::dispatch_repeat(&ctx, &app, "page_next", HoldTier::Page);
    assert!(view.get_rail_visible());
    CLOCK.with(|clock| clock.set(clock.get() + zaparoo_app::input::RAPID_QUIET_MS));
    slint::platform::update_timers_and_animations();
    assert!(!view.get_rapid_active());
    assert!(view.get_rail_visible());
    CLOCK.with(|clock| clock.set(clock.get() + zaparoo_app::media_list::RAIL_LINGER_MS));
    slint::platform::update_timers_and_animations();
    assert!(!view.get_rail_visible());
}

#[test]
fn taps_after_a_fast_scroll_do_not_extend_the_rail_linger() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    let (_runtime, ctx) = offline_ctx();
    crate::sizing::apply_scene(
        &app,
        crate::sizing::Scene::of(&app, f64::from(W), f64::from(H), false),
    );
    app.global::<Shell>().set_active_screen(Screen::Games);
    seat_folder(&ctx, &app, "Game", 0);
    settle(&window);
    let view = app.global::<crate::GamesView>();
    crate::input::dispatch_repeat(&ctx, &app, "page_next", HoldTier::Page);
    assert!(view.get_rail_visible());
    crate::router::handle_action(&ctx, &app, "down");
    assert!(!view.get_rapid_active());
    assert!(view.get_rail_visible(), "the rail lingers after the scroll");
    let half = zaparoo_app::media_list::RAIL_LINGER_MS / 2;
    CLOCK.with(|clock| clock.set(clock.get() + half));
    slint::platform::update_timers_and_animations();
    crate::router::handle_action(&ctx, &app, "up");
    CLOCK.with(|clock| {
        clock.set(clock.get() + zaparoo_app::media_list::RAIL_LINGER_MS - half);
    });
    slint::platform::update_timers_and_animations();
    assert!(
        !view.get_rail_visible(),
        "a tap during the linger must not push its deadline back"
    );
}

#[test]
fn wrapped_game_grids_follow_keys_and_pointer_direction_in_every_mode() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    let (_runtime, ctx) = offline_ctx();
    let ctx = std::sync::Arc::new(ctx);
    crate::sizing::apply_scene(
        &app,
        crate::sizing::Scene::of(&app, f64::from(W), f64::from(H), false),
    );
    crate::games::bind_input(&ctx, &app);
    seat_folder(&ctx, &app, "Game", 0);
    let size = crate::router::lock(&ctx.shared).games.grid.page_size();
    let count = size * 3 - 1;
    {
        let mut shared = crate::router::lock(&ctx.shared);
        shared.games.rows = game_rows("Game", count).into();
        shared.games.total_files = count as u32;
        shared.games.grid.set_item_count(count);
    }
    let view = app.global::<crate::GamesView>();
    for (screen, mode) in [
        (Screen::Games, GamesMode::Browse),
        (Screen::Favorites, GamesMode::Favorites),
        (Screen::Recents, GamesMode::Recents),
    ] {
        app.global::<Shell>().set_active_screen(screen);
        crate::router::lock(&ctx.shared).games.mode = mode;
        for (action, direction, pointer) in [
            ("page_prev", -1, false),
            ("page_next", 1, false),
            ("up", -1, false),
            ("down", 1, false),
            ("page_prev", -1, true),
            ("page_next", 1, true),
        ] {
            crate::router::lock(&ctx.shared)
                .games
                .grid
                .set_current_index_immediate(if direction < 0 { 0 } else { count - 1 });
            crate::games::render(&ctx, &app);
            settle(&window);
            if pointer {
                app.global::<crate::GamesInput>()
                    .invoke_page_requested(direction);
            } else {
                crate::router::handle_action(&ctx, &app, action);
            }
            assert_eq!(
                view.get_slide_dir(),
                direction,
                "{screen:?} {action} pointer={pointer}"
            );
            assert!(crate::router::lock(&ctx.shared).games.page.sliding);
            assert!(distinct_frames(&window, 18) > 5);
            assert_eq!(view.get_page(), if direction < 0 { 2 } else { 0 });
        }
    }
    app.global::<crate::Motion>().set_enabled(false);
    crate::router::handle_action(&ctx, &app, "page_prev");
    assert_eq!(view.get_page(), 2);
    assert_eq!(view.get_slide_dir(), -1);
    assert!(
        !crate::router::lock(&ctx.shared).games.page.sliding,
        "reduced motion still cuts"
    );
}

#[test]
fn a_held_page_flip_stops_at_the_ends_from_its_first_repeat() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    let (_runtime, ctx) = offline_ctx();
    crate::sizing::apply_scene(
        &app,
        crate::sizing::Scene::of(&app, f64::from(W), f64::from(H), false),
    );
    app.global::<Shell>().set_active_screen(Screen::Games);
    seat_folder(&ctx, &app, "Game", 0);
    let size = crate::router::lock(&ctx.shared).games.grid.page_size();
    {
        let mut shared = crate::router::lock(&ctx.shared);
        shared.games.rows = game_rows("Game", size * 2).into();
        shared.games.grid.set_item_count(size * 2);
        shared.games.grid.set_current_index_immediate(size);
    }
    crate::games::render(&ctx, &app);
    settle(&window);
    let page = || crate::router::lock(&ctx.shared).games.grid.current_page();
    assert_eq!(page(), 1);
    // An early repeat is still a hold, though not yet a fast scroll.
    crate::input::dispatch_repeat(&ctx, &app, "page_next", HoldTier::Row);
    assert_eq!(page(), 1, "a held flip must not wrap past the last page");
    assert!(!crate::router::lock(&ctx.shared).games.page.sliding);
    // A tap still wraps, continuing forward rather than reversing to page zero.
    crate::router::handle_action(&ctx, &app, "page_next");
    assert!(crate::router::lock(&ctx.shared).games.page.sliding);
    assert_eq!(app.global::<crate::GamesView>().get_slide_dir(), 1);
}

#[test]
fn long_holds_step_pages_then_letters_and_the_rail_follows() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    let (_runtime, ctx) = offline_ctx();
    crate::sizing::apply_scene(
        &app,
        crate::sizing::Scene::of(&app, f64::from(W), f64::from(H), false),
    );
    app.global::<Shell>().set_active_screen(Screen::Games);
    let bucket = |label: &str, offset: u32| zaparoo_core::media_types::BrowseIndexGroup {
        key: label.to_lowercase(),
        label: label.into(),
        count: 0,
        cursor: String::new(),
        offset,
    };
    {
        let mut shared = crate::router::lock(&ctx.shared);
        shared.games.mode = GamesMode::Browse;
        shared.games.system_id = "NES".into();
        shared.games.browse_path = "/roms/nes".into();
        shared.games.rows = game_rows("Game", 60).into();
        shared.games.grid.set_item_count(60);
        shared.games.grid.set_current_index_immediate(0);
        shared.letter_buckets = vec![bucket("A", 0), bucket("B", 20), bucket("C", 45)];
        shared.letter_scope = Some(("NES".into(), "/roms/nes".into()));
    }
    crate::games::render(&ctx, &app);
    settle(&window);
    let view = app.global::<crate::GamesView>();
    assert_eq!(view.get_rail_letters().row_count(), 3);
    assert_eq!(view.get_rail_index(), 0);
    let index = || crate::router::lock(&ctx.shared).games.grid.current_index();
    let page = crate::router::lock(&ctx.shared).games.grid.page_size();

    // Past the hold threshold a held Down moves a page, not a row.
    crate::input::dispatch_repeat(&ctx, &app, "down", HoldTier::Page);
    assert_eq!(index(), page);
    // A long hold steps a letter at a time.
    crate::input::dispatch_repeat(&ctx, &app, "down", HoldTier::Letter);
    assert_eq!(index(), 20);
    assert_eq!(view.get_rail_index(), 1);
    assert_eq!(view.get_rail_letter(), "B");
    crate::input::dispatch_repeat(&ctx, &app, "down", HoldTier::Letter);
    assert_eq!(index(), 45);
    assert_eq!(view.get_rail_letter(), "C");
    // Past the last letter the hold keeps paging toward the end.
    crate::input::dispatch_repeat(&ctx, &app, "down", HoldTier::Letter);
    assert_eq!(index(), (45 + page).min(59));
    crate::input::dispatch_repeat(&ctx, &app, "up", HoldTier::Letter);
    assert_eq!(index(), 20);
    crate::input::dispatch_repeat(&ctx, &app, "page_prev", HoldTier::Letter);
    assert_eq!(index(), 0);
    assert_eq!(view.get_rail_letter(), "A");
    // A fast scroll stops at the ends instead of wrapping round.
    crate::input::dispatch_repeat(&ctx, &app, "up", HoldTier::Letter);
    assert_eq!(index(), 0, "a held Up at the top must not wrap to the end");
    crate::input::dispatch_repeat(&ctx, &app, "page_prev", HoldTier::Page);
    assert_eq!(index(), 0);
    let page_now = || crate::router::lock(&ctx.shared).games.grid.current_page();
    let total_pages = crate::router::lock(&ctx.shared)
        .games
        .grid
        .total_page_count();
    for _ in 0..total_pages + 2 {
        crate::input::dispatch_repeat(&ctx, &app, "down", HoldTier::Page);
    }
    let last_page = page_now();
    assert_eq!(
        last_page + 1,
        total_pages,
        "a held Down reaches the last page"
    );
    crate::input::dispatch_repeat(&ctx, &app, "down", HoldTier::Page);
    assert_eq!(
        page_now(),
        last_page,
        "a held Down at the bottom must not wrap to the top"
    );
    assert!(last_page > 0);
    // A tapped page flip still wraps round.
    crate::router::handle_action(&ctx, &app, "page_next");
    assert_eq!(
        crate::router::lock(&ctx.shared).games.grid.current_page(),
        0
    );
    crate::router::lock(&ctx.shared)
        .games
        .grid
        .set_current_index_immediate(0);

    // Where letters mean nothing the rail keeps only its position marker,
    // and a long hold keeps paging.
    crate::router::lock(&ctx.shared).games.mode = GamesMode::Recents;
    crate::games::render(&ctx, &app);
    assert_eq!(view.get_rail_letters().row_count(), 0);
    crate::input::dispatch_repeat(&ctx, &app, "down", HoldTier::Letter);
    assert_eq!(index(), page);
    assert!(view.get_rail_fraction() > 0.0);
}

#[test]
#[allow(
    clippy::expect_used,
    reason = "cold route must queue its embedded logo"
)]
fn entering_systems_queues_logos_after_route_commit_without_an_extra_key() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    let (_runtime, ctx) = offline_ctx();
    crate::sizing::apply_scene(
        &app,
        crate::sizing::Scene::of(&app, f64::from(W), f64::from(H), false),
    );
    crate::router::lock(&ctx.shared).systems = ["SNES", "ZzzNoArtwork"]
        .into_iter()
        .map(|id| zaparoo_core::media_types::SystemInfo {
            id: id.into(),
            name: id.into(),
            category: "Console".into(),
            media_count: Some(1),
            ..Default::default()
        })
        .collect();
    for animate in [false, true] {
        ctx.logos.clear();
        app.global::<Shell>().set_active_screen(Screen::Hub);
        crate::systems::enter(&ctx, &app, "Console", EntryMode::Fresh, animate);
        assert_eq!(app.global::<Shell>().get_active_screen(), Screen::Systems);
        let view = app.global::<SystemsView>();
        let page = view.get_cells();
        let pending = page.row_data(0).expect("immediate pending tile");
        assert!(!pending.has_cover);
        assert!(
            !pending.wordmark,
            "known artwork must stay blank while preparing"
        );
        assert!(
            !pending.name.is_empty(),
            "caption still identifies the focused tile"
        );
        assert!(page.row_data(1).expect("no embedded artwork").wordmark);
        advance(16);
        let finish = ctx
            .logos
            .hold_job()
            .expect("committed destination must request art without input");
        finish();
        crate::system_logos::refresh(&ctx, &app);
        assert_eq!(view.get_cells(), page, "completion patches same delegates");
        assert!(page.row_data(0).expect("prepared tile").has_cover);
        settle(&window);
    }
    ctx.logos.clear();
    app.global::<Shell>().set_active_screen(Screen::Hub);
    crate::systems::render(&ctx, &app);
    app.global::<Shell>().set_active_screen(Screen::Settings);
    advance(16);
    assert!(
        ctx.logos.hold_job().is_none(),
        "an abandoned destination cannot steal current work"
    );
}

#[test]
fn entering_systems_paints_the_focused_tile_zoomed_from_the_first_frame() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, _window) = boot();
    let (_runtime, ctx) = offline_ctx();
    crate::sizing::apply_scene(
        &app,
        crate::sizing::Scene::of(&app, f64::from(W), f64::from(H), false),
    );
    // The MiSTer draws the focus zoom by size, with a larger copy of the
    // focused tile's logo.
    let motion = app.global::<crate::Motion>();
    motion.set_zoom_by_size(true);
    motion.set_focus_zoom(104.0);
    crate::router::lock(&ctx.shared).systems = ["SNES", "NES"]
        .into_iter()
        .map(|id| zaparoo_core::media_types::SystemInfo {
            id: id.into(),
            name: id.into(),
            category: "Console".into(),
            media_count: Some(1),
            ..Default::default()
        })
        .collect();
    app.global::<Shell>().set_active_screen(Screen::Hub);
    // Nothing is warm: no worker runs here.
    ctx.logos.clear();
    crate::systems::enter(&ctx, &app, "Console", EntryMode::Fresh, true);
    let view = app.global::<SystemsView>();
    assert_eq!(app.global::<Shell>().get_active_screen(), Screen::Systems);
    assert!(view.get_focus_ready());
    assert_eq!(
        view.get_focus_art_local(),
        view.get_selected_local(),
        "the zoomed logo is in place before the route commits"
    );
}

#[test]
#[allow(
    clippy::expect_used,
    reason = "the Hub fixture pins one system with embedded artwork"
)]
fn returning_to_the_hub_requests_its_logos_without_an_extra_key() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    let (_runtime, ctx) = offline_ctx();
    crate::sizing::apply_scene(
        &app,
        crate::sizing::Scene::of(&app, f64::from(W), f64::from(H), false),
    );
    {
        let mut shared = crate::router::lock(&ctx.shared);
        shared.hub.layout.items = vec![zaparoo_core::hub_layout::HubItem {
            kind_raw: "zapscript".into(),
            script: "**launch.system:SNES".into(),
            system: "SNES".into(),
            ..Default::default()
        }];
        shared.hub.categories_loaded = true;
        shared.hub.restore_done = true;
    }
    let shell = app.global::<Shell>();
    // A cold restore into another screen: the Hub is resolved beneath it,
    // and that screen's render takes the one logo window.
    shell.set_active_screen(Screen::Systems);
    crate::hub::rebuild(&ctx, &app);
    crate::systems::render(&ctx, &app);
    advance(16);
    assert!(
        ctx.logos.hold_job().is_none(),
        "the Hub is not on screen yet"
    );
    let tile = || {
        app.global::<HubView>()
            .get_cells()
            .row_data(0)
            .expect("pinned system tile")
    };
    assert!(!tile().has_cover);

    crate::router::handle_action(&ctx, &app, "cancel");

    assert_eq!(shell.get_active_screen(), Screen::Hub);
    let finish = ctx
        .logos
        .hold_job()
        .expect("the returned-to Hub must request art without input");
    finish();
    crate::system_logos::refresh(&ctx, &app);
    assert!(tile().has_cover);
    settle(&window);
}

#[test]
#[allow(
    clippy::expect_used,
    reason = "the Hub fixture pins one system with embedded artwork"
)]
fn leaving_a_game_list_entered_from_the_hub_requests_the_hub_logos() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    let (_runtime, ctx) = offline_ctx();
    crate::sizing::apply_scene(
        &app,
        crate::sizing::Scene::of(&app, f64::from(W), f64::from(H), false),
    );
    {
        let mut shared = crate::router::lock(&ctx.shared);
        shared.hub.layout.items = vec![zaparoo_core::hub_layout::HubItem {
            kind_raw: "zapscript".into(),
            script: "**launch.system:SNES".into(),
            system: "SNES".into(),
            ..Default::default()
        }];
        shared.hub.categories_loaded = true;
        shared.hub.restore_done = true;
        shared.persist.games.entered_from_hub = true;
    }
    let shell = app.global::<Shell>();
    shell.set_active_screen(Screen::Games);
    crate::hub::rebuild(&ctx, &app);
    crate::games::render(&ctx, &app);
    advance(16);
    assert!(
        ctx.logos.hold_job().is_none(),
        "the Hub is not on screen yet"
    );

    crate::router::handle_action(&ctx, &app, "cancel");

    assert_eq!(shell.get_active_screen(), Screen::Hub);
    let finish = ctx
        .logos
        .hold_job()
        .expect("the returned-to Hub must request art without input");
    finish();
    crate::system_logos::refresh(&ctx, &app);
    let tile = app
        .global::<HubView>()
        .get_cells()
        .row_data(0)
        .expect("pinned system tile");
    assert!(tile.has_cover);
    settle(&window);
}

#[test]
#[allow(
    clippy::expect_used,
    reason = "the Hub fixture pins one game whose cover is requested"
)]
fn a_hub_cover_dropped_for_a_launched_core_is_requested_again_on_resume() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    let (_runtime, mut ctx) = offline_ctx();
    ctx.is_mister = true;
    crate::sizing::apply_scene(
        &app,
        crate::sizing::Scene::of(&app, f64::from(W), f64::from(H), false),
    );
    {
        let mut shared = crate::router::lock(&ctx.shared);
        shared.hub.layout.items = vec![zaparoo_core::hub_layout::HubItem {
            kind_raw: "zapscript".into(),
            script: "/g/1".into(),
            system: "NES".into(),
            path: "/g/1".into(),
            ..Default::default()
        }];
        shared.hub.categories_loaded = true;
        shared.hub.restore_done = true;
    }
    app.global::<Shell>().set_active_screen(Screen::Hub);
    crate::hub::rebuild(&ctx, &app);
    let key = ctx
        .media
        .pending_keys()
        .into_iter()
        .find(|key| key.path == "/g/1")
        .expect("the tile asks for its cover");
    ctx.media.seed(
        key.clone(),
        crate::media_cache::DecodedImage {
            buffer: slint::SharedPixelBuffer::<slint::Rgba8Pixel>::new(8, 8),
        },
    );
    crate::hub::rebuild(&ctx, &app);
    let requests = || {
        ctx.media
            .pending_keys()
            .iter()
            .filter(|pending| **pending == key)
            .count()
    };
    let before = requests();

    crate::set_dormant(&ctx, &app, true);
    assert!(!ctx.media.is_cached(&key));
    crate::set_dormant(&ctx, &app, false);

    assert_eq!(
        requests(),
        before + 1,
        "resume asks Core for the cover again"
    );
    settle(&window);
}

#[test]
#[allow(
    clippy::expect_used,
    reason = "fixture must contain two real system tiles"
)]
fn adjacent_systems_move_keeps_logo_images_and_glides_focus() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    let (_runtime, ctx) = offline_ctx();
    crate::sizing::apply_scene(
        &app,
        crate::sizing::Scene::of(&app, f64::from(W), f64::from(H), false),
    );
    app.global::<Shell>().set_active_screen(Screen::Systems);
    {
        let mut shared = crate::router::lock(&ctx.shared);
        shared.systems_model.rows = ["SNES", "NES"]
            .into_iter()
            .map(|id| zaparoo_app::systems::SystemRow {
                id: id.into(),
                name: id.into(),
                cover_key: id.into(),
                category: String::new(),
                hidden: false,
                zap_script: String::new(),
                release_date: String::new(),
                manufacturer: String::new(),
                media_count: None,
            })
            .collect();
        shared.systems_model.grid.set_item_count(2);
        shared.systems_model.focus_armed = true;
    }
    crate::systems::render(&ctx, &app);
    let view = app.global::<SystemsView>();
    let page = view.get_cells();
    assert!(!page.row_data(0).expect("named fallback").has_cover);
    let finish = ctx
        .logos
        .hold_job()
        .expect("cold logo queued off UI thread");
    // A worker stalled inside decode cannot stall publication or input.
    crate::systems::handle_action(&ctx, &app, "right");
    assert_eq!(view.get_selected_local(), 1);
    assert!(!page.row_data(1).expect("next named fallback").has_cover);
    settle(&window);
    finish();
    ctx.logos.prepare_queued();
    crate::system_logos::refresh(&ctx, &app);
    assert_eq!(
        view.get_cells(),
        page,
        "late art patches existing delegates"
    );
    assert_eq!(view.get_selected_local(), 1, "late art cannot steal focus");
    crate::systems::handle_action(&ctx, &app, "left");
    settle(&window);
    let first = view.get_cells().row_data(0).expect("first system");
    assert!(first.has_cover, "test must exercise real logo images");
    crate::systems::handle_action(&ctx, &app, "right");
    let after = view
        .get_cells()
        .row_data(0)
        .expect("first system still visible");
    assert!(
        distinct_frames(&window, 6) > 2,
        "adjacent focus should travel over several frames"
    );
    assert!(
        crate::view_model::same_image(&first.cover, &after.cover),
        "adjacent focus must not rebuild every logo image"
    );
}

#[test]
fn header_cue_requests_use_rust_settings_and_about_drivers() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, _) = boot();
    let (_runtime, ctx) = offline_ctx();
    let ctx = std::sync::Arc::new(ctx);
    crate::settings::bind_input(&ctx, &app);
    app.global::<Shell>().set_active_screen(Screen::Settings);
    crate::settings::open_page(&ctx, &app, SettingsPage::Appearance);
    let before = app.global::<crate::SettingsView>().get_index();
    app.global::<crate::SettingsInput>()
        .invoke_page_requested(1);
    assert_ne!(app.global::<crate::SettingsView>().get_index(), before);
    app.global::<crate::SettingsInput>()
        .invoke_page_requested(-1);
    assert_eq!(app.global::<crate::SettingsView>().get_index(), before);
    crate::about::bind(&ctx, &app);
    app.global::<Shell>().set_active_screen(Screen::About);
    let about = app.global::<crate::AboutView>();
    about.set_maximum_scroll_milli(1000);
    about.invoke_direction_requested(1);
    assert!(about.get_scroll_milli() > 0);
    assert_eq!(
        crate::router::lock(&ctx.shared).persist.about_scroll_milli,
        about.get_scroll_milli() as u32
    );
    about.invoke_direction_requested(-1);
    assert_eq!(about.get_scroll_milli(), 0);
}

#[test]
fn digital_position_changes_paint_only_grouped_header_cue() {
    position_changes_paint_only_header_cue(false);
}

#[test]
fn digital_list_positions_paint_only_grouped_header_cue() {
    position_changes_paint_only_header_cue(true);
}

fn position_changes_paint_only_header_cue(list: bool) {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    let (width, height) = (960u32, 540u32);
    window.set_size(slint::PhysicalSize::new(width, height));
    app.global::<Sizing>().set_screen_width(width as f32);
    app.global::<Sizing>().set_screen_height(height as f32);
    crate::sizing::apply_scene(
        &app,
        crate::sizing::Scene::of(&app, f64::from(width), f64::from(height), false),
    );
    let shot = || {
        let mut buf = vec![Rgb565Pixel(0); (width * height) as usize];
        window.request_redraw();
        assert!(window.draw_if_needed(|renderer| {
            renderer.render(&mut buf, width as usize);
        }));
        buf
    };
    let shell = app.global::<Shell>();
    shell.set_browse_list_layout(list);
    shell.set_systems_list_layout(list);
    let systems = app.global::<SystemsView>();
    systems.set_count(40);
    systems.set_total_pages(3);
    systems.set_has_pages_below(true);
    systems.set_has_items_below(true);
    let games = app.global::<crate::GamesView>();
    games.set_cells(cells(10, "Game"));
    games.set_count(40);
    games.set_total_files(500);
    games.set_total_items(40);
    games.set_total_pages(3);
    games.set_has_pages_below(true);
    games.set_has_items_below(true);
    for screen in [
        Screen::Systems,
        Screen::FavoriteSystems,
        Screen::Games,
        Screen::Favorites,
        Screen::Recents,
    ] {
        shell.set_active_screen(screen);
        systems.set_mode(if screen == Screen::FavoriteSystems {
            SystemsMode::Favorites
        } else {
            SystemsMode::Category
        });
        games.set_mode(match screen {
            Screen::Favorites => GamesMode::Favorites,
            Screen::Recents => GamesMode::Recents,
            _ => GamesMode::Browse,
        });
        crate::router::refresh_layout(&app);
        systems.set_page(0);
        games.set_page(0);
        systems.set_current_index(0);
        games.set_current_index(0);
        shot();
        advance(500);
        let before = shot();
        systems.set_page(1);
        games.set_page(1);
        systems.set_current_index(1);
        games.set_current_index(1);
        shot();
        advance(500);
        let after = shot();
        let changed: Vec<_> = before
            .iter()
            .zip(&after)
            .enumerate()
            .filter(|(_, (a, b))| a != b)
            .map(|(i, _)| i)
            .collect();
        assert!(!changed.is_empty(), "{screen:?} must paint its position");
        let layout = app.global::<crate::Layout>();
        let top = (app.global::<Sizing>().get_header_bottom() + layout.get_top_margin()) as usize;
        let bottom = top + layout.get_strip_height() as usize;
        assert!(
            changed
                .iter()
                .all(|i| (top..bottom).contains(&(i / width as usize))
                    && i % width as usize > width as usize * 2 / 3),
            "{screen:?} position changed outside the header's count/cue group"
        );
    }
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
    app.global::<Shell>().set_active_screen(Screen::Systems);
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
    for (screen, mode) in [
        (Screen::Systems, SystemsMode::Category),
        (Screen::FavoriteSystems, SystemsMode::Favorites),
    ] {
        app.global::<Shell>().set_active_screen(screen);
        crate::router::lock(&ctx.shared).systems_model.mode = mode;
        crate::systems::render(&ctx, &app);
        for direction in [-1, 1, 1, -1] {
            let outgoing = view.get_cells().row_data(0).map(|cell| cell.name);
            crate::router::handle_action(
                &ctx,
                &app,
                if direction < 0 {
                    "page_prev"
                } else {
                    "page_next"
                },
            );
            assert_eq!(
                view.get_slide_dir(),
                direction,
                "ordinary and wrapped turns keep input direction"
            );
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
    app.global::<Shell>().set_active_screen(Screen::Games);
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
    ov.invoke_pointer_choice(PressOwner::List, 0, true);
    crate::router::handle_action(&ctx, &app, "up");
    crate::router::handle_action(&ctx, &app, "accept");
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
    assert!(
        ov.get_launcher_saving_visible(),
        "a caption that only just appeared stays out its hold"
    );
    CLOCK.with(|clock| clock.set(clock.get() + 200));
    slint::platform::update_timers_and_animations();
    assert!(!ov.get_list_open());
    assert_eq!(ov.get_dialog_error(), ErrorKind::LauncherSave);
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
fn a_config_file_that_did_not_load_alerts_once_and_blocks_saves() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, _) = boot();
    let (_runtime, ctx) = offline_ctx();
    app.global::<crate::Motion>().set_enabled(false);
    {
        let mut shared = crate::router::lock(&ctx.shared);
        shared.config_fault = Some("/media/fat/zaparoo/frontend.toml".into());
        shared.hub.read_only = true;
    }
    crate::router::maybe_open_startup_notices(&ctx, &app);
    let ov = app.global::<crate::Overlays>();
    assert!(ov.get_dialog_open());
    assert_eq!(ov.get_dialog_kind(), DialogKind::ActionError);
    assert_eq!(ov.get_dialog_error(), ErrorKind::ConfigFile);
    assert_eq!(ov.get_dialog_arg(), "/media/fat/zaparoo/frontend.toml");

    // Dismissing it hands the surface to the rest of the startup ladder.
    crate::router::handle_action(&ctx, &app, "accept");
    assert_eq!(ov.get_dialog_kind(), DialogKind::Notice);

    // A save while the file is unusable neither writes nor raises its own
    // alert: the fixture's config path would fail the write otherwise.
    crate::settings::save(&ctx, &app);
    let mut shared = crate::router::lock(&ctx.shared);
    assert_eq!(shared.errors.showing(), None);
    assert_eq!(shared.errors.take_next(), None);
    assert!(shared.config_fault_shown);
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
    assert_eq!(ov.get_dialog_error(), ErrorKind::CardWrite);
    assert_eq!(
        ov.get_dialog_buttons().row_data(0),
        Some(DialogButton::Retry)
    );
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
        ov.get_dialog_error(),
        ErrorKind::Setting,
        "retry must not replace a queued alert"
    );
    crate::router::handle_action(&ctx, &app, "cancel");
    assert_eq!(
        ov.get_dialog_error(),
        ErrorKind::CardWrite,
        "retry failure must survive behind the other alert"
    );
    crate::router::handle_action(&ctx, &app, "cancel");
    assert!(!ov.get_dialog_open());
    assert!(crate::router::lock(&ctx.shared).errors.showing().is_none());
}

#[test]
fn token_cancel_push_rejects_a_reopened_write_target() -> Result<(), &'static str> {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    let ov = app.global::<crate::Overlays>();
    ov.set_card_write_key("1".into());
    ov.set_card_write_open(true);
    let target = crate::press_feedback::current(&app).ok_or("Cancel target")?;
    let commits = Rc::new(Cell::new(0));
    arm_feedback(&app, &commits);
    assert_eq!(commits.get(), 0);
    ov.set_card_write_key("2".into());
    settle(&window);
    assert_eq!(
        commits.get(),
        0,
        "old push must not cancel a reopened write"
    );
    assert!(!crate::press_feedback::pending(&app));
    let count = commits.clone();
    crate::press_feedback::dispatch(&app, &target, move |_| count.set(count.get() + 1));
    settle(&window);
    assert_eq!(commits.get(), 0, "already stale target is rejected too");
    arm_feedback(&app, &commits);
    settle(&window);
    assert_eq!(commits.get(), 1, "new write owns its own Accept");
    ov.set_card_write_open(false);
    crate::press_feedback::cancel(&app);
    settle(&window);
    assert_eq!(commits.get(), 1, "no delayed operation remains");
    Ok(())
}

/// Pending work retains local feedback until its completion releases it.
#[test]
#[allow(
    clippy::expect_used,
    reason = "a fixture with no pressable control is a broken test"
)]
fn a_held_press_outlives_its_push_and_lifts_on_release() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    crate::sizing::apply_scene(
        &app,
        crate::sizing::Scene::of(&app, f64::from(W), f64::from(H), false),
    );
    crate::router::open_quit_confirm(&app);
    settle(&window);

    let held = Rc::new(Cell::new(None));
    let target = crate::press_feedback::current(&app).expect("a dialog button to press");
    crate::press_feedback::dispatch(&app, &target, {
        let held = held.clone();
        let weak = app.as_weak();
        move |_| {
            let app = weak.upgrade().expect("the app outlives its own commit");
            held.set(Some(crate::press_feedback::keep_held(&app)));
        }
    });
    settle(&window);

    let hold = held.get().expect("the commit ran and kept the press");
    assert_eq!(
        app.global::<crate::PressFeedback>().get_owner(),
        target.owner,
        "the press must still be down while the work it started runs"
    );

    crate::press_feedback::release(&app, hold);
    assert_eq!(
        app.global::<crate::PressFeedback>().get_owner(),
        PressOwner::None,
        "releasing the hold lifts the control"
    );
}

#[test]
fn launch_tile_releases_face_art_edge_and_ring() {
    launch_tile_release_cycle(false);
}

#[test]
fn launch_tile_releases_face_art_edge_and_ring_in_reused_buffer() {
    launch_tile_release_cycle(true);
}

#[allow(
    clippy::too_many_lines,
    clippy::expect_used,
    reason = "one real driver fixture checks every hold exit against the same resting pixels"
)]
fn launch_tile_release_cycle(reuse: bool) {
    enum Exit {
        Reply,
        Error,
        Cancel,
        Dormant,
        Timeout,
        Selection,
        StaleReply,
    }
    REUSE_BUFFER.with(|mode| mode.set(reuse));
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    let (_runtime, ctx) = offline_ctx();
    crate::sizing::apply_scene(
        &app,
        crate::sizing::Scene::of(&app, f64::from(W), f64::from(H), false),
    );
    let shell = app.global::<Shell>();
    shell.set_active_screen(Screen::Games);
    shell.set_browse_list_layout(false);
    {
        let mut shared = crate::router::lock(&ctx.shared);
        shared.games.rows = game_rows("Launch", 2).into();
        for (i, row) in shared.games.rows.iter_mut().enumerate() {
            row.path = format!("/probe/{i}.rom");
        }
        shared.games.grid.set_item_count(2);
        shared.games.focus_armed = true;
    }
    crate::games::render(&ctx, &app);
    settle(&window);
    let resting = pixels(&window);
    for exit in [
        Exit::Reply,
        Exit::Error,
        Exit::Cancel,
        Exit::Dormant,
        Exit::Timeout,
        Exit::Selection,
        Exit::StaleReply,
    ] {
        // Exercise router -> games::accept_current -> router::launch. The
        // offline runtime leaves the reply pending until this fixture sends it.
        crate::router::handle_action(&ctx, &app, "accept");
        assert!(crate::press_feedback::held_ticket(&app).is_none());
        pixels(&window);
        advance(zaparoo_app::input::PRESS_FEEDBACK_MS - 1);
        assert!(crate::press_feedback::held_ticket(&app).is_none());
        let pushed = pixels(&window);
        assert_ne!(pushed, resting, "launch tile pushes before dispatch");
        advance(1);
        let hold = crate::press_feedback::held_ticket(&app).expect("real launch keeps its ticket");
        assert_region_matches(
            &pixels(&window),
            &pushed,
            0..W as usize,
            32..H as usize - 30,
            "launch hold cannot raise the face between push and reply",
        );
        settle(&window);
        assert_ne!(
            pixels(&window),
            resting,
            "pending launch keeps the face depressed"
        );
        assert_eq!(
            app.global::<crate::PressFeedback>().get_owner(),
            PressOwner::Games
        );
        match exit {
            Exit::Reply => crate::router::finish_launch(
                &ctx,
                &app,
                hold,
                crate::router::LaunchOutcome::Ok,
                "Launch",
            ),
            Exit::Error => {
                crate::router::finish_launch(
                    &ctx,
                    &app,
                    hold,
                    crate::router::LaunchOutcome::Failed,
                    "Launch",
                );
                assert!(app.global::<crate::Overlays>().get_dialog_open());
                crate::router::handle_action(&ctx, &app, "cancel");
            }
            Exit::Cancel => crate::press_feedback::cancel(&app),
            Exit::Dormant => {
                crate::set_dormant(&ctx, &app, true);
                pixels(&window);
                crate::set_dormant(&ctx, &app, false);
            }
            Exit::Timeout => advance(10_001),
            Exit::Selection | Exit::StaleReply => {
                crate::router::handle_action(&ctx, &app, "right");
                crate::router::handle_action(&ctx, &app, "left");
                if matches!(exit, Exit::StaleReply) {
                    crate::router::handle_action(&ctx, &app, "accept");
                    advance(zaparoo_app::input::PRESS_FEEDBACK_MS);
                    let newer = crate::press_feedback::held_ticket(&app).expect("second launch");
                    pixels(&window);
                    crate::router::finish_launch(
                        &ctx,
                        &app,
                        hold,
                        crate::router::LaunchOutcome::Ok,
                        "old launch",
                    );
                    assert!(
                        crate::press_feedback::pending(&app),
                        "old reply cannot lift new face"
                    );
                    crate::router::finish_launch(
                        &ctx,
                        &app,
                        newer,
                        crate::router::LaunchOutcome::Ok,
                        "new launch",
                    );
                }
            }
        }
        settle(&window);
        assert!(!crate::press_feedback::pending(&app));
        assert_region_matches(
            &pixels(&window),
            &resting,
            0..W as usize,
            32..H as usize - 30,
            "complete tile face/art/edge/ring must return to the same resting geometry",
        );
    }
    // A late list-row pulse must never depress a grid face without its ring.
    let view = app.global::<crate::GamesView>();
    view.set_activate_pulse(view.get_activate_pulse().wrapping_add(1));
    settle(&window);
    assert_region_matches(
        &pixels(&window),
        &resting,
        0..W as usize,
        32..H as usize - 30,
        "grid feedback has only one owner, even when list pulse state changes",
    );
    shell.set_reduce_motion(true);
    app.global::<crate::Motion>().set_enabled(false);
    settle(&window);
    let reduced = pixels(&window);
    crate::router::handle_action(&ctx, &app, "accept");
    let hold =
        crate::press_feedback::held_ticket(&app).expect("reduced motion dispatches immediately");
    assert!(crate::press_feedback::pending(&app));
    crate::router::finish_launch(&ctx, &app, hold, crate::router::LaunchOutcome::Ok, "Launch");
    settle(&window);
    assert_eq!(pixels(&window), reduced);
}

/// Back is never swallowed for decoration, even while an operation is pending.
#[test]
#[allow(
    clippy::expect_used,
    reason = "a fixture with no pressable control is a broken test"
)]
fn back_survives_both_a_push_and_a_pending_commit() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    let (_runtime, ctx) = offline_ctx();
    for dispatched in [false, true] {
        crate::router::open_quit_confirm(&app);
        let commits = Rc::new(Cell::new(0));
        arm_feedback(&app, &commits);
        assert_eq!(commits.get(), 0);
        crate::router::handle_action(&ctx, &app, "accept");
        assert_eq!(commits.get(), 0, "duplicate Accept is gated during push");
        if dispatched {
            advance(zaparoo_app::input::PRESS_FEEDBACK_MS);
            assert_eq!(commits.get(), 1);
            crate::router::handle_action(&ctx, &app, "accept");
            assert_eq!(commits.get(), 1, "duplicate Accept is gated during hold");
        }
        crate::router::handle_action(&ctx, &app, "cancel");
        assert!(!app.global::<crate::Overlays>().get_dialog_open());
        settle(&window);
        assert_eq!(
            commits.get(),
            u32::from(dispatched),
            "Back cancels only an undispatched Accept"
        );
        assert!(!crate::press_feedback::pending(&app));
    }
}

/// A launch that answers after the user has moved on must not lift whatever
/// they are pressing now.
#[test]
fn a_stale_release_cannot_lift_the_next_press() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    crate::sizing::apply_scene(
        &app,
        crate::sizing::Scene::of(&app, f64::from(W), f64::from(H), false),
    );
    crate::router::open_quit_confirm(&app);
    settle(&window);

    let stale = crate::press_feedback::keep_held(&app);
    let commits = Rc::new(Cell::new(0));
    arm_feedback(&app, &commits);

    crate::press_feedback::release(&app, stale);

    assert_ne!(
        app.global::<crate::PressFeedback>().get_owner(),
        PressOwner::None,
        "the new press owns the control now"
    );
}

#[test]
fn dialog_pushes_before_dispatch_and_pending_feedback_settles() {
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
    let time = CLOCK.with(Cell::get);
    arm_feedback(&app, &commits);
    assert_eq!(commits.get(), 0);
    assert_eq!(CLOCK.with(Cell::get), time);
    frame(&window);
    distinct_frames(&window, 2);
    app.window().request_redraw();
    assert_ne!(
        resting,
        frame(&window),
        "dialog button visibly depresses before dispatch"
    );
    assert_eq!(commits.get(), 0);
    advance(zaparoo_app::input::PRESS_FEEDBACK_MS - 2 * TICK_MS);
    assert_eq!(commits.get(), 1);
    assert!(crate::press_feedback::pending(&app));
    crate::press_feedback::cancel(&app);
    settle(&window);
    app.window().request_redraw();
    assert_eq!(resting, frame(&window));
    assert_eq!(commits.get(), 1);
}

#[test]
fn feedback_completion_never_dispatches_to_a_later_target() -> Result<(), &'static str> {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    crate::router::open_quit_confirm(&app);
    let target = crate::press_feedback::current(&app).ok_or("No target")?;
    let commits = Rc::new(Cell::new(0));
    arm_feedback(&app, &commits);
    assert_eq!(commits.get(), 0);
    app.global::<crate::Overlays>().set_dialog_focus(1);
    settle(&window);
    assert_eq!(commits.get(), 0, "Yes never inherits No's pending Accept");
    assert!(!crate::press_feedback::pending(&app));
    let count = commits.clone();
    crate::press_feedback::dispatch(&app, &target, move |_| count.set(count.get() + 1));
    settle(&window);
    assert_eq!(commits.get(), 0, "already stale target is rejected too");
    arm_feedback(&app, &commits);
    crate::press_feedback::cancel(&app);
    settle(&window);
    assert_eq!(commits.get(), 0, "canceled push never dispatches");
    assert!(!crate::press_feedback::pending(&app));
    arm_feedback(&app, &commits);
    settle(&window);
    assert_eq!(commits.get(), 1, "new target gets its own Accept");
    crate::press_feedback::cancel(&app);
    app.global::<crate::Motion>().set_enabled(false);
    arm_feedback(&app, &commits);
    assert_eq!(commits.get(), 2, "reduced motion dispatches synchronously");
    crate::press_feedback::cancel(&app);
    Ok(())
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
    for owner in [PressOwner::Letter, PressOwner::Log] {
        if owner == PressOwner::Log {
            ov.set_letter_open(false);
            let log = app.global::<crate::LogUploadView>();
            log.set_open(true);
            log.set_phase(LogPhase::Failed);
        }
        settle(&window);
        app.window().request_redraw();
        let resting = frame(&window);
        arm_feedback(&app, &commits);
        frame(&window);
        distinct_frames(&window, 2);
        app.window().request_redraw();
        assert_eq!(app.global::<crate::PressFeedback>().get_owner(), owner);
        assert_ne!(resting, frame(&window), "{owner:?} button must push down");
        advance(zaparoo_app::input::PRESS_FEEDBACK_MS - 2 * TICK_MS);
        crate::press_feedback::cancel(&app);
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
    app.global::<Shell>().set_active_screen(Screen::Settings);
    let settings = app.global::<crate::SettingsView>();
    settings.set_cells(cells(1, "Settings"));
    settings.set_cell_width(60.0);
    settings.set_cell_height(50.0);
    settings.set_grid_y(40.0);
    settings.set_grid_height(80.0);
    settings.set_rows(ModelRc::new(VecModel::from(vec![crate::SettingsRow {
        kind: RowKind::Field,
        control: ControlKind::Navigate,
        id: "pageDisplayInterface".into(),
        enabled: true,
        ..Default::default()
    }])));
    let commits = Rc::new(Cell::new(0));
    for owner in [PressOwner::Settings, PressOwner::List] {
        if owner == PressOwner::List {
            let ov = app.global::<crate::Overlays>();
            ov.set_list_entries(ModelRc::new(VecModel::from(vec![crate::MenuEntry {
                role: crate::MenuRole::default(),
                detail_key: SharedString::default(),
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
        assert_eq!(app.global::<crate::PressFeedback>().get_owner(), owner);
        assert_ne!(
            resting,
            frame(&window),
            "{owner:?} must show local feedback while pending"
        );
        advance(zaparoo_app::input::PRESS_FEEDBACK_MS - 2 * TICK_MS);
        crate::press_feedback::cancel(&app);
        settle(&window);
        app.window().request_redraw();
        assert_eq!(
            frame(&window),
            resting,
            "{owner:?} feedback must fully release"
        );
    }
    assert_eq!(commits.get(), 2);
}

/// Row geometry the way `settings::render` stacks it: `y_offset` running,
/// `height` per row, so the fixture exercises the real band arithmetic.
fn settings_rows(heights: &[f32]) -> (ModelRc<crate::SettingsRow>, Vec<f32>) {
    let ids = [
        "colorScheme",
        "colorIntensity",
        "systemLogoStyle",
        "reduceMotion",
        "screensaverTimeout",
    ];
    let mut offset = 0.0;
    let mut offsets = Vec::new();
    let rows: Vec<_> = heights
        .iter()
        .enumerate()
        .map(|(i, height)| {
            offsets.push(offset);
            let row = crate::SettingsRow {
                kind: RowKind::Field,
                control: ControlKind::Picker,
                id: ids[i % ids.len()].into(),
                enabled: true,
                y_offset: offset,
                height: *height,
                ..Default::default()
            };
            offset += height;
            row
        })
        .collect();
    (ModelRc::new(VecModel::from(rows)), offsets)
}

#[test]
fn a_240p_help_bar_grows_only_while_its_entries_wrap() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    crate::fonts::register_embedded_fonts();
    let flips = Rc::new(Cell::new(0));
    {
        let weak = app.as_weak();
        let flips = flips.clone();
        app.on_help_wrap_changed(move || {
            let Some(app) = weak.upgrade() else { return };
            if crate::sizing::sync_help_bar_rows(&app) {
                flips.set(flips.get() + 1);
                let sizing = app.global::<Sizing>();
                let (w, h) = (sizing.get_screen_width(), sizing.get_screen_height());
                crate::sizing::apply_scene(
                    &app,
                    crate::sizing::Scene::of(&app, f64::from(w), f64::from(h), false),
                );
            }
        });
    }
    // Draw until the bar's measurement and the table it feeds stop moving.
    let lay_out = |width: u32| {
        let height = 240;
        window.set_size(slint::PhysicalSize::new(width, height));
        let sizing = app.global::<Sizing>();
        sizing.set_screen_width(width as f32);
        sizing.set_screen_height(height as f32);
        crate::sizing::apply_scene(
            &app,
            crate::sizing::Scene::of(&app, f64::from(width), f64::from(height), false),
        );
        let mut pixels = vec![Rgb565Pixel(0); (width * height) as usize];
        for _ in 0..SETTLE_TICKS {
            advance(TICK_MS);
            window.request_redraw();
            window.draw_if_needed(|renderer| {
                renderer.render(&mut pixels, width as usize);
            });
        }
    };
    let sizing = app.global::<Sizing>();

    // Wide enough for one row: one 16 px glyph row and its two pads.
    lay_out(1000);
    assert!(sizing.get_tier_240());
    assert!(!app.get_help_entries_wrap());
    assert!(!sizing.get_help_bar_two_rows());
    assert_eq!(sizing.get_help_bar_height().round() as i32, 20);
    assert_eq!(flips.get(), 0);

    // Too narrow: the entries wrap and the bar takes its second row. The
    // taller bar does not change the measurement, so it flips once.
    lay_out(60);
    assert!(app.get_help_entries_wrap());
    assert!(sizing.get_help_bar_two_rows());
    assert_eq!(sizing.get_help_bar_height().round() as i32, 38);
    assert_eq!(flips.get(), 1);

    // And gives it back when they fit again.
    lay_out(1000);
    assert!(!sizing.get_help_bar_two_rows());
    assert_eq!(sizing.get_help_bar_height().round() as i32, 20);
    assert_eq!(flips.get(), 2);
}

#[test]
fn a_settings_move_that_scrolls_glides_the_band_and_snaps_without_motion() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    let scene = crate::sizing::Scene::of(&app, f64::from(W), f64::from(H), false);
    crate::sizing::apply_scene(&app, scene);
    app.global::<Shell>().set_active_screen(Screen::Settings);
    let settings = app.global::<crate::SettingsView>();
    settings.set_page(SettingsPage::Appearance);
    // The card is placed from what the driver pushes, not by the view.
    crate::settings::push_page_geometry(
        &settings,
        &zaparoo_app::settings::page_geometry(&scene.inputs()),
    );
    // The last row is taller on purpose: the fill animates its height as
    // well as its position, and a move that scrolls must do neither on its
    // own. The fill sits on its row and the band carries both.
    let heights = [30.0, 30.0, 30.0, 30.0, 60.0];
    let (rows, offsets) = settings_rows(&heights);
    settings.set_rows(rows);
    let viewport = 120.0;
    let scroll = offsets[4] + heights[4] - viewport;
    let top = |settings: &crate::SettingsView<'_>| {
        settings.set_rows_height(viewport);
        settings.set_rows_clip_height(viewport);
        settings.set_index(0);
        settings.set_scroll(0.0);
    };
    let scrolled = |settings: &crate::SettingsView<'_>| {
        settings.set_index(4);
        settings.set_scroll(scroll);
    };

    // With motion off the band is placed in one frame.
    app.global::<crate::Motion>().set_enabled(false);
    top(&settings);
    settle(&window);
    scrolled(&settings);
    let first = pixels(&window);
    settle(&window);
    let snapped = pixels(&window);
    assert_eq!(
        first, snapped,
        "without motion a move that scrolls the band lands in one frame"
    );

    // With motion on the rows glide to the same place.
    app.global::<crate::Motion>().set_enabled(true);
    top(&settings);
    settle(&window);
    scrolled(&settings);
    let travelling = pixels(&window);
    settle(&window);
    assert_ne!(
        travelling,
        pixels(&window),
        "a move that scrolls the band glides there"
    );
    assert_eq!(
        pixels(&window),
        snapped,
        "the glide settles exactly where the snap lands"
    );

    // A move inside the band still glides: same scroll, different row.
    settings.set_index(3);
    let stepping = pixels(&window);
    settle(&window);
    assert_ne!(
        stepping,
        pixels(&window),
        "a move that does not scroll still travels"
    );
}

#[test]
fn accepting_a_list_row_flashes_the_whole_row_inverted() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    crate::sizing::apply_scene(
        &app,
        crate::sizing::Scene::of(&app, f64::from(W), f64::from(H), false),
    );
    app.global::<Shell>().set_active_screen(Screen::None);
    let ov = app.global::<crate::Overlays>();
    ov.set_list_title("Pick".into());
    ov.set_list_entries(ModelRc::new(VecModel::from(vec![
        crate::MenuEntry {
            role: crate::MenuRole::default(),
            detail_key: SharedString::default(),
            id: "one".into(),
            label: "One".into(),
            ..Default::default()
        },
        crate::MenuEntry {
            role: crate::MenuRole::default(),
            detail_key: SharedString::default(),
            id: "two".into(),
            label: "Two".into(),
            ..Default::default()
        },
    ])));
    ov.set_list_index(0);
    ov.set_list_open(true);
    settle(&window);

    let theme = app.global::<crate::Theme>();
    let fill = theme.get_selection_fill();
    let inverted = theme.get_on_accent();
    let count = |buf: &[Rgb565Pixel], color: slint::Color| {
        let target = (u16::from(color.red() >> 3) << 11)
            | (u16::from(color.green() >> 2) << 5)
            | u16::from(color.blue() >> 3);
        buf.iter().filter(|pixel| pixel.0 == target).count()
    };
    let resting = pixels(&window);
    assert!(count(&resting, fill) > 0, "the selected row is filled");

    // The flash swaps the bar and its content, so the fill goes away and
    // the row paints in the color its text was using. A stroke drawn on
    // top of an unchanged fill is a different cue and was not this one.
    app.global::<crate::PressFeedback>()
        .set_owner(PressOwner::List);
    app.global::<crate::PressFeedback>().set_index(0);
    let flashed = pixels(&window);
    assert!(
        count(&flashed, fill) < count(&resting, fill) / 4,
        "the fill inverts rather than keeping its color"
    );
    assert!(
        count(&flashed, inverted) > count(&resting, inverted),
        "and the row paints in the swapped color"
    );

    app.global::<crate::PressFeedback>()
        .set_owner(PressOwner::None);
    settle(&window);
    assert_eq!(pixels(&window), resting, "and swaps back");
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
    app.global::<Shell>().set_active_screen(Screen::None);
    let info = app.global::<crate::GameInfoView>();
    info.set_modal_open(true);
    info.set_modal_name("Details".into());
    info.set_info_rows(ModelRc::new(VecModel::from(vec![crate::InfoRow {
        kind: RowKind::Field,
        key: "system".into(),
        value: "SNES".into(),
        stop: -1,
    }])));
    settle(&window);
    app.invoke_game_info_scroll(crate::ScrollAction::Down);
    assert_eq!(info.get_scroll_position(), 0.0);
    info.set_modal_description(
        "Long description with enough words to fill several viewports. "
            .repeat(80)
            .into(),
    );
    settle(&window);
    app.window().request_redraw();
    let top = frame(&window);
    app.invoke_game_info_scroll(crate::ScrollAction::PageNext);
    assert!(info.get_scroll_position() > 0.0);
    // The body glides to the new position, so compare where it settles.
    settle(&window);
    app.window().request_redraw();
    assert_ne!(top, frame(&window), "paging must move rendered content");
    for _ in 0..100 {
        app.invoke_game_info_scroll(crate::ScrollAction::PageNext);
    }
    let bottom = info.get_scroll_position();
    app.invoke_game_info_scroll(crate::ScrollAction::Down);
    assert_eq!(
        info.get_scroll_position(),
        bottom,
        "scroll must clamp at content end"
    );
    info.set_scroll_position(0.0);
    info.set_loading(true);
    app.invoke_game_info_scroll(crate::ScrollAction::Down);
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
    app.global::<Shell>().set_active_screen(Screen::About);
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
    let fav = crate::UserFlag::Favorite;
    assert_eq!(
        labels.invoke_list_systems(fav, 1),
        "1 system with favorites"
    );
    assert_eq!(
        labels.invoke_list_systems(fav, 2),
        "2 systems with favorites"
    );
    assert_eq!(labels.invoke_list_count(fav, 0), "0 favorites");
    assert_eq!(labels.invoke_list_count(fav, 1), "1 favorite");
    assert_eq!(labels.invoke_list_count(fav, 2), "2 favorites");
    let liked = crate::UserFlag::Like;
    assert_eq!(labels.invoke_list_count(liked, 1), "1 liked game");
    assert_eq!(
        labels.invoke_list_systems(crate::UserFlag::PlayLater, 2),
        "2 systems in Play later"
    );
}

#[test]
fn a_two_item_list_glides_on_the_wrap_as_well_as_the_step() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    crate::sizing::apply_scene(
        &app,
        crate::sizing::Scene::of(&app, f64::from(W), f64::from(H), false),
    );
    app.global::<Shell>().set_active_screen(Screen::None);
    let ov = app.global::<crate::Overlays>();
    ov.set_list_entries(ModelRc::new(VecModel::from(
        (0..2)
            .map(|i| crate::MenuEntry {
                role: crate::MenuRole::default(),
                detail_key: SharedString::default(),
                id: i.to_string().into(),
                label: format!("Item {i}").into(),
                label_key: "".into(),
                enabled: true,
                reason_key: "".into(),
                detail: "".into(),
            })
            .collect::<Vec<_>>(),
    )));
    ov.set_list_open(true);
    // In a two-item list the wrap is the same one-row distance as the
    // step, so it has no business snapping when the step glides.
    for (from, to, what) in [(0, 1, "step down"), (1, 0, "wrap to the top")] {
        ov.set_list_index(from);
        settle(&window);
        ov.set_list_index(to);
        let immediate = pixels(&window);
        advance(40);
        let middle = pixels(&window);
        advance(300);
        let settled = pixels(&window);
        assert_ne!(immediate, middle, "{what} must move, not jump");
        assert_ne!(middle, settled, "{what} must still be in flight at 40ms");
    }
}

#[test]
fn picker_keeps_first_row_until_focus_leaves_viewport() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    crate::sizing::apply_scene(
        &app,
        crate::sizing::Scene::of(&app, f64::from(W), f64::from(H), false),
    );
    app.global::<Shell>().set_active_screen(Screen::None);
    app.global::<crate::Theme>()
        .set_text_primary(slint::Color::from_rgb_u8(0, 255, 0));
    let ov = app.global::<crate::Overlays>();
    ov.set_list_entries(ModelRc::new(VecModel::from(
        (0..20)
            .map(|i| crate::MenuEntry {
                role: crate::MenuRole::default(),
                detail_key: SharedString::default(),
                id: i.to_string().into(),
                label: if i == 0 { "Anchor".into() } else { "".into() },
                label_key: "".into(),
                enabled: true,
                reason_key: "".into(),
                detail: "".into(),
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
    app.global::<Shell>().set_active_screen(Screen::None);
    app.global::<crate::Theme>()
        .set_bg_panel(slint::Color::from_rgb_u8(255, 0, 255));
    let ov = app.global::<crate::Overlays>();
    crate::router::open_quit_confirm(&app);
    let mut widths = Vec::new();
    for kind in [
        DialogKind::QuitConfirm,
        DialogKind::ActionError,
        DialogKind::Notice,
    ] {
        ov.set_dialog_kind(kind);
        if kind == DialogKind::ActionError {
            ov.set_dialog_error(ErrorKind::Launch);
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
    app.global::<Shell>().set_active_screen(Screen::None);
    let theme = app.global::<crate::Theme>();
    theme.set_bg_panel(slint::Color::from_rgb_u8(255, 0, 255));
    theme.set_surface_card(slint::Color::from_rgb_u8(0, 255, 255));
    let ov = app.global::<crate::Overlays>();
    ov.set_dialog_kind(DialogKind::ActionError);
    ov.set_dialog_error(ErrorKind::Launch);
    ov.set_dialog_arg("Sonic the Hedgehog".into());
    ov.set_dialog_buttons(ModelRc::new(VecModel::from(vec![DialogButton::Ok])));
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
    app.global::<Shell>().set_active_screen(Screen::None);
    let theme = app.global::<crate::Theme>();
    theme.set_text_primary(slint::Color::from_rgb_u8(0, 255, 0));
    theme.set_text_label(slint::Color::from_rgb_u8(255, 0, 255));
    crate::router::open_documentation_qr(&app);
    let overlays = app.global::<crate::Overlays>();
    assert!(overlays.get_qr_documentation());
    let modules = u32::try_from(overlays.get_qr_modules()).unwrap_or_default();
    assert!(modules > 0);
    // The code is painted in the scheme's own two colors, not black and
    // white; the quiet zone is part of the code and takes the light one,
    // so it is what bounds the matrix here.
    let rgb565 = |color: slint::Color| {
        (u16::from(color.red() >> 3) << 11)
            | (u16::from(color.green() >> 2) << 5)
            | u16::from(color.blue() >> 3)
    };
    let quiet = rgb565(theme.get_qr_light());
    let dark = rgb565(theme.get_qr_dark());
    assert_ne!(quiet, 0xffff, "the quiet zone is themed, not white");
    assert_ne!(dark, 0x0000, "the modules are themed, not black");
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
        let mut dark_modules = 0;
        for y in 35..H - 25 {
            for x in 0..W {
                let pixel = pixels[(y * W + x) as usize].0;
                if pixel == quiet {
                    left = left.min(x);
                    top = top.min(y);
                    right = right.max(x);
                    bottom = bottom.max(y);
                }
                if pixel == dark {
                    dark_modules += 1;
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
        assert!(dark_modules > 0, "QR modules must paint in the dark role");
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
    app.global::<Shell>().set_active_screen(Screen::None);
    app.global::<crate::Theme>()
        .set_text_primary(slint::Color::from_rgb_u8(0, 255, 0));
    let info = app.global::<crate::GameInfoView>();
    info.set_modal_open(true);
    info.set_modal_name("Chrono Trigger".into());
    info.set_info_rows(ModelRc::new(VecModel::from(vec![
        crate::InfoRow {
            kind: RowKind::Field,
            key: "system".into(),
            value: "Super Nintendo Entertainment System".into(),
            stop: -1,
        },
        crate::InfoRow {
            kind: RowKind::Field,
            key: "year".into(),
            value: "1995".into(),
            stop: -1,
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
    app.global::<Shell>().set_active_screen(Screen::None);
    let theme = app.global::<crate::Theme>();
    theme.set_text_label(slint::Color::from_rgb_u8(255, 0, 255));
    theme.set_text_primary(slint::Color::from_rgb_u8(0, 255, 0));
    let info = app.global::<crate::GameInfoView>();
    info.set_modal_open(true);
    info.set_modal_name("Details".into());
    info.set_info_rows(ModelRc::new(VecModel::from(vec![
        crate::InfoRow {
            kind: RowKind::Field,
            key: "system".into(),
            value: "Super Nintendo".into(),
            stop: -1,
        },
        crate::InfoRow {
            kind: RowKind::Field,
            key: "year".into(),
            value: "1994".into(),
            stop: -1,
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
    app.global::<Shell>().set_active_screen(Screen::None);
    let ov = app.global::<crate::Overlays>();
    ov.set_list_entries(ModelRc::new(VecModel::from(vec![crate::MenuEntry {
        role: crate::MenuRole::default(),
        detail_key: SharedString::default(),
        id: "zaparoo-dark".into(),
        label: "Selected label".into(),
        label_key: "".into(),
        enabled: true,
        reason_key: "".into(),
        detail: "".into(),
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
    app.global::<Shell>().set_active_screen(Screen::None);
    let ov = app.global::<crate::Overlays>();
    ov.set_list_entries(ModelRc::new(VecModel::from(vec![crate::MenuEntry {
        role: crate::MenuRole::default(),
        detail_key: SharedString::default(),
        id: "one".into(),
        label: "One".into(),
        label_key: "".into(),
        enabled: true,
        reason_key: "".into(),
        detail: "".into(),
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
        if kind == PressOwner::List && accept {
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
fn letter_picker_routes_vertical_wrap_without_moving_the_background() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    let (_runtime, ctx) = offline_ctx();
    let ov = app.global::<crate::Overlays>();
    ov.set_letter_buckets(ModelRc::new(VecModel::from(
        (0..28)
            .map(|index| crate::LetterBucket {
                label: index.to_string().into(),
                count: 1,
            })
            .collect::<Vec<_>>(),
    )));
    ov.set_letter_open(true);
    settle(&window);
    let columns = ov.get_letter_columns();
    assert!(columns > 1 && columns < 28);
    let last_row = 27 / columns * columns;
    ov.set_letter_index(1);
    crate::router::handle_action(&ctx, &app, "up");
    assert_eq!(ov.get_letter_index(), (last_row + 1).min(27));
    crate::router::handle_action(&ctx, &app, "down");
    assert_eq!(ov.get_letter_index(), (last_row + 1).min(27) % columns);
    assert_eq!(app.global::<Shell>().get_active_screen(), Screen::Hub);
    assert!(!crate::input::rapid_navigation(&ctx));
}

#[test]
fn letter_focus_glides_and_reduced_motion_snaps() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    app.global::<Shell>().set_active_screen(Screen::None);
    app.global::<crate::Theme>()
        .set_accent(slint::Color::from_rgb_u8(255, 0, 255));
    let ov = app.global::<crate::Overlays>();
    ov.set_letter_buckets(ModelRc::new(VecModel::from(
        ["A", "B", "C", "D"]
            .into_iter()
            .map(|label| crate::LetterBucket {
                label: label.into(),
                count: 12,
            })
            .collect::<Vec<_>>(),
    )));
    ov.set_letter_open(true);
    let left = |buf: &[Rgb565Pixel]| {
        buf.iter()
            .enumerate()
            .filter(|(_, pixel)| pixel.0 == 0xf81f)
            .map(|(i, _)| i % W as usize)
            .min()
            .unwrap_or(W as usize)
    };
    settle(&window);
    let first = left(&pixels(&window));
    ov.set_letter_index(1);
    pixels(&window);
    advance(24);
    let middle = left(&pixels(&window));
    advance(100);
    let second = left(&pixels(&window));
    assert!(
        first < middle && middle < second,
        "letter ring must glide: {first}, {middle}, {second}"
    );
    app.global::<crate::Motion>().set_enabled(false);
    ov.set_letter_index(0);
    assert_eq!(left(&pixels(&window)), first);
    advance(100);
    assert_eq!(left(&pixels(&window)), first);
}

#[test]
fn held_hub_tile_reappears_immediately_and_stays_visible_during_motion() {
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
    hub.set_held_local(0);
    settle(&window);
    let first = ring_left(&pixels(&window));
    assert!(first < W as usize);
    advance(400);
    assert_eq!(
        ring_left(&pixels(&window)),
        W as usize,
        "fixture starts in blink's hidden half"
    );
    hub.set_selected_local(1);
    hub.set_held_local(1);
    assert!(
        ring_left(&pixels(&window)) < W as usize,
        "movement reveals held tile immediately"
    );
    for _ in 0..5 {
        advance(12);
        assert!(
            ring_left(&pixels(&window)) < W as usize,
            "held tile cannot disappear in flight"
        );
    }
    advance(100);
    let destination = ring_left(&pixels(&window));
    assert!(first < destination && destination < W as usize);
    advance(700);
    let a = ring_left(&pixels(&window));
    advance(650);
    let b = ring_left(&pixels(&window));
    assert_ne!(a, b, "stationary tile resumes blink");
}

#[allow(
    clippy::expect_used,
    reason = "boot creates isolated persistence for the Hub fixture"
)]
fn seed_hub_pages(ctx: &crate::router::Ctx, app: &App) -> usize {
    crate::sizing::apply_scene(
        app,
        crate::sizing::Scene::of(app, f64::from(W), f64::from(H), false),
    );
    crate::hub::render(ctx, app);
    let size = {
        let mut shared = crate::router::lock(&ctx.shared);
        let size = shared.hub.grid.page_size();
        shared.all_categories = (0..size * 3).map(|i| format!("Category{i}")).collect();
        shared.hub.layout.items = shared
            .all_categories
            .iter()
            .map(|id| zaparoo_core::hub_layout::HubItem {
                kind_raw: "category".into(),
                id: id.clone(),
                ..Default::default()
            })
            .collect();
        shared.hub.layout_path = std::path::PathBuf::from(
            std::env::var_os("ZAPAROO_STATE_FILE").expect("isolated state"),
        )
        .with_file_name("hub.toml");
        shared.hub.categories_loaded = true;
        shared.hub.restore_done = true;
        size
    };
    crate::hub::rebuild(ctx, app);
    size
}

#[test]
fn hub_page_turns_animate_in_navigation_direction() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    let (_runtime, ctx) = offline_ctx();
    let ctx = std::sync::Arc::new(ctx);
    seed_hub_pages(&ctx, &app);
    crate::hub::bind_input(&ctx, &app);
    settle(&window);
    let view = app.global::<HubView>();
    for (action, page, direction) in [
        ("page_prev", 2, -1),
        ("page_next", 0, 1),
        ("page_next", 1, 1),
    ] {
        let outgoing = view.get_cells();
        crate::router::handle_action(&ctx, &app, action);
        assert_eq!(view.get_slide_dir(), direction);
        assert_eq!(
            view.get_cells(),
            outgoing,
            "source delegates survive until landing"
        );
        assert!(
            distinct_frames(&window, 18) > 5,
            "Hub {action} must slide rather than cut"
        );
        assert_eq!(view.get_page(), page);
        assert_eq!(view.get_next_cells().row_count(), 0);
    }
    app.global::<crate::HubInput>().invoke_page_requested(1);
    assert!(crate::router::lock(&ctx.shared).hub.page.sliding);
    advance(32);
    app.global::<crate::HubInput>().invoke_page_requested(1);
    assert_eq!(view.get_page(), 0);
    assert!(
        !crate::router::lock(&ctx.shared).hub.page.sliding,
        "a new command cuts obsolete motion"
    );
    settle(&window);
    assert_eq!(
        view.get_page(),
        0,
        "old completion cannot restore its destination"
    );
    app.global::<crate::Motion>().set_enabled(false);
    crate::router::handle_action(&ctx, &app, "page_next");
    assert_eq!(view.get_page(), 1);
    assert!(!crate::router::lock(&ctx.shared).hub.page.sliding);
}

#[test]
fn cached_hub_pages_interrupt_safely_and_dormancy_clears_motion() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    let (_runtime, mut ctx) = offline_ctx();
    ctx.is_mister = true;
    seed_hub_pages(&ctx, &app);
    settle(&window);
    let view = app.global::<HubView>();
    crate::mister::with_cached_page_transitions(|| {
        crate::router::handle_action(&ctx, &app, "page_prev");
        assert!(view.get_cached_transition());
        assert!(
            view.get_page_slide().abs() < f32::EPSILON,
            "cached pixels move, not the Slint grid"
        );
        assert_eq!(view.get_page(), 2);
        assert_eq!(view.get_slide_dir(), -1);
        assert!(view.get_cells().row_count() > 0);
        crate::router::handle_action(&ctx, &app, "page_next");
        assert!(!view.get_cached_transition());
        assert_eq!(view.get_page(), 0);
        settle(&window);
        assert_eq!(view.get_page(), 0);
        crate::router::handle_action(&ctx, &app, "page_next");
        assert!(view.get_cached_transition());
        crate::set_dormant(&ctx, &app, true);
        assert!(!view.get_cached_transition());
        assert!(!crate::router::lock(&ctx.shared).hub.page.sliding);
        settle(&window);
        assert_eq!(view.get_page(), 1);
    });
    crate::set_dormant(&ctx, &app, false);
    crate::router::handle_action(&ctx, &app, "page_next");
    assert_eq!(view.get_page(), 2);
    assert!(
        !crate::router::lock(&ctx.shared).hub.page.sliding,
        "uncached HDMI never repaints a live full grid"
    );
}

#[test]
fn hub_move_pages_keep_held_identity_and_cancel_rejects_late_completion() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    let (_runtime, ctx) = offline_ctx();
    let size = seed_hub_pages(&ctx, &app);
    let original = crate::router::lock(&ctx.shared).hub.layout.clone();
    settle(&window);
    crate::router::handle_action(&ctx, &app, "context_menu");
    assert!(app.global::<crate::Overlays>().get_context_open());
    focus_context_row(&app, "hub_move");
    crate::router::handle_action(&ctx, &app, "accept");
    settle(&window);
    assert!(crate::router::lock(&ctx.shared).hub.move_armed());
    crate::router::handle_action(&ctx, &app, "right");
    assert_eq!(
        app.global::<HubView>().get_move_origins().row_data(0),
        Some(1)
    );
    assert!(
        distinct_frames(&window, 8) > 2,
        "real Move input animates the local swap"
    );
    crate::router::handle_action(&ctx, &app, "page_next");
    assert!(crate::router::lock(&ctx.shared).hub.page.sliding);
    assert_eq!(
        crate::router::lock(&ctx.shared).hub.grid.current_index(),
        size + 1
    );
    assert!(distinct_frames(&window, 5) > 2);
    crate::router::handle_action(&ctx, &app, "cancel");
    settle(&window);
    let shared = crate::router::lock(&ctx.shared);
    assert_eq!(shared.hub.layout, original);
    assert_eq!(shared.hub.grid.current_index(), 0);
    assert!(!shared.hub.move_armed());
    assert!(!shared.hub.page.sliding);
    assert_eq!(app.global::<HubView>().get_page(), 0);
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

/// The tile that trades places with the held one slides home; it does not
/// appear there. Only the held tile was ever animating, and a blink that
/// takes the held tile away mid-move left the swap with nothing moving in
/// it at all.
///
/// The neighbour is marked `hidden` so it paints a muted `borderMid` edge
/// instead of the usual `tileEdge`, which is what makes it findable: with
/// three identical plates a frame comparison picks up the held tile's own
/// glide and says nothing about the neighbour.
#[test]
fn a_hub_swap_slides_the_neighbour_home() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    let hub = app.global::<HubView>();
    // One cell gets a cover placeholder in a color nothing else paints, so
    // its position can be read straight off the frame. Every
    // `PressableSurface` uses `borderMid` and `tileEdge`, so neither of
    // those singles a tile out, and glyphs do not render here at all --
    // the provider callback belongs to the real app, not to `boot`.
    // `order` is the entry now sitting in each slot, so a swap changes
    // which name each slot carries -- exactly what the app publishes.
    let board = |order: [usize; 3]| {
        order
            .iter()
            .map(|&entry| GridCell {
                name: SharedString::from(format!("Tile {entry}")),
                // Entry 0 is the neighbour, wherever it currently sits. The
                // held entry cannot carry the mark: it blinks, so it is
                // missing from half the frames.
                placeholder: slint::Color::from_rgb_u8(255, 0, 255),
                has_placeholder: entry == 0,
                ..GridCell::default()
            })
            .collect::<Vec<_>>()
    };
    let publish = |rows: Vec<GridCell>| {
        let view = app.global::<HubView>();
        crate::view_model::publish_hub_cells(&view.get_cells(), rows, |m| view.set_cells(m));
    };
    publish(board([0, 1, 2]));
    hub.set_columns(3);
    hub.set_cell_width(50.0);
    hub.set_cell_height(40.0);
    // Clear of the header, so the probe band holds only tiles.
    hub.set_grid_y(80.0);
    hub.set_grid_height(90.0);
    hub.set_selected_local(1);
    hub.set_held_local(1);
    settle(&window);

    let marked_left = |buf: &[Rgb565Pixel]| {
        let target = 0xf81fu16;
        (0..W).find(|x| (82..168).any(|y| buf[(y * W + *x) as usize].0 == target))
    };

    // Hold cell 0 and swap it with cell 1, so cell 1 has to travel one
    // column to the right.
    publish(board([1, 0, 2]));
    hub.set_move_origins(ModelRc::new(VecModel::from(vec![1, 0, 2])));
    hub.set_selected_local(0);
    hub.set_held_local(0);
    hub.set_move_pulse(1);

    advance(2);
    pixels(&window);
    advance(24);
    let midway = marked_left(&pixels(&window));
    advance(150);
    let home = marked_left(&pixels(&window));

    assert!(
        midway.is_some() && home.is_some(),
        "the neighbour must paint"
    );
    assert_ne!(
        midway, home,
        "the neighbour teleported home instead of sliding: {midway:?} then {home:?}"
    );
}

#[test]
fn confirm_defaults_to_no_on_the_left() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, _) = boot();
    app.global::<Shell>().set_active_screen(Screen::None);
    crate::router::open_quit_confirm(&app);
    let ov = app.global::<crate::Overlays>();
    assert_eq!(ov.get_dialog_focus(), 0);
    assert_eq!(ov.get_dialog_buttons().row_data(0), Some(DialogButton::No));
    assert_eq!(ov.get_dialog_buttons().row_data(1), Some(DialogButton::Yes));
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
fn setup_picker_holds_keep_row_cadence_past_page_and_letter_thresholds() {
    use slint::platform::WindowEvent;
    use zaparoo_app::input::{REPEAT_INITIAL_MS, REPEAT_TICK_MS};

    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    let (_runtime, ctx) = offline_ctx();
    let ctx = std::sync::Arc::new(ctx);
    {
        let mut shared = crate::router::lock(&ctx.shared);
        shared.systems = (0..80)
            .map(|index| zaparoo_core::media_types::SystemInfo {
                id: format!("System{index:02}"),
                name: format!("System {index:02}"),
                ..Default::default()
            })
            .collect();
        shared.setup.open = true;
        shared.setup.picker = Some(zaparoo_app::media_setup::FormRow::Systems);
        shared.input.advance_test_clock(1);
    }
    crate::media_setup::render(&ctx, &app);
    crate::input::bind(&ctx, &app, std::collections::HashMap::new());
    settle(&window);
    // Row 0 is All systems and row 1 the header over the systems, which
    // the cursor passes over; start among the systems themselves.
    for (key, start, down) in [
        (slint::platform::Key::DownArrow, 2, true),
        (slint::platform::Key::UpArrow, 60, false),
    ] {
        crate::router::lock(&ctx.shared).setup.picker_index = start;
        crate::media_setup::render(&ctx, &app);
        let picker_rows = app.global::<crate::SetupModalView>().get_picker_rows();
        let key: SharedString = char::from(key).to_string().into();
        app.window()
            .dispatch_event(WindowEvent::KeyPressed { text: key.clone() });
        let step = |offset| if down { start + offset } else { start - offset };
        assert_eq!(crate::router::lock(&ctx.shared).setup.picker_index, step(1));
        for tick in 0..50 {
            let delay = if tick == 0 {
                REPEAT_INITIAL_MS
            } else {
                REPEAT_TICK_MS
            };
            crate::router::lock(&ctx.shared)
                .input
                .advance_test_clock(delay);
            advance(delay);
            assert_eq!(
                crate::router::lock(&ctx.shared).setup.picker_index,
                step(tick + 2),
                "repeat {tick} must still step every {REPEAT_TICK_MS} ms"
            );
            assert!(!crate::input::rapid_navigation(&ctx));
            assert!(
                app.global::<crate::SetupModalView>().get_picker_rows() == picker_rows,
                "scrolling must retain picker delegates"
            );
            frame(&window);
        }
        app.window()
            .dispatch_event(WindowEvent::KeyReleased { text: key });
        let stopped = crate::router::lock(&ctx.shared).setup.picker_index;
        crate::router::lock(&ctx.shared)
            .input
            .advance_test_clock(300);
        advance(300);
        assert_eq!(crate::router::lock(&ctx.shared).setup.picker_index, stopped);
    }
}

#[test]
fn header_logo_uses_a_cached_paint_sized_raster_at_540p() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    crate::theme::apply_palette(&app, "zaparoo-dark", "normal");
    let (width, height) = (960, 540);
    window.set_size(slint::PhysicalSize::new(width, height));
    let sizing = app.global::<Sizing>();
    sizing.set_screen_width(width as f32);
    sizing.set_screen_height(height as f32);
    crate::sizing::apply_scene(
        &app,
        crate::sizing::Scene::of(&app, f64::from(width), f64::from(height), false),
    );
    let brand = app.global::<crate::Brand>();
    let painted_width = sizing.get_header_height() * brand.get_aspect();
    let image = brand.invoke_logo(false, painted_width);
    assert_eq!(image.size().width, painted_width.round() as u32);
    assert_eq!(
        image.size().height,
        sizing.get_header_height().round() as u32
    );
    assert_eq!(
        brand.invoke_saver_logo(painted_width),
        image,
        "screensaver motion must share the prepared raster"
    );
    let capture = || {
        let mut pixels = vec![Rgb565Pixel(0); (width * height) as usize];
        window.request_redraw();
        assert!(window.draw_if_needed(|renderer| {
            renderer.render(&mut pixels, width as usize);
        }));
        pixels
    };
    let with_logo = capture();
    brand.on_logo(|_, _| slint::Image::default());
    let without_logo = capture();
    let changed: Vec<_> = with_logo
        .iter()
        .zip(&without_logo)
        .enumerate()
        .filter(|(_, (a, b))| a != b)
        .map(|(index, _)| index)
        .collect();
    assert!(changed.len() > 100, "prepared header logo must paint");
    assert!(changed
        .iter()
        .all(|index| index / (width as usize) < sizing.get_header_bottom() as usize));
}

#[test]
fn folder_title_descenders_stay_out_of_the_cached_grid_band() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    crate::fonts::register_embedded_fonts();
    crate::theme::apply_palette(&app, "zaparoo-dark", "normal");
    let (_runtime, ctx) = offline_ctx();
    app.global::<Shell>().set_active_screen(Screen::Games);
    app.global::<crate::Motion>().set_enabled(false);
    {
        let mut shared = crate::router::lock(&ctx.shared);
        shared.games.rows = game_rows("Game", 40).into();
        shared.games.grid.set_item_count(40);
        shared.games.focus_armed = true;
        shared.games.restore_done = true;
    }
    for (width, height) in [(960, 540), (1280, 720), (1920, 1080)] {
        window.set_size(slint::PhysicalSize::new(width, height));
        app.global::<Sizing>().set_screen_width(width as f32);
        app.global::<Sizing>().set_screen_height(height as f32);
        crate::sizing::apply_scene(
            &app,
            crate::sizing::Scene::of(&app, f64::from(width), f64::from(height), false),
        );
        crate::games::render(&ctx, &app);
        let view = app.global::<crate::GamesView>();
        let top = view.get_grid_y().round() as usize;
        let bottom = top + view.get_grid_height().round() as usize;
        let capture = || {
            let mut pixels = vec![Rgb565Pixel(0); (width * height) as usize];
            window.request_redraw();
            assert!(window.draw_if_needed(|renderer| {
                renderer.render(&mut pixels, width as usize);
            }));
            pixels
        };
        view.set_title("gyjpq".into());
        let titled = capture();
        view.set_title("".into());
        let blank = capture();
        let start = top * width as usize;
        let end = bottom * width as usize;
        assert!(titled[..start] != blank[..start], "title must paint above grid: top={top}, bottom={bottom}, loading={}, count={}, strip={}", view.get_loading(), view.get_count(), app.global::<crate::Layout>().get_top_strip_visible());
        let differences = titled[start..end]
            .iter()
            .zip(&blank[start..end])
            .filter(|(a, b)| a != b)
            .count();
        assert_eq!(
            differences, 0,
            "{width}x{height}: title pixels must not enter cached page band at y={top}"
        );
    }
}

/// The CRT presenters slide the grid's band on a page turn, and the live
/// strip clips to it, so the footer label under it (drawn a pixel up on CRT)
/// must start at or below the band's bottom, with the help bar shown or
/// hidden. In pixels: `crt-hub`'s label once had its top row inside it.
#[test]
fn crt_hub_footer_label_stays_out_of_the_sliding_band() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    let (_runtime, ctx) = offline_ctx();
    app.global::<crate::Theme>().set_crt(true);
    app.global::<Sizing>().set_crt(true);
    seed_hub_tiles(&ctx, &app);
    let (width, height) = (316u32, 216u32);
    for help_bar in [true, false] {
        window.set_size(slint::PhysicalSize::new(width, height));
        let sizing = app.global::<Sizing>();
        sizing.set_screen_width(width as f32);
        sizing.set_screen_height(height as f32);
        sizing.set_help_bar_enabled(help_bar);
        crate::sizing::apply_scene(
            &app,
            crate::sizing::Scene::of(&app, f64::from(width), f64::from(height), true),
        );
        crate::hub::render(&ctx, &app);
        let view = app.global::<HubView>();
        let raise = sizing.get_label_raise();
        assert!(
            (raise - 1.0).abs() < f32::EPSILON,
            "the CRT label sits a pixel up"
        );
        let (band_y, band_h) = crate::page_motion::cached_band(&view, &app);
        let label_top = view.get_label_y() - raise;
        assert!(
            band_y + band_h <= label_top,
            "help bar {help_bar}: band ends at {} but the label starts at {label_top}",
            band_y + band_h
        );
        // The live strip clips to the same band.
        assert!((view.get_grid_height() - raise - band_h).abs() < f32::EPSILON);
    }
}

#[test]
fn held_window_down_key_enters_rapid_mode_without_direct_repeat_dispatch() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    let (_runtime, ctx) = offline_ctx();
    let ctx = std::sync::Arc::new(ctx);
    {
        let mut shared = crate::router::lock(&ctx.shared);
        shared.games.rows = game_rows("Game", 1_000).into();
        shared.games.grid.set_item_count(1_000);
        shared.games.focus_armed = true;
        shared.games.restore_done = true;
        shared.input.advance_test_clock(1);
    }
    app.global::<Shell>().set_active_screen(Screen::Games);
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
    assert!(app.global::<crate::GamesView>().get_rail_visible());
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
        shared.games.rows = game_rows("Game", 40).into();
        shared.games.grid.set_item_count(40);
        shared.games.focus_armed = true;
    }
    app.global::<Shell>().set_active_screen(Screen::Games);
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
    // The window carries a page of rows either side, and no more.
    assert!(view.get_list_rows().row_count() <= 3 * visible as usize);
    let edge = pixels(&window);
    advance(32);
    assert_ne!(
        pixels(&window),
        edge,
        "the list glides one row to follow the selection past its edge"
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
            shared.games.rows = game_rows("Parent", 8).into();
            shared.games.rows[0].entry_type = zaparoo_app::media_list::EntryType::Directory;
            shared.games.rows[0].path = "/parent/child".into();
            shared.games.grid.set_item_count(8);
            shared.games.grid.set_current_index_immediate(0);
            shared.games.focus_armed = true;
            shared.games.restore_done = true;
            shared.games.browse_path = "/parent".into();
        }
        app.global::<Shell>().set_active_screen(Screen::Games);
        app.global::<Shell>().set_browse_list_layout(list);
        crate::router::refresh_layout(&app);
        crate::games::render(&ctx, &app);
        settle(&window);
        let title = app.global::<crate::GamesView>().get_title();
        let source = pixels(&window);
        crate::router::handle_action(&ctx, &app, "accept");
        if !list {
            assert!(crate::press_feedback::pending(&app));
            assert!(!app.global::<Shell>().get_transitioning());
            pixels(&window);
            advance(zaparoo_app::input::PRESS_FEEDBACK_MS);
            assert!(app.global::<Shell>().get_transitioning());
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
            advance(zaparoo_app::input::PRESS_FEEDBACK_MS);
            assert!(app.global::<Shell>().get_transitioning());
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
    crate::router::begin_pending(&app, Screen::Games);
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
    assert_eq!(app.global::<Shell>().get_active_screen(), Screen::Hub);
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
    assert_eq!(app.global::<Shell>().get_active_screen(), Screen::Games);
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
        app.global::<Shell>().set_active_screen(Screen::Hub);
        {
            let mut shared = crate::router::lock(&ctx.shared);
            shared.persist.active_screen = "hub".into();
            shared.persist.settings.systems_browse_layout = "list".into();
            shared.persist.favorite_systems.selected_path = "System08".into();
            shared.persist.favorite_systems.list_top = Some(6);
        }
        settle(&window);
        crate::systems::enter_list(&ctx, &app, Flag::Favorite, EntryMode::Restore);
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
            Screen::FavoriteSystems
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
        crate::router::begin_pending(&app, Screen::Games);
        let ticket = crate::router::lock(&ctx.shared).games.ticket;
        if timeout {
            advance(15_001);
        } else {
            crate::games::on_append(
                &ctx,
                &app,
                ticket,
                Err(crate::games::PageError::transport("offline")),
            );
        }
        assert!(!app.global::<Shell>().get_transitioning());
        assert_eq!(app.global::<Shell>().get_active_screen(), Screen::Hub);
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
    // Move mode blinks the held tile out of existence and back on a fixed
    // cycle. Sampled a half cycle apart rather than against `endpoint`, so
    // the assertion does not depend on where in the cycle the swap happened
    // to finish.
    advance(700);
    let blink_a = pixels(&window);
    advance(650);
    let blink_b = pixels(&window);
    advance(650);
    assert_ne!(blink_a, blink_b, "the held tile blinks while it is held");
    assert_eq!(
        pixels(&window),
        blink_a,
        "and the blink is a two-state cut, not a drift"
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
    // Freezing a disappear cue on "gone" would hide the tile being moved,
    // so reduce motion rests it on, not off.
    advance(2_000);
    assert_eq!(
        pixels(&window),
        snapped,
        "reduced motion leaves the held tile painted, not blinking"
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
fn grid_push_lowers_face_art_and_ring_before_navigation() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    crate::sizing::apply_scene(
        &app,
        crate::sizing::Scene::of(&app, f64::from(W), f64::from(H), false),
    );
    let shell = app.global::<Shell>();
    shell.set_systems_list_layout(false);
    shell.set_browse_list_layout(false);
    for screen in [
        Screen::Hub,
        Screen::Systems,
        Screen::Games,
        Screen::Settings,
    ] {
        let owner = screen.token(); // Stable screenshot filename, not routing state.
        shell.set_active_screen(screen);
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
        match screen {
            Screen::Hub => {
                let view = app.global::<HubView>();
                view.set_cells(page);
                view.set_cell_width(70.0);
                view.set_cell_height(60.0);
                view.set_grid_y(40.0);
                view.set_grid_height(100.0);
            }
            Screen::Systems => {
                let view = app.global::<SystemsView>();
                view.set_cells(page);
                view.set_count(1);
                view.set_focus_ready(true);
                view.set_cell_width(70.0);
                view.set_cell_height(60.0);
                view.set_grid_y(40.0);
                view.set_grid_height(100.0);
            }
            Screen::Games => {
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
                    kind: RowKind::Field,
                    control: ControlKind::Navigate,
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
            crate::router::transition_to_screen(app, Screen::About, 1);
        });
        assert!(crate::press_feedback::pending(&app));
        assert!(!shell.get_transitioning());
        pixels(&window);
        advance(16);
        save_push_evidence(&format!("{owner}-1-downstroke"), &pixels(&window));
        advance(32);
        let depressed = pixels(&window);
        save_push_evidence(&format!("{owner}-2-depressed"), &depressed);
        assert_eq!(
            shell.get_active_screen(),
            screen,
            "push keeps source visible before navigation"
        );
        let sizing = app.global::<Sizing>();
        let layout = app.global::<crate::Layout>();
        let left = if screen == Screen::Hub {
            sizing.get_hub_grid_side_inset()
        } else {
            layout.get_grid_left_inset()
        } as usize;
        let top = 40
            + if screen == Screen::Hub {
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
        advance(zaparoo_app::input::PRESS_FEEDBACK_MS - 48 - 1);
        assert_eq!(
            shell.get_active_screen(),
            screen,
            "fully depressed source must remain visible"
        );
        advance(1);
        assert_eq!(shell.get_active_screen(), Screen::About);
        assert!(!crate::press_feedback::pending(&app));
        save_push_evidence(&format!("{owner}-3-destination"), &pixels(&window));
    }
}

#[test]
fn window_and_pointer_accept_push_settings_tiles_before_opening_page() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    let (_runtime, ctx) = offline_ctx();
    crate::sizing::apply_scene(
        &app,
        crate::sizing::Scene::of(&app, f64::from(W), f64::from(H), false),
    );
    app.global::<Shell>().set_active_screen(Screen::Settings);
    let input_ctx = std::sync::Arc::new(ctx.clone());
    crate::input::bind(&input_ctx, &app, std::collections::HashMap::new());
    crate::settings::bind_input(&input_ctx, &app);
    let view = app.global::<crate::SettingsView>();
    for pointer in [false, true] {
        crate::settings::open_page(&ctx, &app, SettingsPage::Root);
        view.set_index(0);
        settle(&window);
        let raised = pixels(&window);
        let time = CLOCK.with(Cell::get);
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
        assert_eq!(CLOCK.with(Cell::get), time);
        assert_eq!(view.get_page(), SettingsPage::Root);
        assert!(crate::press_feedback::pending(&app));
        let prefix = if pointer {
            "settings-pointer"
        } else {
            "settings-key"
        };
        save_push_evidence(&format!("{prefix}-0-raised"), &raised);
        pixels(&window);
        advance(48);
        let depressed = pixels(&window);
        save_push_evidence(&format!("{prefix}-1-depressed"), &depressed);
        assert_ne!(raised, depressed, "category tile pushes before navigation");
        assert_eq!(view.get_page(), SettingsPage::Root);
        advance(zaparoo_app::input::PRESS_FEEDBACK_MS - 48 - 1);
        assert_eq!(view.get_page(), SettingsPage::Root);
        assert_eq!(depressed, pixels(&window), "push holds until dispatch");
        advance(1);
        assert_eq!(view.get_page(), SettingsPage::Library);
        assert!(!crate::press_feedback::pending(&app));
        save_push_evidence(&format!("{prefix}-2-page"), &pixels(&window));
        crate::router::handle_action(&ctx, &app, "cancel");
        settle(&window);
        assert_eq!(view.get_page(), SettingsPage::Root);
        assert_eq!(raised, pixels(&window), "return restores raised tile");
    }
    crate::router::handle_action(&ctx, &app, "accept");
    assert_eq!(view.get_page(), SettingsPage::Root);
    assert!(crate::press_feedback::pending(&app));
    app.invoke_input_lost();
    advance(zaparoo_app::input::PRESS_FEEDBACK_MS);
    assert_eq!(
        view.get_page(),
        SettingsPage::Root,
        "input loss cancels pending navigation"
    );
    assert!(!crate::press_feedback::pending(&app));
    app.global::<crate::Motion>().set_enabled(false);
    crate::router::handle_action(&ctx, &app, "accept");
    assert_eq!(view.get_page(), SettingsPage::Library);
    app.invoke_input_lost();
    assert_eq!(
        view.get_page(),
        SettingsPage::Library,
        "input loss cannot retract completed navigation"
    );
    assert!(!crate::press_feedback::pending(&app));
}

#[test]
fn hub_settings_tile_pushes_before_opening_ready_screen() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    let (_runtime, ctx) = offline_ctx();
    crate::sizing::apply_scene(
        &app,
        crate::sizing::Scene::of(&app, f64::from(W), f64::from(H), false),
    );
    {
        let mut shared = crate::router::lock(&ctx.shared);
        shared.hub.layout.items = vec![zaparoo_core::hub_layout::HubItem {
            kind_raw: "action".into(),
            id: "settings".into(),
            ..Default::default()
        }];
        shared.hub.categories_loaded = true;
        shared.hub.restore_done = true;
    }
    crate::hub::rebuild(&ctx, &app);
    settle(&window);
    let raised = pixels(&window);
    save_push_evidence("hub-settings-0-raised", &raised);
    let shell = app.global::<Shell>();
    crate::router::handle_action(&ctx, &app, "accept");
    assert_eq!(shell.get_active_screen(), Screen::Hub);
    assert_eq!(
        app.global::<crate::PressFeedback>().get_owner(),
        PressOwner::Hub
    );
    pixels(&window);
    advance(48);
    let depressed = pixels(&window);
    save_push_evidence("hub-settings-1-depressed", &depressed);
    assert_ne!(raised, depressed, "Hub tile pushes before navigation");
    assert_eq!(shell.get_active_screen(), Screen::Hub);
    advance(zaparoo_app::input::PRESS_FEEDBACK_MS - 48 - 1);
    assert_eq!(shell.get_active_screen(), Screen::Hub);
    assert_eq!(depressed, pixels(&window), "push holds until dispatch");
    advance(1);
    assert_eq!(shell.get_active_screen(), Screen::Settings);
    assert_eq!(
        app.global::<crate::SettingsView>().get_page(),
        SettingsPage::Root
    );
    assert!(!crate::press_feedback::pending(&app));
    save_push_evidence("hub-settings-2-screen", &pixels(&window));

    crate::router::handle_action(&ctx, &app, "cancel");
    app.global::<crate::Motion>().set_enabled(false);
    crate::router::handle_action(&ctx, &app, "accept");
    assert_eq!(shell.get_active_screen(), Screen::Settings);
    assert!(!crate::press_feedback::pending(&app));
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
    app.global::<Shell>().set_active_screen(Screen::Games);
    {
        let mut shared = crate::router::lock(&ctx.shared);
        shared.games.rows = game_rows("Folder", 200).into();
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
        assert!(shared.games.page.sliding);
        shared.games.grid.current_index()
    };
    crate::router::handle_action(&ctx, &app, "accept");
    assert!(!crate::router::lock(&ctx.shared).games.page.sliding);
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
    for (target, direction) in [(Screen::Systems, 1), (Screen::Hub, -1)] {
        crate::router::transition_to_screen(&app, target, direction);
        assert_eq!(app.global::<Shell>().get_active_screen(), target);
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
    crate::router::begin_pending(&app, Screen::Systems);
    for ticks in [1, 6, 20, 100] {
        distinct_frames(&window, ticks);
        assert_region_matches(
            &pixels(&window),
            &source,
            0..W as usize,
            55..H as usize,
            "source body stays intact while waiting",
        );
        assert_eq!(app.global::<Shell>().get_active_screen(), Screen::Hub);
    }
    crate::router::transition_to_screen(&app, Screen::Systems, 1);
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

    crate::router::begin_pending(&app, Screen::Systems);
    distinct_frames(&window, 3);
    assert!(!app.global::<Shell>().get_transition_cue());
    crate::router::transition_to_screen(&app, Screen::Systems, 1);
    assert_eq!(app.global::<Shell>().get_active_screen(), Screen::Systems);
    distinct_frames(&window, SETTLE_TICKS);
    assert!(!app.global::<Shell>().get_transition_cue());
}

#[test]
fn a_slow_fill_keeps_source_and_adds_static_delayed_feedback() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    settle(&window);

    crate::router::begin_pending(&app, Screen::Systems);
    assert_eq!(
        app.global::<Shell>().get_transition_target(),
        Screen::Systems
    );
    distinct_frames(&window, 24);
    let shell = app.global::<Shell>();
    assert!(shell.get_transition_cue());
    assert_eq!(shell.get_active_screen(), Screen::Hub);
    let held = pixels(&window);
    assert_eq!(
        distinct_frames(&window, 8),
        0,
        "pending wait must be static"
    );
    assert_eq!(held, pixels(&window));

    crate::router::transition_to_screen(&app, Screen::Systems, 1);
    assert_eq!(shell.get_active_screen(), Screen::Systems);
    distinct_frames(&window, SETTLE_TICKS);
    assert!(!shell.get_transitioning());
    assert!(!shell.get_transition_cue());
}

#[test]
fn a_batch_of_landed_covers_repaints_games_once() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, _window) = boot();
    let (_runtime, ctx) = offline_ctx();
    crate::sizing::apply_scene(
        &app,
        crate::sizing::Scene::of(&app, f64::from(W), f64::from(H), false),
    );
    app.global::<Shell>().set_active_screen(Screen::Games);
    let mut rows = game_rows("Game", 40);
    for (index, row) in rows.iter_mut().enumerate() {
        row.path = format!("/g/{index}");
        row.system_id = "NES".into();
        row.has_cover = true;
    }
    {
        let mut shared = crate::router::lock(&ctx.shared);
        shared.games.loading = false;
        shared.games.rows = rows.into();
        shared.games.grid.set_item_count(40);
    }
    crate::games::render(&ctx, &app);
    let on_page: Vec<_> = ctx.media.pending_keys().into_iter().take(3).collect();
    assert_eq!(on_page.len(), 3);
    let renders = || crate::games::RENDERS.with(Cell::get);
    let before = renders();
    crate::deliver_covers(&ctx, &app, &on_page);
    assert_eq!(renders() - before, 1, "three previews, one repaint");
    let full_art: Vec<_> = ctx
        .media
        .pending_keys()
        .into_iter()
        .filter(|key| key.max_size > zaparoo_app::covers::COLOR_PREVIEW_MAX_SIZE)
        .take(3)
        .collect();
    assert_eq!(full_art.len(), 3);
    let before = renders();
    crate::deliver_covers(&ctx, &app, &full_art);
    assert_eq!(renders() - before, 1, "three full covers, one repaint");
    let elsewhere: Vec<_> = on_page
        .into_iter()
        .map(|mut key| {
            key.path = format!("/other{}", key.path);
            key
        })
        .collect();
    let before = renders();
    crate::deliver_covers(&ctx, &app, &elsewhere);
    assert_eq!(renders(), before, "covers nobody shows repaint nothing");
}

#[test]
fn fresh_settings_entry_resets_the_page_and_row() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    let (_runtime, ctx) = offline_ctx();
    let view = app.global::<crate::SettingsView>();
    crate::settings::enter(&ctx, &app, EntryMode::Fresh);
    settle(&window);
    assert_eq!(view.get_page(), SettingsPage::Root);
    assert_eq!(view.get_index(), 0);
    crate::settings::handle_action(&ctx, &app, "right");
    let left_on = view.get_index();
    assert!(left_on > 0, "the move must land on another category");
    crate::settings::handle_action(&ctx, &app, "cancel");
    settle(&window);
    // Something else resets the view meanwhile; the memory is Rust's.
    view.set_index(0);
    crate::settings::enter(&ctx, &app, EntryMode::Fresh);
    settle(&window);
    assert_eq!(view.get_page(), SettingsPage::Root);
    assert_eq!(view.get_index(), 0);
    crate::router::lock(&ctx.shared).settings_focus = Some((SettingsPage::Appearance, 999));
    crate::settings::enter(&ctx, &app, EntryMode::Fresh);
    assert_eq!(view.get_page(), SettingsPage::Root);
    assert_eq!(view.get_index(), 0);
}

#[test]
fn a_failed_browse_of_a_remembered_folder_keeps_the_memory() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, _window) = boot();
    let (_runtime, ctx) = offline_ctx();
    let snes = zaparoo_core::media_types::SystemInfo {
        id: "SNES".into(),
        name: "SNES".into(),
        media_count: Some(1),
        ..Default::default()
    };
    let remembered = zaparoo_core::persist::SystemFocus {
        system_id: "SNES".into(),
        path_stack: vec![String::new(), "/snes/rpg".into()],
        selected_at_level: vec!["/snes/rpg".into(), "/snes/rpg/z.sfc".into()],
        list_top_at_level: vec![0, 2],
    };
    crate::router::lock(&ctx.shared).persist.games = zaparoo_core::persist::GamesState {
        system_id: remembered.system_id.clone(),
        path_stack: remembered.path_stack.clone(),
        selected_at_level: remembered.selected_at_level.clone(),
        list_top_at_level: remembered.list_top_at_level.clone(),
        system_focus: vec![remembered.clone()],
        ..Default::default()
    };
    crate::games::enter_restored(&ctx, &app, &snes);
    let ticket = {
        let shared = crate::router::lock(&ctx.shared);
        assert!(shared.games.focus_recalled);
        shared.games.ticket
    };
    // The link drops, or Core is busy: an error, not an empty folder. The
    // entry is called off as any failed navigation is, and the remembered
    // position survives for the next visit.
    crate::games::show_error(&ctx, &app, ticket, "not connected", true);
    let shared = crate::router::lock(&ctx.shared);
    assert!(!shared.games.focus_recalled);
    assert_eq!(shared.persist.games.system_focus, vec![remembered]);
}

#[test]
fn a_fresh_system_entry_ignores_its_remembered_folder_and_viewport() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, _window) = boot();
    let (_runtime, ctx) = offline_ctx();
    let system = |id: &str| zaparoo_core::media_types::SystemInfo {
        id: id.into(),
        name: id.into(),
        media_count: Some(1),
        ..Default::default()
    };
    crate::router::lock(&ctx.shared).persist.games = zaparoo_core::persist::GamesState {
        system_id: "SNES".into(),
        path_stack: vec![String::new(), "/snes/rpg".into()],
        selected_at_level: vec!["/snes/rpg".into(), "/snes/rpg/z.sfc".into()],
        list_top_at_level: vec![0, 2],
        ..Default::default()
    };
    // Leaving SNES for NES: NES has no memory yet and starts at its root.
    crate::games::enter(&ctx, &app, &system("NES"));
    {
        let shared = crate::router::lock(&ctx.shared);
        assert_eq!(shared.persist.games.system_id, "NES");
        assert_eq!(shared.persist.games.path_stack, vec![String::new()]);
        assert!(!shared.games.focus_recalled);
    }
    crate::navigation::finish(&app);
    // Selecting SNES again is a new visit, not Back or process resume.
    crate::games::enter(&ctx, &app, &system("SNES"));
    let ticket = {
        let shared = crate::router::lock(&ctx.shared);
        assert_eq!(shared.persist.games.path_stack, vec![String::new()]);
        assert_eq!(shared.persist.games.selected_at_level, vec![String::new()]);
        assert!(shared
            .persist
            .games
            .list_top_at_level
            .iter()
            .all(|top| *top == 0));
        assert_eq!(shared.games.browse_path, "");
        assert!(!shared.games.focus_recalled);
        shared.games.ticket
    };
    crate::games::apply_fill(&ctx, &app, ticket, game_rows("Root", 40), None, None, true);
    assert_eq!(app.global::<crate::GamesView>().get_current_index(), 0);
    assert_eq!(
        crate::router::lock(&ctx.shared).games.grid.current_page(),
        0
    );
}

fn navigation_catalog() -> Vec<zaparoo_core::media_types::SystemInfo> {
    (0..20)
        .map(|index| zaparoo_core::media_types::SystemInfo {
            id: format!("System{index:02}"),
            name: format!("System {index:02}"),
            category: "Console".into(),
            media_count: Some(40),
            ..Default::default()
        })
        .collect()
}

fn navigation_rows() -> Vec<crate::games::GameRow> {
    game_rows("Game", 40)
        .into_iter()
        .enumerate()
        .map(|(index, mut row)| {
            row.path = format!("/games/{index}");
            row.system_id = "System08".into();
            row
        })
        .collect()
}

#[test]
fn add_to_hub_from_systems_names_a_launchable_tile_only() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, _window) = boot();
    let (_runtime, ctx) = offline_ctx();
    {
        let mut shared = crate::router::lock(&ctx.shared);
        let mut catalog = navigation_catalog();
        catalog.truncate(1);
        catalog.push(zaparoo_core::media_types::SystemInfo {
            id: "ge5jjso5lvkdfgsywmdqjpa5ye".into(),
            name: "DVD Player".into(),
            category: "Console".into(),
            zap_script: "zaparoo://launch/dvd".into(),
            ..Default::default()
        });
        shared.systems = catalog;
        shared.categories = vec!["Console".into()];
        shared.hub.layout.items.clear();
    }
    crate::systems::enter(&ctx, &app, "Console", EntryMode::Fresh, false);
    // Rows sort by name: the launchable first, then the indexed system.
    crate::systems::context_accept(&ctx, &app, "add_to_hub");
    crate::router::handle_action(&ctx, &app, "right");
    crate::systems::context_accept(&ctx, &app, "add_to_hub");

    let shared = crate::router::lock(&ctx.shared);
    let stored: Vec<(&str, &str)> = shared
        .hub
        .layout
        .items
        .iter()
        .map(|item| (item.id.as_str(), item.name.as_str()))
        .collect();
    assert_eq!(
        stored,
        [
            ("ge5jjso5lvkdfgsywmdqjpa5ye", "DVD Player"),
            ("System00", "")
        ]
    );
}

#[test]
fn fresh_systems_reset_but_back_and_resume_keep_the_parent_position() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    let (_runtime, ctx) = offline_ctx();
    crate::sizing::apply_scene(
        &app,
        crate::sizing::Scene::of(&app, f64::from(W), f64::from(H), false),
    );
    let view = app.global::<SystemsView>();
    for list in [false, true] {
        {
            let mut shared = crate::router::lock(&ctx.shared);
            shared.systems = navigation_catalog();
            shared.categories = vec!["Console".into()];
            shared.persist.active_screen = "systems".into();
            shared.persist.hub.category = "Console".into();
            shared.persist.systems.system_id = "System08".into();
            shared.persist.systems.list_top = Some(6);
            shared.persist.settings.systems_browse_layout =
                if list { "list" } else { "grid" }.into();
        }
        crate::restore_screens(&std::sync::Arc::new(ctx.clone()), &app);
        settle(&window);
        assert_eq!(view.get_current_index(), 8);
        if list {
            assert_eq!(view.get_list_scroll_top(), 6);
        }
        // Open this system, then Back: retain its parent row, not row zero.
        crate::games::enter(&ctx, &app, &navigation_catalog()[8]);
        let ticket = crate::router::lock(&ctx.shared).games.ticket;
        crate::games::apply_fill(&ctx, &app, ticket, navigation_rows(), None, None, true);
        crate::router::handle_action(&ctx, &app, "cancel");
        settle(&window);
        assert_eq!(app.global::<Shell>().get_active_screen(), Screen::Systems);
        assert_eq!(view.get_current_index(), 8);
        if list {
            assert_eq!(view.get_list_scroll_top(), 6);
        }
        crate::systems::enter(&ctx, &app, "Console", EntryMode::Fresh, true);
        assert_eq!(view.get_current_index(), 0);
        assert_eq!(
            crate::router::lock(&ctx.shared)
                .systems_model
                .grid
                .current_page(),
            0
        );
        if list {
            assert_eq!(view.get_list_scroll_top(), 0);
        }
    }
}

#[test]
fn flat_entries_reset_only_when_fresh_and_resume_scoped_favorites() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    let (_runtime, ctx) = offline_ctx();
    crate::sizing::apply_scene(
        &app,
        crate::sizing::Scene::of(&app, f64::from(W), f64::from(H), false),
    );
    for mode in [GamesMode::Favorites, GamesMode::Recents] {
        let enter = |ctx: &crate::router::Ctx, app: &App, entry: EntryMode| {
            if mode == GamesMode::Favorites {
                crate::games::enter_list(ctx, app, Flag::Favorite, entry);
            } else {
                crate::games::enter_recents(ctx, app, entry);
            }
        };
        for entry in [EntryMode::Restore, EntryMode::Fresh] {
            app.global::<Shell>().set_active_screen(Screen::Hub);
            {
                let mut shared = crate::router::lock(&ctx.shared);
                shared.persist.active_screen = mode.screen().token().into();
                shared.persist.settings.games_browse_layout = "list".into();
                shared.persist.settings.favorites_grouping = "system".into();
                shared.persist.favorite_systems.selected_path = "System08".into();
                shared.persist.favorites.selected_path = "/games/8".into();
                shared.persist.favorites.list_top = Some(6);
                shared.persist.recents.selected_path = "/games/8".into();
                shared.persist.recents.list_top = Some(6);
            }
            if entry == EntryMode::Restore {
                crate::restore_screens(&std::sync::Arc::new(ctx.clone()), &app);
            } else {
                enter(&ctx, &app, entry);
            }
            let ticket = crate::router::lock(&ctx.shared).games.ticket;
            crate::games::apply_fill(&ctx, &app, ticket, navigation_rows(), None, None, true);
            settle(&window);
            let view = app.global::<crate::GamesView>();
            let (index, top) = if entry == EntryMode::Restore {
                (8, 6)
            } else {
                (0, 0)
            };
            assert_eq!(view.get_current_index(), index);
            assert_eq!(view.get_list_scroll_top(), top);
            if mode == GamesMode::Favorites {
                assert_eq!(
                    crate::router::lock(&ctx.shared).games.list_system,
                    if entry == EntryMode::Restore {
                        "System08"
                    } else {
                        ""
                    }
                );
            }
            // Reprojection, art publication and an in-place refill keep focus.
            crate::games::reproject(&ctx, &app);
            crate::games::render(&ctx, &app);
            if mode == GamesMode::Favorites {
                crate::games::refresh_list(&ctx, &app);
            } else {
                crate::games::refresh_recents(&ctx, &app);
            }
            let ticket = crate::router::lock(&ctx.shared).games.ticket;
            crate::games::apply_fill(&ctx, &app, ticket, navigation_rows(), None, None, false);
            assert_eq!(view.get_current_index(), index);
            assert_eq!(view.get_list_scroll_top(), top);
        }
    }
    crate::games::enter_list_for_system(&ctx, &app, "System12");
    let ticket = crate::router::lock(&ctx.shared).games.ticket;
    crate::games::apply_fill(&ctx, &app, ticket, navigation_rows(), None, None, true);
    assert_eq!(app.global::<crate::GamesView>().get_current_index(), 0);
    assert_eq!(
        crate::router::lock(&ctx.shared).games.list_system,
        "System12"
    );
}

#[test]
fn each_list_keeps_its_own_place_order_and_grouping() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    let (_runtime, ctx) = offline_ctx();
    crate::sizing::apply_scene(
        &app,
        crate::sizing::Scene::of(&app, f64::from(W), f64::from(H), false),
    );
    {
        let mut shared = crate::router::lock(&ctx.shared);
        shared.persist.active_screen = Screen::Favorites.token().into();
        shared.persist.lists.active = "liked".into();
        shared.persist.lists.liked.selected_path = "/games/8".into();
        shared.persist.favorites.selected_path = "/games/3".into();
    }
    // A cold start comes back to Liked, where it was left.
    crate::restore_screens(&std::sync::Arc::new(ctx.clone()), &app);
    let ticket = crate::router::lock(&ctx.shared).games.ticket;
    crate::games::apply_fill(&ctx, &app, ticket, navigation_rows(), None, None, true);
    settle(&window);
    let shell = app.global::<Shell>();
    assert_eq!(shell.get_active_screen(), Screen::Favorites);
    assert_eq!(shell.get_active_list(), crate::UserFlag::Like);
    assert_eq!(app.global::<crate::GamesView>().get_current_index(), 8);
    // Moving keeps Liked's place, not Favorites'.
    crate::router::handle_action(&ctx, &app, "right");
    crate::games::flush_persist(&ctx);
    {
        let shared = crate::router::lock(&ctx.shared);
        assert_eq!(shared.persist.lists.liked.selected_path, "/games/9");
        assert_eq!(shared.persist.favorites.selected_path, "/games/3");
    }
    // Liked's order is its own, and Play later starts newest first.
    {
        let mut shared = crate::router::lock(&ctx.shared);
        assert!(crate::user_lists::set_sort(
            &mut shared,
            Flag::Like,
            zaparoo_app::user_lists::ListSort::Name
        ));
        assert_eq!(shared.persist.lists.liked.sort, "name");
        assert_eq!(shared.favorites_sort, "");
        assert_eq!(
            crate::user_lists::core_sort(&shared).as_deref(),
            Some("name-asc")
        );
        assert_eq!(
            crate::user_lists::sort(&shared, Flag::PlayLater),
            zaparoo_app::user_lists::ListSort::Added
        );
    }
    // Play later from the Hub: fresh, and named on the screen.
    crate::router::enter_list(&ctx, &app, Flag::PlayLater, EntryMode::Fresh);
    let ticket = crate::router::lock(&ctx.shared).games.ticket;
    crate::games::apply_fill(&ctx, &app, ticket, navigation_rows(), None, None, true);
    settle(&window);
    assert_eq!(shell.get_active_list(), crate::UserFlag::PlayLater);
    assert_eq!(app.global::<crate::GamesView>().get_current_index(), 0);
    assert_eq!(
        crate::router::lock(&ctx.shared).persist.lists.active,
        "playlater"
    );
    // Grouping one list by system leaves the others flat.
    {
        let mut shared = crate::router::lock(&ctx.shared);
        assert!(crate::user_lists::set_grouped(
            &mut shared,
            Flag::PlayLater,
            true
        ));
        assert!(!crate::user_lists::grouped(&shared, Flag::Like));
        assert!(!crate::user_lists::grouped(&shared, Flag::Favorite));
    }
    crate::router::enter_list(&ctx, &app, Flag::PlayLater, EntryMode::Fresh);
    assert_eq!(
        crate::router::lock(&ctx.shared).systems_model.mode,
        SystemsMode::Favorites
    );
    assert_eq!(
        crate::router::lock(&ctx.shared)
            .systems_model
            .favorites_list,
        Flag::PlayLater
    );
}

#[test]
fn favorite_systems_fresh_entry_resets_but_return_preserves_the_viewport() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    let (_runtime, ctx) = offline_ctx();
    crate::sizing::apply_scene(
        &app,
        crate::sizing::Scene::of(&app, f64::from(W), f64::from(H), false),
    );
    let result = zaparoo_core::media_types::SystemsResult {
        systems: navigation_catalog(),
    };
    {
        let mut shared = crate::router::lock(&ctx.shared);
        shared.persist.settings.systems_browse_layout = "list".into();
        shared.persist.favorite_systems.selected_path = "System08".into();
        shared.persist.favorite_systems.list_top = Some(6);
    }
    crate::systems::enter_list(&ctx, &app, Flag::Favorite, EntryMode::Fresh);
    crate::systems::apply_favorites(&ctx, &app, &result, 1, true);
    settle(&window);
    let view = app.global::<SystemsView>();
    assert_eq!(view.get_current_index(), 0);
    assert_eq!(view.get_list_scroll_top(), 0);
    {
        let mut shared = crate::router::lock(&ctx.shared);
        shared.persist.favorite_systems.selected_path = "System08".into();
        shared.persist.favorite_systems.list_top = Some(6);
    }
    crate::systems::return_to_list(&ctx, &app);
    assert_eq!(view.get_current_index(), 8);
    assert_eq!(view.get_list_scroll_top(), 6);
    crate::systems::enter_list(&ctx, &app, Flag::Favorite, EntryMode::Fresh);
    assert_eq!(
        view.get_current_index(),
        0,
        "warm entry follows the same policy"
    );
    assert_eq!(view.get_list_scroll_top(), 0);
}

#[test]
fn cold_games_restore_the_folder_selection_and_back_stack() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    let (_runtime, ctx) = offline_ctx();
    crate::sizing::apply_scene(
        &app,
        crate::sizing::Scene::of(&app, f64::from(W), f64::from(H), false),
    );
    {
        let mut shared = crate::router::lock(&ctx.shared);
        shared.systems = navigation_catalog();
        shared.persist.active_screen = "games".into();
        shared.persist.settings.games_browse_layout = "list".into();
        shared.persist.games.system_id = "System08".into();
        shared.persist.games.path_stack = vec![String::new(), "/rpg".into()];
        shared.persist.games.selected_at_level = vec!["/rpg".into(), "/games/8".into()];
        shared.persist.games.list_top_at_level = vec![10, 6];
        shared.persist.games.entered_from_hub = true;
    }
    crate::restore_screens(&std::sync::Arc::new(ctx.clone()), &app);
    let ticket = {
        let shared = crate::router::lock(&ctx.shared);
        assert_eq!(shared.games.browse_path, "/rpg");
        shared.games.ticket
    };
    crate::games::apply_fill(&ctx, &app, ticket, navigation_rows(), None, None, true);
    settle(&window);
    let view = app.global::<crate::GamesView>();
    assert_eq!(view.get_current_index(), 8);
    assert_eq!(view.get_list_scroll_top(), 6);
    crate::router::open_view_menu(&ctx, &app);
    crate::router::handle_action(&ctx, &app, "cancel");
    assert_eq!(
        view.get_current_index(),
        8,
        "closing a modal is not fresh entry"
    );
    for layout in ["grid", "list"] {
        crate::router::lock(&ctx.shared)
            .persist
            .settings
            .games_browse_layout = layout.into();
        crate::games::reproject(&ctx, &app);
        assert_eq!(view.get_current_index(), 8, "view changes keep selection");
    }
    crate::router::handle_action(&ctx, &app, "cancel");
    let ticket = crate::router::lock(&ctx.shared).games.ticket;
    let mut parent = navigation_rows();
    parent[12].path = "/rpg".into();
    crate::games::apply_fill(&ctx, &app, ticket, parent, None, None, false);
    settle(&window);
    assert_eq!(
        view.get_current_index(),
        12,
        "Back restores the parent selection"
    );
    assert_eq!(view.get_list_scroll_top(), 10);
    crate::router::handle_action(&ctx, &app, "cancel");
    assert_eq!(app.global::<Shell>().get_active_screen(), Screen::Hub);
}

#[test]
fn fresh_shortcuts_keep_their_target_and_cancel_keeps_the_source() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, _window) = boot();
    let (_runtime, ctx) = offline_ctx();
    {
        let mut shared = crate::router::lock(&ctx.shared);
        shared.systems = navigation_catalog();
        shared.persist.active_screen = "hub".into();
        shared.persist.games.system_id = "System08".into();
        shared.persist.games.path_stack = vec![String::new(), "/old-folder".into()];
        shared.persist.games.selected_at_level =
            vec!["/old-folder".into(), "/old-folder/game".into()];
        shared.persist.games.list_top_at_level = vec![3, 7];
    }
    let source = crate::router::lock(&ctx.shared).persist.clone();
    for folder in [false, true] {
        if folder {
            crate::games::enter_folder_from_hub(&ctx, &app, "System08", "/explicit-target");
        } else {
            crate::games::enter_from_hub(&ctx, &app, &navigation_catalog()[8]);
        }
        let ticket = {
            let shared = crate::router::lock(&ctx.shared);
            assert_eq!(
                shared.games.browse_path,
                if folder { "/explicit-target" } else { "" }
            );
            assert!(shared.persist.games.entered_from_hub);
            assert!(shared
                .persist
                .games
                .selected_at_level
                .iter()
                .all(String::is_empty));
            shared.games.ticket
        };
        assert_eq!(crate::navigation::source_persist(), Some(source.clone()));
        assert!(crate::navigation::cancel(&ctx, &app));
        crate::games::apply_fill(&ctx, &app, ticket, navigation_rows(), None, None, true);
        assert_eq!(app.global::<Shell>().get_active_screen(), Screen::Hub);
        assert_eq!(crate::router::lock(&ctx.shared).persist.games, source.games);
    }
}

#[test]
fn about_starts_at_top_only_on_fresh_entry_and_back_keeps_settings_focus() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    let (_runtime, ctx) = offline_ctx();
    let ctx = std::sync::Arc::new(ctx);
    {
        let mut shared = crate::router::lock(&ctx.shared);
        shared.persist.active_screen = "about".into();
        shared.persist.about_scroll_milli = 725;
    }
    crate::restore_core_independent(&ctx, &app);
    settle(&window);
    assert_eq!(app.global::<crate::AboutView>().get_scroll_milli(), 725);
    crate::router::enter_about(&ctx, &app, EntryMode::Fresh);
    assert_eq!(app.global::<crate::AboutView>().get_scroll_milli(), 0);
    assert_eq!(
        crate::router::lock(&ctx.shared).persist.about_scroll_milli,
        0
    );
    let rows = zaparoo_app::settings::page_rows("about", &crate::settings::inputs(&ctx));
    let index = rows
        .iter()
        .position(|row| row.id() == "aboutLicense")
        .unwrap_or(0);
    crate::router::lock(&ctx.shared).settings_focus = Some((SettingsPage::About, index));
    crate::router::handle_action(&ctx, &app, "cancel");
    assert_eq!(
        app.global::<crate::SettingsView>().get_page(),
        SettingsPage::About
    );
    assert_eq!(
        app.global::<crate::SettingsView>().get_index(),
        index as i32
    );
}

#[test]
fn restored_missing_folder_falls_back_to_root_but_errors_do_not_reset() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, _window) = boot();
    let (_runtime, ctx) = offline_ctx();
    {
        let mut shared = crate::router::lock(&ctx.shared);
        shared.persist.games.system_id = "System08".into();
        shared.persist.games.path_stack = vec![String::new(), "/gone".into()];
        shared.persist.games.selected_at_level = vec!["/gone".into(), "/gone/game".into()];
    }
    crate::games::enter_restored(&ctx, &app, &navigation_catalog()[8]);
    let ticket = crate::router::lock(&ctx.shared).games.ticket;
    crate::games::on_browse_ready(
        &ctx,
        &app,
        ticket,
        &zaparoo_core::media_types::MediaBrowseResult::default(),
        false,
        true,
        false,
    );
    let shared = crate::router::lock(&ctx.shared);
    assert_eq!(shared.persist.games.path_stack, vec![String::new()]);
    assert_eq!(shared.games.browse_path, "");
    assert_ne!(shared.games.ticket, ticket);
}

#[test]
fn update_buttons_push_before_dispatch_and_reject_stale_targets() -> Result<(), &'static str> {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, _window) = boot();
    let view = app.global::<crate::UpdateView>();
    app.global::<Shell>().set_active_screen(Screen::Update);
    view.set_page(crate::UpdatePage::Intro);
    view.set_buttons(ModelRc::new(VecModel::from(vec![
        crate::UpdateButton::Back,
        crate::UpdateButton::Start,
    ])));
    view.set_button_focus(0);
    let target = crate::press_feedback::current(&app).ok_or("visible Update button")?;
    assert_eq!(target.owner, PressOwner::Update);
    let committed = Rc::new(Cell::new(false));
    let done = committed.clone();
    let time = CLOCK.with(Cell::get);
    crate::press_feedback::dispatch(&app, &target, move |app| {
        done.set(true);
        app.global::<Shell>().set_active_screen(Screen::Hub);
    });
    assert!(!committed.get());
    assert_eq!(CLOCK.with(Cell::get), time);
    assert_eq!(app.global::<Shell>().get_active_screen(), Screen::Update);
    advance(zaparoo_app::input::PRESS_FEEDBACK_MS - 1);
    assert!(!committed.get());
    advance(1);
    assert!(committed.get());
    assert_eq!(app.global::<Shell>().get_active_screen(), Screen::Hub);
    assert!(!crate::press_feedback::pending(&app));

    app.global::<Shell>().set_active_screen(Screen::Update);
    committed.set(false);
    let done = committed.clone();
    crate::press_feedback::dispatch(&app, &target, move |_| done.set(true));
    view.set_page(crate::UpdatePage::Running);
    advance(zaparoo_app::input::PRESS_FEEDBACK_MS);
    assert!(!committed.get(), "changed page cancels the pending Accept");
    let done = committed.clone();
    crate::press_feedback::dispatch(&app, &target, move |_| done.set(true));
    advance(zaparoo_app::input::PRESS_FEEDBACK_MS);
    assert!(!committed.get(), "already stale target is rejected too");
    assert!(!crate::press_feedback::pending(&app));
    assert!(crate::press_feedback::current(&app).is_none());

    view.set_page(crate::UpdatePage::Intro);
    app.global::<crate::Motion>().set_enabled(false);
    let done = committed.clone();
    crate::press_feedback::dispatch(&app, &target, move |_| done.set(true));
    assert!(committed.get());
    assert!(!crate::press_feedback::pending(&app));
    Ok(())
}

#[test]
fn leaving_update_persists_hub_before_returning() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, _window) = boot();
    let (_runtime, ctx) = offline_ctx();
    app.global::<Shell>().set_active_screen(Screen::Update);
    "settings".clone_into(&mut crate::router::lock(&ctx.shared).persist.active_screen);
    let ctx = std::sync::Arc::new(ctx);

    crate::update::run_effect(&ctx, &app, zaparoo_update_api::Effect::LeaveToHub);

    assert_eq!(app.global::<Shell>().get_active_screen(), Screen::Hub);
    assert_eq!(
        crate::router::lock(&ctx.shared).persist.active_screen,
        "hub"
    );
    assert_eq!(zaparoo_core::persist::load().active_screen, "hub");
}

#[test]
fn first_time_setup_closes_before_rpc_and_returns_input_to_screen() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    // This runtime never runs: dismissal cannot depend on any RPC reply.
    let (_runtime, ctx) = offline_ctx();
    app.global::<crate::Motion>().set_enabled(false);
    app.global::<Shell>().set_active_screen(Screen::Settings);
    crate::settings::open_page(&ctx, &app, SettingsPage::Display);
    let overlays = app.global::<crate::Overlays>();
    overlays.set_dialog_kind(DialogKind::FirstRun);
    overlays.set_dialog_buttons(ModelRc::new(VecModel::from(vec![
        DialogButton::StartMediaUpdate,
    ])));
    overlays.set_dialog_open(true);
    crate::router::lock(&ctx.shared).first_run_shown = true;
    crate::router::handle_action(&ctx, &app, "accept");
    distinct_frames(&window, 4);
    assert!(
        !overlays.get_dialog_open(),
        "setup ends without waiting for Core"
    );
    assert_eq!(overlays.get_dialog_kind(), DialogKind::None);
    let before = app.global::<crate::SettingsView>().get_index();
    crate::router::handle_action(&ctx, &app, "down");
    assert_ne!(
        app.global::<crate::SettingsView>().get_index(),
        before,
        "screen input remains usable"
    );
    assert_eq!(app.global::<Shell>().get_active_screen(), Screen::Settings);
}

#[test]
fn catalog_refresh_adds_systems_during_indexing_without_resetting_focus_or_menu_target() {
    use std::sync::Arc;
    use zaparoo_app::status_line::{Link, TaskInput};
    use zaparoo_core::media_types::SystemInfo;
    use zaparoo_core::remote_resource::ResourceStatus;
    use zaparoo_core::systems_catalog::CatalogData;

    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, _window) = boot();
    let (_runtime, ctx) = offline_ctx();
    let ctx = Arc::new(ctx);
    app.global::<Shell>().set_active_screen(Screen::Systems);
    crate::router::lock(&ctx.shared).systems_model.category = "Console".into();
    let system = |id: &str, name: &str| SystemInfo {
        id: id.into(),
        name: name.into(),
        category: "Console".into(),
        media_count: Some(1),
        ..SystemInfo::default()
    };
    let snes = system("SNES", "Super Nintendo");
    let nes = system("NES", "Nintendo");
    crate::apply_catalog(
        &ctx,
        &app,
        &ResourceStatus::Ready(CatalogData {
            systems: vec![snes.clone()],
            categories: vec!["Console".into()],
        }),
    );
    crate::status::set_link(&ctx.status, &app, &ctx.handle, Link::Connected, None);
    crate::status::enable_media_activity(&ctx.status, &app, &ctx.handle);
    crate::status::set_task(
        &ctx.status,
        &app,
        &ctx.handle,
        TaskInput {
            indexing: true,
            current_step: 1,
            total_steps: 10,
            ..Default::default()
        },
    );
    assert!(
        app.global::<HubView>().get_indexing(),
        "empty-library copy follows the job immediately"
    );
    crate::router::lock(&ctx.shared)
        .persist
        .settings
        .screensaver_timeout = "1".into();
    crate::router::reset_idle(&ctx, &app);
    advance(999);
    assert!(!app.global::<Shell>().get_saver_armed());
    let overlays = app.global::<crate::Overlays>();
    overlays.set_context_open(true);
    {
        let mut shared = crate::router::lock(&ctx.shared);
        shared.context_owner = crate::router::ContextOwner::Systems;
        shared.context_target = 0;
    }
    let update = ResourceStatus::Ready(CatalogData {
        systems: vec![nes.clone(), snes],
        categories: vec!["Console".into()],
    });
    crate::apply_catalog(&ctx, &app, &update);
    {
        let shared = crate::router::lock(&ctx.shared);
        assert_eq!(shared.systems_model.rows.len(), 2);
        assert_eq!(
            shared.systems_model.current().map(|row| row.id.as_str()),
            Some("SNES")
        );
        assert_eq!(shared.systems_model.rows[shared.context_target].id, "SNES");
    }
    assert!(overlays.get_context_open());
    assert!(
        app.global::<crate::Status>().get_show_track(),
        "indexing has not finished"
    );
    assert_eq!(app.global::<SystemsView>().get_cells().row_count(), 2);
    assert_eq!(app.global::<Shell>().get_active_screen(), Screen::Systems);
    crate::apply_catalog(&ctx, &app, &update);
    advance(1);
    assert!(
        app.global::<Shell>().get_saver_armed(),
        "catalog polling must not postpone the idle deadline"
    );
    // A disappearing target must not silently redirect its menu to another system.
    crate::apply_catalog(
        &ctx,
        &app,
        &ResourceStatus::Ready(CatalogData {
            systems: vec![nes],
            categories: vec!["Console".into()],
        }),
    );
    assert!(!overlays.get_context_open());
    crate::status::set_task(&ctx.status, &app, &ctx.handle, TaskInput::default());
    assert!(!app.global::<HubView>().get_indexing());
}

#[test]
fn catalog_refresh_keeps_hub_focus_when_first_category_replaces_bootstrap_tiles() {
    use zaparoo_core::{
        media_types::SystemInfo, remote_resource::ResourceStatus, systems_catalog::CatalogData,
    };
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, _window) = boot();
    let (_runtime, ctx) = offline_ctx();
    let ctx = std::sync::Arc::new(ctx);
    crate::apply_catalog(
        &ctx,
        &app,
        &ResourceStatus::Ready(CatalogData {
            systems: vec![],
            categories: vec![],
        }),
    );
    let before = {
        let shared = crate::router::lock(&ctx.shared);
        shared.hub.entries[shared.hub.grid.current_index()]
            .id
            .clone()
    };
    crate::apply_catalog(
        &ctx,
        &app,
        &ResourceStatus::Ready(CatalogData {
            systems: vec![SystemInfo {
                id: "NES".into(),
                name: "Nintendo".into(),
                category: "Console".into(),
                media_count: Some(1),
                ..Default::default()
            }],
            categories: vec!["Console".into()],
        }),
    );
    let shared = crate::router::lock(&ctx.shared);
    assert_eq!(
        shared.hub.entries[shared.hub.grid.current_index()].id,
        before
    );
}

#[allow(
    clippy::expect_used,
    reason = "a fixed search answer always deserialises"
)]
fn search_result(names: &[&str], more: bool) -> zaparoo_core::media_types::MediaSearchResult {
    let results: Vec<_> = names
        .iter()
        .map(|name| {
            serde_json::json!({
                "name": name,
                "path": format!("/games/{name}"),
                "system": { "id": "System08", "name": "System 08" },
            })
        })
        .collect();
    serde_json::from_value(serde_json::json!({
        "results": results,
        "total": names.len(),
        "pagination": { "hasNextPage": more, "pageSize": 100 },
    }))
    .expect("search result")
}

#[test]
fn the_search_pane_lists_and_counts_each_title_core_grouped() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, _window) = boot();
    let (_runtime, ctx) = offline_ctx();
    crate::search::enter(&ctx, &app, EntryMode::Fresh);
    let view = app.global::<crate::SearchView>();
    crate::search::type_char(&ctx, &app, 'g');
    let seq = crate::search::preview_seq(&crate::router::lock(&ctx.shared));

    // Core sends one row per title, the first standing for three versions.
    let titles: Vec<String> = (0..20).map(|n| format!("Game {n:02}")).collect();
    let names: Vec<&str> = titles.iter().map(String::as_str).collect();
    let mut result = search_result(&names, false);
    result.results[0].version_count = Some(3);
    crate::search::preview_landed(&ctx, &app, seq, Ok(&result));
    assert_eq!(view.get_pane_rows().row_count(), 20);
    assert_eq!(view.get_count(), 20, "the heading counts the games listed");

    // The row for the title opens the results on the version Core picked.
    crate::router::handle_action(&ctx, &app, "right");
    crate::router::handle_action(&ctx, &app, "accept");
    assert_eq!(
        crate::router::lock(&ctx.shared)
            .persist
            .search
            .selected_path,
        "/games/Game 00"
    );
}

#[test]
fn search_types_submits_and_returns_with_the_query_intact() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    let (_runtime, ctx) = offline_ctx();
    crate::sizing::apply_scene(
        &app,
        crate::sizing::Scene::of(&app, f64::from(W), f64::from(H), false),
    );
    let view = app.global::<crate::SearchView>();
    crate::router::lock(&ctx.shared).persist.search.query = "stale".into();
    crate::search::enter(&ctx, &app, EntryMode::Fresh);
    settle(&window);
    let shell = app.global::<Shell>();
    assert_eq!(shell.get_active_screen(), Screen::Search);
    // A fresh visit starts empty, with nothing to search on yet.
    assert_eq!(view.get_before(), "");
    assert!(!view.get_can_search());

    // The focused key types, North spaces, a real key types and moves
    // focus to the Search key, L steps the caret back, West deletes there.
    crate::router::handle_action(&ctx, &app, "accept");
    crate::router::handle_action(&ctx, &app, "context_menu");
    crate::search::type_char(&ctx, &app, 'b');
    assert_eq!(view.get_before(), "a b");
    assert_eq!(view.get_key_kind(), crate::KeyKind::Submit);
    crate::router::handle_action(&ctx, &app, "page_prev");
    assert_eq!(
        (view.get_before(), view.get_at(), view.get_after()),
        ("a\u{a0}".into(), "b".into(), "".into())
    );
    crate::router::handle_action(&ctx, &app, zaparoo_app::input::TEXT_DELETE);
    assert_eq!((view.get_before(), view.get_at()), ("a".into(), "b".into()));
    assert!(view.get_can_search());
    // A held Accept arrives as its own repeating action: it types on a
    // letter, deletes on Backspace, and is never offered on Search.
    assert!(!crate::search::key_repeats(&ctx));
    crate::search::focus_key_for_test(&ctx, zaparoo_app::keyboard::Key::Char('z'));
    assert!(crate::search::key_repeats(&ctx));
    crate::router::handle_action(&ctx, &app, zaparoo_app::input::TEXT_KEY);
    assert_eq!(
        (view.get_before(), view.get_at()),
        ("az".into(), "b".into())
    );
    crate::search::focus_key_for_test(&ctx, zaparoo_app::keyboard::Key::Backspace);
    assert!(crate::search::key_repeats(&ctx));
    crate::router::handle_action(&ctx, &app, zaparoo_app::input::TEXT_KEY);
    assert_eq!((view.get_before(), view.get_at()), ("a".into(), "b".into()));
    crate::search::focus_key_for_test(&ctx, zaparoo_app::keyboard::Key::Submit);

    crate::router::handle_action(&ctx, &app, "accept");
    assert!(shell.get_transitioning(), "results are a deferred route");
    assert_eq!(shell.get_active_screen(), Screen::Search);
    let ticket = crate::router::lock(&ctx.shared).games.ticket;
    crate::games::apply_fill(&ctx, &app, ticket, navigation_rows(), None, None, true);
    settle(&window);
    assert_eq!(shell.get_active_screen(), Screen::SearchResults);
    let games = app.global::<crate::GamesView>();
    assert_eq!(games.get_mode(), GamesMode::Search);
    assert_eq!(games.get_title(), "\u{201c}ab\u{201d}");
    {
        let shared = crate::router::lock(&ctx.shared);
        assert_eq!(shared.persist.active_screen, "search-results");
        assert_eq!(shared.persist.search.recent.len(), 1);
        assert_eq!(shared.persist.search.recent[0].query, "ab");
    }

    // Back restores the search as it was, on the Search key.
    crate::router::handle_action(&ctx, &app, "cancel");
    assert_eq!(shell.get_active_screen(), Screen::Search);
    assert_eq!(view.get_before(), "ab");
    assert_eq!(view.get_zone(), crate::SearchZone::Keys);
    assert_eq!(view.get_key_kind(), crate::KeyKind::Submit);
    assert_eq!(
        crate::router::lock(&ctx.shared).persist.active_screen,
        "search"
    );
    crate::router::handle_action(&ctx, &app, "cancel");
    assert_eq!(shell.get_active_screen(), Screen::Hub);
}

/// A Hub holding one saved search, focused and on screen.
fn hub_with_saved_search(ctx: &crate::router::Ctx, app: &App) {
    {
        let mut shared = crate::router::lock(&ctx.shared);
        shared.hub.layout.items = vec![zaparoo_core::hub_layout::HubItem {
            kind_raw: "search".into(),
            name: "RPGs".into(),
            query: "final".into(),
            systems: vec!["SNES".into(), "Genesis".into()],
            tags: vec!["genre:rpg".into(), "genre:action".into()],
            ..Default::default()
        }];
        shared.hub.categories_loaded = true;
        shared.hub.restore_done = true;
    }
    crate::hub::rebuild(ctx, app);
}

#[test]
#[allow(clippy::expect_used, reason = "the View menu has its one row")]
fn a_saved_search_tile_opens_its_results_and_back_returns_to_the_hub() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    let (_runtime, ctx) = offline_ctx();
    crate::sizing::apply_scene(
        &app,
        crate::sizing::Scene::of(&app, f64::from(W), f64::from(H), false),
    );
    hub_with_saved_search(&ctx, &app);
    let shell = app.global::<Shell>();
    assert_eq!(shell.get_active_screen(), Screen::Hub);

    settle(&window);
    crate::router::handle_action(&ctx, &app, "accept");
    advance(zaparoo_app::input::PRESS_FEEDBACK_MS);
    assert!(shell.get_transitioning(), "results are a deferred route");
    assert_eq!(shell.get_active_screen(), Screen::Hub);
    let ticket = crate::router::lock(&ctx.shared).games.ticket;
    crate::games::apply_fill(&ctx, &app, ticket, navigation_rows(), None, None, true);
    settle(&window);
    assert_eq!(shell.get_active_screen(), Screen::SearchResults);
    assert_eq!(
        app.global::<crate::GamesView>().get_title(),
        "\u{201c}final\u{201d} \u{b7} SNES \u{b7} Mega Drive \u{b7} rpg \u{b7} action"
    );
    {
        let shared = crate::router::lock(&ctx.shared);
        let search = &shared.persist.search;
        assert!(search.from_hub);
        assert_eq!(search.systems, ["SNES", "Genesis"]);
        // Two values of one type: more than the picker could choose.
        assert_eq!(
            crate::search::args(&shared).tags,
            ["genre:rpg", "genre:action"]
        );
        assert!(search.recent.is_empty(), "a tile's search is not a recent");
    }

    // The View menu knows the search is on the Hub, and takes it off and
    // puts it back.
    let overlays = app.global::<crate::Overlays>();
    let saved = || {
        crate::router::lock(&ctx.shared)
            .hub
            .layout
            .items
            .iter()
            .filter(|item| item.kind_raw == "search")
            .count()
    };
    for (key, cue, left) in [
        ("hub:remove", crate::AppCue::RemovedFromHub, 0),
        ("save_to_hub", crate::AppCue::SavedToHub, 1),
    ] {
        crate::router::handle_action(&ctx, &app, "page_menu");
        assert!(overlays.get_list_open());
        let entries = overlays.get_list_entries();
        assert_eq!(
            entries.row_data(0).map(|row| row.id.to_string()).as_deref(),
            Some("edit_search")
        );
        let row = entries.row_data(1).expect("the Hub row");
        assert_eq!(
            (row.id.as_str(), row.label_key.as_str()),
            ("add_to_hub", key)
        );
        crate::router::handle_action(&ctx, &app, "down");
        crate::router::handle_action(&ctx, &app, "accept");
        advance(zaparoo_app::input::PRESS_FEEDBACK_MS + 10);
        assert!(!overlays.get_list_open());
        assert_eq!(saved(), left);
        assert_eq!(header_cue(&app), cue);
        advance(zaparoo_app::wait_cue::CONFIRM_MS);
    }
    {
        // The tile it put back runs the same search under the results' name.
        let shared = crate::router::lock(&ctx.shared);
        let item = &shared.hub.layout.items[0];
        assert_eq!(item.query, "final");
        assert_eq!(item.systems, ["SNES", "Genesis"]);
        assert_eq!(item.tags, ["genre:rpg", "genre:action"]);
        assert!(item.name.starts_with('\u{201c}'));
    }

    crate::router::handle_action(&ctx, &app, "cancel");
    assert_eq!(shell.get_active_screen(), Screen::Hub, "no Search screen");
    let shared = crate::router::lock(&ctx.shared);
    assert_eq!(shared.persist.active_screen, "hub");
    assert!(!shared.persist.search.from_hub);
}

#[test]
fn a_cold_start_on_a_saved_searchs_results_still_returns_to_the_hub() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window, _runtime, ctx) = cold_start("search-results");
    {
        let mut shared = crate::router::lock(&ctx.shared);
        shared.persist.search.query = "final".into();
        shared.persist.search.from_hub = true;
    }
    let curtain = pixels(&window);
    crate::restore_screens(&ctx, &app);
    assert_still_curtained(&app, &window, &ctx, &curtain, "search-results");
    let ticket = crate::router::lock(&ctx.shared).games.ticket;
    crate::games::apply_fill(&ctx, &app, ticket, navigation_rows(), None, None, true);
    let shell = app.global::<Shell>();
    assert_eq!(shell.get_active_screen(), Screen::SearchResults);
    assert!(!shell.get_boot_curtain());

    crate::router::handle_action(&ctx, &app, "cancel");
    assert_eq!(shell.get_active_screen(), Screen::Hub);
}

#[test]
fn a_late_preview_cannot_fill_a_newer_search() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, _window) = boot();
    let (_runtime, ctx) = offline_ctx();
    crate::search::enter(&ctx, &app, EntryMode::Fresh);
    let view = app.global::<crate::SearchView>();
    crate::search::type_char(&ctx, &app, 'm');
    let first = crate::search::preview_seq(&crate::router::lock(&ctx.shared));
    crate::search::type_char(&ctx, &app, 'a');
    let second = crate::search::preview_seq(&crate::router::lock(&ctx.shared));
    assert_ne!(first, second);
    assert!(view.get_searching());

    crate::search::preview_landed(&ctx, &app, first, Ok(&search_result(&["Metroid"], false)));
    assert!(!view.get_count_known(), "the answer to \"m\" is stale");
    assert_eq!(view.get_pane_rows().row_count(), 0);

    crate::search::preview_landed(
        &ctx,
        &app,
        second,
        Ok(&search_result(
            &["Mario Kart 64", "Super Mario World"],
            true,
        )),
    );
    assert!(view.get_count_known() && view.get_count_more());
    assert_eq!(view.get_count(), 2);
    assert_eq!(view.get_pane(), crate::SearchPane::Preview);
    assert_eq!(view.get_pane_rows().row_count(), 2);
    assert!(!view.get_searching());

    // The pane is reachable from the keyboard's right edge, and a match
    // opens the results focused on it.
    crate::router::handle_action(&ctx, &app, "right");
    assert_eq!(view.get_zone(), crate::SearchZone::Pane);
    crate::router::handle_action(&ctx, &app, "down");
    assert_eq!(view.get_pane_index(), 1);
    crate::router::handle_action(&ctx, &app, "accept");
    assert_eq!(
        crate::router::lock(&ctx.shared)
            .persist
            .search
            .selected_path,
        "/games/Super Mario World"
    );
    assert!(app.global::<Shell>().get_transitioning());

    // A failed search says so and lists nothing.
    crate::router::handle_action(&ctx, &app, "cancel");
    crate::search::type_char(&ctx, &app, 'r');
    let third = crate::search::preview_seq(&crate::router::lock(&ctx.shared));
    crate::search::preview_landed(&ctx, &app, third, Err("offline"));
    assert!(view.get_failed() && !view.get_count_known());
    assert_eq!(view.get_pane_rows().row_count(), 0);
}

#[test]
fn search_here_scopes_to_the_folder_and_back_returns_to_it() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, _window) = boot();
    let (_runtime, ctx) = offline_ctx();
    let shell = app.global::<Shell>();
    {
        let mut shared = crate::router::lock(&ctx.shared);
        shared.systems = navigation_catalog();
        shared.games.mode = GamesMode::Browse;
        shared.games.system_id = "System08".into();
        shared.games.system_name = "System 08".into();
        shared.games.browse_path = "/roms/System08/RPG".into();
        shared.persist.games.system_id = "System08".into();
        shared.persist.games.path_stack = vec![String::new(), "/roms/System08/RPG".into()];
        shared.persist.games.selected_at_level = vec![String::new(), String::new()];
        shared.persist.search.query = "left over".into();
        shared.persist.active_screen = "games".into();
        shared
            .persist
            .games
            .set_filter("System08", vec!["genre:rpg".into()]);
    }
    shell.set_active_screen(Screen::Games);

    crate::search::enter_scoped(&ctx, &app);
    assert_eq!(shell.get_active_screen(), Screen::Search);
    let view = app.global::<crate::SearchView>();
    assert_eq!(view.get_scope(), crate::SearchScope::Folder);
    assert_eq!(view.invoke_title(), "Search in RPG");
    assert_eq!(view.invoke_system_text(), "System 08 / RPG");
    assert_eq!(view.get_system_name(), "System 08");
    assert_eq!(view.get_scope_name(), "RPG");
    assert_eq!(view.get_before(), "");
    // The folder alone is enough to search on.
    assert!(view.get_can_search());
    {
        let shared = crate::router::lock(&ctx.shared);
        let args = crate::search::args(&shared);
        assert_eq!(args.systems, ["System08"]);
        assert_eq!(args.path_prefix, "/roms/System08/RPG");
        // The folder's filter came along.
        assert_eq!(args.tags, ["genre:rpg"]);
        assert_eq!(shared.persist.search.tags, ["genre:rpg"]);
    }
    // Clearing it here leaves the folder's own filter as it was.
    crate::router::lock(&ctx.shared).persist.search.tags.clear();
    // The scope field takes no focus: Up from the top row lands on the
    // tags, and Up again loops round to the bottom key row.
    crate::router::handle_action(&ctx, &app, "up");
    crate::router::handle_action(&ctx, &app, "up");
    crate::router::handle_action(&ctx, &app, "up");
    assert_eq!(view.get_zone(), crate::SearchZone::Filter);
    crate::router::handle_action(&ctx, &app, "up");
    assert_eq!(view.get_zone(), crate::SearchZone::Keys);
    assert_eq!(
        view.get_key_index(),
        30,
        "the bottom row, under the home row's first key"
    );
    crate::router::handle_action(&ctx, &app, "down");
    assert_eq!(view.get_zone(), crate::SearchZone::Filter);

    crate::router::handle_action(&ctx, &app, "cancel");
    assert_eq!(shell.get_active_screen(), Screen::Games);
    let shared = crate::router::lock(&ctx.shared);
    assert_eq!(shared.persist.active_screen, "games");
    assert_eq!(shared.games.browse_path, "/roms/System08/RPG");
    assert_eq!(
        shared.persist.games.filter_for("System08"),
        ["genre:rpg".to_string()]
    );
    drop(shared);

    // At a system's top level only the system narrows the search.
    {
        let mut shared = crate::router::lock(&ctx.shared);
        shared.games.browse_path = String::new();
        shared.persist.games.path_stack = vec![String::new()];
        shared.persist.games.selected_at_level = vec![String::new()];
    }
    crate::search::enter_scoped(&ctx, &app);
    let shared = crate::router::lock(&ctx.shared);
    assert!(shared.persist.search.scoped);
    assert_eq!(shared.persist.search.scope_path, "");
    assert_eq!(crate::search::args(&shared).systems, ["System08"]);
    drop(shared);
    assert_eq!(view.get_scope(), crate::SearchScope::System);
    assert_eq!(view.invoke_title(), "Search in System 08");
}

#[test]
fn search_here_on_a_category_searches_its_systems_and_back_returns() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, _window) = boot();
    let (_runtime, ctx) = offline_ctx();
    let shell = app.global::<Shell>();
    crate::router::lock(&ctx.shared).systems = navigation_catalog();
    crate::systems::prepare_parent(&ctx, &app, "Console");
    crate::router::lock(&ctx.shared).persist.active_screen = "systems".into();
    shell.set_active_screen(Screen::Systems);

    crate::search::enter_scoped(&ctx, &app);
    assert_eq!(shell.get_active_screen(), Screen::Search);
    let view = app.global::<crate::SearchView>();
    assert_eq!(view.get_scope(), crate::SearchScope::Category);
    assert_eq!(view.invoke_title(), "Search in Consoles");
    // The category names the systems; the field never lists all twenty.
    assert_eq!(view.invoke_system_text(), "Consoles");
    assert!(view.get_can_search());
    {
        let shared = crate::router::lock(&ctx.shared);
        let args = crate::search::args(&shared);
        assert_eq!(args.systems.len(), 20);
        assert!(args.tags.is_empty());
    }
    // With no query, the results are titled by the category alone.
    let games = app.global::<crate::GamesView>();
    games.set_mode(GamesMode::Search);
    crate::search::publish_results_title(&games, &crate::router::lock(&ctx.shared));
    assert_eq!(games.get_title(), "");
    assert_eq!(games.invoke_display_title(), "Consoles");
    games.set_mode(GamesMode::Browse);

    crate::router::handle_action(&ctx, &app, "cancel");
    assert_eq!(shell.get_active_screen(), Screen::Systems);
    assert_eq!(
        crate::router::lock(&ctx.shared).persist.active_screen,
        "systems"
    );
}

#[test]
fn search_here_on_favorites_keeps_the_favorite_tag_and_back_returns() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, _window) = boot();
    let (_runtime, ctx) = offline_ctx();
    let shell = app.global::<Shell>();
    {
        let mut shared = crate::router::lock(&ctx.shared);
        shared.systems = navigation_catalog();
        shared.games.mode = GamesMode::Favorites;
        shared.games.list_system = "System03".into();
        shared.persist.active_screen = "favorites".into();
    }
    shell.set_active_screen(Screen::Favorites);

    crate::search::enter_scoped(&ctx, &app);
    let view = app.global::<crate::SearchView>();
    assert_eq!(view.get_scope(), crate::SearchScope::List);
    assert_eq!(view.invoke_title(), "Search in Favorites");
    assert_eq!(view.invoke_system_text(), "Favorites / System 03");
    {
        let shared = crate::router::lock(&ctx.shared);
        let args = crate::search::args(&shared);
        assert_eq!(args.systems, ["System03"]);
        assert_eq!(args.tags, [zaparoo_app::search::FAVORITE_TAG]);
        // The scope's tag is not one the Tags field can clear.
        assert!(shared.persist.search.tags.is_empty());
        // Nor a chosen user tag, so hidden games stay out as on the list.
        assert!(!args.include_hidden);
    }
    // The results name the scope after the systems.
    crate::router::lock(&ctx.shared).persist.search.query = "mario".into();
    let games = app.global::<crate::GamesView>();
    games.set_mode(GamesMode::Search);
    crate::search::publish_results_title(&games, &crate::router::lock(&ctx.shared));
    assert_eq!(
        games.invoke_display_title(),
        "\u{201c}mario\u{201d} \u{b7} System 03 \u{b7} Favorites"
    );
    games.set_mode(GamesMode::Favorites);
    crate::router::lock(&ctx.shared)
        .persist
        .search
        .query
        .clear();

    crate::router::handle_action(&ctx, &app, "cancel");
    assert_eq!(shell.get_active_screen(), Screen::Favorites);
    assert_eq!(
        crate::router::lock(&ctx.shared).persist.active_screen,
        "favorites"
    );

    // From the systems that hold favorites: every favorite, and Back goes
    // to that screen.
    crate::router::lock(&ctx.shared).persist.active_screen = "favorite-systems".into();
    shell.set_active_screen(Screen::FavoriteSystems);
    crate::search::enter_scoped(&ctx, &app);
    assert_eq!(view.invoke_system_text(), "Favorites");
    {
        let shared = crate::router::lock(&ctx.shared);
        let args = crate::search::args(&shared);
        assert!(args.systems.is_empty());
        assert_eq!(args.tags, [zaparoo_app::search::FAVORITE_TAG]);
    }
    crate::router::handle_action(&ctx, &app, "cancel");
    assert_eq!(
        crate::router::lock(&ctx.shared).persist.active_screen,
        "favorite-systems"
    );
}

#[test]
fn the_tags_picker_keeps_each_pick_and_back_leaves_with_them() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, _window) = boot();
    let (_runtime, ctx) = offline_ctx();
    crate::search::enter(&ctx, &app, EntryMode::Fresh);
    let overlays = app.global::<crate::Overlays>();
    let view = app.global::<crate::SearchView>();

    // Up from the home row's first key twice reaches the top row, then
    // System; Right is Tags.
    for action in ["up", "up", "up", "right"] {
        crate::router::handle_action(&ctx, &app, action);
    }
    assert_eq!(view.get_zone(), crate::SearchZone::Filter);
    crate::router::handle_action(&ctx, &app, "accept");
    assert!(overlays.get_list_open() && overlays.get_list_form());
    assert_eq!(overlays.get_list_title(), "title:filter");
    crate::browse_filter::landed_for_test(
        &ctx,
        &app,
        &[
            ("genre", "rpg", "RPG", 12),
            ("genre", "racing", "Racing", 4),
        ],
    );
    let ids = |app: &App| -> Vec<String> {
        app.global::<crate::Overlays>()
            .get_list_entries()
            .iter()
            .map(|entry| entry.id.to_string())
            .collect()
    };
    // No Apply, and nothing to clear yet.
    assert_eq!(ids(&app), ["cat:genre"]);
    assert_eq!(
        overlays
            .get_list_entries()
            .row_data(0)
            .map(|e| e.detail_key),
        Some("filter:any".into())
    );

    // A list row holds its press for a moment before it dispatches.
    let press = zaparoo_app::input::PRESS_FEEDBACK_MS + 10;
    crate::router::handle_action(&ctx, &app, "accept");
    advance(press);
    assert_eq!(ids(&app), ["any", "v:racing", "v:rpg"]);
    crate::router::handle_action(&ctx, &app, "down");
    crate::router::handle_action(&ctx, &app, "accept");
    advance(press);
    // A tick is kept at once and the values page stays open, focus in place.
    assert_eq!(ids(&app), ["any", "v:racing", "v:rpg"]);
    let rows = overlays.get_list_entries();
    assert_eq!(
        rows.row_data(1).map(|e| e.role),
        Some(crate::MenuRole::CheckOn)
    );
    assert_eq!(
        rows.row_data(2).map(|e| e.role),
        Some(crate::MenuRole::CheckOff)
    );
    assert_eq!(
        crate::router::lock(&ctx.shared).persist.search.tags,
        ["genre:racing"]
    );

    // A second tick in the category makes the pair any-of.
    crate::router::handle_action(&ctx, &app, "down");
    crate::router::handle_action(&ctx, &app, "accept");
    advance(press);
    assert_eq!(
        crate::router::lock(&ctx.shared).persist.search.tags,
        ["~genre:racing", "~genre:rpg"]
    );

    // Back returns to the categories, and Clear appears as the foot's action.
    crate::router::handle_action(&ctx, &app, "cancel");
    advance(press);
    assert_eq!(ids(&app), ["cat:genre", "filter_clear"]);
    let rows = overlays.get_list_entries();
    assert_eq!(rows.row_data(0).map(|e| e.detail), Some("Racing +1".into()));
    assert_eq!(
        rows.row_data(1).map(|e| e.role),
        Some(crate::MenuRole::Action)
    );

    // Back leaves with the picks, rather than discarding them.
    crate::router::handle_action(&ctx, &app, "cancel");
    assert!(!overlays.get_list_open());
    assert_eq!(view.get_filter_text(), "Racing +1");
    assert_eq!(
        crate::router::lock(&ctx.shared).persist.search.tags,
        ["~genre:racing", "~genre:rpg"]
    );
}

#[test]
fn a_settings_picker_after_a_form_list_is_a_plain_list_again() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, _window) = boot();
    let (_runtime, ctx) = offline_ctx();
    crate::search::enter(&ctx, &app, EntryMode::Fresh);
    let overlays = app.global::<crate::Overlays>();
    for action in ["up", "up", "up", "right", "accept"] {
        crate::router::handle_action(&ctx, &app, action);
    }
    assert!(overlays.get_list_open() && overlays.get_list_form());
    crate::router::handle_action(&ctx, &app, "cancel");
    assert!(!overlays.get_list_open());

    app.global::<Shell>().set_active_screen(Screen::Settings);
    crate::settings::open_page(&ctx, &app, SettingsPage::Appearance);
    let settings = app.global::<crate::SettingsView>();
    let rows = settings.get_rows();
    let picker = (0..rows.row_count())
        .find(|&i| rows.row_data(i).is_some_and(|row| row.id == "colorScheme"));
    assert!(picker.is_some(), "Appearance lists the color scheme");
    settings.set_index(picker.unwrap_or(0) as i32);
    crate::settings::handle_action(&ctx, &app, "accept");
    assert!(overlays.get_list_open());
    assert_eq!(overlays.get_list_setting_id(), "colorScheme");
    assert!(
        !overlays.get_list_form(),
        "a settings picker draws its translated values, not form rows"
    );
}

#[test]
fn the_system_picker_skips_headers_and_jumps_by_manufacturer() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, _window) = boot();
    let (_runtime, ctx) = offline_ctx();
    {
        let mut shared = crate::router::lock(&ctx.shared);
        shared.systems = (0..10)
            .map(|index| zaparoo_core::media_types::SystemInfo {
                id: format!("S{index}"),
                name: format!("System {index}"),
                manufacturer: Some(if index < 6 { "Alpha" } else { "Beta" }.into()),
                media_count: Some(5),
                ..Default::default()
            })
            .collect();
    }
    crate::search::enter(&ctx, &app, EntryMode::Fresh);
    for action in ["up", "up", "up", "accept"] {
        crate::router::handle_action(&ctx, &app, action);
    }
    let overlays = app.global::<crate::Overlays>();
    assert!(overlays.get_list_open() && overlays.get_list_form());
    // All systems, "Alpha" and its six, "Beta" and its four.
    assert_eq!(overlays.get_list_entries().row_count(), 13);
    assert_eq!(overlays.get_list_index(), 0);
    crate::router::handle_action(&ctx, &app, "down");
    assert_eq!(overlays.get_list_index(), 2, "the header takes no focus");
    crate::router::handle_action(&ctx, &app, "right");
    assert_eq!(
        overlays.get_list_index(),
        9,
        "the next manufacturer's first system"
    );
    crate::router::handle_action(&ctx, &app, "page_prev");
    assert_eq!(overlays.get_list_index(), 2);
    crate::router::handle_action(&ctx, &app, "up");
    assert_eq!(overlays.get_list_index(), 0);
    crate::router::handle_action(&ctx, &app, "up");
    assert_eq!(
        overlays.get_list_index(),
        12,
        "Up from the top wraps to the last row"
    );

    crate::router::handle_action(&ctx, &app, "accept");
    advance(zaparoo_app::input::PRESS_FEEDBACK_MS + 10);
    assert!(!overlays.get_list_open());
    assert_eq!(
        crate::router::lock(&ctx.shared).persist.search.systems,
        ["S9"]
    );
    assert_eq!(
        app.global::<crate::SearchView>().get_system_name(),
        "System 9"
    );
}

/// A relaunch with saved state: the catalog is in and the curtain is up.
fn cold_start(
    screen: &str,
) -> (
    App,
    Rc<MinimalSoftwareWindow>,
    tokio::runtime::Runtime,
    std::sync::Arc<crate::router::Ctx>,
) {
    let (app, window) = boot();
    let (runtime, ctx) = offline_ctx();
    crate::sizing::apply_scene(
        &app,
        crate::sizing::Scene::of(&app, f64::from(W), f64::from(H), false),
    );
    {
        let mut shared = crate::router::lock(&ctx.shared);
        shared.systems = navigation_catalog();
        shared.categories = vec!["Console".into()];
        shared.persist.hub.category = "Console".into();
        shared.persist.active_screen = screen.into();
        shared.persist.games.system_id = "System08".into();
    }
    app.global::<Shell>().set_boot_curtain(true);
    (app, window, runtime, std::sync::Arc::new(ctx))
}

/// The restore is still behind the curtain: nothing but the curtain has
/// been painted, and the saved state still names the target.
fn assert_still_curtained(
    app: &App,
    window: &Rc<MinimalSoftwareWindow>,
    ctx: &crate::router::Ctx,
    curtain: &[Rgb565Pixel],
    screen: &str,
) {
    let shell = app.global::<Shell>();
    assert!(shell.get_boot_curtain(), "the curtain holds until commit");
    assert_eq!(
        shell.get_active_screen(),
        Screen::Hub,
        "no screen is published under the curtain"
    );
    assert_eq!(
        crate::router::lock(&ctx.shared).persist.active_screen,
        screen,
        "a kill mid-restore comes back to the same screen"
    );
    assert_region_matches(
        &pixels(window),
        curtain,
        0..W as usize,
        0..H as usize,
        "only the curtain is painted during a restore",
    );
}

#[test]
fn cold_games_restore_shows_the_curtain_then_games_and_never_systems() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window, _runtime, ctx) = cold_start("games");
    let curtain = pixels(&window);
    crate::restore_screens(&ctx, &app);
    assert_still_curtained(&app, &window, &ctx, &curtain, "games");
    // Past the loading-cue delay: the curtain carries the only cue.
    for _ in 0..30 {
        CLOCK.with(|clock| clock.set(clock.get() + TICK_MS));
        slint::platform::update_timers_and_animations();
    }
    assert_still_curtained(&app, &window, &ctx, &curtain, "games");
    let ticket = crate::router::lock(&ctx.shared).games.ticket;
    crate::games::apply_fill(&ctx, &app, ticket, navigation_rows(), None, None, true);
    let shell = app.global::<Shell>();
    assert_eq!(shell.get_active_screen(), Screen::Games);
    assert!(!shell.get_boot_curtain(), "the commit lifts the curtain");
    assert!(!shell.get_transitioning());
    // The category was filled beneath it, so Back lands on a ready screen.
    crate::router::handle_action(&ctx, &app, "cancel");
    assert_eq!(shell.get_active_screen(), Screen::Systems);
    assert_eq!(app.global::<SystemsView>().get_category(), "Console");
}

#[test]
fn cold_hub_shortcut_games_restore_without_its_category() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    // No category ever opened, or one left over from an earlier visit.
    for saved_category in ["", "Console"] {
        for folder in [false, true] {
            let (app, window, _runtime, ctx) = cold_start("games");
            {
                let mut shared = crate::router::lock(&ctx.shared);
                shared.persist.hub.category = saved_category.into();
                shared.persist.games.entered_from_hub = true;
                if folder {
                    shared.persist.games.path_stack = vec![String::new(), "/rpg".into()];
                    shared.persist.games.selected_at_level = vec![String::new(), String::new()];
                }
            }
            let curtain = pixels(&window);
            crate::restore_screens(&ctx, &app);
            assert_still_curtained(&app, &window, &ctx, &curtain, "games");
            let ticket = {
                let shared = crate::router::lock(&ctx.shared);
                assert_eq!(shared.games.browse_path, if folder { "/rpg" } else { "" });
                shared.games.ticket
            };
            crate::games::apply_fill(&ctx, &app, ticket, navigation_rows(), None, None, true);
            let shell = app.global::<Shell>();
            assert_eq!(
                shell.get_active_screen(),
                Screen::Games,
                "{saved_category:?}"
            );
            assert!(!shell.get_boot_curtain());
            assert!(
                crate::router::lock(&ctx.shared)
                    .systems_model
                    .rows
                    .is_empty(),
                "no category is filled beneath a Hub shortcut"
            );
            if folder {
                crate::router::handle_action(&ctx, &app, "cancel");
                let ticket = crate::router::lock(&ctx.shared).games.ticket;
                crate::games::apply_fill(&ctx, &app, ticket, navigation_rows(), None, None, false);
                assert_eq!(shell.get_active_screen(), Screen::Games);
                assert_eq!(crate::router::lock(&ctx.shared).games.browse_path, "");
            }
            crate::router::handle_action(&ctx, &app, "cancel");
            assert_eq!(shell.get_active_screen(), Screen::Hub);
        }
    }

    // The system is gone: the Hub it was entered from is the restored screen.
    let (app, _window, _runtime, ctx) = cold_start("games");
    {
        let mut shared = crate::router::lock(&ctx.shared);
        shared.persist.games.entered_from_hub = true;
        shared.persist.games.system_id = "Gone".into();
    }
    crate::restore_screens(&ctx, &app);
    let shell = app.global::<Shell>();
    assert!(!shell.get_transitioning());
    crate::hub::set_resume(
        &ctx,
        &app,
        crate::hub::Resume {
            requested: true,
            loading: false,
            entry: None,
        },
    );
    assert_eq!(shell.get_active_screen(), Screen::Hub);
    assert!(!shell.get_boot_curtain());
    assert!(crate::router::lock(&ctx.shared)
        .systems_model
        .rows
        .is_empty());
}

#[test]
fn cold_flat_list_restores_show_the_curtain_then_the_list_and_never_the_hub() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    for (token, screen) in [
        ("favorites", Screen::Favorites),
        ("recents", Screen::Recents),
        ("search-results", Screen::SearchResults),
    ] {
        let (app, window, _runtime, ctx) = cold_start(token);
        let curtain = pixels(&window);
        crate::restore_screens(&ctx, &app);
        assert!(
            !crate::navigation::active(),
            "{token}: no Hub source is retained under the curtain"
        );
        assert_still_curtained(&app, &window, &ctx, &curtain, token);
        let ticket = crate::router::lock(&ctx.shared).games.ticket;
        crate::games::apply_fill(&ctx, &app, ticket, navigation_rows(), None, None, true);
        let shell = app.global::<Shell>();
        assert_eq!(shell.get_active_screen(), screen, "{token}");
        assert!(!shell.get_boot_curtain(), "{token}");
    }
}

#[test]
fn cold_systems_and_search_restores_commit_in_the_same_turn() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    for (token, screen) in [("systems", Screen::Systems), ("search", Screen::Search)] {
        let (app, _window, _runtime, ctx) = cold_start(token);
        crate::restore_screens(&ctx, &app);
        let shell = app.global::<Shell>();
        assert_eq!(shell.get_active_screen(), screen, "{token}");
        assert!(!shell.get_boot_curtain(), "{token}");
    }
}

#[test]
fn cancel_during_a_cold_restore_retires_the_fill_and_lands_on_the_parent() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, _window, _runtime, ctx) = cold_start("games");
    crate::restore_screens(&ctx, &app);
    let ticket = crate::router::lock(&ctx.shared).games.ticket;
    crate::router::handle_action(&ctx, &app, "cancel");
    let shell = app.global::<Shell>();
    assert_eq!(shell.get_active_screen(), Screen::Systems);
    assert!(!shell.get_boot_curtain());
    assert!(!shell.get_transitioning());
    // The answer that was in flight arrives late and must route nowhere.
    crate::games::apply_fill(&ctx, &app, ticket, navigation_rows(), None, None, true);
    assert_eq!(shell.get_active_screen(), Screen::Systems);
    assert_eq!(
        crate::router::lock(&ctx.shared).persist.active_screen,
        "games",
        "the next start tries the same screen again"
    );

    // A flat list has no parent but the Hub.
    let (app, _window, _runtime, ctx) = cold_start("favorites");
    crate::restore_screens(&ctx, &app);
    crate::router::handle_action(&ctx, &app, "cancel");
    let shell = app.global::<Shell>();
    assert_eq!(shell.get_active_screen(), Screen::Hub);
    assert!(!shell.get_boot_curtain());
}

#[test]
fn a_cold_restore_that_fails_or_times_out_always_lifts_the_curtain() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    // Core answers with an error: the target shows it.
    let (app, _window, _runtime, ctx) = cold_start("games");
    crate::restore_screens(&ctx, &app);
    let ticket = crate::router::lock(&ctx.shared).games.ticket;
    crate::games::show_error(&ctx, &app, ticket, "boom", true);
    let shell = app.global::<Shell>();
    assert_eq!(shell.get_active_screen(), Screen::Games);
    assert!(!shell.get_boot_curtain());

    // Core never answers: the bound gives up onto the parent.
    let (app, _window, _runtime, ctx) = cold_start("games");
    crate::restore_screens(&ctx, &app);
    let ticket = crate::router::lock(&ctx.shared).games.ticket;
    CLOCK.with(|clock| clock.set(clock.get() + 15_001));
    slint::platform::update_timers_and_animations();
    let shell = app.global::<Shell>();
    assert_eq!(shell.get_active_screen(), Screen::Systems);
    assert!(!shell.get_boot_curtain());
    crate::games::apply_fill(&ctx, &app, ticket, navigation_rows(), None, None, true);
    assert_eq!(shell.get_active_screen(), Screen::Systems);

    // The saved system is gone: its category is the nearest screen left.
    let (app, _window, _runtime, ctx) = cold_start("games");
    crate::router::lock(&ctx.shared).persist.games.system_id = "Gone".into();
    crate::restore_screens(&ctx, &app);
    let shell = app.global::<Shell>();
    assert_eq!(shell.get_active_screen(), Screen::Systems);
    assert!(!shell.get_boot_curtain());
}

#[test]
fn a_cold_hub_start_waits_for_the_resume_tile_behind_the_curtain() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let settled = || crate::hub::Resume {
        requested: true,
        loading: false,
        entry: None,
    };
    // The saved category is gone, so the Hub is the restored screen.
    let (app, _window, _runtime, ctx) = cold_start("systems");
    crate::router::lock(&ctx.shared).persist.hub.category = "Gone".into();
    crate::restore_screens(&ctx, &app);
    let shell = app.global::<Shell>();
    assert!(shell.get_boot_curtain(), "the Resume tile is still unknown");
    crate::hub::set_resume(&ctx, &app, settled());
    assert_eq!(shell.get_active_screen(), Screen::Hub);
    assert!(!shell.get_boot_curtain());

    // Core never answers the Resume tile: the Hub shows without an alert.
    let (app, _window, _runtime, ctx) = cold_start("hub");
    crate::finish_hub_restore(&ctx, &app);
    let shell = app.global::<Shell>();
    assert!(shell.get_boot_curtain());
    CLOCK.with(|clock| clock.set(clock.get() + 15_001));
    slint::platform::update_timers_and_animations();
    assert!(!shell.get_boot_curtain());
    assert!(!app.global::<crate::Overlays>().get_dialog_open());

    // A settled Resume tile does not end a restore that is filling a list.
    let (app, _window, _runtime, ctx) = cold_start("games");
    crate::restore_screens(&ctx, &app);
    crate::hub::set_resume(&ctx, &app, settled());
    assert!(app.global::<Shell>().get_boot_curtain());
}

#[test]
fn hub_and_flat_list_logos_follow_the_logo_style_and_region_settings() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (_app, _window) = boot();
    let (_runtime, ctx) = offline_ctx();
    let bounds = zaparoo_app::logo_cache::Bounds::new(128, 128);
    let tinted = ctx.logos.key("Genesis", "Genesis", false, bounds);
    let regional_color = ctx.logos.key("Genesis", "Genesis.jp", true, bounds);
    assert!(tinted.is_some() && regional_color.is_some());
    assert_ne!(tinted, regional_color);

    // The Hub resolves a shortcut's stem by region, then its style.
    assert_eq!(
        crate::hub::logo_key(&ctx, "Genesis", "tinted", bounds),
        tinted
    );
    assert_eq!(
        crate::hub::logo_key(&ctx, "Genesis.jp", "color", bounds),
        regional_color
    );

    // The flat lists' "no cover" logo reads the same two settings.
    {
        let mut shared = crate::router::lock(&ctx.shared);
        shared.persist.settings.system_logo_style = "color".into();
        shared.persist.settings.region = "jp".into();
    }
    let prefs = crate::systems::LogoPrefs::of(&crate::router::lock(&ctx.shared));
    assert_eq!(prefs.key(&ctx, "Genesis", bounds), regional_color);
    {
        let mut shared = crate::router::lock(&ctx.shared);
        shared.persist.settings.system_logo_style = "tinted".into();
        shared.persist.settings.region = "us".into();
    }
    let prefs = crate::systems::LogoPrefs::of(&crate::router::lock(&ctx.shared));
    assert_eq!(prefs.key(&ctx, "Genesis", bounds), tinted);
}

#[test]
fn update_metadata_opens_the_setup_panel_on_the_menus_scope() {
    use zaparoo_app::media_setup::{Kind, Scope};
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, _window) = boot();
    let (_runtime, ctx) = offline_ctx();
    {
        let mut shared = crate::router::lock(&ctx.shared);
        shared.systems = navigation_catalog();
        shared.categories = vec!["Console".into()];
    }
    let opened = |ctx: &crate::router::Ctx| {
        let shared = crate::router::lock(&ctx.shared);
        (
            shared.setup.open,
            shared.setup.kind,
            shared.setup.scope.clone(),
            shared.setup.rescrape,
        )
    };
    crate::router::open_scrape_setup(&ctx, &app, Scope::System("System08".into()));
    assert_eq!(
        opened(&ctx),
        (true, Kind::Scrape, Scope::System("System08".into()), false)
    );
    crate::media_setup::close(&ctx, &app);
    crate::router::scrape_category(&ctx, &app, "Console");
    assert_eq!(
        opened(&ctx),
        (true, Kind::Scrape, Scope::Category("Console".into()), false)
    );
    crate::media_setup::close(&ctx, &app);
    // A category with nothing to scrape opens nothing.
    crate::router::scrape_category(&ctx, &app, "Empty");
    assert!(!opened(&ctx).0);
}

#[test]
fn back_from_what_an_options_row_opened_returns_to_the_menu_on_its_row() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    let (_runtime, ctx) = offline_ctx();
    seat_visibility_list(&ctx, &app, GamesMode::Browse);
    let overlays = app.global::<crate::Overlays>();
    let setup_open = |ctx: &crate::router::Ctx| crate::router::lock(&ctx.shared).setup.open;
    let open_manage_row = |id: &str| {
        crate::router::handle_action(&ctx, &app, "context_menu");
        focus_context_row(&app, "manage_game");
        crate::router::handle_action(&ctx, &app, "accept");
        advance(zaparoo_app::input::PRESS_FEEDBACK_MS);
        settle(&window);
        focus_context_row(&app, id);
        crate::router::handle_action(&ctx, &app, "accept");
        advance(zaparoo_app::input::PRESS_FEEDBACK_MS);
        settle(&window);
    };

    // Update metadata, then Back: the Manage page again, on that row.
    open_manage_row("scrape_game");
    assert!(setup_open(&ctx));
    assert!(!overlays.get_context_open());
    crate::router::handle_action(&ctx, &app, "cancel");
    assert!(!setup_open(&ctx));
    assert!(overlays.get_context_open(), "the menu comes back");
    assert_eq!(
        overlays.get_context_page(),
        crate::ContextPage::ManageGame,
        "on its page"
    );
    assert_eq!(context_row_id(&app).as_deref(), Some("scrape_game"));
    // Back on the page then leaves it as it always did.
    crate::router::handle_action(&ctx, &app, "cancel");
    assert_eq!(overlays.get_context_page(), crate::ContextPage::Root);
    assert_eq!(context_row_id(&app).as_deref(), Some("manage_game"));
    crate::router::handle_action(&ctx, &app, "cancel");
    assert!(!overlays.get_context_open());

    // Once it has finished, Back from anything later is the screen's own.
    open_manage_row("scrape_game");
    crate::router::forget_menu_return(&ctx);
    crate::router::handle_action(&ctx, &app, "cancel");
    assert!(!setup_open(&ctx));
    assert!(
        !overlays.get_context_open(),
        "a finished action ends on the screen"
    );

    // Write to token with no reader shows the QR code; Back returns to
    // the row that opened it.
    crate::router::handle_action(&ctx, &app, "context_menu");
    focus_context_row(&app, "write_token");
    crate::router::handle_action(&ctx, &app, "accept");
    advance(zaparoo_app::input::PRESS_FEEDBACK_MS);
    settle(&window);
    assert!(overlays.get_qr_open());
    crate::router::handle_action(&ctx, &app, "cancel");
    assert!(!overlays.get_qr_open());
    assert!(overlays.get_context_open());
    assert_eq!(overlays.get_context_page(), crate::ContextPage::Root);
    assert_eq!(context_row_id(&app).as_deref(), Some("write_token"));
}

#[test]
fn cold_favorite_systems_restore_commits_behind_the_curtain() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window, _runtime, ctx) = cold_start("favorite-systems");
    let curtain = pixels(&window);
    crate::restore_screens(&ctx, &app);
    assert_still_curtained(&app, &window, &ctx, &curtain, "favorite-systems");
    let result = zaparoo_core::media_types::SystemsResult {
        systems: navigation_catalog(),
    };
    crate::systems::apply_favorites(&ctx, &app, &result, 1, true);
    let shell = app.global::<Shell>();
    assert_eq!(shell.get_active_screen(), Screen::FavoriteSystems);
    assert!(!shell.get_boot_curtain());
}

#[test]
fn a_scoped_metadata_run_never_widens_and_needs_a_source_that_covers_it() {
    use zaparoo_app::media_setup::Scope;
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, _window) = boot();
    let (_runtime, ctx) = offline_ctx();
    let focus_start = |ctx: &crate::router::Ctx| {
        let mut shared = crate::router::lock(&ctx.shared);
        shared.setup.index = shared.setup.rows().len() - 1;
    };
    let dialog_open = |app: &App| app.global::<crate::Overlays>().get_dialog_open();

    // The chosen source does not handle the scoped system: an alert, and
    // the panel stays for another source or scope.
    {
        let mut shared = crate::router::lock(&ctx.shared);
        shared.systems = navigation_catalog();
        shared.categories = vec!["Console".into()];
    }
    crate::router::open_scrape_setup(&ctx, &app, Scope::System("System08".into()));
    {
        let mut shared = crate::router::lock(&ctx.shared);
        shared.setup.scrapers = vec![zaparoo_core::media_types::ScraperInfo {
            id: "pinball".into(),
            name: "Pinball".into(),
            supported_systems: vec!["Pinball".into()],
        }];
        shared.setup.scraper = "pinball".into();
    }
    focus_start(&ctx);
    crate::media_setup::handle_action(&ctx, &app, "accept");
    assert!(dialog_open(&app));
    assert!(crate::router::lock(&ctx.shared).setup.open);

    // The category emptied after the panel opened: Core would read the
    // empty list as every system, so nothing starts.
    let (app, _window) = boot();
    let (_runtime, ctx) = offline_ctx();
    {
        let mut shared = crate::router::lock(&ctx.shared);
        shared.systems = navigation_catalog();
        shared.categories = vec!["Console".into()];
    }
    crate::router::open_scrape_setup(&ctx, &app, Scope::Category("Console".into()));
    crate::router::lock(&ctx.shared).systems.clear();
    focus_start(&ctx);
    crate::media_setup::handle_action(&ctx, &app, "accept");
    assert!(!crate::router::lock(&ctx.shared).setup.open);
    assert!(!dialog_open(&app));
}

#[test]
fn the_systems_page_checks_systems_and_back_keeps_them() {
    use zaparoo_app::media_setup::{FormRow, Scope};
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, _window) = boot();
    let (_runtime, ctx) = offline_ctx();
    {
        let mut shared = crate::router::lock(&ctx.shared);
        shared.systems = navigation_catalog();
        shared.categories = vec!["Console".into()];
    }
    let view = app.global::<crate::SetupModalView>();
    let scope = |ctx: &crate::router::Ctx| crate::router::lock(&ctx.shared).setup.scope.clone();
    let page = |ctx: &crate::router::Ctx| crate::router::lock(&ctx.shared).setup.picker;
    let press = |action: &str| crate::media_setup::handle_action(&ctx, &app, action);
    let open_systems_page = || {
        crate::router::lock(&ctx.shared).setup.index = 1;
        press("accept");
    };

    // The form's own system starts checked and leads the list: All
    // systems, the category, the Selected heading, then System 08.
    crate::router::open_scrape_setup(&ctx, &app, Scope::System("System08".into()));
    open_systems_page();
    assert_eq!(page(&ctx), Some(FormRow::Systems));
    assert_eq!(crate::router::lock(&ctx.shared).setup.picker_index, 3);
    assert!(view.get_picker_toggle());
    assert_eq!(view.get_picker_checked(), 1);

    // Accept on a system checks it and stays on the page. Down passes
    // over the manufacturer heading to the first system under it.
    press("down");
    press("accept");
    assert_eq!(page(&ctx), Some(FormRow::Systems));
    assert_eq!(view.get_picker_checked(), 2);
    assert_eq!(scope(&ctx), Scope::System("System08".into()));

    // Back has nothing to confirm: the checks are the form's scope, in
    // the order the list showed them (the Selected rows lead it).
    press("cancel");
    assert_eq!(page(&ctx), None);
    assert_eq!(
        scope(&ctx),
        Scope::Systems(vec!["System08".into(), "System00".into()])
    );
    assert_eq!(view.get_picker_checked(), 0);

    // Unchecking everything leaves the scope the form already had.
    open_systems_page();
    press("accept");
    press("down");
    press("accept");
    assert_eq!(view.get_picker_checked(), 0);
    press("cancel");
    assert_eq!(
        scope(&ctx),
        Scope::Systems(vec!["System08".into(), "System00".into()])
    );

    // All systems is one press: it clears the checks and returns.
    open_systems_page();
    crate::router::lock(&ctx.shared).setup.picker_index = 0;
    crate::media_setup::render(&ctx, &app);
    assert!(!view.get_picker_toggle());
    press("accept");
    assert_eq!(page(&ctx), None);
    assert_eq!(scope(&ctx), Scope::All);
    assert!(crate::router::lock(&ctx.shared).setup.checked.is_empty());
}

#[test]
fn a_game_stays_on_offer_and_never_widens_to_its_system() {
    use zaparoo_app::media_setup::{GameTarget, Scope};
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, _window) = boot();
    let (_runtime, ctx) = offline_ctx();
    {
        let mut shared = crate::router::lock(&ctx.shared);
        shared.systems = navigation_catalog();
        shared.categories = vec!["Console".into()];
    }
    // Core has nothing to find this row by: no media ID and no path.
    let game = Scope::Game(GameTarget {
        media_id: None,
        system: "System08".into(),
        path: String::new(),
        name: "Game 01".into(),
    });
    let scope = |ctx: &crate::router::Ctx| crate::router::lock(&ctx.shared).setup.scope.clone();
    let press = |action: &str| crate::media_setup::handle_action(&ctx, &app, action);
    let open_systems_page = || {
        crate::router::lock(&ctx.shared).setup.index = 1;
        press("accept");
    };

    // The game leads the Systems page, and the page opens on it.
    crate::router::open_scrape_setup(&ctx, &app, game.clone());
    open_systems_page();
    assert_eq!(crate::router::lock(&ctx.shared).setup.picker_index, 0);
    let first = app
        .global::<crate::SetupModalView>()
        .get_picker_rows()
        .iter()
        .find(|row| !row.name.is_empty())
        .map(|row| (row.kind, row.name.to_string()));
    assert_eq!(first, Some((crate::ScopeKind::Game, "Game 01".to_string())));

    // Widen to the category, then come back to the game: still offered.
    press("down");
    press("down");
    press("accept");
    assert_eq!(scope(&ctx), Scope::Category("Console".into()));
    open_systems_page();
    crate::router::lock(&ctx.shared).setup.picker_index = 0;
    press("accept");
    assert_eq!(scope(&ctx), game);

    // Starting it says so, and leaves the panel open: it must not run
    // over the game's whole system instead.
    {
        let mut shared = crate::router::lock(&ctx.shared);
        shared.setup.scraper = "source".into();
        shared.setup.index = shared.setup.rows().len() - 1;
    }
    press("accept");
    assert!(app.global::<crate::Overlays>().get_dialog_open());
    assert!(crate::router::lock(&ctx.shared).setup.open);
}

#[test]
fn a_game_is_not_sent_to_a_core_that_would_ignore_its_scope() {
    use zaparoo_app::media_setup::{GameTarget, Scope};
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, _window) = boot();
    let (_runtime, ctx) = offline_ctx();
    {
        let mut shared = crate::router::lock(&ctx.shared);
        shared.systems = navigation_catalog();
        // Below the floor: it reads the request as every system.
        shared.core_version = "2.17.2".into();
    }
    crate::router::open_scrape_setup(
        &ctx,
        &app,
        Scope::Game(GameTarget {
            media_id: Some(7),
            system: "System08".into(),
            path: "/games/7".into(),
            name: "Game 07".into(),
        }),
    );
    {
        let mut shared = crate::router::lock(&ctx.shared);
        shared.setup.scraper = "source".into();
        shared.setup.index = shared.setup.rows().len() - 1;
    }
    crate::media_setup::handle_action(&ctx, &app, "accept");
    assert!(app.global::<crate::Overlays>().get_dialog_open());
    assert!(crate::router::lock(&ctx.shared).setup.open);
}

/// A detailed list of 40 rows with the first 20 loaded and more to come.
fn seat_partial_list(ctx: &crate::router::Ctx, app: &App) -> (Vec<crate::games::GameRow>, u64) {
    crate::sizing::apply_scene(
        app,
        crate::sizing::Scene::of(app, f64::from(W), f64::from(H), false),
    );
    app.global::<Shell>().set_active_screen(Screen::Games);
    let mut rows = game_rows("Row", 40);
    for (index, row) in rows.iter_mut().enumerate() {
        row.path = format!("/target/{index}");
    }
    let ticket = {
        let mut shared = crate::router::lock(&ctx.shared);
        shared.persist.settings.games_browse_layout = "list".into();
        shared.persist.games.path_stack = vec!["/target".into()];
        shared.persist.games.selected_at_level = vec![String::new()];
        shared.games.browse_path = "/target".into();
        shared.games.total_known = true;
        shared.games.ticket
    };
    crate::games::apply_fill(
        ctx,
        app,
        ticket,
        rows[..20].to_vec(),
        Some("first".into()),
        Some((40, 0)),
        false,
    );
    (rows, ticket)
}

#[test]
fn up_from_the_top_of_a_partly_loaded_list_walks_to_its_true_last_row() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, _window) = boot();
    let (_runtime, ctx) = offline_ctx();
    let (rows, ticket) = seat_partial_list(&ctx, &app);
    crate::games::handle_action(&ctx, &app, "up");
    {
        let shared = crate::router::lock(&ctx.shared);
        assert_eq!(
            shared.games.grid.current_index(),
            0,
            "the last loaded row is not the tail: nothing moves until the tail loads"
        );
        assert!(shared.games.grid.has_pending_target());
        assert!(shared.games.jump_loading);
    }
    crate::games::on_append(&ctx, &app, ticket, Ok((rows[20..].to_vec(), None)));
    let shared = crate::router::lock(&ctx.shared);
    assert_eq!(shared.games.grid.current_index(), 39);
    assert!(!shared.games.jump_loading);
}

#[test]
fn a_page_move_past_the_loaded_rows_lands_on_the_last_one_and_loads_more() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, _window) = boot();
    let (_runtime, ctx) = offline_ctx();
    let _ = seat_partial_list(&ctx, &app);
    crate::router::lock(&ctx.shared)
        .games
        .grid
        .set_current_index_immediate(17);
    crate::games::handle_action(&ctx, &app, "right");
    let shared = crate::router::lock(&ctx.shared);
    assert_eq!(shared.games.grid.current_index(), 19);
    assert!(shared.games.loading_more);
}

#[test]
fn a_page_core_refuses_restarts_the_list_and_a_link_failure_does_not() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, _window) = boot();
    let (_runtime, ctx) = offline_ctx();
    let (rows, ticket) = seat_partial_list(&ctx, &app);
    crate::router::lock(&ctx.shared)
        .games
        .grid
        .set_current_index_immediate(15);

    // The link dropped: the rows and the cursor stay for the next move.
    crate::games::on_append(
        &ctx,
        &app,
        ticket,
        Err(crate::games::PageError::transport("not connected")),
    );
    {
        let shared = crate::router::lock(&ctx.shared);
        assert_eq!(shared.games.ticket, ticket);
        assert_eq!(shared.games.rows.len(), 20);
        assert!(shared.games.has_more());
        assert!(!shared.games.loading);
    }

    // Core will not serve that cursor again: reload from the first page.
    crate::games::on_append(
        &ctx,
        &app,
        ticket,
        Err(crate::games::PageError::refused(
            "library visibility changed; restart browse without cursor",
        )),
    );
    let restarted = {
        let shared = crate::router::lock(&ctx.shared);
        assert_ne!(shared.games.ticket, ticket, "a fresh fill is under way");
        assert!(shared.games.loading);
        assert!(shared.games.page_restarted);
        assert_eq!(
            shared.persist.games.selected_at_level,
            vec![rows[15].path.clone()],
            "the selection is saved for the reload to restore"
        );
        shared.games.ticket
    };
    // The reload's first page stops short of the selection, so it walks.
    crate::games::apply_fill(
        &ctx,
        &app,
        restarted,
        rows[..10].to_vec(),
        Some("fresh".into()),
        Some((40, 0)),
        false,
    );
    crate::games::on_append(
        &ctx,
        &app,
        restarted,
        Ok((rows[10..20].to_vec(), Some("fresh-2".into()))),
    );
    {
        let shared = crate::router::lock(&ctx.shared);
        assert_eq!(shared.games.grid.current_index(), 15);
        assert!(shared.games.error.is_empty());
        assert!(
            !shared.games.page_restarted,
            "a page loaded, so a later refusal may restart again"
        );
    }
}

#[test]
fn a_reload_that_core_refuses_too_is_shown_instead_of_looping() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, _window) = boot();
    let (_runtime, ctx) = offline_ctx();
    let (rows, ticket) = seat_partial_list(&ctx, &app);
    crate::router::lock(&ctx.shared)
        .games
        .grid
        .set_current_index_immediate(15);
    crate::games::on_append(
        &ctx,
        &app,
        ticket,
        Err(crate::games::PageError::refused("refused")),
    );
    let restarted = crate::router::lock(&ctx.shared).games.ticket;
    crate::games::apply_fill(
        &ctx,
        &app,
        restarted,
        rows[..10].to_vec(),
        Some("fresh".into()),
        Some((40, 0)),
        false,
    );
    crate::games::on_append(
        &ctx,
        &app,
        restarted,
        Err(crate::games::PageError::refused("refused again")),
    );
    let shared = crate::router::lock(&ctx.shared);
    assert_eq!(shared.games.ticket, restarted, "no second restart");
    assert_eq!(shared.games.error, "refused again");
}

#[test]
fn a_page_with_no_cursor_ends_the_list_whatever_total_core_reported() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, _window) = boot();
    let (_runtime, ctx) = offline_ctx();
    let (rows, ticket) = seat_partial_list(&ctx, &app);
    crate::games::on_append(&ctx, &app, ticket, Ok((rows[20..25].to_vec(), None)));
    assert_eq!(app.global::<crate::GamesView>().get_total_items(), 25);
    crate::router::lock(&ctx.shared)
        .games
        .grid
        .set_current_index_immediate(24);
    // With nothing more to load the list wraps instead of waiting on rows
    // that will never come.
    crate::games::handle_action(&ctx, &app, "down");
    let shared = crate::router::lock(&ctx.shared);
    assert_eq!(shared.games.grid.current_index(), 0);
    assert!(!shared.games.loading_more);
}

#[test]
fn a_letter_jump_over_game_folders_counts_from_the_first_folder() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, _window) = boot();
    let (_runtime, ctx) = offline_ctx();
    crate::sizing::apply_scene(
        &app,
        crate::sizing::Scene::of(&app, f64::from(W), f64::from(H), false),
    );
    app.global::<Shell>().set_active_screen(Screen::Games);
    // A system whose every game is a folder: 30 directory rows, no files.
    let mut rows = game_rows("Disc Game", 30);
    for (index, row) in rows.iter_mut().enumerate() {
        row.path = format!("/psx/{index}");
        row.entry_type = zaparoo_app::media_list::EntryType::Directory;
        row.media_capable = true;
    }
    let ticket = {
        let mut shared = crate::router::lock(&ctx.shared);
        shared.persist.settings.games_browse_layout = "list".into();
        shared.games.total_known = true;
        shared.games.ticket
    };
    crate::games::apply_fill(&ctx, &app, ticket, rows, None, Some((0, 30)), false);

    // Core indexed the folders, so the offset is already the row.
    crate::router::lock(&ctx.shared).letter_directories = true;
    crate::games::jump_to_item(&ctx, &app, 12);
    assert_eq!(
        crate::router::lock(&ctx.shared).games.grid.current_index(),
        12
    );

    // A file index counts from the first file, after every directory.
    crate::router::lock(&ctx.shared).letter_directories = false;
    crate::games::jump_to_item(&ctx, &app, 12);
    assert_eq!(
        crate::router::lock(&ctx.shared).games.grid.current_index(),
        29,
        "30 directories lead, so file 12 lies past the end and clamps"
    );
}

fn header_cue(app: &App) -> crate::AppCue {
    app.global::<Shell>().get_status_text()
}

#[test]
fn a_header_wait_cue_skips_a_fast_answer_and_never_flashes_a_slow_one() {
    use zaparoo_app::wait_cue::{CUE_DELAY_MS, CUE_HOLD_MS};
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, _window) = boot();
    let (_runtime, ctx) = offline_ctx();

    // Answered inside the delay: no word at all.
    let fast = crate::cue::begin(&ctx, &app, crate::AppCue::Saving, "", "");
    advance(CUE_DELAY_MS - 1);
    assert_eq!(header_cue(&app), crate::AppCue::None);
    crate::cue::end(&ctx, &app, fast);
    advance(CUE_DELAY_MS + CUE_HOLD_MS);
    assert_eq!(header_cue(&app), crate::AppCue::None);

    // Answered just after the word appears: it stays out its hold.
    let slow = crate::cue::begin(&ctx, &app, crate::AppCue::Launching, "Tetris", "");
    advance(CUE_DELAY_MS);
    assert_eq!(header_cue(&app), crate::AppCue::Launching);
    assert_eq!(app.global::<Shell>().get_status_arg(), "Tetris");
    crate::cue::end(&ctx, &app, slow);
    assert_eq!(header_cue(&app), crate::AppCue::Launching);
    advance(CUE_HOLD_MS);
    assert_eq!(header_cue(&app), crate::AppCue::None);
    assert_eq!(app.global::<Shell>().get_status_arg(), "");

    // Answered long after: cleared the moment the answer arrives.
    let long = crate::cue::begin(&ctx, &app, crate::AppCue::Starting, "", "");
    advance(CUE_DELAY_MS);
    advance(CUE_HOLD_MS);
    advance(500);
    assert_eq!(header_cue(&app), crate::AppCue::Starting);
    crate::cue::end(&ctx, &app, long);
    assert_eq!(header_cue(&app), crate::AppCue::None);
}

#[test]
fn a_confirmation_shows_at_once_and_a_new_wait_takes_the_line_from_it() {
    use zaparoo_app::wait_cue::{CONFIRM_MS, CUE_DELAY_MS};
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, _window) = boot();
    let (_runtime, ctx) = offline_ctx();

    crate::cue::flash(&ctx, &app, crate::AppCue::AddedToHub, CONFIRM_MS);
    assert_eq!(header_cue(&app), crate::AppCue::AddedToHub);
    advance(CONFIRM_MS - 1);
    assert_eq!(header_cue(&app), crate::AppCue::AddedToHub);
    advance(1);
    assert_eq!(header_cue(&app), crate::AppCue::None);

    crate::cue::flash(&ctx, &app, crate::AppCue::TokenWritten, CONFIRM_MS);
    let wait = crate::cue::begin(&ctx, &app, crate::AppCue::Saving, "", "");
    assert_eq!(header_cue(&app), crate::AppCue::None);
    advance(CUE_DELAY_MS);
    assert_eq!(header_cue(&app), crate::AppCue::Saving);
    // The confirmation's timer must not clear the wait that replaced it.
    advance(CONFIRM_MS);
    assert_eq!(header_cue(&app), crate::AppCue::Saving);
    crate::cue::end(&ctx, &app, wait);
    assert_eq!(header_cue(&app), crate::AppCue::None);
}

#[test]
fn a_page_load_says_so_in_the_header_and_leaves_the_list_as_it_is() {
    use zaparoo_app::wait_cue::{CUE_DELAY_MS, CUE_HOLD_MS};
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, _window) = boot();
    let (_runtime, ctx) = offline_ctx();
    let (rows, ticket) = seat_partial_list(&ctx, &app);
    crate::router::lock(&ctx.shared)
        .games
        .grid
        .set_current_index_immediate(19);
    crate::games::handle_action(&ctx, &app, "down");
    assert!(crate::router::lock(&ctx.shared).games.loading_more);
    assert_eq!(header_cue(&app), crate::AppCue::None, "not a wait yet");
    advance(CUE_DELAY_MS);
    assert_eq!(header_cue(&app), crate::AppCue::LoadingMore);
    assert_eq!(app.global::<crate::GamesView>().get_current_index(), 19);

    crate::games::on_append(&ctx, &app, ticket, Ok((rows[20..].to_vec(), None)));
    advance(CUE_HOLD_MS);
    assert_eq!(header_cue(&app), crate::AppCue::None);
}

#[test]
fn a_walk_to_a_distant_row_reports_how_far_it_has_got() {
    use zaparoo_app::wait_cue::CUE_DELAY_MS;
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, _window) = boot();
    let (_runtime, ctx) = offline_ctx();
    let (rows, ticket) = seat_partial_list(&ctx, &app);
    // Up from the top walks to row 40 of 40, with 20 loaded.
    crate::games::handle_action(&ctx, &app, "up");
    advance(CUE_DELAY_MS);
    let shell = app.global::<Shell>();
    assert_eq!(header_cue(&app), crate::AppCue::LoadingProgress);
    assert_eq!(shell.get_status_arg(), "20");
    assert_eq!(shell.get_status_arg2(), "40");
    // A chunk that does not finish the walk moves the count, not the delay.
    crate::games::on_append(
        &ctx,
        &app,
        ticket,
        Ok((rows[20..30].to_vec(), Some("more".into()))),
    );
    assert_eq!(header_cue(&app), crate::AppCue::LoadingProgress);
    assert_eq!(shell.get_status_arg(), "30");
}

#[test]
fn a_list_reloading_in_place_keeps_its_rows_and_cues_in_the_header() {
    use zaparoo_app::wait_cue::{CUE_DELAY_MS, CUE_HOLD_MS};
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, _window) = boot();
    let (_runtime, ctx) = offline_ctx();
    let (rows, ticket) = seat_partial_list(&ctx, &app);
    let view = app.global::<crate::GamesView>();
    let before = view.get_list_rows().row_count();
    assert!(before > 0);

    // Core refused a page: the list reloads from its first page.
    crate::games::on_append(
        &ctx,
        &app,
        ticket,
        Err(crate::games::PageError::refused("refused")),
    );
    assert!(crate::router::lock(&ctx.shared).games.loading);
    assert!(
        !view.get_loading(),
        "the body does not switch to a loading cue"
    );
    assert_eq!(view.get_list_rows().row_count(), before, "the rows stay up");
    advance(CUE_DELAY_MS);
    assert_eq!(header_cue(&app), crate::AppCue::LoadingList);

    // Only Cancel is taken while the rows on screen are stale.
    crate::games::handle_action(&ctx, &app, "down");
    assert_eq!(view.get_list_rows().row_count(), before);

    let restarted = crate::router::lock(&ctx.shared).games.ticket;
    crate::games::apply_fill(
        &ctx,
        &app,
        restarted,
        rows.clone(),
        None,
        Some((40, 0)),
        false,
    );
    assert!(!crate::router::lock(&ctx.shared).games.loading);
    advance(CUE_HOLD_MS);
    assert_eq!(header_cue(&app), crate::AppCue::None);
}

#[test]
fn cancel_gives_up_on_a_launcher_save_core_never_answers() {
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
    crate::router::bind_context_input(&std::sync::Arc::new(ctx.clone()), &app);
    let ticket = crate::launchers::begin_save(&ctx, &app);
    advance(300);
    assert!(ov.get_launcher_saving_visible());
    crate::router::handle_action(&ctx, &app, "cancel");
    assert!(!ov.get_launcher_saving());
    assert!(!ov.get_launcher_saving_visible());
    assert!(!ov.get_list_open(), "the same press closes the picker");
    // The answer, if it ever comes, belongs to a save nobody is waiting on.
    ov.set_list_open(true);
    crate::launchers::finish_save(&ctx, &app, ticket, None);
    assert!(ov.get_list_open());
}

#[test]
fn cancel_retires_a_launcher_save_answer_the_hold_put_off() {
    use zaparoo_app::wait_cue::{CUE_DELAY_MS, CUE_HOLD_MS};
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
    crate::router::bind_context_input(&std::sync::Arc::new(ctx.clone()), &app);
    let before = ov.get_dialog_error();
    let ticket = crate::launchers::begin_save(&ctx, &app);
    advance(CUE_DELAY_MS);
    assert!(ov.get_launcher_saving_visible());
    let payload = serde_json::json!(["system", "SNES", "", "alternate"]).to_string();
    crate::launchers::finish_save(&ctx, &app, ticket, Some(&payload));
    assert!(
        ov.get_launcher_saving_visible(),
        "the hold keeps the cue up"
    );
    crate::router::handle_action(&ctx, &app, "cancel");
    assert!(!ov.get_list_open());
    // A picker opened after Cancel is not the one that save belonged to.
    ov.set_list_open(true);
    advance(CUE_HOLD_MS);
    assert!(ov.get_list_open());
    assert_eq!(ov.get_dialog_error(), before);
}

#[test]
fn a_scraper_list_answer_reaches_only_the_form_that_asked() {
    use zaparoo_app::media_setup::Scope;
    use zaparoo_app::wait_cue::{CUE_DELAY_MS, CUE_HOLD_MS};
    use zaparoo_core::media_types::{ScraperInfo, ScrapersResult};
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, _window) = boot();
    let (_runtime, ctx) = offline_ctx();
    let answer = || {
        Ok(ScrapersResult {
            scrapers: vec![ScraperInfo {
                id: "source".into(),
                name: "Source".into(),
                supported_systems: Vec::new(),
            }],
        })
    };
    let seq = |ctx: &crate::router::Ctx| crate::router::lock(&ctx.shared).setup.sources_seq;
    let listed = |ctx: &crate::router::Ctx| crate::router::lock(&ctx.shared).setup.scrapers.len();

    crate::router::open_scrape_setup(&ctx, &app, Scope::All);
    let first = seq(&ctx);
    crate::media_setup::close(&ctx, &app);
    crate::media_setup::scrapers_answered(&ctx, &app, first, answer());
    assert_eq!(listed(&ctx), 0, "a closed form takes no answer");

    crate::router::open_scrape_setup(&ctx, &app, Scope::All);
    let second = seq(&ctx);
    crate::media_setup::scrapers_answered(&ctx, &app, first, answer());
    assert_eq!(listed(&ctx), 0);
    assert!(
        crate::router::lock(&ctx.shared)
            .setup
            .sources_wait
            .is_some(),
        "the earlier answer leaves the new wait alone"
    );

    // An answer the hold put off is dropped when the form closes first.
    advance(CUE_DELAY_MS);
    assert!(crate::router::lock(&ctx.shared).setup.sources_loading);
    crate::media_setup::scrapers_answered(&ctx, &app, second, answer());
    assert_eq!(listed(&ctx), 0);
    crate::media_setup::close(&ctx, &app);
    advance(CUE_HOLD_MS);
    assert_eq!(listed(&ctx), 0);

    crate::router::open_scrape_setup(&ctx, &app, Scope::All);
    let third = seq(&ctx);
    crate::media_setup::scrapers_answered(&ctx, &app, third, answer());
    assert_eq!(listed(&ctx), 1);
}

#[test]
fn a_hidden_game_shown_among_the_rest_is_marked_hidden() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, _window) = boot();
    let (_runtime, ctx) = offline_ctx();
    crate::sizing::apply_scene(
        &app,
        crate::sizing::Scene::of(&app, f64::from(W), f64::from(H), false),
    );
    app.global::<Shell>().set_active_screen(Screen::Games);
    seat_folder(&ctx, &app, "Game", 0);
    let view = app.global::<crate::GamesView>();
    assert!(!view.get_info().hidden);

    crate::router::lock(&ctx.shared).games.rows[0].is_hidden = true;
    crate::games::render(&ctx, &app);
    assert!(view.get_info().hidden, "the focused game says so");
    assert!(
        view.get_cells().row_data(0).is_some_and(|cell| cell.hidden),
        "and so does its tile"
    );

    // Its neighbor is not hidden.
    crate::games::handle_action(&ctx, &app, "right");
    assert!(!view.get_info().hidden);
}

#[test]
fn settings_root_grid_ignores_the_browse_grid_insets() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    let (_runtime, ctx) = offline_ctx();
    crate::sizing::apply_scene(
        &app,
        crate::sizing::Scene::of(&app, f64::from(W), f64::from(H), false),
    );
    app.global::<Shell>().set_active_screen(Screen::Settings);
    crate::settings::open_page(&ctx, &app, SettingsPage::Root);
    settle(&window);
    let solved = pixels(&window);

    // A list browse profile never pushes these, so they hold whatever a
    // grid profile last left or the `.slint` defaults. The root grid is
    // solved with the Hub's insets and must be drawn with them too.
    let layout = app.global::<crate::Layout>();
    layout.set_grid_left_inset(40.0);
    layout.set_grid_top_inset(40.0);
    layout.set_grid_column_gap(40.0);
    layout.set_grid_row_gap(40.0);
    settle(&window);
    assert!(
        pixels(&window) == solved,
        "the Settings tiles moved with the browse grid's insets"
    );
}

#[test]
fn layout_grid_and_list_tables_both_follow_the_scene_in_either_browse_layout() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, _window) = boot();
    let shell = app.global::<Shell>();
    let layout = app.global::<crate::Layout>();
    // One value from each table a view outside that layout reads: the
    // grid insets, the grid footer's cue slot, and the list card margin.
    let tables = || {
        (
            layout.get_grid_left_inset(),
            layout.get_bottom_status_right_margin(),
            layout.get_card_side_margin(),
        )
    };
    for screen in [Screen::Games, Screen::Systems, Screen::Search] {
        shell.set_active_screen(screen);
        for list in [false, true] {
            shell.set_browse_list_layout(list);
            shell.set_systems_list_layout(list);
            let at = |width: f64, height: f64| {
                crate::sizing::apply_scene(
                    &app,
                    crate::sizing::Scene::of(&app, width, height, false),
                );
                tables()
            };
            let (small, large) = (at(960.0, 540.0), at(1920.0, 1080.0));
            assert!(
                small.0 < large.0 && small.1 < large.1 && small.2 < large.2,
                "{screen:?} list={list}: a table kept another scene's values: {small:?} then {large:?}"
            );
        }
    }
}

/// The columns and rows every pixel of `color` spans: left, right, top
/// and bottom, inclusive.
fn extent(pixels: &[Rgb565Pixel], color: Rgb565Pixel) -> Option<(i32, i32, i32, i32)> {
    let mut found: Option<(i32, i32, i32, i32)> = None;
    for (index, _) in pixels.iter().enumerate().filter(|(_, p)| **p == color) {
        let (x, y) = ((index % W as usize) as i32, (index / W as usize) as i32);
        found = Some(found.map_or((x, x, y, y), |(x0, x1, y0, y1)| {
            (x0.min(x), x1.max(x), y0.min(y), y1.max(y))
        }));
    }
    found
}

/// Where the software renderer draws the focus zoom, the focused cell grows
/// by its real geometry about its center, on the cell that survives a focus
/// move, and it does so in one cut each way: there is no size in between.
/// Where a transform draws it, the cell's geometry never changes.
#[test]
#[allow(
    clippy::too_many_lines,
    reason = "one scenario reads the same marked tile at rest, arriving, grown and leaving"
)]
fn focus_zoom_by_size_grows_the_focused_cell_about_its_center_and_returns() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    let (_runtime, ctx) = offline_ctx();
    seed_hub_pages(&ctx, &app);
    let hub = app.global::<HubView>();
    let motion = app.global::<crate::Motion>();
    let theme = app.global::<crate::Theme>();
    // One tile carries a front edge in a color nothing else paints: every
    // other tile is hidden, which mutes its edge. The edge runs the width
    // of the card along its bottom, so it reads the cell's left, right and
    // bottom straight off the frame; the accent ring reads its top.
    let edge = slint::Color::from_rgb_u8(255, 0, 255);
    let ring = slint::Color::from_rgb_u8(0, 255, 0);
    theme.set_tile_edge(edge);
    theme.set_accent(ring);
    let (edge, ring) = (Rgb565Pixel(0xF81F), Rgb565Pixel(0x07E0));
    let count = hub.get_cells().row_count();
    assert!(count >= 2, "the fixture page holds at least two tiles");
    hub.set_cells(ModelRc::new(VecModel::from(
        (0..count)
            .map(|i| GridCell {
                name: SharedString::from(format!("Tile {i}")),
                hidden: i != 0,
                ..GridCell::default()
            })
            .collect::<Vec<_>>(),
    )));
    // This 180-line scene is in the 240p tier, which has no focus zoom:
    // the scene pushed 100%, and with it nothing grows.
    assert!(app.global::<Sizing>().get_tier_240());
    assert!((motion.get_focus_zoom() - 100.0).abs() < f32::EPSILON);
    motion.set_zoom_by_size(true);
    hub.set_selected_local(1);
    let unfocused = extent(
        &{
            distinct_frames(&window, SETTLE_TICKS);
            pixels(&window)
        },
        edge,
    );
    hub.set_selected_local(0);
    let focused = extent(
        &{
            distinct_frames(&window, SETTLE_TICKS);
            pixels(&window)
        },
        edge,
    );
    assert!(
        unfocused.is_some() && focused == unfocused,
        "no growth at 240p"
    );
    // The mechanism itself is read at a growth this frame can show: large
    // enough to read off 180 lines, small enough that the grown tile stays
    // inside the gap to its neighbors.
    motion.set_focus_zoom(110.0);
    let (grow_x, grow_y) = (hub.get_cell_width() * 0.05, hub.get_cell_height() * 0.05);
    assert!(
        grow_x >= 2.0 && grow_y >= 2.0,
        "the growth must be readable: {grow_x} x {grow_y}"
    );
    let near = |moved: i32, expected: f32| (moved as f32 - expected).abs() <= 1.0;
    let step = |ticks: u32| {
        distinct_frames(&window, ticks);
        pixels(&window)
    };
    let marked = |frame: &[Rgb565Pixel]| extent(frame, edge);

    for by_size in [true, false] {
        motion.set_zoom_by_size(by_size);
        // Focus on the neighbor: the marked tile is at rest.
        hub.set_selected_local(1);
        let rest_frame = step(SETTLE_TICKS);
        let rest = marked(&rest_frame);
        assert!(rest.is_some(), "the marked edge is on screen");
        let Some(rest) = rest else { return };
        let ring_elsewhere = extent(&rest_frame, ring);

        // Focus arrives.
        hub.set_selected_local(0);
        let growing = marked(&step(3));
        let grown_frame = step(SETTLE_TICKS);
        let grown = marked(&grown_frame);
        assert!(growing.is_some() && grown.is_some());
        let (Some(growing), Some(grown)) = (growing, grown) else {
            return;
        };
        let ring_here = extent(&grown_frame, ring);
        assert!(ring_here.is_some() && ring_here != ring_elsewhere);
        let Some(ring_here) = ring_here else { return };

        // Focus leaves again.
        hub.set_selected_local(1);
        let shrinking = marked(&step(3));
        let back = marked(&step(SETTLE_TICKS));

        if by_size {
            // Grown by the same share on every side: left and right off
            // the edge, bottom off the edge, top off the ring.
            assert!(
                near(rest.0 - grown.0, grow_x) && near(grown.1 - rest.1, grow_x),
                "width grows about the center: {rest:?} -> {grown:?}, {grow_x} a side"
            );
            assert!(
                near(grown.3 - rest.3, grow_y),
                "the bottom moves down: {rest:?} -> {grown:?}, {grow_y}"
            );
            assert!(
                ring_here.2 < rest.2 && near(rest.0 - ring_here.0, grow_x),
                "the ring sits on the grown card: {ring_here:?} around {grown:?}"
            );
            // Both directions cut: three frames in, the cell is already
            // at the size it settles at.
            assert_eq!(growing, grown, "the growth is a cut");
            assert_eq!(shrinking, Some(rest), "and so is the return");
        } else {
            // The software renderer ignores the transform, and the
            // geometry it scales is the cell's own, untouched.
            assert_eq!(growing, rest, "no size change while focus arrives");
            assert_eq!(grown, rest, "no size change while focused");
            assert_eq!(shrinking, Some(rest), "no size change while focus leaves");
        }
        assert_eq!(back, Some(rest), "the cell returns to its own rectangle");
    }
}

/// Render one complete frame the way the `MiSTer` presenters do: the window
/// keeps the scene's own shape and the renderer turns it into the frame.
fn render_turned(
    window: &Rc<MinimalSoftwareWindow>,
    rotation: slint::platform::software_renderer::RenderingRotation,
    (width, height): (u32, u32),
) -> Vec<Rgb565Pixel> {
    window.request_redraw();
    let mut frame = vec![Rgb565Pixel(0); (width * height) as usize];
    let drew = window.draw_if_needed(|renderer| {
        renderer.set_rendering_rotation(rotation);
        renderer.render(&mut frame, width as usize);
    });
    assert!(drew, "a requested redraw must produce a frame");
    frame
}

/// A cached page slide moves pixels between two frames Slint rendered once
/// each. It is right when every step shows exactly what Slint draws for the
/// live strip at the same offset, so that is what this compares, pixel for
/// pixel over the whole frame: on HDMI and on the inset CRT scene, unturned
/// and in both TATE orientations, where the band and its direction of
/// travel have to be mapped into the turned frame.
#[cfg(feature = "mister")]
#[test]
#[allow(
    clippy::expect_used,
    clippy::too_many_lines,
    reason = "one matrix walks every frame shape, orientation and direction through the same comparison"
)]
fn cached_slides_match_the_live_strip_in_every_orientation_and_on_crt() {
    use crate::frame_transition::{Active, Spec};
    use crate::Orientation;
    use slint::platform::software_renderer::RenderingRotation;

    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    let (_runtime, ctx) = offline_ctx();
    seed_hub_pages(&ctx, &app);
    let view = app.global::<HubView>();
    let sizing = app.global::<Sizing>();
    let mut state = zaparoo_core::persist::PersistedState::default();
    crate::mister::with_cached_page_transitions(|| {
        for (frame, crt) in [((W, H), false), ((352, 240), true), ((352, 288), true)] {
            for (orientation, rotation) in [
                (Orientation::Horizontal, RenderingRotation::NoRotation),
                (Orientation::Cw, RenderingRotation::Rotate90),
                (Orientation::Ccw, RenderingRotation::Rotate270),
            ] {
                let case = format!("{}x{} crt={crt} {orientation:?}", frame.0, frame.1);
                state.settings.orientation = orientation.token().into();
                crate::seed_display_globals(
                    &app,
                    &state,
                    crt,
                    crt,
                    frame,
                    zaparoo_app::motion_test::Disabled::default(),
                    true,
                );
                let turned = orientation != Orientation::Horizontal;
                window.set_size(if turned {
                    slint::PhysicalSize::new(frame.1, frame.0)
                } else {
                    slint::PhysicalSize::new(frame.0, frame.1)
                });
                let (scene_w, scene_h) =
                    crate::scene_size(f64::from(frame.0), f64::from(frame.1), orientation, crt);
                sizing.set_screen_width(scene_w as f32);
                sizing.set_screen_height(scene_h as f32);
                crate::sizing::apply_scene(
                    &app,
                    crate::sizing::Scene::of(&app, scene_w, scene_h, crt),
                );
                crate::hub::rebuild(&ctx, &app);
                slint::platform::update_timers_and_animations();

                // Two pages that differ, neither symmetric on either axis.
                let outgoing = view.get_cells();
                let incoming: ModelRc<GridCell> =
                    ModelRc::new(VecModel::from(outgoing.iter().take(2).collect::<Vec<_>>()));
                // The band production requests: the grid's, short of the
                // CRT footer label, which is also the live strip's height.
                let (grid_y, grid_height) = crate::page_motion::cached_band(&view, &app);
                let still = |cells: &ModelRc<GridCell>| {
                    view.set_selected_local(-1);
                    view.set_held_local(-1);
                    view.set_slide_anim(false);
                    view.set_page_slide(0.0);
                    view.set_next_cells(ModelRc::default());
                    view.set_cells(cells.clone());
                    slint::platform::update_timers_and_animations();
                    render_turned(&window, rotation, frame)
                };
                for direction in [1, -1] {
                    let source = still(&outgoing);
                    assert!(
                        crate::mister::request_browse_page_transition(
                            &app,
                            grid_y,
                            grid_height,
                            direction
                        ),
                        "{case}: the slide is granted"
                    );
                    let spec = crate::mister::take_page_transition_request()
                        .expect("a granted request is pending");
                    crate::mister::cancel_page_transition();
                    let destination = still(&incoming);
                    assert_ne!(source, destination, "{case}: the pages differ");

                    let mut cached = Active::new(
                        &source,
                        frame.0 as usize,
                        frame.1 as usize,
                        Spec {
                            total_frames: 4,
                            ..spec
                        },
                        Rgb565Pixel(0),
                    )
                    .expect("the mapped band fits the frame");
                    for step in 1..4 {
                        let offset = cached.offset_at(step);
                        let mut composed = source.clone();
                        let result = cached.compose_step(&destination, &mut composed, step);
                        assert!(result.is_ok(), "{case}: step {step} composes");
                        assert!(composed != source && composed != destination);

                        // The live strip, held at the same offset.
                        view.set_cells(outgoing.clone());
                        view.set_next_cells(incoming.clone());
                        view.set_slide_dir(direction);
                        view.set_page_slide(direction as f32 * offset as f32 / grid_height);
                        slint::platform::update_timers_and_animations();
                        let live = render_turned(&window, rotation, frame);
                        let mismatches = composed
                            .iter()
                            .zip(&live)
                            .filter(|(cached, live)| cached != live)
                            .count();
                        // CRT scenes with Sizing.snap() rounding can shift
                        // tile-corner anti-aliasing by one sample relative
                        // to the compositor's integer strip blit, producing
                        // a handful of differing pixels at AA boundaries.
                        let tolerance = if crt { 64 } else { 0 };
                        assert!(
                            mismatches <= tolerance,
                            "{case} direction {direction} step {step} offset {offset}: \
                             the cached frame has {mismatches} mismatches (tolerance {tolerance})"
                        );
                    }
                }
            }
        }
    });
}

/// With a presenter that runs cached slides, every paged grid takes one on
/// HDMI, on CRT and in TATE, and the live strip does not move as well. A
/// list layout still refuses, and without a presenter to grant it each
/// screen keeps its own fallback.
#[cfg(feature = "mister")]
#[test]
#[allow(
    clippy::too_many_lines,
    reason = "one platform-owned matrix covers the three paged grids"
)]
fn every_paged_grid_takes_a_cached_slide_on_crt_and_in_tate_but_lists_do_not() {
    use crate::Orientation;
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    let (_runtime, mut ctx) = offline_ctx();
    ctx.is_mister = true;
    seed_hub_pages(&ctx, &app);
    {
        let mut shared = crate::router::lock(&ctx.shared);
        shared.persist.settings.reduce_motion = false;
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
        shared.games.loading = false;
        shared.games.rows = game_rows("Game", 200).into();
        shared.games.grid.set_item_count(200);
    }
    let shell = app.global::<Shell>();
    let sizing = app.global::<Sizing>();
    let hub = app.global::<HubView>();
    let systems = app.global::<SystemsView>();
    let games = app.global::<crate::GamesView>();
    // (cached transition granted, the live strip is moving)
    let turn = |screen: Screen| {
        shell.set_active_screen(screen);
        match screen {
            Screen::Systems => crate::systems::render(&ctx, &app),
            Screen::Games => crate::games::render(&ctx, &app),
            _ => crate::hub::render(&ctx, &app),
        }
        settle(&window);
        crate::router::handle_action(&ctx, &app, "page_next");
        let state = match screen {
            Screen::Systems => (
                systems.get_cached_transition(),
                systems.get_page_slide().abs() > f32::EPSILON,
            ),
            Screen::Games => (
                games.get_cached_transition(),
                games.get_page_slide().abs() > f32::EPSILON,
            ),
            _ => (
                hub.get_cached_transition(),
                hub.get_page_slide().abs() > f32::EPSILON,
            ),
        };
        // No presenter runs here to show the last step and free the channel.
        crate::mister::cancel_page_transition();
        settle(&window);
        state
    };

    crate::mister::with_cached_page_transitions(|| {
        for crt in [false, true] {
            sizing.set_crt(crt);
            for orientation in [Orientation::Horizontal, Orientation::Cw, Orientation::Ccw] {
                shell.set_orientation(orientation);
                for screen in [Screen::Hub, Screen::Systems, Screen::Games] {
                    assert_eq!(
                        turn(screen),
                        (true, false),
                        "{screen:?} crt={crt} {orientation:?}: cached pixels move, not the Slint grid"
                    );
                }
            }
        }
        sizing.set_crt(false);
        shell.set_orientation(Orientation::Horizontal);
        shell.set_systems_list_layout(true);
        shell.set_browse_list_layout(true);
        for screen in [Screen::Systems, Screen::Games] {
            assert!(!turn(screen).0, "{screen:?}: a list layout has no band");
        }
        shell.set_systems_list_layout(false);
        shell.set_browse_list_layout(false);
    });

    // No presenter grants a slide: the Hub cuts on HDMI and slides its live
    // strip on CRT, and the browse grids slide theirs on either.
    assert_eq!(turn(Screen::Hub), (false, false));
    assert_eq!(turn(Screen::Systems), (false, true));
    assert_eq!(turn(Screen::Games), (false, true));
    sizing.set_crt(true);
    assert_eq!(turn(Screen::Hub), (false, true));
    assert_eq!(turn(Screen::Systems), (false, true));
    assert_eq!(turn(Screen::Games), (false, true));
}

/// Where the software renderer draws, a tile's glyph and a prepared logo
/// are painted at their own pixel size, never fitted: the renderer samples
/// bitmaps nearest-neighbor, so any other size drops or repeats rows and
/// columns. Solid stand-in bitmaps make the painted rectangle readable off
/// the frame: it must be exactly the bitmap, for a tile at rest and for the
/// focused, grown one, on the CRT scene and on HDMI.
#[test]
#[allow(
    clippy::too_many_lines,
    reason = "one scenario reads glyphs and logos, resting and focused, off the same frames"
)]
fn tile_glyphs_and_logos_are_painted_at_their_own_pixel_size() {
    use slint::platform::software_renderer::RenderingRotation;
    use zaparoo_app::sizing::{tile_art_pixels, TileArt};

    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    let (_runtime, ctx) = offline_ctx();
    seed_hub_pages(&ctx, &app);
    let hub = app.global::<HubView>();
    let motion = app.global::<crate::Motion>();
    let art = TileArt {
        compact_padding: true,
        caption: false,
    };
    // Every glyph request, and a solid square of the size asked for.
    let asked: Rc<RefCell<Vec<(String, f32)>>> = Rc::default();
    let solid = |width: u32, height: u32, rgb: [u8; 3]| {
        let mut buffer = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::new(width, height);
        for pixel in buffer.make_mut_slice() {
            *pixel = slint::Rgba8Pixel::new(rgb[0], rgb[1], rgb[2], 255);
        }
        slint::Image::from_rgba8(buffer)
    };
    {
        let asked = asked.clone();
        app.global::<crate::GlyphSource>()
            .on_raster(move |key, px, _, _| {
                asked.borrow_mut().push((key.to_string(), px));
                let side = px.round().max(0.0) as u32;
                // Chrome asks for glyphs too; only the two tiles paint.
                match key.as_str() {
                    "focused" => solid(side, side, [255, 0, 255]),
                    "resting" => solid(side, side, [255, 0, 0]),
                    _ => slint::Image::default(),
                }
            });
    }
    let (magenta, red, cyan, yellow) = (
        Rgb565Pixel(0xF81F),
        Rgb565Pixel(0xF800),
        Rgb565Pixel(0x07FF),
        Rgb565Pixel(0xFFE0),
    );
    let mut state = zaparoo_core::persist::PersistedState::default();
    state.settings.orientation = crate::Orientation::Horizontal.token().into();

    for (frame, crt) in [((352_u32, 240_u32), true), ((1280, 720), false)] {
        let case = format!("{}x{} crt={crt}", frame.0, frame.1);
        crate::seed_display_globals(
            &app,
            &state,
            crt,
            crt,
            frame,
            zaparoo_app::motion_test::Disabled::default(),
            true,
        );
        // The software-rendered builds set this; the probe build may not.
        motion.set_zoom_by_size(true);
        window.set_size(slint::PhysicalSize::new(frame.0, frame.1));
        let (scene_w, scene_h) = crate::scene_size(
            f64::from(frame.0),
            f64::from(frame.1),
            crate::Orientation::Horizontal,
            crt,
        );
        let sizing = app.global::<Sizing>();
        sizing.set_screen_width(scene_w as f32);
        sizing.set_screen_height(scene_h as f32);
        let scene = crate::sizing::Scene::of(&app, scene_w, scene_h, crt);
        crate::sizing::apply_scene(&app, scene);
        crate::hub::rebuild(&ctx, &app);
        let inputs = scene.inputs();
        let (cell_w, cell_h) = (
            hub.get_cell_width().round() as i32,
            hub.get_cell_height().round() as i32,
        );
        // The 240p tier has no focus zoom; every larger one does.
        let zoom = f64::from(motion.get_focus_zoom()) / 100.0;
        let rest = tile_art_pixels(&inputs, cell_w, cell_h, 1.0, art);
        let grown = tile_art_pixels(&inputs, cell_w, cell_h, zoom, art);
        if crt {
            assert!(sizing.get_tier_240(), "{case}: the 240p tier");
            assert!((zoom - 1.0).abs() < f64::EPSILON, "{case}: no focus zoom");
            assert_eq!(grown, rest, "{case}: the focused tile keeps its art box");
            assert_eq!(crate::system_logos::focus_zoom(&app), None);
        } else {
            assert!(
                grown.0 > rest.0 && grown.1 > rest.1,
                "{case}: the grown tile has a larger art box: {rest:?} {grown:?}"
            );
            assert_eq!(crate::system_logos::focus_zoom(&app), Some(zoom));
        }

        // The painted rectangle of one color: left, top, width, height,
        // and how many pixels of it there are.
        let painted = |pixels: &[Rgb565Pixel], color: Rgb565Pixel| {
            let mut found: Option<(u32, u32, u32, u32)> = None;
            let mut count = 0_u32;
            for (index, _) in pixels.iter().enumerate().filter(|(_, p)| **p == color) {
                let (x, y) = (index as u32 % frame.0, index as u32 / frame.0);
                count += 1;
                found = Some(found.map_or((x, y, x, y), |(x0, y0, x1, y1)| {
                    (x0.min(x), y0.min(y), x1.max(x), y1.max(y))
                }));
            }
            found.map(|(x0, y0, x1, y1)| (x1 - x0 + 1, y1 - y0 + 1, count))
        };
        let show = |selected: i32| {
            hub.set_selected_local(selected);
            for _ in 0..SETTLE_TICKS {
                CLOCK.with(|clock| clock.set(clock.get() + TICK_MS));
                slint::platform::update_timers_and_animations();
            }
            render_turned(&window, RenderingRotation::NoRotation, frame)
        };
        // A logo keeps its own shape inside the bounds it was prepared to.
        let logo = |bounds: (u32, u32), rgb| solid(bounds.0, (bounds.1 / 2).max(1), rgb);
        let logo_size = |bounds: (u32, u32)| (bounds.0, (bounds.1 / 2).max(1));
        hub.set_focus_art_local(-1);
        hub.set_next_cells(ModelRc::default());
        hub.set_cells(ModelRc::new(VecModel::from(vec![
            GridCell {
                glyph_key: "focused".into(),
                ..GridCell::default()
            },
            GridCell {
                glyph_key: "resting".into(),
                ..GridCell::default()
            },
            GridCell {
                cover: logo(rest, [0, 255, 255]),
                has_cover: true,
                cover_exact: true,
                ..GridCell::default()
            },
        ])));

        // Glyphs: the resting tile and the focused, grown one.
        asked.borrow_mut().clear();
        let pixels = show(0);
        let side = |bounds: (u32, u32)| bounds.0.min(bounds.1);
        assert_eq!(
            painted(&pixels, red),
            Some((side(rest), side(rest), side(rest) * side(rest))),
            "{case}: a resting glyph is painted at its raster's size"
        );
        assert_eq!(
            painted(&pixels, magenta),
            Some((side(grown), side(grown), side(grown) * side(grown))),
            "{case}: the focused glyph is painted at the grown tile's raster size"
        );
        // The settled frame asked for whole-pixel rasters of those sizes.
        let last = |key: &str| {
            asked
                .borrow()
                .iter()
                .rev()
                .find(|(asked_key, _)| asked_key == key)
                .map(|(_, px)| *px)
        };
        assert_eq!(last("resting"), Some(side(rest) as f32), "{case}");
        assert_eq!(last("focused"), Some(side(grown) as f32), "{case}");
        // A resting logo is painted at its own size.
        let (w, h) = logo_size(rest);
        assert_eq!(painted(&pixels, cyan), Some((w, h, w * h)), "{case}");

        // The focused logo before its larger copy lands: the resting copy,
        // still at its own size, not stretched into the grown box.
        let pixels = show(2);
        assert_eq!(
            painted(&pixels, cyan),
            Some((w, h, w * h)),
            "{case}: no larger copy yet"
        );
        // Without a zoom there is no larger copy to ask for.
        if crt {
            continue;
        }
        // And once it has landed: the larger copy, at its own size.
        hub.set_focus_art(logo(grown, [255, 255, 0]));
        hub.set_focus_art_local(2);
        let pixels = show(2);
        let (w, h) = logo_size(grown);
        assert_eq!(
            painted(&pixels, yellow),
            Some((w, h, w * h)),
            "{case}: the focused logo is painted at the grown tile's copy size"
        );
        assert_eq!(painted(&pixels, cyan), None, "{case}");
        // It belongs to that tile only.
        let pixels = show(1);
        assert_eq!(painted(&pixels, yellow), None, "{case}");

        // A bitmap larger than the box is fitted into it, never past it.
        hub.set_focus_art(solid(grown.0 + 9, grown.1 + 9, [255, 255, 0]));
        let pixels = show(2);
        let fitted = painted(&pixels, yellow);
        assert!(
            fitted.is_some_and(|(w, h, _)| w <= grown.0 + 1 && h <= grown.1 + 1),
            "{case}: an oversize bitmap stays inside the art box: {fitted:?}"
        );
    }

    // Where a transform draws the zoom, art is fitted to its box as before.
    motion.set_zoom_by_size(false);
    hub.set_focus_art_local(-1);
    hub.set_selected_local(1);
    for _ in 0..SETTLE_TICKS {
        CLOCK.with(|clock| clock.set(clock.get() + TICK_MS));
        slint::platform::update_timers_and_animations();
    }
    let asked_before = asked.borrow().len();
    let _ = render_turned(&window, RenderingRotation::NoRotation, (1280, 720));
    assert!(asked.borrow().len() >= asked_before);
}

/// A cold start that resumes inside a system fills the Systems grid under
/// the Games screen, before any logo is prepared and without the grid ever
/// being on screen to ask for them. Backing out to it must show its logos
/// without another key press.
#[test]
fn a_restored_game_list_backs_out_to_a_systems_grid_with_its_logos() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    let (_runtime, ctx) = offline_ctx();
    crate::sizing::apply_scene(
        &app,
        crate::sizing::Scene::of(&app, f64::from(W), f64::from(H), false),
    );
    let ids = ["SNES", "NES", "Genesis", "PSX"];
    {
        let mut shared = crate::router::lock(&ctx.shared);
        shared.persist.settings.system_logo_style = "color".into();
        shared.categories = vec!["Console".into()];
        shared.systems = ids
            .iter()
            .map(|id| zaparoo_core::media_types::SystemInfo {
                id: (*id).into(),
                name: (*id).into(),
                category: "Console".into(),
                media_count: Some(40),
                ..Default::default()
            })
            .collect();
        shared.persist.systems.system_id = "NES".into();
        shared.games.loading = false;
        shared.games.rows = game_rows("Game", 40).into();
        shared.games.grid.set_item_count(40);
    }
    // The restore: the parent is filled unseen, then Games is the screen.
    crate::systems::prepare_parent(&ctx, &app, "Console");
    let shell = app.global::<Shell>();
    shell.set_active_screen(Screen::Games);
    crate::games::render(&ctx, &app);
    let view = app.global::<SystemsView>();
    assert_eq!(view.get_cells().row_count(), ids.len());
    assert!(
        view.get_cells().iter().all(|cell| !cell.has_cover),
        "nothing was prepared when the grid was filled"
    );
    // The logo worker runs in the background while the user plays or
    // browses; no Systems screen is up to be repainted by it.
    settle(&window);
    ctx.logos.prepare_queued();
    settle(&window);

    crate::router::handle_action(&ctx, &app, "cancel");
    assert_eq!(shell.get_active_screen(), Screen::Systems);
    ctx.logos.prepare_queued();
    settle(&window);
    let blank: Vec<_> = view
        .get_cells()
        .iter()
        .filter(|cell| !cell.has_cover)
        .map(|cell| cell.name)
        .collect();
    assert!(blank.is_empty(), "tiles without their logo: {blank:?}");
}

/// The browse list follows the selection with a glide: one row when the
/// selection steps past the window's edge, one page on a page jump. A held
/// key still cuts, and so does everything with motion off.
#[test]
#[allow(
    clippy::too_many_lines,
    reason = "one list walks taps, page jumps, held repeats and reduced motion"
)]
fn the_browse_list_glides_a_row_or_a_page_and_cuts_on_a_held_key() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    let (_runtime, ctx) = offline_ctx();
    crate::sizing::apply_scene(
        &app,
        crate::sizing::Scene::of(&app, f64::from(W), f64::from(H), false),
    );
    let shell = app.global::<Shell>();
    shell.set_systems_list_layout(true);
    shell.set_active_screen(Screen::Systems);
    {
        let mut shared = crate::router::lock(&ctx.shared);
        shared.persist.settings.systems_browse_layout = "list".into();
        shared.persist.settings.reduce_motion = false;
        shared.systems_model.rows = (0..120)
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
        shared.systems_model.grid.set_item_count(120);
    }
    crate::systems::render(&ctx, &app);
    settle(&window);
    let view = app.global::<SystemsView>();
    let visible = view.get_list_visible();
    assert!(visible >= 2, "the list card holds rows: {visible}");
    let top = || view.get_list_scroll_top();
    let tap = |action: &str| {
        crate::router::handle_action(&ctx, &app, action);
        distinct_frames(&window, 6)
    };

    // Walk to the window's last row: the selection moves, the list does not.
    for _ in 1..visible {
        tap("down");
        settle(&window);
    }
    assert_eq!(top(), 0);
    // One more row and the list follows by one row, as a glide.
    let frames = tap("down");
    assert_eq!(top(), 1);
    assert!(view.get_list_glide());
    assert!(frames > 2, "a single row glides: {frames} frames");
    settle(&window);
    // The rows a page either side of the window are in the model.
    assert!(view.get_list_rows().row_count() >= 2 * visible as usize);

    // A page jump travels a page, as one glide over the page duration.
    let before = top();
    let frames = tap("page_next");
    assert!(top() > before + 1, "the window moved by more than a row");
    assert!(top() - before <= visible, "and by no more than a page");
    assert!(frames > 3, "a page jump glides: {frames} frames");
    settle(&window);
    let frames = tap("page_prev");
    assert!(frames > 3, "a page jump back glides: {frames} frames");
    settle(&window);

    // An ordinary held walk glides row by row, as the Settings cards do.
    // Repeats arrive faster than a glide lasts, so each one retargets the
    // glide in flight and the list never stops between them.
    for _ in 0..visible {
        tap("down");
        settle(&window);
    }
    let before = top();
    let mut moving = 0;
    for _ in 0..4 {
        crate::input::dispatch_repeat(&ctx, &app, "down", HoldTier::Row);
        assert!(view.get_list_glide(), "an ordinary held walk glides");
        // Main repeats a held direction every 50 ms: three frames here.
        moving += distinct_frames(&window, 3);
    }
    assert_eq!(top(), before + 4);
    assert!(
        moving >= 10,
        "the list moves through the whole walk: {moving} of 12 frames"
    );
    assert!(
        distinct_frames(&window, 6) > 2,
        "and glides to rest after it"
    );
    settle(&window);

    // A row step glides at every tier of a hold, at the tier's own pace.
    let before = top();
    crate::input::dispatch_repeat(&ctx, &app, "down", HoldTier::Page);
    assert_eq!(top(), before + 1);
    assert!(view.get_list_glide(), "a row step glides at any tier");
    assert_eq!(
        view.get_list_step_ms(),
        112,
        "a quarter over the 90 ms tick"
    );
    assert!(distinct_frames(&window, 6) > 2);
    settle(&window);

    // Motion off: a tap lands at once too.
    app.global::<crate::Motion>().set_enabled(false);
    let before = top();
    let frames = tap("down");
    assert_eq!(top(), before + 1);
    assert!(frames <= 1, "no glide with motion off: {frames} frames");
    let frames = tap("page_next");
    assert!(
        frames <= 1,
        "no page glide with motion off: {frames} frames"
    );
}

/// The pixels the list scrolled by between two frames: the shift that
/// lines the earlier frame's rows up with the later one's, read off a band
/// of the list's text well inside its card.
#[cfg(feature = "mister")]
fn list_shift(earlier: &[Rgb565Pixel], later: &[Rgb565Pixel], limit: usize) -> usize {
    let (width, height) = (W as usize, H as usize);
    let rows = height / 3..height * 2 / 3 - limit;
    (0..limit)
        .min_by_key(|shift| {
            rows.clone()
                .flat_map(|y| (12..width / 3).map(move |x| (x, y)))
                .filter(|&(x, y)| earlier[(y + shift) * width + x] != later[y * width + x])
                .count()
        })
        .unwrap_or(0)
}

/// Hold Down through the real key path for a second and a half, a frame at
/// a time, with the frontend's own repeat timer setting the pace. While the
/// selection steps one row at a time (`row_steps` says so, frame by frame;
/// once it says no the walk has turned into page steps and is no longer
/// watched) the list must scroll every frame and never by a whole row at
/// once.
#[cfg(feature = "mister")]
fn hold_down_and_watch(
    ctx: &crate::router::Ctx,
    app: &App,
    window: &Rc<MinimalSoftwareWindow>,
    stride: usize,
    row_steps: impl Fn() -> bool,
) {
    use slint::platform::WindowEvent;
    let key: SharedString = char::from(slint::platform::Key::DownArrow)
        .to_string()
        .into();
    app.window()
        .dispatch_event(WindowEvent::KeyPressed { text: key.clone() });
    let mut previous = pixels(window);
    let mut elapsed = 0;
    let mut watching = true;
    while elapsed < 1_500 {
        crate::router::lock(&ctx.shared)
            .input
            .advance_test_clock(TICK_MS);
        advance(TICK_MS);
        elapsed += TICK_MS;
        let frame = pixels(window);
        // From the first repeat's glide to the last row step's.
        watching = watching && row_steps();
        if watching && elapsed > zaparoo_app::input::REPEAT_INITIAL_MS + 2 * TICK_MS {
            let shift = list_shift(&previous, &frame, 2 * stride);
            assert!(shift >= 1, "the list stopped {elapsed} ms into the hold");
            assert!(
                shift < stride,
                "the list jumped {shift} px, a whole row, {elapsed} ms into the hold"
            );
        }
        previous = frame;
    }
    app.window()
        .dispatch_event(WindowEvent::KeyReleased { text: key });
    settle(window);
    settle(window);
}

/// A held Down in the Systems list steps a row at every tier: the list
/// scrolls continuously for the whole hold, past the rapid threshold too.
#[cfg(feature = "mister")]
#[test]
fn a_held_walk_scrolls_the_systems_list_continuously() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    let (_runtime, ctx) = offline_ctx();
    let ctx = std::sync::Arc::new(ctx);
    crate::sizing::apply_scene(
        &app,
        crate::sizing::Scene::of(&app, f64::from(W), f64::from(H), false),
    );
    let shell = app.global::<Shell>();
    shell.set_systems_list_layout(true);
    shell.set_active_screen(Screen::Systems);
    {
        let mut shared = crate::router::lock(&ctx.shared);
        shared.persist.settings.systems_browse_layout = "list".into();
        shared.systems_model.rows = (0..200)
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
        shared.systems_model.grid.set_item_count(200);
        shared.input.advance_test_clock(1);
    }
    crate::systems::render(&ctx, &app);
    crate::input::bind(&ctx, &app, std::collections::HashMap::new());
    settle(&window);
    let view = app.global::<SystemsView>();
    let visible = usize::try_from(view.get_list_visible()).unwrap_or(1);
    // Start on the window's last row, so every step scrolls.
    for _ in 1..visible {
        crate::router::handle_action(&ctx, &app, "down");
    }
    settle(&window);
    let start = crate::router::lock(&ctx.shared)
        .systems_model
        .grid
        .current_index();
    let stride = view.get_list_row_height().round() as usize;
    // The hold clock also counts real time, so a loaded machine can carry
    // this hold to the letter tier, where a row every 250 ms moves less
    // than a pixel a frame; the walk is watched up to there.
    hold_down_and_watch(&ctx, &app, &window, stride, || {
        view.get_list_glide() && view.get_list_step_ms() < 200
    });
    let landed = crate::router::lock(&ctx.shared)
        .systems_model
        .grid
        .current_index();
    // The press and its repeats: eighteen rows or so on an idle machine,
    // and at least the six the slowest tier steps when real time has
    // carried the hold there early.
    assert!(
        landed > start + 4,
        "the hold walked on: {start} to {landed}"
    );
    assert_eq!(
        usize::try_from(view.get_list_scroll_top()).unwrap_or(0),
        landed + 1 - visible,
        "and the list rests with the selection on its last row"
    );
    let rest = pixels(&window);
    advance(TICK_MS);
    assert!(pixels(&window) == rest, "nothing moves after the release");
}

/// A held key in the options menu walks its rows at a steady pace, as a
/// list does, though the modal keeps the hold to one row at a time and off
/// the screen behind it.
#[test]
fn a_held_key_in_the_options_menu_scrolls_steadily() {
    use zaparoo_core::input_actions::actions;
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, _window) = boot();
    let (_runtime, ctx) = offline_ctx();
    app.global::<Shell>().set_active_screen(Screen::Games);
    let overlays = app.global::<crate::Overlays>();
    let entries: Vec<crate::MenuEntry> = (0..20)
        .map(|i| crate::MenuEntry {
            id: format!("row{i}").into(),
            label: format!("Row {i}").into(),
            enabled: true,
            ..Default::default()
        })
        .collect();
    overlays.set_context_entries(ModelRc::new(VecModel::from(entries)));
    overlays.set_context_index(0);
    overlays.set_context_open(true);

    crate::input::dispatch_repeat(&ctx, &app, actions::DOWN, HoldTier::Page);
    assert_eq!(
        overlays.get_context_index(),
        1,
        "a held step in a modal is one row"
    );
    assert!(
        overlays.get_context_step_ms() > 0,
        "a held step glides at constant speed"
    );
    assert!(
        !crate::input::dispatching_repeat(&ctx),
        "the hold never reaches the screen behind"
    );

    crate::router::handle_action(&ctx, &app, actions::DOWN);
    assert_eq!(overlays.get_context_index(), 2);
    assert_eq!(overlays.get_context_step_ms(), 0, "a tap eases");
}

/// The Games list walks row by row the same way until the hold turns into
/// a fast scroll, which steps pages and may cut.
#[cfg(feature = "mister")]
#[test]
fn a_held_walk_scrolls_the_games_list_continuously() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    let (_runtime, ctx) = offline_ctx();
    let ctx = std::sync::Arc::new(ctx);
    {
        let mut shared = crate::router::lock(&ctx.shared);
        shared.persist.settings.games_browse_layout = "list".into();
        shared.persist.games.list_top_at_level = vec![0];
        shared.games.rows = game_rows("Game", 400).into();
        shared.games.grid.set_item_count(400);
        shared.games.focus_armed = true;
        shared.input.advance_test_clock(1);
    }
    crate::sizing::apply_scene(
        &app,
        crate::sizing::Scene::of(&app, f64::from(W), f64::from(H), false),
    );
    app.global::<Shell>().set_active_screen(Screen::Games);
    app.global::<Shell>().set_browse_list_layout(true);
    crate::router::refresh_layout(&app);
    crate::games::render(&ctx, &app);
    crate::input::bind(&ctx, &app, std::collections::HashMap::new());
    settle(&window);
    let view = app.global::<crate::GamesView>();
    let visible = usize::try_from(view.get_list_visible()).unwrap_or(1);
    for _ in 1..visible {
        crate::router::handle_action(&ctx, &app, "down");
    }
    settle(&window);
    let start = crate::router::lock(&ctx.shared).games.grid.current_index();
    let stride = view.get_list_row_height().round() as usize;
    // A page step is the one thing that cuts, and Rust says when.
    hold_down_and_watch(&ctx, &app, &window, stride, || view.get_list_glide());
    assert!(
        !view.get_list_glide(),
        "the hold reached its page steps before it ended"
    );
    let landed = crate::router::lock(&ctx.shared).games.grid.current_index();
    assert!(
        landed > start + 2 * visible,
        "rows, then pages: {start} to {landed}"
    );
    let top = usize::try_from(view.get_list_scroll_top()).unwrap_or(0);
    assert!(
        (top..top + visible).contains(&landed),
        "the list rests with the selection in its window"
    );
    let rest = pixels(&window);
    advance(TICK_MS);
    assert!(pixels(&window) == rest, "nothing moves after the release");
}

/// Let `ms` pass for the timers and for the key path's own clock, which
/// times the duplicate guard.
fn pass(ctx: &crate::router::Ctx, ms: u64) {
    crate::router::lock(&ctx.shared)
        .input
        .advance_test_clock(ms);
    advance(ms);
}

/// Press and release a key through the window, as the keyboard would.
fn key_down(app: &App, key: slint::platform::Key) {
    let text: SharedString = char::from(key).to_string().into();
    app.window()
        .dispatch_event(slint::platform::WindowEvent::KeyPressed { text });
}

fn key_up(app: &App, key: slint::platform::Key) {
    let text: SharedString = char::from(key).to_string().into();
    app.window()
        .dispatch_event(slint::platform::WindowEvent::KeyReleased { text });
}

/// A ready browse folder of 60 games under three letters.
fn lettered_folder(ctx: &crate::router::Ctx, app: &App) {
    let bucket = |label: &str, offset: u32| zaparoo_core::media_types::BrowseIndexGroup {
        key: label.to_lowercase(),
        label: label.into(),
        count: 0,
        cursor: String::new(),
        offset,
    };
    app.global::<Shell>().set_active_screen(Screen::Games);
    {
        let mut shared = crate::router::lock(&ctx.shared);
        shared.games.mode = GamesMode::Browse;
        shared.games.system_id = "NES".into();
        shared.games.browse_path = "/roms/nes".into();
        shared.games.rows = game_rows("Game", 60).into();
        shared.games.grid.set_item_count(60);
        shared.games.grid.set_current_index_immediate(0);
        shared.letter_buckets = vec![bucket("A", 0), bucket("B", 20), bucket("C", 45)];
        shared.letter_scope = Some(("NES".into(), "/roms/nes".into()));
    }
    // The picker's own copy, as the index fetch publishes it.
    let overlays = app.global::<crate::Overlays>();
    overlays.set_letter_buckets(ModelRc::new(VecModel::from(
        ["A", "B", "C"]
            .map(|label| crate::LetterBucket {
                label: label.into(),
                count: 20,
            })
            .to_vec(),
    )));
    overlays.set_letter_state(crate::ContentState::Ready);
    crate::games::render(ctx, app);
}

#[test]
#[allow(clippy::expect_used, reason = "the row has its chips")]
fn the_menu_button_focuses_the_chip_row_above_240p_and_back_returns() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    let (_runtime, ctx) = offline_ctx();
    crate::sizing::apply_scene(&app, crate::sizing::Scene::of(&app, 1280.0, 720.0, false));
    lettered_folder(&ctx, &app);
    settle(&window);
    let bar = app.global::<crate::PageBar>();
    let overlays = app.global::<crate::Overlays>();
    let index = || crate::router::lock(&ctx.shared).games.grid.current_index();
    assert_eq!(
        crate::page_bar::chip_ids(&app),
        ["jump_letter", "filter", "search_here"]
    );
    // Go to keeps one label wherever the selection is.
    let go_to = || bar.get_games_chips().row_data(0).expect("Go to");
    let before = go_to();
    crate::router::lock(&ctx.shared)
        .games
        .grid
        .set_current_index_immediate(25);
    crate::games::render(&ctx, &app);
    assert_eq!(go_to(), before);
    assert_eq!(go_to().label_key.as_str(), "jump_letter");

    // Menu puts focus on the first chip; the list does not open.
    crate::router::handle_action(&ctx, &app, "page_menu");
    assert!(bar.get_focused());
    assert_eq!(bar.get_index(), 0);
    assert!(!overlays.get_list_open());
    // Sideways walks the row and wraps; Up has nowhere to go and the grid
    // under the row stays where it was.
    crate::router::handle_action(&ctx, &app, "left");
    assert_eq!(bar.get_index(), 2);
    crate::router::handle_action(&ctx, &app, "right");
    assert_eq!(bar.get_index(), 0);
    crate::router::handle_action(&ctx, &app, "up");
    crate::router::handle_action(&ctx, &app, "page_next");
    assert!(bar.get_focused());
    assert_eq!(index(), 25);

    // Go to opens on the current letter. Backing out returns to the chip.
    crate::router::handle_action(&ctx, &app, "accept");
    assert!(overlays.get_letter_open());
    assert_eq!(overlays.get_letter_index(), 1);
    assert!(!bar.get_focused());
    crate::router::handle_action(&ctx, &app, "cancel");
    assert!(!overlays.get_letter_open());
    assert!(bar.get_focused(), "backing out returns to the chip");
    assert_eq!(bar.get_index(), 0);
    // A pick returns to the content it changed.
    crate::router::handle_action(&ctx, &app, "accept");
    crate::router::handle_action(&ctx, &app, "right");
    crate::router::handle_action(&ctx, &app, "accept");
    advance(zaparoo_app::input::PRESS_FEEDBACK_MS + 10);
    assert!(!overlays.get_letter_open());
    assert!(!bar.get_focused());
    assert_eq!(index(), 45);

    // Back, Menu and Down all leave the row.
    for leave in ["cancel", "page_menu", "down"] {
        crate::router::handle_action(&ctx, &app, "page_menu");
        assert!(bar.get_focused());
        crate::router::handle_action(&ctx, &app, leave);
        assert!(!bar.get_focused(), "{leave} leaves the row");
        assert_eq!(index(), 45, "{leave} does not move the grid");
    }

    // At 240p there is no row: Menu opens the Menu list.
    crate::sizing::apply_scene(
        &app,
        crate::sizing::Scene::of(&app, f64::from(W), f64::from(H), false),
    );
    crate::router::handle_action(&ctx, &app, "page_menu");
    assert!(!bar.get_focused());
    assert!(overlays.get_list_open());
    assert_eq!(overlays.get_list_title(), "title:menu");
}

#[test]
#[allow(clippy::expect_used, reason = "isolated settings fixture")]
fn turning_the_menu_row_off_gives_back_its_band_and_menu_opens_the_list() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    let (_runtime, mut ctx) = offline_ctx();
    ctx.config_path =
        std::path::PathBuf::from(std::env::var("ZAPAROO_STATE_FILE").expect("isolated state"))
            .with_file_name("frontend.toml");
    seed_hub_tiles(&ctx, &app);
    crate::sizing::apply_scene(&app, crate::sizing::Scene::of(&app, 1280.0, 720.0, false));
    settle(&window);
    let sizing = app.global::<Sizing>();
    assert!(sizing.get_menu_row());
    assert!(sizing.get_menu_row_block() > 0.0);

    // Settings > Appearance > Show Menu row.
    app.global::<Shell>().set_active_screen(Screen::Settings);
    let view = app.global::<crate::SettingsView>();
    view.set_page(SettingsPage::Appearance);
    let rows = zaparoo_app::settings::page_rows(
        SettingsPage::Appearance.token(),
        &crate::settings::inputs(&ctx),
    );
    view.set_index(
        rows.iter()
            .position(|row| row.id() == "showMenuRow")
            .and_then(|i| i32::try_from(i).ok())
            .expect("Show Menu row on Appearance"),
    );
    crate::settings::handle_action(&ctx, &app, "accept");
    assert!(
        !crate::router::lock(&ctx.shared)
            .persist
            .settings
            .show_menu_row
    );
    assert!(!app.global::<crate::Overlays>().get_dialog_open());
    assert!(!sizing.get_menu_row());
    assert!(sizing.get_menu_row_block().abs() < f32::EPSILON);

    // Back on the Hub, Menu opens the Menu list instead of focusing a row.
    crate::router::go_home(&ctx, &app);
    settle(&window);
    assert_eq!(app.global::<Shell>().get_active_screen(), Screen::Hub);
    crate::router::handle_action(&ctx, &app, "page_menu");
    assert!(!app.global::<crate::PageBar>().get_focused());
    let overlays = app.global::<crate::Overlays>();
    assert!(overlays.get_list_open());
    assert_eq!(overlays.get_list_title(), "title:menu");
}

#[test]
#[allow(clippy::expect_used, reason = "isolated settings fixture")]
fn hiding_the_help_bar_gives_its_height_to_the_screens() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    let (_runtime, mut ctx) = offline_ctx();
    ctx.config_path =
        std::path::PathBuf::from(std::env::var("ZAPAROO_STATE_FILE").expect("isolated state"))
            .with_file_name("frontend.toml");
    seed_hub_tiles(&ctx, &app);
    crate::sizing::apply_scene(&app, crate::sizing::Scene::of(&app, 316.0, 216.0, true));
    settle(&window);
    let sizing = app.global::<Sizing>();
    let shown_bar = sizing.get_help_bar_height();
    assert!(shown_bar > 0.0);

    // Settings > Appearance > Show button hints.
    app.global::<Shell>().set_active_screen(Screen::Settings);
    let view = app.global::<crate::SettingsView>();
    view.set_page(SettingsPage::Appearance);
    let rows = zaparoo_app::settings::page_rows(
        SettingsPage::Appearance.token(),
        &crate::settings::inputs(&ctx),
    );
    view.set_index(
        rows.iter()
            .position(|row| row.id() == "showHelpBar")
            .and_then(|i| i32::try_from(i).ok())
            .expect("Show button hints on Appearance"),
    );
    crate::settings::handle_action(&ctx, &app, "accept");
    assert!(
        !crate::router::lock(&ctx.shared)
            .persist
            .settings
            .show_help_bar
    );
    assert!(!sizing.get_help_bar_enabled());
    assert!(sizing.get_help_bar_height().abs() < f32::EPSILON);
    assert!(sizing.get_help_bar_band().abs() < f32::EPSILON);

    // And back: the bar takes a height again (sized for the scene the
    // toggle re-solves, which is the window's).
    crate::settings::handle_action(&ctx, &app, "accept");
    assert!(sizing.get_help_bar_enabled());
    assert!(sizing.get_help_bar_height() > 0.0);
}

#[test]
fn search_here_sits_on_the_systems_screens_and_runs_from_menu_at_240p() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    let (_runtime, ctx) = offline_ctx();
    let shell = app.global::<Shell>();
    let overlays = app.global::<crate::Overlays>();
    crate::router::lock(&ctx.shared).systems = navigation_catalog();
    crate::sizing::apply_scene(&app, crate::sizing::Scene::of(&app, 1280.0, 720.0, false));
    crate::systems::enter(&ctx, &app, "Console", EntryMode::Fresh, false);
    settle(&window);
    assert_eq!(shell.get_active_screen(), Screen::Systems);
    assert_eq!(crate::page_bar::chip_ids(&app), ["search_here"]);
    crate::router::handle_action(&ctx, &app, "page_menu");
    assert!(app.global::<crate::PageBar>().get_focused());
    crate::router::handle_action(&ctx, &app, "accept");
    assert_eq!(shell.get_active_screen(), Screen::Search);
    assert_eq!(
        app.global::<crate::SearchView>().get_scope(),
        crate::SearchScope::Category
    );
    crate::router::handle_action(&ctx, &app, "cancel");
    assert_eq!(shell.get_active_screen(), Screen::Systems);

    // At 240p a category's one page action runs straight from Menu.
    crate::sizing::apply_scene(
        &app,
        crate::sizing::Scene::of(&app, f64::from(W), f64::from(H), false),
    );
    crate::router::handle_action(&ctx, &app, "page_menu");
    assert!(!overlays.get_list_open());
    assert_eq!(shell.get_active_screen(), Screen::Search);
    crate::router::handle_action(&ctx, &app, "cancel");
    assert_eq!(shell.get_active_screen(), Screen::Systems);

    // The favorite systems' Menu list carries the grouping and the search,
    // even on an empty list.
    crate::router::lock(&ctx.shared).systems_model.mode = SystemsMode::Favorites;
    let systems = app.global::<SystemsView>();
    systems.set_loading(false);
    systems.set_error("".into());
    systems.set_count(0);
    shell.set_active_screen(Screen::FavoriteSystems);
    crate::router::handle_action(&ctx, &app, "page_menu");
    assert!(overlays.get_list_open());
    let rows: Vec<String> = overlays
        .get_list_entries()
        .iter()
        .map(|row| row.id.to_string())
        .collect();
    assert_eq!(rows, ["favorites_grouping", "search_here"]);
    crate::router::handle_action(&ctx, &app, "down");
    crate::router::handle_action(&ctx, &app, "accept");
    advance(zaparoo_app::input::PRESS_FEEDBACK_MS + 10);
    assert_eq!(shell.get_active_screen(), Screen::Search);
    assert_eq!(
        app.global::<crate::SearchView>().get_scope(),
        crate::SearchScope::List
    );
}

#[test]
fn the_hubs_more_chip_holds_quit_and_backing_out_returns_to_it() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    let (_runtime, ctx) = offline_ctx();
    seed_hub_tiles(&ctx, &app);
    crate::sizing::apply_scene(&app, crate::sizing::Scene::of(&app, 1280.0, 720.0, false));
    settle(&window);
    assert_eq!(app.global::<Shell>().get_active_screen(), Screen::Hub);
    assert_eq!(crate::page_bar::chip_ids(&app), ["hub_add", "more"]);
    let bar = app.global::<crate::PageBar>();
    let overlays = app.global::<crate::Overlays>();
    crate::router::handle_action(&ctx, &app, "page_menu");
    crate::router::handle_action(&ctx, &app, "right");
    crate::router::handle_action(&ctx, &app, "accept");
    assert!(overlays.get_list_open());
    assert_eq!(overlays.get_list_title(), "title:more");
    let rows: Vec<String> = overlays
        .get_list_entries()
        .iter()
        .map(|row| row.id.to_string())
        .collect();
    assert_eq!(rows, ["hub_reset", "hub_settings", "hub_quit"]);
    crate::router::handle_action(&ctx, &app, "cancel");
    assert!(!overlays.get_list_open());
    assert!(bar.get_focused());
    assert_eq!(bar.get_index(), 1);
    // Quit still asks first.
    crate::router::handle_action(&ctx, &app, "accept");
    crate::router::handle_action(&ctx, &app, "up");
    crate::router::handle_action(&ctx, &app, "accept");
    advance(zaparoo_app::input::PRESS_FEEDBACK_MS + 10);
    assert!(overlays.get_dialog_open());
    assert_eq!(overlays.get_dialog_kind(), DialogKind::QuitConfirm);
}

#[test]
fn back_tapped_goes_back_on_release_and_held_goes_straight_to_the_hub() {
    use slint::platform::Key;
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    let (_runtime, ctx) = offline_ctx();
    let ctx = std::sync::Arc::new(ctx);
    crate::sizing::apply_scene(
        &app,
        crate::sizing::Scene::of(&app, f64::from(W), f64::from(H), false),
    );
    crate::input::bind(&ctx, &app, std::collections::HashMap::new());
    let shell = app.global::<Shell>();

    // A tap waits for its release, then goes back one level and no further.
    crate::router::enter_about(&ctx, &app, EntryMode::Fresh);
    settle(&window);
    key_down(&app, Key::Escape);
    assert_eq!(shell.get_active_screen(), Screen::About);
    pass(&ctx, zaparoo_app::input::BACK_HOLD_TO_HUB_MS / 2);
    key_up(&app, Key::Escape);
    assert_eq!(shell.get_active_screen(), Screen::Settings);
    pass(&ctx, zaparoo_app::input::BACK_HOLD_TO_HUB_MS);
    assert_eq!(shell.get_active_screen(), Screen::Settings);

    // Held, it goes straight to the Hub, never stopping on Settings.
    crate::router::enter_about(&ctx, &app, EntryMode::Fresh);
    key_down(&app, Key::Backspace);
    pass(&ctx, zaparoo_app::input::BACK_HOLD_TO_HUB_MS - 1);
    assert_eq!(shell.get_active_screen(), Screen::About);
    pass(&ctx, 1);
    assert_eq!(shell.get_active_screen(), Screen::Hub);
    assert_eq!(
        crate::router::lock(&ctx.shared).persist.active_screen,
        "hub",
        "a relaunch restores the Hub"
    );
    // Its release does nothing more.
    key_up(&app, Key::Backspace);
    assert_eq!(shell.get_active_screen(), Screen::Hub);

    // A modal's Back acts the moment it goes down.
    crate::router::enter_about(&ctx, &app, EntryMode::Fresh);
    settle(&window);
    pass(&ctx, 100);
    crate::router::present_list(
        &ctx,
        &app,
        crate::router::ListContext::HubPageMenu,
        "title:menu",
        vec![crate::router::menu_row("hub_settings")],
    );
    let overlays = app.global::<crate::Overlays>();
    assert!(overlays.get_list_open());
    key_down(&app, Key::Escape);
    assert!(!overlays.get_list_open());
    key_up(&app, Key::Escape);
    pass(&ctx, zaparoo_app::input::BACK_HOLD_TO_HUB_MS);
    assert_eq!(shell.get_active_screen(), Screen::About);
}

#[test]
fn back_over_a_pairing_or_online_panel_is_never_held_to_the_hub() {
    use slint::platform::Key;
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    let (_runtime, ctx) = offline_ctx();
    let ctx = std::sync::Arc::new(ctx);
    crate::input::bind(&ctx, &app, std::collections::HashMap::new());
    crate::router::enter_about(&ctx, &app, EntryMode::Fresh);
    settle(&window);
    let overlays = app.global::<crate::Overlays>();
    let open = |panel: usize, on: bool| match panel {
        0 => overlays.set_pair_open(on),
        1 => overlays.set_online_open(on),
        _ => overlays.set_online_list_open(on),
    };
    for panel in 0..3 {
        open(panel, true);
        key_down(&app, Key::Escape);
        pass(&ctx, zaparoo_app::input::BACK_HOLD_TO_HUB_MS * 2);
        key_up(&app, Key::Escape);
        assert_eq!(app.global::<Shell>().get_active_screen(), Screen::About);
        open(panel, false);
        pass(&ctx, 100);
    }
}

#[test]
fn the_hdmi_head_of_a_dual_head_pair_keeps_the_menu_list() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    let (_runtime, ctx) = offline_ctx();
    seed_hub_tiles(&ctx, &app);
    let sizing = app.global::<Sizing>();
    sizing.set_menu_row_blocked(true);
    crate::sizing::apply_scene(&app, crate::sizing::Scene::of(&app, 1280.0, 720.0, false));
    settle(&window);
    assert!(sizing.get_menu_row_enabled());
    assert!(!sizing.get_menu_row());
    assert!(sizing.get_menu_row_block().abs() < f32::EPSILON);
    crate::router::handle_action(&ctx, &app, "page_menu");
    assert!(!app.global::<crate::PageBar>().get_focused());
    assert!(app.global::<crate::Overlays>().get_list_open());
}

#[test]
fn l1_and_r1_together_open_go_to_and_a_lone_shoulder_pages() {
    use slint::platform::Key;
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    let (_runtime, ctx) = offline_ctx();
    let ctx = std::sync::Arc::new(ctx);
    crate::sizing::apply_scene(
        &app,
        crate::sizing::Scene::of(&app, f64::from(W), f64::from(H), false),
    );
    crate::input::bind(&ctx, &app, std::collections::HashMap::new());
    lettered_folder(&ctx, &app);
    settle(&window);
    let overlays = app.global::<crate::Overlays>();
    let page = || crate::router::lock(&ctx.shared).games.grid.current_page();

    // Both inside the window: Go to, with no page flipped on the way.
    key_down(&app, Key::PageUp);
    assert_eq!(page(), 0, "the first half waits for its partner");
    pass(&ctx, zaparoo_app::input::CHORD_WINDOW_MS / 2);
    key_down(&app, Key::PageDown);
    assert!(overlays.get_letter_open());
    assert_eq!(page(), 0);
    key_up(&app, Key::PageUp);
    key_up(&app, Key::PageDown);
    pass(&ctx, zaparoo_app::input::CHORD_WINDOW_MS);
    assert_eq!(page(), 0, "the chord never pages");
    crate::router::handle_action(&ctx, &app, "cancel");
    assert!(!overlays.get_letter_open());

    // Alone, a shoulder pages once the window closes.
    pass(&ctx, 100);
    key_down(&app, Key::PageDown);
    assert_eq!(page(), 0);
    pass(&ctx, zaparoo_app::input::CHORD_WINDOW_MS);
    assert_eq!(page(), 1);
    assert!(!overlays.get_letter_open());
    key_up(&app, Key::PageDown);

    // Released inside the window, it pages straight away.
    pass(&ctx, 100);
    key_down(&app, Key::PageUp);
    key_up(&app, Key::PageUp);
    assert_eq!(page(), 0);
    assert!(!overlays.get_letter_open());
}

#[test]
#[allow(
    clippy::expect_used,
    reason = "tests should fail fast on a missing tile or scratch folder"
)]
fn rename_tile_types_a_name_saves_it_and_returns_to_the_tile() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    let (_runtime, ctx) = offline_ctx();
    crate::sizing::apply_scene(
        &app,
        crate::sizing::Scene::of(&app, f64::from(W), f64::from(H), false),
    );
    hub_with_saved_search(&ctx, &app);
    let shell = app.global::<Shell>();
    settle(&window);
    crate::router::handle_action(&ctx, &app, "context_menu");
    focus_context_row(&app, "hub_rename");
    crate::router::handle_action(&ctx, &app, "accept");
    advance(zaparoo_app::input::PRESS_FEEDBACK_MS);
    settle(&window);
    assert_eq!(shell.get_active_screen(), Screen::Rename);
    let view = app.global::<crate::RenameView>();
    assert_eq!(
        view.get_before(),
        "RPGs",
        "the field starts on the tile's name"
    );
    assert_eq!(view.get_key_rows(), 5, "a name takes the full keyboard");
    {
        let shared = crate::router::lock(&ctx.shared);
        assert_eq!(shared.persist.active_screen, "rename");
        assert_eq!(shared.persist.rename.draft, "RPGs");
        assert!(shared.persist.rename.target.identity.starts_with("search:"));
    }
    for c in " 94".chars() {
        crate::rename::type_char(&ctx, &app, c);
    }
    assert_eq!(
        crate::router::lock(&ctx.shared).persist.rename.draft,
        "RPGs 94",
        "every edit is kept for a restore"
    );
    // Typing puts focus on Save, so Accept saves.
    crate::router::handle_action(&ctx, &app, "accept");
    settle(&window);
    assert_eq!(shell.get_active_screen(), Screen::Hub);
    let shared = crate::router::lock(&ctx.shared);
    assert_eq!(shared.persist.active_screen, "hub");
    let item = shared.hub.layout.visible().next().expect("the tile");
    assert_eq!(item.name, "RPGs 94");
    assert_eq!(shared.hub.entries[0].name, "RPGs 94");
}

#[test]
#[allow(
    clippy::expect_used,
    reason = "tests should fail fast on a missing tile or scratch folder"
)]
fn rename_back_keeps_the_name_and_a_blank_name_resets_it() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    let (_runtime, ctx) = offline_ctx();
    crate::sizing::apply_scene(
        &app,
        crate::sizing::Scene::of(&app, f64::from(W), f64::from(H), false),
    );
    hub_with_saved_search(&ctx, &app);
    let shell = app.global::<Shell>();
    settle(&window);
    let open = |ctx: &crate::router::Ctx| {
        crate::router::handle_action(ctx, &app, "context_menu");
        focus_context_row(&app, "hub_rename");
        crate::router::handle_action(ctx, &app, "accept");
        advance(zaparoo_app::input::PRESS_FEEDBACK_MS);
        settle(&window);
        assert_eq!(app.global::<Shell>().get_active_screen(), Screen::Rename);
    };
    open(&ctx);
    assert!(
        !app.global::<crate::Overlays>().get_context_open(),
        "a screen ready at once takes the menu with it"
    );
    crate::rename::type_char(&ctx, &app, '!');
    crate::router::handle_action(&ctx, &app, "cancel");
    settle(&window);
    assert_eq!(shell.get_active_screen(), Screen::Hub);
    assert_eq!(
        crate::router::lock(&ctx.shared)
            .hub
            .layout
            .visible()
            .next()
            .map(|item| item.name.clone()),
        Some("RPGs".to_string()),
        "Back saves nothing"
    );
    assert!(
        app.global::<crate::Overlays>().get_context_open(),
        "and brings the menu back"
    );
    assert_eq!(context_row_id(&app).as_deref(), Some("hub_rename"));
    crate::router::handle_action(&ctx, &app, "cancel");
    assert!(!app.global::<crate::Overlays>().get_context_open());

    open(&ctx);
    for _ in 0.."RPGs".len() {
        crate::router::handle_action(&ctx, &app, zaparoo_app::input::TEXT_DELETE);
    }
    assert_eq!(app.global::<crate::RenameView>().get_before(), "");
    // A refused leading space still puts focus on Save.
    crate::rename::type_char(&ctx, &app, ' ');
    crate::router::handle_action(&ctx, &app, "accept");
    settle(&window);
    assert_eq!(shell.get_active_screen(), Screen::Hub);
    assert!(
        !app.global::<crate::Overlays>().get_context_open(),
        "Save ends on the screen"
    );
    let shared = crate::router::lock(&ctx.shared);
    let item = shared.hub.layout.visible().next().expect("the tile");
    assert_eq!(item.name, "", "a blank name is the tile's own again");
    assert_eq!(shared.hub.entries[0].name, "final");
}

#[test]
#[allow(
    clippy::expect_used,
    clippy::too_many_lines,
    reason = "one walk through a pick and every way back out, failing fast on a missing tile"
)]
fn change_icon_browses_images_and_a_pick_becomes_the_tile_icon() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    let (_runtime, ctx) = offline_ctx();
    crate::sizing::apply_scene(
        &app,
        crate::sizing::Scene::of(&app, f64::from(W), f64::from(H), false),
    );
    let root = std::env::temp_dir().join(format!("zaparoo-icon-pick-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("hub")).expect("hub folder");
    std::fs::write(root.join("hub").join("rpg.png"), b"not really a png").expect("file");
    std::fs::write(root.join("hub").join("notes.txt"), b"x").expect("file");
    crate::customization::configure(root.clone(), std::collections::HashMap::new());
    hub_with_saved_search(&ctx, &app);
    let shell = app.global::<Shell>();
    settle(&window);
    crate::router::handle_action(&ctx, &app, "context_menu");
    focus_context_row(&app, "hub_icon");
    crate::router::handle_action(&ctx, &app, "accept");
    advance(zaparoo_app::input::PRESS_FEEDBACK_MS);
    settle(&window);
    assert!(
        shell.get_transitioning(),
        "the folder read is a deferred route"
    );
    assert_eq!(shell.get_active_screen(), Screen::Hub);
    let overlays = app.global::<crate::Overlays>();
    assert!(
        overlays.get_context_open(),
        "the menu stays up while the folder is read"
    );

    let hub_dir = root.join("hub").to_string_lossy().into_owned();
    crate::file_browser::land_now(&ctx, &app, Some(&hub_dir), "");
    assert!(
        !overlays.get_context_open(),
        "and leaves in the turn the browser appears"
    );
    settle(&window);
    assert_eq!(shell.get_active_screen(), Screen::FileBrowser);
    let view = app.global::<crate::FileBrowserView>();
    assert_eq!(view.get_mode(), crate::FileBrowserMode::Images);
    assert_eq!(view.get_title(), "hub");
    let names: Vec<String> = view
        .get_cells()
        .iter()
        .map(|c| c.name.to_string())
        .collect();
    assert_eq!(names, ["rpg.png"], "images only");
    assert!(view.get_focused_picks());
    {
        let shared = crate::router::lock(&ctx.shared);
        assert_eq!(shared.persist.active_screen, "file-browser");
        assert_eq!(shared.persist.file_browser.dir, hub_dir);
        assert_eq!(shared.persist.file_browser.selected, "rpg.png");
    }

    crate::router::handle_action(&ctx, &app, "accept");
    settle(&window);
    assert_eq!(shell.get_active_screen(), Screen::Hub);
    let picked = root
        .join("hub")
        .join("rpg.png")
        .to_string_lossy()
        .into_owned();
    {
        let shared = crate::router::lock(&ctx.shared);
        let item = shared.hub.layout.visible().next().expect("the tile");
        assert_eq!(
            item.icon, picked,
            "the file is named where it is, never copied"
        );
        assert_eq!(
            shared.hub.entries[0].cover_key,
            format!("custom-path:{picked}")
        );
    }

    // Back climbs to the custom folder, then Places, then the Hub.
    crate::router::handle_action(&ctx, &app, "context_menu");
    focus_context_row(&app, "hub_icon");
    crate::router::handle_action(&ctx, &app, "accept");
    advance(zaparoo_app::input::PRESS_FEEDBACK_MS);
    settle(&window);
    crate::file_browser::land_now(&ctx, &app, Some(&hub_dir), &picked);
    crate::router::handle_action(&ctx, &app, "cancel");
    assert!(shell.get_transitioning());
    let custom = root.to_string_lossy().into_owned();
    crate::file_browser::land_now(&ctx, &app, Some(&custom), &hub_dir);
    assert_eq!(
        view.get_title(),
        root.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default()
    );
    crate::router::handle_action(&ctx, &app, "cancel");
    crate::file_browser::land_now(&ctx, &app, None, &custom);
    assert!(view.get_places());
    crate::router::handle_action(&ctx, &app, "cancel");
    settle(&window);
    assert_eq!(shell.get_active_screen(), Screen::Hub);
    assert_eq!(
        crate::router::lock(&ctx.shared)
            .hub
            .layout
            .visible()
            .next()
            .map(|item| item.icon.clone()),
        Some(picked),
        "leaving without a pick changes nothing"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_cold_start_restores_the_rename_draft_only_on_the_same_tile() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    let (_runtime, ctx) = offline_ctx();
    crate::sizing::apply_scene(
        &app,
        crate::sizing::Scene::of(&app, f64::from(W), f64::from(H), false),
    );
    hub_with_saved_search(&ctx, &app);
    let identity = {
        let shared = crate::router::lock(&ctx.shared);
        zaparoo_app::hub::commit(&shared.hub.entries[0])
            .map(|commit| commit.selected_item)
            .unwrap_or_default()
    };
    let saved = |identity: &str| zaparoo_core::persist::RenameState {
        target: zaparoo_core::persist::HubTarget {
            hub_index: 0,
            identity: identity.to_string(),
        },
        draft: "Role-playing".into(),
        caret: 4,
        ..zaparoo_core::persist::RenameState::default()
    };
    {
        let mut shared = crate::router::lock(&ctx.shared);
        shared.persist.active_screen = "rename".into();
        shared.persist.rename = saved("search:another");
    }
    crate::rename::restore(&ctx, &app);
    assert_eq!(app.global::<Shell>().get_active_screen(), Screen::Hub);
    assert_eq!(
        crate::router::lock(&ctx.shared).persist.active_screen,
        "hub",
        "a tile that moved restores the Hub"
    );

    crate::router::lock(&ctx.shared).persist.rename = saved(&identity);
    crate::rename::restore(&ctx, &app);
    settle(&window);
    assert_eq!(app.global::<Shell>().get_active_screen(), Screen::Rename);
    let view = app.global::<crate::RenameView>();
    assert_eq!(view.get_before(), "Role");
    assert_eq!(view.get_at(), "-");
    assert_eq!(view.get_after(), "playing");
}

#[test]
fn a_list_with_no_fixed_total_never_shows_the_rail() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    let (_runtime, ctx) = offline_ctx();
    crate::sizing::apply_scene(
        &app,
        crate::sizing::Scene::of(&app, f64::from(W), f64::from(H), false),
    );
    app.global::<Shell>().set_active_screen(Screen::Games);
    seat_folder(&ctx, &app, "Game", 0);
    settle(&window);
    let view = app.global::<crate::GamesView>();
    // A paged list Core gives no total for, as Favorites, Recently played
    // and Search results are.
    crate::router::lock(&ctx.shared).games.total_known = false;
    crate::input::dispatch_repeat(&ctx, &app, "page_next", HoldTier::Page);
    assert!(view.get_rapid_active(), "the fast scroll itself still runs");
    assert!(!view.get_rail_visible());

    crate::router::lock(&ctx.shared).games.total_known = true;
    crate::input::dispatch_repeat(&ctx, &app, "page_next", HoldTier::Page);
    crate::games::render(&ctx, &app);
    assert!(view.get_rail_visible(), "a counted folder keeps it");
}

/// A folder of `count` images and the Hub tile's browser opened on it.
#[allow(
    clippy::expect_used,
    reason = "tests should fail fast on a scratch folder"
)]
fn browser_on_images(
    ctx: &crate::router::Ctx,
    app: &App,
    window: &Rc<MinimalSoftwareWindow>,
    name: &str,
    count: usize,
) -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!("zaparoo-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("hub")).expect("hub folder");
    for i in 0..count {
        let letter = char::from(b'a' + u8::try_from(i % 26).unwrap_or(0));
        std::fs::write(root.join("hub").join(format!("{letter}{i:03}.png")), b"x").expect("file");
    }
    crate::customization::configure(root.clone(), std::collections::HashMap::new());
    hub_with_saved_search(ctx, app);
    settle(window);
    crate::router::handle_action(ctx, app, "context_menu");
    focus_context_row(app, "hub_icon");
    crate::router::handle_action(ctx, app, "accept");
    advance(zaparoo_app::input::PRESS_FEEDBACK_MS);
    settle(window);
    let hub_dir = root.join("hub").to_string_lossy().into_owned();
    crate::file_browser::land_now(ctx, app, Some(&hub_dir), "");
    settle(window);
    root
}

#[test]
fn the_image_browser_turns_pages_on_the_shared_strip() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    let (_runtime, ctx) = offline_ctx();
    crate::sizing::apply_scene(
        &app,
        crate::sizing::Scene::of(&app, f64::from(W), f64::from(H), false),
    );
    crate::router::lock(&ctx.shared)
        .persist
        .settings
        .reduce_motion = false;
    let root = browser_on_images(&ctx, &app, &window, "browser-pages", 60);
    let shell = app.global::<Shell>();
    assert_eq!(shell.get_active_screen(), Screen::FileBrowser);
    let view = app.global::<crate::FileBrowserView>();
    assert!(view.get_total_pages() > 1);
    crate::router::handle_action(&ctx, &app, "page_next");
    assert!(
        view.get_page_slide().abs() > f32::EPSILON,
        "the page slides"
    );
    assert!(
        view.get_next_cells().iter().count() > 0,
        "into the incoming half"
    );
    assert_eq!(view.get_selected_local(), -1, "no ring while it moves");
    // Another press lands the turn and takes its own at once.
    crate::router::handle_action(&ctx, &app, "page_next");
    assert!(
        view.get_page_slide().abs() < f32::EPSILON,
        "the interrupting turn cuts"
    );
    assert_eq!(view.get_page(), 2);
    CLOCK.with(|clock| clock.set(clock.get() + crate::page_motion::SETTLE_MS));
    slint::platform::update_timers_and_animations();
    assert!(!crate::router::lock(&ctx.shared).file_browser_sliding());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_long_hold_in_the_image_browser_steps_by_letter_and_raises_the_rail() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    let (_runtime, ctx) = offline_ctx();
    crate::sizing::apply_scene(
        &app,
        crate::sizing::Scene::of(&app, f64::from(W), f64::from(H), false),
    );
    let root = browser_on_images(&ctx, &app, &window, "browser-letters", 52);
    let view = app.global::<crate::FileBrowserView>();
    assert_eq!(view.get_rail_letters().iter().count(), 26);
    crate::input::dispatch_repeat(&ctx, &app, "down", HoldTier::Letter);
    assert_eq!(view.get_rail_letter(), "B", "one letter per step");
    assert!(
        view.get_rail_visible(),
        "the rail comes up with the fast scroll"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn every_grid_jumps_pages_while_a_held_direction_runs_fast() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    let (_runtime, ctx) = offline_ctx();
    seed_hub_pages(&ctx, &app);
    crate::router::lock(&ctx.shared)
        .persist
        .settings
        .reduce_motion = false;
    settle(&window);
    let hub = app.global::<HubView>();
    crate::page_motion::turn::<crate::hub::HubScreen>(&ctx, &app, 1, 0, true);
    assert!(hub.get_page_slide().abs() > f32::EPSILON, "a turn slides");
    CLOCK.with(|clock| clock.set(clock.get() + crate::page_motion::SETTLE_MS));
    slint::platform::update_timers_and_animations();
    settle(&window);
    // A held direction that has run long enough to count as fast: a turn
    // that lands then (a page arriving mid-scroll) jumps, on every grid.
    crate::input::note_rapid(&ctx, &app, "down", true);
    assert!(crate::input::rapid_navigation(&ctx));
    crate::page_motion::turn::<crate::hub::HubScreen>(&ctx, &app, 1, 0, true);
    assert!(
        hub.get_page_slide().abs() < f32::EPSILON,
        "a held fast move jumps"
    );
    assert!(!crate::router::lock(&ctx.shared).hub.page.sliding);
}

#[test]
fn back_while_a_menu_row_opens_its_screen_calls_it_off_and_keeps_the_menu() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    let (_runtime, ctx) = offline_ctx();
    crate::sizing::apply_scene(
        &app,
        crate::sizing::Scene::of(&app, f64::from(W), f64::from(H), false),
    );
    hub_with_saved_search(&ctx, &app);
    settle(&window);
    let shell = app.global::<Shell>();
    let overlays = app.global::<crate::Overlays>();
    crate::router::handle_action(&ctx, &app, "context_menu");
    focus_context_row(&app, "hub_icon");
    crate::router::handle_action(&ctx, &app, "accept");
    advance(zaparoo_app::input::PRESS_FEEDBACK_MS);
    settle(&window);
    assert!(shell.get_transitioning());
    assert!(overlays.get_context_open());
    // Moves wait for the route; Back calls it off and keeps the menu.
    crate::router::handle_action(&ctx, &app, "down");
    assert!(overlays.get_context_open());
    crate::router::handle_action(&ctx, &app, "cancel");
    assert!(!shell.get_transitioning());
    assert!(overlays.get_context_open(), "the menu stays, on its row");
    assert_eq!(context_row_id(&app).as_deref(), Some("hub_icon"));
    assert_eq!(
        app.global::<crate::PressFeedback>().get_owner(),
        PressOwner::None
    );
    assert_eq!(shell.get_active_screen(), Screen::Hub);
    // The read that lands after that is dropped.
    crate::file_browser::land_now(&ctx, &app, None, "");
    assert_eq!(shell.get_active_screen(), Screen::Hub);
}

#[test]
fn group_by_keeps_its_list_up_until_the_other_screen_is_ready() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    let (_runtime, ctx) = offline_ctx();
    crate::sizing::apply_scene(
        &app,
        crate::sizing::Scene::of(&app, f64::from(W), f64::from(H), false),
    );
    let shell = app.global::<Shell>();
    let overlays = app.global::<crate::Overlays>();
    shell.set_active_screen(Screen::FavoriteSystems);
    crate::router::lock(&ctx.shared)
        .persist
        .settings
        .favorites_grouping = "system".into();
    settle(&window);
    let pick_none = |ctx: &crate::router::Ctx| {
        crate::router::open_favorites_grouping_menu(ctx, &app);
        assert!(overlays.get_list_open());
        overlays.set_list_index(0);
        crate::router::handle_action(ctx, &app, "accept");
        advance(zaparoo_app::input::PRESS_FEEDBACK_MS);
    };
    pick_none(&ctx);
    assert!(shell.get_transitioning(), "Favorites fills before it shows");
    assert!(overlays.get_list_open(), "the list stays up meanwhile");
    assert_eq!(shell.get_active_screen(), Screen::FavoriteSystems);
    let ticket = crate::router::lock(&ctx.shared).games.ticket;
    crate::games::apply_fill(&ctx, &app, ticket, navigation_rows(), None, None, true);
    assert_eq!(shell.get_active_screen(), Screen::Favorites);
    assert!(!overlays.get_list_open(), "and leaves in the turn it lands");

    // Back while it waits calls off the route and the list.
    shell.set_active_screen(Screen::FavoriteSystems);
    crate::router::lock(&ctx.shared)
        .persist
        .settings
        .favorites_grouping = "system".into();
    settle(&window);
    pick_none(&ctx);
    assert!(overlays.get_list_open());
    crate::router::handle_action(&ctx, &app, "down");
    assert!(overlays.get_list_open(), "nothing else acts while it waits");
    crate::router::handle_action(&ctx, &app, "cancel");
    assert!(!shell.get_transitioning());
    assert!(!overlays.get_list_open());
    assert_eq!(shell.get_active_screen(), Screen::FavoriteSystems);
}

#[test]
fn change_launcher_keeps_the_menu_up_while_core_is_asked() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    let (_runtime, ctx) = offline_ctx();
    seat_visibility_list(&ctx, &app, GamesMode::Browse);
    crate::router::lock(&ctx.shared).launchers = vec![zaparoo_core::media_types::LauncherInfo {
        id: "nes".into(),
        system_id: "NES".into(),
        ..Default::default()
    }];
    let overlays = app.global::<crate::Overlays>();
    crate::router::handle_action(&ctx, &app, "context_menu");
    focus_context_row(&app, "manage_game");
    crate::router::handle_action(&ctx, &app, "accept");
    advance(zaparoo_app::input::PRESS_FEEDBACK_MS);
    settle(&window);
    focus_context_row(&app, "change_launcher");
    crate::router::handle_action(&ctx, &app, "accept");
    advance(zaparoo_app::input::PRESS_FEEDBACK_MS);
    settle(&window);
    assert!(overlays.get_context_open(), "the menu waits for the picker");
    assert!(!overlays.get_list_open());
    assert_eq!(
        app.global::<crate::PressFeedback>().get_owner(),
        PressOwner::Context,
        "its row stays pressed"
    );
    let seq = crate::router::lock(&ctx.shared).game_launcher_seq;
    let index = overlays.get_context_index();
    crate::router::handle_action(&ctx, &app, "down");
    crate::router::handle_action(&ctx, &app, "accept");
    advance(zaparoo_app::input::PRESS_FEEDBACK_MS);
    assert!(
        overlays.get_context_open(),
        "nothing else acts while it waits"
    );
    assert_eq!(overlays.get_context_index(), index, "focus does not move");
    assert!(!overlays.get_list_open() && !overlays.get_qr_open());
    crate::router::handle_action(&ctx, &app, "cancel");
    assert!(overlays.get_context_open(), "Back keeps the menu");
    assert!(!overlays.get_list_open());
    assert_eq!(context_row_id(&app).as_deref(), Some("change_launcher"));
    assert_ne!(
        crate::router::lock(&ctx.shared).game_launcher_seq,
        seq,
        "and the picker that answers late is dropped"
    );
    assert_eq!(
        app.global::<crate::PressFeedback>().get_owner(),
        PressOwner::None
    );
}

#[test]
fn game_info_moves_focus_over_what_acts_and_a_tag_searches_and_comes_back() {
    use zaparoo_core::media_types::{MediaMeta, MediaMetaProperty, TagInfo};
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    let (_runtime, ctx) = offline_ctx();
    let row = seat_visibility_list(&ctx, &app, GamesMode::Browse);
    let shell = app.global::<Shell>();
    let info = app.global::<crate::GameInfoView>();
    let tag = |tag_type: &str, tag: &str| TagInfo {
        tag_type: tag_type.into(),
        tag: tag.into(),
        ..Default::default()
    };
    let mut meta = MediaMeta {
        available_image_types: vec!["boxart".into(), "screenshot".into()],
        ..Default::default()
    };
    meta.title.system.name = "Nintendo".into();
    meta.title.tags = vec![
        tag("developer", "Square"),
        tag("genre", "rpg"),
        tag("genre", "rpg:j"),
        tag("scraper.gamelist.xml", "matched"),
    ];
    meta.title.properties.insert(
        "property:description".into(),
        MediaMetaProperty {
            text: "A journey through time.".into(),
            ..Default::default()
        },
    );
    let land = |ctx: &crate::router::Ctx| {
        crate::game_info::land_for_test(ctx, &app, &meta);
        settle(&window);
    };
    crate::game_info::open(&ctx, &app, &row);
    land(&ctx);
    let rows: Vec<(String, String, i32)> = info
        .get_info_rows()
        .iter()
        .map(|row| (row.key.to_string(), row.value.to_string(), row.stop))
        .collect();
    // One row per type, the internal tag gone, the system first.
    assert_eq!(
        rows,
        [
            ("details".into(), String::new(), -1),
            ("system".into(), "Nintendo".into(), -1),
            ("developer".into(), "Square".into(), 2),
            ("genre".into(), "RPG, JRPG".into(), 3),
            ("technical".into(), String::new(), -1),
            ("path".into(), "/g/Game 0.nes".into(), -1),
        ]
    );
    // The art, the description, then the rows that search.
    assert_eq!(info.get_focus(), 0);
    assert_eq!(info.get_image_stop(), 0);
    assert_eq!(info.get_manual_stop(), -1);
    assert_eq!(info.get_description_stop(), 1);
    for _ in 0..3 {
        crate::router::handle_action(&ctx, &app, "down");
    }
    assert_eq!(info.get_focus(), 3, "on Genre");
    // Two values: A lists them; Back returns to the row.
    crate::router::handle_action(&ctx, &app, "accept");
    assert!(info.get_values_open());
    assert_eq!(info.get_values_kind(), "genre");
    crate::router::handle_action(&ctx, &app, "cancel");
    assert!(!info.get_values_open());
    assert!(info.get_modal_open());
    assert_eq!(info.get_focus(), 3);

    // One value: A searches the game's system for it.
    crate::router::handle_action(&ctx, &app, "up");
    assert_eq!(info.get_focus(), 2, "on Developer");
    crate::router::handle_action(&ctx, &app, "accept");
    advance(zaparoo_app::input::PRESS_FEEDBACK_MS);
    assert!(!info.get_modal_open());
    {
        let shared = crate::router::lock(&ctx.shared);
        assert_eq!(shared.persist.search.systems, ["NES"]);
        assert_eq!(shared.persist.search.tags, ["developer:Square"]);
        assert!(crate::game_info::drilled(&shared));
    }
    let ticket = crate::router::lock(&ctx.shared).games.ticket;
    crate::games::apply_fill(&ctx, &app, ticket, game_rows("Square", 2), None, None, true);
    assert_eq!(shell.get_active_screen(), Screen::SearchResults);

    // Back: the list as it was, and Game Info again on Developer, in the
    // same turn: nothing waits on Core, so nothing flickers between them.
    crate::router::handle_action(&ctx, &app, "cancel");
    assert_eq!(shell.get_active_screen(), Screen::Games);
    {
        let shared = crate::router::lock(&ctx.shared);
        assert_eq!(shared.games.rows.len(), 3);
        assert_eq!(shared.games.mode, GamesMode::Browse);
        assert!(!crate::game_info::drilled(&shared));
    }
    assert!(info.get_modal_open());
    assert!(!info.get_loading());
    assert!(
        info.get_scroll_snap(),
        "the reopened panel cuts to its focus"
    );
    assert_eq!(info.get_focus(), 2, "back on the row it searched from");
}

#[test]
fn edit_search_on_a_tag_search_keeps_its_way_back_to_game_info() {
    use zaparoo_core::media_types::{MediaMeta, TagInfo};
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    let (_runtime, ctx) = offline_ctx();
    let row = seat_visibility_list(&ctx, &app, GamesMode::Browse);
    let shell = app.global::<Shell>();
    let info = app.global::<crate::GameInfoView>();
    let mut meta = MediaMeta::default();
    meta.title.tags = vec![TagInfo {
        tag_type: "developer".into(),
        tag: "Square".into(),
        ..Default::default()
    }];
    crate::game_info::open(&ctx, &app, &row);
    crate::game_info::land_for_test(&ctx, &app, &meta);
    settle(&window);
    // Developer is the one stop: A searches the game's system for it.
    assert_eq!(info.get_focus(), 0);
    crate::router::handle_action(&ctx, &app, "accept");
    advance(zaparoo_app::input::PRESS_FEEDBACK_MS);
    let ticket = crate::router::lock(&ctx.shared).games.ticket;
    crate::games::apply_fill(&ctx, &app, ticket, game_rows("Square", 2), None, None, true);
    assert_eq!(shell.get_active_screen(), Screen::SearchResults);

    // Edit search opens the Search screen with the search as it was; Back
    // there is Back to Game Info, as it is from the results.
    crate::search::edit(&ctx, &app);
    assert_eq!(shell.get_active_screen(), Screen::Search);
    {
        let shared = crate::router::lock(&ctx.shared);
        assert_eq!(shared.persist.search.systems, ["NES"]);
        assert_eq!(shared.persist.search.tags, ["developer:Square"]);
        assert!(crate::game_info::drilled(&shared));
    }
    crate::router::handle_action(&ctx, &app, "cancel");
    assert_eq!(shell.get_active_screen(), Screen::Games);
    assert!(info.get_modal_open());
    assert_eq!(info.get_focus(), 0);
}

#[test]
fn more_info_keeps_the_menu_up_until_the_details_are_ready() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    let (_runtime, ctx) = offline_ctx();
    seat_visibility_list(&ctx, &app, GamesMode::Browse);
    let overlays = app.global::<crate::Overlays>();
    let info = app.global::<crate::GameInfoView>();
    crate::router::handle_action(&ctx, &app, "context_menu");
    focus_context_row(&app, "more_info");
    crate::router::handle_action(&ctx, &app, "accept");
    advance(zaparoo_app::input::PRESS_FEEDBACK_MS);
    settle(&window);
    assert!(
        !info.get_modal_open(),
        "the panel is not shown before its metadata"
    );
    assert!(overlays.get_context_open(), "the menu waits for it");
    assert_eq!(
        app.global::<crate::PressFeedback>().get_owner(),
        PressOwner::Context,
        "its row stays pressed"
    );
    let ticket = crate::router::lock(&ctx.shared).game_info.ticket();
    crate::router::handle_action(&ctx, &app, "cancel");
    assert!(overlays.get_context_open(), "Back keeps the menu");
    assert_eq!(context_row_id(&app).as_deref(), Some("more_info"));
    assert!(!info.get_modal_open());
    assert_ne!(
        crate::router::lock(&ctx.shared).game_info.ticket(),
        ticket,
        "and details that answer late are dropped"
    );
}

#[test]
fn shift_covers_one_letter_and_held_it_locks_caps() {
    use slint::platform::Key as PlatformKey;
    use zaparoo_app::keyboard::Key;
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    let (_runtime, ctx) = offline_ctx();
    let ctx = std::sync::Arc::new(ctx);
    crate::sizing::apply_scene(
        &app,
        crate::sizing::Scene::of(&app, f64::from(W), f64::from(H), false),
    );
    crate::input::bind(&ctx, &app, std::collections::HashMap::new());
    hub_with_saved_search(&ctx, &app);
    settle(&window);
    crate::router::handle_action(&ctx, &app, "context_menu");
    focus_context_row(&app, "hub_rename");
    crate::router::handle_action(&ctx, &app, "accept");
    advance(zaparoo_app::input::PRESS_FEEDBACK_MS);
    settle(&window);
    assert_eq!(app.global::<Shell>().get_active_screen(), Screen::Rename);
    let view = app.global::<crate::RenameView>();
    let tap = |ms: u64| {
        key_down(&app, PlatformKey::Return);
        pass(&ctx, ms);
        key_up(&app, PlatformKey::Return);
        pass(&ctx, 200);
    };

    // A tap shifts the next letter, which lets it go again.
    crate::rename::focus_key_for_test(&ctx, Key::Shift);
    tap(50);
    assert!(view.get_shift_active());
    assert!(!view.get_shift_locked());
    crate::rename::focus_key_for_test(&ctx, Key::Char('Q'));
    tap(50);
    assert!(view.get_after().is_empty());
    assert!(view.get_before().ends_with('Q'));
    assert!(!view.get_shift_active(), "one letter, then lowercase");

    // Held past the threshold, Shift locks; letters stay uppercase.
    crate::rename::focus_key_for_test(&ctx, Key::Shift);
    tap(zaparoo_app::input::SHIFT_LOCK_HOLD_MS + 10);
    assert!(view.get_shift_locked());
    crate::rename::focus_key_for_test(&ctx, Key::Char('W'));
    tap(50);
    tap(50);
    assert!(view.get_before().ends_with("QWW"));
    assert!(view.get_shift_locked(), "caps stay on");

    // A tap turns caps off.
    crate::rename::focus_key_for_test(&ctx, Key::Shift);
    tap(50);
    assert!(!view.get_shift_active());
    assert!(!view.get_shift_locked());
}

#[test]
fn a_turn_whose_rows_were_replaced_still_comes_back_to_rest() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    let (_runtime, ctx) = offline_ctx();
    crate::sizing::apply_scene(
        &app,
        crate::sizing::Scene::of(&app, f64::from(W), f64::from(H), false),
    );
    crate::router::lock(&ctx.shared)
        .persist
        .settings
        .reduce_motion = false;
    let root = browser_on_images(&ctx, &app, &window, "browser-epoch", 60);
    let view = app.global::<crate::FileBrowserView>();
    crate::router::handle_action(&ctx, &app, "page_next");
    assert!(view.get_page_slide().abs() > f32::EPSILON);
    // A new folder read starts under the turn, without any input.
    crate::router::lock(&ctx.shared).file_browser_bump_ticket();
    CLOCK.with(|clock| clock.set(clock.get() + crate::page_motion::SETTLE_MS));
    slint::platform::update_timers_and_animations();
    assert!(
        view.get_page_slide().abs() < f32::EPSILON,
        "the strip is at rest"
    );
    assert!(!view.get_cached_transition());
    assert!(!crate::router::lock(&ctx.shared).file_browser_sliding());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_launch_refills_a_browse_list_only_when_it_holds_a_multi_disc_folder() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    let (_runtime, ctx) = offline_ctx();
    crate::sizing::apply_scene(
        &app,
        crate::sizing::Scene::of(&app, f64::from(W), f64::from(H), false),
    );
    let psx = zaparoo_core::media_types::SystemInfo {
        id: "PSX".into(),
        name: "PSX".into(),
        ..Default::default()
    };
    crate::games::enter(&ctx, &app, &psx);
    let ticket = crate::router::lock(&ctx.shared).games.ticket;
    crate::games::apply_fill(&ctx, &app, ticket, game_rows("Game", 12), None, None, true);
    settle(&window);
    assert_eq!(app.global::<Shell>().get_active_screen(), Screen::Games);
    let ticket = crate::router::lock(&ctx.shared).games.ticket;

    // No folder could have changed its launch disc.
    crate::games::refresh_after_launch(&ctx, &app);
    assert_eq!(crate::router::lock(&ctx.shared).games.ticket, ticket);

    crate::router::lock(&ctx.shared).games.rows[3].multi_disc = true;
    crate::games::refresh_after_launch(&ctx, &app);
    assert_eq!(crate::router::lock(&ctx.shared).games.ticket, ticket + 1);
}

#[test]
#[allow(
    clippy::expect_used,
    clippy::too_many_lines,
    reason = "one walk from Add item to a placed live folder tile, failing fast on a missing row"
)]
fn add_item_picks_a_folder_from_disk_for_a_live_folder_tile() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    let (_runtime, mut ctx) = offline_ctx();
    // Folders are picked where the frontend shares Core's disk.
    ctx.is_mister = true;
    crate::sizing::apply_scene(
        &app,
        crate::sizing::Scene::of(&app, f64::from(W), f64::from(H), false),
    );
    let root = std::env::temp_dir().join(format!("zaparoo-live-pick-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("_RA_Cores")).expect("folder");
    std::fs::create_dir_all(root.join("_Arcade")).expect("folder");
    std::fs::write(root.join("menu.rbf"), b"x").expect("file");
    hub_with_saved_search(&ctx, &app);
    let shell = app.global::<Shell>();
    settle(&window);

    crate::hub::add_picked(&ctx, &app, zaparoo_app::hub::CUSTOM_FOLDER_ENTRY);
    assert!(
        shell.get_transitioning(),
        "the folder read is a deferred route"
    );
    let fat = root.to_string_lossy().into_owned();
    crate::file_browser::land_now(&ctx, &app, Some(&fat), "");
    settle(&window);
    assert_eq!(shell.get_active_screen(), Screen::FileBrowser);
    let view = app.global::<crate::FileBrowserView>();
    assert_eq!(view.get_mode(), crate::FileBrowserMode::Plain);
    assert!(!view.get_has_menu());
    let names: Vec<String> = view
        .get_list_rows()
        .iter()
        .map(|c| c.name.to_string())
        .collect();
    assert_eq!(
        names,
        ["Add this folder", "_Arcade/", "_RA_Cores/"],
        "the pick row, then folders only"
    );
    assert!(view.get_focused_picks());
    assert_eq!(
        crate::router::lock(&ctx.shared).persist.file_browser.mode,
        "folders"
    );

    // Into a folder, then pick it.
    crate::router::handle_action(&ctx, &app, "down");
    crate::router::handle_action(&ctx, &app, "down");
    crate::router::handle_action(&ctx, &app, "accept");
    let cores = root.join("_RA_Cores").to_string_lossy().into_owned();
    crate::file_browser::land_now(&ctx, &app, Some(&cores), "");
    assert_eq!(view.get_title(), "_RA_Cores");
    crate::router::handle_action(&ctx, &app, "accept");
    settle(&window);
    assert_eq!(shell.get_active_screen(), Screen::Hub);
    {
        let shared = crate::router::lock(&ctx.shared);
        let tile = shared
            .hub
            .layout
            .visible()
            .find(|item| item.is_live_folder())
            .expect("the live folder tile");
        assert_eq!(tile.path, cores);
        assert!(tile.system.is_empty());
        let entry = shared
            .hub
            .entries
            .iter()
            .find(|entry| entry.live)
            .expect("its entry");
        assert_eq!(entry.name, "_RA_Cores");
        assert!(shared.hub.move_armed(), "handed to the user to carry");
    }

    // Back out of an empty pick returns to Add item.
    crate::hub::add_picked(&ctx, &app, zaparoo_app::hub::CUSTOM_FOLDER_ENTRY);
    crate::file_browser::land_now(&ctx, &app, None, "");
    assert!(view.get_places());
    crate::router::handle_action(&ctx, &app, "cancel");
    settle(&window);
    assert_eq!(shell.get_active_screen(), Screen::Hub);
    assert!(app.global::<crate::Overlays>().get_list_open());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_folder_pick_restores_on_its_folder_after_a_kill() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, _window) = boot();
    let (_runtime, ctx) = offline_ctx();
    let root = std::env::temp_dir().join(format!("zaparoo-live-restore-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::create_dir_all(root.join("_Favorites"));
    let dir = root.to_string_lossy().into_owned();
    {
        let mut shared = crate::router::lock(&ctx.shared);
        shared.persist.active_screen = "file-browser".into();
        shared.persist.file_browser = zaparoo_core::persist::FileBrowserState {
            target: zaparoo_core::persist::HubTarget {
                hub_index: 3,
                identity: String::new(),
            },
            mode: "folders".into(),
            dir: dir.clone(),
            selected: "_Favorites".into(),
            deck_id: String::new(),
            header_logo: false,
        };
    }
    crate::file_browser::restore(&ctx, &app);
    assert!(app.global::<Shell>().get_transitioning());
    crate::file_browser::land_now(
        &ctx,
        &app,
        Some(&dir),
        &root.join("_Favorites").to_string_lossy(),
    );
    let view = app.global::<crate::FileBrowserView>();
    assert_eq!(view.get_current_index(), 1, "the folder it was on");
    assert!(!view.get_has_menu(), "still a folder pick");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
#[allow(
    clippy::too_many_lines,
    reason = "one walk through a live folder: its menus, a subfolder and back out"
)]
fn a_live_folder_browses_without_a_system_and_back_returns_to_the_hub() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    let (_runtime, ctx) = offline_ctx();
    crate::sizing::apply_scene(
        &app,
        crate::sizing::Scene::of(&app, f64::from(W), f64::from(H), false),
    );
    let shell = app.global::<Shell>();
    let rows = |base: &str| {
        let mut rows = game_rows("Core", 3);
        for (index, row) in rows.iter_mut().enumerate() {
            row.path = format!("{base}/core{index}.rbf");
            row.zap_script = row.path.clone();
        }
        rows[0].entry_type = zaparoo_app::media_list::EntryType::Directory;
        rows[0].media_capable = false;
        rows[0].zap_script.clear();
        rows[0].path = format!("{base}/Sub");
        for row in &mut rows {
            row.tags_known = false;
        }
        rows
    };

    crate::games::enter_live_folder_from_hub(&ctx, &app, "/media/fat/_RA_Cores");
    let ticket = {
        let shared = crate::router::lock(&ctx.shared);
        assert!(shared.games.live && shared.persist.games.live);
        assert!(shared.games.system_id.is_empty());
        assert_eq!(shared.games.browse_path, "/media/fat/_RA_Cores");
        assert_eq!(shared.persist.games.path_stack, ["/media/fat/_RA_Cores"]);
        shared.games.ticket
    };
    crate::games::apply_fill(
        &ctx,
        &app,
        ticket,
        rows("/media/fat/_RA_Cores"),
        None,
        None,
        true,
    );
    settle(&window);
    assert_eq!(shell.get_active_screen(), Screen::Games);
    let view = app.global::<crate::GamesView>();
    assert_eq!(view.get_title(), "_RA_Cores");
    assert!(!view.get_page_menu());
    assert!(crate::page_bar::chip_ids(&app).is_empty());
    {
        let shared = crate::router::lock(&ctx.shared);
        assert!(!crate::games::go_to_available(&shared));
    }

    // Options pins a subfolder as another live folder, and a file as its
    // own launch script.
    crate::games::context_accept(&ctx, &app, "add_to_hub");
    crate::router::handle_action(&ctx, &app, "right");
    crate::games::context_accept(&ctx, &app, "add_to_hub");
    {
        let shared = crate::router::lock(&ctx.shared);
        let items: Vec<(bool, &str, &str)> = shared
            .hub
            .layout
            .items
            .iter()
            .map(|item| (item.live, item.path.as_str(), item.script.as_str()))
            .collect();
        assert!(items.contains(&(true, "/media/fat/_RA_Cores/Sub", "")));
        assert!(items.contains(&(
            false,
            "/media/fat/_RA_Cores/core1.rbf",
            "/media/fat/_RA_Cores/core1.rbf"
        )));
        assert!(crate::hub::has_live_folder(
            &shared,
            "/media/fat/_RA_Cores/Sub"
        ));
    }
    crate::router::handle_action(&ctx, &app, "left");

    // Into a subfolder and back to the pinned one, then the Hub.
    crate::router::handle_action(&ctx, &app, "accept");
    advance(zaparoo_app::input::PRESS_FEEDBACK_MS);
    let ticket = crate::router::lock(&ctx.shared).games.ticket;
    assert_eq!(
        crate::router::lock(&ctx.shared)
            .persist
            .games
            .path_stack
            .len(),
        2
    );
    crate::games::apply_fill(
        &ctx,
        &app,
        ticket,
        rows("/media/fat/_RA_Cores/Sub"),
        None,
        None,
        true,
    );
    crate::router::handle_action(&ctx, &app, "cancel");
    let ticket = {
        let shared = crate::router::lock(&ctx.shared);
        assert_eq!(shared.games.browse_path, "/media/fat/_RA_Cores");
        shared.games.ticket
    };
    crate::games::apply_fill(
        &ctx,
        &app,
        ticket,
        rows("/media/fat/_RA_Cores"),
        None,
        None,
        true,
    );
    crate::router::handle_action(&ctx, &app, "cancel");
    settle(&window);
    assert_eq!(shell.get_active_screen(), Screen::Hub);

    // A plain system browse afterwards is not live.
    crate::games::enter_from_hub(&ctx, &app, &navigation_catalog()[8]);
    let shared = crate::router::lock(&ctx.shared);
    assert!(!shared.games.live && !shared.persist.games.live);
}

#[test]
fn a_live_folder_restores_inside_its_last_folder() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, _window) = boot();
    let (_runtime, ctx) = offline_ctx();
    {
        let mut shared = crate::router::lock(&ctx.shared);
        shared.persist.games.live = true;
        shared.persist.games.entered_from_hub = true;
        shared.persist.games.path_stack = vec![
            "/media/fat/_Arcade".into(),
            "/media/fat/_Arcade/_Organized".into(),
        ];
        shared.persist.games.selected_at_level = vec![String::new(), String::new()];
    }
    crate::games::enter_live_restored(&ctx, &app);
    assert!(app.global::<Shell>().get_transitioning());
    let shared = crate::router::lock(&ctx.shared);
    assert!(shared.games.live);
    assert_eq!(shared.games.browse_path, "/media/fat/_Arcade/_Organized");
    assert_eq!(shared.games.system_name, "_Arcade");
}

fn core_state(revision: u64, events: &serde_json::Value) -> zaparoo_core::ui_events::UiState {
    serde_json::from_value(serde_json::json!({
        "revision": revision, "events": events, "resolved": []
    }))
    .unwrap_or_else(|_| unreachable!("a fixed UI state deserializes"))
}

fn core_picker() -> serde_json::Value {
    serde_json::json!([{
        "id": "e1", "kind": "picker", "title": "Couch co-op", "dismissible": true,
        "selectedChoiceId": "c2",
        "choices": [
            {"id": "c1", "label": "Super Mario World",
             "media": {"system": "SNES", "path": "/g/smw.sfc", "name": "Super Mario World"}},
            {"id": "c2", "label": "F-Zero",
             "media": {"system": "SNES", "path": "/g/fzero.sfc", "name": "F-Zero"}},
            {"id": "c3", "label": "Random Genesis game"}
        ]
    }])
}

#[test]
fn a_core_picker_opens_on_the_preselected_choice_and_closes_when_resolved() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    let (_runtime, ctx) = offline_ctx();
    crate::core_ui::apply_state(&ctx, &app, core_state(3, &core_picker()));
    frame(&window);
    let view = app.global::<crate::CoreUiView>();
    assert_eq!(view.get_kind(), crate::CoreUiKind::Picker);
    assert_eq!(view.get_title(), "Couch co-op");
    assert_eq!(view.get_rows().row_count(), 3);
    assert_eq!(view.get_index(), 1, "focus starts on Core's choice");
    assert!(view.get_has_art());
    // The picker owns input: moves stay in it and wrap.
    crate::router::handle_action(&ctx, &app, "down");
    crate::router::handle_action(&ctx, &app, "down");
    assert_eq!(view.get_index(), 0);
    // A stale state changes nothing.
    crate::core_ui::apply_state(&ctx, &app, core_state(2, &serde_json::json!([])));
    assert_eq!(view.get_kind(), crate::CoreUiKind::Picker);
    // Another client answered: Core lists it no more, and it closes here.
    crate::core_ui::apply_state(&ctx, &app, core_state(4, &serde_json::json!([])));
    assert_eq!(view.get_kind(), crate::CoreUiKind::None);
    frame(&window);
}

#[test]
fn answering_a_core_picker_closes_it_for_good() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, _window) = boot();
    let (_runtime, ctx) = offline_ctx();
    crate::core_ui::apply_state(&ctx, &app, core_state(1, &core_picker()));
    crate::core_ui::handle_action(&ctx, &app, "accept");
    let view = app.global::<crate::CoreUiView>();
    assert_eq!(view.get_kind(), crate::CoreUiKind::None);
    // A state from before Core took the answer still lists it: it stays shut.
    crate::core_ui::apply_state(&ctx, &app, core_state(1, &core_picker()));
    assert_eq!(view.get_kind(), crate::CoreUiKind::None);
}

#[test]
fn a_core_event_waits_under_the_boot_curtain() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, _window) = boot();
    let (_runtime, ctx) = offline_ctx();
    app.global::<Shell>().set_boot_curtain(true);
    crate::core_ui::apply_state(&ctx, &app, core_state(1, &core_picker()));
    let view = app.global::<crate::CoreUiView>();
    assert_eq!(view.get_kind(), crate::CoreUiKind::None);
    app.global::<Shell>().set_boot_curtain(false);
    crate::core_ui::present(&ctx, &app);
    assert_eq!(view.get_kind(), crate::CoreUiKind::Picker);
}

#[test]
fn a_core_confirmation_uses_the_dialog_and_waits_for_one_already_up() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    let (_runtime, ctx) = offline_ctx();
    let overlays = app.global::<crate::Overlays>();
    crate::router::open_quit_confirm(&app);
    assert!(overlays.get_dialog_open());
    let confirm = serde_json::json!([{
        "id": "e2", "kind": "confirm", "title": "Change game?",
        "message": "Launch Tetris?", "dismissible": true
    }]);
    crate::core_ui::apply_state(&ctx, &app, core_state(1, &confirm));
    assert_eq!(overlays.get_dialog_kind(), DialogKind::QuitConfirm);
    // The quit prompt is declined; the confirmation takes the dialog.
    crate::router::handle_action(&ctx, &app, "cancel");
    assert!(overlays.get_dialog_open());
    assert_eq!(overlays.get_dialog_kind(), DialogKind::CoreConfirm);
    assert_eq!(overlays.get_dialog_detail(), "Change game?");
    assert_eq!(overlays.get_dialog_arg(), "Launch Tetris?");
    frame(&window);
    // Back dismisses it.
    crate::router::handle_action(&ctx, &app, "cancel");
    assert!(!overlays.get_dialog_open());
}

#[test]
fn core_loaders_and_notices_take_the_header_line() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, _window) = boot();
    let (_runtime, ctx) = offline_ctx();
    let status = app.global::<crate::Status>();
    crate::status::set_link(
        &ctx.status,
        &app,
        &ctx.handle,
        zaparoo_app::status_line::Link::Connected,
        None,
    );
    let loader = serde_json::json!([{
        "id": "e3", "kind": "loader", "title": "Fetching deck", "dismissible": false
    }]);
    crate::core_ui::apply_state(&ctx, &app, core_state(1, &loader));
    assert_eq!(status.get_kind(), crate::StatusKind::CoreLoader);
    assert_eq!(status.get_arg(), "Fetching deck");
    let notice = serde_json::json!([{
        "id": "e4", "kind": "notice", "title": "Deck", "message": "Deck saved",
        "dismissible": true
    }]);
    crate::core_ui::apply_state(&ctx, &app, core_state(2, &notice));
    assert_eq!(status.get_kind(), crate::StatusKind::CoreNotice);
    assert_eq!(status.get_arg(), "Deck saved");
    crate::core_ui::apply_state(&ctx, &app, core_state(3, &serde_json::json!([])));
    assert_ne!(status.get_kind(), crate::StatusKind::CoreNotice);
    // Nothing modal opened for either.
    assert!(!app.global::<crate::Overlays>().get_dialog_open());
    assert_eq!(
        app.global::<crate::CoreUiView>().get_kind(),
        crate::CoreUiKind::None
    );
}

/// A background slot with a two-track playlist, the second track playing.
fn music_state(playing: bool) -> zaparoo_core::store::BackgroundMediaState {
    use zaparoo_core::media_types::{ActiveMediaInfo, PlaylistItem, PlaylistState};
    zaparoo_core::store::BackgroundMediaState {
        seeded: true,
        pushes_playback: false,
        track: Some(ActiveMediaInfo {
            slot: "background".into(),
            media_path: "/m/2.mp3".into(),
            media_name: "Two".into(),
            playback_state: if playing { "playing" } else { "paused" }.into(),
            launcher_controls: vec![
                "toggle_pause".into(),
                "rewind".into(),
                "fast_forward".into(),
            ],
            position_ms: Some(61_000),
            duration_ms: Some(180_000),
            ..ActiveMediaInfo::default()
        }),
        playlist: Some(std::sync::Arc::new(PlaylistState {
            id: "/m".into(),
            name: "Mix".into(),
            slot: "background".into(),
            repeat: "all".into(),
            items: vec![
                PlaylistItem {
                    name: "One".into(),
                    zap_script: "/m/1.mp3".into(),
                },
                PlaylistItem {
                    name: "Two".into(),
                    zap_script: "/m/2.mp3".into(),
                },
            ],
            index: 1,
            total: 2,
            playing,
            shuffle: Some(false),
        })),
    }
}

/// The Background music tile opens the player straight from the Settings root, the
/// saved screen follows it, and Back lands on the tile it came from.
#[test]
#[allow(
    clippy::expect_used,
    reason = "the tile is registered; fail fast if not"
)]
fn the_music_tile_opens_the_player_and_back_returns_to_it() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    let (_runtime, ctx) = offline_ctx();
    app.global::<crate::Motion>().set_enabled(false);
    crate::settings::enter(&ctx, &app, EntryMode::Fresh);
    let music_tile = crate::settings::root_pages()
        .iter()
        .position(|page| page.id == zaparoo_app::settings::MUSIC_PAGE)
        .expect("a Background music tile");
    app.global::<crate::SettingsView>()
        .set_index(i32::try_from(music_tile).unwrap_or(0));
    crate::router::handle_action(&ctx, &app, "accept");
    advance(zaparoo_app::input::PRESS_FEEDBACK_MS);
    settle(&window);
    let shell = app.global::<Shell>();
    assert_eq!(shell.get_active_screen(), Screen::Music);
    assert_eq!(
        crate::router::lock(&ctx.shared).persist.active_screen,
        "music"
    );
    assert!(
        !crate::router::lock(&ctx.shared)
            .persist
            .music
            .entered_from_hub
    );
    let view = app.global::<crate::MusicView>();
    assert!(!view.get_has_track(), "nothing has been heard from Core");
    assert_eq!(view.get_page(), crate::MusicPage::Player);
    assert_eq!(view.get_control_index(), 2, "focus starts on Play/Pause");

    crate::router::handle_action(&ctx, &app, "cancel");
    settle(&window);
    assert_eq!(shell.get_active_screen(), Screen::Settings);
    assert_eq!(
        app.global::<crate::SettingsView>().get_page(),
        SettingsPage::Root
    );
    assert_eq!(
        app.global::<crate::SettingsView>().get_index(),
        i32::try_from(music_tile).unwrap_or(-1),
        "seated on the Background music tile"
    );
    assert_eq!(
        crate::router::lock(&ctx.shared).persist.active_screen,
        "settings"
    );
}

/// A pinned Hub tile opens the player, and Back goes to the Hub.
#[test]
fn a_pinned_music_tile_returns_to_the_hub() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    let (_runtime, ctx) = offline_ctx();
    app.global::<crate::Motion>().set_enabled(false);
    crate::music::enter(&ctx, &app, crate::music::Origin::Hub);
    settle(&window);
    let shell = app.global::<Shell>();
    assert_eq!(shell.get_active_screen(), Screen::Music);
    crate::router::handle_action(&ctx, &app, "cancel");
    settle(&window);
    assert_eq!(shell.get_active_screen(), Screen::Hub);
    assert_eq!(
        crate::router::lock(&ctx.shared).persist.active_screen,
        "hub"
    );
}

/// The background slot drives the player and the header's music note; the
/// focus walks the transport row into the playlist and back.
#[test]
fn the_player_follows_the_background_slot_and_walks_into_the_playlist() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    let (_runtime, ctx) = offline_ctx();
    app.global::<crate::Motion>().set_enabled(false);
    crate::music::enter(&ctx, &app, crate::music::Origin::Settings);
    crate::music::background_changed(&ctx, &app, music_state(true));
    settle(&window);
    let view = app.global::<crate::MusicView>();
    assert!(view.get_has_track());
    assert_eq!(view.get_title(), "Two");
    assert_eq!(view.get_playlist_name(), "Mix");
    assert_eq!(view.get_track_number(), 2);
    assert_eq!(view.get_playback(), crate::PlaybackState::Playing);
    assert_eq!(view.get_position_text(), "1:01");
    assert_eq!(view.get_duration_text(), "3:00");
    assert_eq!(view.get_repeat(), crate::RepeatMode::All);
    assert!(app.global::<Shell>().get_music_playing());
    assert_eq!(
        view.get_up_next(),
        "One",
        "Repeat all comes around to the first"
    );

    // Left and Right walk the one row of controls.
    crate::router::handle_action(&ctx, &app, "right");
    assert_eq!(view.get_control_index(), 3);
    crate::router::handle_action(&ctx, &app, "left");
    assert_eq!(view.get_control_index(), 2);

    // Accept pushes the focused control through the shared press cue, as
    // every other control is pushed.
    app.global::<crate::Motion>().set_enabled(true);
    crate::router::handle_action(&ctx, &app, "accept");
    let feedback = app.global::<crate::PressFeedback>();
    assert_eq!(feedback.get_owner(), PressOwner::Music);
    assert_eq!(feedback.get_index(), 2);
    crate::press_feedback::cancel(&app);
    app.global::<crate::Motion>().set_enabled(false);

    // The Menu button puts focus on the chip row: the page's actions, with
    // what the slot cannot take left off (no Volume until Core answers).
    crate::router::handle_action(&ctx, &app, "page_menu");
    assert!(app.global::<crate::PageBar>().get_focused());
    assert_eq!(
        crate::page_bar::chip_ids(&app),
        ["music_playlist", "music_choose", "music_stop", "more"]
    );
    // Playlist opens the playlist as a page, on the playing track.
    crate::router::handle_action(&ctx, &app, "accept");
    assert!(!app.global::<crate::PageBar>().get_focused());
    assert_eq!(view.get_page(), crate::MusicPage::Playlist);
    assert_eq!(
        crate::router::lock(&ctx.shared).music_focus().1,
        1,
        "the playlist seats on the current track"
    );
    let marked: Vec<bool> = view.get_list_rows().iter().map(|r| r.now_playing).collect();
    assert_eq!(marked, vec![false, true], "the playing track is marked");

    // The marker follows the track when only the playlist index changes.
    let mut first = music_state(true);
    if let Some(list) = first.playlist.as_mut() {
        std::sync::Arc::make_mut(list).index = 0;
    }
    crate::music::background_changed(&ctx, &app, first);
    let marked: Vec<bool> = view.get_list_rows().iter().map(|r| r.now_playing).collect();
    assert_eq!(
        marked,
        vec![true, false],
        "the marker moved to the new track"
    );
    crate::music::background_changed(&ctx, &app, music_state(true));

    // Back returns to the player with its control kept.
    crate::router::handle_action(&ctx, &app, "cancel");
    assert_eq!(view.get_page(), crate::MusicPage::Player);
    assert_eq!(view.get_control_index(), 2);

    // Pausing shows at once and clears the header's note.
    crate::music::background_changed(&ctx, &app, music_state(false));
    assert_eq!(view.get_playback(), crate::PlaybackState::Paused);
    assert!(!app.global::<Shell>().get_music_playing());
}

/// A list panel's toggle rows: they take no press (the knob is the cue),
/// the help bar says Toggle, a changed value is written in place so the
/// knob slides, and a picker's current choice is marked.
#[test]
fn a_toggle_row_takes_no_press_and_updates_in_place() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    let (_runtime, ctx) = offline_ctx();
    app.global::<crate::Motion>().set_enabled(true);
    crate::music::enter(&ctx, &app, crate::music::Origin::Settings);
    crate::sizing::apply_scene(&app, crate::sizing::Scene::of(&app, 352.0, 240.0, false));
    crate::router::lock(&ctx.shared).music_settings_for_test(Some(70));
    settle(&window);
    crate::router::handle_action(&ctx, &app, "page_menu");
    let overlays = app.global::<crate::Overlays>();
    let rows = overlays.get_list_entries();
    let at = |id: &str| rows.iter().position(|e| e.id == id).unwrap_or(usize::MAX);
    let autoplay = at("music_autoplay");
    assert_ne!(autoplay, usize::MAX, "an autoplay row");
    assert_ne!(at("music_volume"), usize::MAX, "a volume row");
    assert_eq!(
        rows.row_data(autoplay).map(|e| e.role),
        Some(crate::MenuRole::ToggleOff)
    );
    overlays.set_list_index(i32::try_from(autoplay).unwrap_or(0));
    assert!(
        crate::press_feedback::current(&app).is_none(),
        "a toggle is not pushed in; its knob is the cue"
    );
    let volume = at("music_volume");
    overlays.set_list_index(i32::try_from(volume).unwrap_or(0));
    assert!(
        crate::press_feedback::current(&app).is_some(),
        "a row that opens something is"
    );

    // A changed value is written into the rows the panel already has.
    crate::router::lock(&ctx.shared).music_settings_for_test(Some(70));
    let before = overlays.get_list_entries();
    crate::music::list_accept(
        &ctx,
        &app,
        &crate::router::ListContext::MusicMenu,
        "music_autoplay",
    );
    assert_eq!(
        overlays.get_list_entries(),
        before,
        "the same model, rows updated in place"
    );
    assert_eq!(
        overlays
            .get_list_entries()
            .row_data(autoplay)
            .map(|e| e.role),
        Some(crate::MenuRole::ToggleOn)
    );
}

/// A track change reaches the card: the title, artist, album and Up next
/// follow the track Core reports, not the one the player opened on.
#[test]
fn a_track_change_updates_the_card() {
    use zaparoo_core::media_types::TrackInfo;
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    let (_runtime, ctx) = offline_ctx();
    app.global::<crate::Motion>().set_enabled(false);
    crate::music::enter(&ctx, &app, crate::music::Origin::Settings);
    let mut state = music_state(true);
    if let Some(track) = state.track.as_mut() {
        track.track = Some(TrackInfo {
            title: "Two".into(),
            artist: "Artist A".into(),
            album: "Album A".into(),
            ..TrackInfo::default()
        });
    }
    crate::music::background_changed(&ctx, &app, state);
    settle(&window);
    let view = app.global::<crate::MusicView>();
    assert_eq!(view.get_title(), "Two");
    assert_eq!(view.get_artist(), "Artist A");
    assert_eq!(view.get_album(), "Album A");
    assert_eq!(view.get_up_next(), "One");

    // Core moves on to the first item: a new track, a new index.
    let mut next = music_state(true);
    if let Some(track) = next.track.as_mut() {
        track.media_path = "/m/1.mp3".into();
        track.media_name = "One".into();
        track.track = Some(TrackInfo {
            title: "One".into(),
            artist: "Artist B".into(),
            album: "Album B".into(),
            ..TrackInfo::default()
        });
    }
    if let Some(list) = next.playlist.as_mut() {
        std::sync::Arc::make_mut(list).index = 0;
    }
    crate::music::background_changed(&ctx, &app, next);
    settle(&window);
    assert_eq!(view.get_title(), "One");
    assert_eq!(view.get_artist(), "Artist B");
    assert_eq!(view.get_album(), "Album B");
    assert_eq!(view.get_up_next(), "Two");
    assert_eq!(view.get_track_number(), 1);
}

/// Browse music is a deferred route on the same screen: the player stays
/// until the folder lands, a stale read is dropped, and Back from the
/// library's top returns to the player.
#[test]
fn the_library_keeps_the_player_until_its_folder_lands() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    let (_runtime, ctx) = offline_ctx();
    app.global::<crate::Motion>().set_enabled(false);
    crate::music::enter(&ctx, &app, crate::music::Origin::Settings);
    settle(&window);
    crate::router::handle_action(&ctx, &app, "page_menu");
    // Nothing plays: Browse music leads the chip row.
    assert_eq!(crate::page_bar::chip_ids(&app), ["music_choose", "more"]);
    crate::router::handle_action(&ctx, &app, "accept");
    let shell = app.global::<Shell>();
    assert!(
        shell.get_transitioning(),
        "the folder read is a deferred route"
    );
    assert!(
        app.global::<crate::PageBar>().get_focused(),
        "the chip keeps focus while the folder loads"
    );
    let view = app.global::<crate::MusicView>();
    assert_eq!(
        view.get_page(),
        crate::MusicPage::Player,
        "the player stays"
    );

    let stale = crate::router::lock(&ctx.shared).music_ticket();
    crate::router::lock(&ctx.shared).music_bump_ticket();
    crate::music::land_for_test(
        &ctx,
        &app,
        stale,
        Vec::new(),
        vec![(zaparoo_app::music::EntryKind::Route, "/music", "/music")],
    );
    assert_eq!(
        view.get_page(),
        crate::MusicPage::Player,
        "a stale read is dropped"
    );

    let ticket = crate::router::lock(&ctx.shared).music_ticket();
    crate::music::land_for_test(
        &ctx,
        &app,
        ticket,
        Vec::new(),
        vec![
            (zaparoo_app::music::EntryKind::Route, "/music", "/music"),
            (zaparoo_app::music::EntryKind::Route, "/audio", "/audio"),
        ],
    );
    settle(&window);
    assert!(!shell.get_transitioning());
    assert!(
        !app.global::<crate::PageBar>().get_focused(),
        "the route's commit gives focus to the folder"
    );
    assert_eq!(view.get_page(), crate::MusicPage::Library);
    assert_eq!(view.get_count(), 2);
    assert!(view.get_library_top());
    assert_eq!(
        crate::router::lock(&ctx.shared).persist.music.page,
        "library"
    );

    crate::router::handle_action(&ctx, &app, "cancel");
    settle(&window);
    assert_eq!(view.get_page(), crate::MusicPage::Player);
    assert_eq!(shell.get_active_screen(), Screen::Music);
}

/// Where the scene has no chip row (240p), the Menu button opens the Menu
/// list, and the volume is a slider row in it: Left and Right adjust it in
/// place, Accept does nothing, and the rows are the ones the panel had.
#[test]
#[allow(
    clippy::expect_used,
    reason = "a fixed Menu that always has a Volume row"
)]
fn without_a_chip_row_the_menu_list_holds_the_volume_slider() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    let (_runtime, ctx) = offline_ctx();
    app.global::<crate::Motion>().set_enabled(false);
    crate::sizing::apply_scene(&app, crate::sizing::Scene::of(&app, 352.0, 240.0, false));
    crate::music::enter(&ctx, &app, crate::music::Origin::Settings);
    crate::music::background_changed(&ctx, &app, music_state(true));
    crate::router::lock(&ctx.shared).music_settings_for_test(Some(70));
    settle(&window);
    assert!(!app.global::<Sizing>().get_menu_row());
    crate::router::handle_action(&ctx, &app, "page_menu");
    assert!(!app.global::<crate::PageBar>().get_focused());
    let overlays = app.global::<crate::Overlays>();
    assert!(overlays.get_list_open());
    let rows = overlays.get_list_entries();
    let volume = rows
        .iter()
        .position(|e| e.id == "music_volume")
        .expect("a Volume row");
    assert_eq!(
        rows.row_data(volume).map(|e| e.role),
        Some(crate::MenuRole::Slider)
    );
    overlays.set_list_index(i32::try_from(volume).unwrap_or(0));
    let detail = |overlays: &crate::Overlays<'_>| {
        overlays
            .get_list_entries()
            .row_data(volume)
            .map(|e| e.detail.to_string())
            .unwrap_or_default()
    };
    assert_eq!(detail(&overlays), "70");

    crate::router::handle_action(&ctx, &app, "right");
    assert_eq!(detail(&overlays), "80", "Right steps up by ten");
    assert_eq!(
        overlays.get_list_entries(),
        rows,
        "the rows were updated in place"
    );
    crate::router::handle_action(&ctx, &app, "left");
    crate::router::handle_action(&ctx, &app, "left");
    assert_eq!(detail(&overlays), "60");
    crate::router::handle_action(&ctx, &app, "accept");
    assert!(overlays.get_list_open(), "Accept does nothing on a slider");
    assert_eq!(detail(&overlays), "60");
    assert_eq!(
        crate::router::lock(&ctx.shared).music_volume_for_test(),
        Some(60)
    );
    crate::router::handle_action(&ctx, &app, "cancel");
    assert!(!overlays.get_list_open());
}

/// Above 240p the Volume chip opens a panel holding the slider alone.
#[test]
fn the_volume_chip_opens_the_slider_on_its_own() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    let (_runtime, ctx) = offline_ctx();
    app.global::<crate::Motion>().set_enabled(false);
    crate::sizing::apply_scene(&app, crate::sizing::Scene::of(&app, 1280.0, 720.0, false));
    crate::music::enter(&ctx, &app, crate::music::Origin::Settings);
    crate::router::lock(&ctx.shared).music_settings_for_test(Some(70));
    // The chip shows once Core's settings are known, at the next render.
    crate::music::background_changed(&ctx, &app, music_state(true));
    settle(&window);
    crate::router::handle_action(&ctx, &app, "page_menu");
    let ids = crate::page_bar::chip_ids(&app);
    let at = ids.iter().position(|id| id == "music_volume");
    assert!(at.is_some(), "a Volume chip: {ids:?}");
    for _ in 0..at.unwrap_or(0) {
        crate::router::handle_action(&ctx, &app, "right");
    }
    crate::router::handle_action(&ctx, &app, "accept");
    let overlays = app.global::<crate::Overlays>();
    assert!(overlays.get_list_open());
    assert_eq!(overlays.get_list_title(), "title:volume");
    let rows = overlays.get_list_entries();
    assert_eq!(rows.row_count(), 1);
    assert_eq!(
        rows.row_data(0).map(|e| e.role),
        Some(crate::MenuRole::Slider)
    );
    crate::router::handle_action(&ctx, &app, "right");
    assert_eq!(
        overlays
            .get_list_entries()
            .row_data(0)
            .map(|e| e.detail.to_string()),
        Some("80".to_string())
    );
    crate::router::handle_action(&ctx, &app, "cancel");
    assert!(!overlays.get_list_open(), "Back closes it");
}

/// A cold start on the player waits under the curtain for Core's first
/// answer, then comes back on the saved playlist row.
#[test]
fn a_restored_player_waits_for_core_and_seats_the_saved_row() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    let (_runtime, ctx) = offline_ctx();
    app.global::<crate::Motion>().set_enabled(false);
    {
        let mut shared = crate::router::lock(&ctx.shared);
        shared.persist.active_screen = "music".into();
        shared.persist.music = zaparoo_core::persist::MusicState {
            page: "playlist".into(),
            control: "next".into(),
            playlist_index: 0,
            playlist_item: "/m/1.mp3".into(),
            entered_from_hub: false,
            ..zaparoo_core::persist::MusicState::default()
        };
    }
    let shell = app.global::<Shell>();
    shell.set_boot_curtain(true);
    crate::music::restore(&ctx, &app);
    assert!(shell.get_transitioning());
    assert!(shell.get_boot_curtain(), "nothing shows until Core answers");
    crate::music::restore_landed(&ctx, &app, music_state(true));
    settle(&window);
    assert!(!shell.get_boot_curtain());
    assert_eq!(shell.get_active_screen(), Screen::Music);
    let (control, row) = crate::router::lock(&ctx.shared).music_focus();
    assert_eq!(
        app.global::<crate::MusicView>().get_page(),
        crate::MusicPage::Playlist,
        "the playlist page comes back"
    );
    assert_eq!(row, 0, "the saved row still holds the saved track");
    assert_eq!(control, zaparoo_app::music::Control::Next);
}

/// A Settings root tile pins to the Hub from its Options, opens its page
/// from there, and Back returns to the Hub.
#[test]
#[allow(
    clippy::expect_used,
    reason = "the tile is registered; fail fast if not"
)]
fn a_settings_tile_pins_to_the_hub_and_opens_its_page_from_there() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    let (_runtime, ctx) = offline_ctx();
    app.global::<crate::Motion>().set_enabled(false);
    crate::settings::enter(&ctx, &app, EntryMode::Fresh);
    let display = crate::settings::root_pages()
        .iter()
        .position(|page| page.id == "pageDisplayInterface")
        .expect("a Display tile");
    app.global::<crate::SettingsView>()
        .set_index(i32::try_from(display).unwrap_or(0));
    crate::router::handle_action(&ctx, &app, "context_menu");
    let overlays = app.global::<crate::Overlays>();
    assert!(overlays.get_list_open());
    crate::router::handle_action(&ctx, &app, "accept");
    assert!(crate::hub::has_setting(
        &crate::router::lock(&ctx.shared),
        "pageDisplayInterface"
    ));

    crate::settings::enter_page_from_hub(&ctx, &app, SettingsPage::Display);
    settle(&window);
    assert_eq!(app.global::<Shell>().get_active_screen(), Screen::Settings);
    assert_eq!(
        app.global::<crate::SettingsView>().get_page(),
        SettingsPage::Display
    );
    crate::router::handle_action(&ctx, &app, "cancel");
    settle(&window);
    assert_eq!(app.global::<Shell>().get_active_screen(), Screen::Hub);
}

fn position_page(
    rows: std::ops::Range<usize>,
    next: Option<&str>,
    offset: Option<u64>,
    anchor_index: Option<u64>,
    total: usize,
) -> zaparoo_core::media_types::MediaBrowseResult {
    zaparoo_core::media_types::MediaBrowseResult {
        entries: rows
            .map(|index| zaparoo_core::media_types::BrowseEntry {
                name: format!("Row {index}"),
                path: format!("/target/{index}"),
                entry_type: "media".into(),
                ..Default::default()
            })
            .collect(),
        total_files: u32::try_from(total).unwrap_or(0),
        total_dirs: Some(0),
        pagination: Some(zaparoo_core::media_types::Pagination {
            has_next_page: next.is_some(),
            next_cursor: next.map(str::to_string),
            offset,
            anchor_index,
            ..Default::default()
        }),
        ..Default::default()
    }
}

/// A 1,000-row folder whose first 50 rows have answered, from a Core that
/// serves pages by position.
fn seat_position_list(ctx: &crate::router::Ctx, app: &App, layout: &str, saved: &str) -> u64 {
    crate::sizing::apply_scene(
        app,
        crate::sizing::Scene::of(app, f64::from(W), f64::from(H), false),
    );
    app.global::<Shell>().set_active_screen(Screen::Games);
    let ticket = {
        let mut shared = crate::router::lock(&ctx.shared);
        shared.persist.settings.games_browse_layout = layout.into();
        shared.persist.games.path_stack = vec!["/target".into()];
        shared.persist.games.selected_at_level = vec![saved.into()];
        shared.games.browse_path = "/target".into();
        shared.games.total_known = true;
        shared.games.loading = true;
        shared.games.ticket
    };
    crate::games::on_browse_ready(
        ctx,
        app,
        ticket,
        &position_page(0..50, Some("first"), Some(0), None, 1000),
        false,
        false,
        false,
    );
    ticket
}

#[test]
fn core_without_positions_keeps_the_walk() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, _window) = boot();
    let (_runtime, ctx) = offline_ctx();
    crate::sizing::apply_scene(
        &app,
        crate::sizing::Scene::of(&app, f64::from(W), f64::from(H), false),
    );
    let ticket = {
        let mut shared = crate::router::lock(&ctx.shared);
        shared.games.browse_path = "/target".into();
        shared.games.total_known = true;
        shared.games.ticket
    };
    crate::games::on_browse_ready(
        &ctx,
        &app,
        ticket,
        &position_page(0..50, Some("first"), None, None, 1000),
        false,
        false,
        false,
    );
    let shared = crate::router::lock(&ctx.shared);
    assert!(!shared.games.rows.is_positional());
    assert_eq!(shared.games.rows.len(), 50);
    assert!(shared.games.has_more());
}

#[test]
fn a_folder_that_fits_one_page_is_not_positional() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, _window) = boot();
    let (_runtime, ctx) = offline_ctx();
    let ticket = crate::router::lock(&ctx.shared).games.ticket;
    crate::games::on_browse_ready(
        &ctx,
        &app,
        ticket,
        &position_page(0..20, None, Some(0), None, 20),
        false,
        false,
        false,
    );
    assert!(!crate::router::lock(&ctx.shared).games.rows.is_positional());
}

#[test]
fn a_positional_list_knows_its_whole_length_from_the_first_page() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, _window) = boot();
    let (_runtime, ctx) = offline_ctx();
    seat_position_list(&ctx, &app, "grid", "");
    let shared = crate::router::lock(&ctx.shared);
    assert!(shared.games.rows.is_positional());
    assert_eq!(shared.games.rows.len(), 1000);
    assert_eq!(shared.games.grid.current_index(), 0);
    assert!(!shared.games.has_more());
}

#[test]
fn a_step_past_the_loaded_rows_waits_and_lands_when_they_arrive() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, _window) = boot();
    let (_runtime, ctx) = offline_ctx();
    let ticket = seat_position_list(&ctx, &app, "list", "");
    {
        let mut shared = crate::router::lock(&ctx.shared);
        shared.games.grid.set_current_index_immediate(49);
        shared.games.loading_more = false;
    }
    crate::games::handle_action(&ctx, &app, "down");
    {
        let shared = crate::router::lock(&ctx.shared);
        assert_eq!(
            shared.games.grid.current_index(),
            49,
            "no move onto nothing"
        );
        assert_eq!(shared.games.landing.map(|landing| landing.index), Some(50));
        assert!(shared.games.loading_more);
    }
    crate::games::on_positional_page(
        &ctx,
        &app,
        ticket,
        50,
        Ok(position_page(50..100, Some("next"), None, None, 1000)),
    );
    let shared = crate::router::lock(&ctx.shared);
    assert_eq!(shared.games.grid.current_index(), 50);
    assert!(shared.games.landing.is_none());
    assert_eq!(shared.games.rows[50].path, "/target/50");
}

#[test]
fn a_letter_jump_asks_once_for_its_page_and_lands_there() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, _window) = boot();
    let (_runtime, ctx) = offline_ctx();
    let ticket = seat_position_list(&ctx, &app, "grid", "");
    crate::router::lock(&ctx.shared).games.loading_more = false;
    crate::games::jump_to_item(&ctx, &app, 500);
    let start = {
        let shared = crate::router::lock(&ctx.shared);
        assert_eq!(shared.games.grid.current_index(), 0, "the source stays up");
        assert!(shared.games.jump_loading);
        assert!(shared.games.loading_more);
        let size = shared.games.grid.page_size();
        500 / size * size
    };
    crate::games::on_positional_page(
        &ctx,
        &app,
        ticket,
        start,
        Ok(position_page(
            start..start + 50,
            Some("next"),
            None,
            None,
            1000,
        )),
    );
    let shared = crate::router::lock(&ctx.shared);
    assert_eq!(shared.games.grid.current_index(), 500);
    assert!(!shared.games.jump_loading);
    assert_eq!(shared.persist.games.selected_at_level, vec!["/target/500"]);
    assert!(
        shared.games.rows.get(100).is_none(),
        "nothing between the two pages was loaded"
    );
}

#[test]
fn rows_dropped_for_memory_load_again_when_moved_back_to() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, _window) = boot();
    let (_runtime, ctx) = offline_ctx();
    let ticket = seat_position_list(&ctx, &app, "list", "");
    crate::router::lock(&ctx.shared).games.loading_more = false;
    crate::games::jump_to_item(&ctx, &app, 500);
    crate::games::on_positional_page(
        &ctx,
        &app,
        ticket,
        450,
        Ok(position_page(450..550, Some("next"), None, None, 1000)),
    );
    {
        let mut shared = crate::router::lock(&ctx.shared);
        assert_eq!(shared.games.grid.current_index(), 500);
        shared.games.rows.evict(450..550, 0, 50, |_| 1);
        assert!(shared.games.rows.get(0).is_none());
        shared.games.loading_more = false;
    }
    crate::games::jump_to_item(&ctx, &app, 0);
    {
        let shared = crate::router::lock(&ctx.shared);
        assert_eq!(shared.games.grid.current_index(), 500);
        assert!(shared.games.loading_more);
    }
    crate::games::on_positional_page(
        &ctx,
        &app,
        ticket,
        0,
        Ok(position_page(0..50, Some("first"), Some(0), None, 1000)),
    );
    let shared = crate::router::lock(&ctx.shared);
    assert_eq!(shared.games.grid.current_index(), 0);
    assert_eq!(shared.games.rows[0].path, "/target/0");
}

#[test]
fn a_failed_position_page_stops_until_the_next_move_retries_it() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, _window) = boot();
    let (_runtime, ctx) = offline_ctx();
    let ticket = seat_position_list(&ctx, &app, "list", "");
    crate::router::lock(&ctx.shared)
        .games
        .grid
        .set_current_index_immediate(49);
    crate::games::handle_action(&ctx, &app, "down");
    crate::games::on_positional_page(
        &ctx,
        &app,
        ticket,
        50,
        Err(crate::games::PageError::transport("not connected")),
    );
    {
        let shared = crate::router::lock(&ctx.shared);
        assert_eq!(shared.games.ticket, ticket);
        assert!(shared.games.page_blocked);
        assert!(!shared.games.loading_more, "no retry loop");
        assert!(shared.games.landing.is_none());
        assert_eq!(shared.games.grid.current_index(), 49);
    }
    crate::games::handle_action(&ctx, &app, "down");
    {
        let shared = crate::router::lock(&ctx.shared);
        assert!(!shared.games.page_blocked);
        assert!(shared.games.loading_more);
    }
    crate::games::on_positional_page(
        &ctx,
        &app,
        ticket,
        50,
        Err(crate::games::PageError::refused("no")),
    );
    let shared = crate::router::lock(&ctx.shared);
    assert_ne!(shared.games.ticket, ticket, "Core refused: reload the list");
    assert!(shared.games.loading);
    assert!(shared.games.page_restarted);
}

#[test]
fn a_late_position_page_is_dropped() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, _window) = boot();
    let (_runtime, ctx) = offline_ctx();
    let ticket = seat_position_list(&ctx, &app, "grid", "");
    crate::games::on_positional_page(
        &ctx,
        &app,
        ticket + 1,
        500,
        Ok(position_page(500..550, Some("next"), None, None, 1000)),
    );
    assert!(crate::router::lock(&ctx.shared)
        .games
        .rows
        .get(500)
        .is_none());
}

#[test]
fn a_list_shorter_than_reported_ends_where_core_says_it_does() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, _window) = boot();
    let (_runtime, ctx) = offline_ctx();
    let ticket = seat_position_list(&ctx, &app, "list", "");
    crate::router::lock(&ctx.shared).games.loading_more = false;
    crate::games::jump_to_item(&ctx, &app, 900);
    crate::games::on_positional_page(
        &ctx,
        &app,
        ticket,
        850,
        Ok(position_page(850..870, None, None, None, 870)),
    );
    let shared = crate::router::lock(&ctx.shared);
    assert_eq!(shared.games.rows.len(), 870);
    assert_eq!(shared.games.grid.current_index(), 869);
    assert!(shared.games.landing.is_none());
}

#[test]
fn a_restore_off_the_first_page_holds_the_source_for_one_request_and_lands() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, _window) = boot();
    let (_runtime, ctx) = offline_ctx();
    let ticket = seat_position_list(&ctx, &app, "grid", "/target/700");
    {
        let shared = crate::router::lock(&ctx.shared);
        assert!(shared.games.loading, "the anchor request is out");
        assert!(
            shared.games.rows.is_empty(),
            "nothing lands at the top first"
        );
    }
    let first = crate::games::FirstPage {
        rows: crate::games::browse_rows(
            &position_page(0..50, None, None, None, 1000).entries,
            false,
        ),
        cursor: Some("first".into()),
        totals: Some((1000, 0)),
        total: 1000,
        pinned: 0,
        flip: false,
    };
    crate::games::on_restore_page(
        &ctx,
        &app,
        ticket,
        first,
        None,
        Ok(Some(position_page(
            675..725,
            Some("next"),
            Some(675),
            Some(700),
            1000,
        ))),
    );
    let shared = crate::router::lock(&ctx.shared);
    assert!(!shared.games.loading);
    assert_eq!(shared.games.grid.current_index(), 700);
    assert_eq!(shared.games.rows[700].path, "/target/700");
    assert_eq!(shared.games.rows[0].path, "/target/0");
    assert_eq!(shared.games.rows.len(), 1000);
}

#[test]
fn a_saved_row_core_no_longer_lists_stays_at_the_top_or_its_old_position() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, _window) = boot();
    let (_runtime, ctx) = offline_ctx();
    let first = |total: usize| crate::games::FirstPage {
        rows: crate::games::browse_rows(
            &position_page(0..50, None, None, None, total).entries,
            false,
        ),
        cursor: Some("first".into()),
        totals: Some((u32::try_from(total).unwrap_or(0), 0)),
        total,
        pinned: 0,
        flip: false,
    };
    let ticket = seat_position_list(&ctx, &app, "grid", "/gone");
    crate::games::on_restore_page(&ctx, &app, ticket, first(1000), None, Ok(None));
    assert_eq!(
        crate::router::lock(&ctx.shared).games.grid.current_index(),
        0
    );

    // A reload: the game went away, and the list came back shorter.
    crate::games::on_restore_page(
        &ctx,
        &app,
        ticket,
        first(600),
        Some(700),
        Ok(Some(position_page(575..600, None, Some(575), None, 600))),
    );
    let shared = crate::router::lock(&ctx.shared);
    assert_eq!(shared.games.rows.len(), 600);
    assert_eq!(shared.games.grid.current_index(), 599);
}

#[test]
fn a_failed_lookup_on_a_reload_lands_the_first_page_and_keeps_the_saved_row() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, _window) = boot();
    let (runtime, ctx) = offline_ctx();
    let ticket = seat_position_list(&ctx, &app, "grid", "/target/700");
    let failure = runtime.block_on(
        ctx.store
            .client()
            .media_browse(zaparoo_core::media_types::MediaBrowseParams::default()),
    );
    assert!(failure.is_err());
    let first = crate::games::FirstPage {
        rows: crate::games::browse_rows(
            &position_page(0..50, None, None, None, 1000).entries,
            false,
        ),
        cursor: Some("first".into()),
        totals: Some((1000, 0)),
        total: 1000,
        pinned: 0,
        flip: false,
    };
    crate::games::on_restore_page(&ctx, &app, ticket, first, Some(700), failure.map(Some));
    let shared = crate::router::lock(&ctx.shared);
    assert!(!shared.games.loading);
    assert!(shared.games.error.is_empty());
    assert_eq!(shared.games.rows.len(), 1000);
    assert_eq!(shared.games.rows[0].path, "/target/0");
    assert_eq!(shared.games.grid.current_index(), 0);
    assert_eq!(
        shared.persist.games.selected_at_level,
        vec!["/target/700".to_string()],
        "a failed lookup is not proof the game is gone"
    );
}

#[test]
fn a_rescan_reloads_an_open_list_and_marks_an_unseen_one() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, _window) = boot();
    let (_runtime, ctx) = offline_ctx();
    let ticket = seat_position_list(&ctx, &app, "grid", "");
    app.global::<Shell>().set_active_screen(Screen::Hub);
    crate::games::reload_positional(&ctx, &app);
    {
        let shared = crate::router::lock(&ctx.shared);
        assert_eq!(shared.games.ticket, ticket, "nothing loads off screen");
        assert!(shared.games.positional_stale);
    }
    app.global::<Shell>().set_active_screen(Screen::Games);
    crate::router::lock(&ctx.shared)
        .games
        .grid
        .set_current_index_immediate(30);
    crate::games::reload_positional(&ctx, &app);
    let shared = crate::router::lock(&ctx.shared);
    assert_ne!(shared.games.ticket, ticket);
    assert_eq!(shared.games.reload_position, Some(30));
    assert!(!shared.games.positional_stale);
    assert_eq!(shared.games.rows.len(), 1000, "the rows stay up");
}

#[test]
fn a_picker_opened_below_its_first_window_paints_the_fill_at_once() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    crate::sizing::apply_scene(
        &app,
        crate::sizing::Scene::of(&app, f64::from(W), f64::from(H), false),
    );
    app.global::<Shell>().set_active_screen(Screen::None);
    let magenta = slint::Rgb8Pixel {
        r: 255,
        g: 0,
        b: 255,
    };
    app.global::<crate::Theme>()
        .set_selection_fill(slint::Color::from_rgb_u8(magenta.r, magenta.g, magenta.b));
    let ov = app.global::<crate::Overlays>();
    let filled = |window: &Rc<MinimalSoftwareWindow>| {
        pixels(window)
            .iter()
            .filter(|p| p.0 == Rgb565Pixel::from(magenta).0)
            .count()
    };
    // Opened the way Settings opens a picker: rows, then the current
    // value, then open. The rows past the first window scroll the list as
    // it is built, and the fill has to follow on the first frame.
    for (index, current) in [(0usize, 0), (3, 3), (10, 10), (12, 12)] {
        let entries: Vec<crate::MenuEntry> = (0..13)
            .map(|i| crate::MenuEntry {
                role: crate::MenuRole::default(),
                detail_key: SharedString::default(),
                id: i.to_string().into(),
                label: format!("Row {i}").into(),
                label_key: "".into(),
                enabled: true,
                reason_key: "".into(),
                detail: "".into(),
            })
            .collect();
        ov.set_list_entries(ModelRc::new(VecModel::from(crate::router::mark_chosen(
            entries, index,
        ))));
        ov.set_list_index(current);
        ov.set_list_open(true);
        let first = filled(&window);
        settle(&window);
        assert!(first > 0, "row {index}: no fill on the first frame");
        assert_eq!(
            first,
            filled(&window),
            "row {index}: the fill moved after opening"
        );
        ov.set_list_open(false);
        settle(&window);
    }
}
