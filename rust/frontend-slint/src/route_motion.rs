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
