# Quickstart

## 1. Install prerequisites

Install the pieces you do not already have.

### Linux

**Fedora / RHEL:**
```bash
sudo dnf install qt6-qtdeclarative-devel qt6-qtquickcontrols2-devel \
    qt6-qttools-devel cmake ninja-build mold clang-tools-extra just
```

**Ubuntu / Debian:**
```bash
sudo apt install qt6-declarative-dev qt6-quick-controls2-dev \
    qt6-tools-dev qt6-l10n-tools cmake ninja-build mold \
    clang-tidy clang-format just
```

(If `just` isn't packaged for your distro, install it with
`cargo install --locked just` after Rust is set up.)

### Rust

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

After cloning the frontend repo (step 2), run `just install-tools` to install
the optional cargo extensions (`cargo-nextest`, `cargo-deny`) used by the
lint and test recipes.

### macOS / Windows

macOS is best-effort and not covered in CI. Qt package names change often
enough that Linux is the supported path unless you are prepared to debug local
setup. Windows is not tested; use WSL2.

## 2. Clone and build

```bash
git clone https://github.com/ZaparooProject/zaparoo-frontend.git
cd zaparoo-frontend
just build
```

The first build pulls and compiles the Rust and Qt dependencies. Incremental
builds are much faster after that.

## 3. Start the mock Core

In one terminal:

```bash
just mock-core
```

You should see:

```text
mock-core listening on ws://127.0.0.1:27497/api/v0.1
```

The mock serves three categories (Consoles, Handhelds, Arcade), ten systems,
and fifty games. That is enough data to exercise every frontend screen.

`27497` is offset from the real Core's `7497` so a real Core, or another Core
test instance, can run on the same machine without colliding with the mock. The
frontend still defaults to `7497` in production. `just run-dev` points it at
the mock through `ZAPAROO_CORE_ENDPOINT`; `just run` reads
`~/.config/zaparoo/frontend.toml` as usual.

### Pick a different port

If something already uses `27497`, override it at startup:

```bash
MOCK_CORE_ADDR=127.0.0.1:9000 just mock-core
ZAPAROO_CORE_ENDPOINT=ws://127.0.0.1:9000/api/v0.1 just run-dev
```

`ZAPAROO_CORE_ENDPOINT` always wins over `~/.config/zaparoo/frontend.toml`.

## 4. Run the frontend

In a second terminal:

```bash
just run-dev
```

`run-dev` points at the mock through `ZAPAROO_CORE_ENDPOINT`. `just run` is
the production-style runner: it reads `~/.config/zaparoo/frontend.toml`
instead.

For a desktop CRT preview, use:

```bash
ZAPAROO_CRT_PREVIEW_RESOLUTION=320x240 just run-dev
```

Setting `ZAPAROO_CRT_PREVIEW_RESOLUTION` also enables CRT preview mode. It
accepts direct `WxH` values and aliases from Update All's CRT selector:
`ntsc-720`, `ntsc-640`, `ntsc-512`, `ntsc-352`, `ntsc-336`, `ntsc-320a`,
`ntsc-320b`, `ntsc-304`, `pal-640`, `pal-512`, `pal-384`, `pal-352`, and
`pal-320`. `ZAPAROO_CRT_PREVIEW_SCALE` controls the integer desktop scale.

## 5. Check the result

- The frontend window opens.
- A static **categories row** of tiles fills with "Favorites",
  "Arcade", "Consoles", "Handhelds". Left/Right cycles between them.
  ("Favorites" is a placeholder until a real Favorites endpoint
  lands in Core; selecting it shows an empty systems grid.)
- A second action row contains "Favorites", "Recently Played", optional
  "Update", and "Settings". "Update" opens the update screen and starts
  the Rust-driven progress bar when the update feature is enabled.
- Pressing Enter drops you into the **paged systems grid** for that
  category. Use Left/Right to move within a page; the grid wraps to
  the next page at the row edge.
- Pressing Enter on a system opens the **paged games grid** (five
  entries per system).
- Pressing Enter on a game sends a `run` RPC to the mock. The mock logs the
  selected game's ZapScript, but the frontend keeps running because nothing is
  actually launched.
- Pressing Tab on a system or game sends a `readers.write` RPC with the
  selected entry's ZapScript. The frontend shows a card-write modal while the
  request is pending; the mock logs the write request.
- Escape backs out; Escape on the top level opens a quit-confirm modal —
  confirm to exit.

The FPS counter in the corner should stay green (≥ 55). Red means the UI fell
below 30 FPS and needs investigation.

## 6. Run tests and lints

Before you open a pull request:

```bash
just lint    # clang-format, clang-tidy, qmllint, rustfmt, clippy, cargo-deny
just test    # ctest + cargo nextest
```

Zero warnings is the bar. If lint complains about formatting or a fixable
clippy issue, `just fix` auto-applies everything CI would accept.

If you would rather not install the lint tools on the host (Qt, clang-format,
qmlformat, cargo-deny, etc.), use the Docker variants — `just fmt-docker`,
`just lint-docker`, `just fix-docker`. See [`docs/building.md`](building.md)
for the full list.

## Next steps

- [`docs/building.md`](building.md): sanitizer builds, ARM32
  cross-build for MiSTer, deployment.
- [`docs/architecture.md`](architecture.md): module graph, Rust↔QML
  data flow, Runtime vs Platform distinction.
- [`docs/qml-gotchas.md`](qml-gotchas.md): QML pitfalls that `qmllint`
  only catches after the fact.
- [`docs/cxx-qt-bridge.md`](cxx-qt-bridge.md): cxx-qt 0.8 bridge
  constraints when editing Rust QML models.
- [`CONTRIBUTING.md`](../CONTRIBUTING.md): CLA flow, PR expectations,
  branch-protection rules.
