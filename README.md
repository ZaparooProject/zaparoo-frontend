# Zaparoo Frontend

Zaparoo Frontend is the game frontend for
[Zaparoo Core](https://zaparoo.org).

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
  See [`src/LICENSES/Qt-LGPL-NOTICE.txt`](src/LICENSES/Qt-LGPL-NOTICE.txt)
  and [`src/LICENSES/LGPLv3.txt`](src/LICENSES/LGPLv3.txt).
- **Noto Sans** fonts: SIL Open Font License 1.1, © The Noto Project Authors.
  See [`src/LICENSES/NotoSans-ATTRIBUTION.txt`](src/LICENSES/NotoSans-ATTRIBUTION.txt)
  and [`src/LICENSES/NotoSans-OFL.txt`](src/LICENSES/NotoSans-OFL.txt).
- **MxPlus HP 100LX 6x8** font: Creative Commons Attribution-ShareAlike 4.0
  International, © VileR. See
  [`src/LICENSES/MxPlus-ATTRIBUTION.txt`](src/LICENSES/MxPlus-ATTRIBUTION.txt).
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

See all bundled asset and third-party notices in [`src/LICENSES/`](src/LICENSES/).
