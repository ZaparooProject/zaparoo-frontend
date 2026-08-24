// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// Coverage and byte-exact lock for ContextMenu's baked corner masks (Part 5).
// Structural checks and a provider round-trip run unconditionally; the
// byte-exact "rect.alpha + atlas.mask == 255" comparison against a fresh Qt
// Quick render is gated behind ZAPAROO_BAKE_STRICT=1 for the same reason the
// SVG parity test gates its strict pass: rasterizer output is not guaranteed
// byte-reproducible across Qt versions, and `just test` runs on the host
// while the bake runs inside the pinned lint image.

#include "baked_icon_atlas.h"
#include "baked_icon_format.h"
#include "tinted_svg_image_provider.h"

#include <QByteArray>
#include <QGuiApplication>
#include <QImage>
#include <QQmlComponent>
#include <QQmlEngine>
#include <QQuickItem>
#include <QQuickWindow>
#include <QSize>
#include <QString>
#include <QUrl>
#include <cstdio>
#include <memory>

namespace
{
// Mirrors tools/bake-icons/main.cpp's kMinCornerRadius/kMaxCornerRadius.
// Deliberately independent constants: this test is pinning the contract
// Resources.qml::cornerCutUrl() and ContextMenu.qml depend on, not reading
// the bake tool's own range back at it.
constexpr int kMinCornerRadius = 1;
constexpr int kMaxCornerRadius = 16;
constexpr int kCornerCount = 4;
constexpr const char* kCorners[kCornerCount] = {"tl", "tr", "bl", "br"};
constexpr int kExpectedCornerEntries = (kMaxCornerRadius - kMinCornerRadius + 1) * kCornerCount;

bool check(bool condition, const QString& message)
{
    if (!condition)
    {
        std::fprintf(stderr, "corner mask test failed: %s\n", qUtf8Printable(message));
    }
    return condition;
}

QString entryPath(int r, const char* corner)
{
    return QStringLiteral("images/corners/cut-%1-%2").arg(r).arg(QString::fromLatin1(corner));
}

// White highlight/midtone/shadow with tintAlpha=255 so the singleTone
// flatPremul LUT reduces to an identity map on the atlas's own alpha byte,
// making the provider's output directly comparable to the raw plane.
QString providerRequestId(int r, const char* corner)
{
    return QStringLiteral("ffffff/ffffff/ffffff/") + entryPath(r, corner);
}

bool checkStructure()
{
    bool passed = true;
    qsizetype cornerEntries = 0;
    for (int r = kMinCornerRadius; r <= kMaxCornerRadius; ++r)
    {
        for (const char* corner : kCorners)
        {
            const QString path = entryPath(r, corner);
            const BakedIconAtlas::Entry* entry = BakedIconAtlas::instance().find(path);
            if (!check(entry != nullptr, QStringLiteral("%1 missing from atlas").arg(path)))
            {
                passed = false;
                continue;
            }
            ++cornerEntries;
            passed &= check(entry->singleTone, QStringLiteral("%1 is not single-tone").arg(path));
            passed &= check(entry->width == r && entry->height == r,
                            QStringLiteral("%1 baked at %2x%3, expected %4x%4")
                                .arg(path)
                                .arg(entry->width)
                                .arg(entry->height)
                                .arg(r));
            passed &= check(entry->baseWidth == r && entry->baseHeight == r,
                            QStringLiteral("%1 recorded base size %2x%3, expected %4x%4")
                                .arg(path)
                                .arg(entry->baseWidth)
                                .arg(entry->baseHeight)
                                .arg(r));
        }
    }
    passed &= check(cornerEntries == kExpectedCornerEntries,
                    QStringLiteral("found %1 corner mask entries, expected %2")
                        .arg(cornerEntries)
                        .arg(kExpectedCornerEntries));
    return passed;
}

// The provider's flatPremul LUT at tintAlpha=255 must hand back the atlas's
// own alpha byte unchanged -- the mask math the runtime relies on, checked
// through the real request path rather than reimplemented here.
bool checkProviderRoundTrip(TintedSvgImageProvider& provider)
{
    bool passed = true;
    for (int r = kMinCornerRadius; r <= kMaxCornerRadius; ++r)
    {
        for (const char* corner : kCorners)
        {
            const QString path = entryPath(r, corner);
            const BakedIconAtlas::Entry* entry = BakedIconAtlas::instance().find(path);
            if (entry == nullptr)
            {
                continue; // already reported by checkStructure()
            }
            QSize actualSize;
            const QImage image =
                provider.requestImage(providerRequestId(r, corner), &actualSize, QSize(r, r));
            if (!check(actualSize == QSize(r, r),
                       QStringLiteral("%1 provider returned a wrong size").arg(path)))
            {
                passed = false;
                continue;
            }
            // entry->alpha is a raw view into the blob, padded to
            // baked::paddedStride(r) bytes per row (see baked_icon_format.h),
            // not packed tight at `r`.
            const int stride = baked::paddedStride(r);
            for (int y = 0; y < r; ++y)
            {
                for (int x = 0; x < r; ++x)
                {
                    const int index = (y * stride) + x;
                    // NOLINTNEXTLINE(cppcoreguidelines-pro-bounds-pointer-arithmetic)
                    const quint8 expected = entry->alpha[index];
                    const int actual = qAlpha(image.pixel(x, y));
                    passed &= check(actual == expected, QStringLiteral("%1 pixel (%2,%3) alpha %4, "
                                                                       "atlas byte %5")
                                                            .arg(path)
                                                            .arg(x)
                                                            .arg(y)
                                                            .arg(actual)
                                                            .arg(expected));
                }
            }
        }
    }
    return passed;
}

// Renders `Rectangle { width: 4*r; height: 4*r; radius: r; color: "white";
// antialiasing: true }` through Qt Quick's own software rasterizer, exactly
// as tools/bake-icons/main.cpp::grabRoundedRectMask() does -- this is the
// live-render side of the "rect.alpha + atlas.mask == 255" invariant.
QImage grabRoundedRectMask(QQmlEngine& engine, int r, QString* error)
{
    const int side = 4 * r;
    const QByteArray qml = QByteArrayLiteral("import QtQuick\n"
                                             "Rectangle {\n"
                                             "    width: %1; height: %1; radius: %2\n"
                                             "    color: \"white\"\n"
                                             "    antialiasing: true\n"
                                             "}\n")
                               .replace("%1", QByteArray::number(side))
                               .replace("%2", QByteArray::number(r));

    QQmlComponent component(&engine);
    component.setData(qml, QUrl(QStringLiteral("qrc:/corner-masks-test/corner-mask.qml")));
    if (component.status() != QQmlComponent::Ready)
    {
        QStringList errors;
        for (const auto& e : component.errors())
        {
            errors << e.toString();
        }
        *error =
            QStringLiteral("corner mask component not ready: ") + errors.join(QStringLiteral("; "));
        return {};
    }

    QQuickWindow window;
    window.setColor(Qt::transparent);

    const std::unique_ptr<QObject> rootObject(component.create());
    auto* rootItem = qobject_cast<QQuickItem*>(rootObject.get());
    if (rootItem == nullptr)
    {
        *error = QStringLiteral("corner mask root is not an Item");
        return {};
    }
    rootItem->setParentItem(window.contentItem());
    window.resize(side, side);
    window.show();

    const QImage grabbed = window.grabWindow();
    if (grabbed.width() != side || grabbed.height() != side)
    {
        *error = QStringLiteral("corner mask grab returned %1x%2, expected %3x%3")
                     .arg(grabbed.width())
                     .arg(grabbed.height())
                     .arg(side);
        return {};
    }
    return grabbed.convertToFormat(QImage::Format_ARGB32);
}

bool checkStrictParityForRadius(QQmlEngine& engine, int r)
{
    QString error;
    const QImage grabbed = grabRoundedRectMask(engine, r, &error);
    if (!check(!grabbed.isNull(), QStringLiteral("radius %1: %2").arg(r).arg(error)))
    {
        return false;
    }

    const int side = 4 * r;
    struct Corner
    {
        const char* suffix;
        int x0;
        int y0;
    };
    const Corner corners[] = {
        {"tl", 0, 0},
        {"tr", side - r, 0},
        {"bl", 0, side - r},
        {"br", side - r, side - r},
    };

    bool passed = true;
    for (const Corner& corner : corners)
    {
        const QString path = entryPath(r, corner.suffix);
        const BakedIconAtlas::Entry* entry = BakedIconAtlas::instance().find(path);
        if (entry == nullptr)
        {
            continue; // already reported by checkStructure()
        }
        const QImage sub = grabbed.copy(corner.x0, corner.y0, r, r);
        const int stride = baked::paddedStride(r);
        for (int y = 0; y < r; ++y)
        {
            // NOLINTNEXTLINE(cppcoreguidelines-pro-type-reinterpret-cast)
            const auto* line = reinterpret_cast<const QRgb*>(sub.constScanLine(y));
            for (int x = 0; x < r; ++x)
            {
                const int index = (y * stride) + x;
                // NOLINTNEXTLINE(cppcoreguidelines-pro-bounds-pointer-arithmetic)
                const auto expected = static_cast<quint8>(255 - qAlpha(line[x]));
                // NOLINTNEXTLINE(cppcoreguidelines-pro-bounds-pointer-arithmetic)
                const quint8 actual = entry->alpha[index];
                passed &= check(actual == expected,
                                QStringLiteral("%1 pixel (%2,%3): atlas byte %4, fresh render "
                                               "complement %5")
                                    .arg(path)
                                    .arg(x)
                                    .arg(y)
                                    .arg(actual)
                                    .arg(expected));
            }
        }
    }
    return passed;
}

bool checkStrictParity()
{
    QQmlEngine engine;
    bool passed = true;
    for (int r = kMinCornerRadius; r <= kMaxCornerRadius; ++r)
    {
        passed &= checkStrictParityForRadius(engine, r);
    }
    return passed;
}
} // namespace

int main(int argc, char** argv)
{
    qputenv("QT_QPA_PLATFORM", "offscreen");
    qputenv("QT_QUICK_BACKEND", "software");
    QQuickWindow::setGraphicsApi(QSGRendererInterface::Software);
    const QGuiApplication app(argc, argv);

    bool passed =
        check(BakedIconAtlas::instance().isValid(), QStringLiteral("baked atlas failed to parse"));
    passed &= checkStructure();

    TintedSvgImageProvider provider;
    passed &= checkProviderRoundTrip(provider);

    const bool strict = qEnvironmentVariable("ZAPAROO_BAKE_STRICT") == QStringLiteral("1");
    if (strict)
    {
        passed &= checkStrictParity();
    }
    else
    {
        std::fprintf(stderr, "note: byte-exact fresh-render parity skipped; set "
                             "ZAPAROO_BAKE_STRICT=1 inside the pinned Qt image to run it\n");
    }

    return passed ? 0 : 1;
}
