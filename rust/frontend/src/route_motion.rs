// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

//! Motion and navigation checked with software-rendered frames on a stepped
//! clock. Assert coherent source/destination composition, local cursor motion,
//! visible pushes before dispatch, command ownership, and eventual quiescence.

use crate::navigation::EntryMode;

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
        image_type: None,
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
    crate::router::lock(&ctx.shared).games.rapid_active = true;
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
    assert_eq!(buttons.get_view(), "FaceNorth");
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
    assert_eq!(buttons.get_view(), "FaceWest");
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
    shell.set_saver_armed(false);
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
    shell.set_saver_armed(false);
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
        state.games.rows = game_rows("Game", 200);
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
    crate::input::dispatch_repeat(&ctx, &app, "page_next", HoldTier::Page);
    assert_eq!(
        crate::router::lock(&ctx.shared).games.grid.current_page(),
        first + 1,
        "new input must interrupt an in-flight page instead of waiting"
    );
    let first = first + 1;
    distinct_frames(&window, 18);
    assert!(!crate::router::lock(&ctx.shared).games.sliding);
    for expected in first + 1..=first + 5 {
        crate::input::dispatch_repeat(&ctx, &app, "page_next", HoldTier::Page);
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
    for (index, page) in zaparoo_app::settings::PAGES.iter().enumerate() {
        settings.set_index(index as i32);
        settle(&window);
        crate::settings::handle_action(&ctx, &app, "accept");
        assert_eq!(
            Ok(settings.get_page()),
            SettingsPage::try_from(page.id),
            "Settings is synchronous"
        );
        assert!(!shell.get_transitioning());
        crate::settings::handle_action(&ctx, &app, "cancel");
        assert_eq!(settings.get_page(), SettingsPage::Root);
        assert_eq!(
            settings.get_index(),
            index as i32,
            "Back restores the category tile"
        );
    }
    crate::settings::open_page(&ctx, &app, SettingsPage::Library);
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
    shared.games.rows = game_rows("Game", 3);
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

#[test]
fn hiding_restarts_browse_and_letters_without_losing_favorites_or_neighbor_focus() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, _window) = boot();
    let (_runtime, ctx) = offline_ctx();
    let target = seat_visibility_list(&ctx, &app, GamesMode::Browse);
    crate::games::on_hidden_updated(&ctx, &app, 0, &target, "NES", Ok(visibility_reply(true)));
    let shared = crate::router::lock(&ctx.shared);
    assert!(shared.games.rows[0].is_hidden);
    assert!(shared.games.rows[0].is_favorite);
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
    view.set_page(SettingsPage::Library);
    let rows = zaparoo_app::settings::page_rows(
        SettingsPage::Library.token(),
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
    shared.games.rows = game_rows(name, 8);
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
            image_type: None,
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
    crate::router::lock(&ctx.shared).games.rapid_active = true;
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
                image_type: None,
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
        !crate::router::lock(&ctx.shared).games.sliding,
        "a partial nonterminal page must wait"
    );
    crate::games::on_append(&ctx, &app, 0, Ok((game_rows("Last", size - 1), None)));
    assert!(crate::router::lock(&ctx.shared).games.sliding);
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
        assert!(crate::router::lock(&ctx.shared).games.sliding);
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
    assert!(!crate::router::lock(&ctx.shared).games.sliding);
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
        shared.games.rows = game_rows("Game", count);
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
            assert!(crate::router::lock(&ctx.shared).games.sliding);
            assert!(distinct_frames(&window, 18) > 5);
            assert_eq!(view.get_page(), if direction < 0 { 2 } else { 0 });
        }
    }
    app.global::<crate::Motion>().set_enabled(false);
    crate::router::handle_action(&ctx, &app, "page_prev");
    assert_eq!(view.get_page(), 2);
    assert_eq!(view.get_slide_dir(), -1);
    assert!(
        !crate::router::lock(&ctx.shared).games.sliding,
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
        shared.games.rows = game_rows("Game", size * 2);
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
    assert!(!crate::router::lock(&ctx.shared).games.sliding);
    // A tap still wraps, continuing forward rather than reversing to page zero.
    crate::router::handle_action(&ctx, &app, "page_next");
    assert!(crate::router::lock(&ctx.shared).games.sliding);
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
        shared.games.rows = game_rows("Game", 60);
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
        shared.games.rows = game_rows("Launch", 2);
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
fn a_settings_move_that_scrolls_glides_the_band_and_snaps_without_motion() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    crate::sizing::apply_scene(
        &app,
        crate::sizing::Scene::of(&app, f64::from(W), f64::from(H), false),
    );
    app.global::<Shell>().set_active_screen(Screen::Settings);
    let settings = app.global::<crate::SettingsView>();
    settings.set_page(SettingsPage::Appearance);
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
    info.set_rows(ModelRc::new(VecModel::from(vec![crate::DetailRow {
        key: "system".into(),
        value: "SNES".into(),
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
    app.global::<Shell>().set_active_screen(Screen::None);
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
    assert!(crate::router::lock(&ctx.shared).hub.sliding);
    advance(32);
    app.global::<crate::HubInput>().invoke_page_requested(1);
    assert_eq!(view.get_page(), 0);
    assert!(
        !crate::router::lock(&ctx.shared).hub.sliding,
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
    assert!(!crate::router::lock(&ctx.shared).hub.sliding);
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
        assert!(!crate::router::lock(&ctx.shared).hub.sliding);
        settle(&window);
        assert_eq!(view.get_page(), 1);
    });
    crate::set_dormant(&ctx, &app, false);
    crate::router::handle_action(&ctx, &app, "page_next");
    assert_eq!(view.get_page(), 2);
    assert!(
        !crate::router::lock(&ctx.shared).hub.sliding,
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
    assert!(crate::router::lock(&ctx.shared).hub.sliding);
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
    assert!(!shared.hub.sliding);
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
    // One cell gets a top label in a color nothing else paints, so its
    // position can be read straight off the frame. Every
    // `PressableSurface` uses `borderMid` and `tileEdge`, so neither of
    // those singles a tile out, and glyphs do not render here at all --
    // the provider callback belongs to the real app, not to `boot`.
    crate::fonts::register_embedded_fonts();
    app.global::<crate::Theme>()
        .set_text_label(slint::Color::from_rgb_u8(255, 0, 255));
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
                top_label: if entry == 0 {
                    "X".into()
                } else {
                    SharedString::default()
                },
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
        shared.games.rows = game_rows("Game", 40);
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
        shared.games.rows = game_rows("Game", 40);
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
        crate::systems::enter_favorites(&ctx, &app, EntryMode::Restore);
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
        advance(32);
        assert_eq!(
            shell.get_active_screen(),
            screen,
            "fully depressed source must remain visible"
        );
        advance(zaparoo_app::input::PRESS_FEEDBACK_MS - 80);
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
        assert_eq!(view.get_page(), SettingsPage::Appearance);
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
    assert_eq!(view.get_page(), SettingsPage::Appearance);
    app.invoke_input_lost();
    assert_eq!(
        view.get_page(),
        SettingsPage::Appearance,
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
        shared.games.rows = rows;
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
        let enter = if mode == GamesMode::Favorites {
            crate::games::enter_favorites
        } else {
            crate::games::enter_recents
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
                    crate::router::lock(&ctx.shared).games.favorites_system,
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
                crate::games::refresh_favorites(&ctx, &app);
            } else {
                crate::games::refresh_recents(&ctx, &app);
            }
            let ticket = crate::router::lock(&ctx.shared).games.ticket;
            crate::games::apply_fill(&ctx, &app, ticket, navigation_rows(), None, None, false);
            assert_eq!(view.get_current_index(), index);
            assert_eq!(view.get_list_scroll_top(), top);
        }
    }
    crate::games::enter_favorites_for_system(&ctx, &app, "System12");
    let ticket = crate::router::lock(&ctx.shared).games.ticket;
    crate::games::apply_fill(&ctx, &app, ticket, navigation_rows(), None, None, true);
    assert_eq!(app.global::<crate::GamesView>().get_current_index(), 0);
    assert_eq!(
        crate::router::lock(&ctx.shared).games.favorites_system,
        "System12"
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
    crate::systems::enter_favorites(&ctx, &app, EntryMode::Fresh);
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
    crate::systems::return_to_favorites(&ctx, &app);
    assert_eq!(view.get_current_index(), 8);
    assert_eq!(view.get_list_scroll_top(), 6);
    crate::systems::enter_favorites(&ctx, &app, EntryMode::Fresh);
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
        shared.categories = vec!["Console".into()];
        shared.persist.hub.category = "Console".into();
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
fn the_search_pane_lists_a_whole_page_with_each_title_once() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, _window) = boot();
    let (_runtime, ctx) = offline_ctx();
    crate::search::enter(&ctx, &app, EntryMode::Fresh);
    let view = app.global::<crate::SearchView>();
    crate::search::type_char(&ctx, &app, 'g');
    let seq = crate::search::preview_seq(&crate::router::lock(&ctx.shared));

    // Three variants of one title, then enough others to pass the old cap.
    let titles: Vec<String> = (0..20).map(|n| format!("Game {n:02}")).collect();
    let mut names = vec!["Game 00", "Game 00"];
    names.extend(titles.iter().map(String::as_str));
    let mut result = search_result(&names, false);
    for (index, item) in result.results.iter_mut().enumerate() {
        item.path = format!("/games/{index}");
    }
    crate::search::preview_landed(&ctx, &app, seq, Ok(&result));
    assert_eq!(view.get_pane_rows().row_count(), 20);
    assert_eq!(view.get_count(), 22, "the heading counts every match");

    // The one row for the title opens the results on its first variant.
    crate::router::handle_action(&ctx, &app, "right");
    crate::router::handle_action(&ctx, &app, "accept");
    assert_eq!(
        crate::router::lock(&ctx.shared)
            .persist
            .search
            .selected_path,
        "/games/0"
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
        ("add_to_hub", crate::AppCue::AddedToHub, 1),
    ] {
        crate::router::handle_action(&ctx, &app, "page_menu");
        assert!(overlays.get_list_open());
        let row = overlays.get_list_entries().row_data(0).expect("one row");
        assert_eq!(
            (row.id.as_str(), row.label_key.as_str()),
            ("add_to_hub", key)
        );
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
    }
    shell.set_active_screen(Screen::Games);

    crate::search::enter_scoped(&ctx, &app);
    assert_eq!(shell.get_active_screen(), Screen::Search);
    let view = app.global::<crate::SearchView>();
    assert!(view.get_scoped());
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
    }
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
    // The pick is kept at once, and Clear appears as the foot's action.
    assert_eq!(ids(&app), ["cat:genre", "filter_clear"]);
    let rows = overlays.get_list_entries();
    assert_eq!(rows.row_data(0).map(|e| e.detail), Some("Racing".into()));
    assert_eq!(
        rows.row_data(1).map(|e| e.role),
        Some(crate::MenuRole::Action)
    );
    assert_eq!(
        crate::router::lock(&ctx.shared).persist.search.tags,
        ["genre:racing"]
    );

    // Back leaves with the pick, rather than discarding it.
    crate::router::handle_action(&ctx, &app, "cancel");
    assert!(!overlays.get_list_open());
    assert_eq!(view.get_filter_text(), "Racing");
    assert_eq!(
        crate::router::lock(&ctx.shared).persist.search.tags,
        ["genre:racing"]
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
fn a_hidden_game_shown_among_the_rest_is_labelled_hidden() {
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
    assert!(!view.get_label_hidden());

    crate::router::lock(&ctx.shared).games.rows[0].is_hidden = true;
    crate::games::render(&ctx, &app);
    assert!(view.get_label_hidden(), "the focused game says so");
    assert!(
        view.get_cells().row_data(0).is_some_and(|cell| cell.hidden),
        "and so does its tile"
    );

    // Its neighbor is not hidden.
    crate::games::handle_action(&ctx, &app, "right");
    assert!(!view.get_label_hidden());
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
