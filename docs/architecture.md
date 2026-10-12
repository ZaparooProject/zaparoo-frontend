# Architecture

## Module graph

```
rust/frontend/  [frontend library and binary; Slint UI]
  src/main.rs
  │   Thin binary entry calling frontend::run().
  src/lib.rs
  │   Application: config, logger, tokio runtime, Client + Store, persisted
  │   state, window and globals, language, background services, event loop.
  │
  ├── ui/*.slint  [compiled by build.rs through slint-build]
  │     app.slint        : root App component, exported globals (Shell, Overlays,
  │                        HubView, SystemsView, GamesView, ...), screens, modals
  │     chrome.slint     : header, status line, help bar, modal shell, cues
  │     tiles.slint      : Tile and the paged grid view
  │     browse_list.slint, settings.slint, setup.slint, game_info.slint,
  │     about.slint      : screen and modal views
  │     focus.slint      : FocusTarget, the scroll offset and selection cursor
  │     theme.slint      : Theme, Sizing, Layout, Motion globals
  │     labels.slint     : key-to-@tr vocabularies
  │     state_types.slint : enums shared with Rust
  │
  ├── src/host.rs        : hosted entry (host::run) and the Input seam
  ├── src/router.rs      : input dispatch and forward orchestration
  ├── src/navigation.rs, folder_motion.rs, route_motion.rs
  │                        deferred routes that keep the source until ready,
  │                        and the stepped-clock motion tests over them
  ├── src/{hub,systems,games,search,settings,about,update}.rs
  │                        per-screen drivers (update.rs hosts the Update
  │                        module's session; see "Update module")
  ├── src/keyboard.rs    : on-screen keyboard state for one text field
  ├── src/browse_filter.rs : the tags picker, for Games and Search
  ├── src/{game_info,media_setup,log_upload,launchers,card_write}.rs
  │                        modal drivers
  ├── src/context_page.rs : a page of the context menu: Manage and Write to
  │                        token from the menu's own rows, and the two Core
  │                        is asked for, alternates.rs and discs.rs
  ├── src/game_info_data.rs : Game Info metadata and carousel ordering
  ├── src/tag_utils.rs   : compact tag tokens for inline metadata
  ├── src/qr.rs          : QR matrix for the write deep-link and doc links
  ├── src/{input,actions,gamepad}.rs
  │                        key path, keyboard bindings, desktop gamepads
  ├── src/status.rs      : header status-line driver over the ladder
  ├── src/system_status.rs : host-local hardware and network HUD probes
  ├── src/{power_supply,mister_battery}.rs
  │                        the two battery probes behind one HUD field
  ├── src/media_cache.rs : bounded in-memory cover cache (LRU, bytes cap)
  ├── src/hub_covers.rs  : cold-boot cover path manifest: Hub tiles and the
  │                        browse page a game was launched from (MiSTer only)
  ├── src/hub_refresh.rs : keeps pinned Hub games and folders on Core's current rows
  ├── src/customization.rs : user overrides from the customization folder
  ├── src/{theme,sizing,glyphs,system_logos,fonts}.rs
  │                        palette push, scene sizing, embedded art and fonts
  ├── src/display.rs     : output timing, framebuffer render size, CRT scene size
  ├── src/drs.rs         : heavy-phase signal for dynamic resolution scaling
  ├── src/frame_transition.rs : cached page transitions over endpoint frames
  ├── src/browse_motion.rs : scroll window for Slint's bounded list view
  ├── src/press_feedback.rs : hold the accepting control visible, then dispatch
  ├── src/view_model.rs  : republish models without resetting unchanged rows
  ├── src/state_types.rs : token boundaries for UI enums (disk and API text)
  ├── src/latch_protocol.rs : vblank-latch scanout packing, CRC and parsing
  ├── src/{steam,steam_host,gamescope}.rs
  │                        SteamOS: app identity, the runtime host, compositor
  │                        focus in a gamescope session
  ├── src/dual_head.rs   : HDMI state mirrored onto the CRT component
  ├── src/bin/snapshot.rs : offline software-rendered screen snapshots
  ├── assets/            : embedded system logo PNGs, grayscale (systems/, plus
  │                        a half-size copy in systems-half/ for small tiles)
  │                        and full color (systems-color/), from `just logos`
  └── src/mister/        [feature = "mister"]
        platform.rs      : custom slint::platform: frame loop, DRS, clock
        fb0.rs, fb_mapping.rs, ddr.rs, latch.rs : presenters and fb mapping
        transition.rs    : page-commit handoff to latch presentation
        input.rs, lease.rs, service.rs, video_mode.rs, tty.rs, uio.rs

rust/zaparoo-app/  [toolkit-free product rules]
  sizing, layouts, palette      : geometry and color, pinned by golden fixtures
  paged_grid, media_list, hub, systems, settings, letter_jump
                                  navigation, paging, menus, rows
  options_menu                    : what an item's Options menu offers, in
                                    order, and what moves to its Manage page
  keyboard, search, browse_filter : key layout and text editing, the Search
                                    screen's focus and count rules, tag filters
  form_list, system_picker        : cursor rules for sectioned lists, and the
                                    system list grouped by manufacturer
  input, status_line, action_error, buttons, clock, covers, customization,
  launchers, alternate_versions, multi_disc, media_setup, log_upload, format
  (no Slint or other toolkit dependency; scripts/check-toolkit-free.sh)

rust/zaparoo-core/  [Core client and shared state]
  client.rs           : WebSocket JSON-RPC 2.0 (tokio-tungstenite)
  transport.rs        : Core endpoint: TCP, or a Unix socket with an API key
  remote_resource.rs  : RemoteResource<T>/ResourceStatus<T>
  store/              : Endpoint, Mutation, Tag, Store cache
  endpoints/          : CatalogEndpoint, MediaSearchEndpoint, RunMutation
  systems_catalog.rs  : CatalogData payload + by-category filter
  input_actions.rs    : action names + key-code bindings
  persist.rs          : atomic persisted UI state
  config.rs           : TOML config (frontend.toml)
  hub_layout.rs       : Hub's persisted [[hub.items]] layout schema
  controller_report.rs : Main_MiSTer input report watcher
  logger.rs           : tracing-subscriber: stderr + JSONL file sinks
  runtime.rs          : Runtime enum: what device the frontend runs on
  display_class.rs    : Viewing enum: how far away the screen we paint on is
  platform_paths.rs   : log/config/state/cache paths routed through runtime,
                        or under a host's HostPaths roots
  media_types.rs      : Core media types

rust/build-info/  : commit, date and channel baked in by build.rs (leaf crate)
rust/mock-core/   : mock Zaparoo Core for dev runs
```

## Key constraints

- **Software rendering on MiSTer.** No GPU. The MiSTer build drives Slint's
  software renderer from its own frame loop and presenters. Frame cost is
  painted area times per-pixel cost; see `docs/slint-gotchas.md`.
- **Resolution-agnostic layout.** The UI runs from 240p CRT output to 1080p.
  Geometry comes from the `Sizing` and `Layout` globals, which Rust pushes from
  `zaparoo_app::sizing` and `zaparoo_app::layouts` whenever the scene changes.
- **One static binary.** Fonts, glyph SVGs, logos and translation catalogs are
  embedded at build time. The MiSTer binary is a static musl build made in
  the toolchain image (`Dockerfile.toolchain`).
- **Core is the canonical store.** Covers and metadata live in process memory
  only, with a strict bytes cap. The one on-disk exception is the Hub cover
  path manifest (see `AGENTS.md`).

## Hosted library

The `hosted` feature builds `rust/frontend` as a library another application
embeds. The host owns the process, the Slint backend and renderer, the
window lifecycle, process logging and the Core connection's endpoint; this
crate still owns all UI, navigation and input semantics.

- `host::run(&Options, ready)` runs the application on the host's already
  initialized Slint thread and calls `ready` with an `Input` for that window.
  When the event loop returns, the instance is gone and the host may start a
  new one.
- `Options::paths` (`HostPaths`) gives absolute config, data and cache roots,
  installed once per process; every path in `platform_paths` resolves under
  them.
- `Options::core_transport` is the Core endpoint: `Transport::tcp` (a
  WebSocket URL, optionally with an API key) or, on Unix, `Transport::unix`
  (a socket path and a required API key). `None` waits for the
  host; a hosted build never falls back to localhost. `Input::set_core_transport`
  replaces it at runtime; pending calls on the old session fail and nothing
  is replayed. API keys never appear in `Debug` output or config.
- `Options::log_upload` (`LogUploader`) posts the support bundle. The
  frontend builds the multipart body; the uploader sends one HTTPS POST and
  returns the response body. Without one, Settings > Upload log file is not
  offered. Standalone builds post with curl; on MiSTer curl is pointed at
  the downloader's CA bundle when it exists, because the stock bundle is
  too old to verify the upload service.
- `Input::action(Action, pressed)` takes semantic actions as raw press and
  release; duplicate suppression and hold-repeat stay in `input.rs`, so
  framework key repeat must not be forwarded. `Input::clear` drops held
  inputs on focus loss.
- `Input::core_phase(CorePhase)` shows the host's Core startup phase on the
  boot curtain until boot completes.
- `Input::activated` tells the frontend its window is showing again (the
  user returned from another app). It counts as activity: a screensaver
  that armed in the background is dismissed and the idle countdown
  restarts.
- `host::system_status` reports network, Bluetooth and battery state for
  the header; a hosted build never probes the machine itself. It is
  device-level: callable from any thread before any window exists, and the
  latest report wakes the header at once.
- `host::controller(name)` names the pad now driving the UI (`None`: none).
  It sets the help bar's glyphs and returns the actions for the pad's
  labelled A, B, X and Y buttons, so a host that reads buttons by label
  binds options and view to the faces the bar draws.
- `host::trim_memory` drops every decoded cover when the system asks the
  app to use less memory. Tiles on screen keep their copy; anything else is
  fetched again when next shown, and screens re-resolve their art on the
  next activation. Safe from any thread.
- `host::install_perf(PerfHooks)` installs timing hooks once per process,
  before `run`. The frontend reports milestones as an event name plus
  `k=v` fields for the host to stamp with its own clock: first frame, the
  first Hub frame with every cover present (also `fully_drawn`), open press
  to first rows and to covered rows, launch press and reply, the first
  frame after `host::window_renewed` (the host gave the window a new
  surface), and a frame-time summary when a held scroll ends. Without hooks
  every mark is a no-op.
- `Input::configure_folder_picker` and `configure_launcher_scan` register
  optional host actions. Each adds a row to Settings > Library ("Add game
  folder", "Detect launchers") that is absent until configured. The host
  reports progress with `folder_picker_status` and `launcher_scan_status`:
  ordered snapshots of a state and a count, where a stale revision is
  ignored and no path or platform handle crosses the seam. A drop in saved
  folders shows as revoked access until access returns or the user acts. A
  finished launcher scan makes the frontend ask Core to refresh its
  launcher list. `request_folder_picker` and `request_launcher_scan` trigger
  the same guarded request as the rows. Configuration and status live with
  the window, so a host configures them again for every new window.
- `Input::configure_playtime_access` offers a handoff to the system
  setting that lets the host measure foreground playtime exactly; it adds
  "Verify playtime…" to Settings > Library. `playtime_access_status`
  reports whether it is granted, which the row shows. Granting stays the
  user's action in system settings.
- `Input::navigation_events`, `indexed_system_count`, `core_connected`,
  `folder_picker_pending`, `folder_permission_count` and
  `folder_permission_revoked` are read-only diagnostics for host tests; they
  carry no behavior.

A hosted build compiles out what the host owns: process-global logging,
command-line arguments, process restart, the Linux network probe, the Steam
session host and gamescope focus claims. `just hosted-check` runs clippy and
the library tests for the hosted feature set on the software renderer; it
does not compile for any particular host target.

## Update module

The Update screen (firmware and core updates through the Downloader) belongs
to a separately owned module, `zaparoo-update`, whose source is private. The
public repository carries only its interface and a stand-in, and official
builds compile the real thing in.

```
rust/zaparoo-update-api/   toolkit-free contract: ViewState, Input, Effect,
                           Event, the row and enum types (plain data, no deps)
rust/zaparoo-update/       public stub: is_updater_available() is false,
                           UpdateSession does nothing, ui/update.slint is empty
rust/private/zaparoo-update/   gitignored checkout of the private repo
rust/frontend/ui/update_view.slint   the UpdateView global and its enums
rust/frontend/src/update.rs          the adapter
```

- **One crate identity.** `rust/zaparoo-update/build.rs` looks for
  `rust/private/zaparoo-update`. When `src/imp/mod.rs` and `ui/update.slint`
  are there, it compiles `src/imp/**` into the stub crate as its `imp`
  module (`#[path]` through a generated file) and publishes the private
  `ui/` and `translations/` directories through cargo metadata
  (`DEP_ZAPAROO_UPDATE_UI_DIR`, `DEP_ZAPAROO_UPDATE_TRANSLATIONS_DIR`).
  Otherwise it publishes its own empty screen. `Cargo.lock`, `cargo metadata`,
  `cargo deny` and every workspace command are identical either way. The
  private crate may therefore use only the dependencies the stub lists
  (`tokio`, `tracing`, `libc`, `zaparoo-update-api`) and module-relative paths,
  and is linted under this workspace's lint table.
- **Who owns what.** The module owns the update run (the updater tool, the
  DLP1 event stream, cancellation) and the screen's whole state machine:
  page, focus, filter, collapse, timers. It publishes a `ViewState`.
  `update.rs` maps that onto `UpdateView` and applies row deltas to one
  `VecModel`; the private `update.slint` only renders the global and forwards
  taps. Every sentence is composed in `.slint` from enums and ids with
  `@tr()`; the module never sends prose.
- **Effects.** `LeaveToHub`, `ConfirmStop` (the "Stop update?" decision
  dialog, `DialogKind::UpdateStop`), `CloseStopConfirm` and `Reboot` come back
  through the same sink as state. The router owns dialogs and routing, and the
  adapter answers with `Input::StopConfirmed`, `StopDeclined` or
  `RebootFailed`.
- **Build.** `build.rs` maps `@zaparoo-update` (the module's `ui/`) and
  `@zaparoo-ui` (this crate's `ui/`) as Slint library paths, so the private
  screen imports `Theme`, `Sizing` and the components with
  `@zaparoo-ui/...` and shares the one set of globals. It also merges the
  module's `translations/<lang>/LC_MESSAGES/frontend.po` into the bundled
  catalogs; the frontend's entry wins on a duplicate msgid.
- **Availability.** The Hub tile shows only when `update::available()`: the
  module is built in, the device has an updater tool
  (`ZAPAROO_UPDATE_TOOL`, `/media/fat/Scripts/update.sh`, then
  `downloader.sh`), and the build is not `hosted`. The screen is not restored
  after a restart: a killed run cannot resume, and the screensaver stays off
  while a run is active.

## Rust → Slint data flow

1. `zaparoo-core` owns the Core connection. `Store` caches endpoint results and
   publishes them through `tokio::sync::watch` channels.
2. Drivers in `rust/frontend/src/` subscribe on the tokio runtime, project the
   data with the rules in `zaparoo-app`, and hand the result to the UI thread
   with `upgrade_in_event_loop`.
3. On the UI thread, drivers write Slint globals and models (`VecModel`), and
   the `.slint` views render them. Rust publishes stable keys and values; the
   views turn keys into translated text.
4. Input flows the other way: the root view forwards every key event to Rust,
   `input.rs` applies the duplicate guard, swaps and hold-repeat, and
   `router::dispatch_action` routes the action to whichever surface owns input.
5. State that must survive a kill is written through
   `zaparoo_core::persist::save` (atomic write) from the drivers, and loaded
   before the first frame.

### Navigation state

Forward routes are deferred. `router::begin_pending` marks the transition and
keeps the source screen visible; after 300 ms the header status line shows the
loading cue. The destination driver fills its model, and
`router::transition_to_screen` commits the whole route in one turn.
`navigation.rs` holds the one retained source (moved, not copied) so Cancel can
restore it and persistence stays on the coherent source until the destination
is ready. While a transition is pending, only Cancel is accepted.

Async fills carry tickets; a completion whose ticket no longer matches is
dropped. Per-screen selection state lives in its own section of
`PersistedState`, written on directional moves with a 250 ms debounce and
flushed on Accept, Back, and hold release.

### Cold-start restore

The wrapper can kill and relaunch the frontend at any time, so every start
restores the saved screen. The rule is that a start shows the boot curtain and
then the restored screen, with nothing in between: no parent screen, no Hub, no
half-filled list.

- The curtain is seeded before the first frame for every Core-dependent start
  (`boot_curtain_for` in `lib.rs`). Settings and About need no Core and paint
  final from the first frame. A first start, with no state file, has nothing to
  restore and paints its Hub optimistically instead.
- `restore_screens` runs when the first catalog arrives and fills the target
  under the curtain. The Hub stays the (hidden) active screen until the target
  commits; a restored Games screen fills its category with
  `systems::prepare_parent`, which does not show it or name it the saved screen.
  `navigation::stage` retains no source under the curtain.
- `router::finish_restore` is the only place the curtain lifts.
  `router::transition_to_screen` calls it, so the commit of the restored screen
  and the lift are the same turn. The other exits call it too: a missing
  category or system, a catalog error, Cancel and the 15 second bound
  (`router::abandon_restore`, which retires the fill and lands on the parent),
  and Core staying unreachable for that same bound (`give_up_boot_restore`).
- A Hub start also waits for the Resume tile's answer (`hub::resume_settled`),
  under the same bound.
- The saved screen keeps naming the restore target until it commits, so a kill
  mid-restore comes back to the same place. Startup notices open only after the
  curtain lifts.

## Runtime vs Platform

The frontend tracks two separate facts. Do not collapse them; that is how old
runtime/platform bugs come back.

| Concept | Source of truth | Question answered |
|---|---|---|
| **Runtime** | `zaparoo_core::runtime::current()` (filesystem-cached) | What device is the **frontend binary** running on? |
| **Platform** | Core's `version` RPC (`platform` field) | What OS/device is **Zaparoo Core** running on? Not consumed today; every colocation check keys off Runtime. |
| **Viewing** | `zaparoo_core::display_class::current()` (re-read, never cached) | How far away is the person from the screen we are **painting on right now**? |

`Runtime == Mister` does **not** imply `Platform == Mister`. The frontend
can run on a desktop while talking to Core on a MiSTer on the network,
or vice-versa.

Runtime has three values: `Mister` (the `/media/fat` marker), `SteamOs`
(`ID` or `ID_LIKE` in `/etc/os-release`), and `Desktop`. SteamOS is a
desktop-Linux runtime and answers `is_desktop()`; it shares the XDG paths,
the windowing system and the rendering backend, so `platform_paths.rs`
stays a two-way `is_mister()` split. The variant exists only to change
defaults for a device that presents like a console: it comes up fullscreen.
Set `ZAPAROO_RUNTIME_OVERRIDE=steamos` to develop that behavior off a Deck.

### Viewing class

Layout density wants to know how far away the person is, and nothing
reports that. Every platform that solves it assumes a distance per device
class and bakes it into a design unit: UWP's effective pixels fold density
and an assumed distance together so a control subtends a constant angle
from a phone to a Surface Hub, Android TV assumes 3 m and ships a fixed
12-column grid at both 1080p and 4K, and tvOS designs at 1920x1080 and
renders 4K at 2x. None of them measure anything, and neither do we.

`display_class::Viewing` is `Handheld` or `Seated`, and it is a property of
the **output**, not of the machine, so it is re-read on every scene change
instead of cached. A docked Steam Deck is `Seated` and an undocked one is
`Handheld` from the same process. Detection reads `/sys/class/drm`: an
external connector that is both `connected` and `enabled` wins, otherwise a
lit internal panel (`eDP`/`LVDS`/`DSI`/`DPI`) means handheld. `enabled` is
the load-bearing word, because a cable in a dock with the display asleep is
not the screen the user is looking at.

Only a `SteamOs` runtime can be `Handheld` at all. A laptop also drives a
built-in panel, and a 13-inch screen at desk distance subtends more than
twice the angle a Deck does, so treating every internal panel as handheld
would hand the roomier layout to the screen that least needs it.

The angular arithmetic the two classes rest on, written down so the call
stays falsifiable:

| Screen | Width | Distance | Subtends |
|---|---|---|---|
| Steam Deck panel | 151 mm | ~400 mm | ~21° |
| 52 inch TV | 1150 mm | ~3000 mm | ~22° |
| 24 inch monitor | 530 mm | ~600 mm | ~48° |

Note what that says: a handheld at arm's length and a TV across the room
subtend nearly the same angle, so angular size alone does **not** separate
them, and a desk monitor is far wider than either. The split is an
ergonomic call, not a derivation. What the angle does give you is a floor:
ISO 9241 wants characters subtending 20-22 arcminutes and treats ~16 as the
minimum, so it can say when a choice is wrong even though it cannot say
which choice is best. Column counts above that floor are a product
decision, which is why Android TV ships "12 columns of 52dp" as a literal.

`ZAPAROO_VIEWING_OVERRIDE=handheld|seated` forces the class for off-device
work. Never derive it from reported DPI: inside a gamescope session the
game's Xwayland output is a hardcoded 100x150mm whatever is really
connected, which is why the desktop build pins `SLINT_SCALE_FACTOR=1`.

### When to use which

- **Runtime gate**: use this when the frontend's host device changes the
  behavior. Read `runtime::current()`. Prefer runtime gating for behavior.
- **Cargo feature `mister`** (`#[cfg(feature = "mister")]`): use this only
  for code that cannot compile into desktop binaries: the custom Slint
  platform, presenters, evdev input, and similar. Everything else compiles
  into both builds and branches on `Runtime`.
- **Platform gate**: use this when a feature depends on what Core supports.
  Subscribe to `platform::subscribe()` and treat `None` as unknown; do not
  enable platform-specific behavior until the first `version` RPC completes.
  Route the decision through Rust and expose the result to the view as a
  property. The frontend does not start `platform::spawn_fetcher` today, so
  wire that up in `lib.rs` before relying on this gate.

**Never gate runtime behavior on `Platform`, never gate Core
assumptions on `Runtime`.** They are independent.
