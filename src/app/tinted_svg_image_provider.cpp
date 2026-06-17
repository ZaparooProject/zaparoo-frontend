// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

#include "tinted_svg_image_provider.h"

#include <QColor>
#include <QFile>
#include <QImage>
#include <QPainter>
#include <QQuickTextureFactory>
#include <QStringList>
#include <QSvgRenderer>
#include <QtGlobal>
#include <algorithm>
#include <sys/resource.h>
#include <utility>

namespace
{
constexpr int kDefaultSvgSize = 256;
constexpr auto kResourcePrefix = ":/qt/qml/Zaparoo/App/resources/";

QSize renderSizeFor(const QSvgRenderer& renderer, const QSize& requestedSize)
{
    const QSize defaultSize = renderer.defaultSize();
    QSize base = defaultSize.isValid() ? defaultSize : QSize(kDefaultSvgSize, kDefaultSvgSize);
    const int reqW = requestedSize.width();
    const int reqH = requestedSize.height();
    if (reqW > 0 && reqH > 0)
    {
        return requestedSize;
    }
    if (reqW > 0)
    {
        return {reqW, std::max(1, (base.height() * reqW) / std::max(1, base.width()))};
    }
    if (reqH > 0)
    {
        return {std::max(1, (base.width() * reqH) / std::max(1, base.height())), reqH};
    }
    return base;
}

QColor colorFromToken(const QString& token, const QColor& fallback)
{
    QColor color(QStringLiteral("#") + token);
    return color.isValid() ? color : fallback;
}

int channelMix(int a, int b, int amountB)
{
    return ((a * (255 - amountB)) + (b * amountB) + 127) / 255;
}

int lumaOf(QRgb source)
{
    return (((qRed(source) * 299) + (qGreen(source) * 587) + (qBlue(source) * 114)) + 500) / 1000;
}

struct ToneRange
{
    int min = 255;
    int max = 0;
    int pixels = 0;
};

ToneRange toneRangeOf(const QImage& image)
{
    ToneRange range;
    for (int y = 0; y < image.height(); ++y)
    {
        // NOLINTNEXTLINE(cppcoreguidelines-pro-type-reinterpret-cast)
        const auto* line = reinterpret_cast<const QRgb*>(image.constScanLine(y));
        for (int x = 0; x < image.width(); ++x)
        {
            // NOLINTNEXTLINE(cppcoreguidelines-pro-bounds-pointer-arithmetic)
            const QRgb source = line[x];
            if (qAlpha(source) <= 16)
            {
                continue;
            }
            const int luma = lumaOf(source);
            range.min = std::min(range.min, luma);
            range.max = std::max(range.max, luma);
            ++range.pixels;
        }
    }
    return range;
}

void tintImage(QImage& image, const QColor& highlight, const QColor& midtone, const QColor& shadow)
{
    QImage straight = image.convertToFormat(QImage::Format_ARGB32);
    const ToneRange range = toneRangeOf(straight);
    const bool singleTone = range.pixels == 0 || (range.max - range.min) < 16;
    const int highlightR = highlight.red();
    const int highlightG = highlight.green();
    const int highlightB = highlight.blue();
    const int midtoneR = midtone.red();
    const int midtoneG = midtone.green();
    const int midtoneB = midtone.blue();
    const int shadowR = shadow.red();
    const int shadowG = shadow.green();
    const int shadowB = shadow.blue();

    for (int y = 0; y < straight.height(); ++y)
    {
        // NOLINTNEXTLINE(cppcoreguidelines-pro-type-reinterpret-cast)
        auto* line = reinterpret_cast<QRgb*>(straight.scanLine(y));
        for (int x = 0; x < straight.width(); ++x)
        {
            // NOLINTNEXTLINE(cppcoreguidelines-pro-bounds-pointer-arithmetic)
            const QRgb source = line[x];
            const int alpha = qAlpha(source);
            if (alpha == 0)
            {
                continue;
            }

            int red = highlightR;
            int green = highlightG;
            int blue = highlightB;
            if (!singleTone)
            {
                // Preserve source light/dark ordering. This is a color-grade,
                // not a semantic recolor: darkest source areas map to a lifted
                // shadow tint, midtones pick up the theme tint, and brightest
                // source areas stay primary white. Gradients and antialiasing
                // remain smooth because the curve is monotonic.
                const int tone = std::clamp((lumaOf(source) - range.min) * 255 /
                                                std::max(1, range.max - range.min),
                                            0, 255);
                if (tone < 128)
                {
                    const int amount = tone * 2;
                    red = channelMix(shadowR, midtoneR, amount);
                    green = channelMix(shadowG, midtoneG, amount);
                    blue = channelMix(shadowB, midtoneB, amount);
                }
                else
                {
                    const int amount = (tone - 128) * 2;
                    red = channelMix(midtoneR, highlightR, amount);
                    green = channelMix(midtoneG, highlightG, amount);
                    blue = channelMix(midtoneB, highlightB, amount);
                }
            }

            // NOLINTNEXTLINE(cppcoreguidelines-pro-bounds-pointer-arithmetic)
            line[x] = qRgba(red, green, blue, alpha);
        }
    }
    image = straight.convertToFormat(QImage::Format_ARGB32_Premultiplied);
}
} // namespace

TintedSvgImageResponse::TintedSvgImageResponse(QString id, QSize requestedSize, QMutex* cacheMutex,
                                               QCache<QString, QImage>* logoCache)
    : m_id(std::move(id)), m_requestedSize(requestedSize), m_cacheMutex(cacheMutex),
      m_logoCache(logoCache)
{
    setAutoDelete(false);
}

QQuickTextureFactory* TintedSvgImageResponse::textureFactory() const
{
    if (m_factory)
    {
        return m_factory.release();
    }
    return QQuickTextureFactory::textureFactoryForImage(m_image);
}

QString TintedSvgImageResponse::errorString() const
{
    return m_error;
}

void TintedSvgImageResponse::run()
{
    // Check process-memory cache before doing any SVG render or tint work.
    // The result is deterministic for a given (id, requestedSize) pair, so
    // repeat loads after a QPixmapCache eviction or a category re-entry skip
    // the entire per-pixel pass. Key encodes both id and the requested size so
    // entries with different sizes don't alias each other.
    const QString cacheKey = m_id + QStringLiteral(":") + QString::number(m_requestedSize.width()) +
                             QStringLiteral("x") + QString::number(m_requestedSize.height());
    {
        QMutexLocker locker(m_cacheMutex);
        if (const QImage* cached = m_logoCache->object(cacheKey))
        {
            m_image = *cached;
            m_factory.reset(QQuickTextureFactory::textureFactoryForImage(m_image));
            emit finished();
            return;
        }
    }

    static thread_local bool s_decoderNiced = false;
    if (!s_decoderNiced)
    {
        setpriority(PRIO_PROCESS, 0, 10);
        s_decoderNiced = true;
    }

    const QStringList parts = m_id.split(QLatin1Char('/'));
    if (parts.size() < 4)
    {
        m_error = QStringLiteral("malformed tinted-svg id");
        qWarning("tinted-svg provider: malformed id=%s", qUtf8Printable(m_id));
        emit finished();
        return;
    }

    const QColor primary = colorFromToken(parts.at(0), QColor(Qt::white));
    const QColor secondary = colorFromToken(parts.at(1), QColor(Qt::white));
    const QColor shadow = colorFromToken(parts.at(2), QColor(Qt::black));
    const QString resourcePath = parts.mid(3).join(QLatin1Char('/'));
    // System logos, Hub category icons, and UI glyphs all share the provider.
    // Accept all three path prefixes; still require an .svg suffix.
    const bool knownPrefix = resourcePath.startsWith(QStringLiteral("images/systems/")) ||
                             resourcePath.startsWith(QStringLiteral("images/categories/")) ||
                             resourcePath.startsWith(QStringLiteral("images/icons/"));
    if (!knownPrefix || !resourcePath.endsWith(QStringLiteral(".svg")))
    {
        m_error = QStringLiteral("rejected tinted-svg path");
        qWarning("tinted-svg provider: rejected path=%s", qUtf8Printable(resourcePath));
        emit finished();
        return;
    }

    const QString fullResourcePath = QString::fromLatin1(kResourcePrefix) + resourcePath;
    if (!QFile::exists(fullResourcePath))
    {
        m_error = QStringLiteral("missing tinted-svg resource");
        emit finished();
        return;
    }

    QSvgRenderer renderer(fullResourcePath);
    if (!renderer.isValid())
    {
        m_error = QStringLiteral("invalid tinted-svg resource");
        qWarning("tinted-svg provider: invalid svg path=%s", qUtf8Printable(resourcePath));
        emit finished();
        return;
    }

    const QSize targetSize = renderSizeFor(renderer, m_requestedSize);
    QImage image(targetSize, QImage::Format_ARGB32_Premultiplied);
    image.fill(Qt::transparent);
    QPainter painter(&image);
    painter.setRenderHint(QPainter::Antialiasing, true);
    painter.setRenderHint(QPainter::SmoothPixmapTransform, true);
    renderer.render(&painter);
    painter.end();

    tintImage(image, primary, secondary, shadow);
    m_image = image;
    m_factory.reset(QQuickTextureFactory::textureFactoryForImage(m_image));

    // Store in the provider's process-memory cache so subsequent requests for
    // the same logo (e.g. after a QPixmapCache eviction) skip the render pass.
    // Cost is tracked in bytes so maxCost caps total memory use on MiSTer.
    {
        QMutexLocker locker(m_cacheMutex);
        if (!m_logoCache->contains(cacheKey))
        {
            const auto cost = static_cast<int>(m_image.sizeInBytes());
            // QCache::insert takes ownership of the raw pointer; this is the
            // documented API and the owning-memory diagnostic is expected here.
            // NOLINTNEXTLINE(cppcoreguidelines-owning-memory)
            m_logoCache->insert(cacheKey, new QImage(m_image), cost);
        }
    }

    emit finished();
}

// 16 MB cap: one 256×256 ARGB render is ~256 KB. With two ramps per logo
// (unfocused + focused) plus category and icon glyphs now in the same pool,
// the working set for a full visible page is roughly double the old single-ramp
// estimate. 16 MB keeps ~64 renders without churning re-renders on MiSTer.
static constexpr int kLogoCacheMaxBytes = 16 * 1024 * 1024;

TintedSvgImageProvider::TintedSvgImageProvider() : m_logoCache(kLogoCacheMaxBytes)
{
    m_pool.setMaxThreadCount(4);
}

QQuickImageResponse* TintedSvgImageProvider::requestImageResponse(const QString& id,
                                                                  const QSize& requestedSize)
{
    auto* response = new TintedSvgImageResponse(id, requestedSize, &m_cacheMutex, &m_logoCache);
    m_pool.start(response);
    return response;
}
