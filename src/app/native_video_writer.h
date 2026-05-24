// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

#pragma once

// Open /dev/fb0 and the Menu fork DDR region, validate fb0 geometry,
// and prime both DDR slots and the control word. Idempotent; on
// failure leaves the writer disabled and `copyFrameNativeVideoWriter()`
// becomes a no-op.
void initNativeVideoWriter();

// One 320x240 RGB8888 memcpy from /dev/fb0 to the currently inactive
// Menu fork DDR slot, then publish the slot via the control word and
// flip the active slot. Intended to be invoked from a Qt
// render-finish hook (e.g. `QQuickWindow::frameSwapped`) so the copy
// happens once per actually-rendered frame and not on a free-running
// timer. No-op if `initNativeVideoWriter()` did not initialise
// cleanly.
void copyFrameNativeVideoWriter();

// Zero the control word, unmap both regions, and close the
// descriptors. Safe to call from `std::atexit` or
// `QGuiApplication::aboutToQuit`.
void stopNativeVideoWriter();
