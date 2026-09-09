// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

//! Qt's verified digital-output probe. Never confuse inherited/downscaled fb0
//! geometry with HDMI timing, or claim a requested size without readback.

use crate::display::{automatic_size, selectable_sizes};
use std::io;
use std::sync::OnceLock;

type Size = (u32, u32);
const MODE_PATH: &str = "/sys/module/MiSTer_fb/parameters/mode";
static RESOLVED: OnceLock<Resolved> = OnceLock::new();

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Resolved {
    pub render: Size,
    pub output: Option<Size>,
    pub explicit_applied: bool,
}

pub fn output_size() -> Option<Size> {
    RESOLVED.get().and_then(|resolved| resolved.output)
}

pub fn resolve(requested: Option<Size>) -> Resolved {
    let resolved = resolve_with(&mut Native, requested);
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
    fn run(&mut self, command: Command) -> io::Result<bool>;
}

fn probe(io: &mut impl VideoIo, command: Command) -> Option<Size> {
    let before = io.size()?;
    let timed_out = io.run(command).ok()?;
    let after = io.size()?;
    (!timed_out || before != after).then_some(after)
}

fn apply(io: &mut impl VideoIo, command: Command, target: Size) -> bool {
    io.run(command).is_ok() && io.size() == Some(target)
}

fn resolve_with(io: &mut impl VideoIo, requested: Option<Size>) -> Resolved {
    let inherited = io.size();
    let output = inherited.and_then(|_| {
        probe(io, Command::Full).or_else(|| {
            let half = probe(io, Command::Half)?;
            Some((half.0.checked_mul(2)?, half.1.checked_mul(2)?))
        })
    });
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

struct Native;
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

    fn run(&mut self, command: Command) -> io::Result<bool> {
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
        let output = process.output()?;
        let result = outcome(output.status.code(), &output.stdout, &output.stderr);
        if let Err(error) = &result {
            tracing::warn!(?command, %error, "MiSTer video command failed");
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    struct Fake {
        size: Option<Size>,
        steps: VecDeque<(Command, Option<Size>, io::Result<bool>)>,
    }
    impl VideoIo for Fake {
        fn size(&self) -> Option<Size> {
            self.size
        }
        #[allow(
            clippy::panic,
            reason = "unexpected commands must fail the test, not simulate device rejection"
        )]
        fn run(&mut self, command: Command) -> io::Result<bool> {
            let Some((expected, after, outcome)) = self.steps.pop_front() else {
                panic!("unexpected video command: {command:?}");
            };
            assert_eq!(command, expected);
            self.size = after;
            outcome
        }
    }

    #[test]
    fn auto_1080_probes_output_then_applies_verified_half_size() {
        let mut io = Fake {
            size: Some((960, 540)),
            steps: VecDeque::from([
                (Command::Full, Some((1920, 1080)), Ok(false)),
                (Command::Half, Some((960, 540)), Ok(true)),
            ]),
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
    }

    #[test]
    fn ambiguous_full_probe_can_recover_output_from_changed_half_geometry() {
        let mut io = Fake {
            size: Some((1920, 1080)),
            steps: VecDeque::from([
                (Command::Full, Some((1920, 1080)), Ok(true)),
                (Command::Half, Some((960, 540)), Ok(true)),
            ]),
        };
        let result = resolve_with(&mut io, Some((1280, 720)));
        assert_eq!(result.render, (960, 540));
        assert_eq!(result.output, Some((1920, 1080)));
        assert!(!result.explicit_applied);
        assert!(io.steps.is_empty());
    }

    #[test]
    fn unverified_output_does_not_offer_explicit_choices() {
        let mut io = Fake {
            size: Some((960, 540)),
            steps: VecDeque::from([
                (Command::Full, Some((960, 540)), Ok(true)),
                (Command::Half, Some((960, 540)), Ok(true)),
            ]),
        };
        assert_eq!(
            resolve_with(&mut io, Some((1920, 1080))),
            Resolved {
                render: (960, 540),
                output: None,
                explicit_applied: false
            }
        );
    }

    #[test]
    fn failed_auto_application_reports_actual_framebuffer_not_desired_size() {
        let mut io = Fake {
            size: Some((1920, 1080)),
            steps: VecDeque::from([
                (Command::Full, Some((1920, 1080)), Ok(false)),
                (Command::Half, Some((1920, 1080)), Ok(true)),
                (Command::Explicit((960, 540)), Some((1920, 1080)), Ok(true)),
            ]),
        };
        assert_eq!(resolve_with(&mut io, None).render, (1920, 1080));
    }

    #[test]
    fn explicit_and_four_k_auto_use_only_supported_verified_sizes() {
        for requested in [Some((1920, 1080)), None] {
            let target = requested.unwrap_or((1280, 720));
            let mut io = Fake {
                size: Some((1280, 720)),
                steps: VecDeque::from([
                    (Command::Full, Some((3840, 2160)), Ok(false)),
                    (Command::Explicit(target), Some(target), Ok(true)),
                ]),
            };
            let result = resolve_with(&mut io, requested);
            assert_eq!(result.render, target);
            assert_eq!(result.explicit_applied, requested.is_some());
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
}
