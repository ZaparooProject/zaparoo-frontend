// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

/// Resolve digital `MiSTer` render dimensions before Rust exports video size
/// to C++/QML. Main's launcher defers this process until HDMI mode selection is
/// complete, so a full-scale request reveals active output geometry instead of
/// whichever framebuffer size happened to be inherited during startup.
pub fn resolve_video_size(config: &mut zaparoo_core::config::Config, crt_native_path_forced: bool) {
    #[cfg(zaparoo_runtime = "mister")]
    if !crt_native_path_forced {
        let inherited = current_framebuffer_size();
        let (width, height) = probe_automatic_render_size()
            .or_else(|| inherited.map(|(width, height)| automatic_render_size(width, height)))
            // Keep stale persisted resolution out of the fallback path if
            // neither Main's scale command nor framebuffer sysfs is available.
            .unwrap_or((1280, 720));
        config.video_width = width;
        config.video_height = height;
    }
    #[cfg(not(zaparoo_runtime = "mister"))]
    let _ = (config, crt_native_path_forced);
}

/// Set `linuxfb` environment before `QGuiApplication`. Digital framebuffer
/// sizing was already applied while resolving video size; `--crt` remains
/// config-driven and unchanged. No-op off `MiSTer`.
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
                render_width = config.video_width,
                render_height = config.video_height,
                "using automatic MiSTer linuxfb render size"
            );
        }
    }
    #[cfg(not(zaparoo_runtime = "mister"))]
    let _ = (config, crt_native_path_forced);
}

#[cfg(any(zaparoo_runtime = "mister", test))]
const FULL_SIZE_MAX_HEIGHT: u32 = 720;
#[cfg(zaparoo_runtime = "mister")]
const FB_MODE_PATH: &str = "/sys/module/MiSTer_fb/parameters/mode";
#[cfg(zaparoo_runtime = "mister")]
const FB_VIRTUAL_SIZE_PATH: &str = "/sys/class/graphics/fb0/virtual_size";

/// Fallback for older Main builds that cannot apply scale-relative framebuffer
/// commands. Matching Main builds use `probe_automatic_render_size` instead.
#[cfg(any(zaparoo_runtime = "mister", test))]
fn automatic_render_size(width: u32, height: u32) -> (u32, u32) {
    if height > FULL_SIZE_MAX_HEIGHT {
        ((width / 2).max(1), (height / 2).max(1))
    } else {
        (width, height)
    }
}

#[cfg(zaparoo_runtime = "mister")]
fn probe_automatic_render_size() -> Option<(u32, u32)> {
    use tracing::warn;

    let full_result = run_vmode_scale("f", "rgb32");
    let full_size = current_framebuffer_size()?;
    if full_result.is_ok() && full_size.1 <= FULL_SIZE_MAX_HEIGHT {
        return Some(full_size);
    }
    if let Err(error) = full_result {
        warn!(%error, "full-scale vmode probe unsupported; using explicit fallback");
    } else {
        // Let Main derive half scale from active HDMI timing. Unlike dividing an
        // inherited framebuffer, this remains correct when the previous process
        // left fb0 at an unrelated size.
        match run_vmode_scale("h", "rgb32") {
            Ok(()) => {
                if let Some(scaled_size) = current_framebuffer_size() {
                    if scaled_size != full_size {
                        return Some(scaled_size);
                    }
                }
            }
            Err(error) => warn!(%error, "half-scale vmode probe unsupported"),
        }
    }

    // Older vmode/Main pairs may accept only explicit geometry. Keep this as a
    // compatibility fallback, then verify sysfs instead of treating process
    // creation as proof that the command was understood.
    let target = automatic_render_size(full_size.0, full_size.1);
    run_vmode_with_format(target.0, target.1, "rgb32").ok()?;
    let applied = current_framebuffer_size()?;
    if applied == target {
        Some(applied)
    } else {
        warn!(
            expected_width = target.0,
            expected_height = target.1,
            actual_width = applied.0,
            actual_height = applied.1,
            "explicit vmode fallback did not apply requested geometry"
        );
        None
    }
}

#[cfg(zaparoo_runtime = "mister")]
fn current_framebuffer_size() -> Option<(u32, u32)> {
    std::fs::read_to_string(FB_MODE_PATH)
        .ok()
        .and_then(|mode| parse_fb_mode(&mode))
        .or_else(|| {
            std::fs::read_to_string(FB_VIRTUAL_SIZE_PATH)
                .ok()
                .and_then(|size| parse_virtual_size(&size))
        })
}

#[cfg(any(zaparoo_runtime = "mister", test))]
fn parse_fb_mode(mode: &str) -> Option<(u32, u32)> {
    let mut fields = mode.split_whitespace();
    fields.next()?;
    fields.next()?;
    let width = fields.next()?.parse::<u32>().ok()?;
    let height = fields.next()?.parse::<u32>().ok()?;
    (width > 0 && height > 0).then_some((width, height))
}

#[cfg(any(zaparoo_runtime = "mister", test))]
fn parse_virtual_size(size: &str) -> Option<(u32, u32)> {
    let (width, height) = size.trim().split_once(',')?;
    let width = width.trim().parse::<u32>().ok()?;
    let height = height.trim().parse::<u32>().ok()?;
    (width > 0 && height > 0).then_some((width, height))
}

#[cfg(zaparoo_runtime = "mister")]
fn set_fb_mode_sysfs(width: u32, height: u32) {
    use tracing::{info, warn};
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

#[cfg(any(zaparoo_runtime = "mister", test))]
fn vmode_scale_command(scale: &str, pixel_format: &str) -> std::process::Command {
    let mut command = std::process::Command::new("vmode");
    command.args([scale, pixel_format]);
    command
}

#[cfg(any(zaparoo_runtime = "mister", test))]
fn vmode_resolution_command(width: u32, height: u32, pixel_format: &str) -> std::process::Command {
    let mut command = std::process::Command::new("vmode");
    command.args(["-r", &width.to_string(), &height.to_string(), pixel_format]);
    command
}

#[cfg(zaparoo_runtime = "mister")]
fn run_vmode_scale(scale: &str, pixel_format: &str) -> std::io::Result<()> {
    run_vmode_command(vmode_scale_command(scale, pixel_format))
}

#[cfg(zaparoo_runtime = "mister")]
fn run_vmode_with_format(width: u32, height: u32, pixel_format: &str) -> std::io::Result<()> {
    run_vmode_command(vmode_resolution_command(width, height, pixel_format))
}

#[cfg(zaparoo_runtime = "mister")]
fn run_vmode_command(mut command: std::process::Command) -> std::io::Result<()> {
    // MiSTer's vmode script returns 1 both when res_count confirms a change and
    // when its bounded wait expires. Geometry from sysfs remains authoritative,
    // but usage output or any other exit code means the command was not accepted.
    let output = command.output()?;
    if vmode_result_accepted(output.status.code(), &output.stdout, &output.stderr) {
        Ok(())
    } else {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!(
                "vmode rejected command (exit {:?}): {}{}",
                output.status.code(),
                stdout.trim(),
                stderr.trim()
            ),
        ))
    }
}

#[cfg(any(zaparoo_runtime = "mister", test))]
fn vmode_result_accepted(code: Option<i32>, stdout: &[u8], stderr: &[u8]) -> bool {
    let output = format!(
        "{}\n{}",
        String::from_utf8_lossy(stdout),
        String::from_utf8_lossy(stderr)
    )
    .to_ascii_lowercase();
    let reports_usage = output.contains("usage:") || output.contains("unknown format");
    !reports_usage && matches!(code, Some(0 | 1))
}

#[cfg(any(zaparoo_runtime = "mister", test))]
fn core_service_start_command() -> std::process::Command {
    let mut command = std::process::Command::new("/usr/bin/taskset");
    command.args([
        "-c",
        "0-1",
        "/media/fat/Scripts/zaparoo.sh",
        "-service",
        "start",
    ]);
    command
}

/// Fire-and-forget `zaparoo.sh -service start`. No-op on non-MiSTer builds.
/// Core must not inherit the frontend's CPU-0-only affinity: Go runtime worker
/// and audio threads created from that process would then remain pinned to the
/// same core. `taskset` gives the service wrapper and every descendant both
/// `MiSTer` CPUs while leaving frontend affinity unchanged.
pub fn ensure_core_service_running() {
    #[cfg(zaparoo_runtime = "mister")]
    {
        use tracing::{info, warn};
        info!("spawning core service wrapper with CPU affinity 0-1");
        if let Err(e) = core_service_start_command().spawn() {
            warn!("failed to start zaparoo.sh with taskset: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        automatic_render_size, core_service_start_command, parse_fb_mode, parse_virtual_size,
        vmode_resolution_command, vmode_result_accepted, vmode_scale_command,
    };

    #[test]
    fn automatic_render_size_keeps_720_and_below_native() {
        assert_eq!(automatic_render_size(1280, 720), (1280, 720));
        assert_eq!(automatic_render_size(640, 480), (640, 480));
        assert_eq!(automatic_render_size(352, 240), (352, 240));
    }

    #[test]
    fn automatic_render_size_halves_above_720() {
        assert_eq!(automatic_render_size(1920, 1080), (960, 540));
        assert_eq!(automatic_render_size(2560, 1440), (1280, 720));
        assert_eq!(automatic_render_size(1366, 768), (683, 384));
    }

    #[test]
    fn parses_mister_fb_mode() {
        assert_eq!(parse_fb_mode("8888 1 1920 1080 7680\n"), Some((1920, 1080)));
        assert_eq!(parse_fb_mode("invalid"), None);
        assert_eq!(parse_fb_mode("8888 1 0 720 0"), None);
    }

    #[test]
    fn parses_linux_fb_virtual_size() {
        assert_eq!(parse_virtual_size("1280,720\n"), Some((1280, 720)));
        assert_eq!(parse_virtual_size("1280x720"), None);
        assert_eq!(parse_virtual_size("0,720"), None);
    }

    #[test]
    fn builds_scale_relative_vmode_command() {
        let command = vmode_scale_command("h", "rgb32");
        assert_eq!(command.get_program(), "vmode");
        assert_eq!(
            command
                .get_args()
                .map(|arg| arg.to_string_lossy().into_owned())
                .collect::<Vec<_>>(),
            ["h", "rgb32"]
        );
    }

    #[test]
    fn builds_explicit_vmode_fallback_command() {
        let command = vmode_resolution_command(960, 540, "rgb32");
        assert_eq!(command.get_program(), "vmode");
        assert_eq!(
            command
                .get_args()
                .map(|arg| arg.to_string_lossy().into_owned())
                .collect::<Vec<_>>(),
            ["-r", "960", "540", "rgb32"]
        );
    }

    #[test]
    fn accepts_mister_vmode_success_and_ambiguous_exit_one() {
        assert!(vmode_result_accepted(Some(0), b"", b""));
        assert!(vmode_result_accepted(Some(1), b".... failed!", b""));
    }

    #[test]
    fn rejects_usage_output_and_other_exit_codes() {
        assert!(!vmode_result_accepted(
            Some(0),
            b"usage:\n  vmode -r width height format",
            b""
        ));
        assert!(!vmode_result_accepted(
            Some(0),
            b"error: unknown format",
            b""
        ));
        assert!(!vmode_result_accepted(Some(2), b"", b""));
        assert!(!vmode_result_accepted(None, b"", b"terminated"));
    }

    #[test]
    fn core_service_starts_with_both_mister_cpus() {
        let command = core_service_start_command();
        assert_eq!(command.get_program(), "/usr/bin/taskset");
        assert_eq!(
            command
                .get_args()
                .map(|arg| arg.to_string_lossy().into_owned())
                .collect::<Vec<_>>(),
            [
                "-c",
                "0-1",
                "/media/fat/Scripts/zaparoo.sh",
                "-service",
                "start",
            ]
        );
    }
}
