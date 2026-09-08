// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

#include "fb_mmap_fallback.h"

#if defined(ZAPAROO_EMBEDDED_BUILD) && defined(__linux__)

#include <QLoggingCategory>
#include <cerrno>
#include <cstdarg>
#include <cstdint>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <fcntl.h>
#include <linux/fb.h>
#include <mutex>
#include <sys/ioctl.h>
#include <sys/mman.h>
#include <sys/stat.h>
#include <sys/sysmacros.h>
#include <unistd.h>

// The linker's --wrap gives us __real_* aliases for the genuine libc entry
// points. mmap64 is wrapped alongside mmap because Qt and our own translation
// units compile with _FILE_OFFSET_BITS=64, where glibc rewrites `mmap` to
// `mmap64`; wrapping only one of the two would silently miss every call.
//
// The offset types are spelled out rather than written as `off_t`/`off64_t` on
// purpose. `off_t` changes width with _FILE_OFFSET_BITS, so using it here would
// make these declarations disagree with the libc symbols they bind to whenever
// this file's feature macros differ from the caller's, and on ARM32 a 64-bit
// argument occupies an aligned register pair where a 32-bit one does not. The
// legacy `mmap` always takes a word-sized offset (`long`) and `mmap64` always
// takes a 64-bit one, whatever the macros say.
extern "C"
{
    void* __real_mmap(void* addr, size_t length, int prot, int flags, int fd, long offset);
    void* __real_mmap64(void* addr, size_t length, int prot, int flags, int fd, int64_t offset);
    int __real_munmap(void* addr, size_t length);
    void* __wrap_mmap(void* addr, size_t length, int prot, int flags, int fd, long offset);
    void* __wrap_mmap64(void* addr, size_t length, int prot, int flags, int fd, int64_t offset);
    int __wrap_munmap(void* addr, size_t length);
}

namespace
{

// One entry per distinct physical range. In practice this is one (Qt's
// surface), or two when the CRT path also maps fb0 for its DDR copy.
constexpr int kMaxEntries = 4;
// FB_MAJOR. /dev/fb0 is 29,0 and the check keeps the fallback from firing on
// an unrelated fd that happens to fail with ENODEV.
constexpr unsigned int kFbMajor = 29;

struct Entry
{
    unsigned long physStart;
    size_t length;
    // Buffer handed back to the caller. In direct mode this is `physMap`; in
    // staged mode it is cached anonymous RAM copied out by the flush.
    void* handed;
    void* physMap;
};

std::mutex g_mutex;
Entry g_entries[kMaxEntries];
int g_entryCount = 0;
bool g_active = false;
bool g_staged = false;
char g_status[256] = "framebuffer mmap fallback: not engaged (kernel fbdev mmap works)";

void setStatus(const char* format, ...) __attribute__((format(printf, 1, 2)));
void setStatus(const char* format, ...)
{
    va_list args;
    va_start(args, format);
    vsnprintf(g_status, sizeof(g_status), format, args);
    va_end(args);
}

bool isFramebufferFd(int fd)
{
    struct stat info = {};
    if (fstat(fd, &info) != 0 || !S_ISCHR(info.st_mode))
    {
        return false;
    }
    return major(info.st_rdev) == kFbMajor;
}

// Resolved once, on the first redirect, so a later setenv cannot leave two
// mappings of the same surface disagreeing about the strategy.
void resolveMode()
{
    const char* mode = getenv("ZAPAROO_FB_FALLBACK");
    g_staged = mode != nullptr && strcmp(mode, "staged") == 0;
}

bool disabled()
{
    const char* mode = getenv("ZAPAROO_FB_FALLBACK");
    return mode != nullptr && strcmp(mode, "off") == 0;
}

// Caller holds g_mutex.
void* redirect(size_t length, int fd, int64_t offset)
{
    fb_fix_screeninfo fixed = {};
    // FBIOGET_FSCREENINFO is handled by the fbdev core, not the driver, so it
    // still works on the very kernel whose mmap is missing. This is what makes
    // the fallback self-configuring rather than hardcoding 0x22001000.
    if (ioctl(fd, FBIOGET_FSCREENINFO, &fixed) != 0)
    {
        setStatus("framebuffer mmap fallback: FBIOGET_FSCREENINFO failed (%s)", strerror(errno));
        return MAP_FAILED;
    }
    if (fixed.smem_start == 0 || fixed.smem_len == 0)
    {
        setStatus("framebuffer mmap fallback: driver reports no framebuffer memory "
                  "(smem_start=0x%lx smem_len=%u)",
                  static_cast<unsigned long>(fixed.smem_start), fixed.smem_len);
        return MAP_FAILED;
    }
    if (offset < 0 || static_cast<uint64_t>(offset) + length > fixed.smem_len)
    {
        setStatus("framebuffer mmap fallback: request (offset=%lld len=%zu) exceeds the %u byte "
                  "framebuffer; not redirected",
                  static_cast<long long>(offset), length, fixed.smem_len);
        return MAP_FAILED;
    }

    const unsigned long physStart = fixed.smem_start + static_cast<unsigned long>(offset);

    // A second mapping of the same surface has to see the same bytes, exactly
    // as two MAP_SHARED mappings of fb0 did. Without this the CRT writer would
    // copy a blank buffer to the FPGA while Qt drew into a different one.
    for (int i = 0; i < g_entryCount; ++i)
    {
        if (g_entries[i].physStart == physStart && length <= g_entries[i].length)
        {
            return g_entries[i].handed;
        }
    }

    if (g_entryCount == kMaxEntries)
    {
        setStatus("framebuffer mmap fallback: entry table full; not redirected");
        return MAP_FAILED;
    }

    const int memFd = open("/dev/mem", O_RDWR | O_SYNC | O_CLOEXEC);
    if (memFd < 0)
    {
        setStatus("framebuffer mmap fallback: cannot open /dev/mem (%s)", strerror(errno));
        return MAP_FAILED;
    }
    void* physMap = __real_mmap64(nullptr, length, PROT_READ | PROT_WRITE, MAP_SHARED, memFd,
                                  static_cast<int64_t>(physStart));
    close(memFd);
    if (physMap == MAP_FAILED)
    {
        setStatus("framebuffer mmap fallback: cannot map /dev/mem at 0x%lx+%zu (%s)", physStart,
                  length, strerror(errno));
        return MAP_FAILED;
    }

    if (g_entryCount == 0)
    {
        resolveMode();
    }

    void* handed = physMap;
    if (g_staged)
    {
        void* staging = __real_mmap64(nullptr, length, PROT_READ | PROT_WRITE,
                                      MAP_PRIVATE | MAP_ANONYMOUS, -1, 0);
        if (staging == MAP_FAILED)
        {
            setStatus("framebuffer mmap fallback: cannot allocate %zu byte staging buffer (%s)",
                      length, strerror(errno));
            __real_munmap(physMap, length);
            return MAP_FAILED;
        }
        // Seed from the live framebuffer so a first frame that only touches
        // part of the surface does not publish uninitialised memory.
        memcpy(staging, physMap, length);
        handed = staging;
    }

    g_entries[g_entryCount] = Entry{physStart, length, handed, physMap};
    ++g_entryCount;
    g_active = true;
    setStatus("framebuffer mmap fallback: engaged in %s mode, %zu bytes at 0x%lx via /dev/mem "
              "(kernel fbdev has no fb_mmap)",
              g_staged ? "staged" : "direct", length, physStart);
    return handed;
}

} // namespace

extern "C" void* __wrap_mmap(void* addr, size_t length, int prot, int flags, int fd, long offset)
{
    return __wrap_mmap64(addr, length, prot, flags, fd, static_cast<int64_t>(offset));
}

extern "C" void* __wrap_mmap64(void* addr, size_t length, int prot, int flags, int fd,
                               int64_t offset)
{
    void* result = __real_mmap64(addr, length, prot, flags, fd, offset);
    if (result != MAP_FAILED)
    {
        return result;
    }

    // Anything other than "this fbdev has no mmap" is a real failure and must
    // reach the caller untouched, errno included.
    const int savedErrno = errno;
    if (savedErrno != ENODEV || fd < 0 || addr != nullptr || disabled() || !isFramebufferFd(fd))
    {
        errno = savedErrno;
        return MAP_FAILED;
    }

    std::lock_guard<std::mutex> lock(g_mutex);
    void* redirected = redirect(length, fd, offset);
    if (redirected == MAP_FAILED)
    {
        errno = savedErrno;
        return MAP_FAILED;
    }
    return redirected;
}

extern "C" int __wrap_munmap(void* addr, size_t length)
{
    {
        std::lock_guard<std::mutex> lock(g_mutex);
        for (int i = 0; i < g_entryCount; ++i)
        {
            if (g_entries[i].handed != addr)
            {
                continue;
            }
            // Qt unmaps its surface when the screen goes away. Drop the entry
            // first so a queued flush cannot write through a stale pointer,
            // and release the /dev/mem mapping the caller knows nothing about.
            void* physMap = g_entries[i].physMap;
            const size_t mapped = g_entries[i].length;
            const bool staged = g_entries[i].handed != physMap;
            g_entries[i] = g_entries[g_entryCount - 1];
            --g_entryCount;
            if (g_entryCount == 0)
            {
                g_active = false;
            }
            if (staged)
            {
                __real_munmap(physMap, mapped);
                return __real_munmap(addr, length);
            }
            // Direct mode: `addr` is the /dev/mem mapping itself, so the
            // caller's own munmap below releases it.
            break;
        }
    }
    return __real_munmap(addr, length);
}

bool fbMmapFallbackActive()
{
    std::lock_guard<std::mutex> lock(g_mutex);
    return g_active;
}

void flushFbMmapFallback()
{
    if (!g_active || !g_staged)
    {
        return;
    }
    std::lock_guard<std::mutex> lock(g_mutex);
    for (int i = 0; i < g_entryCount; ++i)
    {
        if (g_entries[i].handed != g_entries[i].physMap)
        {
            memcpy(g_entries[i].physMap, g_entries[i].handed, g_entries[i].length);
        }
    }
}

void stopFbMmapFallback()
{
    std::lock_guard<std::mutex> lock(g_mutex);
    if (g_entryCount == 0)
    {
        g_active = false;
        return;
    }
    for (int i = 0; i < g_entryCount; ++i)
    {
        if (g_entries[i].handed != g_entries[i].physMap)
        {
            __real_munmap(g_entries[i].handed, g_entries[i].length);
        }
        __real_munmap(g_entries[i].physMap, g_entries[i].length);
    }
    g_entryCount = 0;
    g_active = false;
}

void logFbMmapFallbackStatus()
{
    std::lock_guard<std::mutex> lock(g_mutex);
    if (g_active)
    {
        qInfo("%s", g_status);
    }
    else
    {
        qDebug("%s", g_status);
    }
}

#else

#include <QLoggingCategory>

bool fbMmapFallbackActive()
{
    return false;
}
void flushFbMmapFallback() {}
void stopFbMmapFallback() {}
void logFbMmapFallbackStatus() {}

#endif
