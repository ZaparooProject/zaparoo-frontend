// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// Gaming Mode focus, for a Steam Deck and anything else running a
// gamescope session.
//
// gamescope only draws a window that carries a `STEAM_GAME` property, and
// it picks between candidates using the root `GAMESCOPECTRL_BASELAYER_APPID`
// list. Steam sets both for the apps it launches. A frontend Steam did not
// launch is mapped and rendering but never shown until it claims them, and
// it has to claim them again every time a game exits, because Steam puts
// its own shell back in front on the way out.
//
// These are the same two properties Zaparoo Core sets for the emulator
// windows it launches directly, so this is one mechanism seen from both
// sides of the handover rather than two. Core reverts its own claim when
// the emulator window closes; we reclaim when Core tells us the media
// stopped, which also covers the launches Core routes through Steam and
// therefore never focuses itself.
//
// `MiSTer` owns the framebuffer outright and has no compositor to argue
// with, so none of this compiles there.

/// No compositor to claim on `MiSTer`: the presenter owns the screen and
/// the wrapper decides who has it.
#[cfg(feature = "mister")]
pub fn claim_focus_when_mapped(_app: &crate::App) {}

#[cfg(feature = "mister")]
pub fn claim_focus_settling(_app: &crate::App) {}

#[cfg(feature = "desktop")]
pub use desktop::{claim_focus_settling, claim_focus_when_mapped, in_session};

#[cfg(feature = "desktop")]
mod desktop {
    use std::process::{Command, Stdio};
    use std::sync::OnceLock;
    use std::time::Duration;

    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use slint::ComponentHandle;

    use crate::App;

    /// Marks a gamescope session. Present on the root window of the
    /// session's Xwayland display and nowhere else.
    const SESSION_ATOM: &str = "GAMESCOPE_XWAYLAND_SERVER_ID";
    /// Set on our own window: gamescope hides anything without it.
    const WINDOW_ATOM: &str = "STEAM_GAME";
    /// Root-window focus list gamescope picks the visible window from.
    const BASELAYER_ATOM: &str = "GAMESCOPECTRL_BASELAYER_APPID";

    /// Slint documents the window handle as unavailable until the window
    /// manager has created the window, which takes at least one turn of
    /// the event loop after show. Retry a few times rather than racing it.
    const MAP_RETRY_DELAY: Duration = Duration::from_millis(250);
    const MAP_RETRIES: u8 = 8;

    /// Steam finishes putting its own shell back only after the game
    /// process is gone, which is a moment later than the `media.stopped`
    /// that wakes us, so a single claim on that edge loses the race and
    /// leaves the user looking at the Steam library. Reclaim across the
    /// settle window instead: gaps between attempts, each attempt two
    /// property writes that gamescope re-evaluates.
    const SETTLE_GAPS: [Duration; 4] = [
        Duration::from_millis(250),
        Duration::from_millis(500),
        Duration::from_millis(750),
        Duration::from_millis(1500),
    ];

    /// True in a gamescope session. One `xprop` at startup, cached: on an
    /// ordinary desktop the atom is absent, and where `xprop` is missing
    /// entirely the check fails the same way and everything below no-ops.
    pub fn in_session() -> bool {
        static CACHED: OnceLock<bool> = OnceLock::new();
        *CACHED.get_or_init(|| {
            if std::env::var_os("DISPLAY").is_none() {
                return false;
            }
            let found = Command::new("xprop")
                .args(["-root", SESSION_ATOM])
                .stderr(Stdio::null())
                .output()
                .is_ok_and(|out| {
                    out.status.success()
                        && String::from_utf8_lossy(&out.stdout).contains("CARDINAL")
                });
            tracing::debug!(found, "gamescope session detection");
            found
        })
    }

    /// Our window's X11 id. `None` before the window manager has created
    /// it, and on a Wayland surface: Gaming Mode runs us under Xwayland,
    /// and a native Wayland client has no property to set.
    fn window_id(app: &App) -> Option<u64> {
        let handle = app.window().window_handle();
        let handle = handle.window_handle().ok()?;
        match handle.as_raw() {
            RawWindowHandle::Xlib(window) => Some(window.window),
            RawWindowHandle::Xcb(window) => Some(u64::from(window.window.get())),
            _ => None,
        }
    }

    fn xprop(args: &[&str]) -> bool {
        Command::new("xprop")
            .args(args)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    }

    /// Put this window in front of the Steam shell. Idempotent, and a
    /// no-op outside a gamescope session.
    fn claim_focus(app: &App) {
        if !in_session() {
            return;
        }
        let Some(id) = window_id(app) else {
            tracing::debug!("gamescope: no X11 window yet, focus not claimed");
            return;
        };
        let id = format!("{id:#x}");
        // Speak Steam's id for our window when Steam launched us. Writing
        // the placeholder over it desynchronizes Steam's own focus
        // bookkeeping and gamescope ends up drawing nothing.
        let app_id = crate::steam::focus_app_id();
        let claimed = xprop(&[
            "-id",
            &id,
            "-f",
            WINDOW_ATOM,
            "32c",
            "-set",
            WINDOW_ATOM,
            app_id,
        ]) && xprop(&[
            "-root",
            "-format",
            BASELAYER_ATOM,
            "32co",
            "-set",
            BASELAYER_ATOM,
            app_id,
        ]);
        tracing::debug!(window = id, app_id, claimed, "gamescope focus claim");
    }

    /// Claim now and keep claiming across the settle window, for the
    /// return from a launched game. Stops early if another game starts:
    /// stealing the screen back from it would be worse than losing it.
    pub fn claim_focus_settling(app: &App) {
        if !in_session() {
            return;
        }
        claim_focus(app);
        schedule_settle(app.as_weak(), 0);
    }

    fn schedule_settle(weak: slint::Weak<App>, index: usize) {
        let Some(gap) = SETTLE_GAPS.get(index).copied() else {
            return;
        };
        slint::Timer::single_shot(gap, move || {
            let Some(app) = weak.upgrade() else {
                return;
            };
            if app.global::<crate::Shell>().get_dormant() {
                return;
            }
            claim_focus(&app);
            schedule_settle(app.as_weak(), index + 1);
        });
    }

    /// Claim as soon as the window manager has given us a window. Steam
    /// does not do this for us even when it launched us: it sets the
    /// baselayer to its own shell and leaves `STEAM_GAME` off our window
    /// entirely, so an unclaimed frontend is mapped, rendering, and
    /// invisible, with input going to Steam. The claim settles rather
    /// than firing once, to take the last word over Steam's own launch
    /// transition.
    pub fn claim_focus_when_mapped(app: &App) {
        if !in_session() {
            return;
        }
        schedule_claim(app.as_weak(), MAP_RETRIES);
    }

    fn schedule_claim(weak: slint::Weak<App>, remaining: u8) {
        slint::Timer::single_shot(MAP_RETRY_DELAY, move || {
            let Some(app) = weak.upgrade() else {
                return;
            };
            if window_id(&app).is_some() {
                claim_focus_settling(&app);
                return;
            }
            match remaining.checked_sub(1) {
                Some(0) | None => {
                    tracing::debug!("gamescope: window never appeared, focus not claimed");
                }
                Some(next) => schedule_claim(app.as_weak(), next),
            }
        });
    }
}
