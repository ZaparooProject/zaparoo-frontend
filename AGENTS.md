# Zaparoo Frontend Agent Guide

Zaparoo Frontend is a Rust and Slint frontend for Zaparoo Core. It runs on
desktop Linux and on MiSTer FPGA (ARM32, Linux framebuffer, software
rendering). The MiSTer target is the hard constraint: assume no GPU, process
kills without notice, and a small ARM CPU.

Keep this file focused on commands, traps, and rules that are hard to infer
from the tree. Use the docs for longer explanations.

## Commands

Run every workflow from the repo root with `just`. Do not `cd rust/` and run
raw cargo as the default path; the justfile carries the expected environment
(feature sets, the `cross` resource mount, sccache).

| Task | Command |
|---|---|
| Desktop build | `just build` (`just build-release` for the release profile) |
| Desktop run | `just run` |
| Dev run against mock Core | `just run-dev` (starts and stops the mock itself) |
| Mock Core only | `just mock-core` |
| Full test gate | `just test` (workspace plus the `mister` feature set) |
| Full lint gate | `just lint` (fmt, clippy for desktop, mister, and snapshot, deny, toolkit guard, translation template, notices, logo parity) |
| Format | `just fmt` (`just fix` applies clippy fixes first) |
| Regenerate the translation template | `just tr-extract` |
| Render the main screens offline | `just snapshots` (optional language, e.g. `just snapshots de`) |
| MiSTer ARM32 static build | `just arm32` |
| Official MiSTer build with provenance | `just release` |
| MiSTer release bundle | `just release-zip vX.Y.Z` |
| Deploy to MiSTer | `just deploy-mister` |
| Portable x86_64 build (Steam Deck) | `just x86-portable` |
| Desktop tarball with license files | `just package-desktop` |
| Third-party notices | `just notices` |
| Embedded system logo sets | `just logos` (after editing `resources/images/systems*/`) |
| Isolated UI under Slint's MCP server | `just slint-ui` (`--help` lists commands) |
| Host cargo extensions | `just install-tools` |
| Remove `output/` and the cargo target directory | `just clean` |

`just --list` is the source of truth. Every recipe runs on the host; CI runs
the same commands on a bare runner.

## Stack Facts

- Slint 1.17.1, pinned exactly (`=1.17.1`) with `slint-build` and
  `slint-tr-extractor` at the same version. Used under the paid Slint Software
  License.
- Rust workspace under `rust/`, edition 2021, MSRV and toolchain 1.97
  (`rust-toolchain.toml`).
- `rust/frontend` features: `desktop` (default: winit backend, FemtoVG
  renderer, gilrs gamepads), `mister` (custom `slint::platform`, `std` software
  renderer, own presenters), `snapshot` (desktop plus the software renderer for
  the offline `snapshot` binary). Exactly one of `desktop`/`mister`.
- The MiSTer binary is a static `armv7-unknown-linux-musleabihf` build made
  with `cross`. Fonts, logos, glyphs, and translations are embedded; it ships
  as one file.
- `cross` mounts only `rust/`, so `resources/` reaches the container through
  `ZAPAROO_RESOURCES_DIR` (`rust/Cross.toml`). Every `cross` call sets it.

## Always

- Keep comments and docs in American English.
- Use Slint enums for closed sets of UI states (active screen, settings page,
  browse mode, loading/empty/error state), not strings or integer tags. Export
  enums used at the Rust/Slint boundary and use the generated Rust types; map
  toolkit-free domain enums explicitly rather than converting them to strings.
  Model meaningful root/none states as variants, not empty-string sentinels,
  and choose defaults explicitly (Slint otherwise uses the first variant).
  Keep actual indices/counts numeric and text/open-ended IDs as strings. Convert
  persisted/API tokens at the boundary with explicit unknown-value handling;
  preserve existing serialized values and the persisted-schema approval rule.
  See `docs/slint-state-types.md` for the type inventory and boundary exceptions.
- Omit Slint bindings that merely repeat effective built-in defaults. An
  unconstrained `Rectangle` outside a layout already fills its parent; redundant
  `width: 100%` / `height: 100%` (or `parent.width` / `parent.height`) add noise.
  Check element type, inherited bindings, child/preferred-size constraints, and
  layout participation before removing bindings; do not bulk-delete matching
  values. Keep intentional overrides and explicit application-state defaults
  (including enums). Do not assume `Text`, `Image`, custom components, or `x`/`y`
  share Rectangle sizing defaults.
- Prefer Slint's built-in named constants and enums over equivalent magic
  literals: write `font-weight: FontWeight.medium` instead of `500`, and use
  named easing and property-enum values. Keep raw numbers for calculations and
  exact design tokens such as palette colors; keep serialized/API tokens at
  explicit boundaries.
- Let Slint snap ordinary geometry, images, and text to physical pixels. Do not
  wrap visual `x`/`y`/`width`/`height` expressions in
  `Math.round(... / 1px) * 1px` merely for sharpness; this duplicates the
  renderer and rounds at the wrong layer under fractional display scaling. Use
  natural geometry such as `(parent.width - self.width) / 2`. Keep rounding
  only for semantically discrete values or documented exact-pixel contracts
  such as CRT calibration guides, bitmap raster sizing, and QR modules.
- Take geometry from the `Sizing` and `Layout` globals (pushed from
  `zaparoo_app::sizing` and `zaparoo_app::layouts`), not hardcoded pixel sizes
  or element counts. The UI must run cleanly at 240p.
- Put product rules (sizing, palette, grid navigation, menus, input timing,
  status ladders) in the toolkit-free `rust/zaparoo-app` crate with tests, and
  keep `rust/frontend` as the Slint adapter. `scripts/check-toolkit-free.sh`
  enforces the boundary.
- Follow `docs/content-style.md` for every user-visible string: menu
  ordering, capitalization, terminology, and the settings-page checklist.
- Wrap every user-visible string in `@tr()` with `{}` placeholders so
  translators can reorder values. Rust publishes stable keys and values; the
  `.slint` side composes the sentence. Run `just tr-extract` after any `@tr`
  edit. See `docs/translations.md`.
- After editing Rust or `.slint` files, run `just lint`. Run `just test` when
  the change can affect runtime behavior.
- Keep user-visible state persistent. Selected screen, row/grid positions,
  focus, settings, and similar state must be serialized to disk and restored
  before the first frame. MiSTer's wrapper can kill and relaunch the process at
  any time.

## Ask First

- Before adding or changing a `Client` method in
  `rust/zaparoo-core/src/client.rs`, check the upstream API docs:
  <https://zaparoo.org/docs/core/api/>. Method names, params, and return types
  must match Core.
- Before changing sizing rules, their golden fixtures
  (`rust/zaparoo-app/tests/fixtures/`), or the persisted state schema
  (`rust/zaparoo-core/src/persist.rs`), confirm the migration/reset behavior.
- Before adding dependencies, bumping Slint, changing CI, or touching
  license/trademark text, confirm the intended policy.
- Before changing forward screen routing in `rust/frontend/src/router.rs`, see
  "Screens and routing" below.

## Never

- Do not animate properties that force a large dirty region on busy content:
  no translucent overlays fading over a grid, no fading or scaling of a parent
  that contains many tiles, no full-band slides at full resolution. On the
  software renderer the cost is dominated by *painted pixels per frame ×
  per-pixel cost*, not by the animated property: a fading rectangle over 15
  tiles repaints all 15 tiles every frame. Pick animations whose dirty region
  is small (page-dot pulse, focus-ring blink, single-tile move) and let the
  rest of the scene stay static. See `docs/slint-gotchas.md`.
- Do not paint a full-screen background or a translucent overlay over a screen
  body to cover a transition. The source screen stays visible until the
  destination is ready, and the loading cue is the header status line.
- Do not publish state with `tokio::sync::broadcast` when late subscribers need
  the current value. Use `tokio::sync::watch` for state and reserve broadcast
  for lossy events.
- Do not inline a `watch::Sender::borrow()` or any read guard in an `if let`,
  `match`, or `while let` scrutinee when the body writes to the same channel or
  lock. Bind the read in an inner scope first:
  `let next = { let cur = tx.borrow(); fsm.step(&cur) };`.
- Do not leave lint warnings, failing tests, a stale translation template, or
  untranslated user-facing text behind.
- Do not route from a `.slint` view or keep cross-screen state in one. Views
  forward input and render the globals Rust sets; see "Screens and routing".
- Do not persist Core metadata (cover art, scraped properties, descriptions,
  etc.) to disk or any user-visible cache. Zaparoo Core is the canonical store;
  the frontend caches in process memory only and re-fetches what it needs after
  a cold start. Any in-memory cache must enforce a strict bytes cap with LRU
  eviction: MiSTer has under 512 MB of shared system RAM and the frontend
  competes with Core, the FPGA wrapper, and the active core for it.
  **Scoped exception:** `rust/frontend/src/hub_covers.rs` persists a small
  path *list* (`hub_covers.toml` in the cache dir; never image bytes or
  metadata) mapping each Hub `zapscript` tile and the Resume tile to the Core
  thumbnail path Core itself already wrote to
  `/media/fat/zaparoo/cache/thumbs/`, so those covers can seed the in-memory
  cache before the first frame on a cold boot. Bounded by
  `zaparoo_app::covers::MAX_HUB_ENTRIES` plus the Resume tile, colocated
  `MiSTer` only, and self-healing: a stale path just fails to open and falls
  through to a normal Core request. Do not extend this carve-out to any other
  cache without discussing it first.
- Do not open a modal from a modal. A choice made inside a modal is a page of
  that modal's own panel (the setup panel in `ui/setup.slint` swaps between
  its form and its picker page; the context menu hosts the alternate-versions
  page); only an action-error alert (shown through the decision dialog) may
  sit above an open modal. See `docs/style.md` → "Modal depth".
- Do not add `.git/` rerun-if triggers or `ZAPAROO_BUILD_*` provenance baking
  to `rust/frontend/build.rs`. Provenance lives in the `rust/build-info` leaf
  crate precisely so commits don't re-run the Slint compiler. See
  `docs/building.md` → "Build caching".
- Do not load assets from loose files at runtime. Fonts, logos, glyph SVGs and
  catalogs are embedded (`include_bytes!`, `include_str!`, `@image-url`,
  bundled translations). The only images read from disk are user overrides
  from the `custom/` folder and, on a colocated MiSTer, Core's own thumbnail
  files.

## Build caching couplings

Full inventory and rationale: `docs/building.md` → "Build caching". The
update rules, in short:

- Bumping the Rust toolchain pin touches `rust-toolchain.toml` (repo root, so
  `cross` and cargo invoked from `rust/` both resolve it) and the
  `rustup toolchain install` line in `.github/workflows/release.yml`.
- Bumping Slint touches `slint` and `slint-build` in `rust/frontend/Cargo.toml`,
  the `slint-tr-extractor` version in `just install-tools`, CI, and
  `scripts/check-translations.sh`, and the Slint exceptions in `rust/deny.toml`;
  then regenerate `just notices`.
- `RUSTC_WRAPPER` is sccache when installed (set by the justfile). `cross`
  builds keep their own target directories under `rust/target/<triple>/`.

## Project Map

| Path | Purpose |
|---|---|
| `rust/frontend/src/main.rs` | Entry point: config, logger, tokio runtime, `Client`/`Store`, persisted state, window, language, services |
| `rust/frontend/src/router.rs` | All forward orchestration and the single input dispatch (`dispatch_action`) |
| `rust/frontend/src/navigation.rs`, `folder_motion.rs`, `route_motion.rs` | Deferred routes that keep the source until the destination is ready; motion tests on a stepped clock |
| `rust/frontend/src/{hub,systems,games,settings,about}.rs` | Per-screen drivers |
| `rust/frontend/src/{game_info,media_setup,log_upload,launchers,alternates,card_write,qr}.rs` | Modal drivers |
| `rust/frontend/src/{input,actions,gamepad}.rs` | Key path: duplicate guard, swaps, hold-repeat; keyboard bindings; desktop gamepads |
| `rust/frontend/src/{media_cache,hub_covers,customization}.rs` | Bounded in-memory cover cache, cold-boot cover manifest, user overrides |
| `rust/frontend/src/{theme,sizing,glyphs,system_logos,fonts}.rs` | Palette push, scene sizing adapter, embedded art and fonts |
| `rust/frontend/src/mister/` | MiSTer platform: fb0, DDR and vblank-latch presenters, evdev input, lease, service kick |
| `rust/frontend/src/dual_head.rs` | Projects the HDMI component's state onto the CRT component |
| `rust/frontend/src/bin/snapshot.rs` | Offline software-rendered screen snapshots |
| `rust/frontend/ui/` | `.slint` views: `app.slint` (root, globals, modals), `chrome.slint`, `tiles.slint`, `browse_list.slint`, `settings.slint`, `setup.slint`, `game_info.slint`, `about.slint`, `focus.slint`, `theme.slint`, `labels.slint`, `state_types.slint` |
| `rust/frontend/assets/` | Embedded system logo PNGs: grayscale `systems/` and full-color `systems-color/`, generated by `just logos` |
| `rust/frontend/translations/` | Canonical gettext catalogs, bundled by `build.rs` |
| `rust/zaparoo-app/` | Toolkit-free product rules with golden fixtures |
| `rust/zaparoo-core/src/client.rs` | WebSocket JSON-RPC client for Zaparoo Core |
| `rust/zaparoo-core/src/store/` | Endpoint cache, tags, mutations, invalidation |
| `rust/zaparoo-core/src/persist.rs` | Atomic persisted UI state |
| `rust/zaparoo-core/src/platform_paths.rs` | Config, log, state, and cache paths per runtime |
| `rust/mock-core/` | Mock Zaparoo Core for dev runs |
| `rust/build-info/` | Build provenance leaf crate |
| `resources/` | Fonts, glyph SVGs, logos, system logo sources |

## Screens and routing

The frontend has root screens (Hub, Systems, Favorite systems, Games,
Favorites, Recents, Settings, About) plus modals. The `.slint` views are
**pure views**: every key press is forwarded to Rust, `router::dispatch_action`
maps it to an action, and Rust mutates the globals the views render. All
forward orchestration lives in `rust/frontend/src/router.rs` and the
per-screen drivers it calls.

When adding a new screen or routing path, follow this contract:

1. **One dispatch, one priority order.** `dispatch_action` decides ownership
   top-down: screensaver, CRT calibration, decision dialogs, boot curtain,
   modals, then the transition gate, then the active screen's driver. Add a
   new surface at its place in that ladder; do not add a second input gate.
2. **Forward routes are deferred.** `router::begin_pending` marks the
   transition, keeps the source screen visible and interactive only for
   Cancel, and shows the loading cue in the status line after 300 ms. The
   driver fills the destination, then `router::transition_to_screen` commits
   the complete route in one turn. `navigation.rs` retains the one source
   (moved, not copied) so Cancel restores it and persistence stays on the
   coherent source until the destination is ready.
3. **Back is a driver decision.** Each driver owns its peer-up logic
   (e.g. Games returning to Systems or to the Hub it was entered from, which
   is persisted per screen).
4. **No stale tickets.** Async fills carry a sequence or ticket; a completion
   whose ticket no longer matches is dropped. Never apply a late response to
   whatever screen happens to be active.
5. **Persisted state is per screen.** New screen selection state goes in its
   own section of `zaparoo_core::persist::PersistedState`; drivers write it on
   directional moves (selection persist is debounced 250 ms and flushed on
   Accept, Back and hold release).

The class of bug this layout prevents: a stale pending flag or callback from
screen A firing during screen B's fill and routing somewhere the user did not
ask to go.

## Runtime Notes

- `Runtime` answers where the frontend binary is running. Core's own
  platform (the `platform` field of its `version` RPC) answers where Zaparoo
  Core is running. Do not collapse them: a colocated check keys off
  `Runtime`, never off what Core reports.
- Desktop config: `~/.config/zaparoo/frontend.toml`.
- Desktop state: `~/.config/zaparoo/state.toml`.
- Desktop log: `~/.local/share/zaparoo/logs/frontend.log`.
- MiSTer config: `/media/fat/zaparoo/frontend.toml`.
- MiSTer state/log: `/tmp/zaparoo/state.toml`, `/tmp/zaparoo/frontend.log`.
- `ZAPAROO_CORE_ENDPOINT` overrides `[core] endpoint`; `ZAPAROO_STATE_FILE`
  redirects state for tests and ad-hoc runs.
- `Runtime` has three values: `Mister`, `SteamOs`, `Desktop`. SteamOS is a
  desktop-Linux runtime and answers `is_desktop()`, so `platform_paths.rs`
  stays a two-way `is_mister()` split; the variant only changes defaults for
  a device that presents like a console. It starts fullscreen.
  `ZAPAROO_RUNTIME_OVERRIDE=steamos|mister|desktop` forces detection for
  off-device work.
- `Runtime` answers what the machine is; `display_class::Viewing` answers
  how far away the person is from the screen we are painting on, which is
  what layout density actually wants. It is a property of the *output*, so
  it is re-read on every scene change rather than cached: a docked Steam
  Deck is `Seated` and an undocked one is `Handheld`, same binary. Only a
  `SteamOs` runtime can be `Handheld` at all, because a laptop also drives
  a built-in panel and a 13-inch screen at desk distance subtend more
  than twice the angle a Deck does. The `device` interface profile follows
  this, not the runtime; `ZAPAROO_VIEWING_OVERRIDE=handheld|seated` forces
  it. Never derive it from reported DPI: inside a gamescope session the
  game's Xwayland output is a hardcoded 100x150mm whatever is connected.
- `[video] fullscreen` is tri-state on purpose. An absent key lets the
  runtime decide; only an explicit `false` puts a Deck back in a window.
  `--fullscreen` and `--windowed` override it for one run.
- Debug logging is enabled with `[logging] debug = true` or `ZAPAROO_DEBUG=1`.
- The header's battery reading has two probes behind one HUD field.
  `MiSTer` reads the optional pi-top-style `SMBus` fuel gauge
  (`mister_battery.rs`); everything else reads the kernel power-supply
  class (`power_supply.rs`). Filter that second one on `scope`: a paired
  DualSense shows up as a `type=Battery` with `scope=Device`, so a naive
  "first battery wins" puts the controller's charge in the status bar.
- The desktop build pins `SLINT_SCALE_FACTOR=1` before the window exists.
  Sizing keys off the real framebuffer (that is what the resolution tiers
  are), so a compositor-derived scale factor is a bug here, not a feature:
  a Steam Deck reports 2.17, which turns a 1280x800 output into a 591x369
  logical scene, drops the whole UI into the 240p tier, and upscales every
  rasterized glyph. Setting the variable yourself still overrides it.
- `/tmp/zaparoo_launcher_input.json` is Main_MiSTer's alt-launcher input
  report (written on every button press; source is
  `support/zaparoo/launcher_input_metadata.cpp` in the `Main_MiSTer` repo).
  `zaparoo_core::controller_report` polls it, and `apply_buttons` in
  `main.rs` feeds it through `zaparoo_app::buttons::resolve`, which drives the
  help bar's icon style and accept/cancel positions.
  `ZAPAROO_INPUT_REPORT_FILE` redirects it and forces the watcher on
  off-MiSTer, mirroring `ZAPAROO_STATE_FILE`. Off MiSTer the `gamepad` module
  (gilrs, `desktop` feature only) fills the same `controller_report` channel
  from the connected pad, and publishes the keyboard style again on the next
  real key press. One producer owns the channel: the watcher wins where it
  runs. Button-style ids stay the neutral `style_a`/`style_b`/`style_c`/
  `style_d`/`style_e` letters, never a controller maker's name. Any other
  persisted value (the legacy `a`/`b`/`c`/`d` and the older
  `nintendo`/`xbox`/`sony` included) resolves as `auto`.
- `[input.keyboard]` bindings in `frontend.toml` are stored as Qt key codes;
  that is a persisted format, kept for existing installs.
  `actions.rs` maps Slint key events onto those codes.

## MiSTer Deploy

`just deploy-mister` reads `MISTER_IP` (and optional `MISTER_PW`) from `.env`,
builds the ARM32 binary with `cross`, uploads it to
`/media/fat/zaparoo/frontend.new`, verifies the size, keeps the previous
binary as `frontend.bak`, clears `/tmp/zaparoo/frontend.log`, and SIGKILLs the
running frontend so `MiSTer_Zaparoo` respawns it. `--skip-build` reuses the
last build.

`/media/fat/zaparoo/MiSTer_Zaparoo` is the integration binary shipped with
MiSTer. It starts our `frontend`; do not replace that flow with a new wrapper
script.

## Release

A MiSTer release bundles three independently versioned components: the
**frontend** (this repo), the **`MiSTer_Zaparoo`** host wrapper (`Main_MiSTer`),
and the **`menu_zaparoo.rbf`** menu core (`Menu_MiSTer`). The analog video path
depends on all three matching. Full checklist: `docs/building.md` → "Cutting a
release". Key points:

- The version lives in one place: `version` in `rust/Cargo.toml`
  (`[workspace.package]`). Bump it and regenerate `rust/Cargo.lock`
  (`cargo update --workspace`). About and the log upload read
  `CARGO_PKG_VERSION`; do not hardcode it.
- `scripts/package-mister-release.sh` fails when the tag does not match the
  workspace version, and fails when `rust/frontend/LICENSES/THIRD-PARTY-NOTICES.txt`
  is missing. Run `just notices` after dependency changes.
- The release workflow auto-resolves the latest `Main_MiSTer` and `Menu_MiSTer`
  releases; override with the `menu_tag` workflow input or
  `MENU_MISTER_TAG` / `MAIN_MISTER_TAG` when running the packaging script.
- After publishing, update the downloader DB in `Zaparoo_MiSTer` (`db.json` and
  its distributed zip(s)).

## Further Reading

- `docs/architecture.md`: module graph, data flow, runtime/platform split
- `docs/building.md`: requirements, builds, caching, deploy, release checklist
- `docs/slint-gotchas.md`: software-renderer costs, motion rules, Slint traps
- `docs/slint-state-types.md`: typed UI state inventory and boundary rules
- `docs/style.md`: corner-radius token, tile aspect, pill vs. sharp shapes
- `docs/content-style.md`: menu ordering, wording, and terminology rules
- `docs/customization.md`: user overrides: `custom/` image folder, `[custom.system_names]`
- `docs/translations.md`: `@tr()` pipeline and locale resolution
- `rust/frontend/translations/README.md`: catalog maintenance steps
- `LICENSES/`: asset attributions; `rust/frontend/LICENSES/`: crate notices
