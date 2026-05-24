// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

#include "media_image_provider.h"

#include <QByteArray>
#include <QImage>
#include <QImageReader>
#include <QLatin1Char>
#include <QList>
#include <QQuickTextureFactory>
#include <QString>
#include <QStringList>
#include <cstddef>
#include <cstdint>
#include <sys/resource.h>
#include <utility>

extern "C" void zaparoo_media_image_bytes_for(
    const char* encoded, std::size_t encoded_len,
    void (*callback)(void* user_data, const std::uint8_t* data, std::size_t len), void* user_data);

namespace
{
// Fixed-arity callback handed to the Rust ABI; it copies the bytes (or
// nothing, on a cache miss) into the caller-supplied QByteArray. The
// pointer is valid for the duration of the callback only — no caching,
// no aliasing, just one memcpy.
void appendBytesCallback(void* user_data, const std::uint8_t* data, std::size_t len)
{
    auto* out = static_cast<QByteArray*>(user_data);
    if (data == nullptr || len == 0)
    {
        return;
    }
    // QByteArray::append takes `const char*`; the bytes are an opaque
    // image payload, not a logical T*-to-T* reinterpretation, so the
    // cast is the standard Qt idiom for filling a QByteArray.
    // NOLINTNEXTLINE(cppcoreguidelines-pro-type-reinterpret-cast)
    out->append(reinterpret_cast<const char*>(data), static_cast<qsizetype>(len));
}

// QML's `sourceSize.width: N` (with no height) arrives here as
// `QSize(N, 0)`. Handing that straight to `QImage::scaled(KeepAspectRatio)`
// collapses to a (0, 0) target via `QSize::scale` and returns a null
// image, so we dispatch on which dimensions are actually positive
// rather than letting `scaled` see a half-spec'd target.
QImage scaleForRequestedSize(const QImage& image, const QSize& requestedSize)
{
    const int reqW = requestedSize.width();
    const int reqH = requestedSize.height();
    if (reqW > 0 && reqH > 0)
    {
        return requestedSize == image.size()
                   ? image
                   : image.scaled(requestedSize, Qt::KeepAspectRatio, Qt::SmoothTransformation);
    }
    if (reqW > 0)
    {
        return image.width() == reqW ? image : image.scaledToWidth(reqW, Qt::SmoothTransformation);
    }
    if (reqH > 0)
    {
        return image.height() == reqH ? image
                                      : image.scaledToHeight(reqH, Qt::SmoothTransformation);
    }
    return image;
}
} // namespace

MediaImageResponse::MediaImageResponse(QString id, QSize requestedSize)
    : m_id(std::move(id)), m_requestedSize(requestedSize)
{
    // QThreadPool would `delete` the runnable after `run()` returns,
    // but `QQuickAsyncImageProvider` expects the response to live until
    // the QML engine has consumed `textureFactory()`. Disabling
    // auto-delete hands ownership to Qt's QObject lifecycle, which
    // calls `deleteLater()` once the engine is done with it.
    setAutoDelete(false);
}

QQuickTextureFactory* MediaImageResponse::textureFactory() const
{
    // Hand off the worker-built factory to QtQuick (it takes ownership
    // and will destroy it when the response is consumed). On the
    // unexpected path where `run()` didn't populate `m_factory` (decode
    // failed and m_image is null), fall back to the per-call
    // construction the base contract expects so QtQuick still gets a
    // non-null factory and the response unwinds cleanly.
    if (m_factory)
    {
        return m_factory.release();
    }
    return QQuickTextureFactory::textureFactoryForImage(m_image);
}

void MediaImageResponse::run()
{
    // De-prioritize the decoder thread on first use so the QML render
    // thread (default niceness on the GUI thread) always preempts it.
    // setpriority(PRIO_PROCESS, 0, …) on Linux is per-thread (NPTL nice),
    // and bumping nice UP doesn't require CAP_SYS_NICE — any user can
    // drop their own thread's priority. The +10 niceness puts decoders
    // at the bottom of the OS scheduler's preference list without going
    // full IDLE, so they still make progress when the GUI is paused.
    // Persists for the lifetime of the QThreadPool's worker thread, so
    // the thread_local guard skips the syscall on subsequent runs that
    // land on the same worker.
    static thread_local bool s_decoderNiced = false;
    if (!s_decoderNiced)
    {
        setpriority(PRIO_PROCESS, 0, 10);
        s_decoderNiced = true;
    }

    // QtQuick strips the `image://media-image/` prefix before calling
    // the provider, so `m_id` is the raw encoded key (base64url-no-pad).
    const QByteArray idUtf8 = m_id.toUtf8();
    QByteArray bytes;
    zaparoo_media_image_bytes_for(idUtf8.constData(), static_cast<std::size_t>(idUtf8.size()),
                                  &appendBytesCallback, &bytes);
    qInfo("media-image provider: id=%s bytes=%lld", idUtf8.constData(),
          static_cast<long long>(bytes.size()));
    if (bytes.isEmpty())
    {
        qWarning("media-image provider: 0 bytes for id=%s (cache miss or empty payload)",
                 idUtf8.constData());
        emit finished();
        return;
    }
    QImage image;
    if (!image.loadFromData(bytes))
    {
        // First 8 bytes pin down the format: PNG = 89 50 4E 47 0D 0A 1A 0A,
        // JPEG = FF D8 FF, WebP starts "RIFF....WEBP". Pairing the magic
        // bytes with the registered format list tells us whether Core sent
        // the wrong payload type or whether Qt simply has no handler for
        // this format (the static MiSTer Qt build can ship without PNG).
        const qsizetype prefixLen = bytes.size() < 8 ? bytes.size() : 8;
        QString prefixHex;
        prefixHex.reserve(prefixLen * 3);
        for (qsizetype i = 0; i < prefixLen; ++i)
        {
            const auto byteVal = static_cast<std::uint8_t>(bytes.at(i));
            if (i > 0)
            {
                prefixHex.append(QLatin1Char(' '));
            }
            prefixHex.append(QStringLiteral("%1").arg(byteVal, 2, 16, QLatin1Char('0')));
        }
        QStringList formatNames;
        const QList<QByteArray> supportedFormats = QImageReader::supportedImageFormats();
        formatNames.reserve(supportedFormats.size());
        for (const QByteArray& fmt : supportedFormats)
        {
            formatNames << QString::fromLatin1(fmt);
        }
        qWarning(
            "media-image provider: QImage::loadFromData failed for id=%s bytes=%lld prefix=[%s] "
            "supportedFormats=[%s]",
            idUtf8.constData(), static_cast<long long>(bytes.size()), qUtf8Printable(prefixHex),
            qUtf8Printable(formatNames.join(QStringLiteral(", "))));
        emit finished();
        return;
    }
    m_image = scaleForRequestedSize(image, m_requestedSize);
    // Build the texture factory here so the GUI thread doesn't pay
    // the allocation cost during paint. `textureFactoryForImage` is
    // documented as safe to call on any thread; the resulting factory
    // wraps `m_image` and is consumed once by QtQuick after `finished()`.
    m_factory.reset(QQuickTextureFactory::textureFactoryForImage(m_image));
    emit finished();
}

MediaImageProvider::MediaImageProvider()
{
    // Workers run at nice +10 (see MediaImageResponse::run), so they
    // get CPU only when the GUI thread isn't asking for it. With that
    // safety in place, parallelism here is "free" against renderer
    // responsiveness and we want all of it: PagedGrid widens its
    // cover-radius gate to ±2 pages, so a page advance can land 30
    // covers in the queue at once, and a fatter pool drains them
    // sooner so page N+2's decode finishes before the user gets there.
    m_pool.setMaxThreadCount(4);
}

QQuickImageResponse* MediaImageProvider::requestImageResponse(const QString& id,
                                                              const QSize& requestedSize)
{
    auto* response = new MediaImageResponse(id, requestedSize);
    m_pool.start(response);
    return response;
}
