# Slint state types

Closed UI discriminants belong in `rust/frontend-slint/ui/state_types.slint`.
`app.slint` re-exports host-facing enums so the generated setters, getters, and
callbacks take the same types as Rust routing code. Do not add string or integer
conversions merely to keep old routing comparisons working.

## Audit coverage

The audit covered every `.slint` file in `rust/frontend-slint/ui`, Rust producers
and consumers, the snapshot binary, and the MiSTer dual-head mirror/tests.

| Area | Typed state |
| --- | --- |
| Routing | `Screen`: active screen, pending destination, route component, Rust route parameters, retained-source checks, mirror layout identity |
| Browse | `GamesMode`, `SystemsMode`, `ContentState`: list source and ready/loading/empty/error cues |
| Settings | `SettingsPage`, `RowKind`, `ControlKind`, `ActionStatus`: root/subpage, outgoing page, field controls and job status |
| Setup | `SetupKind`, `SetupPicker`, `ScopeKind`, `SetupAction`: job, picker, scope label and help action; Rust selection stores domain `Scope` with its ID payload |
| Status | `StatusKind`, `AppCue`, `DisabledReason`: status ladder, transient launch cue and disabled Hub captions |
| Dialogs | `DialogKind`, `ErrorKind`, `FirstRunPhase`, `DialogProgress`, `DialogButton`: kinds, phase, progress and decisions, separate from text payloads |
| Input | `PressOwner`, `ScrollAction`: delayed-accept ownership, overlay pointer dispatch and Details scroll commands |
| Display | `Orientation`, `VideoStandard`: UI selection; orientation stays typed through scene sizing and live backend updates |
| Log upload | `LogPhase`: uploading, done, failed |

UI-owned browse and first-run enums are reused directly by Rust. Toolkit-free
rules (`zaparoo-app`) retain their own enums; exhaustive `From` matches in
`rust/frontend-slint/src/state_types.rs` project them into Slint types without
string round trips.

## Text and numeric boundaries

Not every repeated literal is state. These remain intentionally primitive:

- Display text, error detail, descriptions, versions, URLs, filenames, and paths.
- Core/system/scraper/media IDs, registry field IDs, setting values, and dynamic
  menu IDs. Generic registries and pickers carry data; they are not screen-state
  storage. Closed control kinds and page selections are typed separately.
- Translation/resource keys, including controller style directories and glyph
  filenames. The glyph resolver accepts resource names, not only today's files.
- Focus identity strings combining page/category/title and generation; card-write
  ticket keys. These identify changing content, not a finite list of modes.
- Raw keyboard text and Core input-action tokens at the input boundary.
- Numeric positions, indices, counts, geometry, progress, generations, palette
  preview indices, and signed pixel/page motion deltas used in arithmetic.
  Do not use numeric values as unrelated mode tags.
- Persisted/config/API tokens. Their existing spelling is unchanged. Decode at
  the boundary, handle unknown values explicitly, and do not change the disk
  schema to serialize generated enum ordinals or debug names.

`StateTokens` is only for vocabulary lookup and composite focus identity. Rust
`token()` methods likewise serve serialized/vocabulary/logging boundaries, not
routing. `TryFrom<&str>` rejects unknown tokens. Display seeding preserves the
existing horizontal/NTSC fallback; the shared error queue intentionally accepts
future kinds, which project to `ErrorKind::Generic` without losing queue context.

`SettingsPage::Root`, `PressOwner::None`, and `DialogKind::None` replace meaningful
empty-string sentinels. `Screen::None` represents an intentionally unmounted
screen (used by standalone overlay snapshots/tests); normal startup explicitly
selects Hub. The pending screen is meaningful only while `Shell.transitioning`
is true.
Defaults are explicit rather than depending on enum declaration order.

## Verification

- `just lint-slint` and `just test-slint` cover desktop and MiSTer builds, routing,
  settings categories, input ownership, software-rendered motion, and mirrors.
- Boundary tests cover legacy token round trips, unknown-token rejection,
  settings/error vocabulary coverage, defaults, display fallback without disk
  rewrites, and typed modal state copied to the CRT component.
- Optional snapshot binary: `just slint-snapshots` exercises fixture adapters;
  it is not included by default-feature compilation.
- Run `just slint-tr-extract` after moving translated strings: the generated POT
  tracks source locations even when message text is unchanged.
