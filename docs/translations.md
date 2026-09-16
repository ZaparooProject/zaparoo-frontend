# Translations

Every user-visible string goes through Slint's translation support. The
pipeline has three parts:

1. `@tr()` at every user-visible string in the `.slint` files.
2. gettext catalogs in `rust/frontend/translations/<lang>/LC_MESSAGES/frontend.po`,
   one per language, with the template `frontend.pot` beside them. These
   are the canonical catalogs.
3. `rust/frontend/build.rs` bundles every catalog into the binary with
   `with_bundled_translations`; nothing is read from disk at runtime.

The step-by-step workflow for changing strings and adding a language lives in
[`rust/frontend/translations/README.md`](../rust/frontend/translations/README.md).

## Locale resolution

| Source | Precedence |
|---|---|
| Language setting (`[general] language = "ja"` in `frontend.toml`) | 1: explicit choice |
| `auto` or unset | 2: `LC_ALL`, then `LC_MESSAGES`, then `LANG` |

`apply_language` in `rust/frontend/src/main.rs` runs before the first frame and
again whenever the Language setting changes, so a switch applies live without
a relaunch. It normalizes `-` to `_`, tries the exact tag (`zh_CN`), then the
language part (`zh`), and falls back to the English source strings when no
bundled catalog matches.

## Writing translatable strings

Every literal a user might read belongs in `@tr()`:

```slint
// Good: the translator can reorder the value.
text: @tr("Built {}", AboutView.build-date);

// Good: the entire sentence is one translation unit.
text: @tr("Version {} · {} · {}", AboutView.version, AboutView.commit, AboutView.channel);

// Bad: splits the sentence; German, Japanese, etc. can't reorder it.
text: @tr("Built") + " " + AboutView.build-date;
```

Rule of thumb: one sentence, one `@tr()`. Use `{}` placeholders for runtime
values; translators can reorder them with `{0}`, `{1}`.

Counts use Slint's plural form so each language applies its own plural rules:

```slint
text: @tr("{n} system with favorites" | "{n} systems with favorites" % count);
```

The extractor only sees `.slint` files, which sets two rules for Rust:

- Rust returns stable ids and structured values; the view composes the
  sentence. Action errors, for example, publish a `kind` that the view maps
  to copy.
- Dynamic lists (menu entries, setting values) go through a key-to-`@tr`
  vocabulary function in `.slint` (see `ui/labels.slint` and
  `ui/settings.slint`).

Strings that never face a user do **not** need wrapping: enum tokens,
filesystem paths, internal error text routed to `tracing::error!`, and similar.
If it could appear in a screenshot, wrap it.

Before choosing the English wording itself, check `docs/content-style.md`'s
terminology glossary and capitalization rules. Consistent source strings make
the translator's job easier too.

## Checks

`just lint` runs `scripts/check-translations.sh`, which regenerates the template
and fails when the committed `frontend.pot` differs (source locations included).
Run `just tr-extract` and merge the catalogs after any `@tr` edit.

## Requirements

- `slint-tr-extractor` 1.17.1, matching the pinned Slint. The toolchain image
  provides it, so `just tr-extract` and `just lint` need no local install.
- gettext (`msgmerge`, `msgattrib`, `msginit`) for catalog maintenance.

## Translators

| Locale | Translator |
|---|---|
| Italian (`it_IT`) | Andrea Bogazzi ([@asturur](https://github.com/asturur)) |
| Spanish (`es_ES`) | Carlos R. ([@crodriguezdominguez](https://github.com/crodriguezdominguez)) |
| Basque (`eu`) | devilschile2 |
| French (`fr`) | Wilfried ([@willoucom](https://github.com/willoucom)) |

Translators are added to this table when their catalog is merged. Names also
appear on the About screen so end users see them.
