// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// QQuickAsyncImageProvider that serves media image bytes (boxart,
// screenshot, wheel, titleshot, map, marquee, fanart, generic image —
// anything Core returns from `media.image`) from the Rust-side in-memory
// cache (`media_image_cache.rs`). QML loads `image://media-image/<key>`
// URLs; QtQuick calls `requestImageResponse` with `<key>` (the bit after
// the scheme + host); we hand the encoded key to the Rust C ABI which
// looks the bytes up in the LRU cache and copies them into a QByteArray.
// Empty bytes → null QImage, and Tile.qml's fallback text stays visible.
//
// Async because the synchronous predecessor blocked the Qt thread on
// every `requestImage` call: a freshly-loaded folder of 10 covers
// produced ~5 s of serial decode (~250–700 ms each) during which the
// page rendered with placeholders and tiles popped in one at a time as
// each lookup finally returned. Moving the FFI lookup, `loadFromData`,
// and scaling onto a `QThreadPool` parallelises decode (4 workers) and
// keeps the Qt thread free to layout and paint while images settle.

#pragma once

#include <QImage>
#include <QQuickAsyncImageProvider>
#include <QQuickImageResponse>
#include <QQuickTextureFactory>
#include <QRunnable>
#include <QSize>
#include <QString>
#include <QThreadPool>
#include <memory>

class MediaImageResponse : public QQuickImageResponse, public QRunnable
{
  public:
    MediaImageResponse(QString id, QSize requestedSize);
    ~MediaImageResponse() override = default;

    [[nodiscard]] QQuickTextureFactory* textureFactory() const override;
    void run() override;

  private:
    QString m_id;
    QSize m_requestedSize;
    QImage m_image;
    // Built on the worker thread once decode completes so the GUI
    // thread doesn't pay the QQuickTextureFactory allocation cost
    // when QtQuick consumes the response. `mutable` because the
    // base-class signature `textureFactory() const` transfers the
    // pointer to QtQuick (which will own and destroy it), so the
    // const method needs to release ownership of the cached unique_ptr.
    mutable std::unique_ptr<QQuickTextureFactory> m_factory;
};

class MediaImageProvider : public QQuickAsyncImageProvider
{
  public:
    MediaImageProvider();
    ~MediaImageProvider() override = default;

    QQuickImageResponse* requestImageResponse(const QString& id,
                                              const QSize& requestedSize) override;

  private:
    // Bounded so a fast-scrolling user enqueueing 30+ tiles doesn't
    // spawn dozens of decode threads on MiSTer's two ARM cores. Four
    // workers is the same cap `MediaImageCache`'s fetch driver uses;
    // beyond that, context-switch cost outweighs parallel decode on
    // the software-rendered build.
    QThreadPool m_pool;
};
