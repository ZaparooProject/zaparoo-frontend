# Zaparoo Frontend

Zaparoo Frontend is the game frontend for
[Zaparoo Core](https://zaparoo.org).

## Build

The frontend is written in Rust with [Slint](https://slint.dev). Start with
[docs/quickstart.md](docs/quickstart.md) for a first run against the mock
Core, and [docs/building.md](docs/building.md) for the packages you need on a
fresh machine and the MiSTer build.

Most commands go through the [`justfile`](justfile). Run `just --list` if you
need the full menu.

```bash
just build && just run    # desktop
just run-dev              # desktop against the mock Core
just arm32                # MiSTer ARM32 static build
just test                 # cargo nextest, desktop and MiSTer feature sets
just lint                 # rustfmt, clippy (all feature sets), cargo-deny,
                          # toolkit-free guard, translations, notices, logo parity
```

`just test`, `just lint` and `just arm32` run inside the project's toolchain
image, so they need only Docker and `just`.

## Customize

You can override system artwork, the Hub menu icons, and system display names
without rebuilding. See [docs/customization.md](docs/customization.md).

## Trademarks

This repository includes Zaparoo trademarks used here with permission from the
trademark owner. If you redistribute or adapt the project, remove or replace
those marks first. See the Zaparoo [Terms of Use](https://zaparoo.org/terms/)
for the details.

## License

Copyright 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
Source available under the [PolyForm Noncommercial License 1.0.0](COPYING).
Non-commercial use only. For commercial licensing, contact
[legal@zaparoo.com](mailto:legal@zaparoo.com).

Third-party components:

- **Slint** UI toolkit: used under the Slint Software License, which covers
  embedded distribution. The license text and the notices for every Rust
  crate linked into the binary are in
  [`rust/frontend/LICENSES/THIRD-PARTY-NOTICES.txt`](rust/frontend/LICENSES/THIRD-PARTY-NOTICES.txt).
- **Noto Sans** fonts: SIL Open Font License 1.1, © The Noto Project Authors.
  See [`LICENSES/NotoSans-ATTRIBUTION.txt`](LICENSES/NotoSans-ATTRIBUTION.txt)
  and [`LICENSES/NotoSans-OFL.txt`](LICENSES/NotoSans-OFL.txt).
- **MxPlus HP 100LX 6x8** font: Creative Commons Attribution-ShareAlike 4.0
  International, © VileR. See
  [`LICENSES/MxPlus-ATTRIBUTION.txt`](LICENSES/MxPlus-ATTRIBUTION.txt).
- **Iconoir** UI icons: MIT License, © Luca Burgio and contributors.
  See [`LICENSES/Iconoir-ATTRIBUTION.txt`](LICENSES/Iconoir-ATTRIBUTION.txt).
- **Lucide** UI icons: ISC License, © 2024 Lucide Contributors (fork of Feather
  Icons by Cole Bemis). See
  [`LICENSES/Lucide-ATTRIBUTION.txt`](LICENSES/Lucide-ATTRIBUTION.txt).
- **Streamline** Core line icon (Handheld category): © Webalys LLC, used
  under the Streamline Free License — <https://streamlinehq.com>. See
  [`LICENSES/Streamline-ATTRIBUTION.txt`](LICENSES/Streamline-ATTRIBUTION.txt).
- **Input Prompts** by Kenney, CC0 1.0 (help-bar button and D-pad glyphs) —
  <https://kenney.nl/assets/input-prompts>. See
  [`LICENSES/Kenney-ATTRIBUTION.txt`](LICENSES/Kenney-ATTRIBUTION.txt).
- **Console logos** redrawn by Dan Patrick (MIT-licensed compilation; platform
  marks remain trademarks of their respective owners). See
  [`LICENSES/console-logos-ATTRIBUTION.txt`](LICENSES/console-logos-ATTRIBUTION.txt).
- **Wikimedia Commons public-domain text/logo assets** used for specific
  missing system logos. See
  [`LICENSES/wikimedia-public-domain-ATTRIBUTION.txt`](LICENSES/wikimedia-public-domain-ATTRIBUTION.txt).
- **Noun Project icons** used in 2-player system logo composites. See
  [`LICENSES/NounProject-ATTRIBUTION.txt`](LICENSES/NounProject-ATTRIBUTION.txt).

See all bundled asset and third-party notices in [`LICENSES/`](LICENSES/).
