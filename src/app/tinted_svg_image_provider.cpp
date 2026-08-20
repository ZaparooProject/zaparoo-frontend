// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

#include "tinted_svg_image_provider.h"

#include "baked_icon_atlas.h"
#include "svg_render_size.h"
#include "tint_lut.h"

#include <QColor>
#include <QCoreApplication>
#include <QFile>
#include <QPainter>
#include <QStringList>
#include <QSvgRenderer>
#include <QThread>
#include <QtGlobal>
#include <sys/resource.h>

namespace
{
constexpr auto kResourcePrefix = ":/qt/qml/Zaparoo/App/resources/";

// 4 MiB, down from the 32 MiB the async provider needed. QQuickPixmapCache
// keeps its own 2 MiB budget for *unreferenced* pixmaps and on-screen tiles are
// referenced, so this only has to cover what goes unreferenced in one step --
// the Settings Loader deactivating (~3 MiB) and a color-scheme change
// re-tinting everything. A miss is now a sub-millisecond LUT pass rather than a
// 10-20 ms SVG parse, so the cache is a comfort, not load-bearing.
constexpr int kTintedCacheMaxBytes = 4 * 1024 * 1024;

struct Request
{
    QColor highlight;
    QColor midtone;
    QColor shadow;
    QString resourcePath;
};

QColor colorFromToken(const QString& token, const QColor& fallback)
{
    QColor color(QStringLiteral("#") + token);
    return color.isValid() ? color : fallback;
}

bool parseRequest(const QString& id, Request* out, QString* error)
{
    const QStringList parts = id.split(QLatin1Char('/'));
    if (parts.size() < 4)
    {
        *error = QStringLiteral("malformed tinted-svg id");
        return false;
    }
    out->highlight = colorFromToken(parts.at(0), QColor(Qt::white));
    out->midtone = colorFromToken(parts.at(1), QColor(Qt::white));
    out->shadow = colorFromToken(parts.at(2), QColor(Qt::black));
    out->resourcePath = parts.mid(3).join(QLatin1Char('/'));

    // System logos, Hub category icons, UI glyphs, and host-status icons all
    // share the provider. Corner masks (ContextMenu's rounded scrim hole)
    // share it too, but have no SVG source -- they are baked directly from a
    // Qt Quick render -- so they are exempt from the .svg suffix check below.
    const bool cornerPrefix = out->resourcePath.startsWith(QStringLiteral("images/corners/"));
    const bool knownPrefix = out->resourcePath.startsWith(QStringLiteral("images/systems/")) ||
                             out->resourcePath.startsWith(QStringLiteral("images/categories/")) ||
                             out->resourcePath.startsWith(QStringLiteral("images/icons/")) ||
                             out->resourcePath.startsWith(QStringLiteral("images/status/")) ||
                             cornerPrefix;
    if (!knownPrefix || (!cornerPrefix && !out->resourcePath.endsWith(QStringLiteral(".svg"))))
    {
        *error = QStringLiteral("rejected tinted-svg path");
        return false;
    }
    return true;
}

// Wraps a mask plane without copying. Planes are tightly packed at `width`
// bytes per row and 16-byte aligned, so the non-owning QImage constructor can
// point straight into the mapped blob.
QImage planeView(const quint8* data, int width, int height)
{
    return {data, width, height, width, QImage::Format_Grayscale8};
}

QImage scalePlane(const QImage& plane, const QSize& size)
{
    QImage scaled = plane.scaled(size, Qt::IgnoreAspectRatio, Qt::SmoothTransformation);
    if (scaled.format() != QImage::Format_Grayscale8)
    {
        scaled = scaled.convertToFormat(QImage::Format_Grayscale8);
    }
    return scaled;
}

// Single pass, no format conversions and no deep copies. The old path made
// three passes plus two convertToFormat() copies of the whole image.
QImage tintFromPlanes(const QImage& alphaPlane, const QImage& tonePlane, const Request& request,
                      bool singleTone)
{
    const int width = alphaPlane.width();
    const int height = alphaPlane.height();
    QImage out(width, height, QImage::Format_ARGB32_Premultiplied);
    if (out.isNull())
    {
        return {};
    }
    const tint::TintLut lut =
        tint::makeTintLut(request.highlight, request.midtone, request.shadow, singleTone);

    for (int y = 0; y < height; ++y)
    {
        const quint8* alpha = alphaPlane.constScanLine(y);
        // NOLINTNEXTLINE(cppcoreguidelines-pro-type-reinterpret-cast)
        auto* dst = reinterpret_cast<QRgb*>(out.scanLine(y));
        if (singleTone)
        {
            for (int x = 0; x < width; ++x)
            {
                // NOLINTNEXTLINE(cppcoreguidelines-pro-bounds-pointer-arithmetic,cppcoreguidelines-pro-bounds-constant-array-index)
                dst[x] = lut.flatPremul[alpha[x]];
            }
            continue;
        }
        const quint8* tone = tonePlane.constScanLine(y);
        for (int x = 0; x < width; ++x)
        {
            // NOLINTNEXTLINE(cppcoreguidelines-pro-bounds-pointer-arithmetic,cppcoreguidelines-pro-bounds-constant-array-index)
            const QRgb graded = lut.tone[tone[x]];
            // NOLINTNEXTLINE(cppcoreguidelines-pro-bounds-pointer-arithmetic)
            dst[x] = qPremultiply(qRgba(qRed(graded), qGreen(graded), qBlue(graded), alpha[x]));
        }
    }
    return out;
}

QImage renderFromAtlas(const BakedIconAtlas::Entry& entry, const Request& request,
                       const QSize& size)
{
    const QImage alphaPlane = planeView(entry.alpha, entry.width, entry.height);
    const QImage tonePlane =
        entry.singleTone ? QImage() : planeView(entry.tone, entry.width, entry.height);

    if (size == QSize(entry.width, entry.height))
    {
        return tintFromPlanes(alphaPlane, tonePlane, request, entry.singleTone);
    }
    // Downscale the masks, not the tinted result: Qt's smooth transform is an
    // area average, so averaging tone indices before the ramp keeps the ramp
    // monotonic instead of blending two graded colors.
    const QImage scaledAlpha = scalePlane(alphaPlane, size);
    const QImage scaledTone = entry.singleTone ? QImage() : scalePlane(tonePlane, size);
    return tintFromPlanes(scaledAlpha, scaledTone, request, entry.singleTone);
}

QImage renderFromSvg(const Request& request, const QSize& requestedSize, QString* error)
{
    // Only reached for requests larger than the baked raster (BrowseDetailPane
    // at 512/768) and for assets missing from the atlas. Rasterizing an SVG is
    // 10-20 ms, so this branch -- and only this branch -- is worth nicing down
    // when it runs off the GUI thread.
    static thread_local bool s_decoderNiced = false;
    if (!s_decoderNiced && QThread::currentThread() != qApp->thread())
    {
        setpriority(PRIO_PROCESS, 0, 10);
        s_decoderNiced = true;
    }

    const QString fullResourcePath = QString::fromLatin1(kResourcePrefix) + request.resourcePath;
    if (!QFile::exists(fullResourcePath))
    {
        *error = QStringLiteral("missing tinted-svg resource");
        return {};
    }
    QSvgRenderer renderer(fullResourcePath);
    if (!renderer.isValid())
    {
        *error = QStringLiteral("invalid tinted-svg resource");
        return {};
    }

    const QSize targetSize = svgsize::renderSizeFor(renderer, requestedSize);
    QImage image(targetSize, QImage::Format_ARGB32_Premultiplied);
    if (image.isNull())
    {
        *error = QStringLiteral("tinted-svg allocation failed");
        return {};
    }
    image.fill(Qt::transparent);
    QPainter painter(&image);
    painter.setRenderHint(QPainter::Antialiasing, true);
    painter.setRenderHint(QPainter::SmoothPixmapTransform, true);
    renderer.render(&painter);
    painter.end();

    tint::tintImage(image, request.highlight, request.midtone, request.shadow);
    return image;
}
} // namespace

TintedSvgImageProvider::TintedSvgImageProvider()
    : QQuickImageProvider(QQuickImageProvider::Image), m_tintedCache(kTintedCacheMaxBytes)
{
}

QImage TintedSvgImageProvider::requestImage(const QString& id, QSize* size,
                                            const QSize& requestedSize)
{
    Request request;
    QString error;
    if (!parseRequest(id, &request, &error))
    {
        qWarning("tinted-svg provider: %s id=%s", qUtf8Printable(error), qUtf8Printable(id));
        return {};
    }

    const BakedIconAtlas::Entry* entry = BakedIconAtlas::instance().find(request.resourcePath);
    // Resolve the output size before keying the cache. The old key embedded the
    // raw request, so a 256x0 and a 256x67 request for the same logo stored two
    // byte-identical copies.
    const QSize resolved =
        entry != nullptr ? BakedIconAtlas::resolveSize(*entry, requestedSize) : QSize();
    const bool atlasFits =
        entry != nullptr && resolved.width() <= entry->width && resolved.height() <= entry->height;
    const QString cacheKey =
        id + QLatin1Char('@') +
        QString::number(atlasFits ? resolved.width() : requestedSize.width()) + QLatin1Char('x') +
        QString::number(atlasFits ? resolved.height() : requestedSize.height());

    {
        const QMutexLocker locker(&m_cacheMutex);
        if (const QImage* cached = m_tintedCache.object(cacheKey))
        {
            *size = cached->size();
            return *cached;
        }
    }

    QImage image;
    if (atlasFits)
    {
        image = renderFromAtlas(*entry, request, resolved);
    }
    else
    {
        image = renderFromSvg(request, requestedSize, &error);
        if (image.isNull())
        {
            qWarning("tinted-svg provider: %s path=%s", qUtf8Printable(error),
                     qUtf8Printable(request.resourcePath));
            return {};
        }
    }

    *size = image.size();
    {
        const QMutexLocker locker(&m_cacheMutex);
        if (!m_tintedCache.contains(cacheKey))
        {
            const auto cost = static_cast<int>(image.sizeInBytes());
            // QCache::insert takes ownership of the raw pointer; this is the
            // documented API and the owning-memory diagnostic is expected here.
            // NOLINTNEXTLINE(cppcoreguidelines-owning-memory)
            m_tintedCache.insert(cacheKey, new QImage(image), cost);
        }
    }
    return image;
}
