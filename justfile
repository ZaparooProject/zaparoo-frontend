# Zaparoo Frontend dev commands.
# `just --list` for the full menu; docs/building.md covers a fresh machine.
#
# Two kinds of recipe. `build`, `run`, `run-dev`, `fmt`, `fix`, and the other
# desktop recipes run cargo on the host. `lint`, `test`, `arm32`, `release`,
# `x86-portable`, `tr-extract`, and `notices` run inside the pinned toolchain
# image (Dockerfile.toolchain) through scripts/toolchain.sh, so they behave the
# same on Linux, macOS, and CI and need only Docker and just on the host. Each
# delegates to a private `_` recipe holding the commands the container runs.

# Use sccache as the rustc wrapper when it's installed. sccache caches
# compiled crates across `target/` directories. Falls back to no wrapper if
# sccache isn't on PATH so contributors who haven't installed it still get
# working builds. The toolchain container clears it.
export RUSTC_WRAPPER := `command -v sccache || true`

# Homebrew installs rustup keg-only, outside PATH. Put its proxies first when
# present so macOS host builds honor rust-toolchain.toml rather than a
# Homebrew `rust` formula's compiler. No effect anywhere else.
export PATH := `for d in /opt/homebrew/opt/rustup/bin /usr/local/opt/rustup/bin; do if [ -x "$d/rustup" ]; then printf '%s:' "$d"; break; fi; done; printf '%s' "$PATH"`

default:
    @just --list

# --- build and run ---

# Desktop debug build
build:
    cd rust && cargo build -p frontend

# Desktop release build
build-release:
    cd rust && cargo build --release -p frontend

# Desktop run against the Core in frontend.toml (or ZAPAROO_CORE_ENDPOINT)
run *args:
    cd rust && cargo run -p frontend -- {{args}}

# Desktop run against the mock Core, started and stopped with the frontend
run-dev *args:
    ./scripts/run-dev.sh {{args}}

# The port is deliberately offset from the real Core's 7497 so dev never
# collides with a running Core. See docs/quickstart.md.
# Run a local mock Zaparoo Core (ws://127.0.0.1:27497/api/v0.1)
mock-core:
    cd rust && cargo run --bin mock-core

# `run` defaults to a headless software canvas; pass `run --visible` only when
# desktop-window behavior matters. `just slint-ui --help` lists screenshot, key,
# click, tree, and lifecycle commands.
# Start and control an isolated UI through Slint's embedded MCP server
[positional-arguments]
slint-ui *args:
    python3 scripts/slint-ui.py "$@"

# --- test ---

# Check the embedding seam with a host-only software renderer. Android's
# backend and renderer are supplied by the owning host repository.
hosted-check:
    ./scripts/toolchain.sh just _hosted-check

[private]
_hosted-check:
    cd rust && cargo clippy -p frontend --lib --no-default-features --features hosted,slint/renderer-software -- -D warnings
    cd rust && cargo nextest run -p frontend --lib --no-default-features --features hosted,slint/renderer-software

# Workspace tests plus the MiSTer feature set (toolchain image)
test:
    ./scripts/toolchain.sh just _test

# The MiSTer feature set has its own modules and tests (presenters, dual
# head); they only build under that feature.
_test:
    cd rust && cargo nextest run --workspace
    cd rust && cargo nextest run -p frontend --no-default-features --features mister

# --- lint and format ---

# Full lint gate, the same one CI runs (toolchain image)
lint:
    ./scripts/toolchain.sh just _lint

_lint:
    cd rust && cargo fmt --all --check
    cd rust && cargo clippy --workspace --all-targets -- -D warnings
    # The MiSTer feature set compiles different modules (the presenters, the
    # dual-head mirror). Lint it against the MiSTer target itself: 32-bit musl
    # types such as ioctl request codes differ from the host's.
    cd rust && cargo clippy -p frontend --no-default-features --features mister --all-targets --target armv7-unknown-linux-musleabihf -- -D warnings
    # The snapshot bin sits behind `required-features`, so `--all-targets`
    # alone never compiles it.
    cd rust && cargo clippy -p frontend --features snapshot --all-targets -- -D warnings
    cd rust && cargo deny check
    bash scripts/check-toolkit-free.sh
    bash scripts/check-translations.sh
    bash scripts/check-notices.sh
    python3 scripts/prepare-system-logos.py --check

# Format the Rust workspace
fmt:
    cd rust && cargo fmt --all

# clippy --fix runs first because its rewrites may not be pre-formatted.
# Apply clippy fixes, then format
fix:
    cd rust && cargo clippy --fix --workspace --all-targets --allow-dirty --allow-staged
    cd rust && cargo fmt --all

# --- translations and notices ---

# Regenerate the gettext template from the .slint files. Run after any @tr edit.
tr-extract:
    ./scripts/toolchain.sh bash scripts/extract-translations.sh

# Regenerate the binary's third-party notices (toolchain image)
notices:
    ./scripts/toolchain.sh bash scripts/generate-notices.sh

# --- embedded art ---

# Rasterizes resources/images/systems/*.svg and scales
# resources/images/systems-color/*.png into rust/frontend/assets/, the two
# logo sets build.rs embeds. Needs resvg, rsvg-convert, or inkscape on PATH
# plus Pillow. `just logos --color-only` skips the SVG step.
# Regenerate the embedded system logo sets from resources/images
logos *args:
    python3 scripts/prepare-system-logos.py {{args}}

# --- MiSTer and portable builds ---

# Static musl because the MiSTer rootfs glibc is older than any distribution
# worth building on. The binary lands in
# rust/target/docker/armv7-unknown-linux-musleabihf/release/frontend.
# Static ARM32 musl MiSTer build, Cortex-A9 tuning (toolchain image)
arm32:
    ./scripts/toolchain.sh just _arm32

_arm32:
    cd rust && RUSTFLAGS="-C target-cpu=cortex-a9" cargo build -p frontend --release --no-default-features --features mister --target armv7-unknown-linux-musleabihf

# scripts/toolchain.sh passes the provenance variables into the container, so
# About and the startup log report the commit, the build date, and
# `channel = "official"`. Use this, not `arm32`, for binaries you ship.
# Official MiSTer binary: `arm32` with build provenance
release:
    ZAPAROO_OFFICIAL_BUILD=1 ZAPAROO_BUILD_COMMIT="$(git rev-parse --short=7 HEAD)" ZAPAROO_BUILD_DATE="$(date -u +%Y-%m-%d)" just arm32

# MiSTer release bundle (output/release/zaparoo-frontend-<tag>.zip)
release-zip *args:
    ./scripts/package-mister-release.sh {{args}}

# A build on a modern host bakes in that host's glibc symbol versions and
# then refuses to start on a Steam Deck or any older distribution. The
# toolchain image's glibc 2.31 is old enough that the result runs everywhere
# we ship. The binary lands in
# rust/target/docker/x86_64-unknown-linux-gnu/release/frontend.
# Portable x86_64 desktop build for Steam Deck (toolchain image)
x86-portable:
    ./scripts/toolchain.sh just _x86-portable

_x86-portable:
    cd rust && cargo build -p frontend --release --target x86_64-unknown-linux-gnu

# Desktop tarball of the release cargo build with its license files.
package-desktop *args:
    ./scripts/package-desktop.sh {{args}}

# Software renderer, no window. Optional language argument, e.g. `de`.
# Render the main screens offline into output/snapshots/
snapshots *args:
    bash scripts/render-snapshots.sh {{args}}

# --- deploy ---

# Build and deploy to /media/fat/zaparoo/frontend on the MiSTer in .env
deploy-mister *args:
    ./scripts/deploy-mister.sh {{args}}

# --- toolchain ---

# Bump scripts/toolchain/VERSION with any Dockerfile.toolchain change; CI
# publishes the new tag.
# Build the toolchain image locally instead of pulling the published one
toolchain-build:
    USE_LOCAL_TOOLCHAIN=1 ZAPAROO_TOOLCHAIN_REBUILD=1 ./scripts/toolchain.sh true

# Open a shell inside the toolchain image at the current directory
toolchain-shell:
    ./scripts/toolchain.sh bash

# --- clean ---

# Remove output/ and the cargo target directory (container builds included)
clean:
    rm -rf output
    cd rust && cargo clean
