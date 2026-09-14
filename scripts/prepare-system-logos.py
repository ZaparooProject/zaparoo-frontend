#!/usr/bin/env python3
# Zaparoo Frontend
# Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
# SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
"""Regenerate the embedded system logo art from the source sets.

The frontend embeds two PNG sets from rust/frontend/assets/ (see
rust/frontend/build.rs and rust/frontend/src/system_logos.rs):

  systems/        the neutral-grayscale logos, rasterized from
                  resources/images/systems/<id>.svg at LOGO_HEIGHT px tall
                  and tinted at runtime
  systems-color/  the original full-color logos, copied from
                  resources/images/systems-color/<id>.png and scaled down
                  to at most LOGO_HEIGHT px tall (the source set mixes
                  small web assets with 4K masters, and a 4K RGBA decode
                  is tens of megabytes on a MiSTer)

Run after changing either source set:

  just logos              regenerate both sets
  just logos --check      fail if the embedded stems drift from the sources

The grayscale set needs a rasterizer on PATH (`resvg`, `rsvg-convert`, or
`inkscape`); the color set only needs Pillow.
"""

from __future__ import annotations

import argparse
import pathlib
import shutil
import subprocess
import sys

LOGO_HEIGHT = 160

ROOT = pathlib.Path(__file__).resolve().parent.parent
SVG_DIR = ROOT / "resources" / "images" / "systems"
COLOR_SRC_DIR = ROOT / "resources" / "images" / "systems-color"
GRAY_OUT_DIR = ROOT / "rust" / "frontend" / "assets" / "systems"
COLOR_OUT_DIR = ROOT / "rust" / "frontend" / "assets" / "systems-color"


def stems(directory: pathlib.Path, suffix: str) -> set[str]:
    return {p.stem for p in directory.glob(f"*{suffix}")}


def check() -> int:
    """The embedded sets must mirror the sources stem for stem."""
    problems: list[str] = []
    svg = stems(SVG_DIR, ".svg")
    gray = stems(GRAY_OUT_DIR, ".png")
    color_src = stems(COLOR_SRC_DIR, ".png")
    color = stems(COLOR_OUT_DIR, ".png")
    for missing in sorted(svg - gray):
        problems.append(f"missing grayscale raster: {GRAY_OUT_DIR.name}/{missing}.png")
    for extra in sorted(gray - svg):
        problems.append(f"orphan grayscale raster (no SVG): {GRAY_OUT_DIR.name}/{extra}.png")
    for missing in sorted(color_src - color):
        problems.append(f"missing color copy: {COLOR_OUT_DIR.name}/{missing}.png")
    for extra in sorted(color - color_src):
        problems.append(f"orphan color copy (no source): {COLOR_OUT_DIR.name}/{extra}.png")
    for stray in sorted(color_src - svg):
        problems.append(f"color logo without a grayscale SVG: {COLOR_SRC_DIR.name}/{stray}.png")
    if problems:
        print("\n".join(problems), file=sys.stderr)
        print("run `just logos` to regenerate rust/frontend/assets/", file=sys.stderr)
        return 1
    print(f"system logos in sync: {len(svg)} grayscale, {len(color)} color")
    return 0


def rasterizer() -> list[str] | None:
    if shutil.which("resvg"):
        return ["resvg", "--height", str(LOGO_HEIGHT)]
    if shutil.which("rsvg-convert"):
        return ["rsvg-convert", "--height", str(LOGO_HEIGHT), "--keep-aspect-ratio", "--output"]
    if shutil.which("inkscape"):
        return ["inkscape", f"--export-height={LOGO_HEIGHT}", "--export-type=png", "--export-filename"]
    return None


def rasterize_grayscale() -> None:
    from PIL import Image

    tool = rasterizer()
    if tool is None:
        sys.exit("no SVG rasterizer found; install resvg, rsvg-convert, or inkscape")
    GRAY_OUT_DIR.mkdir(parents=True, exist_ok=True)
    for old in GRAY_OUT_DIR.glob("*.png"):
        old.unlink()
    for svg in sorted(SVG_DIR.glob("*.svg")):
        out = GRAY_OUT_DIR / f"{svg.stem}.png"
        if tool[0] == "resvg":
            subprocess.run([*tool, str(svg), str(out)], check=True)
        else:
            subprocess.run([*tool, str(out), str(svg)], check=True)
        # The art is neutral grayscale by contract; store it that way so the
        # binary carries one channel plus alpha instead of four.
        with Image.open(out) as image:
            image.convert("LA").save(out, optimize=True)
    print(f"rasterized {len(list(GRAY_OUT_DIR.glob('*.png')))} grayscale logos")


def copy_color() -> None:
    from PIL import Image

    COLOR_OUT_DIR.mkdir(parents=True, exist_ok=True)
    for old in COLOR_OUT_DIR.glob("*.png"):
        old.unlink()
    for src in sorted(COLOR_SRC_DIR.glob("*.png")):
        out = COLOR_OUT_DIR / src.name
        with Image.open(src) as image:
            rgba = image.convert("RGBA")
            width, height = rgba.size
            if height > LOGO_HEIGHT:
                scaled_width = max(1, round(width * LOGO_HEIGHT / height))
                rgba = rgba.resize((scaled_width, LOGO_HEIGHT), Image.Resampling.LANCZOS)
            rgba.save(out, optimize=True)
    print(f"copied {len(list(COLOR_OUT_DIR.glob('*.png')))} color logos")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--check", action="store_true", help="verify the embedded sets match the sources")
    parser.add_argument("--color-only", action="store_true", help="skip the grayscale rasterization")
    args = parser.parse_args()
    if args.check:
        return check()
    if not args.color_only:
        rasterize_grayscale()
    copy_color()
    return check()


if __name__ == "__main__":
    sys.exit(main())
