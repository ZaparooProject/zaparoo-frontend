# Native Core POC

This POC keeps the frontend on Qt's `linuxfb` backend, but lets a custom
MiSTer core own the real analog video path.

## Goal

Prove that Zaparoo Frontend can render into a small Linux framebuffer while
CRT output comes from a Menu-derived MiSTer core, not from MiSTer's framebuffer
scaler path.

## Frontend Config

On MiSTer, set `/media/fat/zaparoo/frontend.toml`:

```toml
[video]
width = 960
height = 720
```

The MiSTer wrapper must launch the frontend with `--crt` when the 3S-ARM core
owns analog output:

```sh
/media/fat/zaparoo/frontend --crt
```

When `--crt` is set, the frontend still uses:

- `QT_QPA_PLATFORM=linuxfb`
- `QT_QUICK_BACKEND=software`

Before Qt starts, it also runs:

```sh
vmode -r 960 720 rgb16
```

After QML loads, the frontend also starts a native-video copy thread:

- reads `/dev/fb0` as 24-bit RGB8888
- requires `/dev/fb0` to be at least `320x240`
- copies only the top-left `320x240` pixels, with no scaling
- copies pixels directly; no RGB conversion
- writes frames to the 3S-ARM native-video DDR layout:

```text
0x3A000000  control word: (frame_counter << 2) | active_buffer
0x3A000100  buffer 0: 320x240 RGB8888
0x3A04B100  buffer 1: 320x240 RGB8888
```

## Current CRT UI Constraints

This branch is still Qt-on-`linuxfb`, so CRT legibility depends on keeping the
QML scene aligned to integer pixels.

- Use `Sizing.px()`, `Sizing.center()`, `Sizing.stroke()`, and `Sizing.half()`
  for geometry that would otherwise land on fractional coordinates.
- Avoid `anchors.horizontalCenter` / `Text.AlignHCenter` for CRT-critical text
  when the final glyph run can land on a half-pixel. Center the `Text` item on
  an integer `x`, then draw the glyphs left-aligned inside it.
- Treat line widths and borders as integer pixels only.
- In CRT mode, `Sizing.fontSize()` is intentionally quantized to `8` or `16`
  pixels only. Intermediate sizes are not allowed for now.
- The CRT path uses `MxPlus HP 100LX 6x8` plus Qt native text rendering. Any new text
  treatment added to the CRT path must be checked against that assumption.
- Header HUD layout reserves clock width using the measured advance of `23:59`
  so the Wi-Fi / LAN / Bluetooth / NFC icons do not shift as time changes.

## Core/Wrapper Contract

The custom wrapper/core side must keep the scaler framebuffer path disabled for
real CRT output:

- do not call `set_vga_fb(1)` for the CRT path
- do not call `video_fb_enable(1)` for the CRT path
- leave the core video path selected so analog output comes from `VGA_R/G/B`
- launch `/media/fat/zaparoo/frontend --crt`

The frontend will still paint `/dev/fb0`. The custom core can read the 3S-ARM
DDR layout above while it drives CRT timing itself.

## Success Criteria

1. Frontend starts and logs `--crt: applying linuxfb mode ... rgb16`.
2. `/dev/fb0` mode is the configured framebuffer size.
3. Frontend logs `native video writer: copying top-left 320x240 RGB8888 from /dev/fb0 ...`.
4. Custom core produces analog video without MiSTer `vga_fb` scaler output.
5. Navigation changes frontend pixels in the native-video DDR buffers.
6. No regression when `--crt` is omitted; default remains the normal `linuxfb`
   path.

## Known Limits

This is not the final native renderer. It still relies on Qt rendering to
`linuxfb`; it only decouples that framebuffer from the actual CRT output path.
Final native video should render offscreen or write a dedicated DDR buffer
directly.

The CRT typography path is also provisional. It currently assumes:

- integer-positioned layout
- 8 px / 16 px font quantization only
- one bundled bitmap-style font family

If those assumptions change, revisit the snapping rules above before adding
more CRT-specific UI polish.
