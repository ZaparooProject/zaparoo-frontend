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

int mixChannel(int a, int b, int amountB)
{
    return (a * (255 - amountB) + b * amountB + 127) / 255;
}

void tintImage(QImage& image, const QColor& foreground, const QColor& background)
{
    QImage straight = image.convertToFormat(QImage::Format_ARGB32);
    const int fgR = foreground.red();
    const int fgG = foreground.green();
    const int fgB = foreground.blue();
    // Map the darkest source pixels to a contrast-safe secondary tint,
    // not to the card background. The black/white upstream logos use
    // black and dark grays for real strokes/details; treating black as
    // transparent/background erases those parts on dark themes.
    constexpr int kDarkTintForegroundMix = 180;
    const int darkR = mixChannel(background.red(), fgR, kDarkTintForegroundMix);
    const int darkG = mixChannel(background.green(), fgG, kDarkTintForegroundMix);
    const int darkB = mixChannel(background.blue(), fgB, kDarkTintForegroundMix);

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
            const int luma =
                (qRed(source) * 299 + qGreen(source) * 587 + qBlue(source) * 114 + 500) / 1000;
            const int inv = 255 - luma;
            const int red = (darkR * inv + fgR * luma + 127) / 255;
            const int green = (darkG * inv + fgG * luma + 127) / 255;
            const int blue = (darkB * inv + fgB * luma + 127) / 255;
            // NOLINTNEXTLINE(cppcoreguidelines-pro-bounds-pointer-arithmetic)
            line[x] = qRgba(red, green, blue, alpha);
        }
    }
    image = straight.convertToFormat(QImage::Format_ARGB32_Premultiplied);
}
} // namespace

TintedSvgImageResponse::TintedSvgImageResponse(QString id, QSize requestedSize)
    : m_id(std::move(id)), m_requestedSize(requestedSize)
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
    static thread_local bool s_decoderNiced = false;
    if (!s_decoderNiced)
    {
        setpriority(PRIO_PROCESS, 0, 10);
        s_decoderNiced = true;
    }

    const QStringList parts = m_id.split(QLatin1Char('/'));
    if (parts.size() < 3)
    {
        m_error = QStringLiteral("malformed tinted-svg id");
        qWarning("tinted-svg provider: malformed id=%s", qUtf8Printable(m_id));
        emit finished();
        return;
    }

    const QColor foreground = colorFromToken(parts.at(0), QColor(Qt::white));
    const QColor background = colorFromToken(parts.at(1), QColor(Qt::black));
    const QString resourcePath = parts.mid(2).join(QLatin1Char('/'));
    if (!resourcePath.startsWith(QStringLiteral("images/systems/")) ||
        !resourcePath.endsWith(QStringLiteral(".svg")))
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

    tintImage(image, foreground, background);
    m_image = image;
    m_factory.reset(QQuickTextureFactory::textureFactoryForImage(m_image));
    emit finished();
}

TintedSvgImageProvider::TintedSvgImageProvider()
{
    m_pool.setMaxThreadCount(4);
}

QQuickImageResponse* TintedSvgImageProvider::requestImageResponse(const QString& id,
                                                                  const QSize& requestedSize)
{
    auto* response = new TintedSvgImageResponse(id, requestedSize);
    m_pool.start(response);
    return response;
}
