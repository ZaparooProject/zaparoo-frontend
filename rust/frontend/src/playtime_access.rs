// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

//! Verified playtime: a host that can measure foreground game time
//! exactly offers a handoff to the system setting that allows it. The user
//! grants it there; neither Core nor the frontend can grant it.

use std::sync::{Arc, Mutex};

use crate::router::Ctx;
use crate::App;

type Request = Arc<dyn Fn() -> bool + Send + Sync>;

#[derive(Default)]
struct Data {
    request: Option<Request>,
    granted: bool,
}

#[derive(Clone, Default)]
pub struct Model(Arc<Mutex<Data>>);

impl std::fmt::Debug for Model {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PlaytimeAccess").finish_non_exhaustive()
    }
}

impl Model {
    pub fn available(&self) -> bool {
        self.0.lock().is_ok_and(|data| data.request.is_some())
    }

    #[cfg(feature = "hosted")]
    pub fn configure(&self, request: Request) {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .request = Some(request);
    }

    #[cfg(feature = "hosted")]
    pub fn set_granted(&self, granted: bool) {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .granted = granted;
    }

    pub fn granted(&self) -> bool {
        self.0.lock().is_ok_and(|data| data.granted)
    }
}

pub fn request(ctx: &Ctx, app: &App) {
    let callback = ctx
        .playtime_access
        .0
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .request
        .clone();
    if let Some(callback) = callback {
        crate::input::stop_repeat(ctx);
        if !callback() {
            crate::router::report_action_error(ctx, app, "playtime_access", "");
        }
    }
}
