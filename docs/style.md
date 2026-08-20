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

`Theme.qml` exposes the derived roles to components. Never branch on scheme ID in
a component or hardcode a preset color outside the catalog. `zaparoo-black` is
fallback for missing, unknown, or removed IDs.

Selection applies live and persists as `[settings] color_scheme` in
`frontend.toml` plus `state.toml`. Tinted image URLs naturally change with
palette roles; custom and full-color artwork remains unchanged.

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

### Browse-list selection latch

Browse lists remain one containing card with flat rows; never turn every row into
a separate raised button. Selected row uses `selectionSurface`, one opaque
`selectionShade` keyline along its **bottom** edge, and the accent rail.
Activation moves only accent rail, title inset, and optional favorite inset
inward by `Sizing.focusRingWidth`, then holds until release/settling. This reads
as a cursor engaging a recessed slot.

The keyline is on the bottom because a recess and a raised object shade
oppositely under the same light. The scene is lit low and from the front, so a
raised `PressableSurface` catches that light on its near face while the near wall
of a recess falls into shade. A top keyline inverts the recess and fights the
tiles beside it.

### Pressable front edge

Grid tiles, buttons, Settings rows, ContextMenu rows, and picker rows use
`PressableSurface.qml`.

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
| `logoShadow` / `logoSecondary` / `logoPrimary` | `surface(0.16, 0.22)` / `surface(0.45, 0.10)` / `surface(0.72, 0.04)` | Resting tinted-artwork ramp |
| `logoFocusShadow` / `logoFocusSecondary` / `logoFocusPrimary` | `mix(accent, ink, 0.35/0.62)` / `accent` / `mix(accent, text, 0.92)` | Focused tinted-artwork ramp |
| `errorHex` | `#ff4f91` dark / `#c2185b` light | Failure text |

`edgeBase` is `mix(primary, text, 0.06)` — one rung up the neutral ladder, so the
accent ramp keeps some body on a near-black primary. Two constants are
direction-dependent (`selectionShade`, `logoFocusShadow`) because equal channel
mixes are not equal perceived steps at opposite ends of the sRGB curve.

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

`Theme.fontUi` is Noto Sans normally and MxPlus HP 100LX 6x8 on CRT native
path. `Theme.fontMono` is diagnostic/log text only.

### Type ladder

Six ordinary text roles only:

| Token | Role | 240 | 480 | 540 | 720 | 1080 |
|---|---|---:|---:|---:|---:|---:|
| `Sizing.fontHero` | Page/selected title | 14 | 22 | 24 | 29 | 43 |
| `Sizing.fontTitle` | Modal/detail title | 12 | 18 | 19 | 23 | 35 |
| `Sizing.fontSection` | Section/list/status | 11 | 16 | 17 | 21 | 31 |
| `Sizing.fontBody` | Body/control/help | 10 | 14 | 15 | 19 | 28 |
| `Sizing.fontCaption` | Secondary/menu/tile fallback | 9 | 12 | 13 | 17 | 26 |
| `Sizing.fontSmall` | Tile/detail small print | 8 | 10 | 11 | 16 | 24 |

CRT tokens resolve through former percentage roles and retain mandatory 8/16px
bitmap quantization. `Sizing.fontSize(percent)` remains for approved specialist
sizes and geometry such as header row height; do not use it to invent seventh
ordinary text role.

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

Radius never doubles as padding. ContextMenu panel vertical padding is
`pctH(1.5)` independently of panel radius.

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
