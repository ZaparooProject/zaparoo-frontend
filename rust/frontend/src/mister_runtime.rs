// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

/// Sets `QT_QPA_PLATFORM=linuxfb`, `QT_QUICK_BACKEND=software`, and the
/// configured linuxfb video mode before `QGuiApplication`. No-op on
/// non-MiSTer builds.
///
/// The frontend owns `MiSTer` resolution startup so restart-applied
/// settings take effect on the very next process boot. Both the normal
/// and `--crt` paths keep linuxfb in `rgb32`, which is the mode the
/// frontend has been using in practice on `MiSTer`.
pub fn apply_pre_qt_setup(config: &zaparoo_core::config::Config, crt_native_path_forced: bool) {
    #[cfg(zaparoo_runtime = "mister")]
    {
        use tracing::info;

        std::env::set_var("QT_QPA_PLATFORM", "linuxfb");
        std::env::set_var("QT_QUICK_BACKEND", "software");

        if crt_native_path_forced {
            info!(
                "--crt: applying linuxfb mode {}x{} rgb32",
                config.video_width, config.video_height
            );
            // The CRT path cannot use `vmode`: its fb_cmd goes through
            // Main's /dev/MiSTer_cmd loop, which is not serviced while
            // the alt launcher owns video. Main itself programs the
            // framebuffer through the MiSTer_fb sysfs param (and is the
            // authority for it on spawn, including a one-shot re-assert
            // ~1 s in, reading the geometry from the mode byte in
            // zaparoo_launcher_crt.bin). This direct write covers the
            // execvp self-restart and bare dev runs where Main is not
            // involved; it is skipped when the geometry already matches.
            set_fb_mode_sysfs(config.video_width, config.video_height);
        } else {
            info!(
                "applying linuxfb mode {}x{} rgb32",
                config.video_width, config.video_height
            );
            run_vmode_with_format(config.video_width, config.video_height, "rgb32");
        }
    }
    #[cfg(not(zaparoo_runtime = "mister"))]
    let _ = (config, crt_native_path_forced);
}

#[cfg(zaparoo_runtime = "mister")]
fn set_fb_mode_sysfs(width: u32, height: u32) {
    use tracing::{info, warn};
    const FB_MODE_PATH: &str = "/sys/module/MiSTer_fb/parameters/mode";
    let stride = width * 4;
    let mode = format!("8888 1 {width} {height} {stride}");
    match std::fs::read_to_string(FB_MODE_PATH) {
        Ok(current) if current.trim() == mode => {
            // Reconfiguring the fb bumps the kernel module's res_count
            // and blanks for a frame; skip when nothing would change.
            return;
        }
        Ok(_) => {}
        Err(e) => warn!("could not read {FB_MODE_PATH}: {e}"),
    }
    match std::fs::write(FB_MODE_PATH, format!("{mode}\n")) {
        Ok(()) => info!("fb mode set via sysfs: {mode}"),
        Err(e) => warn!("could not set fb mode via {FB_MODE_PATH}: {e}"),
    }
}

#[cfg(zaparoo_runtime = "mister")]
fn run_vmode_with_format(width: u32, height: u32, pixel_format: &str) {
    use tracing::warn;
    let status = std::process::Command::new("vmode")
        .args(["-r", &width.to_string(), &height.to_string(), pixel_format])
        .status();
    match status {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            warn!("vmode not found — display mode unchanged");
        }
        Err(e) => warn!("vmode error: {e}"),
        Ok(s) if !s.success() => {
            warn!(
                "vmode exited with {:?} — display mode may not have changed",
                s.code()
            );
        }
        Ok(_) => {}
    }
}

/// Fire-and-forget `zaparoo.sh -service start`. No-op on non-MiSTer builds.
pub fn ensure_core_service_running() {
    #[cfg(zaparoo_runtime = "mister")]
    {
        use tracing::{info, warn};
        info!("spawning core service wrapper: zaparoo.sh -service start");
        if let Err(e) = std::process::Command::new("/media/fat/Scripts/zaparoo.sh")
            .args(["-service", "start"])
            .spawn()
        {
            warn!("failed to start zaparoo.sh: {e}");
        }
    }
}
