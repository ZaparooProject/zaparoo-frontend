#!/usr/bin/env python3
# Zaparoo Frontend
# Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
# SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
#
# Harvest the Qt Linguist catalogs (src/ui/translations/frontend_<lang>.ts)
# into the Slint frontend's gettext catalogs
# (rust/frontend-slint/translations/<lang>/LC_MESSAGES/frontend-slint.po).
#
# The Qt catalogs stay the Qt build's source of truth until the flip. This
# script is the bridge while screens are ported: every run re-harvests any
# Qt translation whose source string is identical to a `@tr()` string in the
# .slint files, so translators' existing work carries over string by
# string. Entries with no matching .slint string are dropped (they come
# back on the next run once the string is ported), and entries the .slint
# files have but Qt never had are kept untranslated for translators.
#
# Mapping rules:
#   - Qt placeholders `%1 %2 ...` become Slint's `{}` in msgid (when they
#     appear in order, which is how the .slint sources are written) and the
#     explicit `{0} {1} ...` in msgstr, so a translator's reordering is kept.
#   - `%n` (plural count) becomes `{n}`.
#   - Literal braces are doubled, as Slint requires.
#   - Qt contexts are dropped: the Slint build runs with no default
#     translation context (build.rs), so one msgid is one entry. When two Qt
#     contexts translated the same source differently, the first non-empty
#     translation wins and the conflict is reported.
#   - English is not converted: Slint's source strings are the English text.
#
# Requires lconvert-qt6 (qt6-qttools), msgmerge and msgattrib (gettext), and
# the committed template rust/frontend-slint/translations/frontend-slint.pot
# (regenerate with `just slint-tr-extract`).

import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
TS_DIR = REPO / "src" / "ui" / "translations"
OUT_DIR = REPO / "rust" / "frontend-slint" / "translations"
POT = OUT_DIR / "frontend-slint.pot"
DOMAIN = "frontend-slint"


def tool(name: str) -> str:
    path = shutil.which(name)
    if not path:
        sys.exit(f"{name} not found on PATH")
    return path


def parse_po(text: str):
    """Yield (msgctxt, msgid, msgid_plural, msgstrs) for live entries."""
    entries = []
    cur = {}
    key = None

    def flush():
        if cur.get("msgid") is not None:
            entries.append(cur.copy())
        cur.clear()

    for raw in text.splitlines():
        line = raw.strip()
        if not line:
            flush()
            key = None
            continue
        if line.startswith("#"):
            # Obsolete entries (#~) and comments are dropped: locations and
            # flags belong to the Qt catalog, not to the Slint one.
            continue
        m = re.match(r'^(msgctxt|msgid_plural|msgid|msgstr(?:\[(\d+)\])?)\s+"(.*)"$', line)
        if m:
            key = m.group(1)
            if key.startswith("msgstr"):
                idx = int(m.group(2)) if m.group(2) is not None else 0
                cur.setdefault("msgstr", {})[idx] = m.group(3)
                key = ("msgstr", idx)
            else:
                cur[key] = m.group(3)
            continue
        m = re.match(r'^"(.*)"$', line)
        if m and key is not None:
            if isinstance(key, tuple):
                cur["msgstr"][key[1]] += m.group(1)
            else:
                cur[key] += m.group(1)
    flush()
    return entries


def map_msgid(s: str) -> str:
    s = s.replace("{", "{{").replace("}", "}}")
    order = []
    for n in re.findall(r"%(\d)", s):
        if n not in order:
            order.append(n)
    sequential = order == [str(i) for i in range(1, len(order) + 1)]

    def repl(m):
        n = int(m.group(1))
        return "{}" if sequential else "{" + str(n - 1) + "}"

    s = re.sub(r"%(\d)", repl, s)
    return s.replace("%n", "{n}")


def map_msgstr(s: str) -> str:
    s = s.replace("{", "{{").replace("}", "}}")
    s = re.sub(r"%(\d)", lambda m: "{" + str(int(m.group(1)) - 1) + "}", s)
    return s.replace("%n", "{n}")


def header_field(text: str, name: str, default: str) -> str:
    m = re.search(r'"' + re.escape(name) + r': ([^\\]*)\\n"', text)
    return m.group(1) if m else default


def convert(lang: str, ts: Path, workdir: Path) -> tuple[int, int, list[str]]:
    raw_po = workdir / f"{lang}.raw.po"
    subprocess.run(
        [tool("lconvert-qt6"), "-locations", "none", "-o", str(raw_po), str(ts)],
        check=True,
    )
    text = raw_po.read_text(encoding="utf-8")
    plural_forms = header_field(text, "Plural-Forms", "nplurals=2; plural=(n != 1);")

    merged = {}
    order = []
    conflicts = []
    for e in parse_po(text):
        msgid = e.get("msgid", "")
        if msgid == "":
            continue
        mid = map_msgid(msgid)
        plural = map_msgid(e["msgid_plural"]) if "msgid_plural" in e else None
        strs = {i: map_msgstr(v) for i, v in e.get("msgstr", {}).items()}
        if mid not in merged:
            merged[mid] = {"plural": plural, "msgstr": strs}
            order.append(mid)
            continue
        have = merged[mid]["msgstr"]
        if any(have.values()):
            if any(strs.values()) and strs != have:
                conflicts.append(mid)
        elif any(strs.values()):
            merged[mid]["msgstr"] = strs

    lines = [
        'msgid ""',
        'msgstr ""',
        '"MIME-Version: 1.0\\n"',
        '"Content-Type: text/plain; charset=UTF-8\\n"',
        '"Content-Transfer-Encoding: 8bit\\n"',
        f'"Language: {lang}\\n"',
        f'"Plural-Forms: {plural_forms}\\n"',
        '"X-Generator: scripts/convert-ts-catalogs.py\\n"',
        "",
    ]
    for mid in order:
        entry = merged[mid]
        lines.append(f'msgid "{mid}"')
        if entry["plural"] is not None:
            lines.append(f'msgid_plural "{entry["plural"]}"')
            n = max(entry["msgstr"].keys(), default=0)
            for i in range(n + 1):
                lines.append(f'msgstr[{i}] "{entry["msgstr"].get(i, "")}"')
        else:
            lines.append(f'msgstr "{entry["msgstr"].get(0, "")}"')
        lines.append("")
    harvested = workdir / f"{lang}.harvested.po"
    harvested.write_text("\n".join(lines), encoding="utf-8")

    out = OUT_DIR / lang / "LC_MESSAGES" / f"{DOMAIN}.po"
    out.parent.mkdir(parents=True, exist_ok=True)
    mergedf = workdir / f"{lang}.merged.po"
    subprocess.run(
        [
            tool("msgmerge"),
            "--quiet",
            "--no-fuzzy-matching",
            "--no-wrap",
            "-o",
            str(mergedf),
            str(harvested),
            str(POT),
        ],
        check=True,
    )
    subprocess.run(
        [tool("msgattrib"), "--no-obsolete", "--no-wrap", "-o", str(out), str(mergedf)],
        check=True,
    )
    final = out.read_text(encoding="utf-8")
    total = len(re.findall(r'^msgid "(?!")', final, re.M))
    translated = len(re.findall(r'^msgstr(?:\[0\])? "[^"]', final, re.M))
    return total, translated, conflicts


def main() -> int:
    if not POT.is_file():
        sys.exit(f"missing {POT}; run `just slint-tr-extract` first")
    langs = sorted(
        p.stem.removeprefix("frontend_") for p in TS_DIR.glob("frontend_*.ts")
    )
    langs = [l for l in langs if l != "en"]
    with tempfile.TemporaryDirectory() as tmp:
        for lang in langs:
            total, translated, conflicts = convert(lang, TS_DIR / f"frontend_{lang}.ts", Path(tmp))
            note = f"; {len(conflicts)} context conflicts" if conflicts else ""
            print(f"{lang:6} {translated:3}/{total} translated{note}")
            for c in conflicts:
                print(f"        conflict: {c}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
