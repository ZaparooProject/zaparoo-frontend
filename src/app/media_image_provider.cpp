// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

#include "media_image_provider.h"

#include <QByteArray>
#include <QElapsedTimer>
#include <QImage>
#include <QImageReader>
#include <QLatin1Char>
#include <QList>
#include <QQuickTextureFactory>
#include <QString>
#include <QStringList>
#include <atomic>
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

MediaImageResponse::MediaImageResponse(QString id, QSize requestedSize, QMutex* cacheMutex,
                                       QCache<QString, QImage>* decodedCache)
    : m_id(std::move(id)), m_requestedSize(requestedSize), m_cacheMutex(cacheMutex),
      m_decodedCache(decodedCache)
{
    // QThreadPool would `delete` the runnable after `run()` returns,
    // but `QQuickAsyncImageProvider` expects the response to live until
    // the QML engine has consumed `textureFactory()`. Disabling
    // auto-delete hands ownership to Qt's QObject lifecycle, which
    // calls `deleteLater()` once the engine is done with it.
    setAutoDelete(false);
    // Start the lifetime clock at enqueue so run() can report how long
    // this response waited for a worker (queue_wait) and the full
    // enqueue->finished span (total).
    m_lifetime.start();
}

// Number of responses currently executing run() across all pool workers.
// Logged as inflight=N so a slow decode can be read against how swamped
// the decoder was at that instant — distinguishing "decode is slow" from
// "decoder is backed up".
namespace
{
// A shared decode-concurrency gauge must be mutable by design; the atomic
// makes the cross-thread increments well-defined.
// NOLINTNEXTLINE(cppcoreguidelines-avoid-non-const-global-variables)
std::atomic<int> g_inflight{0};

// RAII bump of the in-flight counter so every exit path of run() (cache
// hit, empty bytes, decode failure, success) decrements exactly once.
struct InflightGuard
{
    InflightGuard()
    {
        g_inflight.fetch_add(1, std::memory_order_relaxed);
    }
    ~InflightGuard()
    {
        g_inflight.fetch_sub(1, std::memory_order_relaxed);
    }
    InflightGuard(const InflightGuard&) = delete;
    InflightGuard& operator=(const InflightGuard&) = delete;
    InflightGuard(InflightGuard&&) = delete;
    InflightGuard& operator=(InflightGuard&&) = delete;
};
} // namespace

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
    // queue_wait: time this response sat in the pool between enqueue
    // (construction) and a worker picking it up (now). inflight: how many
    // responses are executing run() concurrently at this moment.
    const qint64 queueWaitUs = m_lifetime.nsecsElapsed() / 1000;
    const InflightGuard inflightGuard;
    const int inflight = g_inflight.load(std::memory_order_relaxed);

    // Check the process-memory cache before any FFI fetch or decode. The
    // decoded image is deterministic per (id, requestedSize), so a re-invocation
    // after a PagedGrid teardown or a QPixmapCache eviction reuses the bitmap and
    // skips the ~424 ms WebP decode entirely. Key encodes both id and requested
    // size so entries at different tiers (e.g. grid 0x512 vs detail 512x0) don't
    // alias each other.
    const QString cacheKey = m_id + QStringLiteral(":") + QString::number(m_requestedSize.width()) +
                             QStringLiteral("x") + QString::number(m_requestedSize.height());
    {
        QMutexLocker locker(m_cacheMutex);
        if (const QImage* cached = m_decodedCache->object(cacheKey))
        {
            m_image = *cached;
            m_factory.reset(QQuickTextureFactory::textureFactoryForImage(m_image));
            const qint64 totalUs = m_lifetime.nsecsElapsed() / 1000;
            qDebug("media-image decode: id=%s req=%dx%d out=%dx%d queue_wait_us=%lld fetch_us=0 "
                   "decode_us=0 scale_us=0 total_us=%lld inflight=%d cached=1",
                   qUtf8Printable(m_id), m_requestedSize.width(), m_requestedSize.height(),
                   m_image.width(), m_image.height(), static_cast<long long>(queueWaitUs),
                   static_cast<long long>(totalUs), inflight);
            emit finished();
            return;
        }
    }

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
    QElapsedTimer fetchTimer;
    fetchTimer.start();
    zaparoo_media_image_bytes_for(idUtf8.constData(), static_cast<std::size_t>(idUtf8.size()),
                                  &appendBytesCallback, &bytes);
    const qint64 fetchUs = fetchTimer.nsecsElapsed() / 1000;
    qDebug("media-image provider: id=%s bytes=%lld fetch_us=%lld", idUtf8.constData(),
           static_cast<long long>(bytes.size()), static_cast<long long>(fetchUs));
    if (bytes.isEmpty())
    {
        const qint64 totalUs = m_lifetime.nsecsElapsed() / 1000;
        qDebug("media-image provider: 0 bytes for id=%s (no cover or empty payload) "
               "queue_wait_us=%lld fetch_us=%lld total_us=%lld inflight=%d",
               idUtf8.constData(), static_cast<long long>(queueWaitUs),
               static_cast<long long>(fetchUs), static_cast<long long>(totalUs), inflight);
        emit finished();
        return;
    }
    QElapsedTimer decodeTimer;
    decodeTimer.start();
    QImage image;
    const bool decodeOk = image.loadFromData(bytes);
    const qint64 decodeUs = decodeTimer.nsecsElapsed() / 1000;
    if (!decodeOk)
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
        const qint64 totalUs = m_lifetime.nsecsElapsed() / 1000;
        qWarning(
            "media-image provider: QImage::loadFromData failed for id=%s bytes=%lld prefix=[%s] "
            "supportedFormats=[%s] queue_wait_us=%lld fetch_us=%lld decode_us=%lld total_us=%lld "
            "inflight=%d",
            idUtf8.constData(), static_cast<long long>(bytes.size()), qUtf8Printable(prefixHex),
            qUtf8Printable(formatNames.join(QStringLiteral(", "))),
            static_cast<long long>(queueWaitUs), static_cast<long long>(fetchUs),
            static_cast<long long>(decodeUs), static_cast<long long>(totalUs), inflight);
        emit finished();
        return;
    }
    QElapsedTimer scaleTimer;
    scaleTimer.start();
    m_image = scaleForRequestedSize(image, m_requestedSize);
    // Build the texture factory here so the GUI thread doesn't pay
    // the allocation cost during paint. `textureFactoryForImage` is
    // documented as safe to call on any thread; the resulting factory
    // wraps `m_image` and is consumed once by QtQuick after `finished()`.
    m_factory.reset(QQuickTextureFactory::textureFactoryForImage(m_image));
    const qint64 scaleUs = scaleTimer.nsecsElapsed() / 1000;
    // Store the decoded+scaled image so a re-invocation at the same tier skips
    // the decode. Cost is tracked in bytes so maxCost caps total memory use.
    {
        QMutexLocker locker(m_cacheMutex);
        if (!m_decodedCache->contains(cacheKey))
        {
            const auto cost = static_cast<int>(m_image.sizeInBytes());
            // QCache::insert takes ownership of the raw pointer; this is the
            // documented API and the owning-memory diagnostic is expected here.
            // NOLINTNEXTLINE(cppcoreguidelines-owning-memory)
            m_decodedCache->insert(cacheKey, new QImage(m_image), cost);
        }
    }
    // Decode cost trace. `queue_wait_us` is time spent waiting for a worker;
    // `fetch_us` the Rust FFI byte lookup; `decode_us` the WebP→QImage decode
    // (the work the decoded-image cache eliminates on a re-invocation, logged
    // `cached=1`); `scale_us` the resample-to-tier plus texture-factory build;
    // `total_us` the full enqueue->finished span; `inflight` how many decodes
    // were running concurrently when this one started. `src` is the decoded
    // size Core delivered and `out` the post-scale size — equal when the
    // request tier matches what Core sent, i.e. no resample.
    const qint64 totalUs = m_lifetime.nsecsElapsed() / 1000;
    qDebug("media-image decode: id=%s req=%dx%d src=%dx%d out=%dx%d queue_wait_us=%lld "
           "fetch_us=%lld decode_us=%lld scale_us=%lld total_us=%lld inflight=%d cached=0",
           idUtf8.constData(), m_requestedSize.width(), m_requestedSize.height(), image.width(),
           image.height(), m_image.width(), m_image.height(), static_cast<long long>(queueWaitUs),
           static_cast<long long>(fetchUs), static_cast<long long>(decodeUs),
           static_cast<long long>(scaleUs), static_cast<long long>(totalUs), inflight);
    emit finished();
}

// 64 MB cap: a scaled 374×512 ARGB grid cover is ~765 KB, so this holds ~85
// decoded covers — most of a large system's worth — while leaving SD page
// cache room inside MiSTer's measured ~307 MB available. Tune after re-measuring
// against a real browse session.
static constexpr int kDecodedCacheMaxBytes = 64 * 1024 * 1024;

MediaImageProvider::MediaImageProvider() : m_decodedCache(kDecodedCacheMaxBytes)
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
    auto* response = new MediaImageResponse(id, requestedSize, &m_cacheMutex, &m_decodedCache);
    m_pool.start(response);
    return response;
}
