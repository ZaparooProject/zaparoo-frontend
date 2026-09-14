# Quickstart

## 1. Install prerequisites

Install the pieces you do not already have.

### Linux

**Fedora / RHEL:**
```bash
sudo dnf install fontconfig-devel wayland-devel libxkbcommon-devel \
    systemd-devel mold just
```

**Ubuntu / Debian:**
```bash
sudo apt install libfontconfig1-dev libwayland-dev libxkbcommon-dev \
    libudev-dev mold just
```

(If `just` isn't packaged for your distro, install it with
`cargo install --locked just` after Rust is set up.)

### Rust

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

The toolchain version is pinned in `rust-toolchain.toml`; rustup installs it
the first time you build. After cloning the frontend repo (step 2), run
`just install-tools` to install the cargo extensions the lint, test and
MiSTer recipes use (`cargo-nextest`, `cargo-deny`, `cross`,
`slint-tr-extractor`, `cargo-about`).

### macOS / Windows

macOS is best-effort and not covered in CI. Windows is not tested; use WSL2.

## 2. Clone and build

```bash
git clone https://github.com/ZaparooProject/zaparoo-frontend.git
cd zaparoo-frontend
just build
```

The first build compiles Slint and the rest of the dependency tree.
Incremental builds are much faster after that.

## 3. Run against the mock Core

```bash
just run-dev
```

`run-dev` starts the mock Core when nothing is serving
`ws://127.0.0.1:27497/api/v0.1`, points the frontend at it through
`ZAPAROO_CORE_ENDPOINT`, and stops the mock again when the frontend exits,
logging to `output/mock-core-dev.log`. A mock (or a real Core) already on the
port is used as-is and left running. Arguments after `run-dev` go to the
frontend, for example `just run-dev --fullscreen`.

To run the mock on its own, use `just mock-core`. You should see:

```text
mock-core listening on ws://127.0.0.1:27497/api/v0.1
```

`27497` is offset from the real Core's `7497` so a real Core, or another Core
test instance, can run on the same machine without colliding with the mock. The
frontend still defaults to `7497` in production: `just run` reads
`~/.config/zaparoo/frontend.toml` as usual.

### Pick a different port

If something already uses `27497`, override it at startup:

```bash
MOCK_CORE_ADDR=127.0.0.1:9000 just mock-core
ZAPAROO_CORE_ENDPOINT=ws://127.0.0.1:9000/api/v0.1 just run-dev
```

`ZAPAROO_CORE_ENDPOINT` always wins over `~/.config/zaparoo/frontend.toml`.

### CRT preview

`just run-dev --crt` lays the UI out for native CRT output at the resolution of
the configured CRT video standard (352x240 for NTSC, 352x288 for PAL, 720x480
for 480i). `just snapshots` renders every screen at the CRT tier offline as
well.

### Window size and fullscreen

The frontend opens a 1280x720 window unless `frontend.toml` asks for something
else:

```toml
[video]
width = 1280
height = 800
fullscreen = true
```

`fullscreen` asks the windowing system for a fullscreen surface, which is what
a couch or handheld install wants. The compositor owns the size of a fullscreen
surface, so `width` and `height` are ignored while it is on, and every screen
re-solves its layout against whatever size arrives. `--fullscreen` turns it on
for one run and `--windowed` turns it off for one run, which beats editing the
config to test a layout. Neither applies on MiSTer or in CRT mode, where the
presenter owns the raster.

SteamOS is detected as its own runtime and needs none of this: it starts
fullscreen and picks the handheld page density on its own. Writing
`fullscreen = false` still puts it in a window, because an absent key and an
explicit `false` are not the same answer. `ZAPAROO_RUNTIME_OVERRIDE=steamos`
borrows those defaults on an ordinary desktop for testing.

### Gaming Mode

In a gamescope session the frontend claims the screen for itself, because
gamescope only draws a window carrying the properties Steam sets for what it
launched. It claims again when a game exits, so a launch returns to the
frontend rather than to the Steam library. Both are automatic and inert
outside a gamescope session; `xprop` must be on `PATH`.

Build the binary with `just x86-portable`. A build from an ordinary
`cargo build --release` links against the build host's glibc and will not
start on a Deck.

To install on a Deck, copy the repo (or just the binary and
`scripts/install-steamos.sh`) across and run the script there with Steam
running. It installs to `~/.local/bin/zaparoo-frontend`, writes a desktop
entry, and adds a **Zaparoo** shortcut to the Steam library; rerunning it
updates the binary in place without adding a second shortcut. Being
Steam-owned is the point: Steam only gives its own Steam Input layout, the
overlay and the Quick Access Menu to what it launched.

A Steam-owned frontend also offers itself to Core as its launch host. Core
cannot start an emulator inside a Steam session by itself, so in Gaming Mode
it normally asks Steam to run a second shortcut, **Zaparoo Runtime**, which
execs the emulator on its behalf. When the frontend is registered, Core hands
the command here instead and the game runs inside the session the frontend is
already in: one Steam card rather than two, and one Back press to leave. The
shortcut stays the fallback for a token scanned with no frontend running, for
Desktop Mode, and for a frontend Steam did not launch. Registration needs a
gamescope session, a Steam-started frontend, and a Core on this machine, plus
a Core build that accepts launch hosts. Registration has a socket of its own,
so an older Core has nothing to connect to and keeps using the shortcut with
no idea the frontend offered. Emulators
inherit the frontend shortcut's Steam Input layout while it hosts, where they
would otherwise inherit the Zaparoo Runtime shortcut's.

### Controllers

A connected gamepad drives the UI alongside the keyboard: d-pad or left
stick to move, the south face button to confirm, east to cancel, north for
the context menu, west for the view menu, and the shoulders to page. Steam
Input's virtual pad arrives the same way, so a Steam Deck needs no extra
setup.

The help bar follows whichever device you touched last, drawing keycaps for
the keyboard and the pad's own button glyphs otherwise. Settings > Controls
pins a button style when the autodetected one is wrong, and the
confirm/cancel and options/view swaps there apply to the pad only, never to
Enter and Escape. `[input.keyboard]` in `frontend.toml` remaps keyboard keys
only; pad buttons are not remappable.

## 4. Check the result

- The window opens on the Hub.
- Enter on a category opens the paged **systems grid**; PageUp and PageDown
  flip pages.
- Enter on a system opens the **games grid**. Enter on a game sends a `run`
  RPC to the mock, which logs the selected game's ZapScript; nothing is
  actually launched.
- Tab opens the context menu on the focused tile; Space opens the View menu.
- Escape backs out. On the Hub, Escape does nothing: Quit lives in the View
  menu.

## 5. Run tests and lints

Before you open a pull request:

```bash
just lint    # rustfmt, clippy (all feature sets), cargo-deny,
             # toolkit-free guard, translations, notices, logo parity
just test    # cargo nextest, desktop and MiSTer feature sets
```

Zero warnings is the bar. If lint complains about formatting or a fixable
clippy issue, `just fix` applies clippy's fixes and formats.

## Next steps

- [`docs/building.md`](building.md): the MiSTer ARM32 build, deployment, and
  cutting a release.
- [`docs/architecture.md`](architecture.md): module graph, Rust to Slint data
  flow, Runtime vs Platform distinction.
- [`docs/slint-gotchas.md`](slint-gotchas.md): software-renderer costs and
  motion rules.
- [`CONTRIBUTING.md`](../CONTRIBUTING.md): CLA flow, PR expectations,
  branch-protection rules.
