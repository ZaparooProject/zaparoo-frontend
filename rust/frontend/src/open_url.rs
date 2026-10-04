// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

//! An optional host handoff to open a URL in the device's own browser, for a
//! host with one and no separate device to scan a code with (Android):
//! today, only the Zaparoo Online link panel's fallback to scanning or
//! typing a code. Neither Core nor the frontend opens anything itself.

use std::sync::{Arc, Mutex};

type Request = Arc<dyn Fn(&str) -> bool + Send + Sync>;

#[derive(Default)]
struct Data {
    request: Option<Request>,
}

#[derive(Clone, Default)]
pub struct Model(Arc<Mutex<Data>>);

impl std::fmt::Debug for Model {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpenUrl").finish_non_exhaustive()
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

    /// Ask the host to open url. False when there is no host handoff, or
    /// the host reports it could not open it.
    pub fn open(&self, url: &str) -> bool {
        let callback = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .request
            .clone();
        callback.is_some_and(|callback| callback(url))
    }
}
