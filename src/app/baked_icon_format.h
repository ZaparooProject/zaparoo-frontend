// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

#pragma once

#include <QtGlobal>

// On-disk layout of resources/baked/icons.zbin, shared by the bake tool
// (tools/bake-icons) and the runtime reader (baked_icon_atlas).
//
// The blob is stored in the qrc with `-no-compress`, so rcc emits it verbatim
// into .rodata and the runtime reads mask bytes straight out of the
// MAP_PRIVATE file-backed mapping of the executable. Those pages are clean and
// evictable: under memory pressure the kernel drops them for free instead of
// pushing an equivalent heap cache into swap. That property is the entire
// reason for the format, so BakedIconAtlas asserts NoCompression at startup.
//
// Everything is little-endian and read through qFromLittleEndian(const void*),
// which is memcpy-based — rcc gives no alignment guarantee and ARMv7 would
// fault on a misaligned direct load.

namespace baked
{

constexpr char kMagic[8] = {'Z', 'A', 'P', 'B', 'A', 'K', 'E', '1'};
// v2: plane rows are padded to kRowAlignment bytes (v1 packed rows tight at
// `width` bytes, which violated QImage's raw-buffer alignment contract for
// any non-4-aligned width -- see kRowAlignment above).
constexpr quint32 kVersion = 2;

// Plane data is padded to this so a future SIMD reader can load aligned.
constexpr int kPlaneAlignment = 16;

// QImage's raw-buffer constructor requires bytesPerLine to be a multiple of
// 4 (see QImage's own docs); Format_Grayscale8 is 1 byte/pixel, so a plane's
// row stride must be `width` rounded up to this, not `width` itself. Corner
// masks (baked at radii 1-16) hit this constantly -- only radii 4/8/12/16
// are already 4-aligned. Shared by the bake tool (which pads each row when
// writing a plane) and the runtime reader (which uses it as bytesPerLine in
// the zero-copy QImage view), so the two can never disagree about layout.
constexpr int kRowAlignment = 4;

constexpr int paddedStride(int width)
{
    return (width + kRowAlignment - 1) / kRowAlignment * kRowAlignment;
}

// Header, 32 bytes:
//   0   magic[8]        "ZAPBAKE1"
//   8   version         u32
//   12  entryCount      u32
//   16  stringPoolOff   u32   absolute offset of the NUL-separated path pool
//   20  totalBytes      u32   full blob size, for a cheap sanity check
//   24  digestPrefix[8] first 8 bytes of the manifest's sources_digest
constexpr int kHeaderSize = 32;
constexpr int kOffsetMagic = 0;
constexpr int kOffsetVersion = 8;
constexpr int kOffsetEntryCount = 12;
constexpr int kOffsetStringPool = 16;
constexpr int kOffsetTotalBytes = 20;
constexpr int kOffsetDigestPrefix = 24;
constexpr int kDigestPrefixSize = 8;

// Directory entry, 24 bytes, sorted by path so the committed blob diffs
// reviewably and a reader could bisect it:
//   0   pathOffset   u32   absolute offset into the blob
//   4   width        u16   baked raster size
//   6   height       u16
//   8   baseWidth    u16   the SVG's own defaultSize()
//   10  baseHeight   u16
//   12  flags        u16
//   14  reserved     u16
//   16  alphaOffset  u32   absolute offset, paddedStride(width)*height bytes
//   20  toneOffset   u32   absolute offset, or 0 when kFlagSingleTone is set
//
// baseWidth/baseHeight are carried so the runtime resolves a partially
// specified sourceSize against the SVG's own dimensions, exactly as
// svgsize::renderSizeFor() did when it had the renderer in hand. Resolving
// against the baked size instead would round differently and quietly shift a
// downscaled request by a pixel.
constexpr int kEntrySize = 24;
constexpr int kEntryOffsetPath = 0;
constexpr int kEntryOffsetWidth = 4;
constexpr int kEntryOffsetHeight = 6;
constexpr int kEntryOffsetBaseWidth = 8;
constexpr int kEntryOffsetBaseHeight = 10;
constexpr int kEntryOffsetFlags = 12;
constexpr int kEntryOffsetReserved = 14;
constexpr int kEntryOffsetAlpha = 16;
constexpr int kEntryOffsetTone = 20;

// The asset's luma barely varies, so it takes the tint's highlight flat and
// carries no tone plane. Corner masks (Part 5) are always single-tone.
constexpr quint16 kFlagSingleTone = 0x0001;
// Reserved for the documented follow-up that packs 2-tone logos into a 1-bit
// tone plane plus two palette indices. Readers must reject an unknown flag.
constexpr quint16 kFlagKnownMask = kFlagSingleTone;

// The bake renders every asset this wide; square assets therefore come out
// 256x256 and system logos keep their aspect ratio. Requests at exactly this
// size are zero-copy, smaller ones downscale the mask planes, larger ones fall
// back to rasterizing the SVG.
constexpr int kBakeWidth = 256;

// Where the blob lands in the qrc, and where the atlas keys live.
constexpr auto kResourcePath = ":/qt/qml/Zaparoo/App/resources/baked/icons.zbin";

} // namespace baked
