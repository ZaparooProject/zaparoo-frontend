#!/usr/bin/env bash
# Zaparoo Frontend
# Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
# SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
#
# Regenerate the Slint frontend's third-party notices
# (rust/frontend-slint/LICENSES/THIRD-PARTY-NOTICES.txt).
#
# cargo-about handles the open-source crates. It cannot handle the Slint
# crates themselves: their license text is a custom document its scanner
# cannot map to an SPDX id, so it drops them with a warning. Those crates
# are appended here from the license file the crate itself ships, under
# the one license term of Slint's triple license this project uses.
#
# Needs `cargo install cargo-about --features cli`. The result is
# committed; `scripts/package-mister-release.sh --slint` ships it in the
# bundle's LICENSES folder.
set -euo pipefail

script_dir="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd -P)"
repo_root="$(cd -- "$script_dir/.." && pwd -P)"
rust_dir="$repo_root/rust"
out="$rust_dir/frontend-slint/LICENSES/THIRD-PARTY-NOTICES.txt"

if ! command -v cargo-about > /dev/null 2>&1; then
    echo "cargo-about is not installed: cargo install cargo-about --features cli" >&2
    exit 1
fi

mkdir -p "$(dirname "$out")"
tmp="$(mktemp)"
trap 'rm -f "$tmp"' EXIT

cd "$rust_dir"
# The Slint crates warn here (see the header); every other crate must
# resolve, so anything else in the log is a real gap.
cargo about generate \
    -c about.toml about.hbs \
    --manifest-path frontend-slint/Cargo.toml \
    -o "$tmp" 2>&1 | grep -v 'LicenseRef-Slint-Software-3.0 has no license file' || true

python3 - "$tmp" "$out" <<'PY'
import json
import pathlib
import subprocess
import sys

tmp, out = sys.argv[1], sys.argv[2]
metadata = json.loads(
    subprocess.run(
        [
            "cargo",
            "metadata",
            "--format-version",
            "1",
            "--manifest-path",
            "frontend-slint/Cargo.toml",
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

body = pathlib.Path(tmp).read_text()
rule = "-" * 70
lines = [
    body.rstrip("\n"),
    "",
    rule,
    "Slint Software License 3.0",
    rule,
    "",
    "Used by:",
]
lines += [f"  - {name} {version}" for name, version, _ in slint]
lines += ["", texts.pop().rstrip("\n"), ""]
pathlib.Path(out).write_text("\n".join(lines))
print(f"wrote {out} ({len(slint)} Slint crates)")
PY
