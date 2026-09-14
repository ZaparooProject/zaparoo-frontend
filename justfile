# Zaparoo Frontend dev commands.
# `just --list` for the full menu. Every recipe runs on the host; see
# docs/building.md for the packages a fresh machine needs.

# Use sccache as the rustc wrapper when it's installed. sccache caches
# compiled crates across `target/` directories. Biggest win when round-
# tripping between desktop and MiSTer feature sets, but also speeds up any
# clean build. Falls back to no wrapper if sccache isn't on PATH so
# contributors who haven't installed it still get working builds.
export RUSTC_WRAPPER := `command -v sccache || true`

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

# The MiSTer feature set has its own modules and tests (presenters, dual
# head); they only build under that feature.
# Workspace tests plus the MiSTer feature set
test:
    cd rust && cargo nextest run --workspace
    cd rust && cargo nextest run -p frontend --no-default-features --features mister

# --- lint and format ---

# Full lint gate. Matches CI.
lint:
    cd rust && cargo fmt --all --check
    cd rust && cargo clippy --workspace --all-targets -- -D warnings
    # The MiSTer feature set compiles different modules (the presenters,
    # the dual-head mirror); lint it too or their tests rot unseen.
    cd rust && cargo clippy -p frontend --no-default-features --features mister --all-targets -- -D warnings
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
    bash scripts/extract-translations.sh

# Regenerate the binary's third-party notices
notices:
    bash scripts/generate-notices.sh

# --- embedded art ---

# Rasterizes resources/images/systems/*.svg and scales
# resources/images/systems-color/*.png into rust/frontend/assets/, the two
# logo sets build.rs embeds. Needs resvg, rsvg-convert, or inkscape on PATH
# plus Pillow. `just logos --color-only` skips the SVG step.
# Regenerate the embedded system logo sets from resources/images
logos *args:
    python3 scripts/prepare-system-logos.py {{args}}

# --- MiSTer and portable builds ---

# Static musl because the MiSTer rootfs glibc is older than cross's
# gnueabihf image.
# Static ARM32 musl MiSTer build via `cross` (Cortex-A9 tuning)
arm32:
    cd rust && ZAPAROO_RESOURCES_DIR="$PWD/../resources" RUSTFLAGS="-C target-cpu=cortex-a9" cross build -p frontend --release --no-default-features --features mister --target armv7-unknown-linux-musleabihf

# Build provenance is passed into the cross container (rust/Cross.toml), so
# About and the startup log report the commit, the build date, and
# `channel = "official"`. Use this, not `arm32`, for binaries you ship.
# Official MiSTer binary: `arm32` with build provenance
release:
    ZAPAROO_OFFICIAL_BUILD=1 ZAPAROO_BUILD_COMMIT="$(git rev-parse --short=7 HEAD)" ZAPAROO_BUILD_DATE="$(date -u +%Y-%m-%d)" just arm32

# MiSTer release bundle (output/release/zaparoo-frontend-<tag>.zip)
release-zip *args:
    ./scripts/package-mister-release.sh {{args}}

# A build on a modern host bakes in that host's glibc symbol versions and
# then refuses to start on a Steam Deck or any older distribution; cross's
# image is old enough that the result runs everywhere we ship. RUSTFLAGS is
# overridden because `mold` is a host convenience the container lacks.
# Portable x86_64 desktop build via `cross` (needed for Steam Deck).
x86-portable:
    cd rust && ZAPAROO_RESOURCES_DIR="$PWD/../resources" RUSTFLAGS=" " cross build -p frontend --release --target x86_64-unknown-linux-gnu

# Desktop tarball of the release cargo build with its license files.
package-desktop *args:
    ./scripts/package-desktop.sh {{args}}

# Software renderer, no window. Optional language argument, e.g. `de`.
# Render every screen offline into output/snapshots/
snapshots *args:
    bash scripts/render-snapshots.sh {{args}}

# --- deploy ---

# Build and deploy to /media/fat/zaparoo/frontend on the MiSTer in .env
deploy-mister *args:
    ./scripts/deploy-mister.sh {{args}}

# --- setup ---

# Install the host-only cargo extensions the recipes use. Versions match
# the pins in .github/workflows/ci.yml and release.yml.
install-tools:
    cargo install --locked cargo-nextest
    cargo install --locked --version 0.19.4 cargo-deny
    cargo install --locked --version 0.2.5 cross
    cargo install --locked --version 1.17.1 slint-tr-extractor
    cargo install --locked --version 0.9.2 cargo-about --features cli

# --- clean ---

# Remove output/ and the cargo target directory
clean:
    rm -rf output
    cd rust && cargo clean
