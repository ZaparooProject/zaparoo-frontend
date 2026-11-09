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
- **Picture mode** (`drs::set_picture`). The screensaver's full-screen
  styles go one step further: the latch presenter renders at half its
  geometry on each axis (480x270 under a 960x540 scene) with the window
  scale factor halved, so the layout does not move, and the HDMI scaler
  stretches the frame. The switch lands on the saver's black backdrop as
  it arms and on wake. Presenters without a scaler (fb0, the native CRT)
  ignore it.
- **Cached page transitions** (`rust/frontend/src/frame_transition.rs`). A page slide
  renders each endpoint once and moves pixels between the two cached frames,
  instead of traversing the component tree every frame. Every MiSTer
  presenter runs it (vblank latch, native CRT, fb0) through the shared
  `CachedSlide` in `rust/frontend/src/mister/transition.rs`, in every
  orientation: the band and its direction of travel are mapped into the
  rotated frame. A list layout has no band and still refuses, as does the
  vblank-latch presenter while it holds a dynamic-resolution pair. In dual
  head the HDMI presenter owns the slide and the CRT head replays the page
  as a live strip.
- **One page turn for every paged grid.** The Hub, Systems, the games grids
  and the file browser all turn pages through `rust/frontend/src/page_motion.rs`
  (the cut rules, the cached-slide request, the settle and re-arm timers,
  interrupting a turn) and draw it with `PageStrip` in `ui/tiles.slint`. A
  screen plugs in with a `PagedScreen` impl and keeps only its own render.
  Every grid cuts while a held direction runs fast. The one place the
  screens still differ is `LIVE_STRIP_ON_HDMI` on that impl: with no cached
  slide on MiSTer HDMI (only under the `slides` motion switch or
  `--adaptive-render`), the Hub cuts while the other grids slide live. The browse lists share
  their row window and glide the same way (`browse_motion::publish_list`).
  Do not give a new grid screen its own copy.
- **A fixed-step animation clock** (`zaparoo_app::frame_clock`, driven from
  `rust/frontend/src/mister/platform.rs`). The clock Slint's timers and
  animations read moves one refresh period per presented frame, so motion
  is sampled at even steps and a late frame slows it by that frame instead
  of enlarging the next step. The slowing is bounded: once a run of
  presented frames is `FrameClock::MAX_LAG` (100 ms) behind real time, the
  clock steps enough whole periods to stay within it, so frames that stay
  slow under load (an index running) play motion at real speed instead of
  stretching an 80 ms focus glide over a second. The native CRT presenter reports its period
  (16.667 ms, 20 ms on PAL); the HDMI presenters do not know theirs, so the
  platform measures it from the vertical-blank waits of turns that drew
  nothing (`RefreshEstimate`) and a 50 Hz mode steps 20 ms. A turn that
  presents nothing adds the real time that passed, in whole periods, so
  timers keep real time while nothing animates. Cached page slides count
  their steps on the same clock. The frame profiler and the key path's
  duplicate guard keep their own wall-clock and kernel stamps.
- **Held-key paging cuts.** Qualified hold-repeats change pages without a
  slide; a slide already running finishes first. Ordinary taps keep the slide.
- **Browse lists follow the selection by step size.** `BrowseList` scrolls
  through `ScrollOffset` (`ui/focus.slint`), and
  `zaparoo_app::input::InputModel::list_follow` picks how from the rows the
  selection just moved. A tap, one row or a page, is an eased glide over
  `Motion.scroll-ms`. A held walk of one row per repeat is a constant-speed
  glide that lasts a quarter longer than the repeat interval, so the list
  keeps moving between repeats instead of easing to a stop on each one. A
  held step of more than one row (the Games list paging or jumping letters)
  cuts. The driver pushes the choice as `list-glide` and `list-step-ms`
  before the offset it applies to, and only on a render that moved the
  selection, so a glide in flight is left alone. The list card repaints
  every frame of a glide. The list paints no placeholder square behind a
  cover that has not landed; the cover still fades in.
- **Fast scroll never waits on art.** While a hold runs fast, no new cover
  loads start (the MiSTer cannot fetch and decode covers as fast as pages
  pass) and the list detail pane peeks without loading. Tiles keep their
  captions, hearts and any art already in memory. The fast-scroll rail
  (`FastScrollRail` in `ui/app.slint`) is the only new painting: a narrow
  strip that fades in and out over `Motion.rail-ms` and whose highlight
  glides between letters. Where `Motion.rail-ms` is pushed to 0 it cuts
  instead.

### Cheat sheet

| Cheap on the software renderer | Expensive on the software renderer |
|---|---|
| Instant cut plus a small one-shot cue (tile press, row flash) | Translucent overlays fading over a grid or list |
| Moving one small item (a tile face, a cursor rail) | Moving or fading a band of tiles at native resolution |
| Hard color cuts on one small element | Animating `opacity` on a parent with many children |
| Static scenes with one small property changing | Continuous animation over busy content |

### Background images

The screen background is one opaque RGB image exactly the window's size,
drawn first (`Backdrop` in `ui/theme.slint`). Everything that makes it look
right happens once, off the event loop, in `zaparoo_app::background`: crop,
blur at a reduced size, smooth upscale, dimming, the help bar band, and an
ordered dither to RGB565 values Slint's truncating conversion maps back
exactly. With glass cards a second copy is made at the card's darkness.

- **Never bake where a card sits.** The card's position is only known
  once its screen is showing, and a prepare takes 70 to 300 ms on the A9:
  baking it in left a solid card on the way into a screen and a dark one
  on the way out. The card-darkness copy shows inside whichever card the
  view reports (`GlassCard`), in the same frame; the presenter merges the
  two bitmaps with a row copy per card row when the card moves
  (`set_cutout`).

- **On MiSTer the presenter paints it, not Slint.** Slint draws an
  `Image` through its generic texture loop, a per-pixel gather of about
  20 cycles on the A9 even at 1:1. Under a glass card, which
  repaints its whole area on every scroll frame, that cost about 9 ms a
  frame. The presenters instead render through
  `render_into_buffer` with a `TargetPixelBuffer` whose
  `fill_background` copies each dirty row straight from the bitmap
  (`mister::transition::render_with_backdrop`); Slint then draws the
  items over the real background pixels, and the view drops its `Image`
  (`Backdrop.host-painted`). A new bitmap repaints the whole frame once.
  This needs `i-slint-renderer-software`'s `experimental` feature, so
  check the trait when bumping Slint. A rotated frame keeps the `Image`.
  Desktop and hosted builds keep the `Image` too; the GPU does not care.

- **No per-pixel division.** The `MiSTer`'s Cortex-A9 has no integer divide
  instruction: a `/` or `%` in a pixel loop is a library call. The first
  version of this pipeline was 50 to 100 times slower on the device than on
  a desktop for that reason. Multiply by a fixed-point reciprocal, walk
  rows, index dither tables with masks.
- **Page slides keep the background still.** A cached slide moves raw
  pixels, which would carry the wallpaper with the page. Over an image, the
  presenter compares each page row with the background bitmap it was given
  (`mister::transition::set_background`, converted the way Slint draws it)
  and moves only the runs that differ; the gap and everything else is the
  background in place. Partly transparent edges count as content and carry
  a sliver of background for the length of the slide; the last step is
  Slint's own frame again.
- **Changing the background is a full repaint.** On the software renderer
  it cuts, once focus has rested; the GPU build crossfades
  (`display::background_crossfade`).

### Transforms and effects

- The software renderer has no transform support: `transform-scale` and
  `transform-rotation` are silently ignored there. The focus zoom is the one
  effect that needs one, so where that renderer draws (Rust sets
  `Motion.zoom-by-size`: the MiSTer build and the snapshot binary) the
  focused cell and its ring grow by their real `x`, `y`, `width` and
  `height` about the same center instead, to the rectangle the transform
  would cover, in one cut with no animation. The card and its art are sized from the cell and grow with
  it; type, padding and stroke widths keep their size, and glyphs are
  rasterized for the size the tile settles at.
  `Motion.focus-zoom` is pushed from Rust with every scene rather than
  fixed: 100% turns the growth off, and then nothing reserves room for
  growth that never happens. The 240p sizing tier is pushed 100%
  (`zaparoo_app::sizing::focus_zoom_percent`): a tile that small grows by a
  pixel or two, which reads as a jitter.
  Never make a transform carry meaning; the focus ring is the focus cue.
- The software renderer samples bitmaps nearest-neighbor. An image fitted
  to a box that is not its own pixel size loses or repeats rows and columns:
  uneven strokes, stepped diagonals. So where that renderer draws
  (`Motion.zoom-by-size`), tile glyphs and system logos are prepared for the
  whole pixels of the box that paints them and painted at their own pixel
  size, centered on a whole pixel, never fitted. One rule gives both sides
  the box: `zaparoo_app::sizing::tile_art_pixels` mirrors `Tile`'s art box,
  the list detail pane reports its own, and `GlyphSource.side` rounds a
  square glyph's box to the size it is rasterized at. Logo bounds are exact
  there (`logo_cache::Bounds::exact`) instead of bucketed, and logos are
  downscaled with the brand logo's premultiplied Lanczos3 filter. The
  focused tile paints a second copy prepared for its grown art box, where
  it grows at all; until that copy is ready it paints the resting copy at
  that copy's own size.
  Media covers and user images are still fitted to the box.
- No blur, no shader-like effect, and no subtree grab to fade. There is no way
  to dim a frozen grab of a subtree.

### Motion on the software renderer

The MiSTer build runs most of the GPU build's motion: the focus zoom's
resting size (as a cut, not a scale), the cover fade where it is affordable,
the fast-scroll rail's fade and glide, and page slides on every presenter.
The rules above about
what is cheap and what is expensive still decide what may be added; what
each of these costs on hardware is still to be recorded. Until it is, each
one can be turned off for a run with the `ZAPAROO_MOTION` environment
variable, a comma-separated list of names, so it can be measured against
the cut it replaced. Unset or empty leaves everything on.

| Name | Effect when on | With the name listed |
|---|---|---|
| `zoom` | Above the 240p tier, the focused tile and its ring cut to `Motion.focus-zoom`; a focus move repaints the two cells involved once | `Motion.focus-zoom` is 100%: no growth, no clip headroom, no larger scrim hole |
| `cover-fade` | Cover art that lands while it is on screen fades in: a list's detail pane over `Motion.cover-reveal-ms` in Slint, a grid tile in the presenter (see below) | Both cut in |
| `rail` | The fast-scroll rail fades over `Motion.rail-ms` and its highlight glides | `Motion.rail-ms` is 0: the rail appears, moves and leaves in cuts |
| `slides` | The native CRT and fb0 presenters run cached page slides | Only the vblank-latch presenter does; CRT slides its live strip, and on fb0 the Hub cuts while the browse grids slide theirs |
| `clock` | The fixed-step animation clock | Wall time drives timers, animations and cached slides, and a late slide frame jumps to the current step |

Grid tiles fade art from Core in, its cover and the placeholder colour
that stands in for it (`GridCell.fetched`); built-in logos and glyphs cut
in. On the software renderer the presenter does the fade, not Slint
(`display::tile_cover_fade` pushes `Motion.tile-cover-reveal-ms` to 0). A
page of covers lands together, and a Slint opacity fade repaints each
fading tile every frame and the area enclosing them: it redrew most of a
960x540 grid for its whole length, and on hardware a fifth to a quarter of
rendered frames missed the 16.7 ms budget. Instead Slint paints each cover
once, as a cut. The grid driver hands the cells whose covers just landed to
`cover_fade::request`; the presenter keeps the pixels that were under them
before that render, narrows each to the pixels the cover changed, and for
the length of the fade mixes the old pixels into each row it publishes
(`frame_transition::CoverFades`). Slint keeps rendering everything else.
A fade ends early, showing what Slint painted, when Slint repaints its
pixels with something else (the focus ring arriving, a page change, a
modal), when a page slide starts, and when the frame changes size. Only
the CRT head of a dual-head pair keeps the Slint fade: its presenter
mirrors the other head's state, not its pixels.

The switch only acts on the MiSTer build. `zaparoo_app::motion_test` parses
it and `lib.rs` reads it once at startup, logging any name it does not know.

### Sanctioned one-shot cues

The rule bans **persistent** motion that runs every frame while content is
busy (for example a scale held on every focused tile across every move). It
does not ban short one-shot cues on one small element at a state change:

| Cue | Why it is cheap |
|---|---|
| Tile physical press on activate or launch (`rust/frontend/src/press_feedback.rs`: 50 ms feedback window with a 34 ms downstroke) | One opaque face moves; the dirty region is one tile |
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

The music player's progress row is a `ProgressTrack` across its card with
the blink held off (`paused`): it reports a place in a song rather than
work under way, so nothing blinks. Its cells and the time text change at
most once a second while music plays (`zaparoo_app::music::TICK_MS`), a
cell at a time and each a hard cut over a few dozen pixels in an
otherwise still card, and only while the player is on screen. Everything
else on the screen changes only when Core reports something or focus
moves.

### Screensaver costs

The screensaver is the one place motion runs for as long as nobody touches
the controls, so each style is built around what the software renderer
repaints, not around what looks busiest:

- **Sprite styles repaint only their movers.** Bouncing, Spinning logo,
  System logos, Recent games, Paddle clock, Starfield, Flyby and Snakes
  publish small sprites through `Saver.sprites` over the black backstop.
  A frame's dirty region is the union of what moved: one logo, one line,
  three paddle-game pieces, the stars and up to two logos (Starfield
  keeps 80 stars, 60 at 240p), at most six fliers (Flyby). Stars and
  fliers spread over the whole screen, so Starfield and Flyby repaint
  close to all of it and run like the picture styles below. Snakes keeps
  a fixed set of segment slots per snake and rewrites one slot per snake
  a step (the tail's segment becomes the head's), so a step repaints two
  cells per snake.
  Cover wall changes one tile every few seconds.
- **Full-screen styles render at half resolution.** Polygons, Life and
  Maze render whole frames in Rust into a pixel buffer (`Saver.frame`)
  that the view scales to the screen. Polygons started as Slint `Path`
  outlines: twelve screen-sized paths cost about 136 ms a frame on the
  MiSTer, so it draws its lines in Rust instead. Painting a full-screen
  picture into a 960x540 scene cost about 35 ms whatever was in it, so
  while one of these or Starfield or Flyby plays, `drs::heavy_begin`
  holds the renderer at its motion resolution and `drs::set_picture`
  halves it again (picture mode, above), which brings every one of them
  to about 15 ms. The run clears picture mode and calls `heavy_end`
  exactly once when it disarms. Life's board is 90 rows,
  the Maze's picture 180 lines and Polygons' 270; in a 240p scene Life
  uses four-pixel cells and the others draw at the scene's own lines.
- **Reduced rates.** Each style steps on its own Rust timer, never faster
  than it needs: 30 fps for Spinning logo, Recent games and Paddle clock,
  20 for Starfield, Flyby, Polygons and Maze, 12 for Snakes and Life, 10
  for System logos (its fades animate at the display rate), and 4 for the
  Cover wall. Bouncing keeps its own
  view-side animation. A late tick advances by at most a quarter second.
- **The dual-head CRT shares the HDMI run.** It does not run a second
  simulation or render a second buffer: `dual_head.rs` copies the `Saver`
  global, and the CRT view spreads the scene from its center by
  `Saver.aspect`, so a 16:9 scene on a 4:3 tube keeps its middle.
  Polygons, Life and Maze frames stretch to fill the tube
  (`image-fit: fill`).

Measured on the MiSTer test device at 1080p HDMI output (540p render),
steady state after the first 600-frame profile window, in milliseconds
per rendered frame:

| Style | Average | 99th percentile | Step |
|---|---|---|---|
| Bouncing | 4.7 | 7.0 | view animation |
| Spinning logo | 6.4 | 12.8 | 33 |
| System logos | 11.2 | 17.4 | 100 (fades at display rate) |
| Recent games | 5.0 | 8.7 | 33 |
| Paddle clock | 6.2 | 10.4 | 33 |
| Starfield | 15.3 | 18.7 | 50 |
| Flyby | 10.0 | 13.9 | 50 |
| Polygons | 15.7 | 19.4 | 50 |
| Maze | 15.3 | 18.0 | 50 |

Starfield, Flyby, Polygons and Maze run in picture mode; before it they
cost 25.2, 17.9, 38.2 and 38.0 ms on average. Cover wall and Life render
too few frames to fill a profile window.
Every style shows the same isolated 100 to 300 ms spikes the rest of the
UI does.

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
| `scroll-ms` | 120 ms | A list or menu scrolling to follow one press |
| `page-ms` | 200 ms | Page slides, paged text bodies |
| `zoom-ms` | 110 ms | Focus zoom, where a transform draws it |
| `cover-reveal-ms` | 120 ms | Cover art fading in as it lands in a list's detail pane |
| `tile-cover-reveal-ms` | 180 ms, 0 where the presenter fades covers | Art from Core fading in on a grid tile |
| `rail-ms` | 160 ms | Fast-scroll rail fade |
| `held-blink-ms` | 650 ms | Hub Move held-tile blink cycle |

`press-ms` has a one-frame floor at the slowest target (about 30 fps, so
33.3 ms rounded up to 34). Accept waits through a shared 50 ms feedback window
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
  sizes, QR modules, bitmap-font labels). See the rule in `AGENTS.md`.
- The software renderer does not snap text: it places glyphs at quarter-pixel
  positions, which smears the bitmap face. Under `Sizing.pixel-grid` every
  label starts on a whole pixel, through `Sizing.snap` and `Sizing.even`;
  `just crt-text-check` enforces it. See `docs/style.md` → "Integer-pixel
  drawing".
- Tokens come from the `Sizing` and `Layout` globals, which Rust pushes from
  `zaparoo_app::sizing` and `zaparoo_app::layouts` on every scene change. The
  golden fixtures under `rust/zaparoo-app/tests/fixtures/` pin those rules.
- CRT output, and MiSTer outputs under 400 px tall, use the bitmap font
  (`Sizing.bitmap-fonts`, from `display::bitmap_type`), which has only two
  usable sizes; everything else uses the semantic type ladder.
