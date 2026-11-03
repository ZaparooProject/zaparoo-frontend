// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// The page turn every paged grid shares (the Hub, Systems, the games grids
// and the file browser): one state, one set of rules and one set of timers,
// so the screens cannot drift apart. A screen plugs in through `PagedScreen`
// and keeps its own render, which publishes the destination page into the
// view's `next-cells` while `sliding` is set; the view is a `PageStrip`.
//
// The rules, in order:
// - Reduce motion, the motion switch, a held page flip, a held direction
//   running fast, or a turn that interrupted the one before it cuts.
// - On MiSTer the presenter slides cached endpoints of the grid band.
// - Without them, a screen that sets `LIVE_STRIP_ON_HDMI` false (the Hub)
//   never animates its full grid at native resolution on MiSTer's software
//   renderer and cuts there; the CRT scene keeps the live strip.

use std::time::Duration;

use slint::ComponentHandle;

use crate::router::{lock, Ctx, Shared};
use crate::App;

/// How long a turn takes, on the strip's `Motion.page-ms` easing plus a
/// frame of margin.
pub(crate) const SETTLE_MS: u64 = 220;
/// After a turn settles, the strip's animation is re-armed a beat later so
/// the reset to rest never animates.
pub(crate) const REARM_MS: u64 = 50;

/// One screen's page-turn state.
#[derive(Debug, Clone, Copy, Default)]
pub struct PageMotion {
    pub sliding: bool,
    seq: u64,
    /// The press in flight interrupted a turn: its own turn cuts.
    cut_next: bool,
}

impl PageMotion {
    fn begin(&mut self) -> u64 {
        self.sliding = true;
        self.seq = self.seq.wrapping_add(1);
        self.seq
    }

    /// Expire any turn in flight; its completion ticket no longer matches.
    pub fn expire(&mut self) {
        self.sliding = false;
        self.seq = self.seq.wrapping_add(1);
    }

    pub fn is_current(&self, seq: u64) -> bool {
        self.seq == seq
    }
}

/// The page-strip properties every paged view global carries.
pub(crate) trait StripView {
    fn strip_dir(&self, dir: i32);
    fn strip_target(&self, index: i32);
    fn strip_cached(&self, cached: bool);
    #[cfg_attr(
        not(feature = "mister"),
        allow(dead_code, reason = "only the MiSTer presenters cache a turn")
    )]
    fn strip_is_cached(&self) -> bool;
    fn strip_anim(&self, anim: bool);
    fn strip_slide(&self, slide: f32);
    #[cfg_attr(
        not(feature = "mister"),
        allow(dead_code, reason = "only the MiSTer presenters cache a turn")
    )]
    fn strip_band(&self) -> (f32, f32);
}

macro_rules! strip_view {
    ($($view:ident),+ $(,)?) => {
        $(
            impl StripView for crate::$view<'_> {
                fn strip_dir(&self, dir: i32) {
                    self.set_slide_dir(dir);
                }
                fn strip_target(&self, index: i32) {
                    self.set_transition_target_index(index);
                }
                fn strip_cached(&self, cached: bool) {
                    self.set_cached_transition(cached);
                }
                fn strip_is_cached(&self) -> bool {
                    self.get_cached_transition()
                }
                fn strip_anim(&self, anim: bool) {
                    self.set_slide_anim(anim);
                }
                fn strip_slide(&self, slide: f32) {
                    self.set_page_slide(slide);
                }
                fn strip_band(&self) -> (f32, f32) {
                    (self.get_grid_y(), self.get_grid_height())
                }
            }
        )+
    };
}

strip_view!(HubView, SystemsView, GamesView, FileBrowserView);

/// A screen with a paged grid.
pub(crate) trait PagedScreen {
    type View<'a>: StripView;
    fn view(app: &App) -> Self::View<'_>;
    fn motion(shared: &mut Shared) -> &mut PageMotion;
    fn render(ctx: &Ctx, app: &App);
    /// The screen is showing, so a settled turn repaints it.
    fn on_screen(app: &App) -> bool;
    /// The screen shows its list rather than its grid: nothing to slide.
    fn list_layout(app: &App) -> bool;
    /// Whether the live strip may slide on `MiSTer`'s HDMI scene when no
    /// presenter caches the turn.
    const LIVE_STRIP_ON_HDMI: bool = true;
    /// What the turn belongs to beyond the page, such as the fill a list's
    /// rows came from: a turn that settles under another one is dropped.
    fn epoch(_shared: &Shared) -> u64 {
        0
    }
}

/// A new command commits the logical destination before acting on it, never
/// queuing another page behind an old animation. Returns whether a turn was
/// in flight; the press that interrupted it cuts its own turn.
pub(crate) fn interrupt<S: PagedScreen>(ctx: &Ctx, app: &App) -> bool {
    let interrupted = {
        let mut shared = lock(&ctx.shared);
        let motion = S::motion(&mut shared);
        let interrupted = motion.sliding;
        motion.cut_next = interrupted;
        if interrupted {
            motion.expire();
        }
        interrupted
    };
    if !interrupted {
        return false;
    }
    let view = S::view(app);
    #[cfg(feature = "mister")]
    if view.strip_is_cached() {
        crate::mister::cancel_page_transition();
    }
    view.strip_cached(false);
    view.strip_anim(false);
    view.strip_slide(0.0);
    S::render(ctx, app);
    app.window().request_redraw();
    true
}

#[cfg(feature = "mister")]
fn request_cached(view: &impl StripView, app: &App, direction: i32) -> bool {
    let (grid_y, grid_height) = view.strip_band();
    crate::mister::request_browse_page_transition(app, grid_y, grid_height, direction)
}

#[cfg(not(feature = "mister"))]
fn request_cached(_view: &impl StripView, _app: &App, _direction: i32) -> bool {
    false
}

/// Show the grid's current page, turning to it in its committed direction
/// (wrapped and deferred turns included). `target_local` is the focused
/// cell on the destination page; `animate` false cuts.
pub(crate) fn turn<S: PagedScreen>(
    ctx: &Ctx,
    app: &App,
    direction: i32,
    target_local: usize,
    animate: bool,
) {
    let cut = {
        let mut shared = lock(&ctx.shared);
        let reduce = shared.persist.settings.reduce_motion;
        let interrupted = std::mem::take(&mut S::motion(&mut shared).cut_next);
        !animate || reduce || interrupted
    } || !app.global::<crate::Motion>().get_enabled()
        || crate::input::rapid_page(ctx)
        || crate::input::rapid_navigation(ctx);
    let view = S::view(app);
    view.strip_dir(direction);
    view.strip_target(i32::try_from(target_local).unwrap_or(0));
    if cut || S::list_layout(app) {
        S::render(ctx, app);
        return;
    }
    let cached = request_cached(&view, app, direction);
    if !cached
        && !S::LIVE_STRIP_ON_HDMI
        && ctx.is_mister
        && !app.global::<crate::Sizing>().get_crt()
    {
        S::render(ctx, app);
        return;
    }
    let (seq, epoch) = {
        let mut shared = lock(&ctx.shared);
        (S::motion(&mut shared).begin(), S::epoch(&shared))
    };
    view.strip_cached(cached);
    view.strip_anim(!cached);
    // Publishes the destination into `next-cells` while sliding.
    S::render(ctx, app);
    // Resolving a changed viewport can expire this ticket during render.
    if !S::motion(&mut lock(&ctx.shared)).is_current(seq) {
        return;
    }
    if !cached {
        crate::drs::heavy_begin();
        view.strip_slide(direction as f32);
    }
    let weak = app.as_weak();
    let ctx = ctx.clone();
    slint::Timer::single_shot(Duration::from_millis(SETTLE_MS), move || {
        if !cached {
            crate::drs::heavy_end();
        }
        let Some(app) = weak.upgrade() else {
            return;
        };
        {
            let mut shared = lock(&ctx.shared);
            if S::epoch(&shared) != epoch {
                return;
            }
            let motion = S::motion(&mut shared);
            if !motion.is_current(seq) {
                return;
            }
            motion.sliding = false;
        }
        let view = S::view(&app);
        view.strip_cached(false);
        view.strip_anim(false);
        view.strip_slide(0.0);
        if S::on_screen(&app) {
            S::render(&ctx, &app);
        }
        let weak = app.as_weak();
        slint::Timer::single_shot(Duration::from_millis(REARM_MS), move || {
            {
                let mut shared = lock(&ctx.shared);
                if S::epoch(&shared) != epoch || !S::motion(&mut shared).is_current(seq) {
                    return;
                }
            }
            if let Some(app) = weak.upgrade() {
                S::view(&app).strip_anim(true);
            }
        });
    });
}
