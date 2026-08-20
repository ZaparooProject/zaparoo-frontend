// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

#pragma once

#include <QHash>
#include <QResource>
#include <QSize>
#include <QString>
#include <QtGlobal>

// Read-only view over the baked mask atlas in .rodata. See
// baked_icon_format.h for the layout and for why the blob is uncompressed.
//
// The directory is built once during construction and is immutable afterwards,
// so reads need no mutex and are safe from the GUI thread and the
// QQuickPixmapReader thread at the same time -- a strict improvement on the
// mutexed QCache the provider used to consult on every request.
class BakedIconAtlas
{
  public:
    struct Entry
    {
        const quint8* alpha = nullptr; // width*height bytes in mapped memory
        const quint8* tone = nullptr;  // null when singleTone
        int width = 0;
        int height = 0;
        // The source SVG's own defaultSize(). A partially specified
        // sourceSize resolves against this, not against the baked raster, so
        // the output size matches what the SVG path produced byte for byte.
        int baseWidth = 0;
        int baseHeight = 0;
        bool singleTone = false;
    };

    // Resolves a (possibly zero-filled) sourceSize the way
    // svgsize::renderSizeFor() does, using the entry's recorded SVG size.
    [[nodiscard]] static QSize resolveSize(const Entry& entry, const QSize& requestedSize);

    // Function-local static: parsed on first use, never destroyed early.
    static const BakedIconAtlas& instance();

    // `resourcePath` is the provider's path segment, e.g. "images/systems/SNES.svg".
    [[nodiscard]] const Entry* find(const QString& resourcePath) const;

    [[nodiscard]] bool isValid() const
    {
        return m_valid;
    }
    [[nodiscard]] qsizetype entryCount() const
    {
        return m_entries.size();
    }
    [[nodiscard]] qsizetype byteSize() const
    {
        return m_size;
    }

    // Touches one byte per page across the blob on a background runnable so
    // the first real request does not take a synchronous major fault. On
    // MiSTer the atlas lives on SD and a cold page costs random-read latency;
    // this is the difference between "synchronous" and "synchronous and fast".
    void prefaultAsync() const;

    BakedIconAtlas(const BakedIconAtlas&) = delete;
    BakedIconAtlas& operator=(const BakedIconAtlas&) = delete;
    BakedIconAtlas(BakedIconAtlas&&) = delete;
    BakedIconAtlas& operator=(BakedIconAtlas&&) = delete;

  private:
    BakedIconAtlas();
    ~BakedIconAtlas() = default;

    bool parse();

    QResource m_resource;
    const quint8* m_base = nullptr;
    qsizetype m_size = 0;
    QHash<QString, Entry> m_entries;
    bool m_valid = false;
};
