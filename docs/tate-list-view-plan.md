# TATE List View Plan

Best path: do not fork whole list screen for TATE. Keep one shell, split layout into strategy/profile pieces.

## Plan

1. Add list-detail orientation mode, not special-case soup.
   - New derived mode in `src/ui/screens/MediaListScreen.qml`: `standardList` vs `tateList`.
   - Trigger from `Browse.Settings.current_orientation !== "horizontal"` and `_listLayout`.
   - Keep input/state/detail loading in `MediaListScreen`; only geometry changes.

2. Break `src/ui/components/BrowseListDetailView.qml` into 3 stable pieces.
   - `BrowseListPane`
   - `BrowseDetailPane`
   - thin `BrowseListDetailView` compositor
   - Reason: current component already mixes list + detail geometry. TATE will get messy if we keep one giant anchoring file.

3. Introduce explicit layout profile object for list-detail composition.
   - Today profile mostly tunes sizes.
   - Expand it to include composition slots:
     - `listDetailAxis: "horizontal" | "vertical"`
     - `detailAxis: "horizontal" | "vertical"`
     - `listShare`, `detailShare`
     - `detailImageShare`, `detailMetadataShare`
     - gaps, margins, panel heights
   - Then theme can override profile values without rewriting component logic.

## Target TATE Layout

- Whole screen:
  - top: list
  - bottom: detail panel
- Inside detail panel:
  - left: preview image
  - right: metadata/text
- So:
  - outer compositor = vertical split
  - detail pane internals = horizontal split

## Maintainable Component Shape

- `MediaListScreen.qml`
  - owns navigation, selection persistence, retry/accept/cancel, focused-detail controller
  - decides which layout profile to pass down
- `BrowseListDetailView.qml`
  - only composes panes using profile
  - no business logic
- `BrowseListPane.qml`
  - row rendering, hover/click/current rect
- `BrowseDetailPane.qml`
  - image, title, tags, description
  - already close to reusable; keep pushing it there

## Theming Rule

Do theme with profiles, not `if (tate)` colors/fonts in components.

Suggested profiles in `src/ui/theme/BrowseLayouts.qml`:
- `defaultList`
- `crtList`
- `defaultTateList`
- `crtTateList`

Each profile defines only tokens. Components read tokens. No visual branching all over QML.

## Why This Stays Sane

- One navigation model
- One detail-loading model
- One list row implementation
- Two composition profiles
- TATE-specific work isolated to geometry/profile layer

## Implementation Order

1. Add `tateList` derived mode in `MediaListScreen`.
2. Refactor `BrowseListDetailView` so outer split direction comes from profile.
3. Refactor `BrowseDetailPane` so image/metadata split direction comes from profile.
4. Add `defaultTateList` + `crtTateList` tokens.
5. Tune text truncation/row count for portrait logical space.
6. Only after that, decide if favorites/recents need small per-screen overrides.

## Watchouts

- Keep integer geometry through `Sizing`; rotated portrait space will expose rounding bugs fast.
- In TATE, description area will shrink first. Make description optional/collapsible before shrinking metadata rows too hard.
- Context-menu anchor should still come from list cell rect only; no TATE special case if pane ownership stays unchanged.

## PR 156 Addition

This plan is the intended follow-up for [ZaparooProject/zaparoo-frontend#156](https://github.com/ZaparooProject/zaparoo-frontend/pull/156) after TATE mode lands.

- PR 156 is the right direction for theme/profile-driven tokens.
- PR 156 is not enough by itself for TATE list-detail layout, because `BrowseListDetailView.qml` still hardcodes the outer split as list-left and detail-right.
- After TATE merges, the refactor discussion for PR 156 should continue with one extra requirement: make the list/detail compositor profile-driven, not only size-driven.

Required additions on top of PR 156:

1. `BrowseListDetailView.qml` must support profile-driven outer axis selection.
   - Normal mode: horizontal split
   - TATE mode: vertical split
2. `BrowseDetailPane.qml` must support profile-driven inner axis selection.
   - Normal mode: current layout
   - TATE mode: preview image left, metadata right
3. `BrowseLayouts.qml` profiles should grow composition keys, not only spacing keys.
   - `listDetailAxis`
   - `listShare`
   - `detailShare`
   - `detailAxis`
   - `detailImageShare`
   - `detailMetadataShare`

Conclusion:

- Merge TATE mode first.
- Keep PR 156 as the token/theme foundation.
- Resume PR 156 with this TATE compositor refactor as the next steering point.
