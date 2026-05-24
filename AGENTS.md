# Zaparoo Frontend Agent Guide

Zaparoo Frontend is a Qt/QML frontend for Zaparoo Core. It runs on desktop
Linux and on MiSTer FPGA (ARM32, Linux framebuffer, software rendering). The
MiSTer target is the hard constraint: assume no GPU, process kills without
notice, and a small ARM CPU.

Keep this file focused on commands, traps, and rules that are hard to infer
from the tree. Use the docs for longer explanations.

## Commands

Run every workflow from the repo root with `just`. Do not `cd rust/` and run
raw cargo as the default path; the justfile carries the expected environment.

| Task | Command |
|---|---|
| Desktop build | `just build` |
| Desktop run | `just run` |
| Dev run against mock Core | `just mock-core` in one terminal, `just run-dev` in another |
| Full test gate | `just test` |
| QML/C++ tests only | `just test-qml` |
| Rust tests only | `just test-rust` |
| Full lint gate | `just lint` |
| Rust lint only | `just lint-rust` |
| Format | `just fmt` |
| MiSTer ARM32 build | `just arm32` |
| Deploy to MiSTer | `just deploy-mister` |

`just --list` is the source of truth. `CMakePresets.json` and
`rust/.cargo/config.toml` are tuned for those recipes.

## Stack Facts

- Qt 6.7+ with Qt Quick, QuickControls2, QML, QuickTest, and LinguistTools.
- C++17 executable at `src/app/main.cpp`; Rust static library linked through
  Corrosion and cxx-qt.
- Rust workspace is under `rust/`, edition 2021, MSRV 1.90, cxx-qt 0.8.
- Desktop builds link Qt dynamically for LGPL compliance.
- MiSTer ARM32 builds use the Docker toolchain and static Qt.

## Always

- Keep comments and docs in American English.
- After editing C++, Rust, or QML, run `just lint`. Run `just test` when the
  change can affect runtime behavior.
- Keep user-visible state persistent. Selected screen, row/grid positions,
  focus, settings, and similar state must be serialized to disk and restored
  before the first frame. MiSTer's wrapper can kill and relaunch the process at
  any time.
- Wrap user-visible QML strings in `qsTr()` and C++ strings in `tr()`. Use
  `%1`/`%2` placeholders for runtime values so translators can reorder text.

## Ask First

- Before adding or changing a `Client` method in
  `rust/zaparoo-core/src/client.rs`, check the upstream API docs:
  <https://zaparoo.org/docs/core/api/>. Method names, params, and return types
  must match Core.
- Before changing `Sizing.qml` behavior or the persisted state schema, confirm
  the migration/reset behavior.
- Before adding dependencies, changing CI, or touching license/trademark text,
  confirm the intended policy.
- Before changing forward screen routing (`Main.qml` ↔ screens), see the
  "Screens and routing" rules below. Cross-screen Connections and
  per-screen pending flags are how this module bit us last time.

## Never

- Do not use shader-backed or GPU-dependent QML: `LinearGradient`,
  `RadialGradient`, `DropShadow`, `Glow`, `OpacityMask`, `MultiEffect`,
  `Qt5Compat.GraphicalEffects`, Qt Quick Studio shapes, or custom shaders.
  Stick to software-rendering-safe types such as `Rectangle`, `Image`, `Text`,
  `Repeater`, `Item`, `NumberAnimation`, and `ColorAnimation`.
- Do not animate properties that force a large dirty rectangle on busy content:
  no translucent (`opacity < 1`) overlays over a grid, no fading or scaling of
  a parent that contains many delegates, no slide-translation of a band of
  tiles. Qt Software-adaptation cost is dominated by *painted pixels per frame
  × per-pixel cost*, not by the animated property — a fading rectangle over 15
  tiles repaints all 15 tiles per frame, because translucent nodes do not
  subtract from the renderer's obscured region. Pick animations whose dirty
  rect is small (page-dot pulse, focus-ring blink, single-tile move) and let
  the rest of the scene stay static. See `docs/qml-gotchas.md` →
  "Software-renderer animation costs".
- Do not hardcode pixel sizes or fixed element counts in UI. Use
  `Sizing.pctH()`, `Sizing.pctW()`, `Sizing.fontSize()`,
  `Sizing.visibleCovers`, and `Sizing.cornerRadius` (for any rounded-square
  surface — see `docs/style.md`). Any value that drives `x`/`y`/`width`/
  `height`, border widths, margins, or font sizes must go through
  `Sizing.px()`, `Sizing.stroke()`, `Sizing.center()`, or `Sizing.half()`.
  The whole app must run cleanly at 240p; fractional geometry is a bug
  everywhere, not just on MiSTer.
- Do not center user-visible text via `anchors.horizontalCenter` +
  `Text.AlignHCenter`. Center the `Text` item itself with
  `Sizing.center()` and render its glyphs left-aligned (or pre-measure
  with `TextMetrics`). Glyph runs that straddle a half-pixel soften
  under any 240p rendering and aren't acceptable on any screen. See
  `docs/qml-gotchas.md` → "Integer-pixel rules".
- Do not add Qt5 compatibility code or `#if QT_VERSION` guards. This project is
  Qt 6.7+ only.
- Do not change `BUILD_SHARED_LIBS`. Desktop needs `ON`; the ARM32 toolchain
  sets static linking for MiSTer.
- Do not publish state with `tokio::sync::broadcast` when late subscribers need
  the current value. Use `tokio::sync::watch` for state and reserve broadcast
  for lossy events.
- Do not inline a `watch::Sender::borrow()` or any read guard in an `if let`,
  `match`, or `while let` scrutinee when the body writes to the same channel or
  lock. Bind the read in an inner scope first:
  `let next = { let cur = tx.borrow(); fsm.step(&cur) };`.
- Do not leave lint warnings, failing tests, or untranslated user-facing text
  behind.
- Do not put cross-screen `Connections` (e.g. `target: Browse.GamesModel` from
  `SystemsScreen.qml`) or pending-transition flags into screen files. Forward
  routing is owned by `Main.qml` — see "Screens and routing".
- Do not paint a full-screen background or a translucent overlay over a screen
  body. Source-screen content hides via `transitioning: true`; the global
  "Loading…" overlay is a transparent `Item` with one `Text` child.
- Do not persist Core metadata (cover art, scraped properties, descriptions,
  etc.) to disk or any user-visible cache. Zaparoo Core is the canonical store;
  the frontend caches in process memory only and re-fetches what it needs after
  a cold start. Any in-memory cache must enforce a strict bytes cap with LRU
  eviction — MiSTer has under 512 MB of shared system RAM and the frontend
  competes with Core, the FPGA wrapper, and the active core for it.

## Project Map

| Path | Purpose |
|---|---|
| `src/app/main.cpp` | Thin Qt entry point, translator install, QML engine, Qt log bridge |
| `src/ui/app/Main.qml` | Runtime router: input, persistence, forward-transition orchestration, global "Loading…" overlay, system-cover prefetch |
| `src/ui/app/MainLayout.qml` | Designer-editable visual tree, `pendingTransition` property, screen-state derivations, modal mounts |
| `src/ui/screens/` | `Zaparoo.Screens`: `ScreenManager`, `HubScreen`, `SystemsScreen`, `GamesScreen` |
| `src/ui/components/` | `Zaparoo.Ui`: `Tile`, `TileLoader`, `PagedGrid`, `ActiveLabel`, `LoadingIndicator`, `StatusIcon`, `TopStatusStrip`, `Modal`, `ScreenStateOverlay` |
| `src/ui/theme/` | `Zaparoo.Theme`: `Theme`, `Sizing` singletons |
| `rust/frontend/src/models/` | `Zaparoo.Browse` cxx-qt singletons: `AppStatus`, `CategoriesModel`, `SystemsModel`, `GamesModel`, `AppState`, `HubState`, `SystemsState`, `GamesState`, `Input`, `Runtime` |
| `rust/frontend/src/bind.rs` | Endpoint-to-QML binding macro with synchronous seed |
| `rust/zaparoo-core/src/client.rs` | WebSocket JSON-RPC client for Zaparoo Core |
| `rust/zaparoo-core/src/store/` | Endpoint cache, tags, mutations, invalidation |
| `rust/zaparoo-core/src/persist.rs` | Atomic persisted UI state (`HubState`, `SystemsState`, `GamesState`, `AppState`) |
| `rust/zaparoo-core/src/platform_paths.rs` | Config, log, and state paths per runtime |

QML module URIs are `Zaparoo.App`, `Zaparoo.Screens`, `Zaparoo.Ui`,
`Zaparoo.Theme`, and `Zaparoo.Browse`. Resources are embedded under
`qrc:/qt/qml/Zaparoo/App/resources/...`. `compile_commands.json` is generated
in `build/` by default.

## Screens and routing

The frontend has three peer root screens — `Hub`, `Systems`, `Games` — plus a
modal stack. Screens are **pure input dispatchers**: `handleAction` translates
a key/button to a single `requestAccept(payload)` (forward) or a back signal
(`requestHubScreen`, `requestSystemsScreen`, `requestQuit`). All forward
orchestration lives in `Main.qml`.

When adding a new screen or routing path, follow this contract:

1. **Forward = signal + payload, router decides destination.** Screens emit
   `requestAccept(<id-or-empty>)`. The router reads its own state to decide
   what comes next. Empty payload = "the press was on Empty/Error" or
   "row/grid was empty" — keep the existing convention, don't overload it
   for new meanings.
2. **Back = simple signal.** `requestHubScreen` / `requestSystemsScreen` /
   `requestQuit`. The router owns any peer-up logic (e.g. the
   `_gamesEnteredFromHub` Arcade-bypass back-routing flag).
3. **No cross-screen `Connections` in screens.** A screen must not listen to
   another screen's model. The router has one Connections block per model
   that needs a `loadingChanged` waiter, and uses a single-shot callback slot
   pattern (`_categoryReadyCallback`, `_systemReadyCallback`) — set the
   callback, fire it on the next non-loading edge, clear it.
4. **Source-screen content hiding goes through `transitioning`.** Each screen
   exposes `property bool transitioning: false`; `MainLayout.qml` binds it to
   `root.pendingTransition !== ""`. Bind the row/grid `visible:
   !screen.transitioning` so the live tiles hide while the global "Loading…"
   cue paints alone.
5. **Gate new input during a transition.** `Main.qml`'s `handleAction`
   early-returns when `root.pendingTransition !== "" && !ScreenManager.hasModal`.
   Don't add a second input gate elsewhere.
6. **Persisted state is per-screen, not bundled on `HubState`.** New screen
   selection state goes in its own `Browse.<Screen>State` singleton (cf.
   `HubState` / `SystemsState` / `GamesState`). The router orchestrates
   model fills (`set_category`, `set_system`); screens write their own state
   on directional moves.

The class of bug that this layout prevents: a stale pending flag on screen A
firing during model B's `loadingChanged` and clearing the router's
back-routing flag while screen A isn't even visible. There is no cross-screen
state to go stale because there is no cross-screen state.

## Runtime Notes

- `Runtime` answers where the frontend binary is running. `Platform` answers
  where Zaparoo Core is running. Do not collapse them.
- Desktop config: `~/.config/zaparoo/frontend.toml`.
- Desktop state: `~/.config/zaparoo/state.toml`.
- Desktop log: `~/.local/share/zaparoo/logs/frontend.log`.
- MiSTer config: `/media/fat/zaparoo/frontend.toml`.
- MiSTer state/log: `/tmp/zaparoo/state.toml`, `/tmp/zaparoo/frontend.log`.
- `ZAPAROO_CORE_ENDPOINT` overrides `[core] endpoint`; `ZAPAROO_STATE_FILE`
  redirects state for tests and ad-hoc runs.
- Debug logging is enabled with `[logging] debug = true` or `ZAPAROO_DEBUG=1`.

## MiSTer Deploy

`just deploy-mister` reads `MISTER_IP` from `.env`, builds the ARM32 binary,
copies it to `/media/fat/zaparoo/frontend`, restarts `/media/fat/MiSTer_Zaparoo`,
and clears `/tmp/zaparoo/frontend.log`.

`/media/fat/MiSTer_Zaparoo` is the integration binary shipped with MiSTer. It
starts our `frontend`; do not replace that flow with a new wrapper script.

## Further Reading

- `docs/architecture.md` — module graph, data flow, runtime/platform split
- `docs/building.md` — build matrix, ARM32 toolchain, deploy bundle
- `docs/qml-gotchas.md` — QML issues qmllint often catches late
- `docs/style.md` — corner-radius token, tile aspect, pill vs. sharp shapes
- `docs/cxx-qt-bridge.md` — cxx-qt 0.8 bridge constraints
- `docs/translations.md` — `qsTr()`/`tr()` pipeline and locale catalogs
- `design/README.md` — Qt Design Studio workflow and designer boundaries
- `src/LICENSES/` — Qt LGPL notices
