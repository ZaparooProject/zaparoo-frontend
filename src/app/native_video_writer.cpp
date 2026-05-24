// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

#include "native_video_writer.h"

#if defined(ZAPAROO_EMBEDDED_BUILD) && defined(__linux__)

#include <QLoggingCategory>
#include <atomic>
#include <cstdint>
#include <cstring>
#include <fcntl.h>
#include <linux/fb.h>
#include <sys/ioctl.h>
#include <sys/mman.h>
#include <unistd.h>

namespace
{

constexpr uintptr_t kNativeVideoBase = 0x3A000000u;
constexpr size_t kNativeVideoRegionSize = 0x000A0000u;
constexpr size_t kControlOffset = 0x00000000u;
constexpr size_t kBuffer0Offset = 0x00000100u;
constexpr size_t kBuffer1Offset = 0x0004B100u;
constexpr int kOutputWidth = 320;
constexpr int kOutputHeight = 240;
constexpr size_t kSourceBytesPerPixel = 4;
constexpr size_t kOutputStride = kOutputWidth * kSourceBytesPerPixel;
constexpr size_t kOutputBytes = kOutputStride * kOutputHeight;

int g_fbFd = -1;
int g_memFd = -1;
const uint8_t* g_fb = nullptr;
size_t g_fbSize = 0;
volatile uint8_t* g_nativeBase = nullptr;
volatile uint8_t* g_slot[2] = {nullptr, nullptr};
volatile uint32_t* g_ctrl = nullptr;
uint32_t g_frame = 0;
int g_active = 0;
bool g_initialized = false;

void cleanup()
{
    if (g_ctrl != nullptr)
    {
        *g_ctrl = 0;
        g_ctrl = nullptr;
    }
    g_slot[0] = nullptr;
    g_slot[1] = nullptr;
    if (g_nativeBase != nullptr)
    {
        munmap(const_cast<uint8_t*>(g_nativeBase), kNativeVideoRegionSize);
        g_nativeBase = nullptr;
    }
    if (g_fb != nullptr)
    {
        munmap(const_cast<uint8_t*>(g_fb), g_fbSize);
        g_fb = nullptr;
        g_fbSize = 0;
    }
    if (g_memFd >= 0)
    {
        close(g_memFd);
        g_memFd = -1;
    }
    if (g_fbFd >= 0)
    {
        close(g_fbFd);
        g_fbFd = -1;
    }
    g_frame = 0;
    g_active = 0;
    g_initialized = false;
}

} // namespace

void initNativeVideoWriter()
{
    if (g_initialized)
    {
        qInfo("native video writer: init requested but already initialised");
        return;
    }

    g_fbFd = open("/dev/fb0", O_RDONLY | O_CLOEXEC);
    if (g_fbFd < 0)
    {
        qWarning("native video writer: failed to open /dev/fb0");
        cleanup();
        return;
    }

    fb_fix_screeninfo fixed = {};
    fb_var_screeninfo var = {};
    if (ioctl(g_fbFd, FBIOGET_FSCREENINFO, &fixed) < 0 ||
        ioctl(g_fbFd, FBIOGET_VSCREENINFO, &var) < 0)
    {
        qWarning("native video writer: failed to inspect /dev/fb0");
        cleanup();
        return;
    }

    // Single-memcpy precondition: fb0 must be exactly the 320x240 RGB8888
    // surface the Menu fork core scans, with a tight stride and no pan
    // offsets, so one bulk copy reaches every pixel. MiSTer_Zaparoo's
    // wrapper sets fb0 up before the frontend starts; any deviation here
    // means the host configured the framebuffer differently than the
    // Menu fork core expects, and silently copying the top-left slice
    // would mask that misconfiguration.
    if (var.bits_per_pixel != 32 || var.xres != static_cast<uint32_t>(kOutputWidth) ||
        var.yres != static_cast<uint32_t>(kOutputHeight) || fixed.line_length != kOutputStride ||
        var.xoffset != 0 || var.yoffset != 0)
    {
        qWarning("native video writer: fb0 mode %ux%u %ubpp stride=%u offset=(%u,%u) does not "
                 "match required %dx%d 32bpp stride=%zu at (0,0); writer disabled",
                 var.xres, var.yres, var.bits_per_pixel, fixed.line_length, var.xoffset,
                 var.yoffset, kOutputWidth, kOutputHeight, kOutputStride);
        cleanup();
        return;
    }

    g_fbSize = fixed.smem_len != 0 ? fixed.smem_len : kOutputBytes;
    void* fbMap = mmap(nullptr, g_fbSize, PROT_READ, MAP_SHARED, g_fbFd, 0);
    if (fbMap == MAP_FAILED)
    {
        qWarning("native video writer: failed to map /dev/fb0");
        cleanup();
        return;
    }
    g_fb = static_cast<const uint8_t*>(fbMap);

    g_memFd = open("/dev/mem", O_RDWR | O_SYNC | O_CLOEXEC);
    if (g_memFd < 0)
    {
        qWarning("native video writer: failed to open /dev/mem");
        cleanup();
        return;
    }

    void* ddrMap = mmap(nullptr, kNativeVideoRegionSize, PROT_READ | PROT_WRITE, MAP_SHARED,
                        g_memFd, static_cast<off_t>(kNativeVideoBase));
    if (ddrMap == MAP_FAILED)
    {
        qWarning("native video writer: failed to map native video DDR at 0x%08zx",
                 kNativeVideoBase);
        cleanup();
        return;
    }
    g_nativeBase = static_cast<volatile uint8_t*>(ddrMap);
    g_slot[0] = g_nativeBase + kBuffer0Offset;
    g_slot[1] = g_nativeBase + kBuffer1Offset;
    g_ctrl =
        reinterpret_cast<volatile uint32_t*>(const_cast<uint8_t*>(g_nativeBase + kControlOffset));

    memset(const_cast<uint8_t*>(g_slot[0]), 0, kOutputBytes);
    memset(const_cast<uint8_t*>(g_slot[1]), 0, kOutputBytes);
    *g_ctrl = 0;
    // Control word's slot bit is 0, so the FPGA scans slot 0. Point
    // the first write at slot 1 so the very first frame goes into
    // the slot the FPGA is NOT reading; without this the initial
    // memcpy races the scanout and produces a one-frame tear at
    // startup.
    g_active = 1;

    g_initialized = true;
    qInfo("native video writer: initialised, fb0 %ux%u stride=%u -> DDR slots at "
          "0x%08zx / 0x%08zx, control at 0x%08zx",
          var.xres, var.yres, fixed.line_length, kNativeVideoBase + kBuffer0Offset,
          kNativeVideoBase + kBuffer1Offset, kNativeVideoBase + kControlOffset);
}

void copyFrameNativeVideoWriter()
{
    if (!g_initialized)
    {
        return;
    }

    // Single bulk copy: fb0 is validated as a contiguous 320x240x4 block
    // at (0,0), so the entire frame is one memcpy from the cached fb0
    // mapping to the uncached DDR slot. The cached -> uncached burst is
    // what makes this cheap on Cortex-A9; per-pixel uncached writes
    // from QPainter would not be.
    memcpy(const_cast<uint8_t*>(g_slot[g_active]), g_fb, kOutputBytes);

    // Publish the freshly written slot to the FPGA. The fence ensures
    // the memcpy's stores are visible at the DDR controller before the
    // control word advertises the slot index.
    std::atomic_thread_fence(std::memory_order_seq_cst);
    ++g_frame;
    *g_ctrl = (g_frame << 2) | static_cast<uint32_t>(g_active);
    g_active ^= 1;
}

void stopNativeVideoWriter()
{
    if (!g_initialized && g_fbFd < 0 && g_memFd < 0)
    {
        return;
    }
    qInfo("native video writer: stopping");
    cleanup();
}

#else

#include <QLoggingCategory>

void initNativeVideoWriter()
{
    qInfo("native video writer: init requested on unsupported build/platform");
}
void copyFrameNativeVideoWriter() {}
void stopNativeVideoWriter()
{
    qInfo("native video writer: stop requested on unsupported build/platform");
}

#endif
