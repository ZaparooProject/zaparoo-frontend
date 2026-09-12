// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// Custom `slint::platform::Platform` for the MiSTer target.
//
// Frame loop shape: dispatch a bounded batch of callbacks, poll input,
// advance timers/animations, render if dirty, then present. Vsync paces
// presentation; monotonic elapsed time drives deadlines so contention
// skips obsolete animation frames rather than extending every wait.

pub use super::latch::ResolutionPolicy;
use super::{
    ddr::DdrPresenter, fb0::Fb0Presenter, input::InputReader, latch::LatchPresenter, Presenter,
};
use slint::platform::software_renderer::{
    MinimalSoftwareWindow, RenderingRotation, RepaintBufferType,
};
use slint::platform::{Platform, WindowAdapter};
use std::cell::RefCell;
use std::collections::VecDeque;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::{Duration, Instant};

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

fn rotation_value(value: &str) -> u8 {
    match value {
        "cw" => ROTATION_CW,
        "ccw" => ROTATION_CCW,
        _ => ROTATION_NONE,
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

pub fn set_orientation(value: &str) {
    REQUESTED_ROTATION.store(rotation_value(value), Ordering::SeqCst);
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
    /// Real elapsed time drives both timers and interpolated motion.
    started: Instant,
    /// CRT native path: present through the DDR contract instead of fb0.
    crt: bool,
    crt_size: (u32, u32),
    crt_offsets: (i32, i32),
    /// Render normal HDMI and CRT-profile component instances together.
    dual_head: bool,
    /// Main granted the uio lease (spawned us with --latch): try the
    /// vblank-latch presenter on the HDMI path.
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
    ) -> Self {
        Self {
            windows: RefCell::new(Vec::new()),
            queue: Arc::new(EventQueue::default()),
            started: Instant::now(),
            crt,
            crt_size,
            crt_offsets,
            dual_head,
            latch,
            resolution_policy,
        }
    }

    /// --latch wants the vblank-latch presenter (kernel module +
    /// latch RBF + Main's uio lease). A failed probe falls back to
    /// fb0 loudly because it changes tearing and DRS behavior.
    fn make_hdmi_presenter(&self) -> Result<Box<dyn Presenter>, slint::PlatformError> {
        if self.latch {
            match LatchPresenter::open(self.resolution_policy, true) {
                Ok(p) => return Ok(Box::new(p)),
                Err(e) => {
                    tracing::warn!("latch presenter unavailable, falling back to fb0: {e}");
                }
            }
        }
        Ok(Box::new(Fb0Presenter::open()?))
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
                true,
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
            if hdmi_busy.is_some() {
                // HDMI resolution is the available pressure valve,
                // but dual-head budget includes both render targets.
                drs.rendered(total_busy, hdmi.as_mut(), hdmi_window, rotation);
            } else if !hdmi_rendered {
                hdmi.wait_vsync();
                drs.idle(hdmi.as_mut(), hdmi_window, rotation);
            }
            if hdmi_busy.is_some() || crt_busy.is_some() {
                profile.record(total_busy);
            }
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
        self.started.elapsed()
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

        // Silence the kernel console for the whole frame-loop lifetime;
        // dropped (KD_TEXT restored) on clean exit. The wrapper resets
        // the tty itself if we die without unwinding.
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
                profile.record(busy);
                profile.record_turn(preparation, turn_started.elapsed());
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
                profile.record(busy);
                // Safety net: sustained native-resolution overruns
                // retreat to motion resolution until a quiet stretch.
                drs.rendered(busy, presenter.as_mut(), window.as_ref(), rotation);
            }
            // Idle turns still wait for vsync; timer deadlines use real
            // elapsed time regardless of whether this turn painted.
            if !rendered {
                presenter.wait_vsync();
                // Phase over and settled: pop to native. The resize
                // dirties one final sharp frame while nothing moves.
                drs.idle(presenter.as_mut(), window.as_ref(), rotation);
            }
            profile.record_turn(preparation, turn_started.elapsed());
        }
    }
}

/// Rolling frame-work profiler: records the busy time of every
/// rendered frame (render + copy + publish, vsync waits excluded) and
/// logs avg/p99/max against the 16.7 ms budget every 600 rendered
/// frames, plus a running overrun count. This is the number the
/// migration evaluation runs on.
#[derive(Default)]
struct FrameProfile {
    samples: Vec<Duration>,
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

    fn record(&mut self, busy: Duration) {
        self.total_rendered += 1;
        if busy > FRAME_BUDGET {
            self.overruns += 1;
        }
        self.samples.push(busy);
        if self.samples.len() < PROFILE_WINDOW {
            return;
        }
        self.samples.sort_unstable();
        let avg: Duration = self.samples.iter().sum::<Duration>() / self.samples.len() as u32;
        let p99 = self.samples[self.samples.len() * 99 / 100];
        let max = self.samples[self.samples.len() - 1];
        tracing::info!(
            rendered = self.total_rendered,
            avg_us = avg.as_micros() as u64,
            p99_us = p99.as_micros() as u64,
            max_us = max.as_micros() as u64,
            budget_us = FRAME_BUDGET.as_micros() as u64,
            overruns = self.overruns,
            "frame profile"
        );
        self.samples.clear();
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
    set_orientation(orientation);
    slint::platform::set_platform(Box::new(MisterPlatform::new(
        crt,
        crt_size,
        crt_offsets,
        latch,
        dual_head,
        resolution_policy,
    )))
    .map_err(|e| slint::PlatformError::Other(format!("set_platform: {e:?}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clock_advances_without_presented_frames() {
        let mut platform = MisterPlatform::new(
            false,
            (352, 240),
            (0, 0),
            false,
            false,
            ResolutionPolicy::Adaptive,
        );
        let started = Instant::now().checked_sub(Duration::from_secs(2));
        assert!(started.is_some(), "clock supports fixture offset");
        let Some(started) = started else { return };
        platform.started = started;
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
        assert_eq!(rotation_value("horizontal"), ROTATION_NONE);
        assert_eq!(rotation_value("invalid"), ROTATION_NONE);
        assert_eq!(rotation_value("cw"), ROTATION_CW);
        assert_eq!(rotation_value("ccw"), ROTATION_CCW);
    }
}
