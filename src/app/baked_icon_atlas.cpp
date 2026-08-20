// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

#include "baked_icon_atlas.h"

#include "baked_icon_format.h"

#include <QRunnable>
#include <QThreadPool>
#include <QtEndian>
#include <QtGlobal>
#include <algorithm>
#include <cstring>

#if defined(Q_OS_UNIX)
#include <sys/resource.h>
#endif

namespace
{
constexpr int kPageSize = 4096;

quint16 readU16(const quint8* base, qsizetype offset)
{
    // qFromLittleEndian(const void*) is memcpy-based. rcc gives no alignment
    // guarantee for the blob and ARMv7 would fault on a direct load.
    // NOLINTNEXTLINE(cppcoreguidelines-pro-bounds-pointer-arithmetic)
    return qFromLittleEndian<quint16>(base + offset);
}

quint32 readU32(const quint8* base, qsizetype offset)
{
    // NOLINTNEXTLINE(cppcoreguidelines-pro-bounds-pointer-arithmetic)
    return qFromLittleEndian<quint32>(base + offset);
}

class PrefaultRunnable : public QRunnable
{
  public:
    PrefaultRunnable(const quint8* base, qsizetype size) : m_base(base), m_size(size) {}

    void run() override
    {
#if defined(Q_OS_UNIX)
        setpriority(PRIO_PROCESS, 0, 19);
#endif
        // volatile so the reads are not optimized away; the point is the page
        // fault, not the value.
        volatile quint8 sink = 0;
        for (qsizetype offset = 0; offset < m_size; offset += kPageSize)
        {
            // NOLINTNEXTLINE(cppcoreguidelines-pro-bounds-pointer-arithmetic)
            sink = static_cast<quint8>(sink ^ m_base[offset]);
        }
        Q_UNUSED(sink)
    }

  private:
    const quint8* m_base;
    qsizetype m_size;
};
} // namespace

// m_valid is declared last, so parse() runs after every other member has been
// default-initialized and is free to fill them in.
BakedIconAtlas::BakedIconAtlas()
    : m_resource(QString::fromLatin1(baked::kResourcePath)), m_valid(parse())
{
}

const BakedIconAtlas& BakedIconAtlas::instance()
{
    static const BakedIconAtlas atlas;
    return atlas;
}

bool BakedIconAtlas::parse()
{
    if (!m_resource.isValid())
    {
        qWarning("baked icons: atlas resource missing; falling back to SVG rendering");
        return false;
    }

    // The single highest-value check in the file. If the blob ever ships
    // compressed, everything still works and 6 MB silently moves from clean
    // mapped pages onto the heap, with no visible symptom on desktop and a
    // real one on MiSTer.
    if (m_resource.compressionAlgorithm() != QResource::NoCompression)
    {
        qWarning("baked icons: atlas is compressed (algorithm=%d); the -no-compress rcc option "
                 "was lost. Falling back to SVG rendering.",
                 static_cast<int>(m_resource.compressionAlgorithm()));
        return false;
    }

    m_base = m_resource.data();
    m_size = m_resource.size();
    if (m_base == nullptr || m_size < baked::kHeaderSize)
    {
        qWarning("baked icons: atlas is truncated (%lld bytes)", static_cast<long long>(m_size));
        return false;
    }

    if (std::memcmp(m_base, baked::kMagic, sizeof(baked::kMagic)) != 0)
    {
        qWarning("baked icons: bad magic; falling back to SVG rendering");
        return false;
    }
    const quint32 version = readU32(m_base, baked::kOffsetVersion);
    if (version != baked::kVersion)
    {
        qWarning("baked icons: unsupported atlas version %u (expected %u)", version,
                 baked::kVersion);
        return false;
    }
    const auto totalBytes = static_cast<qsizetype>(readU32(m_base, baked::kOffsetTotalBytes));
    if (totalBytes != m_size)
    {
        qWarning("baked icons: header says %lld bytes but the resource is %lld",
                 static_cast<long long>(totalBytes), static_cast<long long>(m_size));
        return false;
    }

    const auto count = static_cast<qsizetype>(readU32(m_base, baked::kOffsetEntryCount));
    const qsizetype directoryEnd = baked::kHeaderSize + (count * baked::kEntrySize);
    if (count <= 0 || directoryEnd > m_size)
    {
        qWarning("baked icons: directory of %lld entries does not fit",
                 static_cast<long long>(count));
        return false;
    }

    m_entries.reserve(static_cast<int>(count));
    for (qsizetype i = 0; i < count; ++i)
    {
        const qsizetype record = baked::kHeaderSize + (i * baked::kEntrySize);
        const auto pathOffset =
            static_cast<qsizetype>(readU32(m_base, record + baked::kEntryOffsetPath));
        const int width = readU16(m_base, record + baked::kEntryOffsetWidth);
        const int height = readU16(m_base, record + baked::kEntryOffsetHeight);
        const int baseWidth = readU16(m_base, record + baked::kEntryOffsetBaseWidth);
        const int baseHeight = readU16(m_base, record + baked::kEntryOffsetBaseHeight);
        const quint16 flags = readU16(m_base, record + baked::kEntryOffsetFlags);
        const auto alphaOffset =
            static_cast<qsizetype>(readU32(m_base, record + baked::kEntryOffsetAlpha));
        const auto toneOffset =
            static_cast<qsizetype>(readU32(m_base, record + baked::kEntryOffsetTone));

        if ((flags & ~baked::kFlagKnownMask) != 0)
        {
            qWarning("baked icons: entry %lld carries unknown flags 0x%04x",
                     static_cast<long long>(i), flags);
            return false;
        }
        const bool singleTone = (flags & baked::kFlagSingleTone) != 0;
        const qsizetype pixels = static_cast<qsizetype>(width) * height;
        if (width <= 0 || height <= 0 || pathOffset >= m_size || alphaOffset + pixels > m_size ||
            (!singleTone && (toneOffset == 0 || toneOffset + pixels > m_size)))
        {
            qWarning("baked icons: entry %lld has out-of-range offsets", static_cast<long long>(i));
            return false;
        }

        // NOLINTNEXTLINE(cppcoreguidelines-pro-type-reinterpret-cast,cppcoreguidelines-pro-bounds-pointer-arithmetic)
        const auto* pathStart = reinterpret_cast<const char*>(m_base + pathOffset);
        const void* terminator =
            std::memchr(pathStart, '\0', static_cast<size_t>(m_size - pathOffset));
        if (terminator == nullptr)
        {
            qWarning("baked icons: entry %lld has an unterminated path", static_cast<long long>(i));
            return false;
        }

        Entry entry;
        // NOLINTNEXTLINE(cppcoreguidelines-pro-bounds-pointer-arithmetic)
        entry.alpha = m_base + alphaOffset;
        // NOLINTNEXTLINE(cppcoreguidelines-pro-bounds-pointer-arithmetic)
        entry.tone = singleTone ? nullptr : m_base + toneOffset;
        entry.width = width;
        entry.height = height;
        entry.baseWidth = std::max(1, baseWidth);
        entry.baseHeight = std::max(1, baseHeight);
        entry.singleTone = singleTone;
        m_entries.insert(QString::fromUtf8(pathStart), entry);
    }

    return true;
}

QSize BakedIconAtlas::resolveSize(const Entry& entry, const QSize& requestedSize)
{
    const int reqW = requestedSize.width();
    const int reqH = requestedSize.height();
    if (reqW > 0 && reqH > 0)
    {
        return requestedSize;
    }
    if (reqW > 0)
    {
        return {reqW, std::max(1, (entry.baseHeight * reqW) / std::max(1, entry.baseWidth))};
    }
    if (reqH > 0)
    {
        return {std::max(1, (entry.baseWidth * reqH) / std::max(1, entry.baseHeight)), reqH};
    }
    // No sourceSize at all means "the SVG's natural size", which for a wide
    // logo is far larger than the baked raster and therefore routes back to
    // the SVG path. Every shipped call site pins sourceSize (see
    // docs/style.md -> "One rasterization, at the painted size"); this
    // fallback only matters for a call site that forgets to.
    return {entry.baseWidth, entry.baseHeight};
}

const BakedIconAtlas::Entry* BakedIconAtlas::find(const QString& resourcePath) const
{
    if (!m_valid)
    {
        return nullptr;
    }
    const auto it = m_entries.constFind(resourcePath);
    return it == m_entries.constEnd() ? nullptr : &it.value();
}

void BakedIconAtlas::prefaultAsync() const
{
    if (!m_valid)
    {
        return;
    }
    // NOLINTNEXTLINE(cppcoreguidelines-owning-memory)
    auto* runnable = new PrefaultRunnable(m_base, m_size);
    runnable->setAutoDelete(true);
    QThreadPool::globalInstance()->start(runnable, /*priority=*/-1);
}
