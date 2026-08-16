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

#include <QCache>
#include <QElapsedTimer>
#include <QImage>
#include <QMutex>
#include <QQuickAsyncImageProvider>
#include <QQuickImageResponse>
#include <QQuickTextureFactory>
#include <QRunnable>
#include <QSize>
#include <QString>
#include <QThreadPool>
#include <array>
#include <memory>

class MediaImageResponse : public QQuickImageResponse, public QRunnable
{
  public:
    MediaImageResponse(QString id, QSize requestedSize, QMutex* cacheMutex,
                       QCache<QString, QImage>* decodedCache,
                       std::array<QMutex, 16>* decodeMutexes);
    ~MediaImageResponse() override = default;

    [[nodiscard]] QQuickTextureFactory* textureFactory() const override;
    void run() override;

  private:
    struct RawImageResult
    {
        QImage image;
        qint64 fetchUs = 0;
        qint64 decodeUs = 0;
        bool cacheHit = false;
    };
    [[nodiscard]] RawImageResult loadRawImage(qint64 queueWaitUs, int inflight);
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
    QMutex* m_cacheMutex;
    QCache<QString, QImage>* m_decodedCache;
    // Striped per-ID decode locks. Requests for one cover at width-only,
    // height-only, and natural size share one raw decode while unrelated
    // covers still use separate workers.
    std::array<QMutex, 16>* m_decodeMutexes;
    // Started when the response is constructed (i.e. enqueued onto the
    // pool). Read at run() start it yields queue_wait (time spent waiting
    // for a free worker); read before each finished() it yields the total
    // request lifetime. Both are otherwise invisible in the decode-only
    // trace and are the segments most likely to dominate on a fresh page.
    QElapsedTimer m_lifetime;
};

class MediaImageProvider : public QQuickAsyncImageProvider
{
  public:
    MediaImageProvider();
    ~MediaImageProvider() override;
    MediaImageProvider(const MediaImageProvider&) = delete;
    MediaImageProvider& operator=(const MediaImageProvider&) = delete;
    MediaImageProvider(MediaImageProvider&&) = delete;
    MediaImageProvider& operator=(MediaImageProvider&&) = delete;

    QQuickImageResponse* requestImageResponse(const QString& id,
                                              const QSize& requestedSize) override;

  private:
    // Bounded so a fast-scrolling user enqueueing 30+ tiles doesn't
    // spawn dozens of decode threads on MiSTer's two ARM cores. Four
    // workers is the same cap `MediaImageCache`'s fetch driver uses;
    // beyond that, context-switch cost outweighs parallel decode on
    // the software-rendered build.
    QThreadPool m_pool;
    // Process-memory cache for raw decoded images plus scaled variants. Raw
    // entries are keyed only by media ID, allowing width-only, height-only,
    // and natural-size QML consumers to share expensive WebP decoding. Scaled
    // variants keep their requested-size key. One shared byte cap accounts for
    // both forms.
    QMutex m_cacheMutex;
    QCache<QString, QImage> m_decodedCache;
    std::array<QMutex, 16> m_decodeMutexes;
};
