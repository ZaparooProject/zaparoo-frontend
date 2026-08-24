// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// Parity lock between the committed mask atlas and the SVG path it replaced.
// The atlas side goes through the production image provider, not a copy of it;
// the SVG side is reimplemented here on purpose, so the test fails if the
// provider and the original algorithm ever disagree.

#include "baked_icon_atlas.h"
#include "baked_icon_format.h"
#include "svg_render_size.h"
#include "tint_lut.h"
#include "tinted_svg_image_provider.h"

#include <QDirIterator>
#include <QGuiApplication>
#include <QImage>
#include <QList>
#include <QPainter>
#include <QResource>
#include <QSize>
#include <QString>
#include <QStringList>
#include <QSvgRenderer>
#include <cstdio>

namespace
{
constexpr auto kQrcRoot = ":/qt/qml/Zaparoo/App/resources/";
constexpr int kSignatureSide = 16;
// A coarse signature tolerates a stray antialiased pixel from a different Qt
// version while still catching a wrong asset, a wrong ramp, or a stale bake.
constexpr int kSignatureTolerance = 2;

bool check(bool condition, const QString& message)
{
    if (!condition)
    {
        std::fprintf(stderr, "baked icon atlas test failed: %s\n", qUtf8Printable(message));
    }
    return condition;
}

// zaparoo-black's focused ramp -- the widest of the shipped ramps, so it is the
// one most sensitive to a tone plane that is off by a step.
QString rampId(const QString& resourcePath)
{
    return QStringLiteral("e5eef6/168bff/0e5aa6/") + resourcePath;
}

QColor rampHighlight()
{
    return {QStringLiteral("#e5eef6")};
}

QColor rampMidtone()
{
    return {QStringLiteral("#168bff")};
}

QColor rampShadow()
{
    return {QStringLiteral("#0e5aa6")};
}

QStringList bakedSourcePaths()
{
    QStringList paths;
    for (const auto* group : {"systems", "categories", "icons", "status"})
    {
        const QString dir =
            QString::fromLatin1(kQrcRoot) + QStringLiteral("images/") + QString::fromLatin1(group);
        QDirIterator iterator(dir, {QStringLiteral("*.svg")}, QDir::Files);
        while (iterator.hasNext())
        {
            paths.append(iterator.next().mid(static_cast<int>(qstrlen(kQrcRoot))));
        }
    }
    paths.sort();
    return paths;
}

QImage rasterize(QSvgRenderer& renderer, const QSize& size)
{
    QImage image(size, QImage::Format_ARGB32_Premultiplied);
    image.fill(Qt::transparent);
    QPainter painter(&image);
    painter.setRenderHint(QPainter::Antialiasing, true);
    painter.setRenderHint(QPainter::SmoothPixmapTransform, true);
    renderer.render(&painter);
    painter.end();
    return image;
}

// The pre-atlas render path, verbatim: rasterize, then tritone the result.
QImage renderReference(const QString& resourcePath, const QSize& requestedSize)
{
    QSvgRenderer renderer(QString::fromLatin1(kQrcRoot) + resourcePath);
    if (!renderer.isValid())
    {
        return {};
    }
    QImage image = rasterize(renderer, svgsize::renderSizeFor(renderer, requestedSize));
    tint::tintImage(image, rampHighlight(), rampMidtone(), rampShadow());
    return image;
}

QList<int> lumaSignature(const QImage& image)
{
    QList<int> signature;
    signature.reserve(static_cast<qsizetype>(kSignatureSide) * kSignatureSide);
    const QImage small =
        image
            .scaled(kSignatureSide, kSignatureSide, Qt::IgnoreAspectRatio, Qt::SmoothTransformation)
            .convertToFormat(QImage::Format_ARGB32);
    for (int y = 0; y < small.height(); ++y)
    {
        for (int x = 0; x < small.width(); ++x)
        {
            const QRgb pixel = small.pixel(x, y);
            signature.append((tint::lumaOf(pixel) * qAlpha(pixel)) / 255);
        }
    }
    return signature;
}

// The highest-value assertion in this file. If rcc starts compressing the blob
// everything still renders correctly and 6 MB silently moves from a clean,
// file-backed, evictable mapping onto the heap -- on MiSTer that is the whole
// point of the change, undone with no visible symptom.
bool checkAtlasIsUncompressed()
{
    const QResource resource(QString::fromLatin1(baked::kResourcePath));
    bool passed = check(resource.isValid(), QStringLiteral("baked atlas missing from the qrc"));
    if (!passed)
    {
        return false;
    }
    passed &= check(resource.compressionAlgorithm() == QResource::NoCompression,
                    QStringLiteral("baked atlas is compressed in the qrc"));
    passed &=
        check(BakedIconAtlas::instance().isValid(), QStringLiteral("baked atlas failed to parse"));
    passed &= check(BakedIconAtlas::instance().byteSize() == resource.size(),
                    QStringLiteral("baked atlas size disagrees with the resource"));
    return passed;
}

// Corner masks (Part 5) share the atlas but have no SVG source, so they are
// not part of `sources` -- their own count and byte-exactness are covered by
// tst_corner_masks.cpp. This just needs the fixed count of that pinned range
// so an accidental extra or missing entry still fails here too.
constexpr int kMinCornerRadius = 1;
constexpr int kMaxCornerRadius = 16;
constexpr int kCornerVariants = 4;
constexpr int kCornerMaskCount = (kMaxCornerRadius - kMinCornerRadius + 1) * kCornerVariants;

bool checkCoverage(const QStringList& sources)
{
    bool passed = check(!sources.isEmpty(), QStringLiteral("no bundled SVGs found in the qrc"));
    passed &= check(BakedIconAtlas::instance().entryCount() == sources.size() + kCornerMaskCount,
                    QStringLiteral("atlas has %1 entries for %2 SVGs + %3 corner masks")
                        .arg(BakedIconAtlas::instance().entryCount())
                        .arg(sources.size())
                        .arg(kCornerMaskCount));
    for (const QString& path : sources)
    {
        passed &= check(BakedIconAtlas::instance().find(path) != nullptr,
                        QStringLiteral("%1 has no atlas entry").arg(path));
    }
    return passed;
}

bool checkStructuralParity(const QString& path)
{
    const BakedIconAtlas::Entry* entry = BakedIconAtlas::instance().find(path);
    if (entry == nullptr)
    {
        return false;
    }
    QSvgRenderer renderer(QString::fromLatin1(kQrcRoot) + path);
    if (!check(renderer.isValid(), QStringLiteral("%1 is not a valid SVG").arg(path)))
    {
        return false;
    }

    const QSize expected = svgsize::renderSizeFor(renderer, QSize(baked::kBakeWidth, 0));
    bool passed = check(QSize(entry->width, entry->height) == expected,
                        QStringLiteral("%1 baked at %2x%3, provider asks for %4x%5")
                            .arg(path)
                            .arg(entry->width)
                            .arg(entry->height)
                            .arg(expected.width())
                            .arg(expected.height()));
    passed &= check(QSize(entry->baseWidth, entry->baseHeight) == renderer.defaultSize(),
                    QStringLiteral("%1 recorded the wrong SVG default size").arg(path));

    // Recompute the tone class from a fresh raster: a bake that used a
    // different threshold would render flat where the runtime expects a ramp.
    const QImage straight = rasterize(renderer, expected).convertToFormat(QImage::Format_ARGB32);
    const bool singleTone = tint::isSingleTone(tint::toneRangeOf(straight));
    passed &= check(entry->singleTone == singleTone,
                    QStringLiteral("%1 baked tone class disagrees with a fresh raster").arg(path));
    passed &= check((entry->tone == nullptr) == entry->singleTone,
                    QStringLiteral("%1 tone plane presence disagrees with its flag").arg(path));
    return passed;
}

bool checkPerceptualParity(TintedSvgImageProvider& provider, const QString& path)
{
    const BakedIconAtlas::Entry* entry = BakedIconAtlas::instance().find(path);
    if (entry == nullptr)
    {
        return false;
    }
    const QSize bakedSize(entry->width, entry->height);
    QSize actualSize;
    const QImage fromAtlas = provider.requestImage(rampId(path), &actualSize, bakedSize);
    bool passed = check(actualSize == bakedSize,
                        QStringLiteral("%1 provider returned a wrong size").arg(path));

    const QList<int> atlasSignature = lumaSignature(fromAtlas);
    const QList<int> svgSignature = lumaSignature(renderReference(path, bakedSize));
    if (!check(atlasSignature.size() == svgSignature.size(),
               QStringLiteral("%1 signature sizes differ").arg(path)))
    {
        return false;
    }
    for (qsizetype i = 0; i < atlasSignature.size(); ++i)
    {
        const int delta = qAbs(atlasSignature.at(i) - svgSignature.at(i));
        passed &= check(delta <= kSignatureTolerance,
                        QStringLiteral("%1 cell %2 differs by %3").arg(path).arg(i).arg(delta));
    }
    return passed;
}

// QtSvg rasterization is not guaranteed byte-reproducible across Qt versions,
// and the bake runs inside the pinned lint image while `just test` runs on the
// host. CI runs this variant inside the pinned image.
bool checkStrictParity(TintedSvgImageProvider& provider, const QString& path)
{
    const BakedIconAtlas::Entry* entry = BakedIconAtlas::instance().find(path);
    if (entry == nullptr)
    {
        return false;
    }
    const QSize bakedSize(entry->width, entry->height);
    QSize actualSize;
    const QImage fromAtlas = provider.requestImage(rampId(path), &actualSize, bakedSize);
    return check(fromAtlas == renderReference(path, bakedSize),
                 QStringLiteral("%1 is not byte-exact against a fresh SVG render").arg(path));
}

// BrowseDetailPane asks for 512/768, above the baked raster, and must still get
// a natively rasterized image rather than an upscaled mask.
bool checkOversizedRequestFallsBackToSvg(TintedSvgImageProvider& provider)
{
    const QString path = QStringLiteral("images/categories/Arcade.svg");
    bool passed = check(BakedIconAtlas::instance().find(path) != nullptr,
                        QStringLiteral("fallback fixture is missing from the atlas"));
    QSize actualSize;
    const QImage image = provider.requestImage(rampId(path), &actualSize, QSize(512, 512));
    passed &= check(actualSize == QSize(512, 512),
                    QStringLiteral("oversized request did not return the requested size"));
    passed &= check(image == renderReference(path, QSize(512, 512)),
                    QStringLiteral("oversized request did not take the SVG path"));
    return passed;
}
} // namespace

int main(int argc, char** argv)
{
    qputenv("QT_QPA_PLATFORM", "offscreen");
    const QGuiApplication app(argc, argv);

    bool passed = checkAtlasIsUncompressed();
    const QStringList sources = bakedSourcePaths();
    passed &= checkCoverage(sources);

    TintedSvgImageProvider provider;
    const bool strict = qEnvironmentVariable("ZAPAROO_BAKE_STRICT") == QStringLiteral("1");
    for (const QString& path : sources)
    {
        passed &= checkStructuralParity(path);
        passed &= checkPerceptualParity(provider, path);
        if (strict)
        {
            passed &= checkStrictParity(provider, path);
        }
    }
    passed &= checkOversizedRequestFallsBackToSvg(provider);

    if (!strict)
    {
        std::fprintf(stderr, "note: byte-exact parity skipped; set ZAPAROO_BAKE_STRICT=1 inside "
                             "the pinned Qt image to run it\n");
    }
    return passed ? 0 : 1;
}
