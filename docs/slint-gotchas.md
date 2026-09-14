# Slint Gotchas

Read this before writing or reviewing `.slint` views or the frame loop. Most
of it follows from one fact: on MiSTer every frame is rasterized on a dual
Cortex-A9 by Slint's software renderer, with the app owning the frame loop
(`rust/frontend/src/mister/platform.rs`).

## Software-renderer animation costs

### Mental model: painted area dominates, animation choice is downstream

Frame cost on the software renderer is roughly **painted pixels per frame ×
per-pixel cost**. The animation type matters less than people expect. What
matters is what each choice does to that product:

1. **How big is the dirty region?** The MiSTer window uses
   `RepaintBufferType::ReusedBuffer`, so Slint redraws only what changed.
   Animating a 20×20 scroll thumb dirties 400 pixels. Animating a full-screen
   overlay dirties about 2 M pixels at 1080p. Same property, 5000× the cost.
2. **What is *in* the dirty region?** A solid fill is cheap. Shaping text,
   scaling an image, or compositing a stack of tiles is not. A "small" tween
   over expensive content is still expensive.
3. **Does anything underneath get skipped?** Assume not. Plan as if every
   item intersecting the dirty region is redrawn, including items covered by
   an opaque sibling, and measure on hardware before relying on occlusion.

So when picking a transition, don't ask "should this fade, slide or scale?".
Ask **how many pixels of expensive content this marks dirty per frame**, and
pick whatever keeps that small.

Two follow-on rules:

- **Moving a small item is cheap; moving a band is not.** Moving one tile face
  by a pixel dirties that tile. Moving a row of 12 tiles dirties the whole row
  every frame.
- **A full redraw is a full redraw.** Anything that forces
  `RepaintBufferType::NewBuffer` (a resolution switch, a presenter reset)
  repaints the entire frame regardless of what is animating. Native 1080p full
  frames cost on the order of 100 ms on the A9.

### What the frame loop already does about it

- **Dynamic resolution scaling** (`rust/frontend/src/drs.rs`). The router brackets work it
  knows repaints the whole viewport for a sustained stretch (page swoops) with
  `drs::heavy_begin`/`heavy_end`, and the MiSTer loop drops to motion
  resolution only inside that bracket. Ordinary navigation never switches:
  small dirty regions are cheap at native resolution, and a switch on a quiet
  screen is a visible snap. Pair every `heavy_begin` with exactly one
  `heavy_end`.
- **Cached page transitions** (`rust/frontend/src/frame_transition.rs`). A page slide
  renders each endpoint once and moves pixels between the two cached frames,
  instead of traversing the component tree every frame.
- **Held-key paging cuts.** Qualified hold-repeats change pages without a
  slide; a slide already running finishes first. Ordinary taps keep the slide.
- **Rapid scroll goes quiet.** While pages flip in a chain, tiles paint only
  their plate (`quiet` in `ui/tiles.slint`): no art, caption or heart, so cover
  work stops and the letter badge has a calm backdrop.

### Cheat sheet

| Cheap on the software renderer | Expensive on the software renderer |
|---|---|
| Instant cut plus a small one-shot cue (tile press, row flash) | Translucent overlays fading over a grid or list |
| Moving one small item (a tile face, a cursor rail) | Moving or fading a band of tiles at native resolution |
| Hard color cuts on one small element | Animating `opacity` on a parent with many children |
| Static scenes with one small property changing | Continuous animation over busy content |

### Transforms and effects

- The software renderer has no transform support: `transform-scale` and
  `transform-rotation` are silently ignored there. That is why `Motion.focus-zoom`
  is pushed from Rust (100% wherever transforms do not apply) rather than
  fixed, so nothing reserves room for growth that never happens. Never make a
  transform carry meaning; the focus ring is the focus cue.
- No blur, no shader-like effect, and no subtree grab to fade. There is no way
  to dim a frozen grab of a subtree, so rapid scroll drops tiles to the `quiet`
  plate instead.

### Sanctioned one-shot cues

The rule bans **persistent** motion that runs every frame while content is
busy (for example a scale held on every focused tile across every move). It
does not ban short one-shot cues on one small element at a state change:

| Cue | Why it is cheap |
|---|---|
| Tile physical press on activate or launch (`rust/frontend/src/press_feedback.rs`: 34 ms downstroke plus a short hold) | One opaque face moves; the dirty region is one tile |
| List, settings and menu row inverse blink on activate (`Motion.press-ms`) | The selected row swaps fill and ink, then swaps back: two repaints, nothing moves |
| Held tile blink in Hub Move mode (`Motion.held-blink-ms`) | One tile, a hard on/off cut, only while a Move session holds it |

The shared constraint: the scene around the cue must be static. If content
may be busy (rapid scroll, a page fill landing), collapse the cue to instant.

With Reduce motion on, press feedback dispatches synchronously instead of
waiting for the cue.

### The one continuous exception: ProgressTrack's leading-cell blink

`ProgressTrack` in `ui/chrome.slint` (the header status line's segmented
progress bar) blinks the cell at the fill's leading edge, or the marching cell
when the total is unknown, for as long as a background task runs. It is the
only cue in the app that repeats:

- Header chrome only, never painted over a grid or list.
- The dirty region is one small cell.
- It is a hard cut, not a fade: a `Timer` flips a bool every `Motion.pulse-ms`
  and the cell color reads it directly. Nothing interpolates.
- It stops when the task is idle or paused, and under Reduce motion. A stopped
  blink leaves the cell lit rather than frozen dark.

The "persistent motion" ban is about scale and location, not repetition. One
small cell blinking in otherwise static header chrome was never the expensive
case. See `docs/style.md` → "Header status line".

## Motion tokens and Reduce motion

Every animation duration goes through the `Motion` global in `ui/theme.slint`.
Never hardcode a duration inline:

```slint
// Good
animate x { duration: Motion.dur(Motion.settle-ms); }

// Bad: ignores Reduce motion and cannot be tuned from one place
animate x { duration: 140ms; }
```

`Motion.dur(ms)` returns `ms` when `Motion.enabled` is true and `0ms` when it
is false, so animations resolve in one frame with no extra branches. Rust sets
`Motion.enabled` from `display::motion_enabled`: off under Reduce motion, and
off on MiSTer's HDMI output at 1080p or taller, where full-frame motion is too
expensive for the A9.

| Token | Value | Use |
|---|---|---|
| `focus-ms` | 80 ms | Focus moves |
| `press-ms` | 34 ms | Press downstroke and row inverse blink |
| `settle-ms` | 110 ms | Release leg, toggle knob |
| `pulse-ms` | 250 ms | ProgressTrack blink on/off time |
| `zoom-ms` | 160 ms | Focus zoom where transforms apply |
| `held-blink-ms` | 650 ms | Hub Move held-tile blink cycle |

`press-ms` has a one-frame floor at the slowest target (about 30 fps, so
33.3 ms rounded up to 34): it is also the time a deferred Accept waits so the
cue gets at least one presented frame before a forward route replaces it.
Don't drop it below that floor, and there is no perceptual reason to raise it
toward `settle-ms`.

## Loading cues

- A forward route keeps the source screen on screen and adds feedback only when
  the destination is slow: `router::begin_pending` shows "Loading…" in the
  header status line after 300 ms. Never cover the source with a scrim.
- In-screen loading, empty and error cues use `StateCue` in `ui/chrome.slint`,
  which carries a 300 ms delay and a 200 ms minimum visible hold so a fast fill
  never flashes the cue.

## Geometry

- Let Slint snap geometry, images and text to physical pixels. Rounding by hand
  is reserved for exact-pixel contracts (CRT calibration guides, bitmap raster
  sizes, QR modules). See the rule in `AGENTS.md`.
- Tokens come from the `Sizing` and `Layout` globals, which Rust pushes from
  `zaparoo_app::sizing` and `zaparoo_app::layouts` on every scene change. The
  golden fixtures under `rust/zaparoo-app/tests/fixtures/` pin those rules.
- CRT output, and MiSTer outputs under 400 px tall, use the bitmap font
  (`Sizing.bitmap-fonts`, from `display::bitmap_type`), which has only two
  usable sizes; everything else uses the semantic type ladder.
