// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

//! Screen-route motion, checked by rendering rather than by reading the
//! properties back. Whether the strip actually slides is not visible in
//! `route-page-slide`: `animate x` only animates a *change*, so a strip
//! mounted after the offset had already moved evaluates straight to its
//! end value and cuts. That is exactly the bug this file exists to
//! catch, and it can only be seen in the frames.
//!
//! Every case drives a real `router::transition_to_screen` against a
//! software-rendered window on a clock we step by hand, then counts the
//! frames whose pixels differ from the one before.

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

/// A platform whose clock only moves when the test moves it, so a
/// 170 ms animation is the same number of frames on every machine.
struct ProbePlatform;

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
/// `ROUTE_SETTLE_MS` plus a margin, in ticks.
const SETTLE_TICKS: u32 = 16;

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

/// An app on a fixed-size software window with content on both of the
/// screens the cases travel between.
#[allow(
    clippy::expect_used,
    reason = "a probe that cannot build its own window has nothing to assert"
)]
fn boot() -> (App, Rc<MinimalSoftwareWindow>) {
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
    crate::press_feedback::defer(app, target, move |_| commits.set(commits.get() + 1));
}

#[test]
#[allow(
    clippy::expect_used,
    reason = "embedded logo is a required test fixture"
)]
fn system_logo_tile_push_preserves_images_and_paints_before_navigation() {
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
        "system tile must push before the 34 ms navigation delay"
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

#[test]
#[allow(
    clippy::expect_used,
    reason = "offline router fixture must construct its runtime"
)]
fn token_empty_retry_replaces_alert_and_cancel_drains_queue() {
    use std::sync::{Arc, Mutex};
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, _) = boot();
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
        status: crate::status::new("en"),
        config_path: std::path::PathBuf::new(),
        crt_enabled: false,
        is_mister: false,
        framebuffer_size: (W, H),
    };
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
fn token_cancel_paints_before_commit_and_cannot_cancel_reopened_write() {
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
    assert_eq!(commits.get(), 0);
    distinct_frames(&window, 1);
    assert_eq!(commits.get(), 1);
    arm_feedback(&app, &commits);
    ov.set_card_write_key("2".into());
    settle(&window);
    assert_eq!(commits.get(), 1, "old Cancel must not act on a new write");
    arm_feedback(&app, &commits);
    crate::press_feedback::cancel(&app);
    ov.set_card_write_open(false);
    settle(&window);
    assert_eq!(commits.get(), 1);
}

#[test]
fn dialog_push_paints_before_commit_and_then_settles() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
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
        "button must depress before dispatch"
    );
    assert_eq!(commits.get(), 0, "accept must wait for the 34 ms press cue");
    distinct_frames(&window, 1);
    assert_eq!(commits.get(), 1);
    assert!(!crate::press_feedback::pending(&app));
    settle(&window);
    app.window().request_redraw();
    assert_eq!(resting, frame(&window));
}

#[test]
fn deferred_accept_cannot_hit_a_changed_or_canceled_target() {
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
        "cancel must invalidate the deferred ticket"
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
fn settings_category_push_and_picker_blink_precede_accept() {
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
            "{owner} must show feedback before leaving"
        );
        settle(&window);
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
fn dialog_width_tracks_content_instead_of_always_using_cap() {
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
        widths[0] < widths[1],
        "short confirmation should be narrower: {widths:?}"
    );
    assert!(
        widths[2] > widths[1],
        "notice must retain its wider shell: {widths:?}"
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
    let clicks = Rc::new(Cell::new(0));
    let observed = clicks.clone();
    ov.on_pointer_choice(move |kind, _, accept| {
        if kind == "list" && accept {
            observed.set(observed.get() + 1);
        }
    });
    settle(&window);
    for enabled in [false, true] {
        app.global::<Shell>().set_mouse_enabled(enabled);
        frame(&window);
        let position = slint::LogicalPosition::new(160.0, 92.0);
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
    let (app, window) = boot();
    app.global::<Shell>().set_active_screen("".into());
    crate::router::open_quit_confirm(&app);
    let ov = app.global::<crate::Overlays>();
    assert_eq!(ov.get_dialog_focus(), 0);
    assert_eq!(ov.get_dialog_buttons().row_data(0), Some("no".into()));
    assert_eq!(ov.get_dialog_buttons().row_data(1), Some("yes".into()));
    settle(&window);
    app.window().request_redraw();
    let before = frame(&window);
    app.global::<crate::Theme>()
        .set_control_edge(slint::Color::from_rgb_u8(255, 0, 255));
    let after = frame(&window);
    assert!(before.is_some() && after.is_some());
    assert_ne!(
        before, after,
        "dialog buttons must paint raised control edges"
    );
}

#[test]
fn a_route_change_slides_in_both_directions_and_straight_off_a_keypress() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    settle(&window);

    // Forward with nothing to fetch. This is the case that used to cut:
    // the router requested and started the slide in one turn.
    crate::router::transition_to_screen(&app, "systems", 1);
    let forward = distinct_frames(&window, SETTLE_TICKS);
    assert_eq!(
        app.global::<Shell>().get_active_screen().as_str(),
        "systems"
    );
    assert!(
        forward > 6,
        "a forward route change must animate, not cut; {forward} distinct frames"
    );

    settle(&window);

    // Back, which never has anything to fetch and so always took that
    // same one-turn path.
    crate::router::transition_to_screen(&app, "hub", -1);
    let back = distinct_frames(&window, SETTLE_TICKS);
    assert_eq!(app.global::<Shell>().get_active_screen().as_str(), "hub");
    assert!(
        back > 6,
        "a back route change must animate, not cut; {back} distinct frames"
    );
}

#[test]
fn a_fill_that_answers_inside_the_grace_window_never_paints_the_cue() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    settle(&window);

    // Qt hides the source screen only once the cue is up, and the cue
    // waits out `loadingIndicatorDelayMs` first. Inside that window
    // nothing may change on screen, or the content blinks away and back
    // before the slide even starts.
    crate::router::begin_pending(&app, "systems");
    let during_grace = distinct_frames(&window, 3);
    assert!(
        !app.global::<Shell>().get_transition_cue(),
        "the cue must still be waiting out its grace window"
    );
    assert_eq!(
        during_grace, 0,
        "a pending fill must not repaint anything before its grace window is up"
    );

    crate::router::transition_to_screen(&app, "systems", 1);
    let forward = distinct_frames(&window, SETTLE_TICKS);
    assert!(
        forward > 6,
        "the slide still runs after a fill; {forward} distinct frames"
    );
}

#[test]
fn a_slow_fill_raises_the_cue_and_keeps_the_outgoing_screen_hidden() {
    assert!(slint::platform::set_platform(Box::new(ProbePlatform)).is_ok());
    let (app, window) = boot();
    settle(&window);

    crate::router::begin_pending(&app, "systems");
    assert_eq!(app.global::<Shell>().get_transition_target(), "systems");
    // Past the 300 ms grace window.
    distinct_frames(&window, 24);
    assert!(
        app.global::<Shell>().get_transition_cue(),
        "a fill this slow must raise the cue"
    );

    crate::router::transition_to_screen(&app, "systems", 1);
    assert!(
        app.global::<Shell>().get_route_from_gated(),
        "the screen sliding out was already hidden and must stay that way"
    );
    distinct_frames(&window, SETTLE_TICKS);
    assert!(
        !app.global::<Shell>().get_route_from_gated(),
        "the gate is released once the slide has landed"
    );
    assert!(!app.global::<Shell>().get_transition_cue());
}
