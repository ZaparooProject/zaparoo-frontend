// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

#pragma once

#include <QCache>
#include <QImage>
#include <QMutex>
#include <QQuickImageProvider>
#include <QSize>
#include <QString>

// Serves themed bundled artwork as image://tinted-svg/<fg>/<sec>/<bg>/<path>.
//
// This is a *synchronous* QQuickImageProvider on purpose. The async form
// (QQuickAsyncImageProvider) implies QQmlImageProviderBase::ForceAsynchronous-
// ImageLoading, and QQuickPixmap::load() then takes the threaded path for every
// request regardless of Image.asynchronous -- so even a 100% cache hit cost a
// thread-pool dispatch, a mutex, and a queued signal, i.e. at least one frame
// of empty tile. That was the structural cause of tile pop-in, and no cache
// size could fix it. With no flags set, `asynchronous: false` runs inline on
// the GUI thread in the same frame and `asynchronous: true` still goes to the
// shared reader thread, which gives QML per-call-site control.
//
// The work is small enough to do inline because the SVG parse and the
// full-image tone scan happen at build time; see baked_icon_atlas.h. A request
// larger than the baked raster still falls back to rasterizing the SVG.
class TintedSvgImageProvider : public QQuickImageProvider
{
  public:
    TintedSvgImageProvider();
    ~TintedSvgImageProvider() override = default;

    QImage requestImage(const QString& id, QSize* size, const QSize& requestedSize) override;

  private:
    // Tint results are deterministic per (ramp, path, output size), so this
    // absorbs re-tints across screen re-entries and color-scheme changes.
    // Small on purpose: a miss now costs a sub-millisecond LUT pass instead of
    // a 10-20 ms SVG parse, so the cache is a comfort rather than load-bearing.
    mutable QMutex m_cacheMutex;
    mutable QCache<QString, QImage> m_tintedCache;
};
