# Translations

Zaparoo Frontend sends every user-visible string through Qt's `QTranslator`.
Non-English builds should not need code changes. The pipeline has three parts:

1. `qsTr()` in QML and `tr()` in C++ at every user-visible call site.
2. `src/ui/translations/frontend_<tag>.ts` as the canonical catalog
   (one per locale, XML, checked into git).
3. `qt_add_translations` in `cmake/ZaparooRust.cmake`, which runs
   `lrelease` at build time and bundles the resulting `.qm` files
   under `qrc:/i18n/` inside the frontend binary.

At runtime, `src/app/main.cpp` installs a `QTranslator` before the QML engine
loads, then picks the `.qm` file for the configured locale.

## Locale resolution

| Source | Precedence |
|---|---|
| `[general] language = "ja_JP"` in `frontend.toml` | 1: explicit override |
| `[general] language = "auto"` or unset | 2: `QLocale::system()` |

The Rust config loader (`rust/zaparoo-core/src/config.rs`) normalizes `"auto"`
(case-insensitive) to an empty string. `main.cpp` treats that as the signal to
call `QLocale::system()`. Anything else passes through to `QLocale(tag)` and Qt
handles tag validation.

The config-to-C++ handoff goes through FFI. `zaparoo_rust_language_code()`
returns a `'static` NUL-terminated UTF-8 pointer cached in
`LANGUAGE_CODE: OnceLock<CString>`. The main thread reads it once before
constructing the QML engine. There is no reload path; changing the locale takes
a relaunch, like any other `frontend.toml` edit.

## Writing translatable strings

Every literal a user might read belongs in `qsTr()`:

```qml
// Good: translator can reorder the units.
text: qsTr("%1 FPS").arg(root.fps)

// Good: entire sentence is one translation unit.
text: qsTr("Core error: %1").arg(Browse.AppStatus.last_error)

// Bad: splits the sentence; German, Japanese, etc. can't reorder it.
text: qsTr("Core error:") + " " + Browse.AppStatus.last_error
```

Rule of thumb: one sentence, one `qsTr()`. Use `%1` and `%2` placeholders for
runtime values so translators can control word order.

Strings that never face a user do **not** need wrapping: enum tags, filesystem
paths, QRC URLs, internal error codes routed to `tracing::error!`, QML type
names, and similar. If it could appear in a screenshot, wrap it.

## Adding a new locale

1. Copy the English catalog:

        cp src/ui/translations/frontend_en.ts src/ui/translations/frontend_de.ts

2. Run `lupdate-qt6` against the full source tree so every `qsTr()`
   call site populates the new catalog:

        lupdate-qt6 src/ -ts src/ui/translations/frontend_de.ts

   `lupdate` is idempotent. Re-running it adds new strings and marks removed
   strings without clobbering existing translations.

3. Fill in each `<translation>` element in the `.ts` file. An empty
   `<translation>` with `type="unfinished"` falls back to the source string;
   Qt Linguist highlights these in the editor UI.

4. Add the file path to the `TS_FILES` list in
   `cmake/ZaparooRust.cmake`'s `qt_add_translations(frontend ...)`
   call. CMake will pick up the `.qm` on the next build.

5. Configure a dev frontend to test it:

        # frontend.toml
        [general]
        language = "de"

## Updating existing catalogs

After adding or changing any `qsTr()` call, re-run `lupdate-qt6`:

    lupdate-qt6 src/ -ts src/ui/translations/frontend_en.ts

Review the diff. New strings appear with empty `<translation>` elements,
changed strings are flagged `type="unfinished"`, and removed strings are marked
`type="obsolete"`. Commit the `.ts` changes with the QML or C++ edit that
triggered them so translators get a clean incremental diff.

## Build-time mechanics

`qt_add_translations` is called in `cmake/ZaparooRust.cmake` right
after the `frontend` target is created:

```cmake
qt_add_translations(frontend
    TS_FILES "${CMAKE_SOURCE_DIR}/src/ui/translations/frontend_en.ts"
    RESOURCE_PREFIX "/i18n"
    IMMEDIATE_CALL
)
```

The build does this:

- `lrelease` runs per `.ts` to produce `<name>.qm` in the build tree.
- The `.qm` files are packed into a Qt resource bound to the
  `frontend` target at `qrc:/i18n/`.
- An `update_translations` target is registered for `cmake --build .
  --target update_translations`, which runs `lupdate` on demand.
- A `release_translations` target is also registered and wired into
  the default build, so every `just build` refreshes the compiled
  catalogs.

`IMMEDIATE_CALL` runs source-target collection inline. Without it, Qt defers
collection to the end of the top-level `PROJECT_SOURCE_DIR` scope. That races
Corrosion's late-bound Rust staticlib targets on parallel builds and can
produce missing-dependency errors.

## Runtime loading

`src/app/main.cpp` wires the translator before the QML engine:

```cpp
const QString langCode = QString::fromUtf8(zaparoo_rust_language_code());
const QLocale locale = langCode.isEmpty() ? QLocale::system() : QLocale(langCode);
QTranslator translator;
if (translator.load(locale, "frontend", "_", ":/i18n")) {
    QCoreApplication::installTranslator(&translator);
}
```

`QTranslator::load` uses Qt's normal fallback chain: exact `ja_JP` match, then
`ja`, then the base name. A missing `.qm` is logged at info level, not error
level. English-only builds ship one passthrough catalog (`frontend_en.qm`), and
other locales fall through to the source strings.

## Requirements

- **Desktop build**: `qt6-qttools-devel` (Fedora) or `qt6-tools-dev` +
  `qt6-l10n-tools` (Debian). These provide the `Qt6LinguistTools`
  CMake package plus the `lupdate` and `lrelease` binaries. Configure
  fails early if either is missing.
- **ARM32 cross-build**: `qttools` is built for host Qt in
  `Dockerfile.toolchain` (host-only; `.qm` files are
  architecture-independent and bundled into the target resource by
  the host build).

## Translators

| Locale | Translator |
|---|---|
| Italian (`it_IT`) | Andrea Bogazzi ([@asturur](https://github.com/asturur)) |
| Spanish (`es_ES`) | Carlos R. ([@crodriguezdominguez](https://github.com/crodriguezdominguez)) |
| Basque (`eu`) | devilschile2 |

Translators are added to this table when their `.ts` file is merged. Names
also appear on the About screen so end users see them.
