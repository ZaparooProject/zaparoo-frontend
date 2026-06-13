// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

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

class TintedSvgImageResponse : public QQuickImageResponse, public QRunnable
{
  public:
    TintedSvgImageResponse(QString id, QSize requestedSize);
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
};
