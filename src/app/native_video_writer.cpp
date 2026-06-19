// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

#include "native_video_writer.h"

#if defined(ZAPAROO_EMBEDDED_BUILD) && defined(__linux__)

#include <QLoggingCategory>
#include <algorithm>
#include <atomic>
#include <cstdint>
#include <cstring>
#include <fcntl.h>
#include <linux/fb.h>
#include <sys/ioctl.h>
#include <sys/mman.h>
#include <unistd.h>

// Offsets the Rust layer loaded from frontend.toml at startup; the
// writer pulls them here so main.cpp needs no plumbing. Defined in
// rust/frontend/src/lib.rs.
extern "C" int32_t zaparoo_rust_crt_h_offset();
extern "C" int32_t zaparoo_rust_crt_v_offset();

namespace
{

// Menu fork v2 DDR contract (see the fork's docs/native-video-plan.md
// and rtl/native_video_reader.sv, which is the normative consumer).
constexpr uintptr_t kNativeVideoBase = 0x3A000000u;
constexpr size_t kNativeVideoRegionSize = 0x00300000u;
constexpr size_t kWord0Offset = 0x00000000u;
constexpr size_t kWord1Offset = 0x00000004u;
constexpr size_t kBuffer0Offset = 0x00001000u;
constexpr size_t kBuffer1Offset = 0x00180000u;
constexpr size_t kSourceBytesPerPixel = 4;

struct NativeVideoMode
{
    uint32_t mode;
    uint32_t width;
    uint32_t height;

    constexpr size_t stride() const
    {
        return width * kSourceBytesPerPixel;
    }
    constexpr size_t frameBytes() const
    {
        return stride() * height;
    }
};

// fb0 geometry doubles as the mode selector: the host (Main_MiSTer for
// NTSC, our own startup vmode for PAL/480i) configures fb0 to one of
// exactly these shapes before frames flow.
constexpr NativeVideoMode kModes[] = {
    {0, 352, 240}, // NTSC 60p
    {1, 720, 480}, // 480i60, rendered progressive (core extracts fields)
    {2, 352, 288}, // PAL 50p
};

static_assert(kModes[0].frameBytes() == 0x52800);
static_assert(kModes[1].frameBytes() == 0x151800);
static_assert(kModes[2].frameBytes() == 0x63000);
// Both slots must fit ahead of the next buffer / region end even in
// the largest (480i) mode.
static_assert(kBuffer0Offset + kModes[1].frameBytes() <= kBuffer1Offset);
static_assert(kBuffer1Offset + kModes[1].frameBytes() <= kNativeVideoRegionSize);

int g_fbFd = -1;
int g_memFd = -1;
const uint8_t* g_fb = nullptr;
size_t g_fbSize = 0;
volatile uint8_t* g_nativeBase = nullptr;
volatile uint8_t* g_slot[2] = {nullptr, nullptr};
volatile uint32_t* g_word0 = nullptr;
volatile uint32_t* g_word1 = nullptr;
uint32_t g_mode = 0;
size_t g_frameBytes = 0;
uint32_t g_frame = 0;
int g_active = 0;
bool g_initialized = false;

const NativeVideoMode* modeForGeometry(uint32_t width, uint32_t height)
{
    for (const NativeVideoMode& mode : kModes)
    {
        if (mode.width == width && mode.height == height)
        {
            return &mode;
        }
    }
    return nullptr;
}

int clampOffset(int value, int min, int max)
{
    return std::min(std::max(value, min), max);
}

void cleanup()
{
    // Word order matters on the way down too: word0 = 0 is the "writer
    // stopped" signal (the core reverts to its noise pattern within one
    // frame); word1 is zeroed afterwards for tidiness.
    if (g_word0 != nullptr)
    {
        *g_word0 = 0;
        g_word0 = nullptr;
    }
    if (g_word1 != nullptr)
    {
        *g_word1 = 0;
        g_word1 = nullptr;
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
    g_mode = 0;
    g_frameBytes = 0;
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

    // Single-memcpy precondition: fb0 must be exactly one of the v2
    // mode geometries in RGB8888 with a tight stride and no pan
    // offsets, so one bulk copy reaches every pixel. The host sets fb0
    // up before the frontend starts; any deviation means the host
    // configured the framebuffer differently than the Menu fork core
    // expects, and silently copying a slice would mask that
    // misconfiguration. This also self-disables the writer against an
    // old host that still configures 320x240.
    const NativeVideoMode* mode = modeForGeometry(var.xres, var.yres);
    if (mode == nullptr || var.bits_per_pixel != 32 || fixed.line_length != mode->stride() ||
        var.xoffset != 0 || var.yoffset != 0)
    {
        qWarning("native video writer: fb0 mode %ux%u %ubpp stride=%u offset=(%u,%u) does not "
                 "match a v2 mode geometry (352x240, 720x480, 352x288; 32bpp tight stride at "
                 "(0,0)); writer disabled",
                 var.xres, var.yres, var.bits_per_pixel, fixed.line_length, var.xoffset,
                 var.yoffset);
        cleanup();
        return;
    }

    g_fbSize = fixed.smem_len != 0 ? fixed.smem_len : mode->frameBytes();
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
    g_word0 =
        reinterpret_cast<volatile uint32_t*>(const_cast<uint8_t*>(g_nativeBase + kWord0Offset));
    g_word1 =
        reinterpret_cast<volatile uint32_t*>(const_cast<uint8_t*>(g_nativeBase + kWord1Offset));
    g_mode = mode->mode;
    g_frameBytes = mode->frameBytes();

    memset(const_cast<uint8_t*>(g_slot[0]), 0, g_frameBytes);
    memset(const_cast<uint8_t*>(g_slot[1]), 0, g_frameBytes);

    const int hOffset = clampOffset(static_cast<int>(zaparoo_rust_crt_h_offset()),
                                    kNativeVideoHOffsetMin, kNativeVideoHOffsetMax);
    const int vOffset = clampOffset(static_cast<int>(zaparoo_rust_crt_v_offset()),
                                    kNativeVideoVOffsetMin, kNativeVideoVOffsetMax);

    // word1 before word0: the core reads both words in one atomic
    // 64-bit beat per vblank, and publishing (a nonzero word0) with a
    // stale word1 would misinterpret the very first frame. word0 stays
    // 0 ("writer stopped") until the first copy publishes a real frame.
    *g_word1 = packNativeVideoWord1(hOffset, vOffset, g_mode);
    *g_word0 = 0;
    // word0's slot bit is 0, so the FPGA would scan slot 0 first.
    // Point the first write at slot 1 so the very first frame goes
    // into the slot the FPGA is NOT reading; without this the initial
    // memcpy races the scanout and produces a one-frame tear at
    // startup.
    g_active = 1;

    g_initialized = true;
    qInfo("native video writer: initialised, fb0 %ux%u stride=%u mode=%u offsets=(%d,%d) -> DDR "
          "slots at 0x%08zx / 0x%08zx, control at 0x%08zx",
          var.xres, var.yres, fixed.line_length, g_mode, hOffset, vOffset,
          kNativeVideoBase + kBuffer0Offset, kNativeVideoBase + kBuffer1Offset,
          kNativeVideoBase + kWord0Offset);
}

void copyFrameNativeVideoWriter()
{
    if (!g_initialized)
    {
        return;
    }

    // Single bulk copy: fb0 is validated as a contiguous tight-stride
    // block at (0,0), so the entire frame is one memcpy from the cached
    // fb0 mapping to the uncached DDR slot. The cached -> uncached
    // burst is what makes this cheap on Cortex-A9; per-pixel uncached
    // writes from QPainter would not be. 480i is published as one
    // progressive 720x480 frame; the core extracts fields itself.
    memcpy(const_cast<uint8_t*>(g_slot[g_active]), g_fb, g_frameBytes);

    // Publish the freshly written slot to the FPGA. The fence ensures
    // the memcpy's stores (and any preceding word1 rewrite) are visible
    // at the DDR controller before word0 advertises the slot index.
    std::atomic_thread_fence(std::memory_order_seq_cst);
    ++g_frame;
    *g_word0 = (g_frame << 2) | static_cast<uint32_t>(g_active);
    g_active ^= 1;
}

void setNativeVideoOffsets(int hOffset, int vOffset)
{
    if (!g_initialized)
    {
        return;
    }
    const int clampedH = clampOffset(hOffset, kNativeVideoHOffsetMin, kNativeVideoHOffsetMax);
    const int clampedV = clampOffset(vOffset, kNativeVideoVOffsetMin, kNativeVideoVOffsetMax);
    // word1-then-word0 ordering for runtime changes is satisfied by the
    // next copyFrameNativeVideoWriter() call: its seq_cst fence orders
    // this store ahead of the word0 bump, and the core latches both
    // words together at the field boundary.
    *g_word1 = packNativeVideoWord1(clampedH, clampedV, g_mode);
}

extern "C" void zaparoo_native_video_set_offsets(int32_t hOffset, int32_t vOffset)
{
    setNativeVideoOffsets(static_cast<int>(hOffset), static_cast<int>(vOffset));
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
void setNativeVideoOffsets(int /*hOffset*/, int /*vOffset*/) {}
extern "C" void zaparoo_native_video_set_offsets(int32_t hOffset, int32_t vOffset)
{
    setNativeVideoOffsets(static_cast<int>(hOffset), static_cast<int>(vOffset));
}
void stopNativeVideoWriter()
{
    qInfo("native video writer: stop requested on unsupported build/platform");
}

#endif
