Colored platform logos for the optional systems-grid logo style.

Filename stems match the tinted SVG set under resources/images/systems/.
Most base PNGs were restored from the pre-tinting asset set. Regional
variants were sourced from PRO100BYTE/console-logos Dark - Color assets.

The default app style remains tinted SVG. These PNGs are only shown when the
user selects "Full color" system logos in Settings. The frontend embeds
copies scaled to 160 px tall from rust/frontend/assets/systems-color/; run
`just logos` after changing a file here (the source set mixes small web
assets with 4K masters, and the binary only carries the scaled copies).

Source and license: LICENSES/console-logos-ATTRIBUTION.txt
