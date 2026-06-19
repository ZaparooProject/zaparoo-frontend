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

/// Parse a `"WxH"` resolution string like `"1920x1080"` (case-insensitive
/// `x`) into `(width, height)`. Returns `None` on empty input, missing
/// separator, non-numeric components, or zero values.
#[cfg(test)]
pub fn parse_resolution(value: &str) -> Option<(u32, u32)> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return None;
    }
    let (w_str, h_str) = trimmed
        .split_once('x')
        .or_else(|| trimmed.split_once('X'))?;
    let w: u32 = w_str.trim().parse().ok()?;
    let h: u32 = h_str.trim().parse().ok()?;
    if w == 0 || h == 0 {
        return None;
    }
    Some((w, h))
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

#[cfg(test)]
mod tests {
    use super::parse_resolution;

    #[test]
    fn parse_resolution_accepts_lower_x() {
        assert_eq!(parse_resolution("1920x1080"), Some((1920, 1080)));
    }

    #[test]
    fn parse_resolution_accepts_upper_x() {
        assert_eq!(parse_resolution("640X480"), Some((640, 480)));
    }

    #[test]
    fn parse_resolution_trims_whitespace() {
        assert_eq!(parse_resolution("  1280x720 "), Some((1280, 720)));
    }

    #[test]
    fn parse_resolution_rejects_empty() {
        assert!(parse_resolution("").is_none());
        assert!(parse_resolution("   ").is_none());
    }

    #[test]
    fn parse_resolution_rejects_missing_separator() {
        assert!(parse_resolution("1920").is_none());
        assert!(parse_resolution("1920-1080").is_none());
    }

    #[test]
    fn parse_resolution_rejects_non_numeric() {
        assert!(parse_resolution("widexheight").is_none());
        assert!(parse_resolution("1920xfoo").is_none());
    }

    #[test]
    fn parse_resolution_rejects_zero_components() {
        assert!(parse_resolution("0x1080").is_none());
        assert!(parse_resolution("1920x0").is_none());
    }
}
