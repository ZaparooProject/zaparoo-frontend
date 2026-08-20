# UI Style

Zaparoo Frontend's design language. `Theme.qml` owns color and font tokens;
`Sizing.qml` owns integer geometry, resolution tiers, radii, type roles, and
stroke weights. Anything not covered here defers to those singletons.

UI runs on Qt Quick's software adaptation: no shaders, shadows, gradients, or
`Shape`. Build surfaces from `Rectangle`, `Item`, `Text`, and `Image`.

## Color schemes

A preset authors exactly **three** colors and `ColorSchemes.qml` derives every
other role from them, through a semantic tier (below) before it reaches
components:

| Authored | Role |
|---|---|
| `primary` | Page background, and the base every surface mixes from |
| `accent` | Focus, selection, and every highlight in the UI |
| `text` | Primary content color |

There is no fourth *authored* color — presets still author exactly three
hexes. The semantic tier below is derived, not authored, and exists because
state markers (the favorite heart, the Hidden badge) used to ride directly on
`accent` too, a convention that assumes a dark accent and breaks on a
mid-luma one.

Two roles do not derive. `scrim` is always a dark veil, because its job is to
separate a panel from what sits behind it whichever direction the preset runs.
`errorHex` is a semantic constant: deriving it from the accent would make an
amber preset signal danger in amber.

Derivation direction follows `up = luma(text) > luma(primary)`, so light presets
work without a separate ladder. `Qt.lighter()`/`Qt.darker()` are HSV value
multiplies and are no-ops on pure black — the catalog mixes channels explicitly
instead, and components must too.

`markerOutline` (below) does not mix toward `primary`/`text` directly — that
axis is what silently ran backwards on the `zaparoo-white` preset. It mixes
toward `lightPole`/`darkPole` instead, the same two colors ordered by luma
(`up ? text : primary` / `up ? primary : text`), so the outline is always the
pole opposite the marker's own luma regardless of which of `primary`/`text`
happens to be light on a given preset.

### Semantic tier

Standard token pipelines run primitive → semantic → component. Three semantic
roles sit between the three authored hexes and the ~20 component roles:

| Role | Job |
|---|---|
| `onAccent` | Body text, glyphs, and control fills sitting on a solid `accent` surface — the inverted `SelectionBar` fill, the favorite heart on a selected row, the toggle track on a selected row |
| `onAccentMuted` | Subordinate on-accent content — tag suffixes, the "off" toggle track on a selected row |
| `marker` | Fixed-hue state marker — the favorite heart and the Hidden badge — kept independent of `accent` so a favorited tile can't blend into the focus ring |

`onAccent` starts from whichever of the preset's own `primary`/`text` already
contrasts better against `accent`, then walks that color's OKLCh lightness
(holding its own chroma and hue) until it clears 4.5:1 — so it stays tied to
the preset's identity rather than snapping to flat black or white.
`onAccentMuted` is the same hue/chroma walked toward `accent` until contrast
lands in the 3.0–4.5 band. `marker` is a fixed warm-red OKLCh hue (~29°,
matching the iOS-red/heart convention), rotated in 90° steps away from
`accent`'s own hue when they would collide within 40°, with lightness fitted
per preset for ≥3:1 against `surfaceCard`. It is a semantic constant rather
than a derived hue for the same reason `errorHex` is: deriving it from the
accent would make an amber preset signal "favorited" in amber.

### OKLCh derivation ladder

The neutral surface ladder (`bgPanel`, `surfaceCard`, `borderSubtle`,
`borderMid`, `textVariant`, the pressable edges) stays on the old per-channel
sRGB `mix()` — it only mixes near-neutrals, where the error is small. Anything
that carries the accent's own hue — the two logo ramps, `onAccent`,
`onAccentMuted`, `marker` — is derived in OKLCh instead (Björn Ottosson's
perceptually uniform space), holding chroma and hue fixed while only
lightness moves. sRGB per-channel lerp does not preserve chroma: mixing a
saturated accent toward a near-black or near-white pole desaturates it along
the way, which is what made a focused amber tile read near-white and a
focused blue tile read closer to brown than orange before this ladder existed.
Gamut-fitting (`ColorSchemes._gamutFit`) reduces chroma via binary search at a
fixed lightness/hue when a requested OKLCh coordinate falls outside sRGB —
lightness and hue are never altered, only chroma, and only as far as needed.

**Guardrail.** `_clampAccent(accent, primary)` walks an authored (or, later,
user-supplied) accent's OKLCh lightness away from `primary` until it clears
4.5:1, preserving hue and chroma, and logs a warning. Inert for every shipped
preset — they already clear this floor — but is what makes the whole ladder
safe to seed from an arbitrary hex rather than only the curated ones, ahead of
any future custom-palette config surface.

`Theme.qml` exposes the derived roles to components. Never branch on scheme ID in
a component or hardcode a preset color outside the catalog. `zaparoo-black` is
fallback for missing, unknown, or removed IDs.

### Preset catalog

24 presets ship: the three original Zaparoo themes, plus 21 real, unmodified
hex triads pulled from popular editor/terminal palettes (Catppuccin, Nord,
Dracula, Gruvbox, Tokyo Night, Rosé Pine, Kanagawa, Ayu, Nightfox, Monokai,
One Dark Pro, Everforest Dark, Synthwave '84) and documented retro/console
references (amber/green CRT phosphor, Neo Geo, NES, Virtual Boy). None of
these are invented colors — every triad is `primary`/`accent`/`text` picked
from that theme's own published palette.

Two guardrail floors relax from their round-4 values to admit these without
altering a single hex, since the originals were tuned only against the three
shipped presets:

- `textPrimary`/`surfaceCard` contrast: AAA 7.0:1 → AA 4.5:1.
  `textPrimary`/`bgDeep` stays at 7.0 — the primary background is the
  highest-traffic surface and keeps the stricter floor; only the *card*
  surface (mixed partway toward text/accent) relaxes.
- The focus ramp's *primary*-rung chroma-retention floor: 45% of the
  accent's own OKLCh chroma → 33%. The *shadow*-rung floor stays 55% — sRGB's
  own gamut holds less chroma at high lightness regardless of color space,
  which is why the light end needed the lower floor and the dark end didn't.

A handful of well-known palettes — Solarized (both variants), Catppuccin
Latte, Everforest Light, Rosé Pine Moon/Dawn, Night Owl — still don't clear
the catalog even with those two relaxed. Each fails a *different*, deeper
guardrail (`_clampAccent`'s 4.5:1 floor, which would silently mutate their
authored accent away from the real hex; the `textLabel`/`bgDeep` 3:1 floor;
or `tileEdge` failing to read as more saturated than the card) that round 4
built as a real legibility guarantee, not a number tuned to three presets —
so they're left out rather than loosened further. Revisit only deliberately.

Adding a preset touches: `ColorSchemes.qml`'s `ids` and `_sources`;
`rust/frontend/src/models/settings.rs`'s `COLOR_SCHEMES` (same order); and
`SettingsScreen.qml`'s `_colorSchemeDisplay` lookup table (a literal
`qsTr()` call per id so `lupdate` can harvest it). `tst_color_schemes.qml`'s
guardrail tests already iterate `ColorSchemes.ids`, so a new preset gets the
full suite for free except the id-count and named-id assertions, which need
their literals bumped.

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

## Two registers

Everything in the frontend is drawn with one of two visual treatments, and
which one a component gets follows from what the component *is*, not from
where it happens to live on screen.

- **Objects get physical treatment.** Tiles, buttons, `ContextMenu` rows, and
  `ListPickerModal` rows are things you pick up and press — a raised plate on
  `PressableSurface`, a chromatic front edge, an accent focus ring, a
  press-down on activation. See "Pressable front edge" below.
- **Lines of text get typographic treatment.** `BrowseList` rows and
  `SettingsField` rows are read, not handled — no fill or border at rest, and
  selection is inverse video: the row's own `SelectionBar` swaps foreground
  and background rather than lifting off the page. See "Inverse-video rows"
  below.

Both idioms come from the same constraint: this is software rendering on
hardware that cannot composite. A raised button is what depth looks like when
all you have are filled rects and two flat colors on adjacent edges. Inverse
video is what selection looks like under that same limit — no tint, no shade,
so you swap the two colors you already have. Neither idiom is decoration for
its own sake; each is the cheapest correct answer to "how does this state
read" given no shaders, no gradients, and no alpha compositing over busy
content.

Resolution, not geometry, does the era-signalling. The row geometry and the
inverse-video mechanic are identical at 1080p and at 240p — what changes is
`Theme.bitmapType` (the 6x8 bitmap face, quantized to 8/16px strikes), which
auto-engages on the CRT path and at 240p. A solid accent bar with near-black
text at 1080p in Noto Sans reads as high-contrast modern chrome; the same bar
in the bitmap face at 240p reads as a DOS-terminal flash. The geometry never
branches; only the font and the pixel density shift the connotation.

That also sets the refusals that keep this from tipping into retro pastiche:

- No monospace for ordinary lists. `Theme.fontMono` stays diagnostic/log text
  only — see "Fonts" below.
- No scanlines, glow, or box-drawing chrome anywhere in the UI.
- No blinking block cursor. `SelectionBar`'s flash is a single one-shot cue on
  activation (see "Sanctioned one-shot transient cues" in
  `docs/qml-gotchas.md`), never a persisting blink — the no-persistent-motion
  rule in `CLAUDE.md` bans looping cues over content regardless of theme.

Integer-pixel precision (see "Integer-pixel drawing" below) is not only a
240p/MiSTer concern. At 4K, a glyph run that straddles a half-pixel or a card
edge that lands on a fractional coordinate is the difference between "this was
designed" and "this is nostalgic clip art" — softness reads as sloppiness at
high resolution even though nothing is functionally broken.

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
See "Toggle rows" below for the track/knob color rule. Handle insets preserve
integer centering.

### Tile focus ring

`Tile.qml` draws focus as two stacked filled rectangles rather than
`border.width`. Outer accent and inner `surfaceCard` mask avoid stepped rounded
borders under Qt software rendering (QTBUG-123210). Ring thickness is
`Sizing.focusRingWidth`; Tile and PagedGrid placeholder geometry stay
synchronized during press/rapid-render states.

`PressableSurface.qml` — modal buttons, ContextMenu/ListPickerModal rows —
draws the identical two-rect construction when `focused`, inset inside the
face rather than outset (the root `Item` clips). This is what gives a focused
modal button the same visual weight as a focused tile instead of the older
thin `border.color`/`border.width` swap, which had the same QTBUG-123210
stepping problem at any real thickness. The face's own `border` (`borderMid`,
`cardBorderWidth`) no longer changes with focus — the ring is additive, not a
replacement for the resting border.

Tiles use a physical front edge inside their existing cell footprint. Activation
lowers artwork, caption, badges, and ring together without scaling cover art.

### Inverse-video rows (browse lists and Settings rows)

Browse lists and Settings rows are both one containing card with flat rows;
never turn every row into a separate raised button. The selected row fills
solid with `Theme.accent` and most content on it — label, value text,
chevron, and (via the tag suffix's own `variantColor`) the dim disambiguating
tag — flips to `Theme.onAccent` (or `Theme.onAccentMuted` for the tag), the
semantic-tier role guaranteed ≥4.5:1 against `accent` (see "Semantic tier"
above). The favorite heart flips to `Theme.onAccent` too rather than
`Theme.marker` — `marker` is tuned for legibility against `surfaceCard`, not
against a solid `accent` fill. Nothing moves: no rail, no inset, no push-in. A
text row is not a button, so it doesn't get a button's depth cue.

Activation is a one-shot inverse blink: the bar and its content hard-swap
colors for `Motion.pressMs`, then swap back, rather than a crossfade. This is
deliberate — punchier, era-correct for the DOS-terminal-flash half of the
[Two registers](#two-registers) language, and two repaints total instead of
one per animated frame. Under Reduce Motion (`Motion.enabled: false`) the
blink resolves in a single frame and is effectively invisible, the same
convention every other one-shot cue in the app follows.

The highlight itself lives once in `SelectionBar.qml` so `BrowseList` and
`SettingsField` cannot drift apart — each row mounts one, binds its own
label/value/icon colors to `bar.active ? bar.contentColor : Theme.textPrimary`
(`bar.contentColor` is `Theme.onAccent`; see "Semantic tier" above), and
forwards its host's `activatePulse`/`releasePulse`/`screenSettling`.
`releasePulse` and `screenSettling` only cut a held blink short; they don't
hold the flash open past `Motion.pressMs`. Settings rows using
`control: "toggle"` are exempt from the flash: the knob slide is their own
activation cue. See "Toggle rows" below for how the track/knob colors work on
a selected row. `ContextMenu` and `ListPickerModal` rows stay on
`PressableSurface` below — a menu entry *is* a button and a press-in is
correct there; keeping both registers visible in the same app is what stops
the language collapsing into one idiom.

#### Toggle rows

One rule, both row registers: **the track alone carries on/off + row-register
state, at maximum contrast against the row's own current background; the
knob is always a constant neutral (`Theme.surfaceCard`)**:

| | On | Off | Knob |
|---|---|---|---|
| Unselected row | `Theme.accent` | `Theme.borderMid` | `Theme.surfaceCard` |
| Selected row | `Theme.onAccent` | `Theme.onAccentMuted` | `Theme.surfaceCard` |

Before this rule, the track and the knob branched on row-selection
independently of each other, so which element carried state flipped
depending on whether the row was selected — that inconsistency, not the
switch metaphor, was what read as broken. Round 4 fixed the track/knob
inconsistency but had the knob mirror the row's own current background (so
it read as "a hole punched through the track"); round 5 found that on a
selected row that background is solid `Theme.accent`, which made the knob
visually merge into the row itself on lower-chroma presets (Nord, Ayu,
Kanagawa). The knob is now always `Theme.surfaceCard`, in both registers —
giving up the literal "hole reveals the true background" metaphor for a knob
that's never ambiguous. Knob position and travel are unchanged and stay the
primary on/off cue in every state; the off/unselected knob-vs-track contrast
is deliberately low (`borderMid`/`surfaceCard` are both subtle near-card
neutrals), which is fine because position, not color, carries that state.

### Pressable front edge

Grid tiles, buttons, ContextMenu rows, and picker rows use `PressableSurface.qml`.
Its focus ring is the tile ring's construction reused — see "Tile focus ring"
above.

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
rounded squares and remain borderless; outer Settings row carries focus. See
"Toggle rows" above for the track/knob fill rule, which differs between an
unselected and a selected row.

## Colors

Every UI color comes from `Theme.qml`; never inline hex except diagnostic
calibration surfaces with documented reason. Roles have no fixed hex — they are
derived per preset from `primary`, `accent`, and `text`. `mix(a, b, t)` is a
per-channel sRGB lerp; `surface(t, bias)` is `mix(mix(primary, text, t), accent, bias)`,
the neutral ladder with a slight accent cast; `ink` is the pole away from `text`.
`oklch(L, C, h)` below means "hold this OKLCh lightness/chroma/hue, gamut-fit
into sRGB if needed" — see "OKLCh derivation ladder" above.

| Token | Derivation | Use |
|---|---|---|
| `bgDeep` | `primary` | Flat page background |
| `bgBar` | `mix(primary, ink, 0.35)` | Help bar |
| `bgPanel` | `surface(0.05, 0.04)` | Modal/ContextMenu panel |
| `surfaceCard` | `surface(0.08, 0.05)` | Card/control face |
| `borderSubtle` | `surface(0.14, 0.03)` | Low-contrast edges |
| `borderMid` | `surface(0.32, 0.05)` | Resting card edge |
| `tileEdge` | `mix(edgeBase, accent, 0.44)` | Grid tile front edge |
| `controlEdge` | `mix(edgeBase, accent, 0.54)` | Control front edge in panels |
| `scrim` | `#cc000000` | Modal scrim, always dark |
| `textPrimary` | `text` | Primary text |
| `textLabel` | `mix(primary, text, 0.62)` | Secondary text |
| `textVariant` | `surface(0.58, 0.14)` | Tinted secondary text |
| `accent` | `accent`, clamped ≥4.5:1 against `primary` | Focus, selection, and the inverted `SelectionBar` fill |
| `onAccent` | Better-contrasting of `primary`/`text`, OKLCh L walked to clear 4.5:1 vs. `accent` | Body text/glyphs on a solid `accent` surface |
| `onAccentMuted` | `onAccent`'s OKLCh hue/chroma, L walked toward `accent` into the 3.0–4.5 band | Subordinate on-accent content (tag suffixes, off toggle track) |
| `logoShadow` / `logoSecondary` / `logoPrimary` | `oklch(max(0.26, aL−0.28), 0, ah)` / `oklch(max(0.10, aL−0.16), 0, ah)` / `oklch(min(0.74, aL), 0, ah)` | Resting tinted-artwork ramp |
| `logoFocusShadow` / `logoFocusSecondary` / `logoFocusPrimary` | `oklch(max(0.32, aL−0.22), aC, ah)` / `accent` / `oklch(min(0.90, aL+0.16), aC×0.72, ah)` | Focused tinted-artwork ramp |
| `marker` | Fixed ~29° OKLCh hue (rotated ±90° away from `accent`'s hue if within 40°), L fitted for ≥3:1 vs. `surfaceCard` | Favorite heart, Hidden badge |
| `markerOutline` | `mix(luma(marker) > 0.5 ? darkPole : lightPole, marker, 0.12)` | Favorite-heart / marker keyline |
| `errorHex` | `#ff4f91` dark / `#c2185b` light | Failure text |

`aL`/`aC`/`ah` are `accent`'s own OKLCh lightness/chroma/hue. `edgeBase` is
`mix(primary, text, 0.06)` — one rung up the neutral ladder, so the accent
ramp keeps some body on a near-black primary; `tileEdge` and `controlEdge` are
the only roles built on it now that selection no longer has a surface color
of its own — a selected row is `accent`/`onAccent` inverse video
(`SelectionBar.qml`), not a mixed tint. The resting logo ramp reuses the focus
ramp's three lightness rungs desaturated to grey (`C = 0`) and pulled dimmer
on every rung, so an unfocused tile reads as clearly dimmer, not just
ring-less — collapsing that gap is what makes focus read as "ring only." Both
ramps hold `accent`'s own OKLCh hue (and, on the focus ramp, most of its
chroma) rather than mixing toward `primary`/`text` in sRGB, which is what
fixed a focused amber tile reading near-white and a focused blue tile reading
closer to brown than orange.

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
| Grid edge | `pctW(3)` sides, `pctH(2)` vertical |
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

Panel width is content-driven for the four prebaked kinds
(`action_error`/`transient`/`confirm`/the toggle), mirroring [ContextMenu
chrome](#contextmenu-chrome)'s pattern: the max of the measured title, the
body (capped at a 45-character line so a long paragraph wraps instead of
forcing a wide panel), and the summed button label widths, clamped between a
degenerate-case floor (`_minPanelWidth`) and `panelMaxWidth` (which itself
clamps against 92% of the viewport). `kind: "shell"` content is an opaque
caller-supplied `Item` Modal can't measure, so it keeps the old
percentage-of-viewport sizing (`min(78% of viewport, panelMaxWidth)`); a shell
consumer with narrow, measurable content — `ListPickerModal` measures its own
entry labels the same way ContextMenu does — overrides `panelMaxWidth` itself
rather than Modal trying to reach into arbitrary content.

### Picker swatch preview

`ListPickerModal` rows are plain centered labels by default. One picker — the
color-scheme picker — needs to show what a scheme actually looks like, not
just its name: `SettingsScreen._openPickerForField`'s `colorScheme` branch
attaches a `swatch: ColorSchemes.previewColors(id)` (the preset's three
authored colors, in primary/accent/text order) to each entry. `ListPickerModal`
checks only the first entry (`entries[0].swatch !== undefined`) to flag the
whole modal into swatch layout — a picker is assumed homogeneous, never a mix
of swatch and non-swatch rows.

In swatch layout the row's label moves from centered to left-aligned
(`x: rowHorizontalPadding`, still `Sizing.center()`-positioned vertically,
`Text.ElideRight`) and three small `Rectangle` swatches right-align in the
remaining width. Every other picker call site (view/sort menus, launcher,
resolution, language, ...) never sets `swatch`, so it falls through to the
original centered-label row untouched. `_desiredPanelWidth` grows to include
the swatch band + a label gap only when swatches are present, so the panel
still sizes to content per the content-driven-width pattern above.
`ColorSchemes.previewColors()` is independent of the active
`Theme.colorSchemeId` (it's a pure function of the requested id), and is
computed once per entry when the picker opens rather than per-delegate.

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
