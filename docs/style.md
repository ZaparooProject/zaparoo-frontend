# UI Style

Zaparoo Frontend's design language. `Theme.qml` owns the colour and
font tokens; `Sizing.qml` owns the percentage helpers and the corner
radius. Anything not covered here defers to those two singletons.

The whole app runs on Qt Quick's software adaptation — no shaders, no
shadows, no gradients, no `Shape`. Every surface in this guide is built
from `Rectangle` + `Text` + `Image`.

## Cards: the focusable surface recipe

A *card* is any selectable, pressable surface in the app. Cards are
the design's keystone — once a reader has the card recipe, every
focusable surface in the frontend reads the same way:

| Property | Value |
|---|---|
| Fill | `Theme.surfaceCard` |
| Static border | `1px`, `Theme.borderMid` |
| Focused border | `2px`, `Theme.accent` |
| Corner radius | `Sizing.cornerRadius` |

Surfaces that follow the recipe today: tile bodies (Hub categories,
Hub action row, Systems / Games / Favorites / Recents grids),
SettingsField rows, Modal accept/cancel pills, ContextMenu rows,
ListPickerModal rows, and the About / License body card.

The static `borderMid` edge gives every card a subtle depth cue
whether focused or not. The accent border on the focused card sits
*on top* of the static edge, so unfocused cards never look like
"focus minus colour" — they look like cards in a resting state.

When adding a new surface that responds to a press, use the recipe.
When adding a surface that doesn't respond to a press, do not put it
on a card — see "Plain text on background" below.

### Tile focus ring (special case)

Tile.qml's focus ring isn't drawn with `border.width` — it's two
stacked filled rounded rectangles (an outer accent pill and an inner
`surfaceCard` mask that punches the centre back). Equivalent shape,
significantly smoother corners under software rendering. Both
rectangles sit inside the card edge by `_outlineGap` so the ring
never bleeds past the cell. See the comment on `focusRingOuter` in
`Tile.qml` for the QTBUG reference.

## Plain text on background

Text that isn't on a card sits straight on `Theme.bgDeep`. This is
fine — the background is dark enough that white text reads cleanly
without a chrome panel — but it's reserved for **non-interactive**
elements:

- Page titles in `TopStatusStrip`
- Settings section headers (`SettingsSectionHeader.qml`)
- The global "Loading…" overlay
- The active-tile name caption under each grid (`ActiveLabel.qml`)

Rules:

- Use `Theme.textPrimary` for primary content, `Theme.textLabel` for
  secondary/metadata.
- Use a font size of **2.6 or larger** (Body and up — see "Fonts"
  below). Smaller text on plain background reads as fragile.
- Never put a pressable element on plain background. If it responds
  to a press, it goes on a card.

## Focus

Focus is **always `Theme.accent`** across every surface in the app —
tile rings, card borders, modal buttons, picker rows, context-menu
rows, settings rows. There is no second focus colour.

Configurable accent (so users can pick their own palette) is a planned
settings feature, not a per-surface override. If you find yourself
reaching for a focus colour that isn't `Theme.accent`, stop and ask.

## Pills

Toggle controls (`SettingsField.qml` track + thumb) use `height/2`
and `width/2` for radius. They're pills, not rounded squares — a
deliberately different shape for binary on/off. Same convention as
iOS toggles.

The toggle pill is borderless: fill alone carries state.

| State | Fill |
|---|---|
| On | `Theme.accent` |
| Off | `Theme.borderMid` |

The row's outer card carries the focus indicator; the pill itself
doesn't get a focus ring.

## Colors

Every colour in the UI must come from `Theme.qml`. Never inline a
hex literal.

| Token | Hex | Used for |
|---|---|---|
| `bgDeep` | `#0f0f23` | Page background — every screen body |
| `bgPanel` | `#1a1a35` | Modal panels, ContextMenu panel |
| `bgBar` | `#0a0a15` | Help bar at the bottom of the screen |
| `surfaceCard` | `#2a2a45` | Card fill (see "Cards" above) |
| `scrim` | `#cc000000` | Modal scrim — translucent black |
| `borderSubtle` | `#1a1a2e` | Reserved for low-contrast borders |
| `borderMid` | `#404060` | Static card edge, unfocused row borders |
| `textPrimary` | `#ffffff` | Primary text, focused captions |
| `textLabel` | `#888888` | Secondary text, metadata, idle status |
| `accent` | `#FFB347` | Focus indicator everywhere |

Three background tokens cover the layered hierarchy: page (`bgDeep`)
< card (`surfaceCard`) sits above page, panel (`bgPanel`) replaces
the page when a modal is up, bar (`bgBar`) is the help-strip footer.

## Fonts

`Theme.fontUi` is **Atkinson Hyperlegible** for every UI string.
`Theme.fontMono` is `monospace` and is reserved for diagnostic /
log views.

### Font-size ladder

Six sizes only. Each role earns its slot — don't introduce a seventh.
At 240p the small end floors to 8px (`Sizing.fontSize` clamps at 8
for legibility on CRT), so the visual distinction shows up only on
larger screens; the ladder is robust either way.

| Token | Role | Where it's used |
|---|---|---|
| `fontSize(4.0)` | Hero | Page title, About wordmark, ActiveLabel selected name |
| `fontSize(3.2)` | Title | Modal title, BrowseDetailPane title |
| `fontSize(2.9)` | Section | Settings section header, top-strip side metadata, list-view row |
| `fontSize(2.6)` | Body | Settings label/value, About body, modal body, modal button, picker row, help bar |
| `fontSize(2.4)` | Caption | Action status, tile fallback name, secondary About labels, ContextMenu row, ScreenStateOverlay secondary |
| `fontSize(2.2)` | Small | Tile bottom caption, CoreStatusPill, BrowseDetailPane small text, modal small print |

`Body` covers both "text on the screen" (settings labels, About
paragraphs) and "text on a control" (modal buttons, picker rows,
help bar). They were split across two sizes historically; the
split was too subtle on every target resolution to earn the
extra rung.

`renderType: Text.NativeRendering` is the project default for crisp
text under software rendering — set it on every `Text`. Some
components disable it specifically (rare); when in doubt, leave it
on.

## Padding scale

Padding tightens as you move inward. Three layers, with documented
percentages for each:

| Layer | Inset | Where |
|---|---|---|
| Grid edge | `Sizing.pctW(5)` left/right, `Sizing.pctH(2)` top/bottom | `PagedGrid.qml` cell rows |
| Modal panel | `Sizing.pctW(4)` sides, `Sizing.pctH(4)` top, `Sizing.pctH(3)` column spacing | `Modal.qml` content column |
| Inside a card | `Sizing.pctW(2)` left/right | `SettingsField.qml` row content |
| About card body | `Sizing.pctW(3)` sides, `Sizing.pctH(3)` top/bottom | `AboutScreen.qml` Flickable |

The closer to the content, the tighter the padding. The grid is the
loosest because tiles already provide their own visual chunking;
inside a single card, content sits closer to the edge because the
card itself is the visual container.

## Modal chrome

Every modal panel uses the same shell:

| Surface | Token |
|---|---|
| Background | `Theme.bgPanel` |
| Corner radius | `Sizing.cornerRadius` |
| Scrim | `Theme.scrim` |
| Column top margin | `Sizing.pctH(4)` |
| Column side margins | `Sizing.pctW(4)` |
| Column spacing | `Sizing.pctH(3)` |
| Title | `Sizing.fontSize(3.2)`, `Theme.textPrimary` |
| Body | `Sizing.fontSize(2.6)`, `Theme.textPrimary` |
| Button slot height | `Sizing.pctH(7)` |
| Button width | `Sizing.pctW(28)` |
| Button background | `Theme.surfaceCard` |
| Button border | `1px`, `Theme.borderMid` (focus: `2px`, `Theme.accent`) |
| Button radius | `Sizing.cornerRadius` |
| Button text | `Sizing.fontSize(2.6)`, `Theme.textPrimary` |

The panel itself has no border — `bgPanel` against the scrim-dimmed
screen behind already separates the panel cleanly, and adding a
static edge here would be louder than the focused button inside.

When adding a new modal, prefer extending `Modal.qml` (a new `kind`
or the shell content slot) over a bespoke panel. The first-run,
commercial-notice, and log-upload modals all wrap `Modal.qml` via
`kind: "shell"`.

### QrCodeModal

`QrCodeModal.qml` is currently a partial implementation: it paints a
full-screen `Theme.scrim` and centres the QR pixmap on it, with no
panel chrome around the code. Full chrome (panel + title + close
affordance) is a future round; the scrim alone is enough to dim
the screen behind so the QR reads cleanly.

## ContextMenu chrome

`ContextMenu.qml` joins the rounded-square family on the same rules
as `Modal.qml`: `bgPanel` panel fill, `Sizing.cornerRadius` corners,
no panel border. Rows follow the card recipe (`surfaceCard` fill,
1px `borderMid` unfocused, 2px `accent` focused).

The scrim is the one departure from `Modal.qml`. A context menu is
*about* the tile it's anchored to; a full-screen scrim would dim
that tile and defeat the affordance. Instead the menu paints four
`Theme.scrim` bands framing `anchorRect`:

| Band | Geometry |
|---|---|
| top | `(0, 0)` → `(width, anchorRect.y)` |
| bottom | `(0, anchorRect.y + anchorRect.height)` → `(width, height)` |
| left | `(0, anchorRect.y)` → `(anchorRect.x, anchorRect.y + anchorRect.height)` |
| right | `(anchorRect.x + anchorRect.width, anchorRect.y)` → `(width, anchorRect.y + anchorRect.height)` |

Every dimension is clamped with `Math.max(0, ...)` so an anchor
flush against an edge collapses the matching band rather than
overflowing. The anchored tile sits in the un-painted gap and stays
bright, doubling as "this menu is about *that* tile" feedback.
Total scrim-painted pixels go down vs. a full-screen scrim because
the anchor area isn't painted; software-renderer cost is four
opaque rects vs. one.

A click anywhere outside the panel — including on the punched-through
anchor area — fires `closeRequested()` (a single full-parent
`MouseArea` sits beneath the bands so the dismiss area is the union
of scrim + anchor, with the panel's interior `MouseArea`s on top
winning for clicks inside the panel).

## Corner radius

One token, one value: `Sizing.cornerRadius` (`pctH(3.5)`). Every
rounded-square surface in the app uses it.

| Surface | Code |
|---|---|
| Tile card body | `radius: Sizing.cornerRadius` |
| Tile focus ring | `radius: Sizing.cornerRadius - root._outlineGap` |
| SettingsField row | `radius: Sizing.cornerRadius` |
| Modal panel | `radius: Sizing.cornerRadius` |
| Modal button | `radius: Sizing.cornerRadius` |
| ContextMenu panel | `radius: Sizing.cornerRadius` |
| About body card | `radius: Sizing.cornerRadius` |

The Tile focus ring is computed from the token (not hardcoded to a
smaller value) so the ring stays concentric with the card if the
token ever changes.

When adding a new rounded-square surface, use the token. Don't
introduce a second radius value — the visual language is one shape,
one radius.

## Tile aspect

| Surface | Aspect |
|---|---|
| Hub categories row | 1:1 (square, `cellHeight = cellWidth`) |
| Hub action row | 1:1 (mirrors categories row metrics) |
| Systems grid | Aspect driven by `PagedGrid` available height |
| Games grid | Aspect driven by `PagedGrid` available height |
| Favorites grid | Aspect driven by `PagedGrid` available height |
| Recents grid | Aspect driven by `PagedGrid` available height |

The hub uses square tiles because the icons are simple silhouettes
that read fine at 1:1. Cover-art surfaces (systems, games,
favorites, recents) get taller cells from `PagedGrid` because
logos and box-art benefit from vertical room.

## True squircles aren't achievable

A super-ellipse curve needs `Shape` + `PathSvg` or shaders. The
MiSTer build runs Qt Quick's software adaptation — no GPU, no
shaders, no `Shape`, no `MultiEffect`. See `qml-gotchas.md`. The
large `Rectangle.radius` value is a circular-arc approximation;
close enough at this scale that the lack of super-ellipse curvature
is invisible at typical viewing distances.

## Consistency rule

If a new surface has rounded corners, it picks `Sizing.cornerRadius`
or it joins the pill family. There is no third option.

If a new surface is pressable, it follows the card recipe. There is
no second focus colour.

If a new piece of text is added, it picks a size from the six-rung
ladder. There is no seventh size.

Inconsistent radii, focus colours, and font sizes were the problems
these tokens were introduced to solve.

## Integer-pixel drawing

These rules apply to every screen, not just CRT-targeted code paths.
The whole app must render cleanly at 240p; the frontend has one
rendering path, not two.

- Geometry lands on integer pixels (`Sizing.px()`, `Sizing.center()`,
  `Sizing.half()`).
- Stroke widths are integer pixels (`Sizing.stroke()`).
- Text sizes are restricted to `8px` or `16px` when `crtNativePath` is
  active; `Sizing.fontSize()` handles the quantization.
- Bitmap-style text doesn't rely on centered glyph layout; center the
  text item, not the glyph run.

These are implementation constraints, not aesthetic preferences. If a
new surface needs an exception, document the reason in the same change.
