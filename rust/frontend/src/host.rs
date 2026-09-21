// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

//! Platform-neutral entry and input seam. The embedding host owns Slint backend
//! initialization, process logging, and framework lifecycle; this library owns UI.

use std::sync::Arc;

use slint::ComponentHandle;
pub use zaparoo_core::platform_paths::HostPaths;
pub use zaparoo_core::transport::Transport;

use crate::router::Ctx;
use crate::App;

#[derive(Debug)]
pub struct Options {
    pub paths: HostPaths,
    /// None waits for host readiness; never falls back to desktop localhost.
    pub core_transport: Option<Transport>,
}

/// Start the real application on the host's initialized Slint thread.
/// Dropping the event loop releases this instance; paths remain process-stable.
pub fn run(options: &Options, ready: impl FnOnce(Input)) -> Result<(), slint::PlatformError> {
    zaparoo_core::platform_paths::install_host_paths(&options.paths)
        .map_err(|error| slint::PlatformError::Other(error.into()))?;
    crate::run_application(options, ready)
}

/// Semantic action vocabulary; framework key codes remain the host's concern.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Up,
    Down,
    Left,
    Right,
    Accept,
    Cancel,
    ContextMenu,
    PagePrev,
    PageNext,
    PageMenu,
}

impl Action {
    fn name(self) -> &'static str {
        use zaparoo_core::input_actions::actions;
        match self {
            Self::Up => actions::UP,
            Self::Down => actions::DOWN,
            Self::Left => actions::LEFT,
            Self::Right => actions::RIGHT,
            Self::Accept => actions::ACCEPT,
            Self::Cancel => actions::CANCEL,
            Self::ContextMenu => actions::CONTEXT_MENU,
            Self::PagePrev => actions::PAGE_PREV,
            Self::PageNext => actions::PAGE_NEXT,
            Self::PageMenu => actions::PAGE_MENU,
        }
    }
}

/// Weak window identity prevents callbacks for a destroyed instance from
/// dispatching into its replacement. Callers may retain this across threads.
#[derive(Clone)]
pub struct Input {
    ctx: Arc<Ctx>,
    app: slint::Weak<App>,
}

impl std::fmt::Debug for Input {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Input").finish_non_exhaustive()
    }
}

impl Input {
    pub(crate) fn new(ctx: &Arc<Ctx>, app: &App) -> Self {
        Self {
            ctx: ctx.clone(),
            app: app.as_weak(),
        }
    }

    /// Replace the private endpoint only on this live window's event thread.
    pub fn set_core_transport(
        &self,
        transport: Option<Transport>,
    ) -> Result<(), slint::EventLoopError> {
        let ctx = self.ctx.clone();
        self.app.upgrade_in_event_loop(move |_| {
            let client = ctx.store.client();
            client.set_transport(transport);
            crate::media_cache::configure_local_path(cfg!(feature = "mister"), client.is_local());
        })
    }

    /// Initial navigation/paging presses routed by this window (not text or repeats).
    pub fn navigation_events(&self) -> u64 {
        crate::router::lock(&self.ctx.shared)
            .input
            .navigation_events()
    }

    /// Indexed systems represented by the live catalog (excludes launchable-only entries).
    pub fn indexed_system_count(&self) -> usize {
        let shared = crate::router::lock(&self.ctx.shared);
        zaparoo_core::systems_catalog::indexed_count(&shared.systems)
    }

    /// Project host-owned Core startup phases without carrying platform exception text.
    pub fn core_phase(&self, phase: &str) -> Result<(), slint::EventLoopError> {
        let status = match phase {
            "starting" => crate::BootStatus::Starting,
            "migrating" => crate::BootStatus::Migrating,
            "ready" => crate::BootStatus::Loading,
            "reconnecting" => crate::BootStatus::Reconnecting,
            _ => crate::BootStatus::Connecting,
        };
        self.app.upgrade_in_event_loop(move |app| {
            let shell = app.global::<crate::Shell>();
            if !shell.get_boot_complete() {
                shell.set_boot_status(status);
            }
        })
    }

    pub fn core_connected(&self) -> bool {
        matches!(
            *self.ctx.store.client().connection.borrow(),
            zaparoo_core::client::ConnectionState::Connected
        )
    }

    /// Raw press/release only. Duplicate suppression and hold-repeat remain in
    /// the shared Rust input path, not in the host's framework key repeat.
    pub fn action(&self, action: Action, pressed: bool) -> Result<(), slint::EventLoopError> {
        let ctx = self.ctx.clone();
        self.app.upgrade_in_event_loop(move |app| {
            let name = action.name();
            let key = format!("host:{name}");
            if pressed {
                crate::input::gamepad_pressed(&ctx, &app, name, &key);
            } else {
                crate::input::gamepad_released(&ctx, &key);
            }
        })
    }

    /// Focus loss retires held inputs so no repeat survives app switching.
    pub fn clear(&self) -> Result<(), slint::EventLoopError> {
        let ctx = self.ctx.clone();
        self.app
            .upgrade_in_event_loop(move |_| crate::input::stop_repeat(&ctx))
    }
}
