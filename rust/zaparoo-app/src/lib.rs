// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// `zaparoo-app` is the toolkit-agnostic application layer: the rules the
// frontend enforces, with no UI toolkit anywhere in sight. `rust/frontend`
// (cxx-qt) is one adapter over it; a future Slint shell would be a second.
//
// The contract, enforced by `scripts/check-toolkit-free.sh` in the lint gate:
// no `cxx`, no `cxx_qt`, no `slint`, no `QString`/`QVariant`/`QList`/`QColor`,
// no `qt_thread`, no `Pin<&mut ...>`. Decisions are pure functions; state
// machines take an injected clock; timers are declared, not owned.
//
// See `docs/qt-to-rust-extraction.md` for the sequence this crate is being
// filled in by.

pub mod sizing;
