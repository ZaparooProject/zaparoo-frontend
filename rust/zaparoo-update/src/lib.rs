// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

//! The Update module: a public stand-in, or the private implementation.
//!
//! Without the private checkout (`rust/private/zaparoo-update`, see
//! `build.rs`) this crate has the module's whole interface and no behavior:
//! the updater is never available and a session never publishes or runs
//! anything. With it, the private sources compile in as `imp` and provide
//! the same two items. `zaparoo-update-api` is the contract between either
//! one and the frontend; `docs/architecture.md` ("Update module") has the
//! rest.

pub use zaparoo_update_api as api;

#[cfg(not(zaparoo_update_private))]
mod imp {
    use zaparoo_update_api::{Input, Sink};

    /// True when the real module is built in and the device has an updater
    /// tool to run.
    pub fn is_updater_available() -> bool {
        false
    }

    /// One Update screen session. Create it once; it survives visits to the
    /// screen.
    #[derive(Debug)]
    pub struct UpdateSession;

    impl UpdateSession {
        /// `handle` runs the module's background work. `sink` receives every
        /// event the module publishes.
        pub fn new(_handle: tokio::runtime::Handle, _sink: Sink) -> Self {
            Self
        }

        /// Feed one press to the screen's state machine.
        pub fn input(&self, _input: Input) {}
    }
}

#[cfg(zaparoo_update_private)]
include!(concat!(env!("OUT_DIR"), "/private.rs"));

pub use imp::{is_updater_available, UpdateSession};
