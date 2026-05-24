# Zaparoo Frontend — Designer Guide

This directory lets a UX designer open the frontend UI in **Qt Design Studio**.
Nothing under `design/` is compiled into the frontend binary; it exists only
for design-time previews.

## Setup

1. **Install Qt Design Studio.** It is free and GPLv3. The simplest route is
   the Qt online installer (<https://www.qt.io/download-qt-installer>); select
   *Qt Design Studio*. Some Linux package managers ship it as
   `qt-design-studio`.
2. **Build the frontend once** from the repo root so the generated
   QML modules exist:

   ```sh
   just build
   ```

   This populates `build/qml/Zaparoo/{App,Ui,Theme}/` with the real QML files.
   Design Studio reads them from there. The Rust-backed `Zaparoo.Browse` module
   is replaced by mocks under `design/mocks/`.
3. **Open the project** in Qt Design Studio:

   ```text
   design/frontend.qmlproject
   ```

   The 2D view should render `MainPreview.qml` at 1280×720 with the
   populated categories row and systems grid driven by the mock
   singletons. If they are empty, rebuild the frontend and reopen the
   project.

## Editable files

| Path                            | Touch?                                                     |
| ------------------------------- | ---------------------------------------------------------- |
| `src/ui/theme/Theme.qml`        | Yes — colour and font constants.                           |
| `src/ui/theme/Sizing.qml`       | Logic tweaks only; ask first.                              |
| `src/ui/components/*.qml`       | Yes — tile delegates, paged grid, FPS counter.             |
| `src/ui/app/MainLayout.qml`     | Yes — screen layout, backgrounds, anchors.                 |
| `src/ui/app/Main.qml`           | **No.** Engineer-owned state machine. Ask before editing.  |
| `design/mocks/**`               | No. Design-time stubs; edit only if engineering asks.      |
| `design/previews/MainPreview.qml` | Change preview canvas here; don't add real UI.           |

## Software rendering only

The frontend runs on MiSTer FPGA, which has **no GPU**. Anything shader-backed
can crash or render as a grey box. Stay within this set:

Allowed:
`Rectangle`, `Image`, `Text`, `Repeater`, `Item`, `NumberAnimation`,
`ColorAnimation`, `Behavior`.

**Do not use these from the Design Studio Components panel:**

- `LinearGradient`, `RadialGradient`, `ConicalGradient`
- `DropShadow`, `Glow`, `InnerShadow`
- `OpacityMask`, `ColorOverlay`, `FastBlur`, `GaussianBlur`
- `MultiEffect`, anything from `Qt5Compat.GraphicalEffects`
- Qt Quick **Studio Components**: `Pie`, `Arc`, `Triangle`,
  `Regular Polygon`, `Star`, `Svg Path Item`
- Any shader‑backed effect

If an effect seems necessary, talk to an engineer first. A flat `Rectangle` or
`Image` version is usually the safer MiSTer-friendly option.

## Sizing

The frontend scales from 240p CRT output to 1080p. Use the helpers on the
`Sizing` singleton (`import Zaparoo.Theme`). Do not hardcode pixel values or
element counts:

- `Sizing.pctH(n)` — `n` percent of screen height.
- `Sizing.pctW(n)` — `n` percent of screen width.
- `Sizing.fontSize(n)` — percent-of-height font size, floored at 8 px.
- `Sizing.visibleCovers` — element count for tile rows and similar
  repeaters; drops at very low resolutions to avoid crowding.

On the 1280×720 designer canvas, `Sizing.pctH(10)` previews as 72 px.

## Handing work back

1. Commit your changes to a branch (`design/<feature>`).
2. Open a PR, or hand the `.qml` diffs to an engineer.
3. Do not edit `CMakeLists.txt`, `.cpp`, `.rs`, or anything under
   `rust/` — those are engineering concerns.

## If Qt Creator's Design tab is greyed out

That is expected. Qt Creator 6+ ships with its QML visual designer disabled
because Qt Design Studio replaced it. Open this project in **Design Studio**,
not Creator.

## Troubleshooting

- **Red error banners on `Zaparoo.Browse.*`** — `build/qml/` is missing or
  stale. Rerun `just build`.
- **Red error banners on `Zaparoo.Ui` / `Zaparoo.Theme`** — same cause;
  `just build` populates those too.
- **Tile row/grid is empty** — the mock ListModels seed four entries. If
  the categories row or the systems/games grid renders empty, `mocks/`
  is not being resolved. Check that `importPaths` in `frontend.qmlproject`
  still lists `mocks` first.
- **"Cannot find type XYZ"** — probably a Qt Quick Studio Component. Do not
  use it; see the banned list above.
