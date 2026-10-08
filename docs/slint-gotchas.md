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
- **Fast scroll never waits on art.** While a hold runs fast, no new cover
  loads start (the MiSTer cannot fetch and decode covers as fast as pages
  pass) and the list detail pane peeks without loading. Tiles keep their
  captions, hearts and any art already in memory. The fast-scroll rail
  (`FastScrollRail` in `ui/app.slint`) is the only new painting: a narrow
  strip whose highlight moves between letters, with `Motion.rail-ms` at 0 on
  the software renderer so it cuts instead of fading.

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
  to dim a frozen grab of a subtree.

### Sanctioned one-shot cues

The rule bans **persistent** motion that runs every frame while content is
busy (for example a scale held on every focused tile across every move). It
does not ban short one-shot cues on one small element at a state change:

| Cue | Why it is cheap |
|---|---|
| Tile physical press on activate or launch (`rust/frontend/src/press_feedback.rs`: 90 ms feedback window with a 34 ms downstroke) | One opaque face moves; the dirty region is one tile |
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
33.3 ms rounded up to 34). Accept waits through a shared 90 ms feedback window
(`zaparoo_app::input::PRESS_FEEDBACK_MS`): the downstroke plus a short depressed
hold before the action can replace its control. Launches and pending routes
retain that press until completion. Disabled motion skips the wait, and
interrupted or changed targets cancel the pending Accept. Don't drop the
downstroke below its one-frame floor or stretch it toward `settle-ms`.

## Loading cues

One timing rule, three places, nothing else.

- **Timing.** Every cue that says something is loading, saving, starting or
  searching waits 300 ms before it appears and then stays at least 200 ms, so a
  quick answer shows no cue and a slow one never flashes a word.
  `zaparoo_app::wait_cue` holds the rule and the two numbers; `Motion.cue-delay-ms`
  and `Motion.cue-hold-ms` are the same numbers for the views. The boot curtain
  is exempt (it is the whole screen from its first frame), and so is a
  confirmation such as "Added to Hub", which is not a wait and shows at once for
  `CONFIRM_MS`.
- **Content is on screen: the header status line.** A forward route
  (`router::begin_pending`), a list's next page or a walk to a distant row, a
  list reloading in place, a launch, and an action that waits on Core after its
  menu has closed all put their word in the header through `crate::cue`. The
  content stays where it is: never cover it with a scrim, and never swap a cue
  in for the selected item's name or the position counter. A reload in place
  keeps its old rows until the new ones land.
- **Nothing to show yet: the body.** `WaitCue` in `ui/chrome.slint` is the
  hourglass and a line of text, centered. `StateCue` uses it for a screen with no
  rows, and a modal uses it directly for a body that is still loading.
- **A row that is waiting on its own action** ("Saving…" on the launcher just
  picked) relabels itself through `cue::begin_local`, on the same timing.
- **Name what is loading** ("Loading sections…", "Launching Tetris…"), and say
  how far where a count exists ("Loading 1,000 of 5,000…"). A static hourglass
  cannot show that anything is still moving; the words have to.
- **No wait is forever.** `Client::call` gives up 35 seconds after sending,
  just past Core's own 30-second request deadline, except for the calls Core
  itself leaves unbounded (backup, restore, update). The failure then takes the
  screen's ordinary error path.

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
