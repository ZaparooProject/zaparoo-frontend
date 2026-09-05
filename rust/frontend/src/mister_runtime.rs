// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

static DIGITAL_OUTPUT_SIZE: std::sync::OnceLock<(u32, u32)> = std::sync::OnceLock::new();
static EXPLICIT_REQUEST_APPLIED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
#[cfg(zaparoo_runtime = "mister")]
static RESOURCE_LEASE: std::sync::OnceLock<std::fs::File> = std::sync::OnceLock::new();
/// The render geometry actually left on the framebuffer once
/// `resolve_video_size` returns — `resolved` below, i.e. what
/// `current_framebuffer_size()` reads back from that point on. Distinct
/// from `DIGITAL_OUTPUT_SIZE`: that is the pre-downscale output timing, and
/// `automatic_render_size` routinely halves or quarters it to stay under
/// `AUTO_RENDER_MAX_PIXELS`, so the two are legitimately different values
/// most of the time. `watch_for_output_change` polls with the same cheap
/// sysfs read used here, so it must diff against this — the render size —
/// not the output timing, or it would see a permanent mismatch on every
/// downscaled output and restart-loop every debounce window forever.
#[cfg(zaparoo_runtime = "mister")]
static BOOT_RENDER_SIZE: std::sync::OnceLock<(u32, u32)> = std::sync::OnceLock::new();

/// Active digital output timing discovered before the framebuffer is reduced
/// to the frontend's render size. Settings uses this to offer aspect-correct
/// 720p and 1080p targets rather than a platform-agnostic resolution dump.
pub(crate) fn digital_output_size() -> Option<(u32, u32)> {
    DIGITAL_OUTPUT_SIZE.get().copied()
}

/// Whether this process verified that the configured digital render size was
/// actually applied. Settings uses a verified failure to reset the selection to
/// Automatic instead of displaying a mode the framebuffer is not using.
pub(crate) fn explicit_request_applied() -> Option<bool> {
    EXPLICIT_REQUEST_APPLIED.get().copied()
}

/// Resolve digital `MiSTer` render dimensions before Rust exports video size
/// to C++/QML. Main's launcher defers this process until HDMI mode selection is
/// complete, so a full-scale request reveals active output geometry instead of
/// whichever framebuffer size happened to be inherited during startup.
pub fn resolve_video_size(config: &mut zaparoo_core::config::Config, crt_native_path_forced: bool) {
    #[cfg(zaparoo_runtime = "mister")]
    if !crt_native_path_forced {
        let inherited = current_framebuffer_size();
        let requested = config
            .video_explicit
            .then_some((config.video_width, config.video_height));
        let (resolved, explicit_applied) = probe_render_size(requested)
            // Do not claim an automatic size that `vmode` failed to apply. Qt
            // must see the framebuffer geometry that is actually active.
            .or_else(|| {
                current_framebuffer_size()
                    .or(inherited)
                    .map(|size| (size, false))
            })
            // Keep stale persisted resolution out of the fallback path if
            // neither Main's scale command nor framebuffer sysfs is available.
            .unwrap_or(((1280, 720), false));
        let _ = BOOT_RENDER_SIZE.set(resolved);
        if requested.is_some() {
            let _ = EXPLICIT_REQUEST_APPLIED.set(explicit_applied);
        }
        config.video_width = resolved.0;
        config.video_height = resolved.1;
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
                "using MiSTer linuxfb render size"
            );
        }
    }
    #[cfg(not(zaparoo_runtime = "mister"))]
    let _ = (config, crt_native_path_forced);
}

/// Poll interval for `watch_for_output_change`. Cheap sysfs reads (not the
/// heavier `vmode`-forcing probe `resolve_video_size` uses at boot), so a
/// wide margin between checks costs nothing and only matters for how
/// quickly the frontend notices a display that was off at boot coming on.
#[cfg(zaparoo_runtime = "mister")]
const OUTPUT_WATCH_INTERVAL: std::time::Duration = std::time::Duration::from_secs(10);

/// Wait for the live framebuffer render geometry to genuinely differ from
/// `BOOT_RENDER_SIZE` — the render size this process actually settled on at
/// boot, after `automatic_render_size` downscaling — debounced across two
/// consecutive polls so a momentary mode switch can't fire this on its own.
/// Resolves once when a real, stable change is confirmed; callers restart
/// the process to re-run the full (verified) boot-time probe rather than
/// trying to hot-apply a new render size.
///
/// Deliberately compares against `BOOT_RENDER_SIZE`, not
/// `DIGITAL_OUTPUT_SIZE`: both this poll and the boot-time probe read
/// geometry through the same cheap `current_framebuffer_size()` sysfs call,
/// which reflects whatever render size was last applied to the fb, not the
/// pre-downscale output timing `DIGITAL_OUTPUT_SIZE` records. Diffing
/// against `DIGITAL_OUTPUT_SIZE` compares two different quantities and is
/// wrong whenever downscaling is in effect — which is most outputs, since
/// `AUTO_RENDER_MAX_PIXELS` is well under 1080p and 720p — producing a
/// permanent, spurious mismatch that restart-loops the frontend every
/// `OUTPUT_WATCH_INTERVAL` × 2 forever. Caught on real `MiSTer` hardware at
/// 1080p (`DIGITAL_OUTPUT_SIZE` 1920×1080, downscaled render 960×540): every
/// poll confirmed a "change" back to the same 960×540 render size the
/// frontend had itself already applied and left alone.
///
/// This exists for one specific gap: `MiSTer` already kills and relaunches
/// the frontend around every game launch, and each relaunch re-probes
/// correctly for the output state at that moment — a TV that was off at
/// boot and came on later is corrected the next time a game is played.
/// The gap is the window before that: browsing the Hub with the display
/// now on, where nothing else prompts a re-probe. See the "Re-detect
/// output resolution while running" round for the full rationale.
///
/// No-op (never resolves) off `MiSTer` — desktop has no fb sysfs and no
/// need for this; callers must not spawn it unconditionally.
pub async fn watch_for_output_change() {
    #[cfg(zaparoo_runtime = "mister")]
    {
        use tracing::info;

        // `--crt` never sets BOOT_RENDER_SIZE (resolve_video_size skips its
        // whole digital-framebuffer probe on that path), but Settings::init
        // spawns this watcher for every MiSTer run regardless of CRT mode.
        // Returning here would let the `await` above it resolve
        // immediately, which the caller reads as "output changed" and
        // queues a restart on every `--crt` launch. Stay pending forever
        // instead, same as the desktop no-op below.
        let Some(baseline) = BOOT_RENDER_SIZE.get().copied() else {
            std::future::pending::<()>().await;
            return;
        };
        let mut candidate: Option<(u32, u32)> = None;
        loop {
            tokio::time::sleep(OUTPUT_WATCH_INTERVAL).await;
            let (confirmed, next_candidate) =
                debounce_output_change(baseline, candidate, current_framebuffer_size());
            if let Some(current) = confirmed {
                info!(
                    baseline_w = baseline.0,
                    baseline_h = baseline.1,
                    new_w = current.0,
                    new_h = current.1,
                    "mister_runtime: render size changed while running, requesting restart"
                );
                return;
            }
            candidate = next_candidate;
        }
    }
    #[cfg(not(zaparoo_runtime = "mister"))]
    std::future::pending::<()>().await;
}

/// A raw framebuffer render size in pixels, as read from sysfs or frozen
/// into `BOOT_RENDER_SIZE`. Named alias purely to keep
/// `debounce_output_change`'s signature legible.
#[cfg(any(zaparoo_runtime = "mister", test))]
type OutputSize = (u32, u32);

/// Pure debounce decision for one `watch_for_output_change` poll tick,
/// split out so the debounce logic is unit-testable without the
/// `MiSTer`-only sysfs reads or a tokio runtime — same rationale as
/// `resume_cover_key_for`/`resume_cover_key_for_with`'s split in
/// `recents.rs`. `reading` is the tick's sysfs read (`None` on a failed
/// read); `candidate` is the previous tick's unconfirmed reading, if any.
/// Returns `(confirmed, next_candidate)`: `confirmed` is `Some` exactly
/// once `reading` repeats the same non-baseline value on two consecutive
/// ticks; `next_candidate` is what the caller should carry into the next
/// tick otherwise.
#[cfg(any(zaparoo_runtime = "mister", test))]
fn debounce_output_change(
    baseline: OutputSize,
    candidate: Option<OutputSize>,
    reading: Option<OutputSize>,
) -> (Option<OutputSize>, Option<OutputSize>) {
    match reading {
        None => (None, None),
        Some(current) if current == baseline => (None, None),
        Some(current) if candidate == Some(current) => (Some(current), None),
        Some(current) => (None, Some(current)),
    }
}

#[cfg(any(zaparoo_runtime = "mister", test))]
const AUTO_RENDER_MAX_PIXELS: u64 = 1366 * 768;
const USER_RENDER_MAX_WIDTH: u32 = 1920;
const USER_RENDER_MAX_HEIGHT: u32 = 1080;
#[cfg(zaparoo_runtime = "mister")]
const FB_MODE_PATH: &str = "/sys/module/MiSTer_fb/parameters/mode";
#[cfg(zaparoo_runtime = "mister")]
const FB_VIRTUAL_SIZE_PATH: &str = "/sys/class/graphics/fb0/virtual_size";

fn within_user_render_limit((width, height): (u32, u32)) -> bool {
    width > 0 && height > 0 && width <= USER_RENDER_MAX_WIDTH && height <= USER_RENDER_MAX_HEIGHT
}

/// Explicit render sizes offered for an active output. `Main_MiSTer`'s
/// `fb_cmd1` only integer-scales custom framebuffer sizes and centers the
/// result, so offer only exact output divisors that fill the display. Automatic
/// remains a separate empty-string setting.
pub(crate) fn selectable_render_sizes(output: (u32, u32)) -> Vec<(u32, u32)> {
    let mut sizes = Vec::with_capacity(2);
    for divisor in (1..=4).rev() {
        if !output.0.is_multiple_of(divisor) || !output.1.is_multiple_of(divisor) {
            continue;
        }
        let size = (output.0 / divisor, output.1 / divisor);
        if (720..=USER_RENDER_MAX_HEIGHT).contains(&size.1) && within_user_render_limit(size) {
            sizes.push(size);
        }
    }
    sizes
}

pub(crate) fn configured_render_size_supported(
    requested: (u32, u32),
    output: Option<(u32, u32)>,
) -> bool {
    within_user_render_limit(requested)
        && output.is_some_and(|output| selectable_render_sizes(output).contains(&requested))
}

/// Pick the largest integer-divided framebuffer that stays within the target
/// software-renderer pixel budget. Common outputs resolve to 960x540 at 1080p,
/// 1280x720 at 1440p/4K, and native size at 1366x768 or below.
#[cfg(any(zaparoo_runtime = "mister", test))]
fn automatic_render_size(width: u32, height: u32) -> (u32, u32) {
    let pixels = u64::from(width) * u64::from(height);
    if pixels <= AUTO_RENDER_MAX_PIXELS {
        return (width, height);
    }

    for divisor in 2..=4 {
        let target = ((width / divisor).max(1), (height / divisor).max(1));
        let target_pixels = u64::from(target.0) * u64::from(target.1);
        if target_pixels <= AUTO_RENDER_MAX_PIXELS {
            return target;
        }
    }
    ((width / 4).max(1), (height / 4).max(1))
}

#[cfg(any(zaparoo_runtime = "mister", test))]
#[derive(Clone, Copy)]
struct VmodeOutcome {
    timed_out: bool,
}

#[cfg(any(zaparoo_runtime = "mister", test))]
fn scale_probe_verified(before: (u32, u32), after: (u32, u32), outcome: VmodeOutcome) -> bool {
    !outcome.timed_out || before != after
}

#[cfg(any(zaparoo_runtime = "mister", test))]
fn infer_full_size_from_half((width, height): (u32, u32)) -> Option<(u32, u32)> {
    Some((width.checked_mul(2)?, height.checked_mul(2)?))
}

#[cfg(zaparoo_runtime = "mister")]
fn apply_explicit_render_size(target: (u32, u32), message: &'static str) -> Option<(u32, u32)> {
    use tracing::warn;

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
            "{message}"
        );
        None
    }
}

#[cfg(zaparoo_runtime = "mister")]
fn probe_render_size(requested: Option<(u32, u32)>) -> Option<((u32, u32), bool)> {
    use tracing::warn;

    let inherited = current_framebuffer_size()?;
    let full_result = run_vmode_scale("f", "rgb32");
    let after_full = current_framebuffer_size()?;
    let mut current_size = after_full;
    let mut output_size = match full_result {
        Ok(outcome) if scale_probe_verified(inherited, after_full, outcome) => Some(after_full),
        Ok(_) => {
            warn!(
                inherited_width = inherited.0,
                inherited_height = inherited.1,
                "full-scale vmode probe timed out without a geometry change"
            );
            None
        }
        Err(error) => {
            warn!(%error, "full-scale vmode probe unsupported; trying half scale");
            None
        }
    };

    // A timed-out or unsupported full-scale probe cannot make inherited fb0
    // authoritative. Try half scale independently: a verified result both gives
    // Automatic its render size and lets us infer the active output timing.
    if output_size.is_none() {
        let before_half = current_size;
        match run_vmode_scale("h", "rgb32") {
            Ok(outcome) => {
                let after_half = current_framebuffer_size()?;
                current_size = after_half;
                if scale_probe_verified(before_half, after_half, outcome) {
                    output_size = infer_full_size_from_half(after_half);
                } else {
                    warn!(
                        inherited_width = before_half.0,
                        inherited_height = before_half.1,
                        "half-scale vmode probe timed out without a geometry change"
                    );
                }
            }
            Err(error) => warn!(%error, "half-scale vmode probe unsupported"),
        }
    }

    if let Some(output) = output_size {
        let _ = DIGITAL_OUTPUT_SIZE.set(output);
    }

    if let Some(target) = requested {
        if configured_render_size_supported(target, output_size) {
            if target == current_size {
                return Some((current_size, true));
            }
            if let Some(applied) = apply_explicit_render_size(
                target,
                "explicit vmode command did not apply configured geometry",
            ) {
                return Some((applied, true));
            }
            warn!(
                requested_width = target.0,
                requested_height = target.1,
                "configured render size could not be applied; using automatic"
            );
        } else if let Some(output) = output_size {
            warn!(
                requested_width = target.0,
                requested_height = target.1,
                output_width = output.0,
                output_height = output.1,
                "configured render size is unsupported; using automatic"
            );
        } else {
            warn!(
                requested_width = target.0,
                requested_height = target.1,
                "configured render size exceeds supported limits; using automatic"
            );
        }
    }

    let automatic_basis = output_size.unwrap_or(current_size);
    let target = automatic_render_size(automatic_basis.0, automatic_basis.1);
    if target == current_size {
        return Some((current_size, false));
    }

    let half_size = (
        (automatic_basis.0 / 2).max(1),
        (automatic_basis.1 / 2).max(1),
    );
    if output_size.is_some() && target == half_size {
        match run_vmode_scale("h", "rgb32") {
            Ok(_) => {
                if current_framebuffer_size() == Some(target) {
                    return Some((target, false));
                }
            }
            Err(error) => warn!(%error, "half-scale vmode apply unsupported"),
        }
    }

    // Older vmode/Main pairs may accept only explicit geometry. This path also
    // handles 4K's automatic one-third render size.
    apply_explicit_render_size(
        target,
        "explicit vmode fallback did not apply automatic geometry",
    )
    .map(|applied| (applied, false))
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
fn run_vmode_scale(scale: &str, pixel_format: &str) -> std::io::Result<VmodeOutcome> {
    run_vmode_command(vmode_scale_command(scale, pixel_format))
}

#[cfg(zaparoo_runtime = "mister")]
fn run_vmode_with_format(
    width: u32,
    height: u32,
    pixel_format: &str,
) -> std::io::Result<VmodeOutcome> {
    run_vmode_command(vmode_resolution_command(width, height, pixel_format))
}

#[cfg(zaparoo_runtime = "mister")]
fn run_vmode_command(mut command: std::process::Command) -> std::io::Result<VmodeOutcome> {
    // MiSTer's vmode script returns 1 both when res_count confirms a change and
    // when its bounded wait expires. Keep the timeout diagnostic so scale
    // probes can require either a clean result or a verified geometry change.
    let output = command.output()?;
    if vmode_result_accepted(output.status.code(), &output.stdout, &output.stderr) {
        Ok(VmodeOutcome {
            timed_out: vmode_result_timed_out(&output.stdout, &output.stderr),
        })
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
fn normalized_vmode_output(stdout: &[u8], stderr: &[u8]) -> String {
    format!(
        "{}\n{}",
        String::from_utf8_lossy(stdout),
        String::from_utf8_lossy(stderr)
    )
    .to_ascii_lowercase()
}

#[cfg(any(zaparoo_runtime = "mister", test))]
fn vmode_result_accepted(code: Option<i32>, stdout: &[u8], stderr: &[u8]) -> bool {
    let output = normalized_vmode_output(stdout, stderr);
    let reports_usage = output.contains("usage:") || output.contains("unknown format");
    !reports_usage && matches!(code, Some(0 | 1))
}

#[cfg(any(zaparoo_runtime = "mister", test))]
fn vmode_result_timed_out(stdout: &[u8], stderr: &[u8]) -> bool {
    normalized_vmode_output(stdout, stderr).contains("failed!")
}

#[cfg(zaparoo_runtime = "mister")]
fn acquire_resource_lease() {
    use std::os::fd::AsRawFd;
    use std::os::unix::fs::OpenOptionsExt;
    use tracing::{info, warn};

    if RESOURCE_LEASE.get().is_some() {
        return;
    }
    if let Err(e) = std::fs::create_dir_all("/tmp/zaparoo") {
        warn!("failed to create resource lease directory: {e}");
        return;
    }
    let file = match std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open("/tmp/zaparoo/frontend.active.lock")
    {
        Ok(file) => file,
        Err(e) => {
            warn!("failed to open frontend resource lease: {e}");
            return;
        }
    };
    // SAFETY: file remains open in RESOURCE_LEASE for process lifetime.
    if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        warn!(
            "failed to acquire frontend resource lease: {}",
            std::io::Error::last_os_error()
        );
        return;
    }
    if RESOURCE_LEASE.set(file).is_ok() {
        info!("acquired MiSTer frontend resource lease");
    }
}

#[cfg(any(zaparoo_runtime = "mister", test))]
fn set_oom_score_adj(path: &std::path::Path) -> std::io::Result<()> {
    std::fs::write(path, b"500")
}

#[cfg(zaparoo_runtime = "mister")]
fn prefer_frontend_as_oom_victim() {
    // Frontend is supervised by Main and restarts with empty caches; Core owns
    // device services and durable operations. Prefer sacrificing frontend
    // under MiSTer's swap-free memory pressure instead of silently losing Core.
    let path = std::path::Path::new("/proc/self/oom_score_adj");
    match set_oom_score_adj(path) {
        Ok(()) => tracing::info!("set frontend OOM priority above Core"),
        Err(error) => tracing::warn!("failed to set frontend OOM priority: {error}"),
    }
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
/// Core dynamically responds to the kernel-backed frontend resource lease, so
/// launch order and service restarts do not determine CPU or IRQ topology.
pub fn ensure_core_service_running() {
    #[cfg(zaparoo_runtime = "mister")]
    {
        use tracing::{info, warn};
        prefer_frontend_as_oom_victim();
        acquire_resource_lease();
        info!("spawning core service wrapper with CPU affinity 0-1");
        if let Err(e) = core_service_start_command().spawn() {
            warn!("failed to start zaparoo.sh with taskset: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::unwrap_used,
        reason = "tests should fail-fast on unexpected errors"
    )]

    use super::{
        automatic_render_size, configured_render_size_supported, core_service_start_command,
        debounce_output_change, infer_full_size_from_half, parse_fb_mode, parse_virtual_size,
        scale_probe_verified, selectable_render_sizes, set_oom_score_adj, vmode_resolution_command,
        vmode_result_accepted, vmode_result_timed_out, vmode_scale_command, VmodeOutcome,
    };

    #[test]
    fn debounce_ignores_a_reading_matching_the_baseline() {
        let (confirmed, candidate) = debounce_output_change((1280, 720), None, Some((1280, 720)));
        assert_eq!(confirmed, None);
        assert_eq!(candidate, None);
    }

    #[test]
    fn debounce_ignores_a_failed_read() {
        let (confirmed, candidate) = debounce_output_change((1280, 720), Some((1920, 1080)), None);
        assert_eq!(
            confirmed, None,
            "a failed read must not confirm a stale candidate"
        );
        assert_eq!(
            candidate, None,
            "a failed read must not carry the old candidate forward either"
        );
    }

    #[test]
    fn debounce_holds_a_first_differing_reading_as_a_candidate() {
        let (confirmed, candidate) = debounce_output_change((1280, 720), None, Some((1920, 1080)));
        assert_eq!(
            confirmed, None,
            "one differing reading alone must not confirm"
        );
        assert_eq!(candidate, Some((1920, 1080)));
    }

    #[test]
    fn debounce_confirms_when_two_consecutive_readings_agree() {
        let (confirmed, candidate) =
            debounce_output_change((1280, 720), Some((1920, 1080)), Some((1920, 1080)));
        assert_eq!(confirmed, Some((1920, 1080)));
        assert_eq!(candidate, None);
    }

    #[test]
    fn debounce_resets_the_candidate_when_the_second_reading_disagrees() {
        // A flickering mode switch (e.g. 1920x1080 then something else
        // entirely, not back to baseline) must not confirm either value --
        // it restarts the debounce window on the newer reading.
        let (confirmed, candidate) =
            debounce_output_change((1280, 720), Some((1920, 1080)), Some((1366, 768)));
        assert_eq!(confirmed, None);
        assert_eq!(candidate, Some((1366, 768)));
    }

    #[test]
    fn automatic_render_size_keeps_pixel_budget_native() {
        assert_eq!(automatic_render_size(1366, 768), (1366, 768));
        assert_eq!(automatic_render_size(1280, 720), (1280, 720));
        assert_eq!(automatic_render_size(640, 480), (640, 480));
        assert_eq!(automatic_render_size(352, 240), (352, 240));
    }

    #[test]
    fn automatic_render_size_uses_integer_divisors_above_budget() {
        assert_eq!(automatic_render_size(1920, 1080), (960, 540));
        assert_eq!(automatic_render_size(2560, 1440), (1280, 720));
        assert_eq!(automatic_render_size(3840, 2160), (1280, 720));
    }

    #[test]
    fn selectable_render_sizes_fill_output_with_integer_scaling() {
        assert_eq!(selectable_render_sizes((1280, 720)), [(1280, 720)]);
        assert_eq!(selectable_render_sizes((1920, 1080)), [(1920, 1080)]);
        assert_eq!(selectable_render_sizes((2560, 1440)), [(1280, 720)]);
        assert_eq!(
            selectable_render_sizes((3840, 2160)),
            [(1280, 720), (1920, 1080)]
        );
        assert!(selectable_render_sizes((1920, 1200)).is_empty());
        assert_eq!(selectable_render_sizes((1920, 1440)), [(960, 720)]);
    }

    #[test]
    fn selectable_render_sizes_cap_and_reject_legacy_oversize_values() {
        assert!(configured_render_size_supported(
            (1920, 1080),
            Some((3840, 2160))
        ));
        assert!(!configured_render_size_supported(
            (1920, 1080),
            Some((2560, 1440))
        ));
        assert!(!configured_render_size_supported(
            (2560, 1440),
            Some((2560, 1440))
        ));
        assert!(!configured_render_size_supported(
            (1280, 720),
            Some((1920, 1200))
        ));
        assert!(!configured_render_size_supported((1920, 1080), None));
        assert!(!configured_render_size_supported((2560, 1440), None));
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
        assert!(vmode_result_timed_out(b".... failed!", b""));
    }

    #[test]
    fn timed_out_scale_probe_requires_geometry_change() {
        let timeout = VmodeOutcome { timed_out: true };
        let clean = VmodeOutcome { timed_out: false };
        assert!(!scale_probe_verified((960, 540), (960, 540), timeout));
        assert!(scale_probe_verified((960, 540), (1920, 1080), timeout));
        assert!(scale_probe_verified((1920, 1080), (1920, 1080), clean));
        assert_eq!(infer_full_size_from_half((960, 540)), Some((1920, 1080)));
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
    fn writes_frontend_oom_score_adjustment() {
        let file = tempfile::NamedTempFile::new().expect("create score file");
        set_oom_score_adj(file.path()).expect("write score");
        assert_eq!(std::fs::read_to_string(file.path()).unwrap(), "500");
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
