# Customization

The frontend lets you personalize a few things without rebuilding: system
artwork, the Hub menu icons, and the display names of systems. Everything
lives in one place and falls back to the built-in defaults, so you only supply
what you want to change.

All customization is read **once at startup**. After adding, changing, or
removing files, restart the frontend to pick them up.

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
| `recents`   | Recently Played      |
| `settings`  | Settings & Utilities |

```text
hub/arcade.png
hub/favorites.svg
hub/settings.svg
```

### Rendered as-is (no tinting)

The bundled system logos and Hub icons are monochrome SVGs that the app tints
to match the active theme. **Your override images are not tinted** - they are
shown exactly as they are on disk, in full color. If you want an image that
tracks the theme colors, supply a monochrome SVG drawn in the theme's terms;
otherwise expect your PNG/JPG to appear unchanged.

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
