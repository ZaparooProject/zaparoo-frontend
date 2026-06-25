# Zaparoo Frontend dev commands.
# `just --list` for the full menu.

# Use sccache as the rustc wrapper when it's installed. sccache caches
# compiled crates across `target/` directories — biggest win when round-
# tripping between desktop and arm32 targets, but also speeds up CI and
# any clean build. Falls back to no wrapper if sccache isn't on PATH so
# contributors who haven't installed it still get working builds.
export RUSTC_WRAPPER := `command -v sccache || true`

default:
    @just --list

# --- build ---
# Configure runs only when the build dir has no build.ninja (the guard
# is build.ninja, not CMakeCache.txt, because a failed configure leaves
# the cache behind but only a successful generate writes build.ninja).
# Ninja re-runs cmake itself when CMakeLists/*.cmake change, so the
# explicit configure is pure overhead on every other invocation. The one
# case ninja cannot see is an edited cacheVariable in CMakePresets.json —
# after changing presets, run `cmake --preset <name>` once by hand (or
# `just clean`).
build:
    test -f build/build.ninja || cmake --preset desktop-debug
    cmake --build --preset desktop-debug

build-release:
    test -f build-release/build.ninja || cmake --preset desktop-release
    cmake --build --preset desktop-release

# Release build with provenance markers baked in. Sets
# ZAPAROO_OFFICIAL_BUILD=1 so the frontend reports
# `channel = "official"` in About / License and the startup log,
# distinguishing distributed packages from local dev builds. Use this
# (not `build-release`) when producing binaries you intend to ship.
# Produces both shippable artifacts: desktop release in build-release/bin
# and the MiSTer ARM32 binary in output/frontend.
release:
    ZAPAROO_OFFICIAL_BUILD=1 cmake --preset desktop-release
    ZAPAROO_OFFICIAL_BUILD=1 cmake --build --preset desktop-release
    ZAPAROO_OFFICIAL_BUILD=1 ./scripts/build-arm32.sh

release-zip *args:
    ./scripts/package-mister-release.sh {{args}}

build-dev:
    test -f build-dev/build.ninja || cmake --preset desktop-dev
    cmake --build --preset desktop-dev

build-san:
    test -f build-san/build.ninja || cmake --preset desktop-sanitized
    cmake --build --preset desktop-sanitized

arm32:
    ./scripts/build-arm32.sh

# --- run ---
run *args: build
    ./build/bin/frontend {{args}}

run-dev *args: build-dev
    ZAPAROO_CORE_ENDPOINT=ws://127.0.0.1:27497/api/v0.1 ./build-dev/bin/frontend {{args}}

# Run a local mock Zaparoo Core (ws://127.0.0.1:27497/api/v0.1).
# Deliberately offset from the real Core's 7497 so dev never collides
# with a running Core. `just run-dev` automatically points the frontend
# here via ZAPAROO_CORE_ENDPOINT.
# See docs/quickstart.md.
mock-core:
    cd rust && cargo run --bin mock-core

# --- test ---
test: build
    ctest --preset desktop-debug
    cd rust && cargo nextest run --workspace

test-qml: build
    ctest --preset desktop-debug -R ui

test-rust:
    cd rust && cargo nextest run --workspace

test-san: build-san
    ctest --preset desktop-sanitized

# --- lint and format ---
# All lint and format recipes run inside the published lint image.
# Host execution drifts because clang-format, qmlformat, and cmake-format
# are not version-pinnable through any host tool ecosystem (no
# rust-toolchain.toml equivalent), and Qt's qmllint changes its rule set
# between minor releases. The image is the single source of truth for
# tool versions, shared with CI by construction. `scripts/lint/VERSION` drives
# the image tag — see `Dockerfile.lint` for what is pinned.
#
# `_LINT_IMAGE` is read from `scripts/lint/VERSION` so a single source of truth
# drives both the publish workflow and local pulls.
_LINT_IMAGE := "ghcr.io/zaparooproject/zaparoo-lint:" + trim(`cat scripts/lint/VERSION`)
# Docker Desktop on Apple Silicon can run the published amd64 lint image under
# emulation, while x86_64 hosts run it natively. Default to linux/amd64 so a
# tag that has not been rebuilt multi-arch yet still works everywhere we care
# about today. Contributors who have a native arm64 lint image available can
# opt in with `DOCKER_PLATFORM=linux/arm64 just …`.
_LINT_PLATFORM := env_var_or_default("DOCKER_PLATFORM", "linux/amd64")

# Host-side caches survive the ephemeral container: the cargo registry
# (index + crate sources), cargo-deny's advisory DB clone, and ccache's
# object cache otherwise re-download / recompile on every run. They live
# under gitignored .docker-cache/ as plain host dirs (not named volumes)
# so the `-u $(id -u)` user can write to them. Deliberately NOT mounted:
# /usr/local/rustup — shadowing it would hide the image's baked
# toolchains and break the cmake-driven _build-in-image path.
_lint *cmd:
    mkdir -p .docker-cache/cargo-registry .docker-cache/advisory-dbs .docker-cache/ccache
    docker run --rm \
        --platform {{_LINT_PLATFORM}} \
        -v "$PWD":/workdir \
        -v "$PWD/.docker-cache/cargo-registry":/usr/local/cargo/registry \
        -v "$PWD/.docker-cache/advisory-dbs":/usr/local/cargo/advisory-dbs \
        -e CCACHE_DIR=/workdir/.docker-cache/ccache \
        -u "$(id -u):$(id -g)" \
        {{_LINT_IMAGE}} \
        {{cmd}}

# Container-internal: configure + build using the desktop-docker-debug
# preset so cmake artifacts land in build-docker/ instead of stomping the
# host's build/ directory. Underscore-prefixed recipes are private
# (hidden from `just --list`).
_build-in-image:
    test -f build-docker/build.ninja || cmake --preset desktop-docker-debug
    cmake --build --preset desktop-docker-debug

# Container-internal: cmake `lint` target (clang-format dry-run +
# clang-tidy + all_qmllint).
_lint-cpp-target: _build-in-image
    cmake --build build-docker --target lint

# Container-internal: only the qmllint subset.
_lint-qml-target: _build-in-image
    cmake --build build-docker --target all_qmllint

# Container-internal: refresh Qt Linguist catalogs and fail if checked-in
# translations are stale. `lupdate` updates source locations as well as strings,
# so this catches missing line-number/catalog churn from QML/C++ edits.
_lint-translations-internal:
    test -f build-docker/build.ninja || cmake --preset desktop-docker-debug
    bash scripts/check-translations-updated.sh build-docker

# Container-internal: the rust lint surface (fmt --check + clippy + deny).
_lint-rust-internal:
    cd rust && cargo fmt --all --check
    cd rust && cargo clippy --workspace --all-targets -- -D warnings
    cd rust && cargo deny check

_lint-all-internal: _lint-rust-internal _lint-cpp-target _lint-translations-internal

# Container-internal: format-and-autofix surface. xargs -r skips the
# invocation when the file list is empty.
_fmt-internal:
    cd rust && cargo fmt --all
    git ls-files '*.cpp' '*.h' '*.hpp' '*.cc' | xargs -r clang-format -i
    git ls-files '*.qml' | xargs -r qmlformat --inplace
    git ls-files 'CMakeLists.txt' '*.cmake' | xargs -r cmake-format -i

# Container-internal: autofix everything CI would reject. clippy --fix
# runs first because its rewrites may not be pre-formatted; the
# formatters are the cleanup pass.
_fix-internal:
    cd rust && cargo clippy --fix --workspace --all-targets --allow-dirty --allow-staged
    just _fmt-internal

# Full lint gate (rust + cpp + qml). Matches CI exactly.
lint:
    just _lint just _lint-all-internal

lint-docker: lint

lint-rust:
    just _lint just _lint-rust-internal

lint-cpp:
    just _lint just _lint-cpp-target

lint-qml:
    just _lint just _lint-qml-target

lint-translations:
    just _lint just _lint-translations-internal

fmt:
    just _lint just _fmt-internal

fmt-docker: fmt

fix:
    just _lint just _fix-internal

fix-docker: fix

# Install the host-only cargo extensions used by `just test*`.
install-tools:
    cargo install --locked cargo-nextest

# --- deploy ---
deploy-mister *args:
    ./scripts/deploy-mister.sh {{args}}

# --- clean ---
clean:
    rm -rf build build-release build-dev build-dev-no-update build-san build-docker output
    cd rust && cargo clean
