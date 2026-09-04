# QML-to-Rust extraction before the Slint migration

## Decision

Extract a focused set of toolkit-independent rules before replacing Qt/QML with
Slint. Do not complete a broad rewrite of QML architecture in Rust first.

Committed scope:

1. Sizing rules and geometry tables.
2. Palette generation.
3. Browse layout tables.
4. Pure grid-navigation algorithms.

After those stages, begin the Slint migration. Anything else must independently
pass the extraction gate below; it is not a prerequisite.

This document is the master plan. It supersedes the earlier local plans under
`docs/plans/`.

## Why this remains worthwhile

`zaparoo-core` is already portable. QML, C++, and their toolkit-bound tests will
not survive the frontend swap. Moving stable application rules and their tests
into `rust/zaparoo-app` preserves behavior across both frontends and turns the
current implementation into an executable specification for Slint.

Extraction is not valuable merely because code moves to Rust. Adapter code in
`rust/frontend` is still Qt-specific and will be discarded. Success is measured
by portable behavior and portable tests, not total line reduction.

## Current status

### Sizing extraction

Implementation exists in the current working tree:

- `rust/zaparoo-app` workspace crate and toolkit guard.
- Sizing rules in `rust/zaparoo-app/src/sizing.rs`.
- cxx-qt adapter in `rust/frontend/src/models/sizing.rs`.
- Stable QML facade in `src/ui/theme/Sizing.qml`.
- Golden fixture in `tests/fixtures/sizing_golden.txt`.
- Rust parity tests and QML binding-invalidation tests.

Automated checks completed:

- `just lint`
- `just test`
- `cargo test -p zaparoo-app`
- `scripts/check-toolkit-free.sh`

Before merging, still perform:

- Desktop resize and rotation smoke test.
- MiSTer 240p/software-renderer check.
- Cold-boot comparison on MiSTer for obvious startup regression.

No palette, browse-layout, grid, input, router, menu, or screen-reducer work has
started.

## Extraction gate

Move behavior before Slint only when every answer is yes:

1. **Toolkit-independent:** implementation needs no Qt, QML, Slint, renderer,
   object lifecycle, or toolkit event type.
2. **Stable contract:** behavior is product policy or deterministic math, not a
   workaround for current QML structure.
3. **Pinned behavior:** current output can be captured before porting through a
   fixture or independent tests.
4. **Reusable shape:** Slint can call substantially the same Rust API without
   preserving a Qt-shaped adapter contract.
5. **Lower total risk:** extracting and validating now costs less than
   reimplementing and validating during the Slint migration.

Failing any item means defer the work. Small pure functions may still move
opportunistically when touched, but they do not justify a dedicated migration
PR.

## Architecture

```text
rust/
  zaparoo-core/    Core protocol, store, persistence, configuration
  zaparoo-app/     Toolkit-independent application rules
  frontend/        Qt/cxx-qt adapter and runtime shell
  build-info/
  mock-core/
```

A future Slint frontend consumes `zaparoo-core` and `zaparoo-app` through its own
adapter. It does not reuse `rust/frontend`.

### Boundary rules

- `rust/zaparoo-app` contains no `cxx*`, Qt, QML, Slint, toolkit strings, toolkit
  containers, UI-thread handles, or toolkit object types.
- `scripts/check-toolkit-free.sh` enforces that boundary in `just lint`.
- Application functions accept ordinary Rust values and return ordinary Rust
  values, enums, or structs.
- Toolkit conversion belongs in adapter crates.
- QML and later Slint own translated sentences. Rust returns stable IDs,
  structured values, and translation arguments—not localized prose.
- State machines receive clocks as inputs. They do not call the system clock.
- Application logic declares timer effects; toolkit shells own timers.
- Do not move code solely to improve crate organization. Already-portable code
  can move during Slint work unless relocation unlocks a concrete stage here.

### QML facade rule

During interim extraction, retain the public QML singleton/component API. QML
call sites should not churn merely because implementation moved.

cxx-qt invokables do not register QML binding dependencies. Therefore:

- Values read by bindings cross as notifying properties.
- One-line helpers called with literal arguments inside bindings stay in QML.
- Facade wrappers pass every configuration dependency as a real invokable
  argument. Discarded property reads are unsafe because the QML compiler may
  remove them.
- If a derived Rust output feeds a Rust input through QML, prefer one QML
  binding rather than publishing a second stateful copy.
- Batched derived-value publication must tolerate re-entrant setters because
  change signals evaluate dependent bindings synchronously.

`Sizing.qml` and `models/sizing.rs` are the reference implementation.

### Bridge rule

Prefer scalar invokable arguments and results. When several values form one
application concept, use a Rust struct internally and expose explicit adapter
properties or scalar accessors. Toolkit types such as `QColor` may exist in
`rust/frontend`, never in `zaparoo-app`.

Do not reproduce delimiter-encoded pseudo-structures.

## Delivery sequence

Each stage is independently shippable and behavior-preserving. Finish its
acceptance checks before starting the next stage. Reassess the extraction gate
after every stage.

### Stage 1: sizing rules

Status: implemented locally; awaiting manual and hardware checks, review, and
merge.

Scope:

- Establish `rust/zaparoo-app` with no dependencies.
- Add and wire the toolkit-free guard.
- Port sizing tables, tier ladders, Hub geometry, grid-shape selection, header
  geometry, and cover/logo decode tiers.
- Keep `pctH`, `pctW`, `px`, `stroke`, `center`, `half`, and `fontSize` in QML so
  literal-argument bindings continue tracking scene inputs.
- Publish derived values through notifying adapter properties.
- Preserve argument-taking facade helpers with explicit scene configuration.

Behavioral fixture:

- `tests/fixtures/sizing_golden.txt` contains 192 QML-authoritative
  configurations plus pure-function probes.
- `rust/zaparoo-app/tests/sizing_golden.rs` must reproduce it exactly.
- QML tests retain integration assertions: direct property writes, settings
  propagation, binding invalidation, rotation, rendering-path changes, and real
  grid geometry.

Acceptance:

- Existing QML API remains compatible.
- Rust fixture tests pass.
- Resize, rotation, and constant-argument binding probes pass.
- `just lint` and `just test` pass.
- Desktop and MiSTer checks show no visual or startup regression.

Lessons already incorporated:

- Rust singleton names must not shadow facade singleton names; use
  `SizingRules`, not `Sizing`.
- `detailCoverSourceWidth` remains a QML binding because it feeds viewport state
  back into sizing inputs.
- Adapter recomputation uses a pending-pass guard so nested setter calls cannot
  be overwritten by an older outer snapshot.

### Stage 2: palette generation

Target: `src/ui/theme/ColorSchemes.qml`.

Move only deterministic color math and preset data:

- sRGB/Oklab/OKLCh conversion.
- Gamut fitting and contrast calculations.
- Accent clamping and derived palette roles.
- Preset IDs and intensity-independent tables.

Keep `Theme.qml` as facade. `zaparoo-app` returns toolkit-neutral RGB/RGBA values;
`rust/frontend` converts them to `QColor` properties. Preview APIs may use scalar
lists or explicit indexed accessors in the Qt adapter.

Before porting:

1. Capture palette outputs for every preset and supported intensity.
2. Preserve independent contrast/luminance assertions rather than testing the
   implementation with its own helpers.
3. Pin preset ordering and all public palette roles.

Also remove the duplicated color-scheme ID table from settings once Rust owns
that ordering.

Acceptance:

- Every captured palette value matches.
- Existing theme and settings call sites remain unchanged outside the facade.
- Translation behavior remains in QML.
- `just lint` and `just test` pass.
- Desktop and MiSTer checks cover normal and dim intensities plus representative
  presets.

Stop if color conversion semantics cannot be pinned across QML and Rust without
visual drift.

### Stage 3: browse layout tables

Target: `src/ui/theme/BrowseLayouts.qml`.

Move static theme/profile tables and deterministic resolution into typed Rust
values. Do not preserve the string DSL as the internal Rust model. The adapter
may temporarily retain existing `boolValue`, `numberValue`, and `stringValue`
facade calls to avoid QML call-site churn.

Before porting:

1. Capture resolved values for every built-in theme, view, resolution tier,
   orientation, and interface profile used by the app.
2. Add explicit tests for references, fallback behavior, and cycle rejection.
3. Identify every consumer and confirm only the facade contract crosses the
   bridge.

Acceptance:

- Captured layouts match exactly.
- Typed Rust representation contains no QML expression strings.
- Existing QML layout-profile tests pass.
- `just lint` and `just test` pass.
- Desktop and MiSTer checks cover Hub, systems, games, settings, and detail
  layouts at 240p and one larger tier.

Stop if the generic path API forces `zaparoo-app` to become a QML object-model
interpreter rather than a typed layout resolver.

### Stage 4: pure grid navigation

Target: deterministic parts of `src/ui/components/PagedGrid.qml`.

Move pure free functions only:

- Page count and page-step decisions.
- First selectable item on a page.
- Jump-target normalization.
- Directional selection movement.
- Nearest vertical candidate scoring.

Keep in QML:

- Per-instance component state.
- Delegate and Loader lifecycle.
- Geometry mapping and pointer/wheel events.
- Materialization waits and watchdogs.
- Renderer-sensitive pagination behavior.

Transcribe integer/boolean navigation cases to Rust before replacing QML
functions. Keep QML tests that exercise real geometry, delegate lifecycle, mouse
input, or asynchronous materialization.

Emptiness must remain an explicit input to pure functions. Do not make
`zaparoo-app` reach into `zaparoo-core` globals or Qt models to infer it. Use a
compact toolkit-neutral representation only if needed; otherwise keep
Hub-specific padding logic outside the shared algorithm.

Do not bundle unrelated navigation implementations from context menus, pickers,
or screens into this stage. Those can adopt proven helpers later in small PRs.

Acceptance:

- Rust tests cover all moved branches and directional scoring cases.
- Surviving QML tests cover integration behavior.
- No per-grid QObject is introduced.
- `just lint` and `just test` pass.
- Desktop and MiSTer checks cover paging, wrapping, sparse Hub pages, and rapid
  repeated movement.

Stop if a function requires QML object lookup, pending Loader state, or
renderer timing to produce its answer.

## Transition to Slint

After Stage 4:

1. Review maintenance cost and defect history from all four stages.
2. Confirm `zaparoo-app` APIs fit Slint without Qt-shaped compatibility layers.
3. Start Slint shell and routing work using extracted rules as dependencies.
4. Move additional policy only when implementing the corresponding Slint
   feature, where both frontend contracts are visible.

Do not delay Slint work to complete the deferred catalog below.

## Deferred catalog

These were previously planned as dedicated interim PRs. They are now deferred
unless a smaller piece independently passes the extraction gate.

### Input state machine

Potentially reusable:

- Duplicate-input predicate.
- Repeatable-action predicate.
- Confirm/cancel and options-view swaps.
- Hold/repeat transitions with injected timestamps.

Defer broad modal/screen dispatch conversion, screenshot-based rapid navigation,
screensaver geometry, and QML timer rewiring. Design the complete input machine
while building the Slint event loop.

### Router decisions

Defer full interim router extraction. The current QML router has substantial
orchestration state, and the prototype showed that moving it to Rust does not
make it smaller or automatically testable.

If routing policy must be touched before Slint, extract only named pure
predicates with explicit inputs. Keep three distinct Arcade-bypass decisions
(forward, restore, and back); they intentionally have different conditions.
Never cache a live routing answer merely to fit a state machine.

### Menus and status labels

Defer dedicated menu-builder and status-line extraction. Their structure is
likely to change with Slint components and translation integration. Rust may own
stable action IDs and policy predicates, but view layers own ordering presentation
and translated sentences unless reuse is demonstrated.

### Per-screen reducers

Defer. Reducers can be valuable, but designing them around current QML pending
flags, focus plumbing, and component inheritance would preserve architecture
that Slint will not share. Define reducer state and effects alongside Slint
screens, then backport only when doing so materially reduces dual-frontend risk.

### Toolkit-free module relocation

Not a migration prerequisite. Existing Qt-free modules are already portable in
substance even when located under `rust/frontend`.

Any relocation must be split into focused work:

- Resolve Hub cover-manifest dependency direction.
- Preserve C ABI symbols through thin frontend forwarders.
- Reproduce MiSTer build configuration in the destination crate.
- Declare Tokio features directly instead of relying on workspace unification.
- Replace global runtime/store access through explicit seams.
- Centralize UI-thread posting behind the active frontend adapter.

Treat each as architecture work with an ARM32 build, not “housekeeping.”

### Pure C++ ports

Tint LUTs, baked icon parsing, SVG render sizing, and argument parsing can move
when Slint removes their Qt consumers or when an independent maintenance need
justifies it. Existing C++ tests already protect them; porting now offers little
migration leverage.

## Verification policy

Every behavior-moving stage:

1. Capture authoritative outputs before replacing implementation.
2. Add portable Rust tests that state product rules, not only fixture equality.
3. Keep toolkit tests only for integration behavior that Rust cannot observe.
4. Run `just lint` and `just test`.
5. Run `scripts/check-toolkit-free.sh` directly when changing the boundary.
6. Exercise affected screens with `just mock-core` and `just run-dev`; user
   drives the GUI.
7. Check geometry, palette, timing, or rendering changes on MiSTer at 240p.
8. Record anything skipped in the PR description.

### Golden-fixture maintenance

A parity fixture is evidence of old behavior, not a snapshot to update whenever
a test fails.

To regenerate one:

1. Start from the authoritative QML/C++ implementation before its replacement.
2. Add a temporary test-only dump covering the complete input matrix.
3. Run the narrow existing test binary and capture deterministic output.
4. Remove the dump function.
5. Commit the fixture and independent parser with the port.
6. In the PR, state why regeneration was necessary and review fixture changes as
   behavior changes.

For sizing, dump one flat `key=value` line per case from `tst_sizing.qml`, using
the logical values seen by `Sizing` after CRT inset and orientation handling.
The parser intentionally uses only the standard library.

## Risks and stop conditions

- **Toolkit leakage:** stop if `zaparoo-app` requires toolkit types or lifecycle.
- **Binding staleness:** stop and redesign if a QML binding cannot identify all
  notifying inputs.
- **Duplicated state:** stop if adapter publication creates feedback loops or two
  independently mutable copies of one value.
- **Qt-shaped application API:** defer work when pending flags, callback slots,
  Loader state, or screen inheritance appear in `zaparoo-app`.
- **Test migration without coverage:** do not delete QML tests until equivalent
  Rust coverage exists; do not retain mathematical QML duplication indefinitely
  without a stated integration reason.
- **Bridge cost dominates value:** stop a stage if most work is disposable
  adapter code and little reusable policy or test coverage results.
- **Slint schedule impact:** start Slint after Stage 4 even if deferred items look
  extractable.

## Progress measures

Track these per stage:

- Product rules now implemented in `zaparoo-app`.
- Authoritative cases covered by portable tests.
- Toolkit tests removed, retained for integration, or still duplicated.
- Public facade call sites changed.
- Adapter code added; count it as migration cost, not migrated value.
- Manual and MiSTer checks completed.
- Defects found by fixtures, bridge tests, or hardware checks.

Line-count reduction is secondary. Main outcome is a small, stable application
layer that both Qt and Slint can trust.