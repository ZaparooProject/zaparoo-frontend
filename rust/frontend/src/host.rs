// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

//! Platform-neutral entry and input seam. The embedding host owns Slint backend
//! initialization, process logging, and framework lifecycle; this library owns UI.

use std::sync::Arc;

use slint::ComponentHandle;
pub use zaparoo_app::folder_picker::State as FolderPickerState;
pub use zaparoo_app::launcher_scan::State as LauncherScanState;
pub use zaparoo_core::platform_paths::HostPaths;
pub use zaparoo_core::transport::Transport;

pub use crate::log_upload::{LogUploader, UploadRequest};

use crate::router::Ctx;
use crate::App;

#[derive(Debug)]
pub struct Options {
    pub paths: HostPaths,
    /// None waits for host readiness; never falls back to desktop localhost.
    pub core_transport: Option<Transport>,
    /// Posts the support bundle; `None` hides Settings > Upload log file.
    pub log_upload: Option<LogUploader>,
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

/// Host-owned Core startup phases, shown on the boot curtain until the
/// first catalog arrives. Platform error text never crosses this seam.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CorePhase {
    Connecting,
    Starting,
    /// Core is migrating its data before it can serve.
    Migrating,
    /// Core is serving; the library is loading.
    Ready,
    Reconnecting,
}

impl CorePhase {
    fn boot_status(self) -> crate::BootStatus {
        match self {
            Self::Connecting => crate::BootStatus::Connecting,
            Self::Starting => crate::BootStatus::Starting,
            Self::Migrating => crate::BootStatus::Migrating,
            Self::Ready => crate::BootStatus::Loading,
            Self::Reconnecting => crate::BootStatus::Reconnecting,
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

    /// Offer a host-owned picker; no platform handles or media paths enter Frontend.
    pub fn configure_folder_picker(
        &self,
        request: Arc<dyn Fn() -> bool + Send + Sync>,
    ) -> Result<(), slint::EventLoopError> {
        let ctx = self.ctx.clone();
        self.app.upgrade_in_event_loop(move |app| {
            ctx.folders.configure(request);
            crate::settings::refresh(&ctx, &app);
        })
    }

    /// Offer an optional host-owned scan for installed launchers.
    pub fn configure_launcher_scan(
        &self,
        request: Arc<dyn Fn() -> bool + Send + Sync>,
    ) -> Result<(), slint::EventLoopError> {
        let ctx = self.ctx.clone();
        self.app.upgrade_in_event_loop(move |app| {
            ctx.launcher_scan.configure(request);
            crate::settings::refresh(&ctx, &app);
        })
    }

    pub fn launcher_scan_status(
        &self,
        revision: u64,
        state: LauncherScanState,
        detected: u16,
    ) -> Result<(), slint::EventLoopError> {
        let ctx = self.ctx.clone();
        self.app.upgrade_in_event_loop(move |app| {
            if ctx.launcher_scan.update(revision, state, detected) {
                crate::settings::refresh(&ctx, &app);
                if !matches!(state, LauncherScanState::Opening) {
                    crate::launchers::refresh(&ctx, &app);
                }
            }
        })
    }

    pub fn request_launcher_scan(&self) -> Result<(), slint::EventLoopError> {
        let ctx = self.ctx.clone();
        self.app
            .upgrade_in_event_loop(move |app| crate::launcher_scan::request(&ctx, &app))
    }

    /// Apply an ordered permission snapshot for this window only.
    pub fn folder_picker_status(
        &self,
        revision: u64,
        state: FolderPickerState,
        saved: u32,
    ) -> Result<(), slint::EventLoopError> {
        let ctx = self.ctx.clone();
        self.app.upgrade_in_event_loop(move |app| {
            if ctx.folders.update(revision, state, saved) {
                crate::settings::refresh(&ctx, &app);
            }
        })
    }

    pub fn folder_picker_pending(&self) -> bool {
        self.ctx.folders.status().2
    }

    pub fn folder_permission_count(&self) -> u32 {
        self.ctx.folders.status().1
    }

    pub fn folder_permission_revoked(&self) -> bool {
        self.ctx.folders.revoked()
    }

    /// Invoke the same guarded action as the Library settings row.
    pub fn request_folder_picker(&self) -> Result<(), slint::EventLoopError> {
        let ctx = self.ctx.clone();
        self.app
            .upgrade_in_event_loop(move |app| crate::folder_picker::request(&ctx, &app))
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

    /// Show a Core startup phase on the boot curtain; ignored once boot completes.
    pub fn core_phase(&self, phase: CorePhase) -> Result<(), slint::EventLoopError> {
        let status = phase.boot_status();
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

/// The host's network, Bluetooth and battery state for the header. A
/// battery of `None` hides the gauge. Status belongs to the device, not to
/// a window, so it may arrive before any window exists and is kept for the
/// first one. Safe from any thread.
pub fn system_status(
    wifi_internet: bool,
    lan_internet: bool,
    bluetooth: bool,
    battery_percent: Option<i32>,
) {
    crate::system_status::set_host_status(crate::system_status::LocalStatus {
        has_wifi_internet: wifi_internet,
        has_lan_internet: lan_internet,
        has_bluetooth: bluetooth,
        has_battery: battery_percent.is_some(),
        battery_percent: battery_percent.unwrap_or(0).clamp(0, 100),
    });
}
