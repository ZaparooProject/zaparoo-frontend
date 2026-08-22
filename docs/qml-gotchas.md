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

- **Center native text items, not glyphs inside a tall box.** On the CRT path,
  `Text.NativeRendering` with the bitmap font can visually clip or punch out
  glyph rows when a `Text` item fills a taller capsule/card and relies on
  `verticalAlignment: Text.AlignVCenter`. Use the text's natural height
  (`height: Sizing.px(implicitHeight)`) and center the `Text` item itself with
  `y: Sizing.center(parent.height, height)`. This keeps capsule fills behind
  the text without blurring or z-order hacks.

- **Quantize CRT font sizes.** Semantic `Sizing.fontHero` … `fontSmall`
  tokens resolve through `Sizing.fontSize()` on CRT, which snaps to `8` or
  `16` pixels. Specialist calls may still use `fontSize()` directly; never
  bypass Sizing with a raw pixel size.

- **Reserve space from worst-case metrics.** If dynamic text shares a row with
  icons, measure the widest expected string with `TextMetrics` and reserve that
  width up front. Current example: the header clock reserves the advance width
  of `23:59`.

## Loading-cue anchor rule

There are two loading cues in sequence for any screen transition that takes
long enough to show one: the global transition cue (`Main.qml`, shown while
the destination screen has no content yet) hands off to that screen's own
`ScreenStateOverlay` loading cue once the screen mounts. Both cues render the
same `DelayedLoadingIndicator`, so if they center on different rects the
handoff visibly jumps the label by a few pixels — worse in TATE, worse again
at higher resolutions.

**Both cues must center on the same coordinate space.** The global cue is
parented into `scene` (not a bare window-relative `Item`) and sized to it, so
it already accounts for the CRT safe-area inset and `scene.rotation`. A
screen's `ScreenStateOverlay` is not necessarily the same rect: `MediaListScreen`
and `SystemsScreen` both start their overlay below a header bar, so the
overlay's own `height / 2` is *not* the point the global cue centered on.

`ScreenStateOverlay` exposes `cueCenterY` (default `overlay.height / 2`,
correct when the overlay genuinely fills the screen, e.g. `SettingsScreen`)
specifically so an inset host can override it:

```qml
// Content rect starts below the header, so recenter on the full screen
// (which matches `scene`, the global cue's parent) instead of this rect's
// own smaller height.
cueCenterY: root.height / 2 - y
```

The `- y` term converts a window-space target back into the overlay's own
local coordinate space, since `cueCenterY` is consumed as
`y: Sizing.px(cueCenterY - height / 2)` inside the overlay. Get this formula
wrong (e.g. omit the `- y`) and the cue centers on the wrong point only on
screens whose overlay is offset from the window origin — exactly the kind of
regression that is invisible on `SettingsScreen` and only shows up on
`MediaListScreen`/`SystemsScreen`. `tests/ui/tst_screen_state_overlay.qml`
maps both a default and an offset-content-rect overlay's cue into a shared
"window space" `Item` and asserts they land at the same y — add a case there
for any new loading-capable screen with a content rect offset from its
container.

## Software-renderer animation costs

The MiSTer build runs on Qt Quick's Software adaptation — raster paint engine,
basic (non-threaded) render loop. There's no GPU; every frame is rasterized by
`QPainter` on the CPU.

### Mental model: painted area dominates, animation choice is downstream

Frame cost on raster ≈ **painted pixels per frame × per-pixel cost**. The
animation type matters less than people expect — what matters is what each
animation choice does to that product:

1. **How big is the dirty rectangle?** Animating a 20×20 scroll-thumb dirties
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
  costs almost nothing if the Item is small (a single tile, the scroll-thumb).
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
| Instant cut + small one-shot cue (tile press or row flash) | Translucent overlays of any size (see below) |
| Integer translation of small items (one tile face or cursor rail) | Translation of large content (band of N tiles) |
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
(the physical press, row flash, or help-bar text change) over any fade.
Cues are small elements with small dirty rectangles; they paint cheaply
on raster regardless of DPR or partial-update status. Reach for a fade
only after diagnosing DPR and ensuring the destination scene is
genuinely static — and then use `Item.grabToImage()` rather than a
translucent overlay.

### Sanctioned one-shot transient cues

The rule above bans **persistent** motion that runs every frame while content
is busy (e.g., a scale held on every focused tile on every d-pad move). It
does NOT ban short one-shot animations on a single small element triggered
at a state-change moment (activate/launch, selection land). Those are cheap for the same reason a single-tile face translation is cheap:
one element, one short burst, then back to a static scene.

Sanctioned patterns and why they are safe:

| Cue | Cost analysis |
|---|---|
| Tile physical press on activate or launch (~80 ms, held) | One opaque face translates down by `Sizing.pressEdgeHeight`; dirty rect = one tile; no cover-art resampling. The host screen's `settling` flag raises the face off-screen. One shared cue covers forward navigation and game launch |
| List/settings row inverse-blink on activate or launch (~80 ms, self-clearing) | Only the selected row's `SelectionBar` swaps fill and content color for `Motion.pressMs`, then swaps back — 2 repaints total, nothing moves; background and neighboring rows remain static |
| Settings toggle-knob slide (x, ~110 ms) | One tiny Rectangle handle; 1 pctW |

The shared constraint: the source scene must be static or near-static during
the cue. The tile grid is not scrolling; the list row content is not
changing. If there is any chance the surrounding content is busy (rapid
scroll, prefetch, incoming model update), gate the Behavior off via a
`rapidScrollActive` flag or equivalent so the cue collapses to instant.

The previously removed 1.06x Tile focus scale was **persistent** - held
across every d-pad move while the grid was live. Any tile that held the old
scale forced its pixmap to be bilinear-filtered on every rendered frame,
compounding across focus moves. That is the pattern being banned; the
one-shot transients above do not share that cost profile.

### The one continuous exception: ProgressTrack's leading-cell blink

`ProgressTrack.qml` (the header status line's segmented progress bar,
replacing `CoreStatusPill`'s old 4-dot spinner) blinks whichever cell sits
at the fill's leading edge (or the marching cell in indeterminate mode) for
as long as a background task is active. This is not one-shot, and it is
deliberately the only cue in the app that isn't:

| Cue | Why it still qualifies |
|---|---|
| `ProgressTrack` leading-cell blink (`Motion.pulseMs` ≈ 2 Hz, loops while active) | Header chrome only — never painted over a grid or list. Dirty rect is one small `Sizing.radiusSm` cell. Gated on the task actually running and not paused (`running: root._pulsing`, itself `root.active && !root.paused && Motion.enabled`), so it is inert whenever nothing is happening. Stops outright under Reduce Motion rather than collapsing to a 0 ms loop, matching `Tile.qml`'s `interval: Motion.dur(650)` double-gating precedent (`Motion.dur()` on the duration *and* `Motion.enabled` in `running`) |

It is a genuine blink, not a fade: a plain `Timer` flips a bool
(`root._blinkOn`) every `Motion.pulseMs`, and the cell's `color` reads that
bool straight through — `cell._isPulseCell ? (root._blinkOn ? Theme.accent :
Theme.borderSubtle) : ...`. No `ColorAnimation`, no `Behavior on color`,
nothing interpolated. Each tick is a single solid-color repaint of one cell,
the same instant-swap idiom `SelectionBar`'s inverse-video flash already
uses for its own one-shot cue (`PropertyAction`, not an animated transition)
— this just repeats it on a timer instead of firing once.

The distinction from the one-shot cues above is why it needs its own
exemption argument rather than sliding under the same table: those cues
fire once at a state-change moment and return to a static scene; this one
repeats for as long as its task does. It still fits the cost model this
whole section is built on — small dirty rect, cheap per-pixel content (a
solid-fill `Rectangle`, no interpolation to compute), never composited over
busy content — so the "persistent motion" ban is about *scale and
location* (a scale/fade over many delegates or a busy grid), not about
repetition. A single small cell blinking in otherwise-static header chrome
was never the expensive case the rule exists to prevent. See
`docs/style.md` → "Header status line" for the full design writeup.

### Motion tokens and the reduce-motion convention

All animation durations in QML go through the `Motion` singleton in
`Zaparoo.Theme`. Never hardcode a duration inline:

```qml
// Good
NumberAnimation { duration: Motion.dur(Motion.settleMs) }
Behavior on x { enabled: Motion.enabled; NumberAnimation { duration: Motion.dur(Motion.settleMs) } }

// Bad - not controlled by reduce-motion, not adjustable from one place
NumberAnimation { duration: 140 }
```

`Motion.dur(ms)` returns `ms` when `Motion.enabled` is true and `0` when
false. A duration of `0` causes a Behavior or SequentialAnimation to resolve
in one frame (instant cut). This is the reduce-motion path: zero code
branches, zero dead animation objects, no visible change to the rest of the
logic.

`Motion.enabled` is fed from the app layer via a `Binding` in `Main.qml`:

```qml
Binding { target: Motion; property: "enabled"; value: !Browse.Settings.current_reduce_motion }
```

The `Motion` singleton itself does not import `Zaparoo.Browse` - the app
layer crosses the module boundary. This keeps `Zaparoo.Theme` free of
dependencies on the models module, consistent with `Sizing` and `Theme`.

Token summary (`Motion.qml`):

| Token | Value | Use |
|---|---|---|
| `pressMs` | 80 | Physical press or row inverse-blink cue |
| `settleMs` | 110 | Release leg; toggle-knob slide |
| `pulseMs` | 250 | `ProgressTrack` leading-cell blink, on/off hold time (~2 Hz full cycle) |

`pressMs`/`settleMs` sit just above MiSTer's frame-budget floor (~3 frames at
~30fps); see the comments in `Motion.qml` before lowering them. `pulseMs`
isn't bound by that floor the same way — it's a hard on/off cut, not
tracked positional motion — its value comes from matching a deliberately
snappy, "still alive" cadence instead; see the exception writeup above before
changing it.

Pulse counter pattern (how hosts trigger tile cues without coupling to
animation internals): the host increments the `activatePulse` int property
on the grid or TileLoader; `Tile.qml` watches the delegate contract
`delegateActivatePulse` and lowers the `PressableSurface` face if
`_focusedSelection` is true. This keeps cue state entirely inside `Tile.qml` -
hosts only bump a counter. One physical press covers every button-like action:
forward navigation and game launch both use it, so there is no separate launch
animation or pulse counter.
