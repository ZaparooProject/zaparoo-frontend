// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

// Bakes the bundled SVG artwork into resources/baked/icons.zbin.
//
// Run it with `just bake-icons`, which drives it inside the pinned lint image.
// QtSvg rasterization is not stable across Qt versions, so a bake from a host
// Qt would fail the strict parity test in CI. The tool is a standalone CMake
// project that the root build never references, so it cannot affect
// `just build`, `just arm32`, or the ARM32 Docker context.
//
// It renders through the same QSvgRenderer configuration and the same
// svgsize::renderSizeFor() the runtime provider uses, then stores the two mask
// planes the runtime tint needs: source alpha, and luma already normalized by
// the image's own tone range. Both toneRangeOf()'s full-image scan and the SVG
// parse therefore disappear from the runtime.
//
// It also bakes ContextMenu's rounded-corner scrim masks (Part 5): for each
// radius 1..16, a `Rectangle { radius: r }` is rendered through Qt Quick's own
// software rasterizer and its inverse alpha stored, so the runtime hole is the
// exact complement of whatever AA profile the live tile's rounded corner paints
// -- no seam to tune. This is why the tool links Qt6::Quick/Qml, not just
// Gui/Svg.

#include "baked_icon_format.h"
#include "svg_render_size.h"
#include "tint_lut.h"

#include <QByteArray>
#include <QCommandLineOption>
#include <QCommandLineParser>
#include <QCryptographicHash>
#include <QDir>
#include <QFile>
#include <QFileInfo>
#include <QGuiApplication>
#include <QImage>
#include <QPainter>
#include <QQmlComponent>
#include <QQmlEngine>
#include <QQuickItem>
#include <QQuickWindow>
#include <QSvgRenderer>
#include <QTextStream>
#include <QUrl>
#include <QtEndian>
#include <algorithm>
#include <memory>
#include <vector>

namespace
{
// Bumped whenever the bake output changes for reasons other than a changed
// source SVG, so a stale manifest is caught by the cheap check script.
constexpr int kToolVersion = 2;

// Corner masks are baked at every integer radius the radius ladder can reach
// (docs/style.md -> "Radius ladder"). Resources.qml::cornerCutUrl() rejects
// anything outside this range.
constexpr int kMinCornerRadius = 1;
constexpr int kMaxCornerRadius = 16;

const QStringList kAssetDirs = {QStringLiteral("systems"), QStringLiteral("categories"),
                                QStringLiteral("icons"), QStringLiteral("status")};

struct Baked
{
    QString path; // "images/systems/SNES.svg", the qrc alias minus the prefix
    QString sourceFile;
    QByteArray sourceSha;
    int width = 0;
    int height = 0;
    int baseWidth = 0;
    int baseHeight = 0;
    bool singleTone = false;
    QByteArray alphaPlane;
    QByteArray tonePlane;
    QImage preview;
};

void appendU16(QByteArray& out, quint16 value)
{
    char buffer[2];
    qToLittleEndian(value, static_cast<void*>(buffer));
    out.append(buffer, 2);
}

void appendU32(QByteArray& out, quint32 value)
{
    char buffer[4];
    qToLittleEndian(value, static_cast<void*>(buffer));
    out.append(buffer, 4);
}

void padTo(QByteArray& out, int alignment)
{
    while ((out.size() % alignment) != 0)
    {
        out.append('\0');
    }
}

// Matches TintedSvgImageResponse::run() exactly: premultiplied target, both
// render hints, then a convert to straight ARGB32 before any per-pixel work.
QImage renderSvg(const QString& file, QSize* outSize, QSize* outBase, QString* error)
{
    QSvgRenderer renderer(file);
    if (!renderer.isValid())
    {
        *error = QStringLiteral("invalid svg");
        return {};
    }
    const QSize defaultSize = renderer.defaultSize();
    *outBase = defaultSize.isValid() ? defaultSize
                                     : QSize(svgsize::kDefaultSvgSize, svgsize::kDefaultSvgSize);
    const QSize target = svgsize::renderSizeFor(renderer, QSize(baked::kBakeWidth, 0));
    QImage image(target, QImage::Format_ARGB32_Premultiplied);
    if (image.isNull())
    {
        *error = QStringLiteral("allocation failed");
        return {};
    }
    image.fill(Qt::transparent);
    QPainter painter(&image);
    painter.setRenderHint(QPainter::Antialiasing, true);
    painter.setRenderHint(QPainter::SmoothPixmapTransform, true);
    renderer.render(&painter);
    painter.end();
    *outSize = target;
    return image.convertToFormat(QImage::Format_ARGB32);
}

bool bakeOne(const QString& file, const QString& resourcePath, Baked* out, QString* error)
{
    QSize size;
    QSize base;
    const QImage straight = renderSvg(file, &size, &base, error);
    if (straight.isNull())
    {
        return false;
    }

    const tint::ToneRange range = tint::toneRangeOf(straight);
    out->path = resourcePath;
    out->sourceFile = file;
    out->width = size.width();
    out->height = size.height();
    out->baseWidth = base.width();
    out->baseHeight = base.height();
    out->singleTone = tint::isSingleTone(range);

    const int pixels = out->width * out->height;
    out->alphaPlane.resize(pixels);
    if (!out->singleTone)
    {
        out->tonePlane.resize(pixels);
    }

    int index = 0;
    for (int y = 0; y < straight.height(); ++y)
    {
        // NOLINTNEXTLINE(cppcoreguidelines-pro-type-reinterpret-cast)
        const auto* line = reinterpret_cast<const QRgb*>(straight.constScanLine(y));
        for (int x = 0; x < straight.width(); ++x, ++index)
        {
            // NOLINTNEXTLINE(cppcoreguidelines-pro-bounds-pointer-arithmetic)
            const QRgb source = line[x];
            out->alphaPlane[index] = static_cast<char>(qAlpha(source));
            if (!out->singleTone)
            {
                out->tonePlane[index] =
                    static_cast<char>(tint::toneIndex(tint::lumaOf(source), range));
            }
        }
    }

    out->preview = straight;

    QFile handle(file);
    if (!handle.open(QIODevice::ReadOnly))
    {
        *error = QStringLiteral("cannot read source");
        return false;
    }
    QCryptographicHash hash(QCryptographicHash::Sha256);
    hash.addData(&handle);
    out->sourceSha = hash.result().toHex();
    return true;
}

// Renders `Rectangle { width: 4*r; height: 4*r; radius: r; color: "white";
// antialiasing: true }` on a transparent background and grabs it.
//
// QQuickRenderControl::initialize() is not usable here: its initRhi() path
// unconditionally builds a real RHI backend regardless of the graphics API
// requested via setGraphicsApi(), so it cannot drive the Software adaptation.
// grabWindow() has its own explicit Software-only path and self-initializes
// the scenegraph, so it is used directly instead of the render-control
// begin/render/end sequence.
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
    component.setData(qml, QUrl(QStringLiteral("qrc:/bake-icons/corner-mask.qml")));
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

// Emits the four corner variants for one radius from a single render: the
// rendered square is geometrically symmetric under reflection, but each
// corner is still cropped from its own quadrant of the actual grab rather
// than derived by mirroring, so an AA asymmetry in the rasterizer (if any)
// cannot hide in an assumption instead of showing up in the baked bytes.
bool bakeCornersForRadius(QQmlEngine& engine, int r, std::vector<Baked>* entries, QString* error)
{
    const QImage grabbed = grabRoundedRectMask(engine, r, error);
    if (grabbed.isNull())
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

    for (const Corner& corner : corners)
    {
        const QImage sub = grabbed.copy(corner.x0, corner.y0, r, r);

        Baked entry;
        entry.path = QStringLiteral("images/corners/cut-%1-%2")
                         .arg(r)
                         .arg(QString::fromLatin1(corner.suffix));
        entry.width = r;
        entry.height = r;
        entry.baseWidth = r;
        entry.baseHeight = r;
        entry.singleTone = true;
        const int pixels = r * r;
        entry.alphaPlane.resize(pixels);

        int index = 0;
        for (int y = 0; y < r; ++y)
        {
            // NOLINTNEXTLINE(cppcoreguidelines-pro-type-reinterpret-cast)
            const auto* line = reinterpret_cast<const QRgb*>(sub.constScanLine(y));
            for (int x = 0; x < r; ++x, ++index)
            {
                // NOLINTNEXTLINE(cppcoreguidelines-pro-bounds-pointer-arithmetic)
                const QRgb source = line[x];
                // The complement, not the tile's own coverage: whatever the
                // live Rectangle paints at this pixel, the mask carries
                // exactly 255 minus that, so tile and scrim coverage sum to
                // 1 by construction.
                entry.alphaPlane[index] = static_cast<char>(255 - qAlpha(source));
            }
        }

        // No source file to hash; hash the baked bytes themselves so a
        // manifest diff still shows when a Qt upgrade changes the rasterizer
        // output at an unchanged tool version.
        QCryptographicHash hash(QCryptographicHash::Sha256);
        hash.addData(entry.alphaPlane);
        entry.sourceSha = hash.result().toHex();
        entry.preview = sub;

        entries->push_back(std::move(entry));
    }
    return true;
}

bool bakeCorners(std::vector<Baked>* entries, QString* error)
{
    QQmlEngine engine;
    for (int r = kMinCornerRadius; r <= kMaxCornerRadius; ++r)
    {
        if (!bakeCornersForRadius(engine, r, entries, error))
        {
            *error = QStringLiteral("radius %1: %2").arg(r).arg(*error);
            return false;
        }
    }
    return true;
}

QByteArray serialize(const std::vector<Baked>& entries, const QByteArray& digestPrefix)
{
    const auto count = static_cast<quint32>(entries.size());

    // Two passes: the directory needs absolute offsets, so lay the string pool
    // and the planes out first and only then emit fixed-size records.
    const int directoryEnd = baked::kHeaderSize + (static_cast<int>(count) * baked::kEntrySize);

    QByteArray pool;
    std::vector<quint32> pathOffsets;
    pathOffsets.reserve(entries.size());
    for (const Baked& entry : entries)
    {
        pathOffsets.push_back(static_cast<quint32>(directoryEnd + pool.size()));
        pool.append(entry.path.toUtf8());
        pool.append('\0');
    }
    int cursor = directoryEnd + static_cast<int>(pool.size());
    const int poolPadding =
        (baked::kPlaneAlignment - (cursor % baked::kPlaneAlignment)) % baked::kPlaneAlignment;
    cursor += poolPadding;

    QByteArray planes;
    std::vector<quint32> alphaOffsets;
    std::vector<quint32> toneOffsets;
    alphaOffsets.reserve(entries.size());
    toneOffsets.reserve(entries.size());
    for (const Baked& entry : entries)
    {
        alphaOffsets.push_back(static_cast<quint32>(cursor + planes.size()));
        planes.append(entry.alphaPlane);
        padTo(planes, baked::kPlaneAlignment);
        if (entry.singleTone)
        {
            toneOffsets.push_back(0);
            continue;
        }
        toneOffsets.push_back(static_cast<quint32>(cursor + planes.size()));
        planes.append(entry.tonePlane);
        padTo(planes, baked::kPlaneAlignment);
    }

    QByteArray blob;
    blob.reserve(cursor + planes.size());
    blob.append(baked::kMagic, sizeof(baked::kMagic));
    appendU32(blob, baked::kVersion);
    appendU32(blob, count);
    appendU32(blob, static_cast<quint32>(directoryEnd));
    appendU32(blob, static_cast<quint32>(cursor + planes.size()));
    blob.append(digestPrefix.constData(), baked::kDigestPrefixSize);

    for (size_t i = 0; i < entries.size(); ++i)
    {
        const Baked& entry = entries[i];
        appendU32(blob, pathOffsets[i]);
        appendU16(blob, static_cast<quint16>(entry.width));
        appendU16(blob, static_cast<quint16>(entry.height));
        appendU16(blob, static_cast<quint16>(entry.baseWidth));
        appendU16(blob, static_cast<quint16>(entry.baseHeight));
        appendU16(blob, entry.singleTone ? baked::kFlagSingleTone : 0);
        appendU16(blob, 0);
        appendU32(blob, alphaOffsets[i]);
        appendU32(blob, toneOffsets[i]);
    }

    blob.append(pool);
    blob.append(QByteArray(poolPadding, '\0'));
    blob.append(planes);
    return blob;
}

// Collects every SVG asset plus the generated corner masks, sorted by path so
// the committed blob and manifest diff reviewably and the bake is
// reproducible regardless of directory iteration order. Returns an exit code
// on failure (2 for a setup problem, 1 for a bake failure) or -1 on success.
int collectEntries(const QString& sourceRoot, std::vector<Baked>* entries, QTextStream* err)
{
    for (const QString& group : kAssetDirs)
    {
        QDir dir(sourceRoot + QLatin1Char('/') + group);
        if (!dir.exists())
        {
            *err << "bake-icons: missing source directory " << dir.path() << '\n';
            return 2;
        }
        const QStringList files = dir.entryList({QStringLiteral("*.svg")}, QDir::Files, QDir::Name);
        for (const QString& name : files)
        {
            Baked entry;
            QString error;
            const QString resourcePath =
                QStringLiteral("images/") + group + QLatin1Char('/') + name;
            if (!bakeOne(dir.filePath(name), resourcePath, &entry, &error))
            {
                *err << "bake-icons: " << resourcePath << ": " << error << '\n';
                return 1;
            }
            entries->push_back(std::move(entry));
        }
    }

    QString cornerError;
    if (!bakeCorners(entries, &cornerError))
    {
        *err << "bake-icons: corner masks: " << cornerError << '\n';
        return 1;
    }

    std::sort(entries->begin(), entries->end(),
              [](const Baked& a, const Baked& b) { return a.path < b.path; });
    return -1;
}

QByteArray buildManifest(const std::vector<Baked>& entries, const QByteArray& sourcesDigest)
{
    QByteArray text;
    QTextStream stream(&text, QIODevice::WriteOnly);
    stream << "# Generated by `just bake-icons`. Do not edit.\n";
    stream << "tool_version " << kToolVersion << "\n";
    stream << "qt_version " << QT_VERSION_STR << "\n";
    stream << "bake_width " << baked::kBakeWidth << "\n";
    stream << "entries " << entries.size() << "\n";
    stream << "sources_digest " << QString::fromLatin1(sourcesDigest) << "\n";
    for (const Baked& entry : entries)
    {
        stream << entry.path << ' ' << QString::fromLatin1(entry.sourceSha) << ' ' << entry.width
               << 'x' << entry.height << ' ' << (entry.singleTone ? "flat" : "ramp") << '\n';
    }
    stream.flush();
    return text;
}
} // namespace

int main(int argc, char** argv)
{
    // Offscreen so no display is required; QtSvg rasterizes through QPainter
    // and never touches the platform plugin's window surface. Corner masks
    // additionally need the Software scenegraph adaptation -- set before
    // QGuiApplication is constructed, which is the only point Qt Quick
    // accepts a graphics API choice.
    qputenv("QT_QPA_PLATFORM", "offscreen");
    qputenv("QT_QUICK_BACKEND", "software");
    QQuickWindow::setGraphicsApi(QSGRendererInterface::Software);
    const QGuiApplication app(argc, argv);

    QCommandLineParser parser;
    parser.setApplicationDescription(
        QStringLiteral("Bake Zaparoo's bundled SVG artwork into a mask atlas."));
    parser.addHelpOption();
    const QCommandLineOption sourceOption(QStringLiteral("source"),
                                          QStringLiteral("resources/images directory"),
                                          QStringLiteral("dir"));
    const QCommandLineOption outOption(QStringLiteral("out"), QStringLiteral("blob to write"),
                                       QStringLiteral("file"));
    const QCommandLineOption manifestOption(
        QStringLiteral("manifest"), QStringLiteral("manifest to write"), QStringLiteral("file"));
    const QCommandLineOption dumpOption(QStringLiteral("dump-png"),
                                        QStringLiteral("also write each raster as a PNG here"),
                                        QStringLiteral("dir"));
    const QCommandLineOption verifyOption(
        QStringLiteral("verify"),
        QStringLiteral("re-bake in memory and diff against the committed outputs"));
    parser.addOption(sourceOption);
    parser.addOption(outOption);
    parser.addOption(manifestOption);
    parser.addOption(dumpOption);
    parser.addOption(verifyOption);
    parser.process(app);

    QTextStream err(stderr);
    if (!parser.isSet(sourceOption) || !parser.isSet(outOption) || !parser.isSet(manifestOption))
    {
        err << "bake-icons: --source, --out and --manifest are required\n";
        return 2;
    }
    const QString sourceRoot = parser.value(sourceOption);
    const QString outPath = parser.value(outOption);
    const QString manifestPath = parser.value(manifestOption);
    const bool verifyOnly = parser.isSet(verifyOption);

    std::vector<Baked> entries;
    const int collectResult = collectEntries(sourceRoot, &entries, &err);
    if (collectResult >= 0)
    {
        return collectResult;
    }

    QCryptographicHash digest(QCryptographicHash::Sha256);
    for (const Baked& entry : entries)
    {
        digest.addData(entry.path.toUtf8());
        digest.addData(entry.sourceSha);
    }
    const QByteArray sourcesDigest = digest.result().toHex();

    const QByteArray blob = serialize(entries, sourcesDigest);
    const QByteArray manifest = buildManifest(entries, sourcesDigest);

    if (verifyOnly)
    {
        QFile existingBlob(outPath);
        QFile existingManifest(manifestPath);
        if (!existingBlob.open(QIODevice::ReadOnly) || !existingManifest.open(QIODevice::ReadOnly))
        {
            err << "bake-icons: --verify needs both outputs to exist; run `just bake-icons`\n";
            return 1;
        }
        const bool blobMatches = existingBlob.readAll() == blob;
        const bool manifestMatches = existingManifest.readAll() == manifest;
        if (!blobMatches || !manifestMatches)
        {
            err << "bake-icons: committed outputs are stale ("
                << (blobMatches ? "manifest" : "blob") << " differs); run `just bake-icons`\n";
            return 1;
        }
        QTextStream(stdout) << "bake-icons: " << entries.size() << " entries verified\n";
        return 0;
    }

    QDir().mkpath(QFileInfo(outPath).absolutePath());
    QFile blobFile(outPath);
    if (!blobFile.open(QIODevice::WriteOnly | QIODevice::Truncate) ||
        blobFile.write(blob) != blob.size())
    {
        err << "bake-icons: cannot write " << outPath << '\n';
        return 1;
    }
    blobFile.close();

    QFile manifestFile(manifestPath);
    if (!manifestFile.open(QIODevice::WriteOnly | QIODevice::Truncate) ||
        manifestFile.write(manifest) != manifest.size())
    {
        err << "bake-icons: cannot write " << manifestPath << '\n';
        return 1;
    }
    manifestFile.close();

    if (parser.isSet(dumpOption))
    {
        const QString dumpRoot = parser.value(dumpOption);
        for (const Baked& entry : entries)
        {
            const QString target =
                dumpRoot + QLatin1Char('/') + entry.path + QStringLiteral(".png");
            QDir().mkpath(QFileInfo(target).absolutePath());
            if (!entry.preview.save(target))
            {
                err << "bake-icons: cannot write preview " << target << '\n';
                return 1;
            }
        }
    }

    QTextStream(stdout) << "bake-icons: " << entries.size() << " entries, " << blob.size()
                        << " bytes -> " << outPath << '\n';
    return 0;
}
