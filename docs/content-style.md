# Content Style

`docs/style.md` covers pixels and color. This file covers words: what a
menu entry says, what order it appears in, and which term wins when two
words mean the same thing. It exists because the app outgrew its original
design faster than its wording conventions did — menus grew one entry at a
time, each reasonable on its own, and the result drifted into inconsistent
order, case, and vocabulary. Follow this file when adding or renaming any
user-visible string; don't re-derive these rules from nearby code, which
may itself predate them.

## Menu ordering

Order every `ContextMenu` ("Options") and `ListPickerModal` ("View") entry
list by this priority, top to bottom:

1. The primary action — what most users came to this menu to do (usually
   `Launch game` / `Launch system`).
2. Frequent secondary actions (favorite, write a token, details).
3. Organizational actions (Move, Add to Hub, Hide).
4. Long-running maintenance actions (Update media database, Scrape
   metadata) — always last. These start a background job the user waits
   on; they are never what most people open the menu to do.

A destructive or permanent action (Delete) is never first, and sits with
the other organizational actions rather than at the very bottom next to
maintenance — permanence and slowness are different kinds of "be careful
here" and shouldn't be conflated into one junk-drawer tail.

This mirrors both established guidance and this app's own constraints:
GNOME's HIG says order items "by importance, task order, or expected
frequency of use"; Nielsen Norman Group's contextual-menu guidelines say
to "list commands in frequency-of-use order." On a six-button controller
(see `docs/architecture.md`'s input section) every extra row is an extra
press, so getting the top of the list right matters more here than on a
mouse-driven desktop menu.

## Menu size cap

**3–8 entries per `ContextMenu`.** GNOME's HIG allows 3–12, but
`ContextMenu.qml` has no scroll or clip safety margin built for that many
rows on a 240p panel (see `docs/style.md` → "ContextMenu chrome" for the
enforced backstop). A menu that would grow past 8 needs consolidation —
fold two related actions into one, or move a rarely-used one into
Settings — not a scrollbar.

## Capitalization

**Sentence case** for every user-visible label: menu entries, settings
rows, page titles, button-help text. Capitalize only the first word and
proper nouns.

- Right: `Show hidden items`, `Update media database`, `Write to NFC
  token`
- Wrong: `Show Hidden Items`, `Recently Played`, `Settings & Utilities`

Proper nouns keep their casing regardless of position: **Hub**,
**Zaparoo**, **Core**, **NFC**, **QR**, and preset/system names (Dracula,
Nord, SNES, N64, …).

## Verb form

Per GNOME's HIG: commands are verbs, settings are adjectives or nouns.

- Commands: `Launch`, `Write`, `Add`, `Scrape`, `Move`, `Hide`, `Delete`.
- Settings: `Reduce motion`, `Debug logging`, `Mouse support` — not
  `Reduce the motion` or `Enable debug logging`.

## Ellipsis

Use the real ellipsis character (U+2026, `…`), never three ASCII periods.
Add it only when the action needs more input or confirmation before it
runs (GNOME's rule) — never on a row that just opens a subpage or a plain
list.

- Right: `Saving…` (an in-progress action), `Screen position` (no
  ellipsis — Accept jumps straight into calibration, no further input
  needed before that starts)
- Wrong: `Go to...` (ASCII dots), an ellipsis on `Change launcher` (opens
  a list, doesn't need pre-filled input)

## Punctuation

- **Em dash marks a reason clause** (why something stopped): `Core error
  — %1`, `Indexing paused — game running`.
- **Colon marks a live detail** of an ongoing action: `Indexing: %1`,
  `Sort: %1`.
  Don't conflate the two — see `docs/style.md` → "Header status line" for
  the full reasoning.
- No trailing periods on labels, headings, or menu entries. Body/prose
  sentences (error messages, About text) do end with periods.
- Use an ASCII hyphen only inside a prose sentence, never as a list
  separator or in place of an em dash.

## Terminology glossary

One term per concept, matching the vocabulary Zaparoo's own docs
(zaparoo.org) use. Never mix the left and right columns.

| Use this | Not this | Meaning |
|---|---|---|
| **token** | card, tag | The physical NFC/QR/barcode object a user scans or writes |
| **game** | file, entry, item | A piece of launchable media |
| **system** | platform, console (in labels) | An emulated/native platform, e.g. SNES, Genesis |
| **launcher** | core, emulator (in labels) | The program that runs a game on a system |
| **Hub** | home, main menu | The frontend's root screen |
| **Core** | server, backend | The Zaparoo Core service this frontend talks to — never used for an FPGA/emulator core, which is a **launcher** |
| **media database** | library, index (as a noun) | The scanned catalog of games; "Update media database" is always spelled out, never bare "Update" |
| **Automatic** | Auto | The default/no-preference picker choice, spelled out everywhere it appears (resolution, language, region, clock, artwork) |

`Update` (bare, capitalized, as a Hub tile) is reserved for the app/
firmware updater. It is never used for the media-database scan, which is
always the full phrase above.

## Length budget

Menu and settings-row labels should read comfortably at the 240p tier
(see `docs/style.md` → "Resolution tiers"). `ContextMenu.panelWidth`
tracks the single widest entry in the open menu, so one long label widens
the whole panel. Treat anything near the length of `Discover arcade
alternate versions` as a signal to either shorten the label or move the
explanation into a Settings description line (see `docs/style.md` →
"Settings section headers" and this file's checklist below) rather than
lengthening a menu row further.

## Adding a menu entry — checklist

1. Where does it fall in the ordering rule above — primary, frequent,
   organizational, or maintenance?
2. Which glossary term applies? Don't introduce a new synonym.
3. Does adding it push the menu past 8 entries? If so, consolidate first.
4. Is it wrapped in `qsTr()`? (Three menu strings shipped without it in
   the past — see `docs/translations.md`.)
5. Sentence case, no trailing period, ellipsis only if it needs more
   input first.

## Adding a setting — checklist

1. Which of the six Settings pages does it belong to **by what the user
   is trying to do**, not by which Rust module or QML file implements it?
   See `SettingsScreen.qml`'s page-domain doc comment for the current six
   domains.
2. Does it need a "restart required" or "takes effect next launch"
   description line? If the setting doesn't apply live, say so in the
   description — don't let the restart-confirm modal be the first the
   user hears of it.
3. Is the row's own label enough, or does it need a description to avoid
   ambiguity? Prefer a short label plus a description over a long,
   over-explained label. A description renders in the shared hint band at
   the bottom of the settings card (see `docs/style.md` → "Settings hint
   band"), not on the row itself — write to a **two-line budget**: it wraps
   and elides past that, and the band's width is the card's own width
   minus its side padding, narrower again at the 240p/CRT tier.
4. Sentence case, and reuse an existing picker's value vocabulary
   (`Automatic`, not a new synonym) wherever the same concept recurs.
