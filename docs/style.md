# UI Style

Zaparoo Frontend's design language. `Theme.qml` owns color and font tokens;
`Sizing.qml` owns integer geometry, resolution tiers, radii, type roles, and
stroke weights. Anything not covered here defers to those singletons.

UI runs on Qt Quick's software adaptation: no shaders, shadows, gradients, or
`Shape`. Build surfaces from `Rectangle`, `Item`, `Text`, and `Image`.

## Color schemes

A preset authors exactly **three** colors and `ColorSchemes.qml` derives every
other semantic role from them:

| Authored | Role |
|---|---|
| `primary` | Page background, and the base every surface mixes from |
| `accent` | Focus, and every highlight in the UI |
| `text` | Primary content color |

There is no fourth color. State markers — the favorite heart, the Hidden badge —
use `accent` rather than a marker color of their own, so adding a preset never
means picking a coordinating fifth hue.

Two roles do not derive. `scrim` is always a dark veil, because its job is to
separate a panel from what sits behind it whichever direction the preset runs.
`errorHex` is a semantic constant: deriving it from the accent would make an
amber preset signal danger in amber.

Derivation direction follows `up = luma(text) > luma(primary)`, so light presets
work without a separate ladder. `Qt.lighter()`/`Qt.darker()` are HSV value
multiplies and are no-ops on pure black — the catalog mixes channels explicitly
instead, and components must too.

The two logo ramps and `markerOutline` (below) do not mix toward `primary`/
`text` directly — that axis is what silently ran backwards on the `zaparoo-white`
preset. They mix toward `lightPole`/`darkPole` instead, the same two colors
ordered by luma (`up ? text : primary` / `up ? primary : text`), so the shadow
rung of a ramp is always the darker end regardless of which of `primary`/`text`
happens to be light on a given preset.

`Theme.qml` exposes the derived roles to components. Never branch on scheme ID in
a component or hardcode a preset color outside the catalog. `zaparoo-black` is
fallback for missing, unknown, or removed IDs.

Selection applies live and persists as `[settings] color_scheme` in
`frontend.toml` plus `state.toml`. Tinted image URLs naturally change with
palette roles; custom and full-color artwork remains unchanged.

Status icons (`Resources.statusIconUrl()`, `resources/images/status/`) and the
general UI glyph set (`Resources.iconUrl()`, `resources/images/icons/`) are
also SVGs routed through the tinted-svg provider — a required `color` argument
fills all three tint slots so the glyph is a flat tint rather than a ramp.
Every source SVG in both directories is authored white-on-transparent, and a
raw white glyph disappears on a light preset whose `bgBar` resolves lighter
than white; callers pass `Theme.textPrimary` (or another in-scope text/accent
role for context, e.g. `Theme.logoFocusPrimary` where a glyph shares a slot
with tinted cover art) rather than loading the qrc path directly. Gamepad
button glyphs (`Button*`) and D-pad glyphs (`Dpad*`) stay raw PNG — they are
rasters with their own baked shading, not tintable SVGs, and `iconUrl()`
returns their path unchanged regardless of `color`.

## Cards: focusable surface recipe

Card means selectable surface that guarantees content contrast regardless of
page background. Keep opaque card plates under cover art; future custom
backgrounds must not weaken art, caption, or focus contrast.

| Property | Value |
|---|---|
| Fill | `Theme.surfaceCard` |
| Static border | `Sizing.cardBorderWidth`, `Theme.borderMid` |
| Focus border | `Sizing.focusBorderWidth`, `Theme.accent` |
| Outer/card radius | `Sizing.radiusMd` |
| Nested control radius | `Sizing.radiusSm` |

Tile bodies, browse cards, detail panes, and About body use `radiusMd`.
Settings rows, modal buttons, menu/picker rows, nested list selection,
scrollbar thumbs, toggle tracks/handles, and rapid-scroll chrome use `radiusSm`.
Checked toggles use a `bgDeep` handle against the accent track; unchecked toggles
use `textPrimary` against `borderMid`. Handle insets preserve integer centering.

### Tile focus ring

`Tile.qml` draws focus as two stacked filled rectangles rather than
`border.width`. Outer accent and inner `surfaceCard` mask avoid stepped rounded
borders under Qt software rendering (QTBUG-123210). Ring thickness is
`Sizing.focusRingWidth`; Tile and PagedGrid placeholder geometry stay
synchronized during press/rapid-render states.

Tiles use a physical front edge inside their existing cell footprint. Activation
lowers artwork, caption, badges, and ring together without scaling cover art.

### Recessed-slot latch (browse lists and Settings rows)

Browse lists and Settings rows are both one containing card with flat rows;
never turn every row into a separate raised button. Selected row uses
`selectionSurface`, one opaque `selectionShade` keyline along its **bottom**
edge, and the accent rail. Activation moves only accent rail, title inset, and
optional favorite inset inward by `Sizing.focusRingWidth`, then holds until
release/settling. This reads as a cursor engaging a recessed slot.

The keyline is on the bottom because a recess and a raised object shade
oppositely under the same light. The scene is lit low and from the front, so a
raised `PressableSurface` catches that light on its near face while the near wall
of a recess falls into shade. A top keyline inverts the recess and fights the
tiles beside it.

The highlight itself lives once in `LatchSurface.qml` so `BrowseList` and
`SettingsField` cannot drift apart — each row mounts one, binds its own label's
leading inset to the surface's `latchOffset`, and forwards its host's
`activatePulse`/`releasePulse` (or, for a single-row field with no separate
"selection moved off this row" signal, a locally timed release). Settings rows
using `control: "toggle"` are exempt: the knob slide is their own activation
cue, so `LatchSurface` never latches for them. `ContextMenu` rows stay on
`PressableSurface` below — a menu entry *is* a button and a press-in is correct
there.

### Pressable front edge

Grid tiles, buttons, ContextMenu rows, and picker rows use `PressableSurface.qml`.

**Front edge is not a shadow.** It is opaque physical material. Grid tiles use
`Theme.tileEdge` against `bgDeep`; controls use `Theme.controlEdge`, one step
further along the accent ramp, when embedded in panels. Do not describe either as
"lighter" — on a light preset both are darker than their ground. The invariant,
pinned by `tst_pressable_surface.qml`, is that the control edge sits *further
from the ground* than the tile edge, in whichever direction the preset runs.

Edge top corners are square and extend behind the face by its corner radius,
while only bottom corners remain rounded. Never use black, transparency,
gradients, or shader effects. Rest exposes `Sizing.pressEdgeHeight`; press moves
face/content down and collapses exposed bottom edge. Motion routes through
`Motion`, so Reduce motion and native 1080p MiSTer snap immediately.

**Why it reads as gloss.** Two properties do the work, and both are easy to lose
in a refactor that "simplifies" the edge:

1. The edge is *more saturated* than the face and sits well off it on the accent
   ramp. A cast shadow would be a desaturated darkening; a chromatic strip along
   the near face reads as reflected light instead.
2. Because the edge rect is `edgeHeight + radius` tall with square top corners
   and rounded bottom corners, its color fills the notch left by the face's
   rounded corner and follows that curvature. A highlight that follows curvature
   is the canonical gloss cue and is the larger of the two contributions.

The specular read genuinely depends on a dark ground. On a light preset the
accent-derived edge is darker than the face and reads as a correctly-lit bevel
rather than a shine. Depth survives; the shine is a dark-preset property. Tests
therefore assert chromatic separation along the accent ramp, not lightness.

## Plain text on background

Non-interactive text may sit directly on `Theme.bgDeep`:

- TopStatusStrip titles
- Settings section headers
- global Loading cue
- ActiveLabel selected name

Use `Theme.textPrimary` for primary content and `Theme.textLabel` for metadata.
Use Body or larger unless space has an explicitly documented specialist role.
Pressable content belongs on card/control surface.

## Focus

Focus is always `Theme.accent`. No second focus color. Accent configurability is
future theme work, not per-surface override.

## Pills

Toggle track/thumb use `height / 2` or `width / 2`. Pills are distinct from
rounded squares and remain borderless; outer Settings row carries focus.

| State | Fill |
|---|---|
| On | `Theme.accent` |
| Off | `Theme.borderMid` |

## Colors

Every UI color comes from `Theme.qml`; never inline hex except diagnostic
calibration surfaces with documented reason. Roles have no fixed hex — they are
derived per preset from `primary`, `accent`, and `text`. `mix(a, b, t)` is a
per-channel lerp; `surface(t, bias)` is `mix(mix(primary, text, t), accent, bias)`,
the neutral ladder with a slight accent cast; `ink` is the pole away from `text`.

| Token | Derivation | Use |
|---|---|---|
| `bgDeep` | `primary` | Flat page background |
| `bgBar` | `mix(primary, ink, 0.35)` | Help bar |
| `bgPanel` | `surface(0.05, 0.04)` | Modal/ContextMenu panel |
| `surfaceCard` | `surface(0.08, 0.05)` | Card/control face |
| `borderSubtle` | `surface(0.14, 0.03)` | Low-contrast edges |
| `borderMid` | `surface(0.32, 0.05)` | Resting card edge |
| `selectionSurface` | `mix(edgeBase, accent, 0.22)` | Selected list row |
| `selectionShade` | `mix(selectionSurface, black, 0.40/0.12)` | Selected-row bottom keyline |
| `tileEdge` | `mix(edgeBase, accent, 0.44)` | Grid tile front edge |
| `controlEdge` | `mix(edgeBase, accent, 0.54)` | Control front edge in panels |
| `scrim` | `#cc000000` | Modal scrim, always dark |
| `textPrimary` | `text` | Primary text |
| `textLabel` | `mix(primary, text, 0.62)` | Secondary text |
| `textVariant` | `surface(0.58, 0.14)` | Tinted secondary text |
| `accent` | `accent` | Focus, and every state marker |
| `logoShadow` / `logoSecondary` / `logoPrimary` | `mix(accent, darkPole, 0.55)` / `mix(accent, darkPole, 0.18)` / `mix(accent, lightPole, 0.28)` | Resting tinted-artwork ramp |
| `logoFocusShadow` / `logoFocusSecondary` / `logoFocusPrimary` | `mix(accent, darkPole, 0.30)` / `accent` / `mix(accent, lightPole, 0.85)` | Focused tinted-artwork ramp |
| `markerOutline` | `mix(luma(accent) > 0.5 ? darkPole : lightPole, accent, 0.12)` | Favorite-heart / marker keyline |
| `errorHex` | `#ff4f91` dark / `#c2185b` light | Failure text |

`edgeBase` is `mix(primary, text, 0.06)` — one rung up the neutral ladder, so the
accent ramp keeps some body on a near-black primary. `selectionShade` is
direction-dependent (`0.40` dark preset / `0.12` light preset) because equal
channel mixes are not equal perceived steps at opposite ends of the sRGB curve;
the logo ramps and `markerOutline` sidestep the same problem by mixing toward
`lightPole`/`darkPole` (luma-ordered) instead of needing a direction-dependent
weight of their own.

### Header logo asset ladder

The bundled wordmark logo cannot be recolored by a palette role — it's a
full-color brand mark, not a single-hue tinted glyph. `HeaderBar.qml`
instead selects between two pre-rendered PNG variants under
`resources/images/logo/logo-<variant>-<w>.png`:

- `on-dark-<w>` — light wordmark, for `zaparoo-black` / `midnight-amber`.
- `on-light-<w>` — dark wordmark, for `zaparoo-white`.

`Theme.lightSurface` (`ColorSchemes.isLightSurface(id)`) picks the variant.
It is deliberately not a palette role — it's excluded from `requiredRoles`
in `tst_color_schemes.qml` — because it selects an asset, not a color.
`w` is one of `96, 144, 192, 256, 384, 600` (600 is the largest rung, at the
master's own aspect ratio). `Resources.logoUrl(paintedWidth)` is the single
place that snaps a painted width to a rung (`Sizing.snapLogoWidth(px)`,
mirroring `snapCoverTier()`'s "snap up" contract for grid covers) and picks
the variant; `HeaderBar.qml`, `AboutScreen.qml`, and the screensaver's
bouncing copy all call it instead of hardcoding a path, so none of them ever
decode a texture larger than their own painted size. There is no unscaled
monolithic `logo.png` any more — every call site goes through the ladder.

## Resolution tiers

Shape/type tier uses effective unrotated scene height. TATE swaps axes before
tier selection; CRT always uses `crt` despite safe-area reduction.

| Tier | Effective height |
|---|---|
| `240` | below 400 |
| `480` | 400–519 |
| `540` | 520–659 |
| `720` | 660–899 |
| `1080` | 900+ |
| `crt` | any CRT-native scene |

Thickness scales with resolution. Shape and hierarchy use discrete tiers.

## Fonts

`Theme.fontUi` is Noto Sans normally and MxPlus HP 100LX 6x8 when
`Theme.bitmapType` is set. `Theme.fontMono` is diagnostic/log text only.

`bitmapType` is a typography-only flag, separate from `crtNativePath` (CRT
*layout*: overscan insets, grid density, high-DPI pinning, TATE). It is true
on the CRT native path, and also auto-engages on embedded hardware at 240p
(`logicalVideoHeight < 400`) even without `--crt` — a proportional
antialiased face at 8-14px is illegible at that resolution, and the desktop
build never sets it outside the CRT path because a resizable window can cross
the 400px tier boundary at runtime. See `main.cpp`'s `bitmapTypeEnabled`.

### Type ladder

Six ordinary text roles only:

| Token | Role | 240 | 480 | 540 | 720 | 1080 |
|---|---|---:|---:|---:|---:|---:|
| `Sizing.fontHero` | Page/selected title | 14 | 22 | 24 | 29 | 43 |
| `Sizing.fontTitle` | Modal/detail title | 12 | 18 | 20 | 23 | 35 |
| `Sizing.fontSection` | Section/list/status | 11 | 17 | 18 | 21 | 31 |
| `Sizing.fontBody` | Body/control/help | 10 | 16 | 17 | 19 | 28 |
| `Sizing.fontCaption` | Secondary/menu/tile fallback | 9 | 14 | 15 | 17 | 26 |
| `Sizing.fontSmall` | Tile/detail small print | 8 | 13 | 14 | 16 | 24 |

When `Sizing.bitmapType` is set, the six tokens resolve through
`Sizing.fontSize(percent)` instead of this ladder and retain mandatory 8/16px
bitmap quantization — the bitmap face only ships those two strikes, so the
ladder and the quantization must move together. `fontSize(percent)` remains
for approved specialist sizes and geometry such as header row height; do not
use it to invent a seventh ordinary text role.

Use `renderType: Text.NativeRendering`. Center Text item with
`Sizing.center()` and keep glyph run left-aligned when pixel sharpness matters.

## Stroke ladder

| Token | Formula | Use |
|---|---|---|
| `cardBorderWidth` | `stroke(pctH(0.2))` | Resting card edge |
| `focusBorderWidth` | `stroke(pctH(0.4))` | Focused controls |
| `focusRingWidth` | `stroke(pctH(0.6))` | Tile/placeholder ring |
| `pressEdgeHeight` | `stroke(pctH(0.8))` | Control front edge |

Dividers, tiny badges, help-bar edges, and other literal hairlines stay
`Sizing.stroke(1)` unless promoted deliberately.

## Radius ladder

| Tier | `radiusMd` | `radiusSm` |
|---|---:|---:|
| `crt` / `240` | 2 | 1 |
| `480` | 3 | 2 |
| `540` | 4 | 2 |
| `720` | 6 | 3 |
| `1080` | 8 | 4 |

Nesting rule: inner surface uses smaller rung. Pills remain separate family.
No percentage-derived rounded-square radius.

True super-ellipses need unsupported Shape/shader paths. Frontend no longer
wants large squircle arcs anyway: small circular corners land close to integer
right angles and avoid software-rasterizer fringe artifacts.

## Padding scale

Padding tightens inward:

| Layer | Typical inset |
|---|---|
| Grid edge | `pctW(5)` sides, `pctH(2)` vertical |
| Modal panel | `pctW(4)` sides, `pctH(4)` top |
| Card content | `pctW(2)` sides |
| About body | `pctW(3)` sides, `pctH(3)` vertical |
| Tile caption side inset | `pctH(2)` (matches `Tile._padding`, the cover art's own inset) |

Radius never doubles as padding. ContextMenu panel vertical padding is
`pctH(1.5)` independently of panel radius. Tile caption inset shares the
cover's own padding rather than a radius-derived value so caption text clears
the focus ring's inner edge instead of running under it.

## Modal chrome

| Surface | Token |
|---|---|
| Panel | `Theme.bgPanel`, `Sizing.radiusMd` |
| Scrim | `Theme.scrim` |
| Title | `Sizing.fontTitle` |
| Body/button | `Sizing.fontBody` |
| Button surface | `PressableSurface`, `Sizing.radiusSm` |
| Button slot | `pctH(7)` |

Panel has no border. Prefer extending `Modal.qml` shell over bespoke chrome.
GameInfo uses same panel radius. QrCodeModal remains shell-based QR content.

## ContextMenu chrome

Panel uses `bgPanel` + `radiusMd`, no border. Rows use PressableSurface +
`radiusSm`. Panel vertical padding is independent from radius.

Row labels center the `Text` item itself (`Sizing.center(parent.width,
_textWidth)`), per the integer-pixel rule below — never
`anchors.horizontalCenter` + `AlignHCenter`. Panel width tracks content:
`panelWidth` clamps `_desiredPanelWidth` (the widest entry label plus padding)
between a degenerate-case floor (`_minPanelWidth: Sizing.pctW(12)`) and the
available width, so a menu with short labels narrows instead of always paying
for a fixed minimum panel width.

Four `Theme.scrim` bands frame `anchorRect` so anchor stays bright. Dimensions
clamp to nonnegative values. Full-parent MouseArea dismisses outside panel,
including anchor gap, while row MouseAreas win inside.

When the caller passes `anchorRadius` (the anchored tile/row's own corner
radius), four baked antialiased quarter-disc masks cut the bands' square hole
down to the anchor's actual rounded silhouette, closing the bright square
notches a plain rectangular hole leaves past a rounded tile's arcs. Each
mask's alpha is the exact complement of the tile's own corner coverage, baked
once by `tools/bake-icons` for every integer radius in the [Radius
ladder](#radius-ladder) (1..16) and served through
`Resources.cornerCutUrl()`. `anchorRadius: 0` (the default) keeps the plain
square hole byte-identical — callers that haven't confirmed their anchor is a
`PressableSurface` (e.g. `hub_favorites`' action tile) stay on this path.
Corners are skipped, not overlapped, when the anchor is narrower than two
radii on either axis.

## Tile aspect and grid blocks

Hub rows remain square. Systems and media grids use Sizing-declared common
resolution shapes with adaptive fallback for nonstandard desktop/TATE scenes.
PagedGrid floors uniform cell dimensions, then centers cells-plus-gutters block;
odd remainders may differ by one pixel only.

Default-theme grid gaps (`crt` keeps its own raw pixel values, unaffected):
`systemsGrid`/`gamesGrid` `columnGap` is `pctW(2)` and `gamesGrid` `rowGap` is
`pctH(3)` (matching `systemsGrid`'s own `rowGap`), both set in
`BrowseLayouts.qml`. `PagedGrid.qml`'s `cellSpacingX` fallback (used when no
layout profile supplies `columnGap`) mirrors the same `pctW(2)`.

## Consistency rules

- Rounded square chooses `radiusMd` or `radiusSm`; pill chooses half-height.
- Pressable non-browse control uses PressableSurface.
- Focus uses `Theme.accent`.
- Ordinary text chooses six-role ladder.
- Geometry routes through `Sizing.px()`, `center()`, `half()`, or `stroke()`.

## Integer-pixel drawing

Rules apply everywhere, not only CRT:

- static geometry lands on integer pixels
- stroke widths are integer pixels
- CRT text remains 8/16px
- user-visible centered text centers item, not a half-pixel glyph run

Document any exception beside code that needs it.
