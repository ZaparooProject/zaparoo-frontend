# UI Style

Zaparoo Frontend's design language. Tokens live in the Slint globals in
`rust/frontend/ui/theme.slint`: `Theme` owns color and font tokens, `Sizing`
owns geometry, resolution tiers, radii, type roles, and stroke weights,
`Layout` owns the browse layout profile, and `Motion` owns durations. The
rules behind them are toolkit-free Rust in `zaparoo_app::palette`,
`zaparoo_app::sizing`, and `zaparoo_app::layouts`, pushed into the globals by
`rust/frontend/src/theme.rs` and `rust/frontend/src/sizing.rs`. Anything not
covered here defers to those globals.

MiSTer renders through Slint's software renderer with no GPU, so the UI uses
no shaders, shadows, or gradients. Build surfaces from `Rectangle`, `Text`,
and `Image`.

## Color schemes

A preset authors exactly **three** colors and `zaparoo_app::palette`
(`rust/zaparoo-app/src/palette.rs`) derives every other role from them,
through a semantic tier (below) before it reaches components:

| Authored | Role |
|---|---|
| `primary` | Page background, and the base every surface mixes from |
| `accent` | Focus, selection, and every highlight in the UI |
| `text` | Primary content color |

There is no fourth *authored* color, presets still author exactly three
hexes. The semantic tier below is derived, not authored, and exists because
state markers (the favorite heart, the Hidden badge) used to ride directly on
`accent` too, a convention that assumes a dark accent and breaks on a
mid-luma one.

Two roles do not derive. `scrim` is always a dark veil, because its job is to
separate a panel from what sits behind it whichever direction the preset runs.
`error` is a semantic constant: deriving it from the accent would make an
amber preset signal danger in amber.

Derivation direction follows `up = luma(text) > luma(primary)`, so light presets
work without a separate ladder. Slint's `color.brighter()`/`color.darker()`
scale the HSV value and do nothing to pure black, so the palette mixes channels
explicitly instead, and components must too.

`marker-outline` (below) does not mix toward `primary`/`text` directly, that
axis is what silently ran backwards on the `zaparoo-light` preset. It mixes
toward `light_pole`/`dark_pole` instead, the same two colors ordered by luma
(`up ? text : primary` / `up ? primary : text`), so the outline is always the
pole opposite the marker's own luma regardless of which of `primary`/`text`
happens to be light on a given preset.

### Semantic tier

Standard token pipelines run primitive → semantic → component. Three semantic
roles sit between the three authored hexes and the ~20 component roles:

| Role | Job |
|---|---|
| `on-accent` | Body text, glyphs, and control fills sitting on the solid selection fill: the inverted row content, the favorite heart on a selected row, the toggle track on a selected row |
| `on-accent-muted` | Subordinate on-accent content, tag suffixes, the "off" toggle track on a selected row |
| `marker` | Fixed-hue state marker, the favorite heart and the Hidden badge, kept independent of `accent` so a favorited tile can't blend into the focus ring |

`on-accent` starts from whichever of the preset's own `primary`/`text` already
contrasts better against `selection-fill` (the accent at reduced chroma; see
"Colors" below), then walks that color's OKLCh lightness (holding its own
chroma and hue) until it clears 4.5:1, so it stays tied to the preset's
identity rather than snapping to flat black or white.
`on-accent-muted` is the same hue/chroma walked toward `accent` until contrast
lands in the 3.0–4.5 band. `marker` is a fixed warm-red OKLCh hue (~29°,
matching the iOS-red/heart convention), rotated in 90° steps away from
`accent`'s own hue when they would collide within 40°, with lightness fitted
per preset for ≥3:1 against `surface-card`. It is a semantic constant rather
than a derived hue for the same reason `error` is: deriving it from the
accent would make an amber preset signal "favorited" in amber.

### OKLCh derivation ladder

The neutral surface ladder (`bg-panel`, `surface-card`, `border-subtle`,
`border-mid`, `text-variant`, the pressable edges) stays on the old per-channel
sRGB `mix()`, it only mixes near-neutrals, where the error is small. Anything
that carries the accent's own hue, the two logo ramps, `on-accent`,
`on-accent-muted`, `marker`, is derived in OKLCh instead (Björn Ottosson's
perceptually uniform space), holding chroma and hue fixed while only
lightness moves. sRGB per-channel lerp does not preserve chroma: mixing a
saturated accent toward a near-black or near-white pole desaturates it along
the way, which is what made a focused amber tile read near-white and a
focused blue tile read closer to brown than orange before this ladder existed.
Gamut-fitting (`gamut_fit` in `palette.rs`) reduces chroma via binary search at a
fixed lightness/hue when a requested OKLCh coordinate falls outside sRGB,
lightness and hue are never altered, only chroma, and only as far as needed.

**Guardrail.** `clamp_accent(accent, primary)` walks an authored (or, later,
user-supplied) accent's OKLCh lightness away from `primary` until it clears
4.5:1, preserving hue and chroma. Inert for every shipped
preset, they already clear this floor, but is what makes the whole ladder
safe to seed from an arbitrary hex rather than only the curated ones, ahead of
any future custom-palette config surface.

`rust/frontend/src/theme.rs` pushes the derived roles into the `Theme` global,
which is what components read. Never branch on scheme ID in
a component or hardcode a preset color outside the catalog. `zaparoo-dark` is
fallback for missing, unknown, or removed IDs.

### Preset catalog

19 presets ship. **Zaparoo Dark**, **Zaparoo Light**, and **Classic Purple**
are the three original Zaparoo themes (renamed from `zaparoo-black` /
`zaparoo-white` / `midnight-amber` in round 6 so the id describes the
preset rather than an implementation detail; Classic Purple's triad was
further retuned in round 7, see below). **Nord**, **Dracula**, **Synthwave
'84**, **Gruvbox**, **Everforest**, **Solarized Dark**, **Rosé Pine**, and
**Oxocarbon** cover the editor/terminal world. **Amber Phosphor**, **Green
Phosphor**, **Neo Geo**, **NES**, **Virtual Boy**, and **Game Boy** are
documented retro/console references. **Flexoki Paper** and **Solarized
Light** join Zaparoo Light as the light-register options. None of these are
invented colors, every triad is `primary`/`accent`/`text` picked from that
theme's own published palette, with two exceptions where a real value was
deepened to clear a guardrail: Game Boy's background sits below the real
DMG value so text/bg clears 7.0:1, and its text uses the real DMG palette's
lightest tone (`#9bbc0f`) rather than its second-lightest, which additionally
lets it clear `text-primary`/`border-mid`'s 4.0:1 floor; Everforest's
background sits a few steps below its own published `bg_dim` "hard" tone
(`#1e2326`) for the same `text-primary`/`border-mid` reason, `#1e2326` itself
falls just short at 3.98:1.

**Round 5 shipped 24 presets; round 6 pruned to 11; round 7 grew back to
19.** Round 6 kept Zaparoo, phosphor, and console presets unconditionally as
the identity and differentiated end of the catalog, and cut everything else
that read as a near-duplicate of one of them: `catppuccin-mocha`/`-macchiato`/`-frappe`
(next to Dracula, same dark-purple-on-slate register), `tokyo-night`,
`one-dark-pro`, `nightfox`, and `kanagawa-wave` (all blue-accent-on-slate,
next to Nord), `monokai` and `gruvbox-dark` (no console/phosphor counterpart
distinct enough to earn a slot at the time), `rose-pine` and
`everforest-dark` (same reasoning), and `ayu-dark`.

Round 7's problem was the opposite: the round-6 survivors fixed redundancy
but left several presets reading as near-duplicates of *each other*
(Classic Purple/Dracula/Nord/Synthwave '84 are all "dark bg + one accent,"
and Neo Geo/NES/Amber Phosphor cluster the same way), with no room for a
preset whose *background register* differs rather than just its accent hue.
Round 7 re-added Gruvbox and Everforest (this time judging them as
distinct, Gruvbox's neutral warm-grey background and Everforest's
blue-green slate are backgrounds nothing else in the catalog covers, not
just new accent hues on the same near-black canvas Neo Geo/NES/Amber
Phosphor already occupy), added Solarized Dark (deep teal-black, its own
register) and Rosé Pine (purple-black bg, but a rose-pink accent rather
than Dracula's violet), added Oxocarbon (true neutral near-black + electric
cyan, a modern/SaaS register with no precedent in the catalog), and added
Game Boy as a third retro-console preset. Two accent-hue near-collisions
were accepted rather than treated as automatic disqualifiers, matching how
the existing catalog already tolerates Classic Purple/Amber
Phosphor/Neo Geo sharing the amber-orange family: Oxocarbon's cyan sits
close to Solarized Dark's, and Gruvbox's orange sits between the
NES/Virtual Boy red-oranges and the Classic Purple/Amber Phosphor/Neo Geo
amber-oranges. In both cases the background register (not the accent hue)
is what earns the slot, so a nearby accent hue on a genuinely different
background was judged as distinct rather than redundant.

`gruvbox-light` cleared every guardrail in round 6 but was cut anyway: on a
light page the accent must sit dark to clear `clamp_accent`'s 4.5:1 floor,
so its selected row lands on `#af3a03`, cream text at 5.4:1, technically
passing but reading as a heavy brown next to near-black body text. That
tension is structural to light presets with warm accents, not a tuning miss
that a different hex would fix, and round 7 didn't find a fix for it either
Gruvbox Light still doesn't ship. Zaparoo Light's cool blue (`#0a63c9`,
5.19:1) carried the light-preset slot alone through round 6.

Two guardrail floors were relaxed in round 5, kept relaxed since:

- `text-primary`/`surface-card` contrast: AAA 7.0:1 → AA 4.5:1.
  `text-primary`/`bg-deep` stays at 7.0, the primary background is the
  highest-traffic surface and keeps the stricter floor; only the *card*
  surface (mixed partway toward text/accent) relaxes. This is a legitimate
  AA guarantee in its own right, not a number tuned to a handful of
  presets.
- The focus ramp's *primary*-rung chroma-retention floor: 45% of the
  accent's own OKLCh chroma → 33%. The *shadow*-rung floor stays 55%, sRGB's
  own gamut holds less chroma at high lightness regardless of color space,
  which is why the light end needed the lower floor and the dark end didn't.
  Dracula's purple (~39%) and Synthwave '84's pink (~38%) both still rely on
  this relaxation.

Round 6 documented Solarized (both variants), Catppuccin Latte, Everforest
Light, Rosé Pine Moon/Dawn, and Night Owl as unable to clear the catalog
even with those two floors relaxed. Round 7 revisited that list and found
the real blocker for several of them wasn't the palette itself but which of
the theme's *own* published colors was picked for `text`: Solarized's
default body text (`#93a1a1` dark-on-dark, `#586e75` light-on-light) is
deliberately soft for reduced eye strain, and that softness is exactly what
fails `text-primary`/`bg-deep`'s 7.0:1 floor. Substituting Solarized's own
higher-contrast tone, `base2`/`base02` on the dark variant, `base01` on the
light variant, both still colors Solarized itself publishes, not invented
ones, clears every guardrail with room to spare (12.3:1 and 12.1:1
respectively). The same substitution (a theme's own darker ink in place of
its default body text) is what unlocked Flexoki Paper. Catppuccin Latte,
Everforest Light, Rosé Pine Moon/Dawn, and Night Owl were not revisited in
round 7 and remain out, each fails a *different*, deeper guardrail
(`clamp_accent`'s 4.5:1 floor, which would silently mutate the authored
accent away from its real hex; the `text-label`/`bg-deep` 3:1 floor; or
`tile-edge` failing to read as more saturated than the card) that would need
its own justified substitution to fix, not just a swapped text tone.
Revisit only deliberately.

Adding or removing a preset touches: `CATALOG` in
`rust/zaparoo-app/src/palette.rs` (its order is the picker order) and the
id-count and named-id assertions in its `catalog_order_is_the_picker_order`
test; the display name in `SettingsLabels`'s `color-scheme` function in
`rust/frontend/ui/settings.slint` (a literal `@tr()` per id so the translation
catalog picks it up); `rust/zaparoo-app/tests/fixtures/palette_golden.txt` and
the case counts in `rust/zaparoo-app/tests/palette_golden.rs`, which pin every
role of every preset at every intensity; and, for a rename specifically, every
`rust/zaparoo-core/src/{config,persist}.rs` test fixture that hardcodes the
old id as an example value (not validated against the catalog, but kept in
sync for clarity). The Settings picker lists `palette::ids()` directly, so it
needs no change. The golden fixture was captured from the retired Qt build,
whose tests enforced the contrast guardrails this section describes; nothing in
the current tests re-checks those floors.

**Round 10 reordered the catalog** from plain addition-history order (each
round's new presets appended at the end, which read as random rather than
deliberate on the picker) into family blocks: the three Zaparoo/identity
presets first, then the six retro/console presets, then the eight editor/
terminal presets, then the two light presets, alphabetical by display
name within each block. No id was added, removed, or renamed; this is a
pure reorder of the catalog.

Selection applies live and persists as `[settings] color_scheme` in
`frontend.toml` plus `state.toml`. Tinted glyph rasters are cached per key,
size, and color, so a scheme change re-rasterizes them; custom and full-color
artwork remains unchanged.

Status icons (`resources/images/status/`), the general UI glyph set
(`resources/images/icons/`), and the controller button and D-pad glyphs
(`resources/images/buttons/<style>/`) are all SVGs. `GlyphSource.glyph(key,
size, color)` in `chrome.slint` hands them to `rust/frontend/src/glyphs.rs`,
which rasterizes each at its exact painted size with the color baked in, so the
glyph is a flat tint rather than a ramp. Every source SVG is authored
white-on-transparent, and a raw white glyph disappears on a light preset whose
`bg-bar` resolves lighter than white; callers pass `Theme.text-primary` (or
another in-scope text/accent role for context, e.g. `Theme.logo-focus-primary`
where a glyph shares a slot with tinted cover art). A fully transparent color
keeps the artwork's own colors, which is how the two-tone favorite heart
renders: `glyphs.rs` maps its fill and keyline to `marker` and
`marker-outline` before rasterizing.

## Two registers

Everything in the frontend is drawn with one of two visual treatments, and
which one a component gets follows from what the component *is*, not from
where it happens to live on screen.

- **Grids and commitments get physical treatment.** Tiles (`Tile` in a
  `PagedGridView`), `LetterJumpModal`'s A–Z keypad cells, and
  standalone commitment buttons (`DialogModal`'s OK / Cancel / No / Yes) are things
  you pick up and press, a raised plate on `PressableSurface`, a chromatic
  front edge, an accent focus ring, a press-down on activation. See
  "Pressable front edge" below.
- **Vertical option lists get typographic treatment.** `BrowseList` rows,
  `SettingsRowView` rows, `ContextMenu` rows, and `ListPickerModal` rows are all
  the same interaction, scan a list, pick one, and are read, not handled: no
  fill or border at rest, and selection is inverse video, the list's
  `SelectionCursor` fill swapping foreground and background rather than lifting
  off the page. See "Inverse-video rows" below.

An earlier version of this split ran the other way: `ContextMenu` and
`ListPickerModal` rows were raised buttons on the reasoning that "a menu entry
*is* a button." That put a Settings row and the `ListPickerModal` row it
opens, literally the same choice, continued, in opposite registers, and cost
real legibility doing it. At 240p, a `ContextMenu` row (`Sizing.pct-h(6)` = 14px)
has 2px of `press-edge-height` plus a 3px border/ring band on each side, leaving
6px of clear face for an 8px bitmap glyph, the label painted *on* the focus
ring rather than inside it, on the app's smallest interactive text. A solid
accent-filled row is also a stronger low-resolution focus cue than a 1px ring
plus a 2px edge. Reclassifying them lost nothing from the app's physical
identity: tiles carry that identity (on screen essentially always, taking the
press-in on every launch), not menu rows next to them, and flat rows next to
a bright anchored tile make the tile read more like the object it is, not
less.

Both idioms come from the same constraint: this is software rendering on
hardware that cannot composite. A raised button is what depth looks like when
all you have are filled rects and two flat colors on adjacent edges. Inverse
video is what selection looks like under that same limit, no tint, no shade,
so you swap the two colors you already have. Neither idiom is decoration for
its own sake; each is the cheapest correct answer to "how does this state
read" given no shaders, no gradients, and no alpha compositing over busy
content.

Resolution, not geometry, does the era-signalling. The row geometry and the
inverse-video mechanic are identical at 1080p and at 240p, what changes is
`Theme.bitmap-fonts` (the 6x8 bitmap face, quantized to 8/16px strikes), which
auto-engages on the CRT path and at 240p on MiSTer. A solid accent bar with near-black
text at 1080p in Noto Sans reads as high-contrast modern chrome; the same bar
in the bitmap face at 240p reads as a DOS-terminal flash. The geometry never
branches; only the font and the pixel density shift the connotation.

That also sets the refusals that keep this from tipping into retro pastiche:

- No monospace for ordinary lists. The build embeds no monospace face, and
  one added later is for diagnostic/log text only, see "Fonts" below.
- No scanlines, glow, or box-drawing chrome anywhere in the UI.
- No blinking block cursor. `SelectionCursor`'s flash is a single one-shot cue
  on activation (see "Sanctioned one-shot cues" in `docs/slint-gotchas.md`),
  never a persisting blink, the animation-cost rule in `AGENTS.md` bans
  looping cues over content regardless of theme.

Integer-pixel precision (see "Integer-pixel drawing" below) is not only a
240p/MiSTer concern. At 4K, a glyph run that straddles a half-pixel or a card
edge that lands on a fractional coordinate is the difference between "this was
designed" and "this is nostalgic clip art", softness reads as sloppiness at
high resolution even though nothing is functionally broken.

## Cards: focusable surface recipe

Card means selectable surface that guarantees content contrast regardless of
page background. Keep opaque card plates under cover art; future custom
backgrounds must not weaken art, caption, or focus contrast.

| Property | Value |
|---|---|
| Fill | `Theme.surface-card` |
| Static border | `Sizing.card-border-width`, `Theme.border-mid` |
| Focus border | `Sizing.focus-border-width`, `Theme.accent` |
| Outer/card radius | `Sizing.radius-md` |
| Nested control radius | `Sizing.radius-sm` |
| Inset | `Sizing.surface-pad` (see "Surface containment") |

Tile bodies, browse cards, detail panes, and About body use `radius-md`.
Settings rows, modal buttons, menu/picker rows, nested list selection,
toggle tracks/handles, and rapid-scroll chrome use `radius-sm`.
See "Toggle rows" below for the track/knob color rule. Handle insets preserve
integer centering.

### Tile focus ring

A grid's focus ring is `FocusCursor` in `focus.slint`: one accent-bordered
rectangle, `Sizing.focus-ring-width` thick, inset `Sizing.pct-h(0.4)` inside
the focused cell. `PagedGridView` draws it over the page rather than on the
tile, glides it between adjacent cells, and moves it down with the face during
a press (`press-y`), so ring and face never separate. The selected tile itself
sets `own-ring: false`. `Tile`'s own ring, two stacked filled rectangles (an
outer accent rect and an inner `surface-card` mask) inside the card edge, is
what the held tile in a Move session draws.

`PressableSurface` in `tiles.slint` (`DialogModal`'s buttons and
`LetterJumpModal`'s letter cells) draws the two-rect construction when
`focused`, inset inside the face rather than outset (the surface clips). This
gives a focused modal button a solid accent band, like a focused tile, instead
of the older thin border color/width swap. The face's own border
(`border-mid`, `card-border-width`) does not change with focus, the ring is
additive, not a replacement for the resting border.

Ring *thickness* is not shared with `Tile`, though, `PressableSurface`
derives its own `ring-gap`/`ring-width` from `Sizing.card-border-width`, the
same token that already draws the row's own resting border. Two earlier
versions got this wrong in opposite directions: a screen-relative percentage
(matching `Tile`'s own tokens) ate ~38% of a short pressable row's face
height on top of its existing static border and press edge, three
concentric frames reading as clutter instead of one clear "this is focused"
cue. (This history predates `ContextMenu`/`ListPickerModal` moving off
`PressableSurface` entirely, see "Two registers" above, but the fix still
governs every remaining `PressableSurface` caller.) Rescaling that same
percentage to the row's OWN height instead floored to exactly 1px at every
real resolution tier, no heavier than the row's resting border, so focus
read as barely more prominent than idle chrome. Deriving from
`card-border-width` fixes both by construction: `ring-gap: card-border-width`,
`ring-width: card-border-width * 2`, the band is always exactly double the
resting border's weight, and both scale together off the same token, so they
can't drift out of relative proportion at some resolution neither was tested
at. The resting border itself stays untouched (still additive, per above),
and the ring construction (two stacked filled rects, `Theme.accent`) is
unchanged, this is a thickness-derivation fix only.

Thickness alone was still not the whole fix. `Tile`'s own focus cue is not
ring-only: the caption dims to `Theme.text-label` at rest and brightens to
`Theme.text-primary` when focused, and bundled artwork/logos swap
`logo-primary` → `logo-focus-primary` the same way. `PressableSurface` callers
(`DialogModal`'s buttons, `LetterJumpModal`'s letter cells) never picked up
that half, their label text sat at `Theme.text-primary` unconditionally, so
every unfocused row already looked fully lit and the ring was the only signal
carrying focus at all. Each of those now dims/brightens its own label the
same way `Tile` does, so two independent signals reinforce each other. Rows
with only one possible focus target (`focused: true`, no other row to
contrast against, `LogUploadModal`, `CardWriteModal`) are left alone;
there's nothing for dimming to distinguish there.

Tiles use a physical front edge inside their existing cell footprint. Activation
lowers artwork, caption, and ring together without scaling cover art.

### Launch feedback stays on the control that was pressed

The press-in is the launch cue, and it lasts as long as the launch does. An
ordinary push is a fixed 90 ms, which is over before Core has answered, so a
commit that starts work outliving its own push keeps the control down
(`press_feedback::keep_held`) and lifts it when the work resolves, fails, or
the frontend goes dormant behind the game it started. A cap (`HOLD_MAX_MS`)
releases anything still held after ten seconds: a Core that never answers must
not leave a tile pushed in.

The reason to keep it there rather than in the header is that the eye is on
the tile that was just pressed, and on a handheld at arm's length or a TV
across a room the status line is nowhere near it. No WCAG criterion governs
where feedback is placed, so this is a usability rule, not an accessibility
one; what accessibility does require is that a held state carry a 3:1 step
from its resting state if colour is the only difference, and that it have a
form under Reduce Motion.

The header line is the second, worded cue, on the same pattern as a hidden
tile's muted edge plus its worded reason. It waits out `LOADING_CUE_DELAY_MS`
first, so a launch that answers quickly never flashes a word into the header
and away again. Nothing floats on the tile itself: no spinner, no badge, no
looping cue over content.

### Hidden and disabled tiles, muted material, worded reason, never a badge

A tile that isn't in its normal, fully-live state, a user-hidden game or
system, or a Hub tile whose live precondition isn't currently met (Resume
with no history, Update with no internet, a category Core hasn't confirmed)
never disappears and never dims via opacity. Two cues, neither a floating
badge:

- **Muted front edge**, the glanceable cue, visible on every affected tile
  at once, focused or not. `Tile` in `tiles.slint` swaps its surface's
  `edge-color` from `Theme.tile-edge` to `Theme.border-mid` (the same neutral
  "resting card edge" role `PressableSurface`'s own static border already
  uses) whenever the `GridCell` is `hidden` or `disabled`. No new palette
  engineering, reusing
  an existing neutral role is the point.
- **Worded reason**, the detail cue, surfaced through text the screen
  already shows rather than an overlay drawn on top of the tile. Captioned
  tiles (`show-caption: true`, Games/Favorites/Recents) fold it into the
  same dim suffix slot disambiguating tags already use (`MarqueeCaption`'s
  `tags`, e.g. "Cave Story · Hidden"). Non-captioned tiles (Hub, Systems) have
  no per-tile text to fold into, so it surfaces through the screen's
  `ActiveLabel` (also a `tags` suffix) while that tile is focused instead.

This replaces an earlier "Hidden" corner-pill component (`TileBadge`, since
removed) and a considered opacity-based "disabled" treatment. Both were
rejected for the same reason: a raw alpha multiplier has no contrast floor.
Measured against this app's own derived `zaparoo-dark` palette, icon+label at
40% opacity landed at 2.1:1 resting / 3.6:1 focused contrast against the tile
face, both fail WCAG AA text contrast (4.5:1), and the resting case fails
even the 3:1 floor for non-text UI components. Every other semantic color in
this app (`accent`, `on-accent`, `marker`) is explicitly OKLCh-walked to
guarantee a minimum ratio; an opacity trick was the one state cue that
wasn't. Icon/label color for a hidden or disabled tile stays locked to the
tile's normal resting tier permanently (focused or not) instead, the exact
color every tile's unfocused state already ships, so it inherits that
state's already-passing contrast for free, with no new number to get wrong.

A badge was also the wrong shape for the job on its own terms, independent of
contrast: UX writing on disabled-state design consistently flags that a
disabled control giving no reason why is a discoverability failure ("users
wonder why it is not available"), and badge-design guidance separately
warns against overlay pills as clutter when they compete with the content
they're stuck on. Reusing an existing screen surface for the reason (rather
than adding a new overlay) avoids both: it's read, not scanned for, and nothing
new is drawn on top of the artwork.

### Inverse-video rows (browse lists, Settings rows, menus, and pickers)

Browse lists and Settings rows are both one containing card with flat rows;
never turn every row into a separate raised button. The selected row fills
solid with `Theme.selection-fill` and most content on it, label, value text,
chevron, and the dim disambiguating tag, flips to `Theme.on-accent` (or
`Theme.on-accent-muted` for the tag), the semantic-tier role walked to ≥4.5:1
against that fill (see "Semantic tier" above). The favorite heart flips to
`Theme.on-accent` too rather than `Theme.marker`, `marker` is tuned for
legibility against `surface-card`, not against the solid selection fill.
Nothing moves: no rail, no inset, no push-in. A
text row is not a button, so it doesn't get a button's depth cue.

`ContextMenu` and `ListPickerModal` rows follow the identical recipe against
their own `bg-panel` host instead of a `surface-card`: the same
`SelectionCursor`, the same fill-plus-`on-accent` swap, no separate row card
nested inside the panel. The color-scheme picker's swatch-preview border is
the one row element with a per-register color: it sits at `Theme.text-label`
at rest (a mid neutral guaranteed ≥3:1 against `bg-deep` on every preset, so it
separates a near-black or near-white swatch from the row) and flips to the
cursor's `content` color on the selected row, the same fix the favorite heart
uses against the solid selection fill.

Activation is a one-shot inverse blink: the fill and its content hard-swap
colors for `Motion.press-ms`, then swap back, rather than a crossfade. This is
deliberate, punchier, era-correct for the DOS-terminal-flash half of the
[Two registers](#two-registers) language, and two repaints total instead of
one per animated frame. Under Reduce Motion (`Motion.enabled: false`) the
blink resolves in a single frame and is effectively invisible, the same
convention every other one-shot cue in the app follows.

The fill is not part of the row, so it can travel between rows:
`SelectionCursor` in `focus.slint` owns the rectangle, the flash and the two
swap colors, and each row paints an inverted copy of itself inside a
`SelectionClip` bounded by the fill's current band. See "Selection motion"
below.

The highlight lives once in `SelectionCursor` so no two lists can drift
apart, each list mounts one, paints each row's inverted copy from the
cursor's `fill` and `content` colors (`content` is `Theme.on-accent` outside a
flash; see "Semantic tier" above), and forwards its host's
`activate-pulse`/`release-pulse`/`screen-settling`. `release-pulse` and
`screen-settling` only cut a flash short; they don't hold it open past
`Motion.press-ms`. What does hold it is `press-held`, for as long as the router
keeps the accepting row down before it dispatches. Toggle rows are exempt from
the flash (`flash-enabled` is false on them): the knob slide is their own
activation cue. See "Toggle rows" below for how the track/knob colors work on
a selected row. `ContextMenu` and `ListPickerModal` bind only `press-held`:
they close on accept rather than settling back into an idle list, so there is
no pulse to cut short and no `screen-settling` transition to forward.

The physical register still stays visible in the same app, tiles carry it on
every launch, `DialogModal`'s buttons and `LetterJumpModal`'s keypad carry
it inside modals, so this isn't the language collapsing into one idiom, just
narrowing physical treatment to things that are genuinely objects rather than
list choices. See "Two registers" above.

#### Selected-row text weight

Dark-on-light (inverted) text suffers irradiation, it reads as thinner
than light-on-dark text at the identical weight, a real optical effect,
not a rendering bug. Resting rows are `Theme.text-primary` on
`Theme.surface-card`; a selected row is `Theme.on-accent` on the solid
`Theme.selection-fill`, the inverted case. Round 8 added a weight step to
correct it: the selected copy of a row, the one painted inside the
`SelectionClip`, draws at `FontWeight.medium` and the resting copy at
`FontWeight.normal`, in `SettingsRowView` (`content-weight`), `BrowseList`
(through `MarqueeCaption`'s `caption-weight`), `ContextMenu`, and
`ListPickerModal` alike. The goal is parity with the resting row, not
emphasis, `FontWeight.medium` is deliberately one notch, not a jump to
bold. It resolves to a real cut rather than a synthesized one: the binary
embeds static `resources/fonts/NotoSans-Medium.ttf` and
`NotoSans-DemiBold.ttf` instances beside `NotoSans.ttf` (imported in
`ui/app.slint`), because Slint only rasterizes a variable font's default
instance.

No-op under `Theme.bitmap-fonts`: the CRT/240p 6x8 face has a single
strike, and unantialiased 1-bit text has no irradiation to correct in the
first place, the same reasoning `Theme.font-ui`'s bitmap branch already
gets a pass on for size (see "Type ladder" below).

Panel-width measurement (`desired-panel-w` in `ContextMenu`, `desired-w` in
`ListPickerModal`) must size against the *selected* weight, not the resting
one, or the widest label can elide the moment it becomes selected, both
measure every label in a hidden `VerticalLayout` fixed at `FontWeight.medium`
for exactly this.

#### Action rows are the one row kind that centers its label

`ControlKind.action` rows (`updateMediaDb`, `runScraper`, `uploadLog`) used
to signal "this row runs something" purely by tinting the label
`Theme.accent` instead of `Theme.text-primary`, the only cue, and one that
disappeared entirely on a selected row (the label collapsed to the selected
content color regardless of control, identical to every other
selected row). Round 8 kept the tint but added a stronger, selection-proof
signal: the label centers, following Apple's own convention for an
accessory-less "runs an action now" row (Sign Out, Erase All Content and
Settings) rather than inventing a new glyph with no real precedent. It is
deliberately the *one* row kind that centers, every other row stays
left-aligned per "Two registers" above, and an embedded button (WinUI's
`SettingsCard.ActionButton`, libadwaita's `AdwButtonRow`) was considered
and rejected for the same reason `ContextMenu`/`ListPickerModal` moved off
`PressableSurface`: it would reintroduce a button-shaped object into a row
register this file deliberately keeps flat and borderless.

Centered by item position (`x: (parent.width - self.width) / 2` on a label
whose width is its measured text, in `SettingsRowView`), never a full-width
`Text` with `horizontal-alignment: center` (see "Integer-pixel drawing"
below). The row's live status readout ("100,000 indexed", "In progress",
"Paused") moved from right-aligned-next-to-the-label to a second centered
line below it, one type size down (`Sizing.font-caption`), keyed off the
row's `status-key`. Rust adds that line and its gap to the row's height, so
the pair centers as the two-line stack it is.

#### Toggle rows

One rule, both row registers: **the track alone carries on/off + row-register
state, at maximum contrast against the row's own current background; the
knob fill always matches that same background (a hole punched through the
track), with a border in the track's own "on" color for that register**:

| | On track | Off track | Knob fill | Knob border |
|---|---|---|---|---|
| Unselected row | `Theme.accent` | `Theme.border-mid` | `Theme.surface-card` | `Theme.accent` |
| Selected row | `Theme.on-accent` | `Theme.on-accent-muted` | `Theme.selection-fill` | `Theme.on-accent` |

Before this rule, the track and the knob branched on row-selection
independently of each other, so which element carried state flipped
depending on whether the row was selected, that inconsistency, not the
switch metaphor, was what read as broken. Round 4 fixed the track/knob
inconsistency by having the knob fill mirror the row's own current
background, but on a selected row that background is the solid selection
fill, the same color the knob fill uses, so the knob visually merged into the row
itself on lower-chroma presets (Nord, Ayu, Kanagawa). Round 5 kept the
fill rule (it is still correct by construction, the knob fill is never
anything but the row's own two possible backgrounds) and added the border:
the track's own "on" color for that register, which the palette derivation
already holds well clear of the fill (`on-accent` is walked to 4.5:1 against
`selection-fill` for the selected case; `accent` is clamped to 4.5:1 against
`bg-deep`, a proxy for the unselected case), so the knob keeps a visible
silhouette on every preset with no new color derivation needed. Knob position and travel stay the
primary on/off cue in every state; the off/unselected knob-vs-track contrast
is deliberately low (`border-mid`/`surface-card` are both subtle near-card
neutrals), position, not fill color, carries that state, and the border is
what keeps the knob's own silhouette legible regardless.

### Section headings

`SectionHeader` in `chrome.slint` titles a group inside a vertical list: the
Settings form's bands ("Analog video"), Game info's Description block, and the
picker page inside the setup panel. It is a `Theme.text-label` label at
`Sizing.font-body` (semi-bold where weight exists), inset `Sizing.pct-w(2)` like
the row labels under it, on a one-stroke `Theme.border-mid` rule that runs the
row width. The same component, unchanged, on the settings card, in a modal
panel and on the detail sheet, a heading is a heading wherever the list is.

Color and a rule, not size or weight. A bigger, bolder label silently
stops working in bitmap mode: `Sizing.font-size()` quantizes `font-section`
and `font-body` to the same 8/16px there, and `Theme.font-ui`'s bitmap face
("MxPlus HP 100LX 6x8") has a single weight, so `FontWeight.semi-bold` is a
no-op and the heading is pixel-identical to a field label. What survives that
tier is a color step (`text-label` against `text-primary` rows, the same
signal the hint band below leans on) and a rectangle (the rule). `border-mid`
measures 1.5:1+ off both `surface-card` and `bg-panel` across the catalog, and
`text-label` holds 3:1 on both. The rule stays off `Theme.accent`, which
`SettingsRowView` reserves for "this row is an action" (see "Toggle rows"
above and the action-row label rule below it).

An earlier version was a full-card-width `Rectangle` filled `border-mid`
with the label inside it, mounted without the card padding so the band met
the settings card's own frame edge to edge. That only worked where there
was a frame to meet; inside a modal panel it was a gray block with margins,
and it gave one job two looks. The rule-under-label heading needs no mount
special case.

#### Settings hint band

A per-row description line under the label (round 7) did not survive
contact with 540p and CRT: a described row grew from `pct-h(8)` to
`pct-h(11.2)` and painted the description at `Sizing.font-caption`, but
`font-caption`/`font-body` both quantize to the same 8px bitmap strike under
`Theme.bitmap-fonts`, so at 240p/CRT there was no size hierarchy left and the
band clipped, and on a selected row both lines collapsed to the identical
selected content color, losing the colour hierarchy too.

Round 8 replaced it with one shared band pinned to the bottom of the
settings card, showing the *focused* row's description instead of every
described row's own line. The card frame is static: the rows scroll inside
a clipped band above a hairline `Divider`, with the hint `Text` below it.
`Sizing.font-body`/`Theme.text-label`, not `font-caption`, the same
reasoning as above, colour is the only hierarchy signal that survives the
bitmap tier, so lean on it alone rather than a size step that collapses to
nothing there.

The reservation is **two lines at the 240p/bitmap tier and one line
everywhere else**. Descriptions are authored against that tier's line
budget (`docs/content-style.md`), which is the narrowest card we ship, so
every wider tier fits them on one line and a second reserved line is a
blank band holding the rows up for nothing. What is not negotiable is that
the reservation is fixed *per tier* rather than per row: it sets the rows
viewport, so a band that grew and shrank with the focused row's
description would reflow the list under the cursor, which is the bug the
unconditional reservation was there to prevent in the first place.

The divider above the band is a structural line: the rows clip and scroll
under a pinned band, so it runs the **full card width**, edge to edge like
the card's own frame, with `surface-pad` either side of it. That is the
opposite job to a `SectionHeader` rule, which runs the row width because it
belongs to the rows. See "Lines".

A right-hand detail pane (the more common modern pattern, Kodi, Android
TV, Switch) was considered and rejected: the card is already capped at
`pct-w(70)`, and splitting that horizontally at the 240p/CRT tier would
leave row labels roughly half their current width. A bottom band costs
one vertical slot, identical at every tier, and doubles as the fix for a
separate bug: because the card frame is no longer scrollable content
itself, its top/bottom edges can no longer be scrolled out of view the
way they could when the whole card lived inside the scrolling list.

### Pressable front edge

Grid tiles, modal buttons, and `LetterJumpModal`'s letter cells use
`PressableSurface` in `tiles.slint`. Its focus ring is `Tile`'s two-rect
construction reused, see "Tile focus ring" above. `ContextMenu` and
`ListPickerModal` rows do not, see "Two registers" above for why they moved to
the list's selection fill.

**Front edge is not a shadow.** It is opaque physical material. Grid tiles use
`Theme.tile-edge` against `bg-deep`; controls use `Theme.control-edge`, one step
further along the accent ramp, when embedded in panels. Do not describe either as
"lighter", on a light preset both are darker than their ground. The invariant
is that the control edge sits *further from the ground* than the tile edge, in
whichever direction the preset runs.

Edge top corners are square and extend behind the face by its corner radius,
while only bottom corners remain rounded. Never use black, transparency,
gradients, or shader effects. Rest exposes `Sizing.press-edge-height`; press moves
face/content down and collapses exposed bottom edge. Motion routes through
`Motion`, so Reduce motion and native 1080p MiSTer snap immediately.

**Why it reads as gloss.** Two properties do the work, and both are easy to lose
in a refactor that "simplifies" the edge:

1. The edge is *more saturated* than the face and sits well off it on the accent
   ramp. A cast shadow would be a desaturated darkening; a chromatic strip along
   the near face reads as reflected light instead.
2. Because the edge rect is `edge-height + radius` tall with square top corners
   and rounded bottom corners, its color fills the notch left by the face's
   rounded corner and follows that curvature. A highlight that follows curvature
   is the canonical gloss cue and is the larger of the two contributions.

The specular read genuinely depends on a dark ground. On a light preset the
accent-derived edge is darker than the face and reads as a correctly-lit bevel
rather than a shine. Depth survives; the shine is a dark-preset property. Any
check on the edge roles therefore asserts chromatic separation along the accent
ramp, not lightness.

## Plain text on background

Non-interactive text may sit directly on `Theme.bg-deep`:

- TopStatusStrip titles
- Section headings
- global Loading cue
- ActiveLabel selected name
- header status line (see "Header status line" below)

Use `Theme.text-primary` for primary content and `Theme.text-label` for metadata.
Use Body or larger unless space has an explicitly documented specialist role.
Pressable content belongs on card/control surface.

## Focus

Focus is always `Theme.accent`. No second focus color. Accent configurability is
future theme work, not per-surface override.

## Pills

Toggle track/thumb use `height / 2` or `width / 2`. Pills are distinct from
rounded squares and remain borderless; outer Settings row carries focus. See
"Toggle rows" above for the track/knob fill rule, which differs between an
unselected and a selected row.

## Header status line

The header's second row shows Core connection problems and background-task
progress (indexing / optimizing / scraping) as plain text plus a segmented
progress track, never a pill. It replaced a stadium-shaped status pill
card: a bordered, half-height-radius surface is the toggle-track family's
shape (see "Pills" above), not a status readout's, and squeezing a label,
counts, and a spinner into one intrinsically-sized chip is what forced
abbreviated CRT-only wording ("Idx…", "Scr…").

`StatusLine` in `chrome.slint` lays out a right-aligned pair, not a full-width
stretch: a shrink-wrapping primary-text label (`overflow: elide`,
width capped to its own measured content so it hugs the track instead of
leaving a gap), then a fixed-width `ProgressTrack` as the rightmost element,
flush against the header's own right margin. A short message just sits
closer to the right edge, idle space moves to the left of the pair,
between it and the logo, rather than opening a gap in the middle of the
message the way a full-width stretch would. No card fill, no border, no
radius, this is the same "plain text on background" treatment
TopStatusStrip and the global Loading cue already use (above). The row is
reserved by the header's own fixed height (`Sizing.header-height`) whether or
not the line has anything to show, matching every other fixed-slot
discipline in this file; the line itself still collapses to zero height
when idle.

There is deliberately no trailing count next to the track. Two things were
tried and both cut: a step ratio ("3 / 10", redundant with what the track
already shows visually) and, after that, an absolute running total
("18 files" / "1250 scraped"), a real number the track can't express, but
still not worth the layout cost it added. A fixed worst-case reservation
for it ("999999 scraped") left a visibly blank void next to the track
whenever the actual count was short or absent (every optimize/vacuum
phase); sizing it to live content instead fixed the void but reintroduced
the exact kind of shifting anchor point the fixed-slot rules elsewhere in
this file exist to prevent. The bar alone conveys progress, the same as the
mobile app.

Label and detail join with a plain colon (`"Indexing: {}"`), not a
mid-dot, the bitmap CRT font renders `·` as a genuine pixel glyph (it's
not a missing-glyph problem), but at 6×8 a single centered dot is one or
two lit pixels, easy to miss at a glance, and not a character anyone types
by hand. Round 8 gave the *reason*-clause states on this same line
(`"Core error — {}"`, `"… paused — game running"`, `"Scrape failed —
{}"`) a separate em dash join, distinct from the colon a *live detail*
state uses. Round 9 dropped the em dash from every user-visible string,
see `docs/content-style.md` → "Punctuation", so all four now join with
the same colon (`"Core error: {}"`, `"Indexing paused: game running"`,
`"Scraping paused: game running"`, `"Scrape failed: {}"`). The words
already carry the reason-vs-detail distinction ("error", "paused",
"failed" vs. a bare progress readout), so nothing is lost by collapsing
the two joins into one.

One slot, resolved in priority order by `zaparoo_app::status_line`: a Core
connection problem; an active
background task (including why it's paused, "game running"); a terminal
message held ~6 s after a task ends (a scrape failure lands here too, not as
its own tier, since Core only reports `state: "failed"` on that same
terminal frame); a transient event (card scan, playtime warning, inbox
message) dropped rather than queued while a higher tier owns the line; or
nothing.

### Progress track

`ProgressTrack` in `chrome.slint` is a row of discrete cells, not a continuous bar.
Determinate progress (indexing/scraping systems) fills cells up to the
fraction complete; indeterminate progress (optimize/vacuum, or before Core
has reported a total) marches one lit cell along the track instead. Either
way, exactly one cell, the fill's leading edge, or the marching cell,
blinks on and off at `Motion.pulse-ms` (~2 Hz), pausing along with the task
it represents.

**Blinks, does not fade.** A `Timer` flips a bool every `pulse-ms`; the
cell's `background` binding reads it straight through with no `animate` in
between, every tick is a hard cut between `Theme.accent` and its dim color,
never an interpolated crossfade. A breathing/throbbing pulse was tried and
rejected: the spec is a blink, on then off, the same instant-swap register
`SelectionCursor`'s inverse-video flash already uses (a timer-held bool, not
an animated transition), this component just repeats that swap on a timer
instead of firing it once.

Discrete cells are a structural choice, not a stylistic one: a continuous
fill's right edge lands on a fractional pixel as the fraction changes,
which softens under any 240p rendering, the same class of bug "Integer-
pixel drawing" below exists to rule out everywhere else. Cells fill whole,
and the track's width depends only on the cell count, never on the fill
fraction, mirroring how `PageIndicator` reserves its width unconditionally in
the footer.

The blink is the one exception to the no-persistent-motion rule (see
`docs/slint-gotchas.md` → "The one continuous exception: ProgressTrack's
leading-cell blink"): it is header
chrome, never painted over content; its dirty rect is a single small cell;
it only runs while a task is genuinely active and not paused; and it stops
outright under Reduce Motion rather than collapsing to a 0 ms loop. This is
the same small-dirty-rect exemption AGENTS.md already grants a page-dot
pulse or focus-ring blink, just continuous instead of one-shot, because a
background task has no natural per-frame "done" edge the way a press/release
cue does.

## Colors

Every UI color comes from the `Theme` global in `theme.slint`; never inline hex
except diagnostic calibration surfaces with documented reason. Roles have no
fixed hex, they are derived per preset from `primary`, `accent`, and `text`. `mix(a, b, t)` is a
per-channel sRGB lerp; `surface(t, bias)` is `mix(mix(primary, text, t), accent, bias)`,
the neutral ladder with a slight accent cast; `ink` is the pole away from `text`.
`oklch(L, C, h)` below means "hold this OKLCh lightness/chroma/hue, gamut-fit
into sRGB if needed", see "OKLCh derivation ladder" above.

| Token | Derivation | Use |
|---|---|---|
| `bg-deep` | `primary` | Flat page background |
| `bg-bar` | `mix(primary, ink, 0.35)` | Help bar |
| `bg-panel` | `surface(0.05, 0.04)` | Modal/ContextMenu panel |
| `surface-card` | `surface(0.08, 0.05)` | Card/control face |
| `border-subtle` | `surface(0.14, 0.03)` | Low-contrast edges |
| `border-mid` | `surface(0.32, 0.05)` | Resting card edge |
| `tile-edge` | `mix(edgeBase, accent, 0.44)` | Grid tile front edge |
| `control-edge` | `mix(edgeBase, accent, 0.54)` | Control front edge in panels |
| `scrim` | `#000000cc` (80% black) | Modal scrim, always dark |
| `text-primary` | `text` | Primary text |
| `text-label` | `mix(primary, text, 0.62)` | Secondary text |
| `text-variant` | `surface(0.58, 0.14)` | Tinted secondary text |
| `accent` | `accent`, clamped ≥4.5:1 against `primary` | Focus and selection |
| `selection-fill` | `accent`'s OKLCh L and hue, chroma ×0.7 (Subtle) or ×0.9 (Vivid) | Inverted row fill (`SelectionCursor`) |
| `on-accent` | Better-contrasting of `primary`/`text`, OKLCh L walked to clear 4.5:1 vs. `selection-fill` | Body text/glyphs on the solid selection fill |
| `on-accent-muted` | `on-accent`'s OKLCh hue/chroma, L walked toward `accent` into the 3.0–4.5 band | Subordinate on-accent content (tag suffixes, off toggle track) |
| `logo-shadow` / `logo-secondary` / `logo-primary` | `oklch(max(0.26, aL−0.28), 0, ah)` / `oklch(max(0.10, aL−0.16), 0, ah)` / `oklch(min(0.74, aL), 0, ah)` | Resting tinted-artwork ramp |
| `logo-focus-shadow` / `logo-focus-secondary` / `logo-focus-primary` | `oklch(max(0.32, aL−0.22), aC, ah)` / `accent` / `oklch(min(0.90, aL+0.16), aC×0.72, ah)` | Focused tinted-artwork ramp |
| `marker` | Fixed ~29° OKLCh hue (rotated ±90° away from `accent`'s hue if within 40°), L fitted for ≥3:1 vs. `surface-card` | Favorite heart, Hidden badge |
| `marker-outline` | `mix(luma(marker) > 0.5 ? dark_pole : light_pole, marker, 0.12)` | Favorite-heart / marker keyline |
| `error` | `#ff4f91` dark / `#c2185b` light | Failure text |

`aL`/`aC`/`ah` are `accent`'s own OKLCh lightness/chroma/hue. `edgeBase` is
`mix(primary, text, 0.06)`, one rung up the neutral ladder, so the accent
ramp keeps some body on a near-black primary; `tile-edge` and `control-edge` are
the only roles built on it now that selection is no longer a mixed surface
tint, a selected row is `selection-fill`/`on-accent` inverse video
(`SelectionCursor`). The resting logo ramp reuses the focus
ramp's three lightness rungs desaturated to grey (`C = 0`) and pulled dimmer
on every rung, so an unfocused tile reads as clearly dimmer, not just
ring-less, collapsing that gap is what makes focus read as "ring only." Both
ramps hold `accent`'s own OKLCh hue (and, on the focus ramp, most of its
chroma) rather than mixing toward `primary`/`text` in sRGB, which is what
fixed a focused amber tile reading near-white and a focused blue tile reading
closer to brown than orange.

### Header logo asset ladder

The bundled wordmark logo cannot be recolored by a palette role, it's a
full-color brand mark, not a single-hue tinted glyph. `HeaderBar` instead
selects between two pre-rendered PNG variants under
`resources/images/logo/logo-<variant>-<w>.png`, embedded through the `Brand`
global in `chrome.slint`:

- `on-dark-<w>`, light wordmark, for `zaparoo-dark` / `classic-purple`.
- `on-light-<w>`, dark wordmark, for `zaparoo-light`.

`Theme.light-surface` (pushed from `zaparoo_app::palette::is_light_surface`)
picks the variant. It is deliberately not a palette role (it is not a field
of `palette::Palette`), because it selects an asset, not a color.
`w` is one of `96, 144, 192, 256, 384, 600` (600 is the largest rung, at the
master's own aspect ratio). `Brand.logo(light, painted-width)` is the single
place that snaps a painted width up to a rung (the same "snap up" contract
`zaparoo_app::sizing::snap_cover_tier` uses for grid covers) and picks the
variant; `HeaderBar`, `AboutScreen`, and the screensaver's bouncing copy
(through `Brand.saver-logo`, always the on-dark variant over its black
backstop) all call it instead of naming an image, so none of them ever
decode a texture larger than their own painted size. There is no unscaled
monolithic `logo.png` any more, every call site goes through the ladder.

## Resolution tiers

Shape/type tier uses unrotated output resolution height. TATE swaps axes before
tier selection. CRT scenes reconstruct that axis before safe-area reduction;
bitmap type and browse profiles use their dedicated flags. Rendering path does
not affect geometry tier.

| Tier | Effective height |
|---|---|
| `240` | below 400 |
| `480` | 400–519 |
| `540` | 520–659 |
| `720` | 660–899 |
| `1080` | 900+ |

Thickness scales with resolution. Shape and hierarchy use discrete tiers.

## Fonts

`Theme.font-ui` is Noto Sans normally and MxPlus HP 100LX 6x8 when
`Theme.bitmap-fonts` is set. The build embeds no monospace face; if one is
added, it is for diagnostic/log text only.

`bitmap-fonts` (on both `Theme` and `Sizing`) is a typography-only flag,
separate from the CRT *layout* flag (`Sizing.crt`, `crt_native_path` in
`zaparoo_app::sizing::Inputs`: overscan insets, grid density, TATE). It is
true on the CRT native path, and also auto-engages on embedded hardware at
240p (framebuffer height under 400) even without `--crt`, a proportional
antialiased face at 8-14px is illegible at that resolution, and the desktop
build never sets it outside the CRT path because a resizable window can cross
the 400px tier boundary at runtime. See `display::bitmap_type` in
`rust/frontend/src/display.rs`.

### Type ladder

Six ordinary text roles only:

| Token | Role | 240 | 480 | 540 | 720 | 1080 |
|---|---|---:|---:|---:|---:|---:|
| `Sizing.font-hero` | Page/selected title | 14 | 22 | 24 | 29 | 43 |
| `Sizing.font-title` | Modal/detail title | 12 | 18 | 20 | 23 | 35 |
| `Sizing.font-section` | Section/list/status | 11 | 17 | 18 | 21 | 31 |
| `Sizing.font-body` | Body/control/help | 10 | 16 | 17 | 19 | 28 |
| `Sizing.font-caption` | Secondary/menu/tile fallback | 9 | 14 | 15 | 17 | 26 |
| `Sizing.font-small` | Tile/detail small print | 8 | 13 | 14 | 16 | 24 |

`zaparoo_app::sizing::derive` resolves the six tokens and Rust pushes them
into `Sizing`. When `bitmap-fonts` is set, they resolve through the percentage
font size (`Sizing.font-size(percent)` in Slint, `Inputs::font_size` in Rust)
instead of this ladder and retain mandatory 8/16px bitmap quantization, the
bitmap face only ships those two strikes, so the ladder and the quantization
must move together. `font-size(percent)` remains for approved specialist sizes
and geometry such as header row height; do not use it to invent a seventh
ordinary text role.

Center a `Text` item itself, with natural geometry
(`x: (parent.width - self.width) / 2` on a width that hugs the text), and keep
its glyph run left-aligned when pixel sharpness matters; Slint snaps the item
to physical pixels.

## Stroke ladder

| Token | Formula | Use |
|---|---|---|
| `card-border-width` | `stroke(pct-h(0.2))` | Resting card edge |
| `focus-border-width` | `stroke(pct-h(0.4))` | Focused controls |
| `focus-ring-width` | `stroke(pct-h(0.6))` | Tile focus ring |
| `press-edge-height` | `stroke(pct-h(0.8))` | Control front edge |

`zaparoo_app::sizing::derive` resolves these (a percentage of the height axis,
never below 1px) and Rust pushes them into `Sizing`.

Dividers, tiny badges, help-bar edges, and other literal hairlines stay
`Sizing.stroke(1)` unless promoted deliberately. A line inside a surface goes
through the shared `Divider` component rather than being written out, see
"Lines" for which of the four jobs it is doing.

## Radius ladder

| Tier | `radius-md` | `radius-sm` |
|---|---:|---:|
| `crt` / `240` | 2 | 1 |
| `480` | 3 | 2 |
| `540` | 4 | 2 |
| `720` | 6 | 3 |
| `1080` | 8 | 4 |

Nesting rule: inner surface uses smaller rung. Pills remain separate family.
No percentage-derived rounded-square radius.

True super-ellipses need vector paths or shaders. Frontend no longer
wants large squircle arcs anyway: small circular corners land close to integer
right angles and avoid software-rasterizer fringe artifacts.

## Surface containment

Every container in the app is one of two things, and each carries exactly one
containment signal:

| Role | Where it sits | Fill | Edge | Radius |
|---|---|---|---|---|
| **Panel** | over a scrim, modal, anchored menu | `Theme.bg-panel` | none | `Sizing.radius-md` |
| **Card** | directly on the page ground | `Theme.surface-card` | `Sizing.card-border-width`, `Theme.border-mid` | `Sizing.radius-md` |

**One signal, not two.** Material's three container variants, elevated, filled,
outlined, are alternatives you choose between, not layers you stack. We have no
shadows (software renderer, and `docs/slint-gotchas.md` rules out shader-like
effects), so the choice
is fill-only or fill-plus-outline. A scrim already separates a panel from the
page behind it; that is the backdrop's whole job, so an outline over a scrim is
a second mark for one state, the same argument that took the focus ring off
inverted rows. A card has live content beside it and only a small fill step off
`bg-deep`, so it needs the outline: that is Material's outlined card exactly.

Plates that are card recipes are cards. The rapid-scroll letter plate used
`radius-sm` while carrying a `surface-card` fill and a `border-mid` frame; it takes
`radius-md` like everything else with that recipe. The About card used a bare
`Sizing.stroke(1)` where every sibling used `card-border-width`, so its frame was
half the thickness of the Settings card at 800p and 1080p.

The screensaver overlay, the CRT calibration plate and the help bar are screen
chrome rather than containers and keep their own treatment.

## Padding scale

**One inset.** `Sizing.surface-pad` is the lip on all four sides of every card and
every panel, the context menu included, and it is also the gap between the
blocks stacked inside one: title to content, content to buttons, content to
divider. It resolves to `pct-min(2)`:

| | 352×240 | 640×480 | 960×540 | 1280×720 | 1280×800 | 1920×1080 |
|---|---:|---:|---:|---:|---:|---:|
| `surface-pad` | 4.8 | 9.6 | 10.8 | 14.4 | 16 | 21.6 |

Other insets, which are not container lips:

| Layer | Inset |
|---|---|
| Grid edge | `pct-w(3)` sides, `pct-h(2)` vertical |
| Row text inside a list row | `pct-w(2)` (Settings, ContextMenu), `Layout.row-text-left-padding` (browse) |
| Tile caption side inset | `pct-h(2)` (matches `Tile`'s `pad`, the cover art's own inset) |
| Scroll-cue band | `pct-h(3)` plus `pct-h(0.5)`, reserved inside the lip |

### Square insets

A container's inset is **one margin seen four times**, so it measures the same on
both axes. `Sizing.pct-min(p)` is p% of the *shorter* axis and is what every
enclosing inset uses.

Deriving the sides from the width and the ends from the height is the mistake
this rule exists to stop. It produces 26px beside 14px on a 16:9 screen and 38px
beside 22px at 1080p, which does not read as a considered margin, it reads as a
bug, and it got worse the wider the screen got. Nine different scales had
accumulated across the surfaces before this rule existed. The short axis is also
the right one to spend from: it is the scarce dimension, it stops an ultrawide
from inflating the sides, and taking the minimum means nothing ever grows
relative to what the height axis was already giving. On a landscape screen
`pct-min(p) ≡ pct-h(p)`, so squaring an inset only pulls the over-wide sides in.

`pct-w`/`pct-h` stay for things that genuinely belong to one axis. A **screen-edge**
margin is the clear case: an overscan safe area is 5% of the width at the sides
and 5% of the height top and bottom (Android TV), so grid insets and a card's
margin from the screen edge stay per-axis. So does a vertical rhythm between two
stacked lines of text.

`zaparoo_app::sizing::Inputs::pct_min` and `Tok::PctMin` are the same rule on the
Rust side, for the geometry Rust stacks, the settings rows viewport and the
browse card and detail pane. The layout golden fixture was captured from the
retired Qt build, which still had the split, so
`rust/zaparoo-app/tests/layout_golden.rs` skips those four keys
(`SQUARE_INSET_KEYS`) and `default_card_insets_are_square` owns them instead.

Two documented anisotropic exceptions, both commented in `layouts.rs`: the CRT
list tables keep hand-calibrated pixel insets, because 240p is measured against a
real analog frame rather than derived; and the TATE detail pane keeps
`pct-w(3)` / `pct-h(1.2)`, because it is a short wide strip under the list where
vertical padding costs a metadata row.

Radius never doubles as padding.

## Lines

One component, `Divider` in `chrome.slint`. Before it existed every rule was
hand-rolled, and they drifted to two weights and two colours. Four jobs:

| Line | Weight | Colour | Extent |
|---|---|---|---|
| Card edge | `card-border-width` | `border-mid` | the surface |
| Structural divider | `stroke(1)` | `border-mid` | full surface width |
| Group heading rule (`SectionHeader`) | `stroke(1)` | `border-mid` | content column |
| Row separator in a table | `stroke(1)` | `border-subtle` | content column |

**Extent** follows Material: a full-width divider separates unrelated regions and
separates interactive content from non-interactive; an inset one groups related
content inside a region.

**Where a structural divider goes** is the boundary between a pinned region and a
region that scrolls under it, which is where MDC puts its dialog dividers, and
it decides every case in the app from a boolean the component already has. The
settings card gets one always, because the rows clip under the pinned hint band,
and a titled list panel gets one always, because the title is chrome and the
rows under it are interactive. That second one is deliberately not conditional
on length: gating it on whether the list scrolls put a rule on the Color scheme
picker and none on Color intensity, which is the same component doing the same
job and reads as a defect. Nothing else qualifies, so the dialog, setup, QR,
letter-jump, card-write and About surfaces carry no line. Game info is the one place with a pinned/scrolling boundary and no rule:
its scroll chevrons already mark it, and a rule as well would be two cues for one
state.

**Colour** follows M3's `outline` / `outlineVariant` split: a line carrying
structure needs a visible boundary, a row separator inside a table is decorative
and has no contrast requirement. Our `border-subtle` is #262b30 against a #181f26
card and does not survive 240p, see "Section headings", so structure takes
`border-mid` and only decoration takes `border-subtle`. The settings hint divider
had it backwards: the heavy `card-border-width` weight *and* the invisible
`border-subtle` colour.

**Weight** carries the hierarchy. The edge is `card-border-width`, every line inside
is a `stroke(1)` hairline; that is the stroke ladder above, unchanged.

**Spacing.** A structural divider is separated from the content on both sides by
`surface-pad`, the same value as the lip beside it.

## Modal chrome

| Surface | Token |
|---|---|
| Panel | see "Surface containment" |
| Scrim | `Theme.scrim` |
| Title | `Sizing.font-title`, `Theme.text-primary`, centered |
| Body/button | `Sizing.font-body` |
| Button surface | `PressableSurface`, `Sizing.radius-sm` |
| Button slot | `pct-h(7)` |
| Inset and every internal gap | `Sizing.surface-pad` |

**One shell owns the chrome.** `ModalShell` in `chrome.slint` draws the scrim,
the panel, the title and the rule; a host supplies a width and its own content
height and positions its content against `content-y`. Before it existed each
modal was a bespoke panel, which is how four different title gaps, two QR
presentations and three different header treatments grew, and why a change to
any one of them landed on one modal at a time.

One trap when hosting: `@children` are laid out inside the panel, but `parent`
in a host's own declaration still resolves lexically to the host's root, which
is the full-screen scrim. Position against `panel-width` and `content-width`,
never `parent.width`, or the content is measured against the screen and clipped
away at the panel edge.

**One width rule.** A panel that sizes to measured content caps at `0.92` of the
screen; a panel with a fixed width caps at `0.78`. Three different formulas had
grown here.

Panel has no border. Prefer extending the shell over bespoke chrome. Every
modal panel is shell-based: `DialogModal`, `ListPickerModal`,
`LetterJumpModal`, `CardWriteModal`, and `QrCodeModal` in `app.slint`,
`SetupModal` and `LogUploadModal` in `setup.slint`, and `GameInfoModal` in
`game_info.slint`. The setup panel's form rows are `SettingsRowView` rows.
`GameInfoModal` caps its scrolling body at a budget taken from the screen
height rather than growing past it, and round 10 gave its internal breaks the
shared `SectionHeader` heading (see "Section headings") instead of bare
spacing, borrowing the Settings vocabulary rather than inventing new bespoke
chrome for them. `ContextMenu` is the one panel outside the shell, because it
is an anchored transient; see "ContextMenu chrome".

### Modal depth

One modal at a time. Every design system that states a position says the
same thing (Apple's sheets: "display only one sheet at a time"; Material:
"avoid opening dialogs from within a dialog"; GNOME: "avoid stacking
dialog windows"; Fluent: "don't nest dialogs"; Carbon: "one modal should
never trigger another"; WinUI throws), and on a d-pad UI with no visible
back button two identical cards on two scrims give the user no cue of how
deep they are or how many Back presses get out. The rules:

1. A modal never opens another modal. Nothing warns at runtime: each modal
   is its own open flag on a Slint global (`Overlays`, `SetupModalView`,
   `LogUploadView`, `GameInfoView`), and `dispatch_action` in
   `rust/frontend/src/router.rs` hands a press to the first open surface in
   a fixed order that is not the order `App` paints them in. Two open at once
   would split what the user sees from what takes input.
2. A choice made inside a modal is made inside that modal. The panel swaps
   its content to the option list and back: a *page* of the same panel,
   the modal title unchanged, the row's own name as a section header over
   the list. A picks and returns; Back returns without changing, with
   focus on the row that opened the page. One level of pages only. See
   `SetupModal` in `rust/frontend/ui/setup.slint`, whose `picker-page` swaps
   the form for its picker page, and its driver
   `rust/frontend/src/media_setup.rs`, where Back clears the page and leaves
   the form's `index` on the row that opened it.
3. A modal task that needs more than one such page, or a decision that
   needs information the panel can't show, is a screen (a Settings
   sub-page), not a modal.
4. The only thing that may appear over a modal is an action-error alert:
   a system interruption. `report_action_error` in `router.rs` shows it
   through the decision dialog (`DialogModal` with `DialogKind.action-error`),
   which `App` mounts after every other modal and `dispatch_action` checks
   before every other modal. It opens nothing itself. Two alerts never stack:
   `ErrorQueue` in `zaparoo_app::action_error` shows one failure at a time,
   drops a duplicate of one already showing or queued, and holds the rest
   until the dialog is free. A failed alternate-version discovery closes the
   context menu first (`closes_context_menu`), so Back never returns to a
   "Searching…" row that can never resolve.
5. `ContextMenu` is an anchored transient, not a card. It closes before
   anything it triggers opens, and its alternate-versions page
   (`rust/frontend/src/alternates.rs`) is an in-place swap of its rows:
   Back returns to the rows it replaced (`context_action` in `router.rs`).
6. A screen may open a modal (Settings rows open `ListPickerModal`); a
   modal that closes and *then* opens another (View menu -> letter jump,
   error -> retry) is a sequence, not a stack, and is fine.

Panel width is content-driven for the decision dialog (`DialogModal`),
mirroring [ContextMenu chrome](#contextmenu-chrome)'s pattern: the max of the
measured title, the body (capped at a 45-character line so a long paragraph
wraps instead of forcing a wide panel), and the summed button label widths,
clamped between a degenerate-case floor (`Sizing.pct-w(30)`) and 92% of the
viewport, with a second, height-derived cap (`Sizing.pct-h(90)`). A panel whose
content sets its own width
(the QR code, the setup and log-upload panels, the legal notice) takes the
78% breathing-room cap instead. `ListPickerModal` measures its own entry
labels the same way `ContextMenu` does and holds that exact width against the
same 92% ceiling, not the 78% cap meant for content nobody measured. (Round 6
follow-up: the picker's swatch band routinely pushes its measured width past
78% of a small screen, so a 78% cap still truncated rows even after the
label-measurement fix below.)

### Picker swatch preview

`ListPickerModal` rows are plain centered labels by default. One picker, the
color-scheme picker, needs to show what a scheme actually looks like, not
just its name. `ListPickerModal` in `app.slint` turns on swatch layout for the
whole list when its `setting-id` is `colorScheme`; each row then paints three
boxes from `Theme.preview-color(id, n)`, a callback `theme.rs` binds to
`zaparoo_app::palette::preview_colors` (the preset's three authored colors, in
primary/accent/text order). A picker is assumed homogeneous, never a mix of
swatch and non-swatch rows.

In swatch layout the row's label (`PickerRowBody`) moves from centered to
left-aligned (`x: Sizing.pct-w(2)`, still vertically centered,
`overflow: elide`) and three small `Rectangle` swatches right-align in the
remaining width. Every other picker (view menus, launcher, resolution,
language, ...) has a different `setting-id` or none, so it falls through to
the original centered-label row untouched. `desired-w` adds the swatch band
and a label gap only when swatches are present, so the panel still sizes to
content per the content-driven-width pattern above. `preview_colors` is
independent of the applied scheme (it's a pure function of the requested id).
Each swatch box carries a `Theme.text-label` border at rest (round 6), a
near-black or near-white swatch previously sat at the same contrast as the
row's own resting background and disappeared into it; `text-label` is a mid
neutral held >=3:1 against `bg-deep` on every preset, so it separates either
extreme from the row. On a selected row the border flips to the cursor's
`content` color (`Theme.on-accent`) instead, the same fix the favorite heart
uses against the solid selection fill, see "Inverse-video rows" above.

Content-driven width measurement (this section and [ContextMenu
chrome](#contextmenu-chrome) below) carries deliberate slack over the
measured label width (the `preferred-width` of a hidden measuring layout):
`Sizing.stroke(2)` in `ListPickerModal`, `2 * Sizing.stroke(2)` in
`ContextMenu`. The slack covers labels painted on hinted integer advances that
can run a few px past the fractional measured width; a panel sized to the bare
figure, round 5's `ListPickerModal` bug, elided text that should have fit,
with most of the screen still empty.

### Themed QR codes

`QrCodeModal` (write-to-token and the documentation link) and
`LogUploadModal` (log upload success) both show a code rendered by
`rust/frontend/src/qr.rs`: the `qrcode`-crate matrix at one pixel per module
plus the four-module quiet zone, which the view scales to a whole multiple
with `image-rendering: pixelated` on the shared `QrPlate` in `chrome.slint`.
The two fills are palette roles, `Theme.qr-light` (quiet zone + background)
and `Theme.qr-dark` (modules), rather than hardcoded white and black.

`qr-light` stays the light rung and `qr-dark` stays the dark rung on every
preset, regardless of whether the preset itself is light or dark, inverted
QR is out of spec and scans unreliably on a phone camera, the primary use
of this component. Both ride the accent's own OKLCh hue instead, so the
code still reads as themed (a faint tint on the quiet zone, accent-hued
ink) without ever inverting (`zaparoo_app::palette::palette`):

```rust
qr_light: gamut_fit(0.965, accent_lch.c.min(0.022), accent_lch.h),
qr_dark: gamut_fit(accent_lch.l.min(0.45), accent_lch.c, accent_lch.h),
```

Measured contrast between the two rungs ranges 6.37:1 (Green Phosphor) to
7.43:1 (Synthwave '84) across the catalog, with `qr-light` always the lighter
rung; `palette_golden.txt` pins both roles for every preset.

`qr.rs` reads `Theme.qr-light` and `Theme.qr-dark` back off the global that
the rest of the frame is painted from (`code_colors`), so a code can never
disagree with the applied preset. The quiet zone is part of the code and
takes `qr-light` with the rest of the background, a white border around a
tinted matrix is the one thing here that would actually break a scan.
`QrPlate` rounds its corners by at most the quiet zone's width, so a radius
only ever eats light margin, never a module.

## ContextMenu chrome

A menu is a panel like any other: `bg-panel`, `radius-md`, no border, and
`Sizing.surface-pad` on all four sides, see "Surface containment". It had its
own inset a hair below every other panel's, which is how a scale becomes drift.
Rows are inverse-video directly against the panel fill, no nested row surface,
see "Two registers" and "Inverse-video rows" above. The row's own `pct-w(2)` text
inset is a horizontal measure inside a row, not the panel's lip; the two are
separate properties (`label-inset` and `pad`) so they cannot be conflated again.

**No fixed entry cap** (see `docs/content-style.md`'s menu-ordering and
menu-size rules for the content-side reasoning). `ContextMenu` in `app.slint`
scrolls to keep the focused row in view once `content-h` exceeds `panel-h`,
with the same reserved `ScrollCue` band the pickers use, and `context_action`
in `router.rs` wraps Up and Down, so the last entry is one press up from the
first. `panel-h` clamps to the window's usable area above the help bar and
the panel carries `clip: true` as a shipped-build safety net. Judge a menu by
whether it still fits without scrolling at 240p: a scrolled-past entry is an
undiscoverable one.

Row labels center the `Text` item itself on its measured width
(`ContextRowBody`), per the integer-pixel rule below, never a full-width
`Text` with `horizontal-alignment: center`. Panel width tracks content:
`panel-w` clamps `desired-panel-w` (the widest entry label plus padding)
between a degenerate-case floor (`min-panel-w: Sizing.pct-w(12)`) and the
available width, so a menu with short labels narrows instead of always paying
for a fixed minimum panel width.

Four `Theme.scrim` bands frame the anchor's hole so the anchor stays bright;
when the anchored tile holds its focus zoom (`anchor-zoomed`), the hole grows
by the same `Motion.focus-zoom`. Dimensions clamp to nonnegative values. Row
`TouchArea`s take hover and click inside the panel; a press outside it lands
on `App`'s modal blocker and is swallowed rather than dismissing the menu.

When the caller passes `anchor-radius` (the anchored tile/row's own corner
radius, set through `set_context_anchor` in `router.rs`), four antialiased
quarter-disc masks cut the bands' square hole down to the anchor's actual
rounded silhouette, closing the bright square notches a plain rectangular hole
leaves past a rounded tile's arcs. Each mask is an `r` by `r` square with the
tile's own corner arc taken out of it, generated at the exact radius by
`corner_cut_svg` in `rust/frontend/src/glyphs.rs` and served through
`GlyphSource.glyph("corners/cut-tl", ...)` (and its three siblings) in
`Theme.scrim`. The masks sit inside the hole, never over a band: the scrim is
translucent, so an overlap would composite to a darker patch.
`anchor-radius: 0px` (the default) keeps the plain square hole. Corners are
skipped, not overlapped, when the hole is narrower than two radii on either
axis. `QrPlate` uses the same four masks to round a code's plate.

## Tile aspect and grid blocks

Hub tiles are square above 240p: the `square_cells` argument of
`zaparoo_app::paged_grid::fit` clamps the cell width and height to the smaller
of the two independent per-axis fits, fit against both axes of the Hub's own
reserved band (`zaparoo_app::hub::geometry` passes it for every tier but 240p).
This is a grid-level, opt-in argument (false for every other caller) rather
than a Hub-only calculation, because a naive width-only fit, capped by a
constant hand-tuned for one specific row count, silently stopped being square
the moment the row count varied by tier (round 6 follow-up; see
`hub_grid_shape` in `zaparoo_app::sizing`). The Hub also passes a
`height_budget` (`hub_grid_height_budget`, a fixed ceiling distinct from the
grid's own height, since that height is itself derived FROM the fitted cell
size, fitting against it would be circular). Systems and media grids use
`declared_grid_shape`'s common resolution shapes with the adaptive scorer as
fallback for nonstandard desktop/TATE scenes, and do not ask for square cells.
`fit` floors uniform cell dimensions, then centers the cells-plus-insets block
against the full inset-to-inset width; odd remainders may differ by one pixel
only.

Default-theme grid gaps (`crt` keeps its own raw pixel values, unaffected):
the systems and games grids both take a column gap of `pct-w(2)` and a row gap
of `pct-h(3)` from the default profile in `zaparoo_app::layouts`, pushed as
`Layout.grid-column-gap` and `Layout.grid-row-gap`.

### Compact tile padding on full-bleed icon/cover grids

`Tile`'s `pad` (`Sizing.pct-h(2)` on three sides in non-caption mode) insets
art on every `Tile` caller by default, generic to the component, not
specific to any one screen. Hub tiles and Settings' own root category grid
(which caps its cells at the Hub's tile size, `hub_tile_width` and
`hub_tile_height`, so its tiles read as the same physical object as the
Hub's, see above) are both full-bleed icon/cover tiles with no caption band
competing for space, so round 8 gave them an opt-in `compact-padding` property
(same opt-in shape as `square_cells`) that tightens the inset. Round 8 shipped
this as an independent `Sizing.pct-h(1)`, which landed exactly on the focus
ring's own inner edge (`outline-gap + outline-width`, both also
`Sizing.pct-h`-derived) at every resolution tier, art touched the ring by
construction, not rounding. Round 9 derives `pad` from the ring's own geometry
instead (`ring-inner-edge + Sizing.pct-h(0.4)` under `compact-padding`), so a
future change to either ring token can't silently close the gap again.
Systems tiles are a separate, viewport-fitted grid and stay on the default
padding; only Hub and Settings' category grid opt in.

The Standard profile keeps Hub columns fixed by resolution tier rather than
viewport fitting, so a window resize cannot scramble a hand-arranged layout.
The Handheld profile deliberately widens the high-resolution Hub shape to
6×3. It reflows the same persisted linear slot order without resetting it;
switching profiles changes pagination, not ownership or ordering. Low-resolution
tiers already use the compact 4×2 shape and remain unchanged. A separate lever,
`image-fit: cover` for game covers, remains set aside: it enlarges art
but crops the top and bottom, where title art often sits.

### Spent cues

A directional cue whose direction has nothing left to reach stays painted and
changes color. It does not disappear: one press the other way brings it back,
and hiding it takes away the only sign that the surface scrolls at all. The
rule of thumb the industry settled on is to disable a control a user action can
re-enable and hide one nothing can, and a scroll arrow is squarely the first
case.

The spent color is `Theme.cue-spent`, **not** `Theme.text-label`. WCAG 1.4.1
allows a state carried by color alone only when the two colors differ by at
least 3:1 in luminance. Measured across the palette fixture at Subtle
intensity, `text-label` against `text-primary` manages that on three of the
twenty presets: 2.07:1 on Nord, 2.59:1 on the default, and only the three light
schemes pass. That is why a spent arrow read as "very easy to miss" rather than
as a state. `zaparoo_app::palette::spent_from` derives the role per scheme by
walking `text-primary`'s `OKLCh` lightness toward the page until the pair clears
the bar, preserving hue and chroma the way `clamp_accent` does; the two tests
beside it hold every preset above 3:1 against the text and above 1.5:1 against
the page, so a spent arrow is always both distinguishable and visible.

One component owns the rule: `ScrollCue` in `chrome.slint`. `PageIndicator`'s
own chevron pair, the list picker, the setup picker page, the About card, the
game info body and the game info image switcher all go through it. Hosts still
decide whether the *pair* appears at all, which is a different question: a
surface that does not scroll shows neither arrow.

Position is a separate job from direction, and `PageIndicator` carries it as
text. Three modes: page N/M for paged grids, item N/M for lists, and a
percentage for continuously scrolled prose that has no items to count.

There is deliberately no persistent position indicator beyond that readout.
Nothing in this product category has one: Steam Big Picture, EmulationStation,
ES-DE and Plex all answer "where am I in thousands of items" with a transient
alphabetical overlay during fast scroll plus a jump-to-letter control, which is
what the rapid-scroll letter plate (`GamesView.rapid-letter`) and
`LetterJumpModal` are. A segmented track was
prototyped and rejected: it collides with the header's own `ProgressTrack`, and
it degrades into mush long before a real library size.

### No scrollbars, grids are paged, not scrolled

`PagedGridView` has no in-grid scroll indicator of any kind, no gutter, no
proportional thumb. It never did have a scroll-position concept to represent
(it pages, it doesn't scroll); a right-side gutter conditionally reserved
whenever the dataset happened to be multi-page was the wrong metaphor, and
worse, it made the cell block visibly shift and shrink the instant a
single-page grid became multi-page (arming Hub Options → Move always reserves
a second page, so this fired on every single arm). `paged_grid::fit`'s
`available_width` is simply `width - left - right`, nothing subtracts space for a
gutter, so a grid's geometry is identical regardless of page count.

The "where am I" cue is a count badge plus `PageIndicator` (up/down chevrons,
`ScrollUp`/`ScrollDown`, the same glyphs the old gutter used, plus "N / M").
Paged grids pass page numbers; detailed lists pass focused-item and total-item
counts. The text only paints once there's actually somewhere else to navigate
to, `PageIndicator`'s `has-navigation-range`, since a single page or item never
needs a "1 / 1" readout. Round 9 changed the chevrons' own no-direction state from hidden to
dimmed so a bare glance could tell "there is scroll chrome here" even
mid-grid, and only the still-live direction reads as actionable, a colour
swap, not an opacity one: a translucent node would repaint everything it overlaps on every frame
under software rendering (see the animation-cost rules in AGENTS.md), while a
static colour costs nothing extra. Round 10 added the missing case: when
there's only **one** page or item total, both chevrons hide entirely
(`has-navigation-range`, the same gate the "N / M" text already used) rather
than painting two permanently-dim arrows that will never do anything,
dimming is for "this direction specifically has nothing," not for "nothing
here scrolls at all." It sits alongside `TopStatusStrip`'s title,
baseline-aligned to it (`TopStatusStrip`'s `page-indicator-mode`), on every
theme except CRT, CRT hides that strip entirely
(`Layout.top-strip-visible: false`) and keeps the same cue in the host screen's
**footer** instead, alongside `ActiveLabel` (`Layout.page-cue-in-footer`,
resolved by `zaparoo_app::layouts`, is the profile flag both placements key
off). Wherever it lives, the badge and
`PageIndicator` are unconditionally reserved, only the count text's and
each chevron pair's presence, and each chevron's own colour, toggle, so a
single-page grid becoming multi-page (arming Hub Options → Move always
reserves a second page) never shifts anything. The Settings card, the About
card, and `GameInfoModal`'s own scroll chevrons got the same
dim-plus-hide-on-single-page treatment for one consistent rule. The list
picker's chevrons were once claimed to satisfy it by construction and did not:
they hid one arrow per direction and painted the other dim in *both* states.
Every one of them now goes through `ScrollCue`; see "Spent cues" above,
which is where the dim/hide rule and the colour now live.

Two placements exist because putting the cue at the top, next to a title
that's already there, was tried first (pre-round-5) and reads better once a
footer that's ALSO carrying the focused item's own title has room to spare,
`ActiveLabel`'s `side-inset` reverts to its own default (`Sizing.pct-w(3)`)
instead of a corner-slot reservation whenever the footer isn't hosting the
count/page slots, roughly doubling the room a long focused title gets before
eliding. CRT's footer is the one place that still needs the full three-slot
arrangement, since CRT has nowhere else to put it. `PageIndicator` in
`chrome.slint` places each chevron and the text at a fixed x off the element
before it rather than in a layout, so hiding the chevrons never shifts the
"N / M" text, and its `chevron-spacing` is a tighter gap between the two
chevrons than `item-spacing` between the pair and the text,
Gestalt proximity: the chevrons are one control, the text is a separate
readout, and the glyphs' own baked-in side bearing already makes an *equal*
gap read backwards. Detailed `BrowseList` layouts use this same cue for
single-item movement and omit the separate left-side total. The Settings card
uses it too, in item mode in the page's own `TopStatusStrip`, so the cue costs
the card no room.

### Empty slots

A grid cell that is a deliberate structural placeholder (not a real item with
nothing on it) is a `GridCell` with `is-empty` set, and `PagedGridView` mounts
no `Tile` for it. The Hub pads its pages with them
(`zaparoo_app::hub::pad_to_page_size`).

An empty slot paints nothing, ever, not even a focus ring. Normal browsing
cannot land the cursor on one at all: `skip_empty_cells` on
`zaparoo_app::paged_grid::Grid` (which the Hub sets to `!move_armed()`, with
`PagedGridView`'s `skip-empty-cells` mirroring it for the pointer) makes every
cursor path, `move_selection`, `page_by`, and the per-cell mouse hover/click,
treat an empty cell as unreachable.
Left/Right still step past a blank within the same row, same as before; Up/Down
instead run a nearest-candidate search across every real tile on the whole
board, a same-column-only walk tunnels past nearby content on a freely
arranged Hub layout, landing on a distant aligned tile instead of a much
closer one a column over. See `nearest_vertical_candidate`'s doc comment in
`paged_grid.rs` for the weighting (borrowed from Android's `FocusFinder`, the
algorithm behind d-pad navigation on Android TV). The one place a blank IS
still a legitimate destination is a Move session (Hub Options → Move), which
needs to target one, or the reserve page `begin_move` in
`rust/frontend/src/hub.rs` pads in, to place a held tile there; Move disarms
`skip_empty_cells` for exactly that reason, and the held tile swaps into the
cell within the same synchronous call, so no frame ever actually paints an
empty cell under the cursor either way.

This wasn't always the rule. An earlier version let the cursor rest on a gap
and drew an accent ring around it so it still read as "here." It didn't work:
every other focused thing in the app is a solid card with a ring wrapped
around it, and a ring drawn over a face-less cell has nothing to wrap,
it read as a stray rectangle floating on the background, not a cursor. WAI-ARIA's
own guidance points the same way (disabled/inert things are normally not
focusable at all), and the nearest real-world analogue, iOS 18's home-screen
gaps, only accepts a drop while you're actively rearranging, exactly the
Move-only rule here. Landing on a blank had exactly one other job: aiming
View → Add item… at that specific gap. Add now arms Move on the newly placed
item instead, so the "put it exactly where I want" affordance survives
without needing the cursor to ever rest on empty space outside a Move
session.

### The held tile in Move mode

A tile held for a Move (Hub Options → Move) blinks out of existence and
back on a `Motion.held-blink-ms` cycle: nothing is painted in that cell for
the instant it is off, focus ring included, and it returns exactly as it
was. No tint, no recolor, no lift, a hard on/off cut, implemented as
`opacity` toggling between exactly 0 and 1 so one binding takes the art,
the caption and the ring with it.

~0.77 Hz is well under WCAG's 3 Hz flash threshold, and one Hub tile is a
small fraction of the screen; rate and area are the safety factors here,
not the transition style. Under Reduce motion the tile simply stays on.
Freezing a disappear cue on "gone" would hide the very thing the user is
moving, and unlike a color there is no restable middle state for it, the
help bar's move-mode line carries the state for those users instead.

## Selection motion

Two cues exist and they are not interchangeable. A **grid tile** carries a
focus ring, because a tile is art and inverting it is not available. A
**vertical option list** (browse, Settings, menus, pickers) inverts the row
outright, and a ring on top of that is a second mark for one state. Lists
therefore have no ring; what moves is the fill itself.

### The travelling fill

`SelectionCursor` paints one rectangle and slides it. Each row of a list is
drawn twice: once in its resting colors, and once inverted inside a
`SelectionClip` bounded by the fill's current band. The inversion boundary
is the fill's own edge, so a row the fill is half over is half inverted.

This is not decoration, it is the only correct answer. `Theme.on-accent` is
near-black; a row whose text flips before the fill arrives is black on a
dark panel, and one the fill has left is black on nothing. A per-row
boolean cannot express a partial state, so a moving fill and per-row text
colors are mutually exclusive. Pick one.

The clip paints an opaque `selection-fill` rather than clipping alone,
because the resting copy underneath would otherwise composite through the
inverted glyphs and fringe every antialiased edge. It rounds only the
corners that are the fill's own ends, so two segments meeting at a row
boundary read as one bar.

### The activation flash

Accepting a row swaps the fill and its content for `Motion.press-ms` and
swaps back: the whole row flashes inverted, a hard cut both ways, nothing
moves. A text row is not a button, so it gets no depth cue, this is the
DOS-terminal-flash half of the [Two registers](#two-registers) language.

The flash lives on `SelectionCursor`, not on the row, because the thing it
has to invert is the cursor's one rectangle. An earlier version kept a
per-row selection bar owning the timer and, having no fill left to swap, drew a
thin `on-accent` stroke inside the selected row instead. That is a
different cue, and it read as no cue at all. The cursor now exposes
`flashing` plus the two colors the swap produces (`fill` and `content`),
each row's clipped copy paints from them, and there is one timer per list
instead of one per row.

A row whose control answers for itself does not flash: `flash-enabled` is
false while the selection is on a toggle, because the knob sliding is that
row's activation cue. The gate is on the cursor rather than on the row
precisely because the fill is shared, suppressing only the text would
leave the bar inverting under an unchanged label.

### What animates

Distance decides, never index adjacency. Index deltas are a model detail:
a group header occupies a row slot without being selectable, so two rows
that touch on screen are two indices apart. A change in row height is a
move, not a discontinuity; animate `y` and height together.

Duration scales with the distance travelled, non-linearly, floored at the
one-row hop and capped at 2.5x it. This follows Carbon, which scales
duration "to achieve better perceived consistency across all distances",
and keeps everything inside the 100 to 400ms band NN/g identifies as
usable. A single flat duration makes short moves sluggish or long ones a
smear; there is no value that serves both.

### What snaps

Three things, and only these:

- **A scope change.** Different page, different route, different windowed
  list. There is no spatial relationship to express.
- **A wrap.** Pressing right at the end of a row and landing at the start
  of the next, or the last tile wrapping to the first. tvOS is explicit
  that focus movement must follow the spatial layout; animating a
  right-press into a leftward sweep asserts a relationship that is false,
  and on a handheld at arm's length it is the most likely thing here to
  read as motion sickness.
- **A page turn.** The rows underneath are replaced, so there is nothing
  for the highlight to travel across.

Anything beyond `travel-limit` row heights is treated as a jump and snaps
too, which is what catches a wrap inside a short list where the pixel
distance alone would look harmless.

### Focus zoom on tiles

A focused grid tile scales to `Motion.focus-zoom` (104%) about its own
centre, and its ring scales with it so the two stay matched. Three rules
govern it.

**It is an addition, never the indicator.** WCAG 2.4.13 Focus Appearance
requires the focus indicator to carry a 3:1 contrast change over a minimum
area. A size change is neither, so the ring stays and the zoom sits on top
of it. Do not "simplify" the ring away because the zoom reads well.

**Small, because scale is the sensitive class.** Vestibular guidance
singles out zoom over translation: a scaling animation reads as the viewer
moving forward or backward in space, where a slide does not. Leanback ties
its zoom factor inversely to item size and reserves the large factors for
small items; our tiles are large and the screen is close, so 104% rather
than the 110% a TV app would use.

**Tiles only, never text.** Big Picture also scales the focused list row's
text. Scaling text changes its measured width, so a row that fits at rest
ellipsizes when focused, which is a content change on every focus move,
and our captions already run a marquee. The row fill already inverts the
whole row, which is the stronger cue anyway.

Grid clipping has to allow for it: `PagedGridView` widens its clip by the
growth and pushes the cell layer back in by the same amount, so a tile on
the grid's own edge is not trimmed on its outer side and no tile changes
size to make room.

The software renderer has no transform support, so neither the tile nor its
ring scales there, and they stay consistent with each other. So that nothing
reserves room for growth that never happens (the clip headroom, the context
menu's scrim hole), `main.rs` pushes `Motion.focus-zoom` as 100% on MiSTer and
`PagedGridView` skips the zoom on CRT. Snapshots render through that same
renderer, which means the zoom cannot be checked offline; it is a
device-verified effect.

### Reduce motion

`Motion.enabled` gates all of it, with no exceptions and no partial
fallback. WCAG 2.3.3 requires interaction-triggered motion be disableable,
and `display::motion_enabled` also folds in framebuffer height, so native
1080p on MiSTer snaps without anyone opting in.

## Consistency rules

- A container is a panel or a card, and carries one containment signal, not
  two: a scrim replaces an outline. See "Surface containment".
- A directional cue goes through `ScrollCue` and dims to `Theme.cue-spent`
  when its direction is spent; it never disappears. See "Spent cues".
- A container's inset is `Sizing.surface-pad` on all four sides, and the same
  value again between the blocks stacked inside it.
- A line inside a surface goes through the shared `Divider`. See "Lines".
- Rounded square chooses `radius-md` or `radius-sm`; pill chooses half-height.
- Grid content and standalone commitment buttons use `PressableSurface`;
  vertical option lists (browse, Settings, menus, pickers) use the list's own
  `SelectionCursor`, and never a focus ring as well.
- Focus uses `Theme.accent`; an inverted row uses `Theme.selection-fill`.
- A rows band clips on row edges, never through a row. `band_extent` in
  `zaparoo_app::settings` owns that rule for the Settings card.
- Ordinary text chooses six-role ladder.
- Geometry comes from `Sizing` and `Layout` tokens (`pct-h()`, `pct-w()`,
  `pct-min()`, `stroke()`), never a hardcoded pixel size, and Slint snaps it
  to physical pixels.

## Integer-pixel drawing

Rules apply everywhere, not only CRT:

- static geometry lands on whole physical pixels; Slint's renderer snaps it,
  so do not round visual geometry by hand
- stroke widths are whole pixels (`Sizing.stroke()` and the stroke ladder)
- CRT text remains 8/16px
- user-visible centered text centers item, not a half-pixel glyph run

Rounding by hand is reserved for exact-pixel contracts (CRT calibration
guides, bitmap raster sizes, QR modules). Document any exception beside code
that needs it.
