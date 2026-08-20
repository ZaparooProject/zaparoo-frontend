// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

// Byte-exact parity lock on the tritone tint.
//
// `reference::tintImage()` below is the algorithm as it stood in
// tinted_svg_image_provider.cpp before the LUT extraction, pasted verbatim and
// deliberately never refactored. Every bundled logo's appearance is defined by
// it, so any future change to tint_lut.cpp that alters a single byte fails
// here. No QtSvg is involved, so the comparison is deterministic on every host.

#include "tint_lut.h"

#include <QColor>
#include <QGuiApplication>
#include <QImage>
#include <QRgb>
#include <algorithm>
#include <array>
#include <cstdio>
#include <vector>

namespace reference
{
namespace
{
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
} // namespace reference

namespace
{
bool check(bool condition, const char* message)
{
    if (!condition)
    {
        std::fprintf(stderr, "tint lut test failed: %s\n", message);
    }
    return condition;
}

// Every luma value crossed with a spread of alphas, so the fixture exercises
// all 256 ramp entries and the premultiply rounding at once.
QImage gradientFixture(int lumaSpread)
{
    QImage image(256, 256, QImage::Format_ARGB32_Premultiplied);
    for (int y = 0; y < image.height(); ++y)
    {
        // NOLINTNEXTLINE(cppcoreguidelines-pro-type-reinterpret-cast)
        auto* line = reinterpret_cast<QRgb*>(image.scanLine(y));
        const int alpha = y;
        for (int x = 0; x < image.width(); ++x)
        {
            // Unequal channels so the Rec.601 weighting actually matters.
            const int base = (x * lumaSpread) / 255;
            const int red = std::clamp(base + 3, 0, 255);
            const int green = std::clamp(base, 0, 255);
            const int blue = std::clamp(base - 3, 0, 255);
            // NOLINTNEXTLINE(cppcoreguidelines-pro-bounds-pointer-arithmetic)
            line[x] = qPremultiply(qRgba(red, green, blue, alpha));
        }
    }
    return image;
}

bool identical(const QImage& a, const QImage& b)
{
    if (a.size() != b.size() || a.format() != b.format())
    {
        return false;
    }
    for (int y = 0; y < a.height(); ++y)
    {
        // NOLINTNEXTLINE(cppcoreguidelines-pro-type-reinterpret-cast)
        const auto* lineA = reinterpret_cast<const QRgb*>(a.constScanLine(y));
        // NOLINTNEXTLINE(cppcoreguidelines-pro-type-reinterpret-cast)
        const auto* lineB = reinterpret_cast<const QRgb*>(b.constScanLine(y));
        for (int x = 0; x < a.width(); ++x)
        {
            // NOLINTNEXTLINE(cppcoreguidelines-pro-bounds-pointer-arithmetic)
            if (lineA[x] != lineB[x])
            {
                // NOLINTBEGIN(cppcoreguidelines-pro-bounds-pointer-arithmetic)
                std::fprintf(stderr, "  first mismatch at (%d,%d): %08x vs %08x\n", x, y, lineA[x],
                             lineB[x]);
                // NOLINTEND(cppcoreguidelines-pro-bounds-pointer-arithmetic)
                return false;
            }
        }
    }
    return true;
}

struct Ramp
{
    const char* name;
    QRgb highlight;
    QRgb midtone;
    QRgb shadow;
};

// The resting and focused ramps the three shipped presets derive, plus a flat
// white ramp and an amber one, so the parity check covers real inputs.
const std::array<Ramp, 8> kRamps = {{
    {"zaparoo-black resting", 0xffadb2b6, 0xff697581, 0xff27415c},
    {"zaparoo-black focused", 0xffe5eef6, 0xff168bff, 0xff0e5aa6},
    {"midnight-amber resting", 0xffbebbbd, 0xff888180, 0xff625146},
    {"midnight-amber focused", 0xfffff9f0, 0xffffb347, 0xffa6742e},
    {"zaparoo-white resting", 0xff4d535a, 0xff7f8a97, 0xffa3b7d0},
    {"zaparoo-white focused", 0xff101a26, 0xff0a63c9, 0xffa2c4ea},
    {"flat white", 0xffffffff, 0xffffffff, 0xffffffff},
    {"black shadow", 0xffffffff, 0xff808080, 0xff000000},
}};

// 15 is below the flat-tone threshold, 16 is the first multi-tone spread, 17 is
// safely above it. A shifted boundary would silently re-grade every near-flat
// logo, so pin all three.
const std::array<int, 5> kSpreads = {{0, 15, 16, 17, 255}};

bool testParity()
{
    bool passed = true;
    for (const Ramp& ramp : kRamps)
    {
        for (int spread : kSpreads)
        {
            const QImage fixture = gradientFixture(spread);
            QImage expected = fixture;
            reference::tintImage(expected, QColor::fromRgb(ramp.highlight),
                                 QColor::fromRgb(ramp.midtone), QColor::fromRgb(ramp.shadow));
            QImage actual = fixture;
            tint::tintImage(actual, QColor::fromRgb(ramp.highlight), QColor::fromRgb(ramp.midtone),
                            QColor::fromRgb(ramp.shadow));
            char message[160];
            std::snprintf(message, sizeof(message), "%s at spread %d is not byte-exact", ramp.name,
                          spread);
            passed &= check(identical(expected, actual), message);
        }
    }
    return passed;
}

// The single-tone early-out must be an optimization, not a second code path:
// every `tone` entry equals the highlight, so a caller that ignores the flag
// still gets the same answer.
bool testSingleToneCollapses()
{
    const QColor highlight = QColor::fromRgb(0xff168bff);
    const tint::TintLut lut = tint::makeTintLut(highlight, QColor::fromRgb(0xff7c8794),
                                                QColor::fromRgb(0xff2c3542), true);
    bool flat = true;
    for (QRgb entry : lut.tone)
    {
        flat = flat && entry == qRgb(highlight.red(), highlight.green(), highlight.blue());
    }
    return check(flat, "single-tone ramp must be flat highlight across all 256 entries");
}

// Part 5's ContextMenu corner masks are single-tone entries tinted with
// Theme.scrim, which is #cc000000. If flatPremul stops honoring the tint's own
// alpha the scrim corners paint opaque black.
bool testFlatPremulHonorsTintAlpha()
{
    const QColor scrim = QColor::fromRgba(0xcc000000);
    const tint::TintLut lut = tint::makeTintLut(scrim, scrim, scrim, true);
    bool passed =
        check(qAlpha(lut.flatPremul[0]) == 0, "zero coverage must stay fully transparent");
    passed &=
        check(qAlpha(lut.flatPremul[255]) == 0xcc, "full coverage must carry the tint's own alpha");
    passed &= check(qAlpha(lut.flatPremul[128]) == (128 * 0xcc) / 255,
                    "partial coverage scales the alpha");

    const QColor opaque = QColor::fromRgb(0xff168bff);
    const tint::TintLut opaqueLut = tint::makeTintLut(opaque, opaque, opaque, true);
    bool passthrough = true;
    for (int alpha = 0; alpha < 256; ++alpha)
    {
        // NOLINTNEXTLINE(cppcoreguidelines-pro-bounds-constant-array-index)
        passthrough = passthrough && qAlpha(opaqueLut.flatPremul[alpha]) == alpha;
    }
    passed &= check(passthrough, "an opaque tint must leave source alpha untouched");
    return passed;
}

// toneRangeOf ignores near-transparent pixels; a fixture of nothing but those
// must report zero pixels and be treated as flat rather than dividing by zero.
bool testEmptyToneRange()
{
    QImage image(4, 4, QImage::Format_ARGB32);
    image.fill(QColor::fromRgba(0x10ffffff));
    const tint::ToneRange range = tint::toneRangeOf(image);
    bool passed =
        check(range.pixels == 0, "alpha at the 16 floor must not count toward the tone range");
    passed &= check(tint::isSingleTone(range), "an empty tone range must be single-tone");
    return passed;
}
} // namespace

int main(int argc, char** argv)
{
    // QImage format conversion picks SIMD paths at runtime, so go through a
    // real QGuiApplication rather than assuming the scalar fallback.
    const QGuiApplication app(argc, argv);
    bool passed = testParity();
    passed &= testSingleToneCollapses();
    passed &= testFlatPremulHonorsTintAlpha();
    passed &= testEmptyToneRange();
    return passed ? 0 : 1;
}
