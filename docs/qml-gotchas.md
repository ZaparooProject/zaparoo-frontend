# QML Gotchas

Read this before writing or reviewing QML. `qmllint` catches these after the
fact; avoiding them is faster.

- **Typed properties, not `var`.** Use `list<string>`, `list<url>`, `int`, or
  `real`. `var` produces `QVariant` warnings and blocks AOT compilation.

- **`Repeater` delegates need `pragma ComponentBehavior: Bound`** at the top
  of the file. Add `required property int index` to the delegate. Add
  `required property string modelData` when the model is a list.

- **Nested delegate children** must qualify delegate properties. Give the
  delegate an `id` and use `id.modelData`, not bare `modelData`.

- **Singleton QML types** need both `pragma Singleton` in the `.qml` file
  and `set_source_files_properties(Foo.qml PROPERTIES QT_QML_SINGLETON_TYPE TRUE)`
  in CMake, or qmllint will warn "not declared as singleton in qmldir".

- **Function type annotations are required.** Add `: ParamType` parameters and
  `: ReturnType` return types to all functions in singleton `.qml` files.

- **Don't annotate JS-array returns as `list<var>`.** A function whose body
  builds a JS array of plain JS objects — `[{ id, label }, ...]` consumed
  by `.length` and `[i].field` access — must NOT carry a `: list<var>`
  return annotation. On the static QML build (MiSTer ARM32, AOT-compiled)
  the array is coerced through that type and the caller observes
  `result.length === 0` even when the body pushed N items in. The desktop
  dynamic-QML runtime returns the array as-is, so the divergence is
  silent: works in `just run`, breaks on `just deploy-mister`, no qmllint
  warning, no runtime error. Use `: var` or omit the return annotation
  for JS-array helpers; reserve `list<T>` for homogeneous lists of QML
  items consumed by a `Repeater` / model. When something works on
  desktop but not on MiSTer, suspect AOT-QML coercion first.

- **`NumberAnimation on propName`** conflicts with `property T propName: value`.
  Drop the `: value` initializer; the animation takes over immediately.

## Integer-pixel rules

These apply to every screen in the frontend, not just CRT-targeted code
paths. The whole app must render cleanly at 240p; fractional geometry is
a bug everywhere. If a control looks fine on desktop but soft on MiSTer
CRT, assume fractional geometry first — but the fix belongs in the
shared QML, not behind a `crtNativePath` branch.

- **Snap geometry through `Sizing`.** Use `Sizing.px()`, `Sizing.center()`,
  `Sizing.stroke()`, and `Sizing.half()` instead of raw `/ 2`, `%`, or implicit
  centering math when the result drives `x`, `y`, `width`, `height`, margins,
  or border widths.

- **Do not trust centered text by default.** `anchors.horizontalCenter` and
  `Text.AlignHCenter` can leave the glyph run on a half-pixel when the control
  width and measured text width have opposite parity. Center the `Text` item
  itself on an integer `x` (via `Sizing.center()`), then render with
  `horizontalAlignment: Text.AlignLeft` inside that box.

- **Quantize CRT font sizes.** `Sizing.fontSize()` snaps to `8` or `16`
  pixels when `crtNativePath` is active. This is a runtime quantization,
  not a design rule — call `fontSize()` everywhere; the singleton handles
  the quantization where it applies.

- **Reserve space from worst-case metrics.** If dynamic text shares a row with
  icons, measure the widest expected string with `TextMetrics` and reserve that
  width up front. Current example: the header clock reserves the advance width
  of `23:59`.

## Software-renderer animation costs

The MiSTer build runs on Qt Quick's Software adaptation — raster paint engine,
basic (non-threaded) render loop. There's no GPU; every frame is rasterized by
`QPainter` on the CPU.

### Mental model: painted area dominates, animation choice is downstream

Frame cost on raster ≈ **painted pixels per frame × per-pixel cost**. The
animation type matters less than people expect — what matters is what each
animation choice does to that product:

1. **How big is the dirty rectangle?** Animating a 20×20 page-dot dirties
   400 pixels. Animating a full-screen overlay dirties ~2 M pixels. Same
   property (`opacity`), 5000× the cost.
2. **What's *in* the dirty rectangle?** A cached pixmap blit is cheap.
   Re-shaping text glyphs, bilinear-filtering a scaled `Image`, or
   compositing a stack of cells is not. A "small" tween over content
   that's expensive per pixel is still expensive.
3. **Can the renderer short-circuit anything underneath?** Opaque
   covers (`color.a == 1`) subtract their area from the obscured region,
   so the live cells underneath don't repaint. Translucent overlays
   (`opacity < 1`) do *not* subtract — every cell under a fading
   rectangle re-rasterizes per frame, even though "only the rectangle's
   alpha is changing."

So when picking transitions: don't ask "should this fade or slide or
scale?" — ask "**how many pixels of expensive content does this animation
mark dirty per frame?**" and pick whatever keeps that small.

Two follow-on rules from the same model:

- **Translation is free, but its content isn't.** Moving an Item by 1 px
  costs almost nothing if the Item is small (a page-dot, a single tile).
  Moving a band of 12 tiles costs the rasterize of all 12 tiles per
  frame, because the dirty rectangle covers the whole band.
- **Fractional DPR is the absolute version of this.** When Qt's screen
  scale is non-integer, partial updates are disabled and the *entire
  window* repaints every frame regardless of what's animating — at that
  point you've fallen all the way back to "one screen-blit per frame"
  and animation choice is irrelevant. Check `Screen.devicePixelRatio`
  on hardware before redesigning anything.

### Cheat sheet

Pick animations from the cheap column when targeting MiSTer.

| Cheap on raster | Expensive on raster |
|---|---|
| Instant cut + small animated cue (page-dot pulse, focus-ring blink) | Translucent overlays of any size (see below) |
| Translation of small items (one Tile, page dots) | Translation of large content (band of N tiles) |
| ColorAnimation on tints / borders | Concurrent slide + scale (compounds raster cost) |
| Static scenes with one ramping property on a small element | `ShaderEffect` of any kind, `Qt5Compat.GraphicalEffects` |
| `layer.enabled` for caching a complex sub-tree | `Animator` types (no benefit on basic render loop) |

### Translucent overlays force everything underneath to repaint

A fading `Rectangle` (or any Item with `opacity < 1`) over a busy grid does
*not* save paint work — the renderer treats the overlay as non-opaque and
unions its area into the dirty region instead of subtracting it from the
obscured region. Every cell underneath re-rasterizes per frame: text labels,
cover images, card bodies. References:
[`qsgsoftwarerenderablenode.cpp::update()`](https://github.com/qt/qtdeclarative/blob/dev/src/quick/scenegraph/adaptations/software/qsgsoftwarerenderablenode.cpp)
clears `m_isOpaque` whenever opacity < 1;
[`qsgabstractsoftwarerenderer.cpp::optimizeRenderList()`](https://github.com/qt/qtdeclarative/blob/dev/src/quick/scenegraph/adaptations/software/qsgabstractsoftwarerenderer.cpp)
only adds opaque nodes to `m_obscuredRegion`.

For a screen-wide cross-fade you'd want the structural fix:
`Item.grabToImage()` snapshot crossfade — capture both old and new screens
to bitmaps, hide the live content, fade between two single-image blits.
Async grab adds a frame or two of startup latency, snapshot lifetime
needs careful management, and the win still depends on partial updates
being active. The frontend currently sidesteps the problem entirely with
instant cuts.

### Fractional DPR silently disables partial updates entirely

Per Qt's [Software adaptation
docs](https://doc.qt.io/qt-6/qtquick-visualcanvas-adaptations-software.html):
"when using a non-integer scaling factor, the partial update optimization is
disabled, and the entire window is redrawn on every frame." If transitions
feel slow on hardware, check `Screen.devicePixelRatio` and the QPA backend's
reported scale before redesigning anything. A fractional DPR makes every
non-trivial scene structurally choppy regardless of QML technique.

### `layer.enabled` and shader effects

`layer.enabled` itself works on the Software adaptation — there's a real
`QSGSoftwareLayer` class in qtdeclarative. What does *not* work, per the
same Qt docs: `layer.effect: ShaderEffect{}`, the `ShaderEffect` element
generally, and the Qt5Compat `GraphicalEffects` module (DropShadow, Glow,
OpacityMask, RadialGradient, …). Stick to `Rectangle`, `Image`, `Text`,
plain animations, and `layer.enabled` without an effect.

### Recommendation

For state-change feedback, prefer instant cuts with a small localized cue
(page-dot pulse, focus-ring blink, help-bar text change) over any fade.
Cues are small elements with small dirty rectangles; they paint cheaply
on raster regardless of DPR or partial-update status. Reach for a fade
only after diagnosing DPR and ensuring the destination scene is
genuinely static — and then use `Item.grabToImage()` rather than a
translucent overlay.
