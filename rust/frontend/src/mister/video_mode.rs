// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

//! The verified digital-output probe. Never confuse inherited or
//! downscaled fb0 geometry with HDMI timing, or claim a requested size
//! without readback.

use crate::display::{automatic_size, selectable_sizes};
use std::io::{self, Read};
use std::os::fd::AsRawFd;
use std::os::unix::process::CommandExt;
use std::process::{Command as ProcessCommand, Output, Stdio};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

type Size = (u32, u32);
const MODE_PATH: &str = "/sys/module/MiSTer_fb/parameters/mode";
static RESOLVED: OnceLock<Resolved> = OnceLock::new();

/// How long to keep asking a Main that has not answered the output probe.
/// Main is busy at boot and gives up on its black bootstrap 8 seconds after
/// spawning us, so this has to fit well inside that.
const PROBE_WINDOW: Duration = Duration::from_secs(5);
const PROBE_RETRY_PAUSE: Duration = Duration::from_millis(250);
/// The shortest attempt worth starting: vmode waits about 500 ms for Main.
const MIN_PROBE_ATTEMPT: Duration = Duration::from_secs(1);
const COMMAND_LIMIT: Duration = Duration::from_secs(2);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Resolved {
    pub render: Size,
    pub output: Option<Size>,
    pub explicit_applied: bool,
}

impl Resolved {
    /// The verified output does not offer the saved resolution. An unknown
    /// output or a resize that failed says nothing about the choice, so a slow
    /// boot must not cost the user their setting.
    pub fn saved_choice_unusable(&self, requested: Option<Size>) -> bool {
        !self.explicit_applied
            && self.output.is_some_and(|output| {
                requested.is_none_or(|target| !selectable_sizes(output).contains(&target))
            })
    }
}

pub fn output_size() -> Option<Size> {
    RESOLVED.get().and_then(|resolved| resolved.output)
}

pub fn resolve(requested: Option<Size>) -> Resolved {
    let resolved = resolve_with(
        &mut Native {
            started: Instant::now(),
        },
        requested,
    );
    tracing::info!(?resolved, ?requested, "resolved MiSTer digital render size");
    let _ = RESOLVED.set(resolved);
    resolved
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Command {
    Full,
    Half,
    Explicit(Size),
}

trait VideoIo {
    fn size(&self) -> Option<Size>;
    /// True means vmode's bounded wait timed out, not necessarily failure.
    fn run(&mut self, command: Command, limit: Duration) -> io::Result<bool>;
    /// Time since the probe began.
    fn elapsed(&self) -> Duration;
    fn pause(&mut self, duration: Duration);
}

enum Attempt {
    Answered(Size),
    /// Main did not answer in time, which says nothing about the output.
    Stalled,
    /// Asking again would get the same result.
    Failed,
}

fn attempt(io: &mut impl VideoIo, command: Command, limit: Duration) -> Attempt {
    let Some(before) = io.size() else {
        return Attempt::Failed;
    };
    let timed_out = match io.run(command, limit) {
        Ok(timed_out) => timed_out,
        Err(error) if error.kind() == io::ErrorKind::TimedOut => true,
        Err(_) => return Attempt::Failed,
    };
    match io.size() {
        Some(after) if !timed_out || before != after => Attempt::Answered(after),
        Some(_) => Attempt::Stalled,
        None => Attempt::Failed,
    }
}

fn probe(io: &mut impl VideoIo, command: Command) -> Option<Size> {
    match attempt(io, command, COMMAND_LIMIT) {
        Attempt::Answered(size) => Some(size),
        Attempt::Stalled | Attempt::Failed => None,
    }
}

/// The half-size probe, asked again while Main stalls. Main ignores framebuffer
/// commands during startup work, and one missed answer would otherwise leave
/// the output unknown for the whole run.
fn probe_half(io: &mut impl VideoIo) -> Option<Size> {
    loop {
        let left = PROBE_WINDOW.saturating_sub(io.elapsed());
        match attempt(io, Command::Half, left.min(COMMAND_LIMIT)) {
            Attempt::Answered(size) => return Some(size),
            Attempt::Failed => return None,
            Attempt::Stalled => {}
        }
        let left = PROBE_WINDOW.saturating_sub(io.elapsed());
        if left < PROBE_RETRY_PAUSE + MIN_PROBE_ATTEMPT {
            tracing::warn!(
                elapsed = ?io.elapsed(),
                "Main did not answer the output probe, output unknown until the next restart"
            );
            return None;
        }
        io.pause(PROBE_RETRY_PAUSE);
    }
}

fn apply(io: &mut impl VideoIo, command: Command, target: Size) -> bool {
    io.run(command, COMMAND_LIMIT).is_ok() && io.size() == Some(target)
}

/// The kernel module's reservation: one 1920x1080 RGB32 frame. Main does not
/// bound `vmode f` to it, and an oversized mode write makes the console
/// redraw run off the end of the mapping.
const RESERVED_BYTES: u64 = 1920 * 1080 * 4;

/// Bytes Main gives an RGB32 framebuffer: it rounds each row up to 16 bytes.
fn rgb32_bytes((width, height): Size) -> u64 {
    ((u64::from(width) * 4 + 15) & !15) * u64::from(height)
}

/// Half of any output up to 4K fits the reservation, so the half-size
/// framebuffer is the probe and the output is inferred as twice that. The
/// full-size read only sharpens an inference that halving may have
/// truncated, and only where the largest output it could hide still fits.
fn probe_output(io: &mut impl VideoIo) -> Option<Size> {
    let half = probe_half(io)?;
    let inferred = (half.0.checked_mul(2)?, half.1.checked_mul(2)?);
    let largest = (inferred.0.checked_add(1)?, inferred.1.checked_add(1)?);
    if rgb32_bytes(largest) > RESERVED_BYTES {
        return Some(inferred);
    }
    probe(io, Command::Full).or(Some(inferred))
}

fn resolve_with(io: &mut impl VideoIo, requested: Option<Size>) -> Resolved {
    let inherited = io.size();
    let output = inherited.and_then(|_| probe_output(io));
    if output.is_none() {
        if let Some(current) = io.size().or(inherited) {
            return hold_size(io, current);
        }
    }
    let current = io.size().or(inherited).unwrap_or((1280, 720));
    if let Some(target) = requested
        .filter(|target| output.is_some_and(|output| selectable_sizes(output).contains(target)))
    {
        if target == current || apply(io, Command::Explicit(target), target) {
            return Resolved {
                render: target,
                output,
                explicit_applied: true,
            };
        }
    }
    let current = io.size().unwrap_or(current);
    let basis = output.unwrap_or(current);
    let target = automatic_size(basis);
    let half = ((basis.0 / 2).max(1), (basis.1 / 2).max(1));
    let render = if target == current
        || (output.is_some() && target == half && apply(io, Command::Half, target))
        || apply(io, Command::Explicit(target), target)
    {
        target
    } else {
        // A rejected request may still have changed the framebuffer. Its
        // actual geometry, not stale config, determines scene and font tiers.
        io.size().or(inherited).unwrap_or((1280, 720))
    };
    Resolved {
        render,
        output,
        explicit_applied: false,
    }
}

/// Main may still be holding our unanswered commands and will apply them when
/// it resumes, resizing the framebuffer under a running presenter. Stay at the
/// current size and make it the last command Main reads, so the framebuffer
/// ends up at the size we draw. A Main that dropped the commands changes nothing.
fn hold_size(io: &mut impl VideoIo, current: Size) -> Resolved {
    let _ = io.run(Command::Explicit(current), COMMAND_LIMIT);
    Resolved {
        render: current,
        output: None,
        explicit_applied: false,
    }
}

fn parse_mode(mode: &str) -> Option<Size> {
    let mut fields = mode.split_whitespace().skip(2);
    let width = fields.next()?.parse().ok()?;
    let height = fields.next()?.parse().ok()?;
    (width > 0 && height > 0).then_some((width, height))
}

fn parse_virtual_size(value: &str) -> Option<Size> {
    let (w, h) = value.trim().split_once(',')?;
    let size = (w.trim().parse().ok()?, h.trim().parse().ok()?);
    (size.0 > 0 && size.1 > 0).then_some(size)
}

fn outcome(code: Option<i32>, stdout: &[u8], stderr: &[u8]) -> io::Result<bool> {
    let text = format!(
        "{}\n{}",
        String::from_utf8_lossy(stdout),
        String::from_utf8_lossy(stderr)
    );
    let normalized = text.to_ascii_lowercase();
    if matches!(code, Some(0 | 1))
        && !normalized.contains("usage:")
        && !normalized.contains("unknown format")
    {
        Ok(normalized.contains("failed!"))
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("vmode rejected command (exit {code:?}): {text}"),
        ))
    }
}

// vmode's own confirmation timeout starts only AFTER opening the command
// FIFO. Bound the whole process group, including a blocked FIFO open/helper.
fn run_bounded(command: &mut ProcessCommand, timeout: Duration) -> io::Result<Output> {
    let mut child = command
        .process_group(0)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let mut exited = false;
    let result = (|| {
        let mut stdout = child
            .stdout
            .take()
            .ok_or_else(|| io::Error::other("missing vmode stdout"))?;
        let mut stderr = child
            .stderr
            .take()
            .ok_or_else(|| io::Error::other("missing vmode stderr"))?;
        for fd in [stdout.as_raw_fd(), stderr.as_raw_fd()] {
            // SAFETY: these are owned pipe descriptors, not inherited handles.
            if unsafe { libc::fcntl(fd, libc::F_SETFL, libc::O_NONBLOCK) } < 0 {
                return Err(io::Error::last_os_error());
            }
        }
        let mut out = Vec::new();
        let mut err = Vec::new();
        let deadline = Instant::now() + timeout;
        loop {
            drain_output(&mut stdout, &mut out)?;
            drain_output(&mut stderr, &mut err)?;
            if let Some(status) = child.try_wait()? {
                exited = true;
                drain_output(&mut stdout, &mut out)?;
                drain_output(&mut stderr, &mut err)?;
                return Ok(Output {
                    status,
                    stdout: out,
                    stderr: err,
                });
            }
            if Instant::now() >= deadline {
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "vmode process deadline exceeded",
                ));
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    })();
    if result.is_err() && !exited {
        // SAFETY: child has not been reaped on error. Its new process group
        // contains only this command and its helpers; never signal Main's group.
        unsafe {
            libc::kill(-(child.id() as libc::pid_t), libc::SIGKILL);
        }
        let _ = child.kill();
        let _ = child.wait();
    }
    result
}

fn drain_output(reader: &mut impl Read, output: &mut Vec<u8>) -> io::Result<()> {
    let mut bytes = [0; 4096];
    // Bound both retained output and work per turn, even for a noisy command.
    for _ in 0..4 {
        match reader.read(&mut bytes) {
            Ok(0) => break,
            Ok(n) => {
                output.extend_from_slice(&bytes[..n.min(16384_usize.saturating_sub(output.len()))]);
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => break,
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

struct Native {
    started: Instant,
}
impl VideoIo for Native {
    fn size(&self) -> Option<Size> {
        std::fs::read_to_string(MODE_PATH)
            .ok()
            .and_then(|s| parse_mode(&s))
            .or_else(|| {
                std::fs::read_to_string("/sys/class/graphics/fb0/virtual_size")
                    .ok()
                    .and_then(|s| parse_virtual_size(&s))
            })
    }

    fn run(&mut self, command: Command, limit: Duration) -> io::Result<bool> {
        let mut process = std::process::Command::new("vmode");
        match command {
            Command::Full => {
                process.args(["f", "rgb32"]);
            }
            Command::Half => {
                process.args(["h", "rgb32"]);
            }
            Command::Explicit((w, h)) => {
                process.args(["-r", &w.to_string(), &h.to_string(), "rgb32"]);
            }
        }
        let output = run_bounded(&mut process, limit)?;
        let result = outcome(output.status.code(), &output.stdout, &output.stderr);
        if let Err(error) = &result {
            tracing::warn!(?command, %error, "MiSTer video command failed");
        }
        result
    }

    fn elapsed(&self) -> Duration {
        self.started.elapsed()
    }

    fn pause(&mut self, duration: Duration) {
        std::thread::sleep(duration);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    /// vmode's own wait for Main before it reports `failed!`.
    const VMODE_WAIT: Duration = Duration::from_millis(500);

    #[derive(Default)]
    struct Fake {
        size: Option<Size>,
        steps: VecDeque<(Command, Option<Size>, io::Result<bool>)>,
        clock: Duration,
        pauses: usize,
    }
    impl VideoIo for Fake {
        fn size(&self) -> Option<Size> {
            self.size
        }
        #[allow(
            clippy::panic,
            reason = "unexpected commands must fail the test, not simulate device rejection"
        )]
        fn run(&mut self, command: Command, limit: Duration) -> io::Result<bool> {
            assert!(limit <= COMMAND_LIMIT);
            let Some((expected, after, outcome)) = self.steps.pop_front() else {
                panic!("unexpected video command: {command:?}");
            };
            assert_eq!(command, expected);
            self.size = after;
            self.clock += match &outcome {
                Err(error) if error.kind() == io::ErrorKind::TimedOut => limit,
                Ok(true) => VMODE_WAIT.min(limit),
                _ => Duration::ZERO,
            };
            outcome
        }
        fn elapsed(&self) -> Duration {
            self.clock
        }
        fn pause(&mut self, duration: Duration) {
            self.clock += duration;
            self.pauses += 1;
        }
    }

    type Step = (Command, Option<Size>, io::Result<bool>);

    fn stalls(size: Size, count: usize) -> VecDeque<Step> {
        (0..count)
            .map(|_| (Command::Half, Some(size), Ok(true)))
            .collect()
    }

    /// Every attempt the window fits when vmode reports `failed!` each time.
    const STALLS_IN_WINDOW: usize = 6;

    #[test]
    fn whole_process_deadline_covers_blocked_fifo_and_helpers() -> io::Result<()> {
        struct Fifo(std::path::PathBuf);
        impl Drop for Fifo {
            fn drop(&mut self) {
                let _ = std::fs::remove_file(&self.0);
            }
        }
        let fifo =
            Fifo(std::env::temp_dir().join(format!("zaparoo-vmode-test-{}", std::process::id())));
        let fifo = &fifo.0;
        assert!(ProcessCommand::new("mkfifo").arg(fifo).status()?.success());
        let mut command = ProcessCommand::new("sh");
        command
            .args(["-c", "printf 'fb_cmd0 8888 1 1\\n' > \"$1\"", "sh"])
            .arg(fifo);
        let start = Instant::now();
        let result = run_bounded(&mut command, Duration::from_millis(50));
        assert!(matches!(result, Err(e) if e.kind() == io::ErrorKind::TimedOut));
        assert!(start.elapsed() < Duration::from_secs(2));
        let mut command = ProcessCommand::new("sh");
        command.args(["-c", "sleep 30 & wait"]);
        assert!(run_bounded(&mut command, Duration::from_millis(50)).is_err());
        Ok(())
    }

    #[test]
    fn bounded_command_preserves_output_and_normal_exit_one() -> io::Result<()> {
        let mut command = ProcessCommand::new("sh");
        command.args(["-c", "printf done; printf diagnostic >&2; exit 1"]);
        let output = run_bounded(&mut command, Duration::from_secs(1))?;
        assert_eq!(output.status.code(), Some(1));
        assert_eq!(output.stdout, b"done");
        assert_eq!(output.stderr, b"diagnostic");
        Ok(())
    }

    #[test]
    fn auto_1080_keeps_the_probed_half_size_without_a_full_size_write() {
        let mut io = Fake {
            size: Some((1920, 1080)),
            steps: VecDeque::from([(Command::Half, Some((960, 540)), Ok(false))]),
            ..Fake::default()
        };
        assert_eq!(
            resolve_with(&mut io, None),
            Resolved {
                render: (960, 540),
                output: Some((1920, 1080)),
                explicit_applied: false
            }
        );
        assert!(io.steps.is_empty());
        assert_eq!(io.pauses, 0);
    }

    #[test]
    fn outputs_above_the_reservation_never_get_a_full_size_write() {
        // The fake panics on any command it was not told to expect.
        for (half, output) in [
            ((960, 600), (1920, 1200)),
            ((960, 720), (1920, 1440)),
            ((1280, 720), (2560, 1440)),
            // 1921x1079 has fewer pixels than 1920x1080 but its padded rows
            // do not fit, and a 960x539 half cannot rule it out.
            ((960, 539), (1920, 1078)),
        ] {
            let mut io = Fake {
                size: Some(half),
                steps: VecDeque::from([(Command::Half, Some(half), Ok(false))]),
                ..Fake::default()
            };
            assert_eq!(
                resolve_with(&mut io, None),
                Resolved {
                    render: half,
                    output: Some(output),
                    explicit_applied: false
                }
            );
            assert!(io.steps.is_empty());
        }
    }

    #[test]
    fn small_output_reads_back_its_exact_size() {
        for output in [(1280, 720), (1365, 767)] {
            let mut io = Fake {
                size: Some(output),
                steps: VecDeque::from([
                    (Command::Half, Some((output.0 / 2, output.1 / 2)), Ok(false)),
                    (Command::Full, Some(output), Ok(false)),
                ]),
                ..Fake::default()
            };
            assert_eq!(
                resolve_with(&mut io, None),
                Resolved {
                    render: output,
                    output: Some(output),
                    explicit_applied: false
                }
            );
            assert!(io.steps.is_empty());
        }
    }

    #[test]
    fn ambiguous_full_size_read_falls_back_to_the_inferred_output() {
        let mut io = Fake {
            size: Some((1280, 720)),
            steps: VecDeque::from([
                (Command::Half, Some((640, 360)), Ok(false)),
                (Command::Full, Some((640, 360)), Ok(true)),
                (Command::Explicit((1280, 720)), Some((1280, 720)), Ok(false)),
            ]),
            ..Fake::default()
        };
        assert_eq!(
            resolve_with(&mut io, None),
            Resolved {
                render: (1280, 720),
                output: Some((1280, 720)),
                explicit_applied: false
            }
        );
        assert!(io.steps.is_empty());
    }

    #[test]
    fn timed_out_half_probe_still_counts_when_geometry_changed() {
        let mut io = Fake {
            size: Some((1920, 1080)),
            steps: VecDeque::from([(Command::Half, Some((960, 540)), Ok(true))]),
            ..Fake::default()
        };
        let result = resolve_with(&mut io, Some((1280, 720)));
        assert_eq!(result.render, (960, 540));
        assert_eq!(result.output, Some((1920, 1080)));
        assert!(!result.explicit_applied);
        assert!(io.steps.is_empty());
    }

    #[test]
    fn unverified_output_does_not_offer_explicit_choices() {
        let mut steps = stalls((960, 540), STALLS_IN_WINDOW);
        steps.push_back((Command::Explicit((960, 540)), Some((960, 540)), Ok(true)));
        let mut io = Fake {
            size: Some((960, 540)),
            steps,
            ..Fake::default()
        };
        // The fake panics on an attempt past the window.
        let result = resolve_with(&mut io, Some((1920, 1080)));
        assert_eq!(
            result,
            Resolved {
                render: (960, 540),
                output: None,
                explicit_applied: false
            }
        );
        assert!(!result.saved_choice_unusable(Some((1920, 1080))));
        assert!(io.steps.is_empty());
        assert!(io.clock <= PROBE_WINDOW);
    }

    #[test]
    fn stalled_probe_verifies_the_output_when_main_answers_in_the_window() {
        let mut steps = stalls((960, 540), 3);
        steps.push_back((Command::Half, Some((960, 540)), Ok(false)));
        steps.push_back((
            Command::Explicit((1920, 1080)),
            Some((1920, 1080)),
            Ok(false),
        ));
        let mut io = Fake {
            size: Some((960, 540)),
            steps,
            ..Fake::default()
        };
        assert_eq!(
            resolve_with(&mut io, Some((1920, 1080))),
            Resolved {
                render: (1920, 1080),
                output: Some((1920, 1080)),
                explicit_applied: true
            }
        );
        assert!(io.steps.is_empty());
        assert_eq!(io.pauses, 3);
    }

    #[test]
    fn hung_vmode_stays_inside_the_window() {
        let hung = || {
            (
                Command::Half,
                Some((960, 540)),
                Err(io::Error::from(io::ErrorKind::TimedOut)),
            )
        };
        let mut io = Fake {
            size: Some((960, 540)),
            steps: VecDeque::from([
                hung(),
                hung(),
                (Command::Explicit((960, 540)), Some((960, 540)), Ok(true)),
            ]),
            ..Fake::default()
        };
        assert_eq!(resolve_with(&mut io, None).output, None);
        assert!(io.steps.is_empty());
        assert!(io.clock <= PROBE_WINDOW);
    }

    #[test]
    fn stalled_probe_stays_at_the_size_it_found_and_queues_that_size_last() {
        // Automatic would pick 960x540 here if the output were known. A resize
        // Main only applies after we start drawing is what scrambles the picture.
        let mut steps = stalls((1920, 1080), STALLS_IN_WINDOW);
        steps.push_back((
            Command::Explicit((1920, 1080)),
            Some((1920, 1080)),
            Ok(true),
        ));
        let mut io = Fake {
            size: Some((1920, 1080)),
            steps,
            ..Fake::default()
        };
        assert_eq!(
            resolve_with(&mut io, None),
            Resolved {
                render: (1920, 1080),
                output: None,
                explicit_applied: false
            }
        );
        assert!(io.steps.is_empty());
    }

    #[test]
    fn rejected_probe_gives_up_without_waiting() {
        let mut io = Fake {
            size: Some((960, 540)),
            steps: VecDeque::from([
                (
                    Command::Half,
                    Some((960, 540)),
                    Err(io::Error::from(io::ErrorKind::InvalidInput)),
                ),
                (
                    Command::Explicit((960, 540)),
                    Some((960, 540)),
                    Err(io::Error::from(io::ErrorKind::InvalidInput)),
                ),
            ]),
            ..Fake::default()
        };
        assert_eq!(resolve_with(&mut io, None).output, None);
        assert!(io.steps.is_empty());
        assert_eq!(io.pauses, 0);
    }

    #[test]
    fn saved_choice_is_only_unusable_when_a_verified_output_does_not_offer_it() {
        let resolved = |output, explicit_applied| Resolved {
            render: (960, 540),
            output,
            explicit_applied,
        };
        let hd = Some((1920, 1080));
        assert!(resolved(hd, false).saved_choice_unusable(Some((1280, 720))));
        assert!(resolved(hd, false).saved_choice_unusable(None));
        assert!(!resolved(hd, true).saved_choice_unusable(hd));
        // Offered, but the resize did not take this time.
        assert!(!resolved(hd, false).saved_choice_unusable(hd));
        assert!(!resolved(None, false).saved_choice_unusable(hd));
    }

    #[test]
    fn failed_auto_application_reports_actual_framebuffer_not_desired_size() {
        let mut io = Fake {
            size: Some((1920, 1080)),
            steps: VecDeque::from([
                (Command::Half, Some((1920, 1080)), Ok(false)),
                (Command::Explicit((1280, 720)), Some((1920, 1080)), Ok(true)),
            ]),
            ..Fake::default()
        };
        assert_eq!(resolve_with(&mut io, None).render, (1920, 1080));
        assert!(io.steps.is_empty());
    }

    #[test]
    fn explicit_and_four_k_auto_use_only_supported_verified_sizes() {
        for requested in [Some((1920, 1080)), None] {
            let target = requested.unwrap_or((1280, 720));
            let mut steps = VecDeque::from([(Command::Half, Some((1920, 1080)), Ok(false))]);
            if requested.is_none() {
                steps.push_back((Command::Explicit(target), Some(target), Ok(true)));
            }
            let mut io = Fake {
                size: Some((1920, 1080)),
                steps,
                ..Fake::default()
            };
            let result = resolve_with(&mut io, requested);
            assert_eq!(result.render, target);
            assert_eq!(result.output, Some((3840, 2160)));
            assert_eq!(result.explicit_applied, requested.is_some());
            assert!(io.steps.is_empty());
        }
    }

    #[test]
    fn parses_geometry_and_distinguishes_exit_one_from_usage_errors() {
        assert_eq!(parse_mode("8888 1 960 540 3840"), Some((960, 540)));
        assert_eq!(parse_virtual_size("1920,1080\n"), Some((1920, 1080)));
        assert_eq!(parse_mode("8888 1 0 540 0"), None);
        assert_eq!(parse_virtual_size("0,1080"), None);
        assert_eq!(parse_virtual_size("bad"), None);
        assert!(matches!(outcome(Some(1), b"done", b""), Ok(false)));
        assert!(matches!(outcome(Some(1), b"... failed!", b""), Ok(true)));
        assert!(outcome(Some(1), b"Usage:", b"").is_err());
        assert!(outcome(Some(1), b"", b"Unknown format").is_err());
        assert!(outcome(Some(2), b"", b"").is_err());
    }

    #[test]
    fn framebuffer_bytes_include_row_padding() {
        assert_eq!(rgb32_bytes((1920, 1080)), RESERVED_BYTES);
        assert_eq!(rgb32_bytes((1921, 1079)), 8_303_984);
        assert!(rgb32_bytes((1921, 1079)) > RESERVED_BYTES);
    }
}
