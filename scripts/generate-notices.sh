#!/usr/bin/env bash
# Zaparoo Frontend
# Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
# SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
#
# Regenerate rust/frontend/LICENSE.txt: the frontend's own license
# (COPYING), the notices for the fonts, artwork and data it includes
# (LICENSES/), and the third-party notices for every crate the binary
# links. It is the one license file the release bundles ship, and the
# binary embeds it for the Credits screen (`zaparoo_app::credits` reads
# its layout, so keep the two in step).
#
# cargo-about handles the open-source crates. It cannot handle the Slint
# crates themselves: their license text is a custom document its scanner
# cannot map to an SPDX id, so it drops them with a warning. Those crates
# are appended here from the license file the crate itself ships, under
# the one license term of Slint's triple license this project uses.
#
# Needs cargo-about, which the toolchain image provides (`just notices`).
# The result is committed; `scripts/check-notices.sh` fails when it is
# stale.
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)"
repo_root="$(cd -- "$script_dir/.." && pwd -P)"
rust_dir="$repo_root/rust"
out="$rust_dir/frontend/LICENSE.txt"

if ! command -v cargo-about > /dev/null 2>&1; then
    echo "cargo-about not found; run this through \`just notices\`, which uses the toolchain image" >&2
    exit 1
fi

tmp="$(mktemp)"
trap 'rm -f "$tmp"' EXIT

cd "$rust_dir"
# The Slint crates warn here (see the header); every other crate must
# resolve, so anything else in the log is a real gap.
cargo about generate \
    -c about.toml about.hbs \
    --manifest-path frontend/Cargo.toml \
    -o "$tmp" 2>&1 | grep -v 'LicenseRef-Slint-Software-3.0 has no license file' || true

python3 - "$tmp" "$out" "$repo_root" <<'PY'
import json
import pathlib
import re
import subprocess
import sys

tmp, out, repo_root = sys.argv[1], sys.argv[2], pathlib.Path(sys.argv[3])

# The layout `zaparoo_app::credits` parses. A part opens with a title
# between two PART rules; an entry opens with a title between two ENTRY
# rules. An entry that starts with "Used by:" is a crate license: the
# crates it lists are the components. Any other entry is one component,
# named by its title.
PART = "#" * 70
ENTRY = "-" * 70


def text_of(path):
    return path.read_text().rstrip("\n").replace("\r\n", "\n")


def check_free_of_rules(name, text):
    for line in text.split("\n"):
        if line in (PART, ENTRY):
            raise SystemExit(f"{name} has a line that reads as a section rule")


# Every file under LICENSES/ is listed here exactly once, under the name
# Credits shows. A new attribution file needs a row, so it cannot ship
# without landing in the app.
PROJECT = [
    ("Zaparoo Frontend", ["COPYING"]),
    ("Zaparoo Project artwork", ["LICENSES/Zaparoo-ASSETS-NOTICE.txt"]),
]
ASSETS = [
    ("Noto Sans", ["LICENSES/NotoSans-ATTRIBUTION.txt", "LICENSES/NotoSans-OFL.txt"]),
    ("MxPlus HP 100LX 6x8", ["LICENSES/MxPlus-ATTRIBUTION.txt"]),
    ("Iconoir", ["LICENSES/Iconoir-ATTRIBUTION.txt"]),
    ("Lucide", ["LICENSES/Lucide-ATTRIBUTION.txt"]),
    ("Streamline", ["LICENSES/Streamline-ATTRIBUTION.txt"]),
    ("Kenney Input Prompts", ["LICENSES/Kenney-ATTRIBUTION.txt"]),
    ("Console logos", ["LICENSES/console-logos-ATTRIBUTION.txt"]),
    ("Simple Icons", ["LICENSES/SimpleIcons-ATTRIBUTION.txt"]),
    ("Noun Project icons", ["LICENSES/NounProject-ATTRIBUTION.txt"]),
    ("Tux", ["LICENSES/Tux-ATTRIBUTION.txt"]),
    ("Wikimedia Commons logos", ["LICENSES/wikimedia-public-domain-ATTRIBUTION.txt"]),
    ("Names_MiSTer", ["LICENSES/Names_MiSTer-ATTRIBUTION.txt"]),
]
SOFTWARE_NOTICES = [
    ("zaparoo-update", ["LICENSES/zaparoo-update-NOTICE.txt"]),
]

listed = {
    f
    for _, files in PROJECT + ASSETS + SOFTWARE_NOTICES
    for f in files
    if f.startswith("LICENSES/")
}
present = {
    f"LICENSES/{p.name}" for p in (repo_root / "LICENSES").iterdir() if p.is_file()
}
if listed != present:
    missing = sorted(present - listed)
    stale = sorted(listed - present)
    raise SystemExit(
        f"LICENSES/ and the Credits table disagree: unlisted {missing}, missing {stale}"
    )


def file_entries(rows):
    entries = []
    for title, files in rows:
        texts = []
        for f in files:
            text = text_of(repo_root / f)
            check_free_of_rules(f, text)
            texts.append(text)
        entries.append((title, "\n\n".join(texts)))
    return entries


metadata = json.loads(
    subprocess.run(
        [
            "cargo",
            "metadata",
            "--format-version",
            "1",
            "--manifest-path",
            "frontend/Cargo.toml",
        ],
        capture_output=True,
        check=True,
        text=True,
    ).stdout
)

# Every Slint-authored crate in the graph, and the license file one of
# them ships (they all carry the same text).
slint = sorted(
    (p["name"], p["version"], pathlib.Path(p["manifest_path"]).parent)
    for p in metadata["packages"]
    if "LicenseRef-Slint-Software-3.0" in (p.get("license") or "")
)
if not slint:
    raise SystemExit("no Slint crates found in the graph; check the manifest path")

texts = {
    (root / "LICENSES" / "LicenseRef-Slint-Software-3.0.md").read_text()
    for _, _, root in slint
    if (root / "LICENSES" / "LicenseRef-Slint-Software-3.0.md").is_file()
}
if len(texts) != 1:
    raise SystemExit(
        f"expected one Slint license text across the crates, found {len(texts)}"
    )
slint_text = texts.pop().rstrip("\n")
check_free_of_rules("the Slint license", slint_text)

# cargo-about's blocks, one per distinct license text. Crates are listed
# by name only, so two versions of one crate under the same license
# collapse to one "Used by" line.
raw = pathlib.Path(tmp).read_text()
blocks = raw.split(f"{ENTRY}\n")[1:]
if len(blocks) % 2:
    raise SystemExit("cargo-about output is not title/body pairs; check about.hbs")
crate_entries = []
for title_part, body in zip(blocks[0::2], blocks[1::2]):
    title = title_part.rstrip("\n")
    if "\n" in title:
        raise SystemExit(f"unexpected cargo-about block title: {title!r}")
    lines = body.strip("\n").split("\n")
    if lines[0] != "Used by:":
        raise SystemExit(f"cargo-about block {title!r} has no Used by list")
    crates, rest = [], 1
    while rest < len(lines) and lines[rest].startswith("  - "):
        name = lines[rest][4:]
        if name not in crates:
            crates.append(name)
        rest += 1
    text = "\n".join(lines[rest:]).strip("\n")
    check_free_of_rules(f"the {title} text", text)
    crate_entries.append((title, crates, text))
crate_entries.append(
    ("Slint Software License 3.0", sorted({name for name, _, _ in slint}), slint_text)
)


def part(title):
    return [PART, title, PART, ""]


def entry(title, text, crates=None):
    lines = [ENTRY, title, ENTRY, ""]
    if crates is not None:
        lines.append("Used by:")
        lines += [f"  - {name}" for name in crates]
        lines.append("")
    return lines + [text, ""]


lines = [
    "Zaparoo Frontend - license and notices",
    "======================================",
    "",
    "The Zaparoo Frontend's own license comes first. The notices for the",
    "fonts, artwork and data it includes follow, then the third-party",
    "software the binary links: each license, the crates used under it,",
    "and the full license text. Crates are listed by name only, so a",
    "version bump leaves this file unchanged. The app shows the same text",
    "in Settings > About > Credits.",
    "",
    "Generated by `just notices`; do not edit by hand.",
    "",
]
lines += part("Zaparoo Frontend")
for title, text in file_entries(PROJECT):
    lines += entry(title, text)
lines += part("Fonts, artwork and data")
for title, text in file_entries(ASSETS):
    lines += entry(title, text)
lines += part("Software")
for title, text in file_entries(SOFTWARE_NOTICES):
    lines += entry(title, text)
for title, crates, text in crate_entries:
    lines += entry(title, text, crates)

pathlib.Path(out).write_text("\n".join(lines).rstrip("\n") + "\n")
print(f"wrote {out} ({len(crate_entries)} crate licenses, {len(slint)} Slint crates)")
PY
