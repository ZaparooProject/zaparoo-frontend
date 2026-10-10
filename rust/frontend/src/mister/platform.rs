// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// Custom `slint::platform::Platform` for the MiSTer target.
//
// Frame loop shape: dispatch a bounded batch of callbacks, poll input,
// advance timers/animations, render if dirty, then present. Vsync paces
// presentation. The clock timers and animations read is a stepped timeline
// (`zaparoo_app::frame_clock`): one refresh period per presented frame, so
// motion is sampled at even steps and a late frame slows it by that frame,
// and the real time that passed, in whole periods, across a turn that
// presented nothing, so timers keep real time while nothing animates. The
// `clock` name of the `ZAPAROO_MOTION` test switch puts wall time back.

pub use super::latch::ResolutionPolicy;
use super::{
    ddr::DdrPresenter, fb0::Fb0Presenter, input::InputReader, latch::LatchPresenter, Presenter,
};
use slint::platform::software_renderer::{
    MinimalSoftwareWindow, RenderingRotation, RepaintBufferType,
};
use slint::platform::{Platform, WindowAdapter};
use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicU8, Ordering};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::{Duration, Instant};
use zaparoo_app::frame_clock::{FrameClock, RefreshEstimate, Turn};

/// Nominal presentation budget, independent of the animation clock.
const FRAME_PERIOD_US: u64 = 16_667;
const CALLBACK_BUDGET: Duration = Duration::from_millis(2);
const CALLBACK_LIMIT: usize = 32;

/// DRS policy, heavy-phase scoped: motion res only while the router
/// declares a phase that repaints the whole viewport for a sustained
/// stretch (page swoops - see `crate::drs`). Ordinary navigation
/// stays native: small dirty rects are cheap there, and every switch
/// the old input-driven policy made on a quiet screen was a visible
/// texture snap. Scoped switches land while the entire frame is
/// already in motion, which masks them.
///
/// The pop after a phase is fast (a couple of quiet frames, under
/// cover of the fresh content); a pop after an unforeseen-burst
/// retreat waits much longer so a sustained burst can't oscillate.
/// The grace window still exempts the pop's own full re-render from
/// the retreat check.
///
/// The retreat fires only on SUSTAINED overruns of the vsync budget:
/// a single late frame presents as one latched slow cut (invisible),
/// and ordinary focus moves at native res cost right around the
/// budget - a single-frame threshold turned every cursor move into a
/// drop-then-pop cycle, which is the exact visibility problem the
/// scoping exists to kill.
const POP_QUIET_FAST: u32 = 2;
const POP_QUIET_RETREAT: u32 = 30;
const RETREAT_CONSECUTIVE: u32 = 3;
const SHARP_GRACE_FRAMES: u32 = 3;

const ROTATION_NONE: u8 = 0;
const ROTATION_CW: u8 = 1;
const ROTATION_CCW: u8 = 2;
static REQUESTED_ROTATION: AtomicU8 = AtomicU8::new(ROTATION_NONE);

fn rotation_value(value: crate::Orientation) -> u8 {
    match value {
        crate::Orientation::Cw => ROTATION_CW,
        crate::Orientation::Ccw => ROTATION_CCW,
        crate::Orientation::Horizontal => ROTATION_NONE,
    }
}

fn requested_rotation() -> RenderingRotation {
    match REQUESTED_ROTATION.load(Ordering::SeqCst) {
        ROTATION_CW => RenderingRotation::Rotate90,
        ROTATION_CCW => RenderingRotation::Rotate270,
        _ => RenderingRotation::NoRotation,
    }
}

fn logical_size(size: (u32, u32), rotation: RenderingRotation) -> (u32, u32) {
    if matches!(
        rotation,
        RenderingRotation::Rotate90 | RenderingRotation::Rotate270
    ) {
        (size.1, size.0)
    } else {
        size
    }
}

fn apply_window_geometry(
    presenter: &dyn Presenter,
    window: &MinimalSoftwareWindow,
    rotation: RenderingRotation,
) {
    window
        .window()
        .dispatch_event(slint::platform::WindowEvent::ScaleFactorChanged {
            scale_factor: presenter.window_scale_factor(),
        });
    let (width, height) = logical_size(presenter.size(), rotation);
    window.set_size(slint::PhysicalSize::new(width, height));
}

pub fn set_orientation(value: crate::Orientation) {
    REQUESTED_ROTATION.store(rotation_value(value), Ordering::SeqCst);
}

/// The stepped timeline in nanoseconds, published for the cached page
/// slide, which counts its steps on the clock Slint samples. `u64::MAX`
/// while no frame loop is stepping one and wall time drives everything.
static TIMELINE_NANOS: AtomicU64 = AtomicU64::new(u64::MAX);

/// The stepped timeline's current value, or None where wall time is the
/// animation clock.
pub(super) fn timeline() -> Option<Duration> {
    match TIMELINE_NANOS.load(Ordering::SeqCst) {
        u64::MAX => None,
        nanos => Some(Duration::from_nanos(nanos)),
    }
}

/// What timers and animations read as the time.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AnimationClock {
    /// A timeline the frame loop steps one refresh period per presented
    /// frame.
    FixedStep,
    /// Wall time, whatever was presented.
    Wall,
}

type QueuedEvent = Box<dyn FnOnce() + Send>;

#[derive(Default)]
struct EventQueue {
    queue: Mutex<VecDeque<QueuedEvent>>,
    wake: Condvar,
    quit: AtomicBool,
}

impl EventQueue {
    fn push(&self, event: QueuedEvent) {
        self.queue
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push_back(event);
        self.wake.notify_one();
    }

    fn dispatch(&self) {
        let started = Instant::now();
        self.dispatch_while(|| started.elapsed() < CALLBACK_BUDGET);
    }

    // Never hold the queue lock while invoking user code. New callbacks
    // stay behind existing work; a busy producer cannot starve rendering.
    fn dispatch_while(&self, mut within_budget: impl FnMut() -> bool) {
        for index in 0..CALLBACK_LIMIT {
            if self.quit.load(Ordering::SeqCst) || (index > 0 && !within_budget()) {
                break;
            }
            let event = self
                .queue
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .pop_front();
            let Some(event) = event else { break };
            event();
        }
    }
}

struct Proxy(Arc<EventQueue>);

impl slint::platform::EventLoopProxy for Proxy {
    fn quit_event_loop(&self) -> Result<(), slint::EventLoopError> {
        self.0.quit.store(true, Ordering::SeqCst);
        self.0.wake.notify_one();
        Ok(())
    }

    fn invoke_from_event_loop(
        &self,
        event: Box<dyn FnOnce() + Send>,
    ) -> Result<(), slint::EventLoopError> {
        self.0.push(event);
        Ok(())
    }
}

struct DynamicResolution {
    enabled: bool,
    high: bool,
    force_full_redraw: bool,
    idle_frames: u32,
    sharp_grace: u32,
    pop_quiet: u32,
    over_budget_streak: u32,
}

impl DynamicResolution {
    fn new(enabled: bool) -> Self {
        Self {
            enabled,
            high: false,
            force_full_redraw: false,
            idle_frames: 0,
            sharp_grace: 0,
            pop_quiet: POP_QUIET_FAST,
            over_budget_streak: 0,
        }
    }

    fn set_mode(
        &mut self,
        presenter: &mut dyn Presenter,
        window: &MinimalSoftwareWindow,
        rotation: RenderingRotation,
        high: bool,
    ) {
        self.high = high;
        presenter.set_res_mode(high);
        apply_window_geometry(presenter, window, rotation);
        // Reused-buffer damage caches and the backing frame both use
        // physical stride. A geometry switch must repaint every pixel
        // before either latch slot receives the new stride.
        self.force_full_redraw = true;
    }

    fn needs_full_redraw(&self) -> bool {
        self.force_full_redraw
    }

    fn full_redraw_complete(&mut self) {
        self.force_full_redraw = false;
    }

    fn before_render(
        &mut self,
        presenter: &mut dyn Presenter,
        window: &MinimalSoftwareWindow,
        rotation: RenderingRotation,
    ) {
        if self.enabled && self.high && crate::drs::heavy_active() {
            self.set_mode(presenter, window, rotation, false);
        }
    }

    fn rendered(
        &mut self,
        busy: Duration,
        presenter: &mut dyn Presenter,
        window: &MinimalSoftwareWindow,
        rotation: RenderingRotation,
    ) {
        self.idle_frames = 0;
        self.sharp_grace = self.sharp_grace.saturating_sub(1);
        if busy > FRAME_BUDGET && self.sharp_grace == 0 {
            self.over_budget_streak = self.over_budget_streak.saturating_add(1);
        } else {
            self.over_budget_streak = 0;
        }
        if self.enabled && self.high && self.over_budget_streak >= RETREAT_CONSECUTIVE {
            self.over_budget_streak = 0;
            self.pop_quiet = POP_QUIET_RETREAT;
            self.set_mode(presenter, window, rotation, false);
        }
    }

    fn idle(
        &mut self,
        presenter: &mut dyn Presenter,
        window: &MinimalSoftwareWindow,
        rotation: RenderingRotation,
    ) {
        self.idle_frames = self.idle_frames.saturating_add(1);
        if self.enabled
            && !self.high
            && !crate::drs::heavy_active()
            && self.idle_frames >= self.pop_quiet
            && !window.window().has_active_animations()
        {
            self.sharp_grace = SHARP_GRACE_FRAMES;
            self.pop_quiet = POP_QUIET_FAST;
            self.over_budget_streak = 0;
            self.set_mode(presenter, window, rotation, true);
        }
    }
}

pub struct MisterPlatform {
    windows: RefCell<Vec<Rc<MinimalSoftwareWindow>>>,
    queue: Arc<EventQueue>,
    /// Wall-clock origin. Timers and interpolated motion read the time
    /// since it directly until the frame loop starts its stepped timeline,
    /// and for the whole run on a wall `animation_clock`.
    started: Instant,
    animation_clock: AnimationClock,
    /// The stepped timeline, once the frame loop has started it.
    clock: Cell<Option<FrameClock>>,
    /// CRT native path: present through the DDR contract instead of fb0.
    crt: bool,
    crt_size: (u32, u32),
    crt_offsets: (i32, i32),
    /// Render normal HDMI and CRT-profile component instances together.
    dual_head: bool,
    /// Main offered its scanout lease. Off the native CRT path that is the
    /// HDMI slots (try the vblank-latch presenter); on it, the CRT window
    /// mapped through the module instead of /dev/mem, and a dual-head HDMI
    /// side stays on fb0.
    latch: bool,
    /// Select adaptive motion/settled geometry or keep the sharpest
    /// supported latch geometry fixed.
    resolution_policy: ResolutionPolicy,
}

impl MisterPlatform {
    fn new(
        crt: bool,
        crt_size: (u32, u32),
        crt_offsets: (i32, i32),
        latch: bool,
        dual_head: bool,
        resolution_policy: ResolutionPolicy,
        animation_clock: AnimationClock,
    ) -> Self {
        Self {
            windows: RefCell::new(Vec::new()),
            queue: Arc::new(EventQueue::default()),
            started: Instant::now(),
            animation_clock,
            clock: Cell::new(None),
            crt,
            crt_size,
            crt_offsets,
            dual_head,
            latch,
            resolution_policy,
        }
    }

    /// Starts the stepped timeline level with wall time. The frame loop
    /// calls this once, before its first turn.
    fn start_clock(&self) {
        self.start_clock_at(self.started.elapsed());
    }

    fn start_clock_at(&self, real_now: Duration) {
        if self.animation_clock == AnimationClock::FixedStep {
            self.set_clock(FrameClock::new(real_now));
        }
    }

    /// Accounts for the turn that just ended. Each frame loop calls this
    /// exactly once per turn, before Slint samples the clock for that turn.
    fn advance_clock(&self, previous: Option<Turn>, period: Duration) {
        self.advance_clock_at(previous, self.started.elapsed(), period);
    }

    fn advance_clock_at(&self, previous: Option<Turn>, real_now: Duration, period: Duration) {
        let (Some(mut clock), Some(previous)) = (self.clock.get(), previous) else {
            return;
        };
        clock.advance(previous, real_now, period);
        self.set_clock(clock);
    }

    fn set_clock(&self, clock: FrameClock) {
        self.clock.set(Some(clock));
        TIMELINE_NANOS.store(
            u64::try_from(clock.now().as_nanos()).unwrap_or(u64::MAX - 1),
            Ordering::SeqCst,
        );
    }

    /// --latch wants the vblank-latch presenter (kernel module +
    /// latch RBF + Main's uio lease). A failed probe falls back to
    /// fb0 loudly because it changes tearing and DRS behavior.
    fn make_hdmi_presenter(&self) -> Result<Box<dyn Presenter>, slint::PlatformError> {
        if self.latch && !self.crt {
            match LatchPresenter::open(self.resolution_policy, true) {
                Ok(p) => return Ok(Box::new(p)),
                Err(e) => {
                    tracing::warn!("latch presenter unavailable, falling back to fb0: {e}");
                }
            }
        }
        Ok(Box::new(Fb0Presenter::open(
            !crate::motion_test().slides(),
        )?))
    }

    /// CRT wants the DDR presenter; normal HDMI uses the latch-first
    /// path above. Failures are loud because fallback changes output
    /// behavior visibly.
    fn make_presenter(&self) -> Result<Box<dyn Presenter>, slint::PlatformError> {
        if self.crt {
            match DdrPresenter::open(
                self.crt_size.0,
                self.crt_size.1,
                self.crt_offsets.0,
                self.crt_offsets.1,
                true,
                self.latch,
                !crate::motion_test().slides(),
            ) {
                Ok(p) => return Ok(Box::new(p)),
                Err(e) => {
                    tracing::warn!("DDR presenter unavailable, falling back to fb0: {e}");
                }
            }
        }
        self.make_hdmi_presenter()
    }

    fn run_dual_event_loop(
        &self,
        hdmi_window: &MinimalSoftwareWindow,
        crt_window: &MinimalSoftwareWindow,
    ) -> Result<(), slint::PlatformError> {
        let _tty = super::tty::TtyGuard::acquire();
        let mut hdmi = self.make_hdmi_presenter()?;
        let mut crt: Box<dyn Presenter> = Box::new(DdrPresenter::open(
            self.crt_size.0,
            self.crt_size.1,
            self.crt_offsets.0,
            self.crt_offsets.1,
            false,
            self.latch,
            // The HDMI presenter paces this pair and owns its cached page
            // slides; the CRT head replays the page as a live strip.
            false,
        )?);
        let mut rotation = requested_rotation();
        let (hdmi_w, hdmi_h) = logical_size(hdmi.size(), rotation);
        let (crt_w, crt_h) = logical_size(crt.size(), rotation);
        apply_window_geometry(hdmi.as_ref(), hdmi_window, rotation);
        apply_window_geometry(crt.as_ref(), crt_window, rotation);

        let mut input = InputReader::open();
        let mut profile = FrameProfile::default();
        let mut drs = DynamicResolution::new(hdmi.supports_res_modes());
        tracing::info!(
            hdmi_width = hdmi_w,
            hdmi_height = hdmi_h,
            crt_width = crt_w,
            crt_height = crt_h,
            drs = drs.enabled,
            "mister platform: entering dual-head frame loop"
        );

        self.start_clock();
        let mut pace = Pace::new();
        let mut previous = None;
        loop {
            if self.queue.quit.load(Ordering::SeqCst) {
                return Ok(());
            }
            let turn_started = Instant::now();
            self.queue.dispatch();
            let _ = input.poll(hdmi_window.window());

            let requested = requested_rotation();
            if requested != rotation {
                rotation = requested;
                apply_window_geometry(hdmi.as_ref(), hdmi_window, rotation);
                apply_window_geometry(crt.as_ref(), crt_window, rotation);
            }
            if crt.sync_controls().is_some() {
                apply_window_geometry(crt.as_ref(), crt_window, rotation);
            }
            // The HDMI presenter paces both heads, so its period is the step.
            let period = pace.period(hdmi.as_ref());
            self.advance_clock(previous, period);
            slint::platform::update_timers_and_animations();
            drs.before_render(hdmi.as_mut(), hdmi_window, rotation);

            let preparation = turn_started.elapsed();
            let force_full_redraw = drs.needs_full_redraw();
            let mut hdmi_busy = hdmi.present_cached_transition();
            let hdmi_cached = hdmi_busy.is_some();
            let hdmi_rendered = hdmi_cached
                || hdmi_window.draw_if_needed(|renderer| {
                    renderer.set_rendering_rotation(rotation);
                    if force_full_redraw {
                        renderer.set_repaint_buffer_type(RepaintBufferType::NewBuffer);
                    }
                    hdmi_busy = Some(hdmi.render_and_present(renderer));
                    if force_full_redraw {
                        renderer.set_repaint_buffer_type(RepaintBufferType::ReusedBuffer);
                    }
                });
            if hdmi_rendered && force_full_redraw {
                drs.full_redraw_complete();
            }
            let mut crt_busy = None;
            crt_window.draw_if_needed(|renderer| {
                renderer.set_rendering_rotation(rotation);
                crt_busy = Some(crt.render_and_present(renderer));
            });
            let total_busy = hdmi_busy.unwrap_or_default() + crt_busy.unwrap_or_default();
            if hdmi_rendered || crt_busy.is_some() {
                pace.drew();
            }
            if hdmi_busy.is_some() {
                // HDMI resolution is the available pressure valve,
                // but dual-head budget includes both render targets.
                drs.rendered(total_busy, hdmi.as_mut(), hdmi_window, rotation);
            } else if !hdmi_rendered {
                pace.idle_wait(hdmi.as_mut());
                drs.idle(hdmi.as_mut(), hdmi_window, rotation);
            }
            if hdmi_busy.is_some() || crt_busy.is_some() {
                profile.record(total_busy, hdmi.last_copy() + crt.last_copy(), period);
            }
            // A turn that drew only the CRT head still waited one HDMI
            // refresh above.
            previous = Some(if hdmi_rendered || crt_busy.is_some() {
                Turn::Presented
            } else {
                Turn::Idle
            });
            profile.record_turn(preparation, turn_started.elapsed());
        }
    }
}

impl Platform for MisterPlatform {
    fn create_window_adapter(&self) -> Result<Rc<dyn WindowAdapter>, slint::PlatformError> {
        let window = MinimalSoftwareWindow::new(RepaintBufferType::ReusedBuffer);
        self.windows.borrow_mut().push(window.clone());
        Ok(window)
    }

    fn duration_since_start(&self) -> Duration {
        self.clock
            .get()
            .map_or_else(|| self.started.elapsed(), |clock| clock.now())
    }

    fn new_event_loop_proxy(&self) -> Option<Box<dyn slint::platform::EventLoopProxy>> {
        Some(Box::new(Proxy(self.queue.clone())))
    }

    #[allow(
        clippy::too_many_lines,
        reason = "the frame loop reads best as one piece: pacing, DRS policy, and render belong together"
    )]
    fn run_event_loop(&self) -> Result<(), slint::PlatformError> {
        let windows = self.windows.borrow().clone();
        if self.dual_head {
            if windows.len() != 2 {
                return Err(slint::PlatformError::Other(format!(
                    "dual-head mode requires two windows, got {}",
                    windows.len()
                )));
            }
            return self.run_dual_event_loop(windows[0].as_ref(), windows[1].as_ref());
        }
        let window = windows
            .first()
            .cloned()
            .ok_or_else(|| slint::PlatformError::Other("no window created".into()))?;

        // Silence the kernel console for the whole frame-loop lifetime
        // where startup could not already; dropped (KD_TEXT restored) on
        // clean exit. The wrapper resets the tty itself if we die without
        // unwinding.
        let _tty = super::tty::TtyGuard::acquire();

        let mut presenter = self.make_presenter()?;
        let mut rotation = requested_rotation();
        let (width, height) = logical_size(presenter.size(), rotation);
        apply_window_geometry(presenter.as_ref(), window.as_ref(), rotation);

        let mut input = InputReader::open();
        let mut profile = FrameProfile::default();
        let mut drs = DynamicResolution::new(presenter.supports_res_modes());
        tracing::info!(
            width,
            height,
            drs = drs.enabled,
            "mister platform: entering frame loop"
        );

        self.start_clock();
        let mut pace = Pace::new();
        let mut previous = None;
        loop {
            if self.queue.quit.load(Ordering::SeqCst) {
                return Ok(());
            }
            let turn_started = Instant::now();
            self.queue.dispatch();
            // Input dispatches only - it no longer drives DRS.
            let _ = input.poll(window.window());
            let requested = requested_rotation();
            if requested != rotation {
                rotation = requested;
                apply_window_geometry(presenter.as_ref(), window.as_ref(), rotation);
            }
            if presenter.sync_controls().is_some() {
                apply_window_geometry(presenter.as_ref(), window.as_ref(), rotation);
            }
            let period = pace.period(presenter.as_ref());
            self.advance_clock(previous, period);
            slint::platform::update_timers_and_animations();

            // Drop to motion res the moment a router-declared heavy
            // phase begins, BEFORE its first frame renders.
            drs.before_render(presenter.as_mut(), window.as_ref(), rotation);

            // Cached page motion owns presentation while active. Slint's
            // canonical destination buffer stays untouched between endpoint
            // renders, and pending timer dirt remains queued for the first
            // normal frame after the transition.
            let preparation = turn_started.elapsed();
            if let Some(busy) = presenter.present_cached_transition() {
                profile.record(busy, Duration::ZERO, period);
                profile.record_turn(preparation, turn_started.elapsed());
                pace.drew();
                previous = Some(Turn::Presented);
                continue;
            }

            let force_full_redraw = drs.needs_full_redraw();
            let mut busy = None;
            let rendered = window.draw_if_needed(|renderer| {
                renderer.set_rendering_rotation(rotation);
                if force_full_redraw {
                    renderer.set_repaint_buffer_type(RepaintBufferType::NewBuffer);
                }
                busy = Some(presenter.render_and_present(renderer));
                if force_full_redraw {
                    renderer.set_repaint_buffer_type(RepaintBufferType::ReusedBuffer);
                }
            });
            if rendered && force_full_redraw {
                drs.full_redraw_complete();
            }
            if let Some(busy) = busy {
                profile.record(busy, presenter.last_copy(), period);
                // Safety net: sustained native-resolution overruns
                // retreat to motion resolution until a quiet stretch.
                drs.rendered(busy, presenter.as_mut(), window.as_ref(), rotation);
            }
            // Idle turns still wait for vsync, and the next turn puts the
            // real time that wait took on the clock, so timer deadlines
            // keep real time whether or not a turn painted.
            if rendered {
                pace.drew();
            } else {
                pace.idle_wait(presenter.as_mut());
                // Phase over and settled: pop to native. The resize
                // dirties one final sharp frame while nothing moves.
                drs.idle(presenter.as_mut(), window.as_ref(), rotation);
            }
            previous = Some(if rendered {
                Turn::Presented
            } else {
                Turn::Idle
            });
            profile.record_turn(preparation, turn_started.elapsed());
        }
    }
}

/// The refresh period a frame loop steps and budgets by. A presenter that
/// knows its output's rate reports it; for the rest it is measured from the
/// vertical-blank waits of turns that drew nothing, so a 50 Hz HDMI mode
/// steps 20 ms instead of running its motion a sixth slow.
struct Pace {
    estimate: RefreshEstimate,
    last_blank: Option<Instant>,
}

impl Pace {
    fn new() -> Self {
        Self {
            estimate: RefreshEstimate::new(Duration::from_micros(FRAME_PERIOD_US)),
            last_blank: None,
        }
    }

    fn period(&self, presenter: &dyn Presenter) -> Duration {
        if presenter.reports_period() {
            presenter.frame_period()
        } else {
            self.estimate.period()
        }
    }

    /// Waits out a turn that drew nothing.
    fn idle_wait(&mut self, presenter: &mut dyn Presenter) {
        presenter.wait_vsync();
        let now = Instant::now();
        if let Some(last) = self.last_blank.replace(now) {
            self.estimate.observe(now.duration_since(last));
        }
    }

    /// A turn that drew: its wait may span more than one refresh.
    fn drew(&mut self) {
        self.last_blank = None;
    }
}

/// Rolling frame-work profiler: records the busy time of every
/// rendered frame (render + copy + publish, vsync waits excluded) and
/// logs avg/p99/max against the presenter's refresh period every 600
/// rendered frames, plus a running overrun count and how much of the
/// average went on the copy out to the display's memory. This is the
/// number to check against the frame budget on hardware.
#[derive(Default)]
struct FrameProfile {
    samples: Vec<Duration>,
    copy_total: Duration,
    total_rendered: u64,
    overruns: u64,
    turns: usize,
    preparation_max: Duration,
    turn_max: Duration,
}

const FRAME_BUDGET: Duration = Duration::from_micros(FRAME_PERIOD_US);
const PROFILE_WINDOW: usize = 600;

impl FrameProfile {
    /// Include callback, input, timer, and layout work, even on turns
    /// that never render. Total turn time also includes vsync waits.
    fn record_turn(&mut self, preparation: Duration, total: Duration) {
        self.turns += 1;
        self.preparation_max = self.preparation_max.max(preparation);
        self.turn_max = self.turn_max.max(total);
        if preparation > FRAME_BUDGET {
            tracing::debug!(
                preparation_us = preparation.as_micros() as u64,
                turn_us = total.as_micros() as u64,
                "UI preparation exceeded frame budget"
            );
        }
        if self.turns >= PROFILE_WINDOW {
            tracing::info!(
                preparation_max_us = self.preparation_max.as_micros() as u64,
                turn_max_us = self.turn_max.as_micros() as u64,
                "event loop profile (turn includes vsync)"
            );
            self.turns = 0;
            self.preparation_max = Duration::ZERO;
            self.turn_max = Duration::ZERO;
        }
    }

    fn record(&mut self, busy: Duration, copy: Duration, budget: Duration) {
        self.total_rendered += 1;
        if busy > budget {
            self.overruns += 1;
        }
        self.samples.push(busy);
        self.copy_total += copy;
        if self.samples.len() < PROFILE_WINDOW {
            return;
        }
        let copy_avg = self.copy_total / PROFILE_WINDOW as u32;
        if let Some(summary) = crate::perf::summarize(&mut self.samples) {
            tracing::info!(
                rendered = self.total_rendered,
                avg_us = summary.avg.as_micros() as u64,
                copy_avg_us = copy_avg.as_micros() as u64,
                p99_us = summary.p99.as_micros() as u64,
                max_us = summary.max.as_micros() as u64,
                budget_us = budget.as_micros() as u64,
                overruns = self.overruns,
                "frame profile"
            );
        }
        self.samples.clear();
        self.copy_total = Duration::ZERO;
    }
}

/// Install the `MiSTer` platform. Must run before the first component
/// is created. `crt` selects the DDR contract presenter; `dual_head`
/// adds a full-resolution fb0 window beside it. The offsets come from
/// launcher config and are clamped again at the contract edge.
pub fn install_platform(
    crt: bool,
    crt_size: (u32, u32),
    crt_offsets: (i32, i32),
    latch: bool,
    dual_head: bool,
    resolution_policy: ResolutionPolicy,
    orientation: &str,
) -> Result<(), slint::PlatformError> {
    set_orientation(
        crate::Orientation::try_from(orientation).unwrap_or(crate::Orientation::Horizontal),
    );
    slint::platform::set_platform(Box::new(MisterPlatform::new(
        crt,
        crt_size,
        crt_offsets,
        latch,
        dual_head,
        resolution_policy,
        if crate::motion_test().clock() {
            AnimationClock::Wall
        } else {
            AnimationClock::FixedStep
        },
    )))
    .map_err(|e| slint::PlatformError::Other(format!("set_platform: {e:?}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn platform(animation_clock: AnimationClock) -> MisterPlatform {
        MisterPlatform::new(
            false,
            (352, 240),
            (0, 0),
            false,
            false,
            ResolutionPolicy::Adaptive,
            animation_clock,
        )
    }

    #[test]
    fn clock_advances_without_presented_frames() {
        const PERIOD: Duration = Duration::from_micros(16_667);
        let mut platform = platform(AnimationClock::FixedStep);
        let started = Instant::now().checked_sub(Duration::from_secs(2));
        assert!(started.is_some(), "clock supports fixture offset");
        let Some(started) = started else { return };
        platform.started = started;
        // Before the frame loop starts its timeline the clock is wall time.
        assert!(platform.duration_since_start() >= Duration::from_secs(2));

        let start = Duration::from_secs(2);
        platform.start_clock_at(start);
        assert_eq!(platform.duration_since_start(), start);
        assert_eq!(timeline(), Some(start));
        // The first turn has nothing to account for.
        platform.advance_clock_at(None, start + Duration::from_millis(5), PERIOD);
        assert_eq!(platform.duration_since_start(), start);

        // Turns that present nothing put the real time that passed on the
        // clock, in whole periods: three seconds is 179 of them and a rest.
        let mut real = start + Duration::from_secs(3);
        platform.advance_clock_at(Some(Turn::Idle), real, PERIOD);
        assert_eq!(platform.duration_since_start(), start + PERIOD * 179);
        // Between turns it does not move at all.
        assert_eq!(platform.duration_since_start(), start + PERIOD * 179);

        // A presented frame is one period however long it took.
        for (frame, took_ms) in [4_u32, 90, 17].into_iter().enumerate() {
            real += Duration::from_millis(u64::from(took_ms));
            platform.advance_clock_at(Some(Turn::Presented), real, PERIOD);
            assert_eq!(
                platform.duration_since_start(),
                start + PERIOD * (180 + frame as u32)
            );
        }
        // A PAL presenter steps 20 ms.
        platform.advance_clock_at(Some(Turn::Presented), real, Duration::from_millis(20));
        assert_eq!(
            platform.duration_since_start(),
            start + PERIOD * 182 + Duration::from_millis(20)
        );
        assert_eq!(timeline(), Some(platform.duration_since_start()));
    }

    #[test]
    fn the_clock_test_switch_keeps_wall_time() {
        let mut platform = platform(AnimationClock::Wall);
        let started = Instant::now().checked_sub(Duration::from_secs(2));
        assert!(started.is_some(), "clock supports fixture offset");
        let Some(started) = started else { return };
        platform.started = started;
        platform.start_clock_at(Duration::ZERO);
        platform.advance_clock_at(
            Some(Turn::Presented),
            Duration::ZERO,
            Duration::from_micros(16_667),
        );
        assert!(platform.clock.get().is_none());
        assert!(platform.duration_since_start() >= Duration::from_secs(2));
    }

    #[test]
    fn callback_batches_preserve_fifo_and_yield_at_count_limit() {
        let queue = EventQueue::default();
        let seen = Arc::new(Mutex::new(Vec::new()));
        for value in 0..=CALLBACK_LIMIT {
            let seen = seen.clone();
            queue.push(Box::new(move || {
                seen.lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .push(value);
            }));
        }
        queue.dispatch_while(|| true);
        assert_eq!(
            *seen.lock().unwrap_or_else(PoisonError::into_inner),
            (0..CALLBACK_LIMIT).collect::<Vec<_>>()
        );
        queue.dispatch_while(|| true);
        assert_eq!(
            *seen.lock().unwrap_or_else(PoisonError::into_inner),
            (0..=CALLBACK_LIMIT).collect::<Vec<_>>()
        );
    }

    #[test]
    fn callback_time_budget_yields_after_one_slow_callback() {
        let queue = Arc::new(EventQueue::default());
        let seen = Arc::new(Mutex::new(Vec::new()));
        let nested_queue = queue.clone();
        let nested_seen = seen.clone();
        queue.push(Box::new(move || {
            nested_seen
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(1);
            nested_queue.push(Box::new(move || {
                nested_seen
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .push(3);
            }));
        }));
        let second_seen = seen.clone();
        queue.push(Box::new(move || {
            second_seen
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .push(2);
        }));
        // Inject an exhausted budget instead of relying on scheduler sleeps.
        queue.dispatch_while(|| false);
        assert_eq!(
            *seen.lock().unwrap_or_else(PoisonError::into_inner),
            vec![1]
        );
        queue.dispatch_while(|| true);
        assert_eq!(
            *seen.lock().unwrap_or_else(PoisonError::into_inner),
            vec![1, 2, 3]
        );
    }

    #[test]
    fn preparation_profile_includes_turns_without_rendering() {
        let mut profile = FrameProfile::default();
        profile.record_turn(Duration::from_millis(80), Duration::from_millis(100));
        assert_eq!(profile.total_rendered, 0);
        assert_eq!(profile.preparation_max, Duration::from_millis(80));
        assert_eq!(profile.turn_max, Duration::from_millis(100));
    }

    #[test]
    fn logical_window_transposes_for_quarter_turns() {
        assert_eq!(
            logical_size((1280, 720), RenderingRotation::NoRotation),
            (1280, 720)
        );
        assert_eq!(
            logical_size((1280, 720), RenderingRotation::Rotate90),
            (720, 1280)
        );
        assert_eq!(
            logical_size((1280, 720), RenderingRotation::Rotate270),
            (720, 1280)
        );
    }

    #[test]
    fn orientation_values_map_to_renderer_turns() {
        assert_eq!(
            rotation_value(crate::Orientation::Horizontal),
            ROTATION_NONE
        );
        assert_eq!(rotation_value(crate::Orientation::Cw), ROTATION_CW);
        assert_eq!(rotation_value(crate::Orientation::Ccw), ROTATION_CCW);
    }
}
