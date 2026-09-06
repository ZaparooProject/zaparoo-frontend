# Slint frontend translations

gettext catalogs bundled into the binary by `build.rs`
(`with_bundled_translations`), one per language at
`<lang>/LC_MESSAGES/frontend-slint.po`, plus the template
`frontend-slint.pot`. English is the source text in the `.slint` files and
has no catalog. Language names are the Qt catalog suffixes (`de`, `zh_CN`,
...) and must match the Language setting exactly or by language part; see
`apply_language` in `src/main.rs`.

Until the flip, the Qt catalogs in `src/ui/translations/*.ts` stay the Qt
build's source of truth and this directory is derived from them:

1. `just slint-tr-extract` regenerates the template from every `.slint` file
   (`slint-tr-extractor`, no default context). `just lint-slint` fails when
   the committed template is stale, the same rule the Qt catalogs have.
2. `just slint-tr-convert` re-harvests every Qt catalog
   (`scripts/convert-ts-catalogs.py`): identical source strings carry their
   translation across, `%1` placeholders become `{}` / `{0}`, Qt contexts
   are dropped, and strings with no `.slint` counterpart are left out until
   the screen that uses them is ported.

After the flip the `.ts` files go away and these `.po` files become
canonical; translators edit them directly (poedit, Lokalize, Weblate) and
the harvest step is deleted.
