# Building

Day-to-day builds, lints, and tests go through the
[`justfile`](../justfile). `just --list` shows the full menu.
`CMakePresets.json` and `rust/.cargo/config.toml` are written for those
recipes. If you need raw `cmake` or `cargo`, double-check that the justfile does
not already cover the job.

## Requirements

### Desktop

- Qt 6.10+ (Quick, QuickControls2, Qml, LinguistTools)
- CMake 3.22+
- C++17 compiler (GCC 10+, Clang 12+, MSVC 2019+)
- Rust stable toolchain (`rustup install stable`)
- Ninja (required; pinned by `CMakePresets.json`)
- mold (used as linker on x86_64 Linux; pinned by `rust/.cargo/config.toml`)
- `just`
- Docker (used by `just lint`, `just fmt`, `just fix`, and the ARM32
  cross-build; see [Lints](#lints) for the rationale on running them in
  the published image rather than against host tools)

Run `just install-tools` once after cloning to install the cargo
extensions used by the host test recipes (currently `cargo-nextest`).
The lint image carries every other tool — clang-format, qmlformat,
cmake-format, qmllint, cargo-deny — so they do not need to be on the
host PATH.

Fedora / RHEL:
```bash
sudo dnf install qt6-qtdeclarative-devel qt6-qtquickcontrols2-devel \
    qt6-qttools-devel cmake ninja-build mold clang-tools-extra just
```

Ubuntu / Debian:
```bash
sudo apt install qt6-declarative-dev qt6-quick-controls2-dev \
    qt6-tools-dev qt6-l10n-tools cmake ninja-build mold \
    clang-tidy clang-format just
```

Install Rust via rustup, then run `just install-tools` after cloning the
frontend to install `cargo-nextest`:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
# After cloning the frontend repo:
just install-tools
```

If `just` isn't packaged for your distro, install it the same way:
`cargo install --locked just`.

### MiSTer ARM32 cross-build

- Docker with Buildx (Docker Desktop includes it)
- x86_64 Linux Docker platform (`linux/amd64`)
- ~5 GB disk space for the toolchain image

The toolchain Docker image provides the ARM build environment. Cargo still gets
its target and linker settings from `rust/.cargo/config.toml`; the desktop
`mold` linker setting lives there too. You should not need to edit Cargo config
by hand.

macOS users only need Docker Desktop for the ARM32 path. The build scripts
default to Docker platform `linux/amd64`, including on Apple Silicon Macs,
because the MiSTer ARM GCC toolchain is the official x86_64 Linux release from
Arm. Apple Silicon hosts therefore build through Docker's amd64 emulation while
the project itself is still pure ARM32 cross-compilation inside the container.

## Desktop builds

```bash
just build           # debug build (default)
just build-release   # release build
just build-dev       # dev preset (relwithdebinfo + extra checks)
just build-san       # ASan + UBSan
just run             # build then ./build/bin/frontend
```

The first build pulls and compiles the Rust and Qt dependencies. Incremental
builds are much faster after that.

For a faster local build without tests, configure with
`-DZAPAROO_BUILD_TESTS=OFF`:

```bash
cmake --preset desktop-debug -DZAPAROO_BUILD_TESTS=OFF
cmake --build --preset desktop-debug
```

## MiSTer ARM32 cross-build

The default path uses the official prebuilt toolchain image published by this
repository:

```bash
./scripts/build-arm32.sh
```

This pulls
`ghcr.io/zaparooproject/qt6-arm32-mister:<scripts/toolchain/VERSION>` if it is not
already cached locally, builds the application in Docker, and writes the MiSTer
binary to `output/frontend`. It does not require `just`, Qt, CMake, Rust, or
the ARM toolchain on the host.

If GHCR asks for authentication, authorize the GitHub CLI with package-read
scope and log Docker in:

```bash
gh auth refresh -h github.com -s read:packages
gh auth token | docker login ghcr.io -u <github-user> --password-stdin
```

If you need to rebuild the toolchain image locally, building Qt from source
takes about 45 minutes:

```bash
./scripts/build-toolchain.sh
```

This creates the local `zaparoo/qt6-arm32-mister:<version>` Docker image. The
tag comes from `scripts/toolchain/VERSION`.

Use that local toolchain image for the application build with:

```bash
USE_LOCAL_TOOLCHAIN=1 ./scripts/build-arm32.sh
```

Later builds usually take under a minute because Docker reuses the toolchain
and application layers.

`DOCKER_PLATFORM` defaults to `linux/amd64`. Override it only if you are using
a different compatible toolchain image:

```bash
DOCKER_PLATFORM=linux/amd64 ./scripts/build-arm32.sh
```

Check the ARM binary:

```bash
file output/frontend
# Should report: ELF 32-bit LSB executable, ARM, EABI5 ...
```

## Tests

```bash
just test            # ctest + cargo nextest
just test-qml        # only the Qt/QML tests
just test-rust       # only cargo nextest
just test-san        # ASan/UBSan suite
```

## Lints

All lint and format recipes run inside the published lint image
(`ghcr.io/zaparooproject/zaparoo-lint:<scripts/lint/VERSION>`). Host execution
is not exposed because clang-format, qmlformat, and cmake-format have
no per-project version pin (no rust-toolchain.toml equivalent), and
host distros routinely package different majors than the image. Routing
through Docker means host runs, Docker runs, and CI produce identical
output by construction.

The Docker-backed recipes default to `DOCKER_PLATFORM=linux/amd64`. That keeps
Apple Silicon macOS hosts working even when a lint-image tag has only been
published for amd64 so far; Docker Desktop runs it under emulation. If the
matching lint-image tag is available as native arm64 and you want that path,
override the platform explicitly:

```bash
DOCKER_PLATFORM=linux/arm64 just fmt
```

```bash
just lint            # everything (rust + cpp + qml)
just lint-cpp        # clang-format check + clang-tidy
just lint-qml        # qmllint
just lint-rust       # rustfmt check + clippy + cargo-deny
just fix             # clippy --fix, then all formatters
just fmt             # formatters only (cargo fmt + clang-format +
                     # qmlformat + cmake-format) on tracked files
just lint-docker     # alias for `just lint`
just fmt-docker      # alias for `just fmt`
just fix-docker      # alias for `just fix`
```

`just lint` is the zero-warnings gate before a PR. `just fix` runs
`cargo clippy --fix` first because its rewrites may not be pre-formatted;
the formatters are the cleanup pass.

The image carries Rust 1.90 + rustfmt + clippy + cargo-deny +
cargo-nextest, clang-format / clang-tidy 19, qmlformat / qmllint /
qmake from Qt 6.10.3 (installed via aqtinstall), cmake-format 0.6.13,
ccache, and mold. The same image runs in CI, so the version pin is
shared by construction.

The lint recipes configure CMake into `build-docker/`, not `build/`,
so they never stomp the artifacts from a host `just build`. First run
is slow (full Qt-linked build inside the container). Subsequent runs
reuse `build-docker/` via the bind mount and are fast.

When you bump `scripts/lint/VERSION` (because `Dockerfile.lint` changed), the
first CI run on the PR builds and pushes the new image to GHCR
automatically before the lint/test/build jobs start. After the PR
merges to main, `lint-image-build.yml` rebuilds the image multi-arch
so non-amd64 hosts have a native pull available too.

## Deploy desktop bundle

```bash
just build
./packaging/deploy-desktop.sh
./deploy/frontend/run.sh
```

The deploy script copies Qt shared libraries next to the binary. Qt must be on
your PATH (`qmake6` or `qmake` must be findable).

## Deploy to MiSTer

```bash
echo 'MISTER_IP=<your-mister-ip>' > .env
./scripts/deploy-mister.sh
```

To copy and restart an already-built `output/frontend` without rebuilding:

```bash
./scripts/deploy-mister.sh --skip-build
```

The MiSTer binary is self-contained. It sets `QT_QPA_PLATFORM=linuxfb` and
`QT_QUICK_BACKEND=software`, runs `vmode -r W H rgb32` using the configured
width and height (default `1920×1080`), and starts
`/media/fat/Scripts/zaparoo.sh -service start`. No wrapper script is needed.

User-editable config lives at `/media/fat/zaparoo/frontend.toml`.
Example:

```toml
[video]
width = 1280
height = 720

[logging]
debug = true
```

## Run on framebuffer (desktop headless)

Use this to reproduce the MiSTer rendering path on a desktop:

```bash
QT_QPA_PLATFORM=linuxfb QT_QUICK_BACKEND=software ./build/bin/frontend
```

## Underlying mechanics

Use these only when debugging the build itself or doing something the justfile
does not cover.

`just build` resolves to:

```bash
cmake --preset desktop-debug
cmake --build --preset desktop-debug
```

`just lint-cpp` resolves to `cmake --build build-docker --target lint`
inside the lint container — which runs clang-format (check only),
clang-tidy, and qmllint together against the same artifacts a fresh
configure would produce. The individual targets are:

```bash
cmake --build build --target format-check   # clang-format dry-run
cmake --build build --target tidy           # clang-tidy
cmake --build build --target all_qmllint    # QML linting
```

`just test` resolves to `ctest --preset desktop-debug` plus
`cargo nextest run --workspace`. Nextest needs the Rust workspace path, so the
justfile runs that command from `rust/`. Plain ctest works too:

```bash
ctest --test-dir build --output-on-failure
```
