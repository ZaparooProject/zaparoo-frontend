// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

#pragma once

#include <QQuickImageProvider>
#include <QSize>
#include <QString>

/// Synchronous image provider for user-supplied customization images
/// (system artwork and Hub icons).
///
/// Registered as `"custom-image"` in `main.cpp`. Receives the absolute
/// path (URL-component after `image://custom-image/`) to the override file
/// and returns a `QImage`. Raster formats are loaded via `QImage(path)`;
/// `.svg` files are rendered via `QSvgRenderer` to `requestedSize` (or the
/// SVG's natural size when `requestedSize` is unset). No tint is applied —
/// the image is served exactly as it is on disk.
///
/// The provider validates that the decoded path:
///   1. Has an extension in the allowed set (png/jpg/jpeg/webp/bmp/svg).
///   2. Is inside the customization root scanned at startup by the Rust side.
/// Requests that fail either check are logged and return a null `QImage`.
class CustomImageProvider : public QQuickImageProvider
{
  public:
    CustomImageProvider();
    ~CustomImageProvider() override = default;

    QImage requestImage(const QString& id, QSize* size, const QSize& requestedSize) override;
};
