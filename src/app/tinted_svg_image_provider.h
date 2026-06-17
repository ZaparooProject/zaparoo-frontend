// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

#pragma once

#include <QCache>
#include <QImage>
#include <QMutex>
#include <QQuickAsyncImageProvider>
#include <QQuickImageResponse>
#include <QQuickTextureFactory>
#include <QRunnable>
#include <QSize>
#include <QString>
#include <QThreadPool>
#include <memory>

class TintedSvgImageProvider;

class TintedSvgImageResponse : public QQuickImageResponse, public QRunnable
{
  public:
    TintedSvgImageResponse(QString id, QSize requestedSize, QMutex* cacheMutex,
                           QCache<QString, QImage>* logoCache);
    ~TintedSvgImageResponse() override = default;

    [[nodiscard]] QQuickTextureFactory* textureFactory() const override;
    [[nodiscard]] QString errorString() const override;
    void run() override;

  private:
    QString m_id;
    QSize m_requestedSize;
    QString m_error;
    QImage m_image;
    mutable std::unique_ptr<QQuickTextureFactory> m_factory;
    QMutex* m_cacheMutex;
    QCache<QString, QImage>* m_logoCache;
};

class TintedSvgImageProvider : public QQuickAsyncImageProvider
{
  public:
    TintedSvgImageProvider();
    ~TintedSvgImageProvider() override = default;

    QQuickImageResponse* requestImageResponse(const QString& id,
                                              const QSize& requestedSize) override;

  private:
    QThreadPool m_pool;
    // Process-memory cache for tinted logo renders. The tint result is
    // deterministic per (id, requestedSize) so repeated loads — after
    // the QML pixmap cache evicts, or across category re-entries — skip
    // the SVG rasterize + per-pixel tint pass entirely. Cost is tracked
    // in bytes; maxCost caps the total footprint on MiSTer (<512 MB shared).
    QMutex m_cacheMutex;
    QCache<QString, QImage> m_logoCache;
};
