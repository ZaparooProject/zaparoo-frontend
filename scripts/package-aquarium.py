#!/usr/bin/env python3
"""Build the deterministic, directly addressable embedded Aquarium archive.

PNG payloads already compress well. An uncompressed USTAR archive lets the
frontend borrow one image without inflating or copying the whole collection.
"""
import argparse
import io
import tarfile
from pathlib import Path

from PIL import Image

ROOT = Path(__file__).resolve().parent.parent
SOURCE = ROOT / "resources/aquarium"
OUTPUT = ROOT / "rust/frontend/assets/aquarium.tar"


def build():
    stream = io.BytesIO()
    files = sorted(SOURCE.rglob("*.png"))
    if not files:
        raise ValueError("Aquarium source collection is empty")
    with tarfile.open(fileobj=stream, mode="w", format=tarfile.USTAR_FORMAT) as archive:
        catalog = []
        for path in files:
            if path.is_symlink():
                raise ValueError(f"Symlink not allowed: {path}")
            name = path.relative_to(SOURCE).as_posix()
            if len(name.encode("utf-8")) > 100:
                raise ValueError(f"Archive path exceeds the reader's name field: {name}")
            data = path.read_bytes()
            entry = tarfile.TarInfo(name)
            entry.size = len(data)
            entry.mode = 0o644
            entry.mtime = 0
            archive.addfile(entry, io.BytesIO(data))
            if path.parent == SOURCE / "fish":
                with Image.open(path) as image:
                    bounds = image.convert("RGBA").getbbox()
                if bounds is None:
                    raise ValueError(f"Fish is empty: {path}")
                x, y, right, bottom = bounds
                # Numbered fish face left; the twelve named tropical fish face right.
                faces_right = int(not path.stem.startswith("fish_"))
                catalog.append(f"{name}\t{x}\t{y}\t{right-x}\t{bottom-y}\t{faces_right}\n")
        # Small crop/orientation metadata avoids decoding all 178 fish at startup.
        data = "".join(catalog).encode("utf-8")
        entry = tarfile.TarInfo("fish-catalog.tsv")
        entry.size = len(data)
        entry.mode = 0o644
        entry.mtime = 0
        archive.addfile(entry, io.BytesIO(data))
    return stream.getvalue(), len(files)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    data, count = build()
    if args.check:
        if not OUTPUT.exists() or OUTPUT.read_bytes() != data:
            raise SystemExit("Aquarium archive is stale; run just aquarium-assets")
        print(f"Aquarium archive in sync: {count} images")
    else:
        OUTPUT.write_bytes(data)
        print(f"Packed {count} images into {OUTPUT.relative_to(ROOT)} ({len(data):,} bytes)")


if __name__ == "__main__":
    main()
