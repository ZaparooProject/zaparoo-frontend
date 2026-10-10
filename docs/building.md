# Building

Day-to-day builds, lints, and tests go through the
[`justfile`](../justfile). `just --list` shows the full menu. If you need raw
`cargo`, double-check that the justfile does not already cover the job; it
carries the feature sets, the toolchain image, and sccache.

The recipes come in two kinds. The desktop recipes (`build`, `run`, `run-dev`,
`fmt`, `fix`, `snapshots`) run cargo on your machine. `lint`, `test`, `arm32`,
`release`, `x86-portable`, `tr-extract`, and `notices` run inside the pinned
toolchain image, so they need only Docker and `just` and behave the same on
Linux, macOS, and CI. See [Toolchain image](#toolchain-image).

## Requirements

### Every platform

- Docker: Docker Engine on Linux, Docker Desktop on macOS
- `just`

That is enough for the lint, test, MiSTer, and portable builds.

### Desktop builds on Linux

- Rust via rustup. The toolchain version comes from `rust-toolchain.toml`
  (1.97.0 with rustfmt, clippy, and the MiSTer musl target); rustup installs it
  on first use.
- mold (the x86_64 Linux linker, set in `rust/.cargo/config.toml`)
- Slint's desktop system libraries: fontconfig, wayland, xkbcommon, and udev
  development packages
- Optional: sccache (the justfile uses it as `RUSTC_WRAPPER` when installed)

Fedora / RHEL:
```bash
sudo dnf install fontconfig-devel wayland-devel libxkbcommon-devel \
    systemd-devel mold just
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

Ubuntu / Debian:
```bash
sudo apt install libfontconfig1-dev libwayland-dev libxkbcommon-dev \
    libudev-dev mold just
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

If `just` isn't packaged for your distro, install it with
`cargo install --locked just`.

### Desktop builds on macOS

Install rustup, `just`, and Docker Desktop with Homebrew:

```bash
brew install rustup just
brew install --cask docker
```

Use rustup rather than Homebrew's `rust` formula: rustup honors
`rust-toolchain.toml`, so the build uses the pinned compiler. Homebrew installs
rustup keg-only, outside `PATH`; the justfile finds it in
`/opt/homebrew/opt/rustup/bin` (or `/usr/local/opt/rustup/bin` on Intel) and
puts it first for its recipes, so `just build`, `just run`, and `just run-dev`
work without shell setup. Run `rustup` itself through that path, or add it to
your shell's `PATH`, if you want the pinned toolchain outside `just`.

The toolchain image is linux/amd64. On Apple Silicon, Docker Desktop runs it
under emulation; turn on "Use Rosetta for x86_64/amd64 emulation" in Docker
Desktop's settings, which is far faster than QEMU.

## The private Update module

The Hub's Update tile and its screen come from a private repository,
`theypsilon/zaparoo-update`. Public builds use a stand-in and have no Update
tile; official builds compile the real module in. Architecture:
`docs/architecture.md` ("Update module").

To build with it (needs read access to that repository):

```sh
git clone git@github.com:theypsilon/zaparoo-update.git rust/private/zaparoo-update
just update-module    # cargo does not notice the checkout appearing
just build
```

`rust/private/` is gitignored. Run `just update-module` again after removing
the checkout to go back to the stand-in. The container recipes (`just lint`,
`just test`, `just arm32`) see the same directory, so they build the same
variant. The private code is linted and tested under this workspace's rules
through the stub crate, on top of the private repository's own checks.

The release workflow clones it with the `ZAPAROO_UPDATE_DEPLOY_KEY` secret
(a read-only deploy key) at `inputs.update_ref`, then the
`ZAPAROO_UPDATE_REF` repository variable, then `main`. A release that would
be published fails when the key is missing; a dispatch that uploads nothing
only warns and builds the stand-in. The stand-in ships
`LICENSES/zaparoo-update-NOTICE.txt` in every bundle either way.

## Toolchain image

[`Dockerfile.toolchain`](../Dockerfile.toolchain) defines the build
environment: Rust 1.97.0 with rustfmt, clippy, and the MiSTer target; the
static-musl ARM32 cross compiler (taken from the cross-rs 0.2.5 image); the
desktop development libraries; and `just`, `cargo-nextest`, `cargo-deny`,
`cargo-about`, and `slint-tr-extractor` at pinned, checksum-verified versions.
It is based on Ubuntu 20.04 on purpose: its glibc 2.31 is the floor the
portable x86_64 build links against.

[`scripts/toolchain.sh`](../scripts/toolchain.sh) runs a command inside it with
the repository mounted and your working directory mapped, as your own user.
The containerized recipes delegate to private `_` recipes through it
(`just lint` runs `scripts/toolchain.sh just _lint`). By default it uses
`ghcr.io/zaparooproject/zaparoo-frontend-toolchain:<VERSION>`, where
`<VERSION>` is [`scripts/toolchain/VERSION`](../scripts/toolchain/VERSION),
pulling it on first use. When the pull fails it builds the image locally
instead.

```bash
just toolchain-build   # build the image locally and use it (USE_LOCAL_TOOLCHAIN=1)
just toolchain-shell   # a shell inside the image at the current directory
```

Container builds keep their own target directory (`rust/target/docker`) and
cargo cache (`rust/target/docker-cargo`), so switching between `just build` and
`just lint` never invalidates either build.

To change the image, edit `Dockerfile.toolchain` **and** bump
`scripts/toolchain/VERSION` in the same PR. CI's first job
([`toolchain-image.yml`](../.github/workflows/toolchain-image.yml)) publishes
any tag that does not exist yet, and fails a PR that changes the Dockerfile
without bumping the version. A PR from a fork cannot publish, so a maintainer
lands toolchain changes from a branch in the main repository.

## Desktop builds

```bash
just build           # debug build
just build-release   # release build
just run             # cargo run against the Core in frontend.toml
just run-dev         # run against the mock Core, started and stopped for you
```

The binary lands in `rust/target/debug/frontend` (or `release/`). Fonts, art
and translations are embedded, so the binary runs from anywhere.

### Portable x86_64 build (Steam Deck)

A build on a modern host bakes in that host's glibc symbol versions and then
refuses to start on a Steam Deck or an older distribution. `just x86-portable`
builds inside the toolchain image (glibc 2.31) instead, producing
`rust/target/docker/x86_64-unknown-linux-gnu/release/frontend`.
`scripts/install-steamos.sh` installs that binary on a Deck and adds it to
Steam.

### Desktop tarball

`just package-desktop [tag]` builds the release binary and writes
`output/release/zaparoo-frontend-<tag>-<os>-<arch>.tar.gz` with the binary,
`COPYING`, the asset attributions, and the crate notices.

## MiSTer ARM32 build

```bash
just arm32
```

This runs `cargo build -p frontend --release --no-default-features --features
mister --target armv7-unknown-linux-musleabihf` with Cortex-A9 tuning inside
the toolchain image and produces a static binary at
`rust/target/docker/armv7-unknown-linux-musleabihf/release/frontend`:

```bash
file rust/target/docker/armv7-unknown-linux-musleabihf/release/frontend
# Should report: ELF 32-bit LSB executable, ARM, EABI5 ... statically linked
```

The build is static musl because the MiSTer image ships an old glibc; a static
binary carries no glibc symbol versions at all. The `mister` feature swaps
the winit backend for a custom `slint::platform` with the software renderer and
the fb0, DDR and vblank-latch presenters.

`just release` is `just arm32` with build provenance: it passes
`ZAPAROO_OFFICIAL_BUILD=1`, the short commit, and the UTC date into the
container, so About and the startup log report `channel = "official"`. Use it
for binaries you ship.

### Framebuffer mapping on newer MiSTer kernels

The `/dev/fb0` presenter first tries ordinary framebuffer mmap. If it fails
with `ENODEV` on a framebuffer character device, it maps the physical range
reported by `FBIOGET_FSCREENINFO` through `/dev/mem`. This covers kernels whose
`MiSTer_fb` driver lacks `fb_mmap`; working drivers keep the native path. No
physical address is hardcoded, and other mmap errors do not trigger the
fallback.

The fallback requires existing permission to open `/dev/mem`; it does not
change permissions. Startup logs report activation or the underlying error.
Set `ZAPAROO_FB_FALLBACK=off` to disable it for diagnosis. Slint retains its
cached render buffer and dirty-row copies in either case. Framebuffer clears
and pixel stores use aligned volatile writes: ARM Device-memory mappings can
fault on unaligned accesses emitted by libc `memset` or `memcpy`. DDR and latch
presenters are unchanged because they do not mmap the fbdev surface.

Host tests cover fallback selection and mapping bounds without opening either
hardware device; a device boot is still needed to verify the affected kernel.

### Native MiSTer CRT compatibility

Slint publishes extended DDR magic `0x5A51` (signed vertical bits 7:2, mode bits
1:0). Menu must support this alongside legacy `0x5A50`; older Main/Qt writers
remain compatible with the dual decoder. Native geometry belongs to the DDR
writer, independently of fb0. No persisted-state reset is needed.

After replacing `menu_zaparoo.rbf`, explicitly load it through Main's normal
`load_core` path. Restarting Main with an RBF argument does not program FPGA.
Keep the original RBF for rollback, which also requires an explicit load.

### Optional MiSTer HDMI scanout (local testing)

A coordinated Main/Menu/module integration can replace fb0 copies with
write-combined RGB565 slots and vblank-latched flips. It is not enabled by
`--latch` alone: Main must acknowledge the private v2 slot/proxy handshake
**after** frontend video probing. Main executes complete UIO transactions and
remains the sole FPGA-bus writer, so OSD drawing and video queries stay live.
Frontend never maps the FPGA registers. Missing components or mismatched old/new
handshakes fall back to ordinary fb0; `--no-latch` opts out. Managed display
restarts go through Main for a fresh lease.

Eligibility is an exact qualified kernel build, with `/dev/zaparoo-scanout`
ABI v2. Main selects
`/media/fat/zaparoo/modules/<release>/<kernel-build-id>/zaparoo_scanout.ko`
using the running kernel's GNU build ID from `/sys/kernel/notes`. It verifies
the module SHA-256 before loading and its loaded build ID before granting access.
The old flat release-only module path no longer enables scanout. Older/unknown
kernels and HDMI under Direct Video retain existing paths. Never force-load the
prototype's 5.15 module, replace `mem_wc`/MagiK modules, or claim independent
renderers can safely run concurrently.

On the native CRT path Main answers the same request with a native grant
instead: no bus proxy, only permission to map the Menu core's native video
window through the module. The window lies outside the kernel's RAM, so
`/dev/mem` can only map it as uncached device memory, where writing one PAL
frame takes about 27 ms. The module maps the frame slots write-combined (about
1 ms) and keeps the two control words on an uncached page so a publish never
queues behind its pixels. The DDR presenter copies only the rows that changed
since each slot was last filled, and waits on the raster's own vertical sync
through the module instead of sleeping. Without a grant it maps the window
through `/dev/mem` and paces by sleep, as before; the changed-row copy and
whole-word stores apply there too. In dual head the HDMI side stays on fb0.

The `frame profile` log line reports `copy_avg_us` beside `avg_us` and judges
overruns against the presenter's own refresh period (20 ms in PAL).

Automatic keeps **960x540 rendering into 1920x1080 HDMI**. Physical output timing
and source geometry remain separate. Default rendering is fixed at the resolved
Automatic/explicit size; `--adaptive-render` is experimental and opt-in.
Desktop remains FemtoVG. GPL module/RTL implementation lives only in the Menu
fork; frontend uses the public protocol and namespaced UAPI.

Build the frontend with `just arm32` and run `just lint` and `just test`. Main
requires its ARM cross-build. Menu's `README.md` and
`kernel/scanout-slots/README.md` cover RTL tests, module provenance and exact
kernel qualification. Local gates do not establish hardware smoothness or
lifecycle acceptance.

## Tests

```bash
just test
```

Inside the toolchain image, this runs `cargo nextest run --workspace` and then
`cargo nextest run -p frontend --no-default-features --features mister`: the
presenter, dual-head and other MiSTer-only modules only build under that
feature. Render tests draw through Slint's software renderer offline; nothing
opens a window.

`just snapshots` renders the main screens (the curated list in
`scripts/render-snapshots.sh`: each root screen, the modals, and the CRT
variants) at 540p, 720p, 1080p and 352x240 CRT into `output/snapshots/`,
optionally through a bundled language (`just snapshots de`).

## Lints

```bash
just lint
```

The gate, which CI runs as-is inside the toolchain image:

- `cargo fmt --all --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- the same clippy for the `mister` feature set, against the MiSTer target
  (`armv7-unknown-linux-musleabihf`), since 32-bit musl types such as ioctl
  request codes differ from the host's
- the same clippy for the `snapshot` feature set, which sits behind
  `required-features` and so is never built by `--all-targets` alone
- `cargo deny check` (advisories, licenses, bans, sources; `rust/deny.toml`)
- `scripts/check-toolkit-free.sh`: `rust/zaparoo-app` must not depend on Slint
  or any other toolkit
- `scripts/check-translations.sh`: the committed `frontend.pot` must match a
  fresh extraction from the `.slint` files
- `scripts/check-notices.sh`: `THIRD-PARTY-NOTICES.txt` must match the current
  dependency set; `just notices` regenerates it
- `python3 scripts/prepare-system-logos.py --check`: the embedded logo sets
  under `rust/frontend/assets/` must match the sources under
  `resources/images/systems*/`; `just logos` regenerates them

`just fmt` formats; `just fix` applies clippy's fixes, then formats.

## Build caching

The build is fast only because several caches cooperate. Each one has a
coupling that is easy to break silently; this section is the inventory.
AGENTS.md points here; keep both in sync when any of these change.

### Build provenance lives in `rust/build-info`, nowhere else

The commit hash / build date / channel shown in About and the startup
log are baked by `rust/build-info/build.rs`, a deliberate leaf crate.
It is the only build script allowed to declare
`rerun-if-changed=../../.git/HEAD` (and `refs/heads`): those triggers
fire on every commit, rebase, and branch switch, so whatever build
script carries them re-runs on every commit. In `rust/build-info` that
re-run costs milliseconds. In `rust/frontend/build.rs`, which runs the Slint
compiler over every `.slint` file and generates the logo table, it would cost
a rebuild of the frontend crate after every commit.

Do not add `.git/` rerun triggers or `ZAPAROO_BUILD_*` provenance env
baking to `rust/frontend/build.rs`. New provenance fields go in
`rust/build-info` and are consumed as `zaparoo_build_info::*` consts.

`just release` sets `ZAPAROO_BUILD_COMMIT`, `ZAPAROO_BUILD_DATE` and
`ZAPAROO_OFFICIAL_BUILD`, and `scripts/toolchain.sh` passes them into the
toolchain container. Without them the build script asks `git` and falls back to
`unknown`/`dev`.

### The Rust toolchain pin

`rust-toolchain.toml` sits at the **repo root** (not in `rust/`) so rustup
resolves it for every cargo invocation in the tree. Bumping it also means
updating `RUST_VERSION` in `Dockerfile.toolchain` (and bumping
`scripts/toolchain/VERSION`) and `rust-version` in `rust/Cargo.toml`.

### Slint version couplings

`slint` and `slint-build` are pinned exactly in `rust/frontend/Cargo.toml`.
`slint-tr-extractor` must match: its version is `SLINT_TR_EXTRACTOR_VERSION` in
`Dockerfile.toolchain` (bump `scripts/toolchain/VERSION` with it).
`fontique` and `resvg` in `rust/frontend/Cargo.toml` must stay on the versions
Slint resolves. The
Slint license exceptions in `rust/deny.toml` list crates by name, so a Slint
bump can add or remove one. Regenerate `just notices` afterward.

### Compiler caches

For host builds the justfile exports `RUSTC_WRAPPER` as sccache when it is on
`PATH`, which shares compiled crates across clean builds. The toolchain
container clears it and relies on its own target directory
(`rust/target/docker`) instead. CI caches that directory and the container's
cargo cache per job with `actions/cache`, keyed on the toolchain version and
`rust/Cargo.lock`.

## Deploy to MiSTer

Create `.env` in the repo root with the MiSTer address:

```bash
echo 'MISTER_IP=192.168.1.100' > .env
# optional, for password auth through sshpass:
echo 'MISTER_PW=<password>' >> .env
```

Then:

```bash
just deploy-mister              # build and deploy
just deploy-mister --skip-build # deploy the last build
```

The script builds with `just arm32`, uploads to
`/media/fat/zaparoo/frontend.new`, verifies the size, keeps the previous binary
as `frontend.bak`, syncs, clears `/tmp/zaparoo/frontend.log`, and SIGKILLs the
running frontend. `/media/fat/zaparoo/MiSTer_Zaparoo` respawns it with the new
binary.
SIGKILL is deliberate: a clean exit counts as an escape and the wrapper will
not respawn. SIGKILL counts as a crash toward the wrapper's three-strike limit,
so after three deploys without a clean exit, `killall MiSTer_Zaparoo` to reset.

## Cutting a release

A MiSTer release bundles three independently versioned components: the
**frontend** binary (this repo), the **`MiSTer_Zaparoo`** host wrapper
(`ZaparooProject/Main_MiSTer`), and the **`menu_zaparoo.rbf`** menu core
(`ZaparooProject/Menu_MiSTer`). The analog video path depends on all three
matching. The release workflow resolves the host wrapper and menu core to their
latest published releases automatically, so a normal release is a version bump
and a tag.

1. **Bump the version.** Update `version` in `rust/Cargo.toml`
   (`[workspace.package]`), then regenerate the lockfile with
   `cargo update --workspace` from `rust/`. That is the only place to edit;
   About and the log upload read `CARGO_PKG_VERSION`.
2. **Refresh the notices** if dependencies changed: `just notices`, and commit
   `rust/frontend/LICENSES/THIRD-PARTY-NOTICES.txt`. Packaging fails when the
   file is missing.
3. **Tag and publish.** Push a `vX.Y.Z` tag (or run the `Build release ZIP`
   workflow with `upload` enabled). The workflow checks the tag against the
   workspace version, builds with `just release`, packages the bundle, uploads
   the GitHub release, and attests the ZIP.
4. **Update the downloader database.** In `ZaparooProject/Zaparoo_MiSTer`,
   update `db.json` (`archives.zaparoo_frontend` url/hash/size and the
   `summary_inline.files` hashes), then regenerate its distributed zip(s).
5. **Verify** on a clean unit installed via the downloader.

To bundle a specific menu or host build instead of latest, set the workflow's
`menu_tag` input (or `MENU_MISTER_TAG` / `MAIN_MISTER_TAG` when running
`scripts/package-mister-release.sh` directly).

The bundle layout: `zaparoo/frontend`, `zaparoo/MiSTer_Zaparoo`,
`zaparoo/menu_zaparoo.rbf`, `LICENSES/` (asset attributions plus
`THIRD-PARTY-NOTICES.txt`), `README.txt`, and `COPYING`.

### Bundling scanout modules

Update All 2.11 offers a Linux-pinned default distribution and Edge Linux. Keep
one frontend installation across both; bundle each device-qualified kernel
build under its own release/build-ID directory. Do not infer compatibility from
the channel name, `uname -r` alone, or an updated kernel image awaiting reboot.
Unknown builds retain fb0 without changing the user's Linux update policy.

Menu CI produces `zaparoo-scanout.zip` with profiles, module objects, build
provenance and matching source. Building that ZIP does not qualify a stock
kernel: the exact running-image identity and matched Main/Menu/frontend stack
need hardware testing, including kill/relaunch, ownership handoff and fallback.
The default Menu build reproduces the pinned official September 12, 2026 stock
image. Packaging requires byte equality outside its GNU build-ID descriptor,
matching config/symbol-table hashes, and records both stock and reproduced IDs.
See Menu's `kernel/stock-20260912.json` and `kernel/scanout-slots/README.md`.
The older prototype build remains separate. This does not qualify future pinned
or Edge images or waive hardware checks for a newly produced bundle.

After qualification, attach the ZIP to the matching Menu release and use
`ZAPAROO_INCLUDE_SCANOUT=1 just release-zip vX.Y.Z`. The release workflow has
an `include_scanout` input; set repository variable `ZAPAROO_INCLUDE_SCANOUT=1`
only once automatically selected Menu releases carry qualified bundles. A
requested missing or invalid bundle fails the release. For local packaging,
`ZAPAROO_SCANOUT_BUNDLE=/absolute/path/zaparoo-scanout.zip` supplies the artifact.
Without either setting, packaging continues to ship the fb0 baseline.
Combine separately qualified builds with
`python3 scripts/scanout_bundle.py pinned.zip edge.zip --merge zaparoo-scanout.zip`;
the validator rejects duplicate profiles, including builds sharing a release
string and build ID. Distinct build IDs under the same release coexist.

`just test-scanout-bundle` checks bundle validation without a device.
Main's identity tests cross-compile with its ARM GNU toolchain; target execution
and physical scanout qualification are separate gates. No MagiK kernel/RTL code
or licensing changes are imported by this profile and packaging integration.
