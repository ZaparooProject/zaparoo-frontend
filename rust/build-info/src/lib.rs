// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

//! Build provenance constants baked in by this crate's `build.rs`.
//!
//! Lives in its own leaf crate so that the `.git/` rerun triggers that
//! keep the commit stamp fresh do not re-run the expensive cxx-qt
//! codegen in `zaparoo-frontend-rs`'s build script after every commit.
//! A commit rebuilds only this crate plus an incremental recompile of
//! its dependents.

/// Short git commit hash of the source tree, or "unknown".
pub const COMMIT: &str = env!("ZAPAROO_BUILD_COMMIT");

/// UTC build date (`YYYY-MM-DD`), or "unknown".
pub const BUILD_DATE: &str = env!("ZAPAROO_BUILD_DATE");

/// "official" when built with `ZAPAROO_OFFICIAL_BUILD` set, else "dev".
pub const CHANNEL: &str = env!("ZAPAROO_BUILD_CHANNEL");
