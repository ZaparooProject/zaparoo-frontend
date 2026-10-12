// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

//! Optional embedding-host scan for installed launchers, projected into Library settings.

use crate::router::Ctx;
use crate::{ActionStatus, App};
use std::sync::{Arc, Mutex};
use zaparoo_app::launcher_scan::{Model as Rules, State};

type Request = Arc<dyn Fn() -> bool + Send + Sync>;

#[derive(Default)]
struct Data {
    request: Option<Request>,
    rules: Rules,
}

#[derive(Default, Clone)]
pub struct Model(Arc<Mutex<Data>>);

impl std::fmt::Debug for Model {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LauncherScan").finish_non_exhaustive()
    }
}

fn lock<T>(value: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    value
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

impl Model {
    pub fn available(&self) -> bool {
        lock(&self.0).request.is_some()
    }

    #[cfg(any(feature = "hosted", all(test, feature = "mister")))]
    pub fn configure(&self, request: Request) {
        lock(&self.0).request = Some(request);
    }

    #[cfg(any(feature = "hosted", all(test, feature = "mister")))]
    pub fn update(&self, revision: u64, state: State, detected: u16) -> bool {
        lock(&self.0).rules.update(revision, state, detected)
    }

    pub fn status(&self) -> (ActionStatus, u16, bool) {
        let data = lock(&self.0);
        let status = match data.rules.state {
            State::Ready => ActionStatus::LauncherReady,
            State::Opening => ActionStatus::LauncherOpening,
            State::Detected => ActionStatus::LauncherDetected,
            State::Cancelled => ActionStatus::LauncherCancelled,
            State::Denied => ActionStatus::LauncherDenied,
            State::Failed => ActionStatus::LauncherFailed,
            State::Unavailable => ActionStatus::LauncherUnavailable,
        };
        (
            status,
            data.rules.detected,
            data.rules.state == State::Opening,
        )
    }
}

pub fn request(ctx: &Ctx, app: &App) {
    let callback = {
        let mut data = lock(&ctx.launcher_scan.0);
        let Some(callback) = data.request.clone() else {
            return;
        };
        if !data.rules.begin() {
            return;
        }
        callback
    };
    crate::input::stop_repeat(ctx);
    if !callback() {
        lock(&ctx.launcher_scan.0).rules.state = State::Failed;
    }
    crate::settings::refresh(ctx, app);
}
