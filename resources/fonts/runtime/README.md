# Runtime fonts for the Slint frontend

Static Regular instances of the six script faces in the parent directory,
loaded at startup by `rust/frontend-slint/src/fonts.rs` (from `<exe dir>/fonts`
on MiSTer, from here on the desktop). The Qt build keeps embedding the
variable files in the parent directory; nothing there changed.

Why static: Slint's runtime text path renders a variable font's default
instance and does not apply the weight the UI asks for. Noto Sans Hebrew,
JP, KR and TC default to Thin (wght 100), which rendered as spindly text in
the offline probe. Instancing at wght 400 (and wdth 100 where the axis
exists) sidesteps that and also drops the variation tables, so the files
are smaller than their variable sources.

Regenerate with fontTools (`pip install fonttools`):

```sh
cd resources/fonts
for f in NotoSansArabic NotoSansDevanagari NotoSansHebrew; do
  fonttools varLib.instancer -q -o runtime/$f-Regular.ttf $f.ttf wght=400 wdth=100
done
for f in NotoSansJP NotoSansKR NotoSansTC; do
  fonttools varLib.instancer -q -o runtime/$f-Regular.ttf $f.ttf wght=400
done
```

Only Regular is shipped. A label that asks for a heavier weight in one of
these scripts renders Regular until Slint applies variation axes at runtime
(tracked on the parity ledger in `docs/plans/slint-migration.md`).
