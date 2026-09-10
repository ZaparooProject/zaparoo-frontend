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

mod about;
mod actions;
mod alternates;
mod card_write;
mod customization;
mod display;
mod drs;
#[cfg(feature = "mister")]
mod dual_head;
mod folder_motion;
mod fonts;
#[cfg(any(feature = "mister", test))]
mod frame_transition;
mod game_info;
mod game_info_data;
mod games;
mod glyphs;
mod hub;
mod hub_covers;
mod input;
mod latch_protocol;
mod launchers;
mod log_upload;
mod media_cache;
mod media_setup;
#[cfg(feature = "mister")]
mod mister;
#[cfg_attr(
    not(feature = "mister"),
    allow(
        dead_code,
        reason = "the SMBus probe is MiSTer-only; the desktop stub keeps the call site uniform"
    )
)]
mod mister_battery;
mod qr;
// Route motion has to be checked by rendering, and that needs the
// software renderer, which only the MiSTer feature set links.
mod press_feedback;
#[cfg(all(test, feature = "mister"))]
mod route_motion;
mod router;
mod settings;
mod sizing;
mod status;
mod system_logos;
mod system_status;
mod systems;
mod tag_utils;
mod theme;
mod view_model;

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

use router::{lock, Ctx, Shared};
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use std::sync::atomic::{AtomicU64, AtomicU8, Ordering};
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
    #[cfg(feature = "mister")]
    if mister::lease::managed() {
        request_main_reload();
        return;
    }
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
    s.resolution = if config.video_explicit {
        format!("{}x{}", config.video_width, config.video_height)
    } else {
        String::new()
    };
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

pub(crate) fn scene_size(width: f64, height: f64, orientation: &str, crt: bool) -> (f64, f64) {
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
        .set_systems_list_layout(persisted.settings.systems_browse_layout == "list");
    app.global::<Sizing>()
        .set_handheld(persisted.settings.interface_profile == "handheld");
    app.global::<Motion>().set_enabled(display::motion_enabled(
        persisted.settings.reduce_motion,
        cfg!(feature = "mister"),
        visual_crt,
        framebuffer_size.1,
    ));
    display::register_labels(app);
    app.global::<Shell>()
        .set_is_mister(cfg!(feature = "mister"));
    app.global::<Shell>().set_crt_enabled(crt_enabled);
    app.global::<Shell>().set_crt_standard(SharedString::from(
        persisted.settings.crt_video_standard.as_str(),
    ));
    let bitmap = display::bitmap_type(cfg!(feature = "mister"), visual_crt, framebuffer_size.1);
    app.global::<Sizing>().set_bitmap_fonts(bitmap);
    app.global::<Theme>().set_bitmap_fonts(bitmap);
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

/// Install the launcher's logger with its own file
/// (`frontend-slint.log`), so a side-by-side install never interleaves
/// with the Qt frontend's log. Returns the logger guard, which must
/// live for the process lifetime.
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
    // Coexistence only: while both frontends ship side by side, keep
    // this one's restore state in state-slint.toml so a Slint run never
    // clobbers the Qt launcher's. Both write the same
    // `zaparoo_core::persist` schema, so at the flip this redirect is
    // simply deleted and the existing state.toml loads as it stands --
    // there is no migration to write. An explicit ZAPAROO_STATE_FILE
    // always wins, for tests and ad-hoc runs.
    if std::env::var_os("ZAPAROO_STATE_FILE").is_none() {
        let mut path = platform_paths::state_file_path();
        path.set_file_name("state-slint.toml");
        std::env::set_var("ZAPAROO_STATE_FILE", &path);
    }

    let config = zaparoo_core::config::load_config(&platform_paths::config_file_path());
    let _log_guard = init_demo_paths(&config);
    tracing::info!(endpoint = %config.core_endpoint, "Zaparoo Slint demo starting");
    #[cfg(feature = "mister")]
    let scanout_offer = mister::lease::configure();

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
    // An inherited Main offer is not ownership: the presenter waits for an
    // acknowledged grant after mode setup. A manual --latch grants nothing.
    #[cfg(feature = "mister")]
    let latch = scanout_offer && !args.iter().any(|a| a == "--no-latch");
    #[cfg(not(feature = "mister"))]
    let latch = false;
    let adaptive_render = args.iter().any(|a| a == "--adaptive-render")
        && !args.iter().any(|a| a == "--fixed-render");

    let hdmi_framebuffer_size = (config.video_width, config.video_height);
    let crt_framebuffer_size =
        zaparoo_core::config::crt_video_dimensions(&persisted.settings.crt_video_standard);
    let visual_crt = crt && !dual_head;
    #[cfg(feature = "mister")]
    let hdmi_framebuffer_size = if visual_crt {
        hdmi_framebuffer_size
    } else {
        let resolved =
            mister::video_mode::resolve(config.video_explicit.then_some(hdmi_framebuffer_size));
        if !resolved.explicit_applied {
            persisted.settings.resolution.clear();
        }
        resolved.render
    };
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
        // Ask for Core before the window exists, so its boot overlaps
        // ours the way the Qt build's post-init hook does.
        mister::ensure_core_running();
        if visual_crt {
            mister::prepare_crt_mode(crt_framebuffer_size);
        }
        mister::install_platform(
            crt,
            crt_framebuffer_size,
            offsets,
            latch,
            dual_head,
            if adaptive_render {
                mister::ResolutionPolicy::Adaptive
            } else {
                mister::ResolutionPolicy::Fixed(framebuffer_size.0, framebuffer_size.1)
            },
            &persisted.settings.orientation,
        )?;
    }
    #[cfg(not(feature = "mister"))]
    let _ = (latch, dual_head, adaptive_render);

    let ui_framebuffer_size = if cfg!(feature = "mister") || crt {
        framebuffer_size
    } else {
        (1280, 720)
    };
    let app = App::new()?;
    fonts::register_embedded_fonts();
    apply_language(&persisted.settings.language);
    let palette = theme::apply_palette(
        &app,
        &persisted.settings.color_scheme,
        &persisted.settings.color_intensity,
    );
    let (rest, focus) = theme::logo_tints(&palette);
    system_logos::set_tints(rest, focus);
    seed_display_globals(&app, &persisted, visual_crt, crt, ui_framebuffer_size);
    app.global::<GlyphSource>().on_glyph(|key, px, tint| {
        glyphs::render(key.as_str(), px.round().max(0.0) as u32, tint).unwrap_or_default()
    });

    #[cfg(feature = "mister")]
    let crt_mirror = if dual_head {
        let mirror = App::new()?;
        theme::apply_palette(
            &mirror,
            &persisted.settings.color_scheme,
            &persisted.settings.color_intensity,
        );
        seed_display_globals(&mirror, &persisted, true, true, crt_framebuffer_size);
        mirror.global::<GlyphSource>().on_glyph(|key, px, tint| {
            glyphs::render(key.as_str(), px.round().max(0.0) as u32, tint).unwrap_or_default()
        });
        Some(mirror)
    } else {
        None
    };

    seed_startup_state(&app, &persisted, boot_curtain);

    let (media, media_rx) = media_cache::MediaCache::new();
    media.set_preferred_image_type(&persisted.settings.media_image_type);
    // Cover art can be read straight off the SD card when Core is on
    // this machine; the manifest then paints the Hub's real art on the
    // first frame instead of a placeholder.
    media_cache::configure_local_path(cfg!(feature = "mister"), &config.core_endpoint);
    hub_covers::seed(&media);
    games::seed_detail_ctx(client.clone(), handle.clone());
    let notice_ack = config.notice.commercial_ack;

    let clock_twelve_hour = Arc::new(std::sync::atomic::AtomicBool::new(clock_twelve_hour(
        &persisted.settings,
    )));
    let status_language = effective_language(&persisted.settings.language);
    let (dormant, _) = tokio::sync::watch::channel(false);
    let ctx = Arc::new(Ctx {
        store: store.clone(),
        handle: handle.clone(),
        media,
        clock_twelve_hour: clock_twelve_hour.clone(),
        dormant,
        status: status::new(&status_language),
        config_path: platform_paths::config_file_path(),
        crt_enabled: crt,
        is_mister: cfg!(feature = "mister"),
        framebuffer_size: ui_framebuffer_size,
        shared: Arc::new(Mutex::new(Shared::new(
            persisted,
            restore_pending,
            config.settings.hidden_categories.clone(),
            config.settings.hidden_system_ids.clone(),
            config.settings.favorites_sort.clone().unwrap_or_default(),
            platform_paths::config_file_path(),
        ))),
    });
    start_media_cache(&ctx, &app, &client, media_rx);

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
        let ctx = ctx.clone();
        app.on_viewport_changed(move |w, h| {
            if let Some(app) = weak.upgrade() {
                let orientation = app.global::<Shell>().get_orientation().to_string();
                let (w, h) = output_size().map_or((f64::from(w), f64::from(h)), |(ow, oh)| {
                    scene_size(f64::from(ow), f64::from(oh), &orientation, visual_crt)
                });
                apply_grid_shapes(&app, w, h, visual_crt);
                // The window is rarely the size the config asked for
                // (a tiling WM, a smaller display, a live resize), so
                // every screen has to re-solve against what it got.
                router::relayout(&ctx, &app);
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
    // User customization: the name table applies at once, the artwork
    // folder is walked off the event loop once there is a frame up.
    customization::configure(
        config
            .custom_dir
            .as_ref()
            .map_or_else(platform_paths::custom_dir, std::path::PathBuf::from),
        config.system_names.clone(),
    );
    scan_customization(&ctx, &app);
    input::bind(&ctx, &app, config.key_to_action.clone());
    // The Hub paints its persisted layout before the first frame; the
    // catalog reconciles it when Core answers.
    hub::bind_input(&ctx, &app);
    systems::bind_input(&ctx, &app);
    games::bind_input(&ctx, &app);
    settings::bind_input(&ctx, &app);
    media_setup::bind_input(&ctx, &app);
    log_upload::bind_input(&ctx, &app);
    router::bind_context_input(&ctx, &app);
    hub::rebuild(&ctx, &app);
    hub::restore(&ctx, &app);
    bind_resume(&ctx, &app, &client);

    restore_core_independent(&ctx, &app);
    bind_catalog(&ctx, &app, &store);
    bind_connection_status(&ctx, &app, &client, &config.core_endpoint);
    bind_media_status(&ctx, &app, &store);
    bind_desktop_lifecycle(&ctx, &app, &client, &config.core_endpoint);
    bind_status_events(&ctx, &app, &client);
    bind_launchers(&ctx, &store);
    apply_buttons(&ctx, &app);
    bind_controller_report(&ctx, &app);
    start_clock(&app, &handle, clock_twelve_hour, ctx.dormant.subscribe());
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
            settings::enter(ctx, app);
        } else {
            router::enter_about(ctx, app);
        }
    }
}

/// Walk the customization folder off the event loop and repaint what
/// it can change. On `MiSTer` the folder is on the SD card, so the
/// first frame must never wait on it; a zero-config install finds
/// nothing and repaints nothing.
fn scan_customization(ctx: &Arc<Ctx>, app: &App) {
    let weak = app.as_weak();
    let ctx = ctx.clone();
    ctx.handle.clone().spawn(async move {
        let found = tokio::task::spawn_blocking(customization::scan)
            .await
            .unwrap_or(0);
        if found == 0 {
            return;
        }
        let _ = weak.upgrade_in_event_loop(move |app| {
            hub::render(&ctx, &app);
            systems::render(&ctx, &app);
        });
    });
}

/// Media status -> the header status line's task tier, plus the
/// settings Library page so its Start/Cancel verbs and busy gates track
/// the running job. Seeded synchronously from the store's singleton
/// `MediaStatusResource`, then followed through its watch channel.
fn bind_media_status(ctx: &Arc<Ctx>, app: &App, store: &Arc<Store>) {
    let resource = store.media_status();
    let mut rx = resource.subscribe();
    status::set_task(
        &ctx.status,
        app,
        &ctx.handle,
        status::task_of(&rx.borrow_and_update()),
    );
    status::enable_media_activity(&ctx.status, app, &ctx.handle);
    let weak = app.as_weak();
    let ctx = ctx.clone();
    ctx.handle.clone().spawn(async move {
        while rx.changed().await.is_ok() {
            let task = status::task_of(&rx.borrow_and_update());
            let ctx = ctx.clone();
            let _ = weak.upgrade_in_event_loop(move |app| {
                status::set_task(&ctx.status, &app, &ctx.handle, task);
                settings::refresh(&ctx, &app);
                router::refresh_first_run(&ctx, &app);
            });
        }
    });
}

/// Local launch lifecycle. Core remains the process supervisor; every
/// frontend goes cooperatively idle for primary media. Desktop stays mapped
/// behind the game so Wayland can reveal it without an unsupported unminimize;
/// `MiSTer` waits quietly for its wrapper to kill the process.
fn bind_desktop_lifecycle(ctx: &Arc<Ctx>, app: &App, client: &Arc<Client>, endpoint: &str) {
    if !local_lifecycle_enabled(endpoint) {
        return;
    }

    let resource = ctx.store.media_status();
    let mut media_rx = resource.subscribe();
    let weak = app.as_weak();
    let ctx_media = ctx.clone();
    ctx.handle.clone().spawn(async move {
        let mut was_active = false;
        loop {
            let snapshot = media_rx.borrow_and_update().clone();
            let active = snapshot.primary_active.is_some();
            let resumed = was_active && !active;
            was_active = active;
            let ctx_event = ctx_media.clone();
            let _ = weak.upgrade_in_event_loop(move |app| {
                set_dormant(&ctx_event, &app, active);
            });
            if resumed {
                // Core's history tracker consumes the same stop event.
                // Give its durable row a beat to close before refetching,
                // without blocking a rapid next start notification.
                let ctx_refresh = ctx_media.clone();
                let weak_refresh = weak.clone();
                ctx_media.handle.spawn(async move {
                    tokio::time::sleep(Duration::from_millis(200)).await;
                    if *ctx_refresh.dormant.borrow() {
                        return;
                    }
                    ctx_refresh
                        .store
                        .invalidate(&zaparoo_core::store::Tag::any("MediaHistory"));
                    let ctx_event = ctx_refresh.clone();
                    let _ = weak_refresh.upgrade_in_event_loop(move |app| {
                        refresh_resume(&ctx_event, &app);
                        games::refresh_recents(&ctx_event, &app);
                    });
                });
            }
            if media_rx.changed().await.is_err() {
                return;
            }
        }
    });

    // Never leave input and background work suspended indefinitely when
    // local Core disappears. A short grace avoids flashing awake during
    // an ordinary reconnect; a later successful seed can suspend again.
    let mut connection_rx = client.connection.subscribe();
    let generation = Arc::new(AtomicU64::new(0));
    let weak = app.as_weak();
    let handle = ctx.handle.clone();
    let ctx_connection = ctx.clone();
    handle.clone().spawn(async move {
        while connection_rx.changed().await.is_ok() {
            let state = connection_rx.borrow_and_update().clone();
            let current_generation = generation.fetch_add(1, Ordering::SeqCst) + 1;
            if matches!(state, ConnectionState::Connected) {
                continue;
            }
            let generation = generation.clone();
            let weak = weak.clone();
            let ctx_event = ctx_connection.clone();
            handle.spawn(async move {
                tokio::time::sleep(Duration::from_secs(2)).await;
                if generation.load(Ordering::SeqCst) != current_generation {
                    return;
                }
                let _ = weak.upgrade_in_event_loop(move |app| {
                    set_dormant(&ctx_event, &app, false);
                });
            });
        }
    });
}

fn local_lifecycle_enabled(endpoint: &str) -> bool {
    zaparoo_app::covers::endpoint_is_loopback(endpoint)
}

fn set_dormant(ctx: &Ctx, app: &App, dormant: bool) {
    let shell = app.global::<Shell>();
    if shell.get_dormant() == dormant {
        return;
    }
    shell.set_dormant(dormant);
    ctx.dormant.send_replace(dormant);
    if dormant {
        input::stop_repeat(ctx);
        {
            let mut shared = lock(&ctx.shared);
            shared.saver_seq += 1;
        }
        shell.set_saver_armed(false);
        app.global::<Motion>().set_enabled(false);
        if ctx.is_mister {
            ctx.media.clear_decoded();
        }
        tracing::info!("primary media active; frontend dormant");
    } else {
        let reduce_motion = lock(&ctx.shared).persist.settings.reduce_motion;
        app.global::<Motion>().set_enabled(display::motion_enabled(
            reduce_motion,
            ctx.is_mister,
            app.global::<Theme>().get_crt(),
            ctx.framebuffer_size.1,
        ));
        router::reset_idle(ctx, app);
        tracing::info!("primary media stopped; frontend resumed");
    }
}

/// Core notifications the status line surfaces as transient events
/// (playtime warnings, inbox messages); everything else is dropped.
fn bind_status_events(ctx: &Arc<Ctx>, app: &App, client: &Arc<Client>) {
    let mut rx = client.subscribe_notifications();
    let weak = app.as_weak();
    let ctx = ctx.clone();
    ctx.handle.clone().spawn(async move {
        loop {
            match rx.recv().await {
                Ok(notification) => {
                    let Some(event) = status::classify(&notification) else {
                        continue;
                    };
                    let ctx = ctx.clone();
                    let _ = weak.upgrade_in_event_loop(move |app| {
                        status::observe_event(&ctx.status, &app, &ctx.handle, &event);
                    });
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                Err(tokio::sync::broadcast::error::RecvError::Closed) => return,
            }
        }
    });
}

/// Help-bar glyphs: the controller report (`MiSTer`'s input report,
/// polled by `zaparoo_core::controller_report`) combined with the
/// Controls settings, re-resolved whenever either changes.
fn bind_controller_report(ctx: &Arc<Ctx>, app: &App) {
    let started = zaparoo_core::controller_report::spawn_watcher();
    tracing::debug!(started, "controller report watcher");
    let mut rx = zaparoo_core::controller_report::subscribe();
    let weak = app.as_weak();
    let ctx = ctx.clone();
    ctx.handle.clone().spawn(async move {
        while rx.changed().await.is_ok() {
            let ctx = ctx.clone();
            let _ = weak.upgrade_in_event_loop(move |app| apply_buttons(&ctx, &app));
        }
    });
}

/// Push the resolved glyph selection into the `Buttons` global.
pub(crate) fn apply_buttons(ctx: &Ctx, app: &App) {
    let report = zaparoo_core::controller_report::subscribe()
        .borrow()
        .clone();
    let (layout, swap_cc, swap_ov) = {
        let guard = lock(&ctx.shared);
        let s = &guard.persist.settings;
        (
            s.button_layout.clone(),
            s.swap_confirm_cancel,
            s.swap_options_view,
        )
    };
    let resolved = zaparoo_app::buttons::resolve(
        report.as_ref().map(|r| zaparoo_app::buttons::Report {
            layout: r.layout,
            accept_button: r.accept_button,
            cancel_button: r.cancel_button,
        }),
        &layout,
        swap_cc,
        swap_ov,
    );
    let buttons = app.global::<Buttons>();
    buttons.set_style(SharedString::from(resolved.style));
    buttons.set_confirm(SharedString::from(resolved.confirm));
    buttons.set_cancel(SharedString::from(resolved.cancel));
    buttons.set_options(SharedString::from(resolved.options));
    buttons.set_view(SharedString::from(resolved.view));
}

/// Fetch `media.history.latest` once the connection is up and hand the
/// Hub's Resume tile the last-played game (`RecentsModel`'s resume state).
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
        let _ = weak.upgrade_in_event_loop(move |app| {
            request_resume(&ctx, &app, true);
        });
    });
}

/// Refresh the Resume tile after a game exits without blanking its old
/// value while Core writes the new history row.
fn refresh_resume(ctx: &Arc<Ctx>, app: &App) {
    request_resume(ctx, app, false);
}

fn request_resume(ctx: &Arc<Ctx>, app: &App, show_loading: bool) {
    if show_loading {
        hub::set_resume(
            ctx,
            app,
            hub::Resume {
                requested: true,
                loading: true,
                entry: None,
            },
        );
    }
    let ctx = ctx.clone();
    let weak = app.as_weak();
    let client = ctx.store.client();
    ctx.handle.clone().spawn(async move {
        let entry = match client.media_history_latest().await {
            Ok(result) => result.entry,
            Err(e) => {
                tracing::debug!("media.history.latest failed: {}", e.message);
                if show_loading {
                    None
                } else {
                    return;
                }
            }
        };
        let _ = weak.upgrade_in_event_loop(move |app| {
            // Remember which thumbnail the Resume tile wants, so the
            // next cold boot paints it before Core answers.
            hub_covers::refresh_resume_entry(
                &ctx,
                entry
                    .as_ref()
                    .filter(|e| !e.system_id.is_empty() && !e.media_path.is_empty())
                    .map(|e| (e.system_id.clone(), e.media_path.clone())),
            );
            hub::set_resume(
                &ctx,
                &app,
                hub::Resume {
                    requested: true,
                    loading: false,
                    entry,
                },
            );
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
    mut dormant: tokio::sync::watch::Receiver<bool>,
) {
    push_clock(app, twelve_hour.load(Ordering::Relaxed));
    let weak = app.as_weak();
    handle.spawn(async move {
        loop {
            while *dormant.borrow_and_update() {
                if dormant.changed().await.is_err() {
                    return;
                }
            }
            let text = clock_string(twelve_hour.load(Ordering::Relaxed));
            let _ = weak.upgrade_in_event_loop(move |app| {
                app.global::<Shell>()
                    .set_clock_text(SharedString::from(text.as_str()));
            });
            tokio::select! {
                () = tokio::time::sleep(Duration::from_secs(30)) => {}
                changed = dormant.changed() => {
                    if changed.is_err() {
                        return;
                    }
                }
            }
        }
    });
}

/// Header HUD status icons, refreshed every 30 seconds: host-local
/// probe (default-route class, internet reachability, Bluetooth
/// adapter, battery HAT) on a blocking thread, NFC projected from
/// Core's `readers` (Core owns the reader). Keys are in display order.
fn start_status(app: &App, ctx: &Arc<Ctx>) {
    let weak = app.as_weak();
    let mut dormant = ctx.dormant.subscribe();
    let ctx = ctx.clone();
    ctx.handle.clone().spawn(async move {
        loop {
            while *dormant.borrow_and_update() {
                if dormant.changed().await.is_err() {
                    return;
                }
            }
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
            let ctx_inner = ctx.clone();
            let _ = weak.upgrade_in_event_loop(move |app| {
                let shell = app.global::<Shell>();
                shell.set_status_keys(ModelRc::new(VecModel::from(keys)));
                shell.set_has_battery(local.has_battery);
                shell.set_battery_percent(local.battery_percent);
                shell.set_status_icons_enabled(true);
                hub::set_internet(
                    &ctx_inner,
                    &app,
                    local.has_wifi_internet || local.has_lan_internet,
                );
            });
            tokio::select! {
                () = tokio::time::sleep(Duration::from_secs(30)) => {}
                changed = dormant.changed() => {
                    if changed.is_err() {
                        return;
                    }
                }
            }
        }
    });
}

/// Header clock text for the current minute.
pub(crate) fn clock_string(twelve_hour: bool) -> String {
    let now = time::OffsetDateTime::now_local().unwrap_or_else(|_| time::OffsetDateTime::now_utc());
    zaparoo_app::clock::format(now.hour(), now.minute(), twelve_hour)
}

/// Clock text plus the widest sample for its format, so the header's
/// slot is measured once.
pub(crate) fn push_clock(app: &App, twelve_hour: bool) {
    let shell = app.global::<Shell>();
    shell.set_clock_text(SharedString::from(clock_string(twelve_hour).as_str()));
    shell.set_clock_sample(SharedString::from(zaparoo_app::clock::widest_sample(
        twelve_hour,
    )));
}

/// The 12-hour decision for the persisted clock and language settings.
pub(crate) fn clock_twelve_hour(settings: &persist::SettingsState) -> bool {
    let host = system_locale();
    zaparoo_app::clock::uses_twelve_hour(
        &settings.clock_format,
        &settings.language,
        (!host.is_empty()).then_some(host.as_str()),
    )
}

/// The language tag number formatting follows: the setting, or the
/// host locale when it is `auto`.
pub(crate) fn effective_language(setting: &str) -> String {
    let setting = setting.trim();
    if setting.is_empty() || setting.eq_ignore_ascii_case("auto") {
        system_locale()
    } else {
        setting.to_string()
    }
}

/// Media cover fetch driver. Ready covers are marshaled onto the event
/// loop and patched into whatever tile still shows that path.
fn start_media_cache(
    ctx: &Arc<Ctx>,
    app: &App,
    client: &Arc<Client>,
    media_rx: tokio::sync::mpsc::UnboundedReceiver<media_cache::MediaKey>,
) {
    let weak = app.as_weak();
    let media = ctx.media.clone();
    let handle = ctx.handle.clone();
    let ctx = ctx.clone();
    media_cache::spawn_driver(
        media,
        client.clone(),
        &handle,
        media_rx,
        ctx.dormant.subscribe(),
        move |key, image| {
            let ctx = ctx.clone();
            let _ = weak.upgrade_in_event_loop(move |app| {
                if key.image_type.is_some() {
                    game_info::cover_landed(&ctx, &app, &key, &image);
                } else {
                    hub::cover_landed(&ctx, &app, &key);
                    games::cover_landed(&ctx, &app, &key);
                }
            });
        },
    );
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
fn bind_connection_status(ctx: &Arc<Ctx>, app: &App, client: &Arc<Client>, endpoint: &str) {
    let seed = {
        let rx = client.connection.subscribe();
        let state = rx.borrow().clone();
        state
    };
    status::set_link(&ctx.status, app, &ctx.handle, status::link_of(&seed), None);
    app.global::<Shell>()
        .set_boot_text(SharedString::from(boot_text(&seed, false).as_str()));

    let mut rx = client.connection.subscribe();
    let generation = Arc::new(AtomicU64::new(0));
    let weak = app.as_weak();
    let handle = ctx.handle.clone();
    let escalate_handle = handle.clone();
    let ctx = ctx.clone();
    let endpoint = endpoint.to_string();
    handle.spawn(async move {
        while rx.changed().await.is_ok() {
            let state = rx.borrow_and_update().clone();
            // The connect loop logs its failures at debug, so at the
            // default level an unreachable Core looks like nothing
            // happening at all. Say it once per transition instead.
            match &state {
                ConnectionState::Connected => tracing::info!("connected to core at {endpoint}"),
                ConnectionState::Unreachable(message) => {
                    tracing::warn!("cannot reach core at {endpoint}, retrying: {message}");
                }
                other => tracing::info!("core link: {other:?} ({endpoint})"),
            }
            let my_generation = generation.fetch_add(1, Ordering::SeqCst) + 1;
            let boot = boot_text(&state, false);
            let link = status::link_of(&state);
            let ctx_inner = ctx.clone();
            let _ = weak.upgrade_in_event_loop(move |app| {
                status::set_link(&ctx_inner.status, &app, &ctx_inner.handle, link, None);
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

/// Push freshly-solved grid shapes into the UI. The games page size
/// follows the shape (columns x rows), and the router derives its
/// browse page size from the same properties.
/// Re-solve everything the scene decides: the derived Sizing table, the
/// page grid shapes and the browse layout profile.
fn apply_grid_shapes(app: &App, width: f64, height: f64, crt: bool) {
    sizing::apply_scene(app, sizing::Scene::of(app, width, height, crt));
}

/// Project a catalog `ResourceStatus` into the UI. Runs on the Slint
/// event loop thread (or on the main thread for the pre-run seed).
fn apply_catalog(ctx: &Arc<Ctx>, app: &App, status: &ResourceStatus<CatalogData>) {
    match status {
        ResourceStatus::Ready(data) => {
            app.global::<HubView>()
                .set_hub_error(SharedString::default());
            status::set_catalog_error(&ctx.status, app, &ctx.handle, None);
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

            // Visible categories, then the Hub: reconcile the persisted
            // layout against what Core reported and seat the saved focus.
            router::reproject_hub(ctx, app);
            hub::on_catalog_ready(ctx, app);

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
            // CategoriesModel.error_message, and the status line's
            // "Core error" tier (AppStatus.connection_state == ERROR).
            app.global::<HubView>()
                .set_hub_error(SharedString::from(message.as_str()));
            status::set_catalog_error(&ctx.status, app, &ctx.handle, Some(message));
            hub::render(ctx, app);
        }
        ResourceStatus::Idle | ResourceStatus::Loading => {}
    }
}

/// Persisted-state seeds that must land before the first frame: hub
/// selection, the cold-launch curtain, and the reduce-motion flag.
fn seed_startup_state(app: &App, persisted: &persist::PersistedState, boot_curtain: bool) {
    app.global::<Shell>().set_boot_curtain(boot_curtain);
    app.global::<Shell>()
        .set_boot_text(SharedString::from("Connecting to Zaparoo Core…"));
    app.global::<Shell>()
        .set_reduce_motion(persisted.settings.reduce_motion);
    app.global::<Shell>()
        .set_mouse_enabled(persisted.settings.mouse_enabled);
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
            games::enter_favorites(ctx, app);
            return;
        }
        "favorite-systems" => {
            systems::enter_favorites(ctx, app);
            return;
        }
        "recents" => {
            games::enter_recents(ctx, app);
            return;
        }
        "settings" => {
            settings::enter(ctx, app);
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
        systems::enter(ctx, app, &category, false);
        let sys = lock(&ctx.shared)
            .systems
            .iter()
            .find(|s| s.id == system_id)
            .cloned();
        if let Some(sys) = sys {
            // Restored entry preserves the persisted folder stack and
            // browses its top level, so a kill inside a folder resumes
            // inside that folder.
            games::enter_restored(ctx, app, &sys);
        }
    } else {
        systems::enter(ctx, app, &category, true);
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
        config.video_explicit = false;
        merge_config_settings(&mut persisted, &config);
        assert!(persisted.settings.resolution.is_empty());
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
