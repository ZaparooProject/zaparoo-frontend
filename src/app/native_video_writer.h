// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

#pragma once

#include <cstdint>

// Offset ranges the Menu fork core honors before clamping in RTL.
// Calibration UI and persisted config stay within these so a saved
// value never depends on the hardware clamp.
constexpr int kNativeVideoHOffsetMin = -8;
constexpr int kNativeVideoHOffsetMax = 8;
constexpr int kNativeVideoVOffsetMin = -8;
constexpr int kNativeVideoVOffsetMax = 2;

// Pack the v2 control block's word1: [31:16] magic 0x5A50 ("ZP"),
// [15:8] h_offset as signed int8 (+ = right), [7:4] v_offset as a
// signed 4-bit field (+ = down), [3:0] mode (0 = 352x240p60,
// 1 = 720x480i60, 2 = 352x288p50). The (h, v) parameter order mirrors
// the hardware field order.
// NOLINTNEXTLINE(bugprone-easily-swappable-parameters)
constexpr uint32_t packNativeVideoWord1(int hOffset, int vOffset, uint32_t mode)
{
    const auto h = static_cast<uint32_t>(static_cast<uint8_t>(static_cast<int8_t>(hOffset)));
    const auto v = static_cast<uint32_t>(static_cast<int8_t>(vOffset)) & 0xFU;
    return (0x5A50U << 16) | (h << 8) | (v << 4) | (mode & 0xFU);
}

// Compile-time contract checks against the values the Menu fork's
// native_video_reader.sv parses (word1 bit layout is the normative
// source of truth there).
static_assert(packNativeVideoWord1(0, 0, 0) == 0x5A500000U);
static_assert(packNativeVideoWord1(1, 1, 1) == 0x5A500111U);
static_assert(packNativeVideoWord1(8, 2, 2) == 0x5A500822U);
static_assert(packNativeVideoWord1(-1, -1, 0) == 0x5A50FFF0U);
static_assert(packNativeVideoWord1(-8, -8, 1) == 0x5A50F881U);

// Open /dev/fb0 and the Menu fork DDR region, validate fb0 geometry,
// and prime both DDR slots and the control block. The fb0 resolution
// selects the video mode (352x240 -> 0, 720x480 -> 1, 352x288 -> 2);
// any other geometry disables the writer, which is also the
// new-launcher/old-core compatibility story. Idempotent; on failure
// leaves the writer disabled and `copyFrameNativeVideoWriter()`
// becomes a no-op.
void initNativeVideoWriter();

// One full-frame memcpy from /dev/fb0 to the currently inactive Menu
// fork DDR slot, then publish the slot via word0 and flip the active
// slot. Intended to be invoked from a Qt render-finish hook (e.g.
// `QQuickWindow::frameSwapped`) so the copy happens once per
// actually-rendered frame and not on a free-running timer. No-op if
// `initNativeVideoWriter()` did not initialise cleanly.
void copyFrameNativeVideoWriter();

// Rewrite word1 with new centering offsets (clamped to the ranges
// above). The core latches word0+word1 in one 64-bit beat per vblank,
// so the change takes effect with the next published frame. Must be
// called from the Qt main thread (same thread as the per-frame copy).
// No-op if the writer is not initialised.
void setNativeVideoOffsets(int hOffset, int vOffset);

// Zero word0 then word1 (the core reverts to its noise pattern within
// one frame), unmap both regions, and close the descriptors. Safe to
// call from `std::atexit` or `QGuiApplication::aboutToQuit`.
void stopNativeVideoWriter();

// C entry point for the Rust side (Browse.CrtVideo live calibration).
extern "C" void zaparoo_native_video_set_offsets(int32_t hOffset, int32_t vOffset);
