// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

#pragma once

// Restores framebuffer mmap on MiSTer kernels whose fbdev driver has no
// `fb_mmap`.
//
// Linux 6.8 ("fbdev: Remove default file-I/O implementations") deleted the
// fbdev core's default read/write/mmap handlers; every driver must now supply
// its own. `MiSTer_fb` still declares only fillrect/copyarea/imageblit/
// setcolreg/ioctl, so from the 6.18 MiSTer kernel onwards `mmap()` on
// /dev/fb0 fails with ENODEV and Qt's linuxfb plugin aborts the process with
// "Cannot create window: no screens available". Main_MiSTer is unaffected
// because it reaches the same pixels through /dev/mem, which is exactly the
// route taken here.
//
// The shim is passive. It engages only when the real `mmap` fails with ENODEV
// on a framebuffer character device, so a kernel with a fixed driver keeps the
// native path and none of this code runs. It is installed via
// `-Wl,--wrap=mmap` at the executable's link step, which rewrites the calls
// made from Qt's statically linked linuxfb plugin and from
// `native_video_writer.cpp` without patching Qt. glibc is linked dynamically,
// so libc's own internal mmap calls resolve inside libc.so and are untouched.
//
// Two mapping strategies, selected by ZAPAROO_FB_FALLBACK:
//
//   direct (default) - hand the caller a /dev/mem mapping of the same
//       physical range fbdev would have mapped. Semantics match the working
//       5.15 behavior exactly: Qt composites straight into framebuffer
//       memory and only the rects it actually touched are written. The
//       mapping is uncached, but `QLinuxFbScreen::doRedraw()` blits with
//       CompositionMode_Source (a straight overwrite, not a read-modify-write
//       blend), and this UI is built to keep dirty rects small, so the
//       written area per frame is small.
//
//   staged - hand the caller cached anonymous RAM and copy the whole surface
//       out to framebuffer memory once per rendered frame. Every pixel moves
//       every frame regardless of dirty area, which wins only when most of
//       the screen changes on most frames. Requires the per-frame
//       `flushFbMmapFallback()` hook to be wired, and therefore makes the
//       display depend on `frameSwapped` firing, which `direct` does not.

// True once the shim has redirected at least one framebuffer mapping.
bool fbMmapFallbackActive();

// Publish staged pixels to framebuffer memory. No-op in direct mode, and no-op
// when the shim never engaged. Must run after Qt's `doRedraw()` has composited
// into the mapping, so connect it to `QQuickWindow::frameSwapped` with
// Qt::QueuedConnection for the same reason the native video writer does.
void flushFbMmapFallback();

// Release the shim's mappings and stop `flushFbMmapFallback()` from touching
// them. Safe to call more than once.
void stopFbMmapFallback();

// One line describing what the shim did, for the startup log. The wrapper
// itself cannot log: it runs during Qt platform initialization, before the
// message handler is installed.
void logFbMmapFallbackStatus();
