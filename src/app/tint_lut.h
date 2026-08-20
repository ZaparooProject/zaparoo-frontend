// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

#pragma once

#include <QColor>
#include <QImage>
#include <QRgb>
#include <array>

// The tritone gradient map that gives bundled artwork its themed look, lifted
// out of the image provider so the bake tool and the tests can use it without
// dragging in QtSvg.
//
// The map is a color-grade, not a semantic recolor. A source image's luma is
// contrast-stretched to the full 0-255 range and then lerped continuously
// through shadow -> midtone -> highlight. Antialiased glyph boundaries land at
// mid-luma and pick up the midtone, which is what reads as a rim light on a
// tinted logo. See docs/style.md -> "Pressable front edge" for why that matters.

namespace tint
{

// Rec.601 luma, matching the weights the provider has always used. Not the
// Rec.709 relative luminance the color-scheme guardrails use; changing it here
// would re-grade every bundled logo.
[[nodiscard]] int lumaOf(QRgb source);

// Luma extent of an image's non-transparent pixels. `pixels` counts only those
// pixels, so a fully transparent image reports zero and is treated as flat.
struct ToneRange
{
    int min = 255;
    int max = 0;
    int pixels = 0;
};

// Expects QImage::Format_ARGB32 (straight alpha).
[[nodiscard]] ToneRange toneRangeOf(const QImage& image);

// A logo whose luma barely varies has no gradient to map, so it takes the
// highlight color flat. This also skips the ramp for the ~38 single-color
// system logos.
[[nodiscard]] bool isSingleTone(const ToneRange& range);

// Normalize a pixel's luma into the 0-255 index `TintLut::tone` expects.
[[nodiscard]] int toneIndex(int luma, const ToneRange& range);

struct TintLut
{
    // Straight RGB (alpha bits unused), indexed by normalized tone.
    std::array<QRgb, 256> tone{};
    // Premultiplied ARGB for the single-tone path, indexed by source alpha.
    std::array<QRgb, 256> flatPremul{};
};

// Builds both tables. When `singleTone` is true every `tone` entry is the
// highlight color, so the fast path and the ramp path are provably the same
// result rather than merely similar.
//
// Alpha asymmetry, deliberate: `flatPremul` folds `highlight`'s own alpha into
// the output, so a translucent tint color works on the single-tone path. The
// `tone` table carries no alpha at all — the caller supplies it per pixel — so
// the multi-tone path ignores the tint colors' alpha. Every artwork caller
// passes opaque colors; the ContextMenu scrim corner masks are single-tone and
// rely on the alpha being honored.
[[nodiscard]] TintLut makeTintLut(const QColor& highlight, const QColor& midtone,
                                  const QColor& shadow, bool singleTone);

// In-place tritone tint of an image of any format. Output is
// QImage::Format_ARGB32_Premultiplied.
void tintImage(QImage& image, const QColor& highlight, const QColor& midtone, const QColor& shadow);

} // namespace tint
