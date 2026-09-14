// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// `zaparoo-app` is the toolkit-agnostic application layer: the rules the
// frontend enforces, with no UI toolkit anywhere in sight. `rust/frontend`
// is the Slint adapter over it.
//
// The contract, enforced by `scripts/check-toolkit-free.sh` in the lint gate:
// this crate must never gain a `slint` dependency, nor any other UI
// toolkit, `cxx`-style bindings included. Decisions are pure functions;
// state machines take an injected clock; timers are declared, not owned.
//
// See `docs/architecture.md` for where this crate sits in the frontend.

pub mod action_error;
pub mod alternate_versions;
pub mod buttons;
pub mod clock;
pub mod covers;
pub mod customization;
pub mod format;
pub mod hub;
pub mod input;
pub mod launchers;
pub mod layouts;
pub mod letter_jump;
pub mod log_upload;
pub mod media_list;
pub mod media_setup;
pub mod paged_grid;
pub mod palette;
pub mod settings;
pub mod sizing;
pub mod status_line;
pub mod systems;
