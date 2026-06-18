// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

#include "custom_image_provider.h"

#include <QFile>
#include <QImage>
#include <QImageReader>
#include <QPainter>
#include <QSize>
#include <QString>
#include <QSvgRenderer>
#include <algorithm>
#include <cstddef>
#include <cstdint>
#include <string_view>

// Rust FFI: check that an absolute path is inside the customization root.
// Returns false if the root is unset (feature off) or the path escapes.
extern "C" bool zaparoo_override_image_is_in_override_dir(const uint8_t* path, size_t len);

namespace
{
constexpr int kDefaultSvgSize = 256;

constexpr std::string_view kAllowedExtensions[] = {"png", "jpg", "jpeg", "webp", "bmp", "svg"};

/// Return only the final path component so logs don't expose the full
/// customization directory (which may contain a username or other private
/// directory names).
QString logSafeImageName(const QString& path)
{
    const qsizetype sep = path.lastIndexOf(QLatin1Char('/'));
    return sep >= 0 ? path.mid(sep + 1) : path;
}

bool isAllowedExtension(const QString& ext)
{
    const std::string extStd = ext.toLower().toStdString();
    return std::any_of(std::begin(kAllowedExtensions), std::end(kAllowedExtensions),
                       [&](std::string_view a) { return extStd == a; });
}

/// Validate that `path` is inside the Rust-managed customization root.
/// Prevents the image URL from being manipulated to read arbitrary files.
bool isValidOverridePath(const QString& path)
{
    const QByteArray pathBytes = path.toUtf8();
    return zaparoo_override_image_is_in_override_dir(
        // NOLINTNEXTLINE(cppcoreguidelines-pro-type-reinterpret-cast)
        reinterpret_cast<const uint8_t*>(pathBytes.constData()),
        static_cast<size_t>(pathBytes.size()));
}

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
} // namespace

CustomImageProvider::CustomImageProvider() : QQuickImageProvider(QQuickImageProvider::Image) {}

QImage CustomImageProvider::requestImage(const QString& id, QSize* size, const QSize& requestedSize)
{
    // `id` is the absolute path to the override file, passed verbatim from the
    // `custom-image/` cover key emitted by `image_overrides::override_path`.

    // Security: validate extension before touching the filesystem.
    const qsizetype dotPosQ = id.lastIndexOf(QLatin1Char('.'));
    if (dotPosQ < 0 || !isAllowedExtension(id.mid(dotPosQ + 1)))
    {
        qWarning("custom-image provider: rejected extension in id=%s",
                 qUtf8Printable(logSafeImageName(id)));
        return {};
    }

    // Security: validate that the path is inside the customization root.
    if (!isValidOverridePath(id))
    {
        qWarning("custom-image provider: path not in customization root, id=%s",
                 qUtf8Printable(logSafeImageName(id)));
        return {};
    }

    if (!QFile::exists(id))
    {
        qWarning("custom-image provider: file not found, id=%s",
                 qUtf8Printable(logSafeImageName(id)));
        return {};
    }

    const QString ext = id.mid(dotPosQ + 1).toLower();
    QImage image;

    if (ext == QStringLiteral("svg"))
    {
        QSvgRenderer renderer(id);
        if (!renderer.isValid())
        {
            qWarning("custom-image provider: invalid SVG, id=%s",
                     qUtf8Printable(logSafeImageName(id)));
            return {};
        }
        const QSize targetSize = renderSizeFor(renderer, requestedSize);
        image = QImage(targetSize, QImage::Format_ARGB32_Premultiplied);
        image.fill(Qt::transparent);
        QPainter painter(&image);
        painter.setRenderHint(QPainter::Antialiasing, true);
        painter.setRenderHint(QPainter::SmoothPixmapTransform, true);
        renderer.render(&painter);
        painter.end();
    }
    else
    {
        QImageReader reader(id);
        // Scale during decode to avoid allocating a full-resolution buffer
        // for high-resolution user overrides on memory-constrained devices.
        const int reqW = requestedSize.width();
        const int reqH = requestedSize.height();
        if (reqW > 0 || reqH > 0)
        {
            const QSize nativeSize = reader.size();
            if (nativeSize.isValid())
            {
                const int targetW = reqW > 0 ? reqW : nativeSize.width();
                const int targetH = reqH > 0 ? reqH : nativeSize.height();
                reader.setScaledSize(
                    nativeSize.scaled(QSize(targetW, targetH), Qt::KeepAspectRatio));
            }
        }
        image = reader.read();
        if (image.isNull())
        {
            qWarning("custom-image provider: could not load image, id=%s",
                     qUtf8Printable(logSafeImageName(id)));
            return {};
        }
        image = image.convertToFormat(QImage::Format_ARGB32_Premultiplied);
    }

    if (size != nullptr)
    {
        *size = image.size();
    }
    return image;
}
