# Zaparoo Frontend

Zaparoo Frontend is the game frontend for
[Zaparoo Core](https://zaparoo.org).

## Early beta rename

This project was renamed from Zaparoo Launcher to Zaparoo Frontend.
The repository slug is now `zaparoo-frontend`, the binary is `frontend`,
the config file is `frontend.toml`, and the log file is `frontend.log`.
No automatic migration is provided for old beta installs; copy any settings
from `launcher.toml` to `frontend.toml` manually if needed.

## Build

Start with [docs/building.md](docs/building.md). It covers the packages you
need on a fresh machine and the MiSTer cross-build path.

Most commands go through the [`justfile`](justfile). Run `just --list` if you
need the full menu.

```bash
just build && just run    # desktop
./scripts/build-arm32.sh  # MiSTer ARM32 cross-build (Docker-only)
just test                 # ctest + cargo nextest
just lint                 # clang-format, clang-tidy, qmllint, rustfmt, clippy, cargo-deny
```

The MiSTer ARM32 path uses the official Docker Buildx toolchain image and does
not need Qt, CMake, Rust, or `just` installed on the host.

`just test` and `just lint` need `cargo-nextest` and `cargo-deny`:

```bash
cargo install --locked cargo-nextest cargo-deny
```

## Trademarks

This repository includes Zaparoo trademarks used here with permission from the
trademark owner. If you redistribute or adapt the project, remove or replace
those marks first. See the Zaparoo [Terms of Use](https://zaparoo.org/terms/)
for the details.

## License

Copyright 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
Source available under the [PolyForm Noncommercial License 1.0.0](COPYING).
Non-commercial use only. For commercial licensing, contact
[legal@zaparoo.org](mailto:legal@zaparoo.org).

Third-party components:

- **Qt framework**: LGPLv3. Dynamically linked on desktop builds; statically
  linked on MiSTer ARM32. Object files for re-linking against a modified Qt
  are available on request at
  [legal@zaparoo.org](mailto:legal@zaparoo.org).
  See [`src/LICENSES/`](src/LICENSES/).
- **Atkinson Hyperlegible** font: SIL Open Font License 1.1, © 2020 Braille
  Institute of America, Inc. See
  [`src/LICENSES/AtkinsonHyperlegible-OFL.txt`](src/LICENSES/AtkinsonHyperlegible-OFL.txt)
  and
  [`src/LICENSES/AtkinsonHyperlegible-ATTRIBUTION.txt`](src/LICENSES/AtkinsonHyperlegible-ATTRIBUTION.txt).
- **Iconoir** UI icons: MIT License, © Luca Burgio and contributors.
  See [`src/LICENSES/Iconoir-ATTRIBUTION.txt`](src/LICENSES/Iconoir-ATTRIBUTION.txt).
- **Lucide** UI icons: ISC License, © 2024 Lucide Contributors (fork of Feather
  Icons by Cole Bemis). See
  [`src/LICENSES/Lucide-ATTRIBUTION.txt`](src/LICENSES/Lucide-ATTRIBUTION.txt).
- **Streamline** Core line icon (Handheld category): © Webalys LLC, used
  under the Streamline Free License — <https://streamlinehq.com>. See
  [`src/LICENSES/Streamline-ATTRIBUTION.txt`](src/LICENSES/Streamline-ATTRIBUTION.txt).
- **Controller Input Icons** by ElDuderino, released into the public domain.
  See [`src/LICENSES/controller-icons-ATTRIBUTION.txt`](src/LICENSES/controller-icons-ATTRIBUTION.txt).
- **Console logos** redrawn by Dan Patrick (MIT-licensed compilation; platform
  marks remain trademarks of their respective owners). See
  [`src/LICENSES/console-logos-ATTRIBUTION.txt`](src/LICENSES/console-logos-ATTRIBUTION.txt).
