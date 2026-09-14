# Translations

gettext catalogs bundled into the binary by `build.rs`
(`with_bundled_translations`), one per language at
`<lang>/LC_MESSAGES/frontend.po`, plus the template `frontend.pot`. The file
name is the gettext domain, which Slint takes from the crate name. English
is the source text in the `.slint` files and has no catalog.

These `.po` files are the canonical catalogs. Translators edit them directly
with any gettext editor (Poedit, Lokalize, Weblate).

## Updating after a string change

1. Mark every user-visible string with `@tr()` in a `.slint` file. Rust
   returns stable ids and structured values; the view composes the sentence.
2. Run `just tr-extract` to regenerate `frontend.pot` (`slint-tr-extractor`,
   no default context, so one msgid is one entry across components).
   `just lint` fails when the committed template is stale.
3. Merge the template into each catalog so new strings appear untranslated
   and source locations stay current:

   ```sh
   cd rust/frontend/translations
   for po in */LC_MESSAGES/frontend.po; do
       msgmerge --quiet --no-fuzzy-matching --no-wrap -o "$po.tmp" "$po" frontend.pot
       msgattrib --no-obsolete --no-wrap -o "$po" "$po.tmp"
       rm "$po.tmp"
   done
   ```

## Adding a language

Language names are the directory names (`de`, `zh_CN`, ...). They must match
the Language setting exactly or by language part; see `apply_language` in
`src/main.rs`. To add one:

1. Create `<lang>/LC_MESSAGES/frontend.po` from the template
   (`msginit --no-translator -l <lang> -i frontend.pot -o <lang>/LC_MESSAGES/frontend.po`).
2. Add the id to `LANGUAGES` in `rust/zaparoo-app/src/settings.rs` and its
   display name to the language vocabulary in `ui/settings.slint`.

See `docs/translations.md` for placeholders, plurals and locale resolution.
