# Architecture

## Module graph

```
src/app/main.cpp
  frontend (executable)
    │   Thin C++ entry point: constructs QGuiApplication + QQmlApplicationEngine,
    │   installs Qt message handler, calls zaparoo_rust_init() from the Rust staticlib.
    │
    ├── rust/frontend/  [zaparoo_frontend_rs staticlib]
    │     ├── src/lib.rs
    │     │     zaparoo_rust_init()      — tokio runtime, logger, WebSocket client,
    │     │                               Store, model globals
    │     │     zaparoo_rust_post_qt_start() — post-engine hooks
    │     │     zaparoo_log_qt()         — Qt message handler sink → tracing registry
    │     │
    │     ├── src/bind.rs
    │     │     `bind_to_endpoint!` macro — emits the cxx_qt::Initialize
    │     │     impl for QML singletons (sync seed + qt_thread watcher).
    │     │
    │     ├── src/mister_runtime.rs
    │     │     Pre-Qt setup on ARM32: vmode resolution switch, zaparoo.sh start.
    │     │     Compiled on all platforms; MiSTer-specific calls are gated by cfg.
    │     │
    │     ├── src/models/  [Zaparoo.Browse QML module via cxx-qt 0.8]
    │     │     AppStatus, CategoriesModel, SystemsModel, GamesModel,
    │     │     AppState, HubState, SystemsState, GamesState, Input, Runtime,
    │     │     BrowseModel. All registered via build.rs QmlModule.
    │     │
    │
    ├── rust/zaparoo-core/  [non-Qt Rust crate]
    │     client.rs           — WebSocket JSON-RPC 2.0 (tokio-tungstenite)
    │     remote_resource.rs  — RemoteResource<T>/ResourceStatus<T>
    │     store/              — Endpoint, Mutation, Tag, Store cache
    │     endpoints/          — CatalogEndpoint, MediaSearchEndpoint, RunMutation
    │     systems_catalog.rs  — CatalogData payload + by-category filter
    │     input_actions.rs    — action names + Qt key-code mapping
    │     persist.rs          — write-through persisted UI state
    │     config.rs           — TOML config (frontend.toml)
    │     logger.rs           — tracing-subscriber: stderr + JSONL file sinks
    │     runtime.rs          — Runtime enum: what device the frontend runs on
    │     platform.rs         — Platform enum: what Zaparoo Core is running on
    │     platform_paths.rs   — log/config paths routed through runtime
    │     media_types.rs      — file-extension → media-type lookup
    │
    └── src/ui/app/  [Zaparoo.App QML module]
          Main.qml          — runtime router: input, persistence, transitions,
                              "Loading…" overlay, system-cover prefetch
          MainLayout.qml    — designer-editable visual tree, pendingTransition
                              property, screen-state derivations, modal mounts
          │
          ├── src/ui/screens/  [Zaparoo.Screens QML module]
          │     ScreenManager.qml, HubScreen.qml, SystemsScreen.qml,
          │     GamesScreen.qml
          │
          ├── src/ui/components/  [Zaparoo.Ui QML module]
          │     Tile.qml, TileLoader.qml, PagedGrid.qml,
          │     ActiveLabel.qml, LoadingIndicator.qml, StatusIcon.qml,
          │     TopStatusStrip.qml, Modal.qml, ScreenStateOverlay.qml
          │
          └── src/ui/theme/  [Zaparoo.Theme QML module]
                Sizing.qml  — pctH/pctW/fontSize singletons
                Theme.qml   — colors and font-family constants
```

## QML module URIs

| Target | URI | Load path |
|---|---|---|
| zaparoo_frontend_rs (plugin) | `Zaparoo.Browse` | `qrc:/qt/qml/Zaparoo/Browse/` |
| zaparoo_ui_app | `Zaparoo.App` | `qrc:/qt/qml/Zaparoo/App/` |
| zaparoo_ui_screens | `Zaparoo.Screens` | `qrc:/qt/qml/Zaparoo/Screens/` |
| zaparoo_ui_components | `Zaparoo.Ui` | `qrc:/qt/qml/Zaparoo/Ui/` |
| zaparoo_ui_theme | `Zaparoo.Theme` | `qrc:/qt/qml/Zaparoo/Theme/` |
| zaparoo_update_qml (optional) | `Zaparoo.Update` | `qrc:/qt/qml/Zaparoo/Update/` |
| zaparoo-update (optional plugin) | `Zaparoo.Update.Native` | `qrc:/qt/qml/Zaparoo/Update/Native/` |

`engine.loadFromModule("Zaparoo.App", "Main")` is the only entry point. Keep
`qrc:/` strings out of the rest of the app.

The Update bounded context is enabled by default through CMake's
`ZAPAROO_WITH_UPDATE` option. CMake opts out of Cargo default features and adds
the `zaparoo-frontend-rs/update` feature explicitly when enabled; build with
`-DZAPAROO_WITH_UPDATE=OFF` to skip the `Zaparoo.Update` QML module and omit the
Hub Update tile.

## Key constraints

- **Software rendering only.** MiSTer has no GPU. Do not use shaders,
  `LinearGradient`, `RadialGradient`, `DropShadow`, `Glow`, `OpacityMask`,
  `MultiEffect`, or `Qt5Compat.GraphicalEffects`. Use `Rectangle`, `Image`,
  `Text`, `Repeater`, `NumberAnimation`, and `ColorAnimation`.

- **Resolution-agnostic layout.** The UI runs from 240p CRT output to 1080p.
  Use `Sizing.pctH()`, `Sizing.pctW()`, and `Sizing.fontSize()` for
  dimensions. Do not hardcode pixel values.

- **Dynamic Qt on desktop, static Qt on MiSTer.** `BUILD_SHARED_LIBS=ON` is
  the default for LGPL-compliant desktop distribution. The ARM32 Docker build
  passes `-DBUILD_SHARED_LIBS=OFF` through the Qt CMake toolchain.

## Runtime vs Platform

The frontend tracks two separate facts. Do not collapse them; that is how old
runtime/platform bugs come back.

| Concept | Source of truth | Question answered |
|---|---|---|
| **Runtime** | `zaparoo_core::runtime::current()` (filesystem-cached) | What device is the **frontend binary** running on? |
| **Platform** | `zaparoo_core::platform::subscribe()` (from `version` RPC) | What OS/device is **Zaparoo Core** running on? |

`Runtime == Mister` does **not** imply `Platform == Mister`. The frontend
can run on a desktop while talking to Core on a MiSTer on the network,
or vice-versa.

### When to use which

- **Runtime gate** — use this when the frontend's host device changes the
  behavior. Read `runtime::current()`. Prefer runtime gating for behavior.
- **Build-time cfg `#[cfg(zaparoo_runtime = "mister")]`** — use this only for
  code that should not compile into desktop binaries: system calls,
  MiSTer-only dependencies, and similar. Currently only `mister_runtime.rs`
  uses it. `ZAPAROO_RUNTIME=mister` is set in `cmake/ZaparooRust.cmake` for
  static-Qt ARM32 builds.
- **Platform gate** — use this when a feature depends on what Core supports.
  Subscribe to `platform::subscribe()` and treat `None` as unknown; do not
  enable platform-specific behavior until the first `version` RPC completes.
  Do not gate on `Platform` directly from C++ or QML. Route the decision
  through Rust and expose a QML property.

**Never gate runtime behavior on `Platform`, never gate Core
assumptions on `Runtime`.** They are independent.

## LGPL compliance

Qt is used under LGPLv3. The desktop binary links Qt dynamically, so end
users can replace the bundled Qt libraries. The MiSTer ARM32 binary is
statically linked; object files are available on request per LGPL §4(d)(1).
License texts live in `src/LICENSES/`.

## Rust → QML data flow

The data layer follows the RTK Query shape. A single `Store` owns the `Client`,
hands out shared `RemoteResource<T>` values keyed by `(endpoint NAME, args
hash)`, and routes mutations through the same client. QML singletons subscribe
by binding to an `Endpoint`; `rust/frontend/src/bind.rs` emits the bridge code
for the sync seed, the `qt_thread` watcher, and the property apply step.

```
zaparoo_rust_init()
    │
    ├── logger::install()          — tracing-subscriber (stderr + JSONL file)
    ├── Config::load()             — frontend.toml
    ├── tokio::Runtime::new()      — multi-thread executor
    ├── Client::new(endpoint)      — WebSocket JSON-RPC, auto-reconnects
    └── Store::new(client, runtime)
          │
          │   subscribe::<E>(args) → Arc<RemoteResource<E::Output>>
          │     ─ keyed cache: identical args reuse the same Arc
          │     ─ per-entry watcher updates `provides` on each Ready
          │
          │   run_mutation::<M>(args) → invalidates matching tags,
          │     each refetch pulses Notify on its RemoteResource
          │
          ├── CatalogEndpoint           Args = ()        provides: any("Catalog")
          │     └── bound by AppStatus, CategoriesModel, SystemsModel
          │           via `bind_to_endpoint!`
          │
          ├── MediaSearchEndpoint       Args = SystemId  provides: specific("MediaSearch", id)
          │     └── GamesModel::set_system subscribes per-system; the
          │         store keys cache entries by id so re-selecting a
          │         system reuses its cached resource without re-fetching
          │
          └── RunMutation               Args = RunParams  invalidates: ()
                └── GamesModel::launch_at → store.run_mutation::<RunMutation>
                      Today no tags are invalidated; future
                      NowPlayingEndpoint can opt in by adding its tag.
```

`RemoteResource<T>` combines the connection FSM and per-fetch state into one
`ResourceStatus<T>`: `Idle`, `Loading`, `Ready(T)`, or
`Errored { message, retrying }`. Each binding reads the current status
synchronously before it spawns the watcher. That closes the MiSTer race where
Core can connect before QML loads and the first screen never updates.

The Qt message handler (`qInstallMessageHandler`) forwards Qt log output to
`zaparoo_log_qt()` in the Rust staticlib. From there it goes through the same
tracing registry as Rust logs. Both end up in stderr and `frontend.log`.

### Navigation state

`Main.qml` extends `MainLayout.qml`. The layout owns the visual tree; `Main.qml`
owns the runtime wiring: key translation, forward-transition orchestration, and
persistence. Screens live under `Zaparoo.Screens` so they can be tested without
embedding the whole application shell.

```
ScreenManager.activeScreen:    "hub" | "systems" | "games"
ScreenManager.modalStack:      list<string>      // top-of-stack receives input
MainLayout.pendingTransition:  "" | "systems" | "games"   // owned by Main.qml
```

Persisted state is split across Rust-backed QML singletons:

| Singleton | Stored fields | Owner |
|---|---|---|
| `Browse.AppState` | `active_screen` | cross-screen route |
| `Browse.HubState` | `category` | hub-screen row selection |
| `Browse.SystemsState` | `system_id` | systems-screen grid selection |
| `Browse.GamesState` | `system_id`, `game_path` | games-screen grid selection |

State is loaded before the first QML frame and written through on user actions.
That is deliberate: MiSTer's parent process can kill and relaunch the frontend
without warning. Each screen writes its own `*State` singleton on directional
moves; the router writes `AppState.active_screen` when the screen flips.

#### Shared focused-detail policy

List-detail media screens use `FocusedMediaDetailController.qml` as the
single policy point for focused metadata and cover behavior. New media
list screens should reuse that controller instead of open-coding detail
debounce, clearing, or repeat-scroll rules in the screen.

The shared contract covers:

- selection identity tracking (`system_id + path`, or a screen-supplied equivalent)
- debounced detail loading
- stale-request suppression
- transient detail clearing
- rapid vertical repeat behavior: while held `up` / `down` repeat is active in
  list mode, metadata and screenshots stay hidden; title may continue to follow
  selection, and detail reload resumes after repeat stops

`MediaListScreen.qml` is the shared list/detail shell for media browse
screens. Favorites and Recently Played use it directly; Games layers its
folder-navigation, per-level persisted selection, and pagination rules on
top of the same shell instead of duplicating the list/detail render tree.
New media list screens should extend that component first and only add
screen-specific hooks where the data model or navigation semantics differ.

#### Screen flow

- **Hub** (`HubScreen.qml`) — two centered rows: category tiles plus action
  tiles for Favorites, Recently Played, optional Update, and Settings.
  Left/Right cycles within the focused row and writes `HubState`. Accept
  emits the matching forward/action signal; Escape emits `requestQuit`.
- **Update** (`zaparoo-update` package `qml/UpdateScreen.qml`) — crate-owned update screen
  with a Rust-driven progress value surfaced through `Zaparoo.Update.Native`.
  Accept is a no-op for now; Escape emits `requestHubScreen`.
- **Systems** (`SystemsScreen.qml`) — paged grid of systems for the active
  category. Directional moves write `SystemsState.system_id`. Accept on a
  Ready system emits `requestAccept(systemId)`; Accept on Empty/Error emits
  `requestAccept("")` so the router can re-fire `set_category` as a retry.
  Escape emits `requestHubScreen`. Tab on a tile emits
  `requestSystemCardWrite(index)`.
- **Games** (`GamesScreen.qml`) — paged grid of games for the active system.
  Accept on Ready calls `GamesModel.launch_at(index)`; Accept on Empty/Error
  re-fires `set_system` against the cached `current_system_id` as the retry.
  Escape emits `requestSystemsScreen` (the router decides whether to land on
  Hub or Systems via `_gamesEnteredFromHub`). Tab on a tile emits
  `requestGameCardWrite(index)`.

#### Forward-transition orchestration

`Main.qml` is the single owner of forward routing. Screens are pure input
dispatchers and never call `set_category` / `set_system` themselves. The
router's flow on a Hub Accept:

1. Set `pendingTransition = "systems"` (tentative). The screen's
   `transitioning` binding flips true and the source row/grid hides.
2. `_ensureCategory(category, cb)` short-circuits when the model is already
   on that category with `count > 0`; otherwise it parks `cb` in the
   `_categoryReadyCallback` slot, restarts a 50 ms `deferredCategorySetTimer`,
   and that Timer calls `SystemsModel.set_category(...)`. The defer is
   essential: `set_category` runs synchronously on the GUI thread and tears
   down `SystemsScreen`'s tile delegates, which freezes the frame budget if
   the "Loading…" cue hasn't painted yet.
3. The router's `Connections { target: Browse.SystemsModel }` fires
   `onLoadingChanged`. When `loading` flips false, the router pulls the
   stored callback, clears the slot, and runs it.
4. Inside that callback the router decides: Arcade-bypass on MiSTer (one
   system, drill straight to Games) or normal Hub→Systems. Arcade-bypass
   re-uses the same machinery via `_ensureSystem(systemId, cb)` against
   `GamesModel`. Hub→Systems calls `_prefetchSystemCovers(cb)` to warm the
   `QPixmapCache` so the destination grid paints with logos in place.
5. `_completeTransition(screen)` clears `pendingTransition` and calls
   `_goto(screen)` which writes `AppState.active_screen`.

Input is gated during the wait: `handleAction` early-returns when
`pendingTransition !== "" && !ScreenManager.hasModal` so a user mashing keys
during the load can't queue a second transition.

There are exactly two `loadingChanged` listeners — one per browse model —
both on the router. There is no cross-screen `Connections` block. There is
no per-screen pending flag. The class of routing bug where a stale
per-screen flag fires while its owning screen isn't even visible cannot
exist when there is no cross-screen state.

Model reset handlers in `Main.qml` restore saved row/grid indices as
catalog data arrives. Missing IDs fall back to index 0 without erasing the
saved value from disk, so a temporary catalog gap does not destroy the
user's last selection.
