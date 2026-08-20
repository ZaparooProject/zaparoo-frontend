// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

#include "tint_lut.h"

#include <algorithm>

namespace tint
{
namespace
{
constexpr int kFlatToneSpread = 16;
constexpr int kRampMidpoint = 128;
constexpr int kAlphaFloor = 16;

int channelMix(int a, int b, int amountB)
{
    return ((a * (255 - amountB)) + (b * amountB) + 127) / 255;
}
} // namespace

int lumaOf(QRgb source)
{
    return (((qRed(source) * 299) + (qGreen(source) * 587) + (qBlue(source) * 114)) + 500) / 1000;
}

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
            if (qAlpha(source) <= kAlphaFloor)
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

bool isSingleTone(const ToneRange& range)
{
    return range.pixels == 0 || (range.max - range.min) < kFlatToneSpread;
}

int toneIndex(int luma, const ToneRange& range)
{
    return std::clamp((luma - range.min) * 255 / std::max(1, range.max - range.min), 0, 255);
}

TintLut makeTintLut(const QColor& highlight, const QColor& midtone, const QColor& shadow,
                    bool singleTone)
{
    TintLut lut;

    const int highlightR = highlight.red();
    const int highlightG = highlight.green();
    const int highlightB = highlight.blue();

    if (singleTone)
    {
        const QRgb flat = qRgb(highlightR, highlightG, highlightB);
        lut.tone.fill(flat);
    }
    else
    {
        const int midtoneR = midtone.red();
        const int midtoneG = midtone.green();
        const int midtoneB = midtone.blue();
        const int shadowR = shadow.red();
        const int shadowG = shadow.green();
        const int shadowB = shadow.blue();

        for (int tone = 0; tone < 256; ++tone)
        {
            int red = 0;
            int green = 0;
            int blue = 0;
            if (tone < kRampMidpoint)
            {
                const int amount = tone * 2;
                red = channelMix(shadowR, midtoneR, amount);
                green = channelMix(shadowG, midtoneG, amount);
                blue = channelMix(shadowB, midtoneB, amount);
            }
            else
            {
                const int amount = (tone - kRampMidpoint) * 2;
                red = channelMix(midtoneR, highlightR, amount);
                green = channelMix(midtoneG, highlightG, amount);
                blue = channelMix(midtoneB, highlightB, amount);
            }
            // NOLINTNEXTLINE(cppcoreguidelines-pro-bounds-constant-array-index)
            lut.tone[static_cast<size_t>(tone)] = qRgb(red, green, blue);
        }
    }

    const int tintAlpha = highlight.alpha();
    for (int alpha = 0; alpha < 256; ++alpha)
    {
        const int effective = (alpha * tintAlpha) / 255;
        // NOLINTNEXTLINE(cppcoreguidelines-pro-bounds-constant-array-index)
        lut.flatPremul[static_cast<size_t>(alpha)] =
            qPremultiply(qRgba(highlightR, highlightG, highlightB, effective));
    }

    return lut;
}

void tintImage(QImage& image, const QColor& highlight, const QColor& midtone, const QColor& shadow)
{
    QImage straight = image.convertToFormat(QImage::Format_ARGB32);
    const ToneRange range = toneRangeOf(straight);
    const bool singleTone = isSingleTone(range);
    const TintLut lut = makeTintLut(highlight, midtone, shadow, singleTone);

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
            // NOLINTNEXTLINE(cppcoreguidelines-pro-bounds-constant-array-index)
            const QRgb graded = lut.tone[static_cast<size_t>(toneIndex(lumaOf(source), range))];
            // NOLINTNEXTLINE(cppcoreguidelines-pro-bounds-pointer-arithmetic)
            line[x] = qRgba(qRed(graded), qGreen(graded), qBlue(graded), alpha);
        }
    }
    image = straight.convertToFormat(QImage::Format_ARGB32_Premultiplied);
}

} // namespace tint
