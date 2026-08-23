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
axis is what silently ran backwards on the `zaparoo-light` preset. It mixes
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
a component or hardcode a preset color outside the catalog. `zaparoo-dark` is
fallback for missing, unknown, or removed IDs.

### Preset catalog

19 presets ship. **Zaparoo Dark**, **Zaparoo Light**, and **Classic Purple**
are the three original Zaparoo themes (renamed from `zaparoo-black` /
`zaparoo-white` / `midnight-amber` in round 6 so the id describes the
preset rather than an implementation detail; Classic Purple's triad was
further retuned in round 7 — see below). **Nord**, **Dracula**, **Synthwave
'84**, **Gruvbox**, **Everforest**, **Solarized Dark**, **Rosé Pine**, and
**Oxocarbon** cover the editor/terminal world. **Amber Phosphor**, **Green
Phosphor**, **Neo Geo**, **NES**, **Virtual Boy**, and **Game Boy** are
documented retro/console references. **Flexoki Paper** and **Solarized
Light** join Zaparoo Light as the light-register options. None of these are
invented colors — every triad is `primary`/`accent`/`text` picked from that
theme's own published palette, with two exceptions where a real value was
deepened to clear a guardrail: Game Boy's background sits below the real
DMG value so text/bg clears 7.0:1, and its text uses the real DMG palette's
lightest tone (`#9bbc0f`) rather than its second-lightest, which additionally
lets it clear `textPrimary`/`borderMid`'s 4.0:1 floor; Everforest's
background sits a few steps below its own published `bg_dim` "hard" tone
(`#1e2326`) for the same `textPrimary`/`borderMid` reason — `#1e2326` itself
falls just short at 3.98:1.

**Round 5 shipped 24 presets; round 6 pruned to 11; round 7 grew back to
19.** Round 6 kept Zaparoo, phosphor, and console presets unconditionally as
the identity and differentiated end of the catalog, and cut everything else
that read as a near-duplicate of one of them: `catppuccin-mocha`/`-macchiato`/`-frappe`
(next to Dracula — same dark-purple-on-slate register), `tokyo-night`,
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
distinct — Gruvbox's neutral warm-grey background and Everforest's
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
light page the accent must sit dark to clear `_clampAccent`'s 4.5:1 floor,
so its selected row lands on `#af3a03` — cream text at 5.4:1, technically
passing but reading as a heavy brown next to near-black body text. That
tension is structural to light presets with warm accents, not a tuning miss
that a different hex would fix, and round 7 didn't find a fix for it either
— Gruvbox Light still doesn't ship. Zaparoo Light's cool blue (`#0a63c9`,
5.19:1) carried the light-preset slot alone through round 6.

Two guardrail floors were relaxed in round 5, kept relaxed since:

- `textPrimary`/`surfaceCard` contrast: AAA 7.0:1 → AA 4.5:1.
  `textPrimary`/`bgDeep` stays at 7.0 — the primary background is the
  highest-traffic surface and keeps the stricter floor; only the *card*
  surface (mixed partway toward text/accent) relaxes. This is a legitimate
  AA guarantee in its own right, not a number tuned to a handful of
  presets.
- The focus ramp's *primary*-rung chroma-retention floor: 45% of the
  accent's own OKLCh chroma → 33%. The *shadow*-rung floor stays 55% — sRGB's
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
fails `textPrimary`/`bgDeep`'s 7.0:1 floor. Substituting Solarized's own
higher-contrast tone — `base2`/`base02` on the dark variant, `base01` on the
light variant, both still colors Solarized itself publishes, not invented
ones — clears every guardrail with room to spare (12.3:1 and 12.1:1
respectively). The same substitution (a theme's own darker ink in place of
its default body text) is what unlocked Flexoki Paper. Catppuccin Latte,
Everforest Light, Rosé Pine Moon/Dawn, and Night Owl were not revisited in
round 7 and remain out — each fails a *different*, deeper guardrail
(`_clampAccent`'s 4.5:1 floor, which would silently mutate the authored
accent away from its real hex; the `textLabel`/`bgDeep` 3:1 floor; or
`tileEdge` failing to read as more saturated than the card) that would need
its own justified substitution to fix, not just a swapped text tone.
Revisit only deliberately.

Adding or removing a preset touches: `ColorSchemes.qml`'s `ids` and
`_sources`; `rust/frontend/src/models/settings.rs`'s `COLOR_SCHEMES` (same
order) and its default-fallback test; `SettingsScreen.qml`'s
`_colorSchemeDisplay` lookup table (a literal `qsTr()` call per id so
`lupdate` can harvest it); and, for a rename specifically, every
`rust/zaparoo-core/src/{config,persist}.rs` test fixture that hardcodes the
old id as an example value (not validated against the catalog, but kept in
sync for clarity). `tst_color_schemes.qml`'s guardrail tests already iterate
`ColorSchemes.ids`, so a catalog change gets the full suite for free except
the id-count and named-id/hardcoded-hex assertions, which need their
literals updated.

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

- **Grids and commitments get physical treatment.** Tiles (`Tile`,
  `PagedGrid` placeholder cards), `LetterJumpModal`'s A–Z keypad cells, and
  standalone commitment buttons (`Modal`'s OK / Cancel / No / Yes) are things
  you pick up and press — a raised plate on `PressableSurface`, a chromatic
  front edge, an accent focus ring, a press-down on activation. See
  "Pressable front edge" below.
- **Vertical option lists get typographic treatment.** `BrowseList` rows,
  `SettingsField` rows, `ContextMenu` rows, and `ListPickerModal` rows are all
  the same interaction — scan a list, pick one — and are read, not handled: no
  fill or border at rest, and selection is inverse video, the row's own
  `SelectionBar` swapping foreground and background rather than lifting off
  the page. See "Inverse-video rows" below.

An earlier version of this split ran the other way: `ContextMenu` and
`ListPickerModal` rows were raised buttons on the reasoning that "a menu entry
*is* a button." That put a `SettingsField` row and the `ListPickerModal` row it
opens — literally the same choice, continued — in opposite registers, and cost
real legibility doing it. At 240p, a `ContextMenu` row (`Sizing.pctH(6)` = 14px)
has 2px of `pressEdgeHeight` plus a 3px border/ring band on each side, leaving
6px of clear face for an 8px bitmap glyph — the label painted *on* the focus
ring rather than inside it, on the app's smallest interactive text. A solid
accent-filled row is also a stronger low-resolution focus cue than a 1px ring
plus a 2px edge. Reclassifying them lost nothing from the app's physical
identity: tiles carry that identity (on screen essentially always, taking the
press-in on every launch), not menu rows next to them — and flat rows next to
a bright anchored tile make the tile read more like the object it is, not
less.

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
toggle tracks/handles, and rapid-scroll chrome use `radiusSm`.
See "Toggle rows" below for the track/knob color rule. Handle insets preserve
integer centering.

### Tile focus ring

`Tile.qml` draws focus as two stacked filled rectangles rather than
`border.width`. Outer accent and inner `surfaceCard` mask avoid stepped rounded
borders under Qt software rendering (QTBUG-123210). Ring thickness is
`Sizing.focusRingWidth`; Tile and PagedGrid placeholder geometry stay
synchronized during press/rapid-render states.

`PressableSurface.qml` — `Modal`'s confirm buttons and `LetterJumpModal`'s
letter cells — draws the identical two-rect construction when `focused`,
inset inside the face rather than outset (the root `Item` clips). This is
what gives a focused modal button the same visual *construction* as a
focused tile instead of the older thin `border.color`/`border.width` swap,
which had the same QTBUG-123210 stepping problem at any real thickness. The
face's own `border` (`borderMid`, `cardBorderWidth`) no longer changes with
focus — the ring is additive, not a replacement for the resting border.

Ring *thickness* is not shared with `Tile`, though — `PressableSurface`
derives its own `_ringGap`/`_ringWidth` from `Sizing.cardBorderWidth`, the
same token that already draws the row's own resting border. Two earlier
versions got this wrong in opposite directions: a screen-relative percentage
(matching `Tile`'s own tokens) ate ~38% of a short pressable row's face
height on top of its existing static border and press edge — three
concentric frames reading as clutter instead of one clear "this is focused"
cue. (This history predates `ContextMenu`/`ListPickerModal` moving off
`PressableSurface` entirely — see "Two registers" above — but the fix still
governs every remaining `PressableSurface` caller.) Rescaling that same
percentage to the row's OWN height instead floored to exactly 1px at every
real resolution tier — no heavier than the row's resting border, so focus
read as barely more prominent than idle chrome. Deriving from
`cardBorderWidth` fixes both by construction: `_ringGap: cardBorderWidth`,
`_ringWidth: cardBorderWidth * 2` — the band is always exactly double the
resting border's weight, and both scale together off the same token, so they
can't drift out of relative proportion at some resolution neither was tested
at. The resting border itself stays untouched (still additive, per above),
and the ring construction (two stacked filled rects, `Theme.accent`) is
unchanged — this is a thickness-derivation fix only.

Thickness alone was still not the whole fix. `Tile`'s own focus cue is not
ring-only: the caption dims to `Theme.textLabel` at rest and brightens to
`Theme.textPrimary` when focused, and bundled artwork/logos swap
`logoPrimary` → `logoFocusPrimary` the same way. `PressableSurface` callers
(`Modal`'s confirm buttons, `LetterJumpModal`'s letter cells) never picked up
that half — their label text sat at `Theme.textPrimary` unconditionally, so
every unfocused row already looked fully lit and the ring was the only signal
carrying focus at all. Each of those now dims/brightens its own label the
same way `Tile` does, so two independent signals reinforce each other. Rows
with only one possible focus target (`focused: true`, no other row to
contrast against — `LogUploadModal`, `CommercialNoticeModal`) are left alone;
there's nothing for dimming to distinguish there.

Tiles use a physical front edge inside their existing cell footprint. Activation
lowers artwork, caption, and ring together without scaling cover art.

### Hidden and disabled tiles — muted material, worded reason, never a badge

A tile that isn't in its normal, fully-live state — a user-hidden game or
system, or a Hub tile whose live precondition isn't currently met (Resume
with no history, Update with no internet, a category Core hasn't confirmed)
— never disappears and never dims via opacity. Two cues, neither a floating
badge:

- **Muted front edge** — the glanceable cue, visible on every affected tile
  at once, focused or not. `Tile.qml`'s `edgeColor` swaps from `Theme.tileEdge`
  to `Theme.borderMid` (the same neutral "resting card edge" role
  `PressableSurface`'s own static border already uses) whenever
  `delegateHidden || delegateDisabled`. No new palette engineering — reusing
  an existing neutral role is the point.
- **Worded reason** — the detail cue, surfaced through text the screen
  already shows rather than an overlay drawn on top of the tile. Captioned
  tiles (`showCaption: true` — Games/Favorites/Recents) fold it into the
  same dim suffix slot disambiguating tags already use (`ScrollingCaption`'s
  `tags`, e.g. "Cave Story · Hidden"). Non-captioned tiles (Hub, Systems) have
  no per-tile text to fold into, so it surfaces through the screen's
  `ActiveLabel` (also a `tags` suffix) while that tile is focused instead.

This replaces an earlier "Hidden" corner-pill component (`TileBadge`, since
removed) and a considered opacity-based "disabled" treatment. Both were
rejected for the same reason: a raw alpha multiplier has no contrast floor.
Measured against this app's own derived `zaparoo-dark` palette, icon+label at
40% opacity landed at 2.1:1 resting / 3.6:1 focused contrast against the tile
face — both fail WCAG AA text contrast (4.5:1), and the resting case fails
even the 3:1 floor for non-text UI components. Every other semantic color in
this app (`accent`, `onAccent`, `marker`) is explicitly OKLCh-walked to
guarantee a minimum ratio; an opacity trick was the one state cue that
wasn't. Icon/label color for a hidden or disabled tile stays locked to the
tile's normal resting tier permanently (focused or not) instead — the exact
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
solid with `Theme.accent` and most content on it — label, value text,
chevron, and (via the tag suffix's own `variantColor`) the dim disambiguating
tag — flips to `Theme.onAccent` (or `Theme.onAccentMuted` for the tag), the
semantic-tier role guaranteed ≥4.5:1 against `accent` (see "Semantic tier"
above). The favorite heart flips to `Theme.onAccent` too rather than
`Theme.marker` — `marker` is tuned for legibility against `surfaceCard`, not
against a solid `accent` fill. Nothing moves: no rail, no inset, no push-in. A
text row is not a button, so it doesn't get a button's depth cue.

`ContextMenu` and `ListPickerModal` rows follow the identical recipe against
their own `bgPanel` host instead of a `surfaceCard`: the same `SelectionBar`,
the same accent-fill-plus-`onAccent` swap, no separate row card nested inside
the panel. The color-scheme picker's swatch-preview border is the one row
element with a per-register color: it sits at `Theme.textLabel` at rest (a
mid neutral guaranteed ≥3:1 against `bgDeep` on every preset, so it separates
a near-black or near-white swatch from the row) and flips to
`bar.contentColor` on the selected row, the same fix the favorite heart uses
against a solid `accent` fill.

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
a selected row. `ContextMenu` and `ListPickerModal` bind only `activatePulse`
— they close on accept rather than settling back into an idle list, so there
is no held-flash case to cut short and no `screenSettling` transition to
forward.

The physical register still stays visible in the same app — tiles carry it on
every launch, `Modal`'s confirm buttons and `LetterJumpModal`'s keypad carry
it inside modals — so this isn't the language collapsing into one idiom, just
narrowing physical treatment to things that are genuinely objects rather than
list choices. See "Two registers" above.

#### Selected-row text weight

Dark-on-light (inverted) text suffers irradiation — it reads as thinner
than light-on-dark text at the identical weight, a real optical effect,
not a rendering bug. Resting rows are `Theme.textPrimary` on
`Theme.surfaceCard`; a selected row is `Theme.onAccent` on solid
`Theme.accent` — the inverted case. Round 8 added a weight step to
correct it: `SelectionBar.contentWeight` resolves to `Font.Medium` when
`active`, `Font.Normal` otherwise, and every consumer — `SettingsField`,
`BrowseList` (via `ScrollingCaption.fontWeight`), `ContextMenu`,
`ListPickerModal` — binds its text to it, the same shared-vocabulary
guarantee `contentColor` already provides. The goal is parity with the
resting row, not emphasis — `Font.Medium` is deliberately one notch, not
a jump to bold. `resources/fonts/NotoSans.ttf` is a variable font (has an
`fvar` table), so this resolves to a real cut off the weight axis rather
than a synthesised one.

No-op under `Theme.bitmapType`: the CRT/240p 6x8 face has a single
strike, and unantialiased 1-bit text has no irradiation to correct in the
first place — the same reasoning `Theme.fontUi`'s bitmap branch already
gets a pass on for size (see "Type ladder" below).

Panel-width measurement (`ContextMenu._desiredPanelWidth`,
`ListPickerModal._desiredPanelWidth`) must size against the *selected*
weight, not the resting one, or the widest label can elide the moment it
becomes selected — both files keep a dedicated `FontMetrics` fixed at
`Font.Medium` for exactly this, separate from each row's own live-weight
`FontMetrics` used for that row's individual label-centering box.

#### Action rows are the one row kind that centers its label

`control: "action"` rows (`updateMediaDb`, `runScraper`, `uploadLog`) used
to signal "this row runs something" purely by tinting the label
`Theme.accent` instead of `Theme.textPrimary` — the only cue, and one that
disappeared entirely on a selected row (the ternary collapsed to
`bar.contentColor` regardless of `control`, identical to every other
selected row). Round 8 kept the tint but added a stronger, selection-proof
signal: the label centers, following Apple's own convention for an
accessory-less "runs an action now" row (Sign Out, Erase All Content and
Settings) rather than inventing a new glyph with no real precedent. It is
deliberately the *one* row kind that centers — every other row stays
left-aligned per "Two registers" above, and an embedded button (WinUI's
`SettingsCard.ActionButton`, libadwaita's `AdwButtonRow`) was considered
and rejected for the same reason `ContextMenu`/`ListPickerModal` moved off
`PressableSurface`: it would reintroduce a button-shaped object into a row
register this file deliberately keeps flat and borderless.

Centered via item position (`x: Sizing.center(parent.width, labelText.width)`),
never `anchors.horizontalCenter` + `AlignHCenter` — see "Integer-pixel
drawing" below. The row's live status readout ("100,000 indexed", "In
progress", "Paused") moved from right-aligned-next-to-the-label to a
second centered line below it, the same reserved-height mechanism the
Settings hint band above uses, keyed off `actionStatus` instead of
`description` (the two are mutually exclusive in practice — action rows
carry no description, described rows carry no `actionStatus`).

#### Toggle rows

One rule, both row registers: **the track alone carries on/off + row-register
state, at maximum contrast against the row's own current background; the
knob fill always matches that same background (a hole punched through the
track), with a border in the track's own "on" color for that register**:

| | On track | Off track | Knob fill | Knob border |
|---|---|---|---|---|
| Unselected row | `Theme.accent` | `Theme.borderMid` | `Theme.surfaceCard` | `Theme.accent` |
| Selected row | `Theme.onAccent` | `Theme.onAccentMuted` | `Theme.accent` | `Theme.onAccent` |

Before this rule, the track and the knob branched on row-selection
independently of each other, so which element carried state flipped
depending on whether the row was selected — that inconsistency, not the
switch metaphor, was what read as broken. Round 4 fixed the track/knob
inconsistency by having the knob fill mirror the row's own current
background — but on a selected row that background is solid `Theme.accent`,
the same color the knob fill uses, so the knob visually merged into the row
itself on lower-chroma presets (Nord, Ayu, Kanagawa). Round 5 kept the
fill rule (it is still correct by construction — the knob fill is never
anything but the row's own two possible backgrounds) and added the border:
the track's own "on" color for that register, which the semantic-tier
guardrail tests already guarantee clears the fill by a wide margin
(`test_on_accent_clears_body_text_contrast` for the selected case;
`test_accent_against_bg_deep_clears_body_text_contrast` as a proxy for the
unselected case) — so the knob keeps a visible silhouette on every preset
with no new color derivation needed. Knob position and travel stay the
primary on/off cue in every state; the off/unselected knob-vs-track contrast
is deliberately low (`borderMid`/`surfaceCard` are both subtle near-card
neutrals) — position, not fill color, carries that state, and the border is
what keeps the knob's own silhouette legible regardless.

### Settings section headers

`SettingsSectionHeader.qml` splits the Settings form into bands (e.g.
"Analog video"). Round 5 shipped it as a bigger, bolder label
(`Sizing.fontSection`, `Font.DemiBold`, `Theme.textPrimary`) — a treatment
that silently stops working in bitmap mode: `Sizing.fontSize()` quantizes
`fontSection` and `fontBody` to the same 8/16px there, and `Theme.fontUi`'s
bitmap face ("MxPlus HP 100LX 6x8") has a single weight, so `Font.DemiBold`
is a no-op. At `--crt` or embedded 240p the header was pixel-identical to a
field label. Size and weight simply aren't available as signals at that
tier, so round 6 made the break structural instead: a full-card-width
`Rectangle` filled `Theme.borderMid`, with the label unchanged and inset by
`contentInset + Sizing.pctW(2)` (the screen drops its usual card-padding
margin on this one row so the band can reach the card's own edges — see
`SettingsScreen.qml`'s mount comment). `borderMid` measures 1.6-2.3:1 off
`surfaceCard` across the catalog (an unmistakable block on every preset) and
`textPrimary` reads 4.1-7.7:1 on it — both a rectangle and a color step, so
the break renders identically at 1080p and at 240p. The band stays off
`Theme.accent`, which `SettingsField.qml` reserves for "this row is an
action" (see "Toggle rows" above and the action-row label rule below it) —
reusing it for a header would blur that meaning.

#### Settings hint band

A per-row description line under the label (round 7) did not survive
contact with 540p and CRT: `SettingsField.qml` grew a described row from
`pctH(8)` to `pctH(11.2)` and painted the description at
`Sizing.fontCaption`, but `fontCaption`/`fontBody` both quantize to the
same 8px bitmap strike under `Theme.bitmapType`, so at 240p/CRT there was
no size hierarchy left and the band clipped — and on a selected row both
lines collapsed to the identical `bar.contentColor`, losing the colour
hierarchy too.

Round 8 replaced it with one shared band pinned to the bottom of the
settings card, showing the *focused* row's description instead of every
described row's own line. The card frame became static (an `Item` with
fixed geometry) instead of scrolling with the row `Column` — the
`Flickable` now occupies only the region above a hairline divider, with
the hint `Text` below it. Two lines reserved unconditionally (empty when
the focused row has no description) so the row column never reflows under
the cursor; `Sizing.fontBody`/`Theme.textLabel`, not `fontCaption` — the
same reasoning as above, colour is the only hierarchy signal that
survives the bitmap tier, so lean on it alone rather than a size step
that collapses to nothing there.

A right-hand detail pane (the more common modern pattern — Kodi, Android
TV, Switch) was considered and rejected: the card is already capped at
`pctW(70)`, and splitting that horizontally at the 240p/CRT tier would
leave row labels roughly half their current width. A bottom band costs
one vertical slot, identical at every tier, and doubles as the fix for a
separate bug: because the card frame is no longer scrollable content
itself, its top/bottom edges can no longer be scrolled out of view the
way they could when the whole card lived inside the `Flickable`.

### Pressable front edge

Grid tiles, modal buttons, and `LetterJumpModal`'s letter cells use
`PressableSurface.qml`. Its focus ring is the tile ring's construction
reused — see "Tile focus ring" above. `ContextMenu` and `ListPickerModal` rows
do not — see "Two registers" above for why they moved to `SelectionBar`.

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
- header status line (see "Header status line" below)

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

## Header status line

The header's second row shows Core connection problems and background-task
progress (indexing / optimizing / scraping) as plain text plus a segmented
progress track — never a pill. It replaced `CoreStatusPill`'s stadium-shaped
card: a bordered, half-height-radius surface is the toggle-track family's
shape (see "Pills" above), not a status readout's, and squeezing a label,
counts, and a spinner into one intrinsically-sized chip is what forced
abbreviated CRT-only wording ("Idx…", "Scr…").

`StatusLine.qml`'s content is a right-aligned pair, not a full-width
stretch: a shrink-wrapping primary-text label (`elide: Text.ElideRight`,
width capped to its own measured content so it hugs the track instead of
leaving a gap), then a fixed-width `ProgressTrack` as the rightmost element,
flush against the header's own right margin. A short message just sits
closer to the right edge — idle space moves to the left of the pair,
between it and the logo, rather than opening a gap in the middle of the
message the way a full-width stretch would. No card fill, no border, no
radius — this is the same "plain text on background" treatment
TopStatusStrip and the global Loading cue already use (above). The row is
reserved by the header's own fixed height (`Sizing.headerHeight`) whether or
not the line has anything to show, matching every other fixed-slot
discipline in this file; the line itself still collapses to zero height
when idle.

There is deliberately no trailing count next to the track. Two things were
tried and both cut: a step ratio ("3 / 10", redundant with what the track
already shows visually) and, after that, an absolute running total
("18 files" / "1250 scraped") — a real number the track can't express, but
still not worth the layout cost it added. A fixed worst-case reservation
for it ("999999 scraped") left a visibly blank void next to the track
whenever the actual count was short or absent (every optimize/vacuum
phase); sizing it to live content instead fixed the void but reintroduced
the exact kind of shifting anchor point the fixed-slot rules elsewhere in
this file exist to prevent. The bar alone conveys progress, the same as the
mobile app.

Label and detail join with a plain colon (`"Indexing: %1"`), not a
mid-dot — the bitmap CRT font renders `·` as a genuine pixel glyph (it's
not a missing-glyph problem), but at 6×8 a single centered dot is one or
two lit pixels, easy to miss at a glance, and not a character anyone types
by hand. This is deliberately a different join than the em dash used
elsewhere on the same line (`"Core error — %1"`, `"… paused — game
running"`, `"Scrape failed — %1"`): the em dash marks a *reason* clause
(why something stopped), the colon marks a *live detail* of an ongoing
action. Don't conflate the two when adding a new state.

One slot, resolved in priority order: a Core connection problem; an active
background task (including why it's paused — "game running"); a terminal
message held ~6 s after a task ends (a scrape failure lands here too, not as
its own tier, since Core only reports `state: "failed"` on that same
terminal frame); a transient event (card scan, playtime warning, inbox
message) dropped rather than queued while a higher tier owns the line; or
nothing.

### Progress track

`ProgressTrack.qml` is a row of discrete cells, not a continuous bar.
Determinate progress (indexing/scraping systems) fills cells up to the
fraction complete; indeterminate progress (optimize/vacuum, or before Core
has reported a total) marches one lit cell along the track instead. Either
way, exactly one cell — the fill's leading edge, or the marching cell —
blinks on and off at `Motion.pulseMs` (~2 Hz), pausing along with the task
it represents.

**Blinks, does not fade.** A `Timer` flips a bool every `pulseMs`; the
cell's `color` reads it straight through with no `ColorAnimation` and no
`Behavior on color` in between — every tick is a hard cut between
`Theme.accent` and its dim color, never an interpolated crossfade. A
breathing/throbbing pulse was tried and rejected: the spec is a blink, on
then off, the same instant-swap register `SelectionBar`'s inverse-video
flash already uses (a `PropertyAction`, not an animated transition) — this
component just repeats that swap on a timer instead of firing it once.

Discrete cells are a structural choice, not a stylistic one: a continuous
fill's right edge lands on a fractional pixel as the fraction changes,
which softens under any 240p rendering — the same class of bug "Integer-
pixel drawing" below exists to rule out everywhere else. Cell width and gap
are chosen as integers first, so the track's total width is always a
whole-pixel sum regardless of fill fraction, mirroring how `PageIndicator`
reserves its width unconditionally in the footer.

The blink is the one exception to the no-persistent-motion rule (see
`docs/qml-gotchas.md` → "Sanctioned one-shot transient cues"): it is header
chrome, never painted over content; its dirty rect is a single small cell;
it only runs while a task is genuinely active and not paused; and it stops
outright under Reduce Motion rather than collapsing to a 0 ms loop. This is
the same small-dirty-rect exemption CLAUDE.md already grants a page-dot
pulse or focus-ring blink — just continuous instead of one-shot, because a
background task has no natural per-frame "done" edge the way a press/release
cue does.

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

- `on-dark-<w>` — light wordmark, for `zaparoo-dark` / `classic-purple`.
- `on-light-<w>` — dark wordmark, for `zaparoo-light`.

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
caller-supplied `Item` Modal can't measure, so by default it keeps the old
percentage-of-viewport sizing (`min(78% of viewport, panelMaxWidth)`) — the
real ceiling for the QR/legal-notice shells, which just bump `panelMaxWidth`
itself and rely on that 78% breathing-room cap. A shell consumer that
measures its own content precisely — `ListPickerModal` measures its own entry
labels the same way ContextMenu does, and hands the exact target width
through `panelMaxWidth` — sets `contentSized: true` so that number is honored
against the same 92% ceiling the four prebaked kinds use, instead of being
clamped a second time by the 78% cap meant for content Modal can't see. (Round
6 follow-up: the picker's swatch band routinely pushes its measured width
past 78% of a small screen, so leaving the 78% cap unconditional still
truncated rows even after the label-measurement fix below.)

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
computed once per entry when the picker opens rather than per-delegate. Each
swatch box carries a `Theme.textLabel` border at rest (round 6) — a near-black
or near-white swatch previously sat at the same contrast as the row's own
resting background and disappeared into it; `textLabel` is a mid neutral the
semantic-tier guardrails already hold >=3:1 against `bgDeep` on every preset,
so it separates either extreme from the row. On a selected row the border
flips to `bar.contentColor` (`Theme.onAccent`) instead, the same fix the
favorite heart uses against a solid `accent` fill — see "Inverse-video rows"
above.

Content-driven width measurement (this section and [ContextMenu
chrome](#contextmenu-chrome) below) carries deliberate slack over the raw
`FontMetrics.advanceWidth()` figure — `Math.ceil(Math.max(advanceWidth,
boundingRect.width)) + Sizing.stroke(2)`. Labels paint with
`renderType: Text.NativeRendering`, which lays out on integer, hinted
per-glyph advances; `advanceWidth()` alone is `QFontMetricsF`'s fractional,
unhinted total, and a long label can paint a few px wider than that alone
measures. A panel sized to the bare `advanceWidth()` figure — round 5's
`ListPickerModal` bug — could then elide text that should have fit, with
most of the screen still empty. `ContextMenu.qml` already carried this
slack (`+ 2 * Sizing.stroke(2)` in its own `_desiredPanelWidth`);
`ListPickerModal.qml` didn't, despite the header comment saying it mirrors
that file's pattern.

### Themed QR codes

`QrCodeModal.qml` (write-to-token) and `LogUploadModal.qml` (log upload
success) both render the single shared `Browse.QrCode` matrix — a Rust
`qrcode`-crate result exposed as one bit-string row at a time, drawn as
nested `Repeater`s of `Rectangle`s rather than a raster image. Both used to
carry a near-verbatim copy of that matrix with hardcoded `"white"`/`"black"`
fills; round 6 extracts the shared render into `QrMatrix.qml` and themes the
two fills as `Theme.qrLight` (quiet zone + background) and `Theme.qrDark`
(modules).

`qrLight` stays the light rung and `qrDark` stays the dark rung on every
preset, regardless of whether the preset itself is light or dark — inverted
QR is out of spec and scans unreliably on a phone camera, the primary use
of this component. Both ride the accent's own OKLCh hue instead, so the
code still reads as themed (a faint tint on the quiet zone, accent-hued
ink) without ever inverting:

```qml
"qrLight": _gamutFit(0.965, Math.min(accentLch.C, 0.022), accentLch.h),
"qrDark":  _gamutFit(Math.min(accentLch.L, 0.45), accentLch.C, accentLch.h)
```

Measured contrast between the two rungs ranges 6.37:1 (Green Phosphor) to
7.43:1 (Synthwave '84) across the catalog; `test_qr_rungs_stay_scannable`
asserts >=6.0:1 and that `qrLight` is always the lighter rung. The frame
border stays `Theme.borderSubtle`, unthemed.

## ContextMenu chrome

Panel uses `bgPanel` + `radiusMd`, no border. Rows are inverse-video —
`SelectionBar` + `radiusSm` directly against the panel fill, no nested row
surface — see "Two registers" and "Inverse-video rows" above. Panel vertical
padding is independent from radius.

**3-8 entries maximum** (see `docs/content-style.md`'s menu-ordering and
menu-size rules for the content-side reasoning). The `Column` painting rows
has no scroll of its own; `panelHeight` clamps to the window's usable area
and the panel carries `clip: true` as a shipped-build safety net, but a
menu that reaches the cap should be consolidated, not left to rely on that
clip. `ContextMenu.qml` also logs a `console.warn` in development when
`entries.length` exceeds the cap.

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

Hub tiles are square: `PagedGrid.squareCells` clamps `cellWidth`/`cellHeight`
to the smaller of the two independent per-axis fits, fit against both axes of
the Hub's own reserved band. This is a `PagedGrid`-level, opt-in property
(default `false`, byte-identical for every other caller) rather than a
Hub-only calculation, because a naive width-only fit — capped by a constant
hand-tuned for one specific row count — silently stopped being square the
moment the row count varied by tier (round 6 follow-up; see `Sizing.qml`'s
`hubGridShape` comment). The Hub also sets `heightBudget` (a fixed ceiling
distinct from the grid's own `height`, since `height` is itself derived FROM
the fitted cell size — fitting against `height` would be circular). Systems
and media grids use Sizing-declared common resolution shapes with adaptive
fallback for nonstandard desktop/TATE scenes, and leave `squareCells` at its
default. PagedGrid floors uniform cell dimensions, then centers the
cells-plus-insets block against the full inset-to-inset width; odd remainders
may differ by one pixel only.

Default-theme grid gaps (`crt` keeps its own raw pixel values, unaffected):
`systemsGrid`/`gamesGrid` `columnGap` is `pctW(2)` and `gamesGrid` `rowGap` is
`pctH(3)` (matching `systemsGrid`'s own `rowGap`), both set in
`BrowseLayouts.qml`. `PagedGrid.qml`'s `cellSpacingX` fallback (used when no
layout profile supplies `columnGap`) mirrors the same `pctW(2)`.

### Compact tile padding on full-bleed icon/cover grids

`Tile._padding` (`Sizing.pctH(2)` on three sides in non-caption mode) insets
art on every `Tile` caller by default — generic to the component, not
specific to any one screen. Hub tiles and Settings' own root category grid
(which deliberately reuses `Sizing.hubTileSize` so its tiles read as the
same physical object as the Hub's — see above) are both full-bleed
icon/cover tiles with no caption band competing for space, so round 8 gave
them an opt-in `compactPadding` property (same opt-in shape as
`squareCells`) that halves the inset to `Sizing.pctH(1)`. The focus ring's
own inset (`_outlineGap`, `Sizing.pctH(0.4)`) stays well clear either way
— it anchors to the tile's full bounds independently of `_padding`, so
tightening the art padding can never make art touch or overlap the ring.
Systems tiles are a separate, viewport-fitted grid and stay on the default
padding; only Hub and Settings' category grid opt in.

Two other levers were considered and set aside rather than folded into
this pass: fewer Hub columns (`hubGridShape` is deliberately fixed, not
viewport-fitted — see above — specifically so a hand-arranged Hub layout
can't get scrambled by a display change; fewer columns reopens that), and
`Image.PreserveAspectCrop` for game covers (a bigger visual gain, but
crops the top/bottom of every cover, where title art often sits — worth a
follow-up look against real covers, not bundled into a padding change).

### No scrollbars — grids are paged, not scrolled

`PagedGrid` has no in-grid scroll indicator of any kind — no gutter, no
proportional thumb. It never did have a scroll-position concept to represent
(it pages, it doesn't scroll); a right-side gutter conditionally reserved
whenever the dataset happened to be multi-page was the wrong metaphor, and
worse, it made the cell block visibly shift and shrink the instant a
single-page grid became multi-page (arming Hub Options → Move always reserves
a second page, so this fired on every single arm). `_availableWidth` is
simply `width - leftInset - rightInset` — nothing subtracts space for a
gutter, so a grid's geometry is identical regardless of page count.

The "where am I" cue is a count badge plus `PageIndicator` (up/down chevrons —
`ScrollUp`/`ScrollDown`, the same glyphs the old gutter used — plus "N / M").
The "N / M" text (and the "N" it falls back to when the total isn't known
yet) only paints once there's actually somewhere else to page to —
`PageIndicator._hasMultiplePages` — since a single page never needs a "1 / 1"
readout next to two chevrons that are also both hidden; the reserved width
doesn't change either way, so this never causes the shift the paragraph below
is about. It sits alongside `TopStatusStrip`'s title, baseline-aligned to it
(`TopStatusStrip.pageIndicatorMode`), on every theme except CRT — CRT hides
that strip entirely (`status.topStripVisible: false`) and keeps the same cue
in the host screen's **footer** instead, alongside `ActiveLabel`
(`footer.pageCueInFooter` in `BrowseLayouts.qml` is the profile flag both
placements key off). Wherever it lives, the badge and `PageIndicator` are
unconditionally reserved — only their content toggles (a chevron's own
`visible`, the count text's presence) — so a single-page grid becoming
multi-page (arming Hub Options → Move always reserves a second page) never
shifts anything.

Two placements exist because putting the cue at the top, next to a title
that's already there, was tried first (pre-round-5) and reads better once a
footer that's ALSO carrying the focused item's own title has room to spare —
`ActiveLabel.sideInset` reverts to its own default (`pctW(3)`) instead of a
`width/3` corner reservation whenever the footer isn't hosting the count/page
slots, roughly doubling the room a long focused title gets before eliding.
CRT's footer is the one place that still needs the full three-slot arrangement,
since CRT has nowhere else to put it. See `PageIndicator.qml`'s doc comment for
why each chevron is anchored off a fixed chain rather than a `Row` (hiding one
must not shift the "N / M" text next to it) and for `chevronSpacing`, a
tighter gap between the two chevrons than between the pair and the text —
Gestalt proximity: the chevrons are one control, the text is a separate
readout, and the glyphs' own baked-in side bearing already makes an *equal*
gap read backwards. List layouts (Settings-style vertical lists, `BrowseList`)
are unaffected by any of this; they use a fixed chevron-band reserved by
margin, unrelated to paged grids entirely.

### Empty slots

A grid row that is a deliberate structural placeholder (not a real item with
nothing on it) renders through `PagedGrid.emptyDelegate` — a small
`EmptySlot.qml` component — instead of the normal per-item delegate. The
model row's `isEmpty` role selects it; `emptyDelegate: null` (the default,
every caller but the Hub) makes `isEmpty` a no-op, so this is opt-in per
caller. `PagedGrid`'s own card-shaped skeleton placeholder (normally painted
behind every cell while its Tile incubates, on the assumption the Tile will
end up fully opaque on top) is skipped entirely for `isEmpty` rows — an
empty slot is not opaque, so that assumption doesn't hold, and the skeleton
would otherwise stay visible underneath it permanently.

An empty slot paints nothing, ever — not even a focus ring. Normal browsing
cannot land the cursor on one at all: `PagedGrid.skipEmptyCells` (which the
Hub sets to `!moveArmed`) makes every cursor path — `moveSelection`, `pageBy`,
and the per-cell mouse hover/click — treat an `isEmpty` row as unreachable.
Left/Right still step past a blank within the same row, same as before; Up/Down
instead run a nearest-candidate search across every real tile on the whole
board — a same-column-only walk tunnels past nearby content on a freely
arranged Hub layout, landing on a distant aligned tile instead of a much
closer one a column over. See `PagedGrid.qml`'s `_nearestVerticalCandidate`
doc comment for the weighting (borrowed from Android's `FocusFinder`, the
algorithm behind d-pad navigation on Android TV). The one place a blank IS
still a legitimate destination is a Move session (Hub Options → Move), which
needs to target one — or the reserve page `beginMove` pads in — to place a
held tile there; Move disarms `skipEmptyCells` for exactly that reason, and
the held tile swaps into the cell within the same synchronous call, so no
frame ever actually paints `EmptySlot` under the cursor either way.

This wasn't always the rule. An earlier version let the cursor rest on a gap
and drew an accent ring around it so it still read as "here." It didn't work:
every other focused thing in the app is a solid card with a ring wrapped
around it, and a ring drawn over a face-less `Item` has nothing to wrap —
it read as a stray rectangle floating on the background, not a cursor. WAI-ARIA's
own guidance points the same way (disabled/inert things are normally not
focusable at all), and the nearest real-world analogue — iOS 18's home-screen
gaps — only accepts a drop while you're actively rearranging, exactly the
Move-only rule here. Landing on a blank had exactly one other job: aiming
View → Add item… at that specific gap. Add now arms Move on the newly placed
item instead, so the "put it exactly where I want" affordance survives
without needing the cursor to ever rest on empty space outside a Move
session.

## Consistency rules

- Rounded square chooses `radiusMd` or `radiusSm`; pill chooses half-height.
- Grid content and standalone commitment buttons use `PressableSurface`;
  vertical option lists (browse, Settings, menus, pickers) use `SelectionBar`.
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
