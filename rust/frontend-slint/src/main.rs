// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// Slint migration demo entry point. Owns the same init sequence as the
// Qt frontend's `zaparoo_rust_init`, minus the FFI split: config,
// tokio runtime, `Client`, `Store`, persisted state, then the UI. The
// `zaparoo-core` data layer is reused unchanged; only the presentation
// binding differs (watch channels project into Slint properties via
// `upgrade_in_event_loop` instead of `qt_thread.queue`).

mod actions;
mod drs;
#[cfg(feature = "mister")]
mod dual_head;
mod fonts;
#[cfg(any(feature = "mister", test))]
mod frame_transition;
mod glyphs;
mod hub_nav;
mod latch_protocol;
mod media_cache;
#[cfg(feature = "mister")]
mod mister;
mod qr;
mod router;
mod sizing;
mod system_logos;
mod system_status;
mod tag_utils;

#[cfg(all(feature = "desktop", feature = "mister"))]
compile_error!("features `desktop` and `mister` are mutually exclusive; build the MiSTer target with --no-default-features --features mister");
#[cfg(not(any(feature = "desktop", feature = "mister")))]
compile_error!("enable exactly one of the `desktop` (default) or `mister` features");

// The generated component code does not follow the workspace's
// warn-level style lints; scope the allowances to the macro output.
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
pub use generated::*;

use router::{lock, Ctx, Shared, HUB_ACTIONS};
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use zaparoo_core::client::{Client, ConnectionState};
use zaparoo_core::endpoints::catalog::CatalogEndpoint;
use zaparoo_core::persist;
use zaparoo_core::platform_paths;
use zaparoo_core::remote_resource::ResourceStatus;
use zaparoo_core::store::Store;
use zaparoo_core::systems_catalog::CatalogData;

/// Categories Core surfaces but the frontend doesn't expose (mirrors
/// the Qt `CategoriesModel`). User-hidden category preferences are not
/// wired in the demo.
const HIDDEN_CATEGORIES: &[&str] = &["Media"];

/// Physical output raster when a `MiSTer` presenter owns the screen;
/// None on desktop (window == output). Under dynamic resolution the
/// Slint window is smaller than the output while things move, so
/// anything that must stay stable across res switches (grid shapes,
/// cover tiers) reads this instead of the window size.
pub(crate) fn output_size() -> Option<(u32, u32)> {
    #[cfg(feature = "mister")]
    {
        mister::OUTPUT_SIZE.get().copied()
    }
    #[cfg(not(feature = "mister"))]
    {
        None
    }
}

const EXIT_NONE: u8 = 0;
const EXIT_RESTART: u8 = 1;
const EXIT_MAIN_RELOAD: u8 = 2;
static EXIT_ACTION: AtomicU8 = AtomicU8::new(EXIT_NONE);

/// Quit the Slint loop, then exec this binary again after the runtime
/// and presenter have shut down. Used by restart-applied HDMI settings.
pub(crate) fn request_restart() {
    EXIT_ACTION.store(EXIT_RESTART, Ordering::SeqCst);
    let _ = slint::quit_event_loop();
}

/// Select the bundled translation for the Language setting. `auto` (or an
/// empty value) follows the process locale the way Qt's `QLocale::system()`
/// did; anything else is tried as given and then by its language part, so
/// `de_AT` still lands on the `de` catalog. English is the source text, so
/// no bundled match means English. Safe to call again when the setting
/// changes: Slint re-evaluates every `@tr` binding.
pub(crate) fn apply_language(setting: &str) {
    let setting = setting.trim();
    let auto = setting.is_empty() || setting.eq_ignore_ascii_case("auto");
    let requested = if auto {
        system_locale()
    } else {
        setting.to_string()
    };
    let requested = requested.replace('-', "_");
    let base = requested.split('_').next().unwrap_or_default().to_string();
    for candidate in [requested.as_str(), base.as_str()] {
        if !candidate.is_empty() && slint::select_bundled_translation(candidate).is_ok() {
            tracing::info!(language = candidate, setting, "translation selected");
            return;
        }
    }
    let _ = slint::select_bundled_translation("");
    tracing::info!(setting, "translation: English (no bundled catalog matched)");
}

/// The locale the environment declares, as a plain `ll_CC` tag, or empty.
fn system_locale() -> String {
    ["LC_ALL", "LC_MESSAGES", "LANG"]
        .iter()
        .filter_map(|key| std::env::var(key).ok())
        .find(|value| !value.is_empty() && value != "C" && value != "POSIX")
        .map(|value| value.split('.').next().unwrap_or_default().to_string())
        .unwrap_or_default()
}

/// Ask patched `Main_MiSTer` to re-read the CRT state file, reconfigure
/// fb geometry, and respawn us. Exit 42 is its reserved reload code.
pub(crate) fn request_main_reload() {
    EXIT_ACTION.store(EXIT_MAIN_RELOAD, Ordering::SeqCst);
    let _ = slint::quit_event_loop();
}

fn restart_current_process() -> Result<(), slint::PlatformError> {
    use std::os::unix::process::CommandExt as _;
    let exe = std::env::current_exe()
        .map_err(|e| slint::PlatformError::Other(format!("current executable: {e}")))?;
    let args: Vec<std::ffi::OsString> = std::env::args_os().skip(1).collect();
    let error = std::process::Command::new(exe).args(args).exec();
    Err(slint::PlatformError::Other(format!(
        "restart frontend: {error}"
    )))
}

/// Apply a calibration nudge to the live DDR presenter. Desktop keeps
/// only the UI state; the next launch reads persisted offsets.
pub(crate) fn set_live_crt_offsets(h_offset: i32, v_offset: i32) {
    #[cfg(feature = "mister")]
    mister::set_crt_offsets(h_offset, v_offset);
    #[cfg(not(feature = "mister"))]
    let _ = (h_offset, v_offset);
}

pub(crate) fn set_live_orientation(app: &App, value: &str, framebuffer_size: (u32, u32)) {
    #[cfg(feature = "mister")]
    {
        let _ = (app, framebuffer_size);
        mister::set_orientation(value);
    }
    #[cfg(not(feature = "mister"))]
    {
        let (w, h) = if matches!(value, "cw" | "ccw") {
            (framebuffer_size.1, framebuffer_size.0)
        } else {
            framebuffer_size
        };
        app.window()
            .set_size(slint::LogicalSize::new(w as f32, h as f32));
    }
}

/// Merge frontend.toml's durable settings over the tmpfs state
/// snapshot, matching the Qt Settings model's startup precedence.
fn merge_config_settings(
    persisted: &mut persist::PersistedState,
    config: &zaparoo_core::config::Config,
) {
    let s = &mut persisted.settings;
    if config.video_explicit {
        s.resolution = format!("{}x{}", config.video_width, config.video_height);
    }
    let c = &config.settings;
    if let Some(v) = &c.orientation {
        s.orientation = if matches!(v.as_str(), "cw" | "ccw") {
            v.clone()
        } else {
            "horizontal".to_string()
        };
    }
    if let Some(v) = &c.clock_format {
        s.clock_format.clone_from(v);
    }
    if let Some(v) = &c.games_browse_layout {
        s.games_browse_layout = if v == "list" { "list" } else { "grid" }.to_string();
    }
    if let Some(v) = &c.systems_browse_layout {
        s.systems_browse_layout = if v == "list" { "list" } else { "grid" }.to_string();
    }
    if let Some(v) = &c.system_logo_style {
        s.system_logo_style.clone_from(v);
    }
    if let Some(v) = &c.button_layout {
        s.button_layout.clone_from(v);
    }
    if let Some(v) = c.mouse_enabled {
        s.mouse_enabled = v;
    }
    if let Some(v) = c.reduce_motion {
        s.reduce_motion = v;
    }
    if let Some(v) = &c.screensaver_timeout {
        s.screensaver_timeout.clone_from(v);
    }
    if let Some(v) = &c.media_image_type {
        s.media_image_type.clone_from(v);
    }
    if let Some(v) = c.show_hidden {
        s.show_hidden = v;
    }
    if let Some(v) = c.show_original_filenames {
        s.show_original_filenames = v;
    }
    if let Some(v) = &c.region {
        s.region.clone_from(v);
    }
    if let Some(v) = &c.crt_video_standard {
        s.crt_video_standard = zaparoo_core::config::normalize_crt_video_standard(v).to_string();
    }
    let (h, v) = zaparoo_core::config::clamp_crt_offsets(
        c.crt_h_offset.unwrap_or(s.crt_h_offset),
        c.crt_v_offset.unwrap_or(s.crt_v_offset),
    );
    s.crt_h_offset = h;
    s.crt_v_offset = v;
}

fn scene_size(width: f64, height: f64, orientation: &str, crt: bool) -> (f64, f64) {
    let inset_w = if crt {
        2.0 * (width * 0.05).round()
    } else {
        0.0
    };
    let inset_h = if crt {
        2.0 * (height * 0.05).round()
    } else {
        0.0
    };
    let safe_w = (width - inset_w).max(1.0);
    let safe_h = (height - inset_h).max(1.0);
    if matches!(orientation, "cw" | "ccw") {
        (safe_h, safe_w)
    } else {
        (safe_w, safe_h)
    }
}

/// CRT/bitmap-font and orientation seeding shared by both platforms.
fn seed_display_globals(
    app: &App,
    persisted: &persist::PersistedState,
    visual_crt: bool,
    crt_enabled: bool,
    framebuffer_size: (u32, u32),
) {
    app.global::<Theme>().set_crt(visual_crt);
    app.global::<Sizing>().set_crt(visual_crt);
    let rotated = matches!(persisted.settings.orientation.as_str(), "cw" | "ccw");
    app.global::<Sizing>().set_swap_axes(rotated);
    app.global::<Shell>()
        .set_orientation(SharedString::from(persisted.settings.orientation.as_str()));
    app.global::<Shell>()
        .set_browse_list_layout(persisted.settings.games_browse_layout == "list");
    app.global::<Shell>()
        .set_is_mister(cfg!(feature = "mister"));
    app.global::<Shell>().set_crt_enabled(crt_enabled);
    app.global::<Shell>().set_crt_standard(SharedString::from(
        persisted.settings.crt_video_standard.as_str(),
    ));
    #[cfg(feature = "mister")]
    app.global::<Sizing>().set_bitmap_fonts(true);
    #[cfg(not(feature = "mister"))]
    {
        let (w, h) = if rotated {
            (framebuffer_size.1, framebuffer_size.0)
        } else {
            framebuffer_size
        };
        app.window()
            .set_size(slint::LogicalSize::new(w as f32, h as f32));
    }
    let (scene_w, scene_h) = scene_size(
        f64::from(framebuffer_size.0),
        f64::from(framebuffer_size.1),
        &persisted.settings.orientation,
        visual_crt,
    );
    app.global::<Sizing>().set_screen_width(scene_w as f32);
    app.global::<Sizing>().set_screen_height(scene_h as f32);
}

/// Demo isolation: redirect persisted state to state-slint.toml
/// (unless already redirected) and install the launcher's logger with
/// a demo-suffixed file, so the Qt frontend's state and log stay
/// untouched. Returns the logger guard, which must live for the
/// process lifetime.
fn init_demo_paths(config: &zaparoo_core::config::Config) -> zaparoo_core::logger::LoggerGuard {
    let mut log_path = platform_paths::log_file_path();
    log_path.set_file_name("frontend-slint.log");
    if let Some(dir) = log_path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    zaparoo_core::logger::install_at(config, &log_path)
}

#[allow(
    clippy::too_many_lines,
    reason = "startup wires independent runtime services in one ordered orchestration path"
)]
fn main() -> Result<(), slint::PlatformError> {
    // Keep the demo's persisted state separate from the Qt frontend's
    // state.toml unless the caller redirected it explicitly, so running
    // the demo never clobbers the real launcher's restore state.
    if std::env::var_os("ZAPAROO_STATE_FILE").is_none() {
        let mut path = platform_paths::state_file_path();
        path.set_file_name("state-slint.toml");
        std::env::set_var("ZAPAROO_STATE_FILE", &path);
    }

    let config = zaparoo_core::config::load_config(&platform_paths::config_file_path());
    let _log_guard = init_demo_paths(&config);
    tracing::info!(endpoint = %config.core_endpoint, "Zaparoo Slint demo starting");
    // Before the platform exists: Slint builds its font collection from
    // SLINT_FONT_PATH when the platform is installed.
    if let Some(dir) = fonts::install_font_path() {
        tracing::info!(dir = %dir.display(), "runtime fonts");
    } else {
        tracing::warn!("no runtime font directory found; non-Latin scripts may not render");
    }

    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(r) => r,
        Err(e) => {
            tracing::error!("failed to build tokio runtime: {e}");
            return Err(slint::PlatformError::Other(format!("tokio runtime: {e}")));
        }
    };
    let handle = runtime.handle().clone();

    let client = Client::new(config.core_endpoint.clone(), &handle);
    let store = Store::new(client.clone(), handle.clone());

    let mut persisted = persist::load();
    merge_config_settings(&mut persisted, &config);
    let restore_pending = matches!(
        persisted.active_screen.as_str(),
        "systems" | "games" | "favorites" | "recents" | "settings" | "about"
    );
    // Cold-launch curtain only for Core-dependent restore targets; a
    // hub restore paints the (empty) hub optimistically and
    // Settings/About never need the catalog, matching MainLayout's
    // optimisticHubVisible / coreIndependentStartupVisible split.
    let boot_curtain = matches!(
        persisted.active_screen.as_str(),
        "systems" | "games" | "favorites" | "recents"
    );

    let args: Vec<String> = std::env::args().collect();
    // Main passes --dual-head only when analog CRT is active without
    // direct_video, leaving fb0 available for an independent HDMI UI.
    let crt = args.iter().any(|a| a == "--crt");
    let dual_head = cfg!(feature = "mister") && crt && args.iter().any(|a| a == "--dual-head");
    // Passed by patched Main only when it grants the uio latch lease.
    let latch = args.iter().any(|a| a == "--latch");
    // Main selects fixed 720p rendering by default on MiSTer; the
    // adaptive policy remains available for explicit experiments.
    let fixed_render = args.iter().any(|a| a == "--fixed-render");

    let hdmi_framebuffer_size = (config.video_width, config.video_height);
    let crt_framebuffer_size =
        zaparoo_core::config::crt_video_dimensions(&persisted.settings.crt_video_standard);
    let visual_crt = crt && !dual_head;
    let framebuffer_size = if visual_crt {
        crt_framebuffer_size
    } else {
        hdmi_framebuffer_size
    };

    #[cfg(feature = "mister")]
    {
        let offsets = zaparoo_core::config::clamp_crt_offsets(
            persisted.settings.crt_h_offset,
            persisted.settings.crt_v_offset,
        );
        mister::prepare_video_mode(crt, dual_head, hdmi_framebuffer_size, crt_framebuffer_size);
        mister::install_platform(
            crt,
            crt_framebuffer_size,
            offsets,
            latch,
            dual_head,
            if fixed_render {
                mister::ResolutionPolicy::Fixed(1280, 720)
            } else {
                mister::ResolutionPolicy::Adaptive
            },
            &persisted.settings.orientation,
        )?;
    }
    #[cfg(not(feature = "mister"))]
    let _ = (latch, dual_head, fixed_render);

    let ui_framebuffer_size = if cfg!(feature = "mister") || crt {
        framebuffer_size
    } else {
        (1280, 720)
    };
    let app = App::new()?;
    apply_language(&persisted.settings.language);
    seed_display_globals(&app, &persisted, visual_crt, crt, ui_framebuffer_size);
    app.global::<GlyphSource>().on_glyph(|key, px| {
        glyphs::render(key.as_str(), px.round().max(0.0) as u32).unwrap_or_default()
    });

    #[cfg(feature = "mister")]
    let crt_mirror = if dual_head {
        let mirror = App::new()?;
        seed_display_globals(&mirror, &persisted, true, true, crt_framebuffer_size);
        mirror.global::<GlyphSource>().on_glyph(|key, px| {
            glyphs::render(key.as_str(), px.round().max(0.0) as u32).unwrap_or_default()
        });
        Some(mirror)
    } else {
        None
    };

    seed_startup_state(&app, &persisted, boot_curtain);

    let media = start_media_cache(&app, &client, &handle);
    media.set_preferred_image_type(&persisted.settings.media_image_type);
    router::seed_detail_ctx(client.clone(), handle.clone());
    app.global::<GamesView>()
        .set_games_list_layout(persisted.settings.games_browse_layout == "list");
    let notice_ack = config.notice.commercial_ack;

    let clock_twelve_hour = Arc::new(std::sync::atomic::AtomicBool::new(
        persisted.settings.clock_format == "12h",
    ));
    let ctx = Arc::new(Ctx {
        store: store.clone(),
        handle: handle.clone(),
        media,
        clock_twelve_hour: clock_twelve_hour.clone(),
        config_path: platform_paths::config_file_path(),
        crt_enabled: crt,
        is_mister: cfg!(feature = "mister"),
        framebuffer_size: ui_framebuffer_size,
        shared: Arc::new(Mutex::new(Shared::new(
            persisted,
            restore_pending,
            config.settings.hidden_categories.clone(),
            config.settings.hidden_system_ids.clone(),
        ))),
    });

    // Solve the initial grid shapes in logical scene space and re-solve
    // on resize/orientation changes. DRS still keys from the physical
    // output raster so its fidelity switch never changes page shape.
    let initial_orientation = app.global::<Shell>().get_orientation().to_string();
    let (initial_w, initial_h) = scene_size(
        f64::from(ui_framebuffer_size.0),
        f64::from(ui_framebuffer_size.1),
        &initial_orientation,
        visual_crt,
    );
    apply_grid_shapes(&app, initial_w, initial_h, visual_crt);
    {
        let weak = app.as_weak();
        app.on_viewport_changed(move |w, h| {
            if let Some(app) = weak.upgrade() {
                let orientation = app.global::<Shell>().get_orientation().to_string();
                let (w, h) = output_size().map_or((f64::from(w), f64::from(h)), |(ow, oh)| {
                    scene_size(f64::from(ow), f64::from(oh), &orientation, visual_crt)
                });
                apply_grid_shapes(&app, w, h, visual_crt);
            }
        });
    }

    #[cfg(feature = "mister")]
    if let Some(mirror) = crt_mirror.as_ref() {
        let orientation = mirror.global::<Shell>().get_orientation().to_string();
        let (w, h) = scene_size(
            f64::from(crt_framebuffer_size.0),
            f64::from(crt_framebuffer_size.1),
            &orientation,
            true,
        );
        apply_grid_shapes(mirror, w, h, true);
        let weak = mirror.as_weak();
        mirror.on_viewport_changed(move |w, h| {
            if let Some(mirror) = weak.upgrade() {
                apply_grid_shapes(&mirror, f64::from(w), f64::from(h), true);
            }
        });
    }

    lock(&ctx.shared).notice_ack = notice_ack;
    bind_input(&ctx, &app, config.key_to_action.clone());
    bind_resume(&ctx, &app, &client);

    restore_core_independent(&ctx, &app);
    bind_catalog(&ctx, &app, &store);
    bind_connection_status(&app, &client, &handle);
    bind_media_status(&ctx, &app, &store);
    bind_launchers(&ctx, &store);
    start_clock(&app, &handle, clock_twelve_hour);
    start_status(&app, &ctx);

    #[cfg(feature = "mister")]
    let _dual_runtime = crt_mirror
        .map(|mirror| dual_head::Runtime::start(&app, mirror))
        .transpose()?;

    app.run()?;

    // Drain tokio with a deadline so worker threads exit while main is
    // still alive, mirroring `zaparoo_rust_shutdown`.
    runtime.shutdown_timeout(Duration::from_secs(2));
    match EXIT_ACTION.swap(EXIT_NONE, Ordering::SeqCst) {
        EXIT_RESTART => restart_current_process(),
        EXIT_MAIN_RELOAD => std::process::exit(42),
        _ => Ok(()),
    }
}

/// Settings/About are core-independent: restore them immediately
/// instead of holding them behind the first catalog Ready, the way
/// `MainLayout`'s `coreIndependentStartupVisible` path paints them.
fn restore_core_independent(ctx: &Arc<Ctx>, app: &App) {
    let target = lock(&ctx.shared).persist.active_screen.clone();
    if matches!(target.as_str(), "settings" | "about") {
        lock(&ctx.shared).restore_pending = false;
        if target == "settings" {
            router::enter_settings(ctx, app);
        } else {
            router::enter_about(ctx, app);
        }
    }
}

/// Key events -> normalized actions -> router, honoring the merged
/// `[input.keyboard]` bindings from frontend.toml.
fn bind_input(ctx: &Arc<Ctx>, app: &App, bindings: std::collections::HashMap<i32, String>) {
    let ctx = ctx.clone();
    let weak = app.as_weak();
    app.on_key_pressed(move |text| {
        if let (Some(app), Some(action)) = (
            weak.upgrade(),
            actions::action_for_key_with(&bindings, &text),
        ) {
            router::handle_action(&ctx, &app, &action);
        }
    });
}

/// Media-database build status -> the header's Core status line (the
/// `CoreStatusPill`'s role): indexing shows step progress, optimizing
/// shows its phase, scraping shows its counter, idle clears. Uses the
/// store's singleton `MediaStatusResource`, so late subscription
/// still sees the current state through the watch channel. Also
/// re-renders the settings Library page so its Start/Cancel verbs and
/// busy gates track the running job.
fn bind_media_status(ctx: &Arc<Ctx>, app: &App, store: &Arc<Store>) {
    fn status_line(s: &zaparoo_core::store::MediaStatusState) -> Option<String> {
        if s.indexing {
            if s.paused {
                return Some("Paused".to_string());
            }
            if s.total_steps > 0 {
                let pct = (f64::from(s.current_step.max(0)) / f64::from(s.total_steps) * 100.0)
                    .clamp(0.0, 100.0) as u32;
                if s.current_step_display.is_empty() {
                    return Some(format!("Indexing… {pct}%"));
                }
                return Some(format!("Indexing… {pct}% {}", s.current_step_display));
            }
            return Some("Indexing…".to_string());
        }
        if s.optimizing {
            return Some("Optimizing database…".to_string());
        }
        if s.scraping {
            if s.scrape_paused {
                return Some("Paused".to_string());
            }
            if s.scrape_total > 0 {
                return Some(format!(
                    "Scraping {}/{}",
                    s.scrape_processed.max(0),
                    s.scrape_total
                ));
            }
            return Some("Scraping…".to_string());
        }
        None
    }

    let resource = store.media_status();
    let mut rx = resource.subscribe();
    if let Some(line) = status_line(&rx.borrow_and_update()) {
        app.global::<Shell>()
            .set_media_status_text(SharedString::from(line.as_str()));
    }
    let weak = app.as_weak();
    let ctx = ctx.clone();
    ctx.handle.clone().spawn(async move {
        while rx.changed().await.is_ok() {
            let line = status_line(&rx.borrow_and_update()).unwrap_or_default();
            let ctx = ctx.clone();
            let _ = weak.upgrade_in_event_loop(move |app| {
                app.global::<Shell>()
                    .set_media_status_text(SharedString::from(line.as_str()));
                router::refresh_settings_fields(&ctx, &app);
                router::refresh_first_run(&ctx, &app);
            });
        }
    });
}

/// Fetch `media.history.latest` once the connection is up and label
/// the Hub's Resume tile with the last-played game (the Qt hub's
/// resumeName behavior). No entry keeps the plain "Resume" label and
/// the action reports "nothing to resume".
fn bind_resume(ctx: &Arc<Ctx>, app: &App, client: &Arc<Client>) {
    let ctx = ctx.clone();
    let weak = app.as_weak();
    let client = client.clone();
    ctx.handle.clone().spawn(async move {
        let mut rx = client.connection.subscribe();
        loop {
            if matches!(*rx.borrow_and_update(), ConnectionState::Connected) {
                break;
            }
            if rx.changed().await.is_err() {
                return;
            }
        }
        let Ok(result) = client.media_history_latest().await else {
            return;
        };
        let Some(entry) = result.entry else {
            return;
        };
        lock(&ctx.shared).resume_entry = Some(entry.clone());
        let label = if entry.media_name.is_empty() {
            "Resume".to_string()
        } else {
            entry.media_name
        };
        let _ = weak.upgrade_in_event_loop(move |app| {
            let labels: Vec<SharedString> =
                [label.as_str(), "Favorites", "Recents", "Update", "Settings"]
                    .iter()
                    .map(|s| SharedString::from(*s))
                    .collect();
            app.global::<HubView>()
                .set_hub_actions(ModelRc::new(VecModel::from(labels)));
        });
    });
}

/// Wall-clock readout for the header, ticking every 30 seconds like
/// the Qt `HeaderBar` (minutes-only display never needs finer).
/// The flag follows the persisted `clock_format` setting ("12h"
/// selects h:mm AM/PM; "24h" and "auto" render HH:mm - full locale
/// inference is out of demo scope) and is shared with the Settings
/// toggle so format changes apply live.
fn start_clock(
    app: &App,
    handle: &tokio::runtime::Handle,
    twelve_hour: Arc<std::sync::atomic::AtomicBool>,
) {
    app.global::<Shell>().set_clock_text(SharedString::from(
        clock_string(twelve_hour.load(Ordering::Relaxed)).as_str(),
    ));
    let weak = app.as_weak();
    handle.spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(30));
        tick.tick().await;
        loop {
            tick.tick().await;
            let text = clock_string(twelve_hour.load(Ordering::Relaxed));
            let _ = weak.upgrade_in_event_loop(move |app| {
                app.global::<Shell>()
                    .set_clock_text(SharedString::from(text.as_str()));
            });
        }
    });
}

/// Header HUD status icons, refreshed every 30 seconds: host-local
/// probe (default-route class + internet reachability + Bluetooth
/// adapter) on a blocking thread, NFC projected from Core's `readers`
/// (Core owns the reader). Keys are pushed reversed because the
/// header lays icons out right-to-left from the clock.
fn start_status(app: &App, ctx: &Arc<Ctx>) {
    let weak = app.as_weak();
    let ctx = ctx.clone();
    ctx.handle.clone().spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(30));
        loop {
            tick.tick().await;
            let local = tokio::task::spawn_blocking(system_status::probe)
                .await
                .unwrap_or_default();
            let (has_nfc, has_readers) = match ctx.store.client().readers().await {
                Ok(result) => (
                    result
                        .readers
                        .iter()
                        .any(zaparoo_core::media_types::ReaderInfo::is_nfc_reader),
                    !result.readers.is_empty(),
                ),
                Err(_) => (false, false),
            };
            {
                let mut guard = lock(&ctx.shared);
                guard.has_readers = has_readers;
                guard.has_nfc = has_nfc;
            }

            let mut keys: Vec<SharedString> = Vec::new();
            if has_nfc {
                keys.push("NFC".into());
            }
            if local.has_wifi_internet {
                keys.push("WiFi".into());
            }
            if local.has_lan_internet {
                keys.push("WiredNetwork".into());
            }
            if local.has_bluetooth {
                keys.push("Bluetooth".into());
            }
            keys.reverse();
            let _ = weak.upgrade_in_event_loop(move |app| {
                app.global::<Shell>()
                    .set_status_keys(ModelRc::new(VecModel::from(keys)));
            });
        }
    });
}

/// Header clock text for the current minute.
pub(crate) fn clock_string(twelve_hour: bool) -> String {
    let now = time::OffsetDateTime::now_local().unwrap_or_else(|_| time::OffsetDateTime::now_utc());
    if twelve_hour {
        let (hour, suffix) = match now.hour() {
            0 => (12, "AM"),
            h @ 1..=11 => (h, "AM"),
            12 => (12, "PM"),
            h => (h - 12, "PM"),
        };
        format!("{hour}:{:02} {suffix}", now.minute())
    } else {
        format!("{:02}:{:02}", now.hour(), now.minute())
    }
}

/// Media cover cache + its serialized fetch driver. Ready covers are
/// marshaled onto the event loop and patched into whatever games row
/// still shows that path.
fn start_media_cache(
    app: &App,
    client: &Arc<Client>,
    handle: &tokio::runtime::Handle,
) -> Arc<media_cache::MediaCache> {
    let (media, media_rx) = media_cache::MediaCache::new();
    let weak = app.as_weak();
    media_cache::spawn_driver(
        media.clone(),
        client.clone(),
        handle,
        media_rx,
        move |key, image| {
            let _ = weak.upgrade_in_event_loop(move |app| apply_cover(&app, &key, &image));
        },
    );
    media
}

/// Seed the Hub selection from persisted state before the first
/// frame; the category index is re-derived when the catalog arrives.
fn seed_hub_selection(app: &App, persisted: &persist::PersistedState) {
    app.global::<HubView>()
        .set_hub_row(persisted.hub.selected_row.min(1) as i32);
    let action_index = HUB_ACTIONS
        .iter()
        .position(|a| *a == persisted.hub.selected_action)
        .unwrap_or(0);
    app.global::<HubView>()
        .set_hub_action_index(action_index as i32);
}

/// Catalog endpoint -> categories, systems, cold-start restore.
/// Sync seed first (the `bind_to_endpoint` contract), then a watcher.
fn bind_catalog(ctx: &Arc<Ctx>, app: &App, store: &Arc<Store>) {
    let resource = store.subscribe::<CatalogEndpoint>(());
    let mut rx = resource.subscribe();
    apply_catalog(ctx, app, &rx.borrow_and_update().clone());
    let ctx = ctx.clone();
    let weak = app.as_weak();
    ctx.handle.clone().spawn(async move {
        while rx.changed().await.is_ok() {
            let snapshot = rx.borrow_and_update().clone();
            let ctx = ctx.clone();
            let _ = weak.upgrade_in_event_loop(move |app| apply_catalog(&ctx, &app, &snapshot));
        }
    });
}

/// Connection state -> status readout, seeded synchronously. Also
/// drives the cold-launch curtain's line, including the `BootOverlay`'s
/// 5-second escalation before an unreachable Core is blamed on the
/// user's network (a transient probe failure on first connect isn't
/// worth scaring anyone about).
fn bind_connection_status(app: &App, client: &Arc<Client>, handle: &tokio::runtime::Handle) {
    let seed = {
        let rx = client.connection.subscribe();
        let state = rx.borrow().clone();
        state
    };
    app.global::<Shell>()
        .set_status_text(SharedString::from(connection_text(&seed).as_str()));
    app.global::<Shell>()
        .set_boot_text(SharedString::from(boot_text(&seed, false).as_str()));

    let mut rx = client.connection.subscribe();
    let generation = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let weak = app.as_weak();
    let escalate_handle = handle.clone();
    handle.spawn(async move {
        while rx.changed().await.is_ok() {
            let state = rx.borrow_and_update().clone();
            let my_generation = generation.fetch_add(1, Ordering::SeqCst) + 1;
            let text = connection_text(&state);
            let boot = boot_text(&state, false);
            let _ = weak.upgrade_in_event_loop(move |app| {
                app.global::<Shell>()
                    .set_status_text(SharedString::from(text.as_str()));
                if !app.global::<Shell>().get_boot_complete() {
                    app.global::<Shell>()
                        .set_boot_text(SharedString::from(boot.as_str()));
                }
            });
            if matches!(state, ConnectionState::Unreachable(_)) {
                let generation = generation.clone();
                let weak = weak.clone();
                let escalated = boot_text(&state, true);
                escalate_handle.spawn(async move {
                    tokio::time::sleep(Duration::from_secs(5)).await;
                    if generation.load(Ordering::SeqCst) == my_generation {
                        let _ = weak.upgrade_in_event_loop(move |app| {
                            if !app.global::<Shell>().get_boot_complete() {
                                app.global::<Shell>()
                                    .set_boot_text(SharedString::from(escalated.as_str()));
                            }
                        });
                    }
                });
            }
        }
    });
}

/// Cold-launch curtain line for a link state, mirroring `BootOverlay`'s
/// wording: don't distinguish "not connected yet" flavors until the
/// unreachable state has persisted long enough to escalate.
fn boot_text(state: &ConnectionState, unreachable_long: bool) -> String {
    match state {
        ConnectionState::Connected => "Loading library…".to_string(),
        ConnectionState::Reconnecting => "Reconnecting…".to_string(),
        ConnectionState::Unreachable(_) if unreachable_long => {
            "Can't reach Zaparoo Core. Check your connection.".to_string()
        }
        _ => "Connecting to Zaparoo Core…".to_string(),
    }
}

/// Patch a freshly-decoded cover into the games model row that still
/// shows the key's path, and into the game-info modal when it shows
/// the same item. Matching is by path (not index) so a screen change
/// between fetch and apply cannot mislabel a tile.
pub(crate) fn apply_cover(
    app: &App,
    key: &media_cache::MediaKey,
    decoded: &media_cache::DecodedImage,
) {
    use slint::Model as _;
    // The cache stores a refcounted pixel buffer; wrapping it is a
    // refcount bump, not a copy - cheap even mid-animation.
    let make_image = || slint::Image::from_rgba8(decoded.buffer.clone());
    let is_thumb = key.max_size == media_cache::THUMB_TIER;
    if !is_thumb
        && app.global::<GameInfoView>().get_modal_open()
        && app.global::<GameInfoView>().get_modal_path().as_str() == key.path
    {
        app.global::<GameInfoView>().set_modal_cover(make_image());
        app.global::<GameInfoView>().set_modal_has_cover(true);
    }
    // Detail pane (list layout): the pane requested this cover when
    // the selection moved; patch it if the selection still matches.
    if !is_thumb
        && app.global::<GamesView>().get_games_list_layout()
        && app.global::<GamesView>().get_detail_path().as_str() == key.path
    {
        app.global::<GamesView>().set_detail_cover(make_image());
        app.global::<GamesView>().set_detail_has_cover(true);
    }
    let games = app.global::<GamesView>().get_games();
    for i in 0..games.row_count() {
        let Some(mut row) = games.row_data(i) else {
            continue;
        };
        if row.path.as_str() == key.path {
            if is_thumb {
                // Never regress a full cover back to its preview.
                if !row.has_cover {
                    row.thumb = make_image();
                    row.has_thumb = true;
                    games.set_row_data(i, row);
                }
            } else {
                row.cover = make_image();
                row.has_cover = true;
                games.set_row_data(i, row);
            }
            return;
        }
    }
}

/// Push freshly-solved grid shapes into the UI. The games page size
/// follows the shape (columns x rows), and the router derives its
/// browse page size from the same properties.
fn apply_grid_shapes(app: &App, width: f64, height: f64, crt: bool) {
    let scene = sizing::Scene {
        width,
        height,
        crt,
        bitmap_fonts: app.global::<Sizing>().get_bitmap_fonts(),
        swap_axes: app.global::<Sizing>().get_swap_axes(),
    };
    let games = sizing::games_grid_shape(scene);
    app.global::<GamesView>().set_games_grid_cols(games.columns);
    app.global::<GamesView>().set_games_grid_rows(games.rows);
    let systems = sizing::systems_grid_shape(scene);
    app.global::<SystemsView>()
        .set_systems_grid_cols(systems.columns);
    app.global::<SystemsView>()
        .set_systems_grid_rows(systems.rows);
}

fn connection_text(state: &ConnectionState) -> String {
    match state {
        ConnectionState::Connected => String::new(),
        ConnectionState::Connecting => "Connecting to Core…".to_string(),
        ConnectionState::Reconnecting => "Reconnecting…".to_string(),
        ConnectionState::Disconnected => "Disconnected".to_string(),
        ConnectionState::Unreachable(_) => "Core unreachable".to_string(),
    }
}

/// Project a catalog `ResourceStatus` into the UI. Runs on the Slint
/// event loop thread (or on the main thread for the pre-run seed).
fn apply_catalog(ctx: &Arc<Ctx>, app: &App, status: &ResourceStatus<CatalogData>) {
    match status {
        ResourceStatus::Ready(data) => {
            app.global::<HubView>()
                .set_hub_error(SharedString::default());
            let categories: Vec<String> = data
                .categories
                .iter()
                .filter(|c| !HIDDEN_CATEGORIES.iter().any(|h| c.eq_ignore_ascii_case(h)))
                .cloned()
                .collect();

            let was_restore_pending = {
                let mut shared = lock(&ctx.shared);
                shared.all_categories = categories;
                shared.systems.clone_from(&data.systems);
                let pending = shared.restore_pending;
                shared.restore_pending = false;
                pending
            };

            // User-hidden projection + tiles + case-sensitive restore
            // of the persisted category (a vanished one lands on 0).
            router::reproject_hub(ctx, app);

            // One-shot boot latch: the curtain lifts here and never
            // re-asserts; later disconnects surface through the header
            // status line only, exactly like the Qt overlay contract.
            app.global::<Shell>().set_boot_complete(true);
            app.global::<Shell>().set_boot_curtain(false);
            // Start the screensaver idle countdown now that there is
            // something on screen worth protecting.
            router::reset_idle(ctx, app);
            // Kick the sequential startup chain (commercial notice ->
            // core-version warning -> first-run index gate). Re-runs
            // are cheap and the chain self-gates on dialog-open.
            router::maybe_open_startup_notices(ctx, app);
            fetch_system_defaults(ctx);

            if was_restore_pending {
                restore_screens(ctx, app);
            }
        }
        ResourceStatus::Errored { message, .. } => {
            // In-screen terminal error like the Qt overlay reading
            // CategoriesModel.error_message; the header status line
            // stays reserved for connection state.
            app.global::<HubView>()
                .set_hub_error(SharedString::from(message.as_str()));
        }
        ResourceStatus::Idle | ResourceStatus::Loading => {}
    }
}

/// Persisted-state seeds that must land before the first frame: hub
/// selection, the cold-launch curtain, and the reduce-motion flag.
fn seed_startup_state(app: &App, persisted: &persist::PersistedState, boot_curtain: bool) {
    seed_hub_selection(app, persisted);
    app.global::<Shell>().set_boot_curtain(boot_curtain);
    app.global::<Shell>()
        .set_boot_text(SharedString::from("Connecting to Zaparoo Core…"));
    app.global::<Shell>()
        .set_reduce_motion(persisted.settings.reduce_motion);
}

/// Launcher inventory + per-system defaults for the "Change launcher"
/// picker: the launchers endpoint streams into Shared, the defaults
/// come from the settings RPC on every catalog Ready (cheap; keeps
/// them fresh after out-of-band changes).
fn bind_launchers(ctx: &Arc<Ctx>, store: &Arc<Store>) {
    use zaparoo_core::endpoints::launchers::LaunchersEndpoint;
    let resource = store.subscribe::<LaunchersEndpoint>(());
    let mut rx = resource.subscribe();
    let shared = ctx.shared.clone();
    ctx.handle.spawn(async move {
        loop {
            let snapshot = rx.borrow_and_update().clone();
            if let ResourceStatus::Ready(data) = snapshot {
                lock(&shared).launchers.clone_from(&data.launchers);
            }
            if rx.changed().await.is_err() {
                return;
            }
        }
    });
}

fn fetch_system_defaults(ctx: &Arc<Ctx>) {
    let client = ctx.store.client();
    let shared = ctx.shared.clone();
    ctx.handle.spawn(async move {
        if let Ok(result) = client.settings().await {
            lock(&shared).system_defaults = result.system_defaults;
        }
    });
}

/// Cold-start restore of the persisted screen once the catalog is
/// available: kill-and-relaunch must come back where the user was.
fn restore_screens(ctx: &Arc<Ctx>, app: &App) {
    let (target, category, system_id) = {
        let shared = lock(&ctx.shared);
        (
            shared.persist.active_screen.clone(),
            shared.persist.hub.category.clone(),
            shared.persist.games.system_id.clone(),
        )
    };
    // Favorites/Recents restore straight from the Hub - they don't
    // pass through a category.
    match target.as_str() {
        "favorites" => {
            router::enter_favorites(ctx, app);
            return;
        }
        "recents" => {
            router::enter_recents(ctx, app);
            return;
        }
        "settings" => {
            router::enter_settings(ctx, app);
            return;
        }
        "about" => {
            router::enter_about(ctx, app);
            return;
        }
        _ => {}
    }
    let category_exists = lock(&ctx.shared).categories.contains(&category);
    if !category_exists {
        return;
    }
    if target == "games" {
        // Establish the parent synchronously so restored Games cannot
        // overlap a Hub -> Systems route transition while its browse
        // request is in flight.
        router::enter_systems_immediate(ctx, app, &category);
        let sys = lock(&ctx.shared)
            .screen_systems
            .iter()
            .find(|s| s.id == system_id)
            .cloned();
        if let Some(sys) = sys {
            // Restored entry preserves the persisted folder stack and
            // browses its top level, so a kill inside a folder resumes
            // inside that folder.
            router::enter_games_restored(ctx, app, &sys);
        }
    } else {
        router::enter_systems(ctx, app, &category);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scene_size_insets_then_rotates_crt_canvas() {
        assert_eq!(
            scene_size(1280.0, 720.0, "horizontal", false),
            (1280.0, 720.0)
        );
        assert_eq!(scene_size(720.0, 480.0, "horizontal", true), (648.0, 432.0));
        assert_eq!(scene_size(720.0, 480.0, "cw", true), (432.0, 648.0));
        assert_eq!(scene_size(720.0, 480.0, "ccw", true), (432.0, 648.0));
    }

    #[test]
    fn durable_display_config_overrides_tmpfs_state() {
        let mut persisted = persist::PersistedState::default();
        persisted.settings.orientation = "ccw".to_string();
        persisted.settings.crt_video_standard = "ntsc".to_string();
        persisted.settings.crt_h_offset = 3;
        persisted.settings.crt_v_offset = -4;

        let mut config = zaparoo_core::config::Config {
            video_explicit: true,
            video_width: 1024,
            video_height: 768,
            ..zaparoo_core::config::Config::default()
        };
        config.settings.orientation = Some("cw".to_string());
        config.settings.crt_video_standard = Some("pal".to_string());
        config.settings.crt_h_offset = Some(99);
        config.settings.crt_v_offset = Some(-99);

        merge_config_settings(&mut persisted, &config);

        assert_eq!(persisted.settings.resolution, "1024x768");
        assert_eq!(persisted.settings.orientation, "cw");
        assert_eq!(persisted.settings.crt_video_standard, "pal");
        // Offsets clamp to whatever range Core's config currently honors
        // (the v2e widened range is a parity-ledger row, not this test's
        // concern); the point is that a wild config value is clamped rather
        // than persisted verbatim.
        let (h, v) = zaparoo_core::config::clamp_crt_offsets(99, -99);
        assert!(h < 99 && v > -99);
        assert_eq!(persisted.settings.crt_h_offset, h);
        assert_eq!(persisted.settings.crt_v_offset, v);
    }
}
