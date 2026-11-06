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
pub mod background;
pub mod browse_filter;
pub mod buttons;
pub mod clock;
pub mod core_ui;
pub mod cover_landing;
pub mod covers;
pub mod customization;
pub mod file_browser;
pub mod folder_picker;
pub mod form_list;
pub mod format;
pub mod frame_clock;
pub mod game_details;
pub mod han_fonts;
pub mod hub;
pub mod image_picker;
pub mod info_line;
pub mod input;
pub mod keyboard;
pub mod launcher_preference;
pub mod launcher_scan;
pub mod launchers;
pub mod layouts;
pub mod letter_jump;
pub mod log_upload;
pub mod logo_cache;
pub mod manual;
pub mod media_list;
pub mod media_setup;
pub mod motion_test;
pub mod multi_disc;
pub mod music;
pub mod online_link;
pub mod online_lists;
pub mod online_settings;
pub mod options_menu;
pub mod page_bar;
pub mod paged_grid;
pub mod pairing;
pub mod palette;
pub mod retro_achievements;
pub mod search;
pub mod settings;
pub mod sizing;
pub mod status_line;
pub mod system_picker;
pub mod systems;
pub mod text_entry;
pub mod title_versions;
pub mod top_played;
pub mod user_flags;
pub mod user_lists;
pub mod wait_cue;
pub mod zapscript;
