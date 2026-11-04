# Customization

The frontend lets you personalize a few things without rebuilding: system
artwork, the Hub menu icons, and the display names of systems. Everything
lives in one place and falls back to the built-in defaults, so you only supply
what you want to change.

Files in the `custom/` folder are read **once at startup**. After adding,
changing, or removing files there, restart the frontend to pick them up. A
Hub tile's name and icon can also be set from the Hub itself (see "Renaming
a tile and changing its icon" below), and those apply at once.

## The `custom/` folder

Override images go in a single customization root with one subfolder per kind:

```text
<root>/
  systems/      # system artwork (the paged systems grid)
  hub/          # Hub menu icons (categories + actions)
```

The default root is:

- **MiSTer:** `/media/fat/zaparoo/custom/`
- **Desktop:** next to `frontend.toml` (e.g. `~/.config/zaparoo/custom/`)

No configuration is needed - just create the folders and drop files in. To put
the root somewhere else, set it in `frontend.toml`:

```toml
[custom]
dir = "/media/usb/zaparoo-art"
```

### Naming and formats

Name each file after the **id** of the thing it overrides, with any of these
extensions: `png`, `jpg`, `jpeg`, `webp`, `bmp`, `svg`. Matching is
**case-insensitive** (`snes.png` and `SNES.png` are equivalent), so use
whatever casing you like - just provide one file per id.

**System art** (`systems/`) is keyed by the Zaparoo system id - the same id
used by the bundled logos under `resources/images/systems/`:

```text
systems/snes.png
systems/genesis.svg
systems/turbografx16.png
```

**Hub icons** (`hub/`) are keyed by category id or action id:

| id          | Hub item             |
|-------------|----------------------|
| `arcade`    | Arcade category      |
| `computer`  | Computer category    |
| `console`   | Console category     |
| `handheld`  | Handheld category    |
| `resume`    | Resume Game          |
| `favorites` | Favorites            |
| `recents`   | Recently played      |
| `search`    | Search               |
| `settings`  | Settings             |

```text
hub/arcade.png
hub/favorites.svg
hub/settings.svg
```

Any Hub tile (a built-in one, a specific system, a folder, a saved search,
or arbitrary ZapScript; see `[[hub.items]]` in `frontend.toml`) can set its
own `icon`, which takes one of two forms:

- A name, `icon = "name"`, matched against `hub/name.png` (or any of the
  extensions above) in this folder, the same way a built-in tile's id is.
- An absolute path, `icon = "/media/usb0/art/zelda.png"`, which is read from
  where it is. The file stays where it is; nothing is copied. A file that
  has moved, or a drive that is not mounted yet, shows the tile's own icon,
  and stays that way until the next restart.

A tile's own `icon` wins over a `hub/<id>` file for a built-in tile, which
wins over the bundled icon.

### Hub tile entries

Each tile is one `[[hub.items]]` table in `frontend.toml`. "Add to Hub"
writes these for you; they can also be written by hand.

| `type`      | Keys                                                  |
|-------------|-------------------------------------------------------|
| `system`    | `id`: the system id                                   |
| `folder`    | `path`, `system`, and optionally `relative`           |
| `zapscript` | `script`, and optionally `relative`, `path`, `system` |
| `search`    | any of `query`, `systems`, `tags`, `path`             |

Any tile can also set `name` and `icon`, the built-in categories and actions
included. A `name` on a built-in tile replaces its translated word, so it
stays the same in every language; on Resume it leads the game's name
(`Continue: Zelda`).

```toml
[[hub.items]]
type = "zapscript"
relative = "NES/Zelda.nes"
script = "@NES/The Legend of Zelda"
path = "/media/fat/games/NES/Zelda.nes"
name = "The Legend of Zelda"
system = "NES"
```

A pinned game keeps every way Zaparoo Core identified it, and launches by
the first one Core can still resolve:

1. `relative`: the path under the system's games folder. It keeps working
   when the games move to another drive.
2. `script`: a ZapScript title launch. It keeps working when the file moves
   to a different folder.
3. `path`: the full path at the time the game was pinned.

`path` and `system` also select the tile's cover art. After Core rescans
your games, the frontend asks Core where each pinned game and folder is now
and updates `path` to match, so covers and folder tiles follow a move too.
A tile with only a `script` is launched exactly as written and is never
changed.

### Renaming a tile and changing its icon

A tile's Options menu has **Rename tile…** and **Change icon…** (on a game or
system tile they are on its Manage page, with Move tile and Remove from
Hub). Both write the tile's `[[hub.items]]` entry for you.

- **Rename tile…** opens the name on the on-screen keyboard. Save keeps
  it; Back leaves it as it was. Saving an empty name, or the tile's own
  name, goes back to the tile's own name and removes `name` from the file.
  A script tile has no name of its own to go back to, so it cannot be
  saved empty.
- **Change icon…** opens the file browser on the folder of the tile's
  current icon, or `custom/hub/` the first time. It lists folders and
  images as previews. Picking an image saves its absolute path as `icon`.
  Back goes up a folder and, from the top of a drive, to Places (the
  custom folder, the SD card, USB drives and network shares on MiSTer;
  Home and removable drives on a desktop). The Menu button has **Use
  default icon**, which removes `icon` from the file.

An embedding host that runs its own image picker shows that instead of the
file browser. A host without one does not offer Change icon….

#### Saved searches

A `search` tile opens the results of a search. "Save to Hub" in the Search
results' Menu writes one for the search on screen; Back from results
opened this way returns to the Hub.

```toml
[[hub.items]]
type = "search"
name = "SNES and Genesis RPGs"
query = "final"
systems = ["SNES", "Genesis"]
tags = ["genre:rpg", "year:1994"]
icon = "rpgs"
```

- `query`: the text to match in names. Leave it out to list everything the
  other keys allow.
- `systems`: the system ids to search. Leave it out to search every system.
  A tile can name several, as one saved from a category's "Search here…"
  does; the Search screen's System field picks one.
- `tags`: `type:value` tags a game must carry. They are sent to Zaparoo Core
  exactly as written, so a tile can also use tags the tags picker does not
  offer. A search saved from the favorites carries `user:favorite`.
- `path`: limits the search to one folder, as "Search here…" does. It is
  the full path, so it has to be edited by hand if the games move to another
  drive.

A tile needs at least one of the four. Without an `icon` it shows the Search
glyph.

### Rendered as-is (no tinting)

The bundled system logos default to tintable SVGs that match the active theme.
Settings → Appearance → System logos can switch bundled system art to the
full-color logo set; a system the color set does not cover shows its grayscale
logo un-tinted. Hub icons remain theme-tinted. **Your override images are not
tinted** - they are shown exactly as they are on disk, in full color. If you
want an image that tracks the theme colors, supply a monochrome SVG drawn in
the theme's terms; otherwise expect your PNG/JPG to appear unchanged.

### Loading and memory limits

Artwork discovery, file reads, decoding, and resizing run in the background.
Built-in art remains visible until an override is ready. Original files are
never changed; prepared images keep their colors and aspect ratio.

Preparation is serial, with at most 32 queued requests and a 16 MiB in-memory
LRU cache, shared with the file browser's image previews and the icons named
by an absolute path. Bitmaps must fit an 8 MiB file limit, an 8192-pixel source-edge limit,
and the decoder's 32 MiB allocation budget. Output follows the displayed size,
capped at 1024 pixels per edge. Small bitmaps are not enlarged during preparation.

SVG files are limited to 256 KiB and must be self-contained vector artwork:
referenced/embedded images and filter graphs fall back to built-in art rather
than bypassing the decode-memory limits. Paths, gradients, clipping, and masks
remain supported. Discovery retains up to 512 files per namespace within a
shared 1 MiB index budget. Invalid or over-budget files use built-in art.

## Color scheme

Choose a preset live under Settings → Appearance → Color scheme. There are 19
built-in presets — Zaparoo Dark, Zaparoo Light, and Classic Purple, plus
popular editor/terminal palettes (Nord, Dracula, Synthwave '84, Gruvbox,
Everforest, Solarized Dark, Rosé Pine, Oxocarbon), a couple of light-register
alternatives (Flexoki Paper, Solarized Light), and retro/console-inspired
ones (Amber Phosphor, Green Phosphor, Neo Geo, NES, Virtual Boy, Game Boy) —
each shown with a 3-color swatch preview in the picker. Selection is stored
as `[settings] color_scheme` in `frontend.toml`:

```toml
[settings]
color_scheme = "zaparoo-dark" # or "nord", "dracula", "amber-phosphor", ...
```

See `ids()` in `rust/zaparoo-app/src/palette.rs` for every available id.
Unknown values fall back to `zaparoo-dark`. Presets recolor built-in tinted
artwork; custom and full-color artwork remains unchanged.

## System display names

To rename a system for display (for example `SNES` to `Super Nintendo`, or to
fix capitalization), add a `[custom.system_names]` table to `frontend.toml`
keyed by system id:

```toml
[custom.system_names]
snes = "Super Nintendo"
psx = "PlayStation"
genesis = "Mega Drive"
```

The key is the system id (same as the `systems/` image filenames) and the
value is the name to show. The key match is forgiving about case and
punctuation (`snes`, `SNES`, and `S.N.E.S` all resolve to the same entry). An
override here takes priority over the built-in regional names and the name
reported by Zaparoo Core.

## Config file locations

- **MiSTer:** `/media/fat/zaparoo/frontend.toml`
- **Desktop:** `~/.config/zaparoo/frontend.toml`

Save the file as UTF-8. A file in another encoding still loads, with any
character that is not valid UTF-8 replaced by `�`; the next time the frontend
saves a setting it rewrites the file as UTF-8 and keeps the original beside it
as `frontend.toml.bak`.

A file that is not valid TOML, or that gives a key the wrong type of value, is
not loaded at all. The frontend starts on its defaults, shows an alert naming
the file, and writes nothing to it until it is fixed or removed and the
frontend is restarted. The log names the line.
