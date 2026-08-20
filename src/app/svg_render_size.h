// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

#pragma once

#include <QSize>
#include <QSvgRenderer>
#include <algorithm>

// Shared by the image provider and the bake tool. It lives in a header rather
// than being duplicated because a divergence here means the baked raster is a
// different size than the one the runtime asks for, which shows up as a
// silently blurry logo rather than as a build failure.
namespace svgsize
{

constexpr int kDefaultSvgSize = 256;

inline QSize renderSizeFor(const QSvgRenderer& renderer, const QSize& requestedSize)
{
    const QSize defaultSize = renderer.defaultSize();
    const QSize base =
        defaultSize.isValid() ? defaultSize : QSize(kDefaultSvgSize, kDefaultSvgSize);
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

} // namespace svgsize
