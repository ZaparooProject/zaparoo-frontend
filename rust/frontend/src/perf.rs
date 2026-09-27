// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

//! Timing milestones for an embedding host. The host installs [`Hooks`];
//! each milestone is handed over as an event name plus `k=v` fields, and the
//! host stamps it with its own clock so every line shares one timeline.
//! Without hooks every call here is a cheap no-op.
//!
//! Frame-driven milestones fire from the window's rendering notifier, on
//! the first frame that actually contains the state being measured.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock, PoisonError};
use std::time::{Duration, Instant};

/// Host callbacks. `mark` receives an event and its space-separated
/// `k=v` fields; `fully_drawn` runs once per window when the first
/// screen is complete (the Hub with its covers).
#[derive(Clone, Copy, Debug)]
pub struct Hooks {
    pub mark: fn(event: &str, fields: &str),
    pub fully_drawn: fn(),
}

static HOOKS: OnceLock<Hooks> = OnceLock::new();
static WINDOW_RENEWED: AtomicBool = AtomicBool::new(false);
static STATE: Mutex<State> = Mutex::new(State::new());

/// Held-scroll recordings stop growing here (about 80 s at 120 Hz).
const MAX_SCROLL_FRAMES: usize = 10_000;

pub fn install(hooks: Hooks) {
    let _ = HOOKS.set(hooks);
}

pub fn enabled() -> bool {
    HOOKS.get().is_some()
}

pub fn mark(event: &str, fields: &str) {
    if let Some(hooks) = HOOKS.get() {
        (hooks.mark)(event, fields);
    }
}

/// The host replaced the native window (a return from another app); the
/// next frame is reported as `window-init-frame`.
pub fn window_renewed() {
    WINDOW_RENEWED.store(true, Ordering::Relaxed);
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Open {
    Idle,
    Pressed,
    RowsQueued,
    RowsPainted,
    CoversQueued,
}

#[derive(Debug)]
struct State {
    first_frame: bool,
    hub_complete: Option<(usize, usize)>,
    hub_reported: bool,
    open: Open,
    open_rows: usize,
    open_visible: usize,
    open_pending: usize,
    scroll: Option<Vec<Duration>>,
    last_frame: Option<Instant>,
}

impl State {
    const fn new() -> Self {
        Self {
            first_frame: false,
            hub_complete: None,
            hub_reported: false,
            open: Open::Idle,
            open_rows: 0,
            open_visible: 0,
            open_pending: 0,
            scroll: None,
            last_frame: None,
        }
    }
}

fn state() -> std::sync::MutexGuard<'static, State> {
    STATE.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Start measuring a new window: reset per-window milestones and follow
/// its frames. Only called when hooks are installed.
#[cfg(feature = "hosted")]
pub(crate) fn attach(app: &crate::App) {
    use slint::ComponentHandle;
    if !enabled() {
        return;
    }
    *state() = State::new();
    WINDOW_RENEWED.store(false, Ordering::Relaxed);
    let weak = app.as_weak();
    let result = app.window().set_rendering_notifier(move |rendering, _| {
        if matches!(rendering, slint::RenderingState::AfterRendering) {
            if let Some(app) = weak.upgrade() {
                let shell = app.global::<crate::Shell>();
                let covered = shell.get_transitioning() || shell.get_boot_curtain();
                after_rendering(shell.get_active_screen(), covered);
            }
        }
    });
    if let Err(error) = result {
        tracing::warn!("perf: no rendering notifier: {error}");
    }
}

/// One rendered frame. `screen` is the active screen; `covered` means it
/// is not what shows yet (a route change still shows its source, or the
/// cold-launch curtain is up).
fn after_rendering(screen: crate::Screen, covered: bool) {
    let now = Instant::now();
    let mut marks: Vec<(&'static str, String)> = Vec::new();
    let mut fully_drawn = false;
    {
        let mut s = state();
        if let Some(previous) = s.last_frame.replace(now) {
            if let Some(frames) = s.scroll.as_mut() {
                if frames.len() < MAX_SCROLL_FRAMES {
                    frames.push(now.saturating_duration_since(previous));
                }
            }
        }
        if !s.first_frame {
            s.first_frame = true;
            WINDOW_RENEWED.store(false, Ordering::Relaxed);
            marks.push(("first-frame", String::new()));
        } else if WINDOW_RENEWED.swap(false, Ordering::Relaxed) {
            marks.push(("window-init-frame", format!("screen={screen:?}")));
        }
        if !s.hub_reported && !covered && screen == crate::Screen::Hub {
            if let Some((tiles, covers)) = s.hub_complete {
                s.hub_reported = true;
                fully_drawn = true;
                marks.push((
                    "hub-covers-painted",
                    format!("tiles={tiles} covers={covers}"),
                ));
            }
        }
        if !covered && screen == crate::Screen::Games {
            match s.open {
                Open::RowsQueued => {
                    marks.push((
                        "open-first-rows",
                        format!("rows={} pending={}", s.open_rows, s.open_pending),
                    ));
                    s.open = Open::RowsPainted;
                    if s.open_pending == 0 {
                        marks.push((
                            "open-covers-complete",
                            format!("visible={}", s.open_visible),
                        ));
                        s.open = Open::Idle;
                    }
                }
                Open::CoversQueued => {
                    marks.push((
                        "open-covers-complete",
                        format!("visible={}", s.open_visible),
                    ));
                    s.open = Open::Idle;
                }
                Open::Idle | Open::Pressed | Open::RowsPainted => {}
            }
        }
    }
    for (event, fields) in marks {
        mark(event, &fields);
    }
    if fully_drawn {
        if let Some(hooks) = HOOKS.get() {
            (hooks.fully_drawn)();
        }
    }
}

/// The Hub page just pushed: whether its live state has `settled` (Core
/// answered), `tiles` on the page, `covers` of them that show game art,
/// and `outstanding` of those still waiting for it.
pub(crate) fn hub_rendered(settled: bool, tiles: usize, covers: usize, outstanding: usize) {
    if !enabled() {
        return;
    }
    state().hub_complete = (settled && tiles > 0 && outstanding == 0).then_some((tiles, covers));
}

/// Accept on a system or folder: start timing its first rows and covers.
pub(crate) fn open_pressed(kind: &str) {
    if !enabled() {
        return;
    }
    {
        let mut s = state();
        s.open = Open::Pressed;
        s.open_rows = 0;
        s.open_visible = 0;
        s.open_pending = 0;
    }
    mark("open-press", &format!("kind={kind}"));
}

/// The games view just pushed: its row count, whether it is still
/// loading, the visible rows and how many of them still wait for art.
pub(crate) fn games_rendered(rows: usize, loading: bool, visible: usize, pending: usize) {
    if !enabled() {
        return;
    }
    let mut s = state();
    match s.open {
        Open::Pressed if rows > 0 && !loading => {
            s.open = Open::RowsQueued;
            s.open_rows = rows;
        }
        Open::RowsPainted if pending == 0 => s.open = Open::CoversQueued,
        _ => {}
    }
    s.open_visible = visible;
    s.open_pending = pending;
}

/// A held fast scroll started or ended. The end logs frame intervals
/// rendered while it ran.
pub(crate) fn scroll(active: bool) {
    if !enabled() {
        return;
    }
    let frames = {
        let mut s = state();
        if active {
            if s.scroll.is_none() {
                s.scroll = Some(Vec::new());
                s.last_frame = None;
            }
            return;
        }
        s.scroll.take()
    };
    let Some(mut frames) = frames else {
        return;
    };
    let fields = match summarize(&mut frames) {
        Some(summary) => format!(
            "frames={} p50_ms={:.1} p99_ms={:.1} max_ms={:.1}",
            summary.count,
            ms(summary.p50),
            ms(summary.p99),
            ms(summary.max),
        ),
        None => "frames=0".to_string(),
    };
    mark("scroll-frames", &fields);
}

fn ms(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1000.0
}

/// Percentiles over a set of frame durations.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Summary {
    pub count: usize,
    pub avg: Duration,
    pub p50: Duration,
    pub p99: Duration,
    pub max: Duration,
}

/// Sorts `samples` in place; `None` when empty.
pub(crate) fn summarize(samples: &mut [Duration]) -> Option<Summary> {
    if samples.is_empty() {
        return None;
    }
    samples.sort_unstable();
    let count = samples.len();
    let total: Duration = samples.iter().sum();
    Some(Summary {
        count,
        avg: total / u32::try_from(count).unwrap_or(u32::MAX),
        p50: samples[count / 2],
        p99: samples[count * 99 / 100],
        max: samples[count - 1],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summary_orders_and_picks_percentiles() {
        let mut samples: Vec<Duration> = (1..=100).rev().map(Duration::from_millis).collect();
        let summary = summarize(&mut samples);
        assert_eq!(
            summary,
            Some(Summary {
                count: 100,
                avg: Duration::from_micros(50_500),
                p50: Duration::from_millis(51),
                p99: Duration::from_millis(100),
                max: Duration::from_millis(100),
            })
        );
        assert_eq!(summarize(&mut []), None);
    }

    #[test]
    fn hooks_absent_leave_state_untouched() {
        // No test installs hooks, so recording is disabled.
        open_pressed("system");
        games_rendered(10, false, 10, 0);
        assert_eq!(state().open, Open::Idle);
    }
}
