// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

#pragma once

#include <QQuickImageProvider>
#include <QSize>
#include <QString>

/// Synchronous image provider for user-supplied system artwork overrides.
///
/// Registered as `"system-image"` in `main.cpp`. Receives the absolute
/// path (URL-component after `image://system-image/`) to the override file
/// and returns a `QImage`. Raster formats are loaded via `QImage(path)`;
/// `.svg` files are rendered via `QSvgRenderer` to `requestedSize` (or the
/// SVG's natural size when `requestedSize` is unset). No tint is applied.
///
/// The provider validates that the decoded path:
///   1. Has an extension in the allowed set (png/jpg/jpeg/webp/bmp/svg).
///   2. Is inside the directory scanned at startup by the Rust side.
/// Requests that fail either check are logged and return a null `QImage`.
class SystemImageProvider : public QQuickImageProvider
{
  public:
    SystemImageProvider();
    ~SystemImageProvider() override = default;

    QImage requestImage(const QString& id, QSize* size, const QSize& requestedSize) override;
};
