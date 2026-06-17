Platform logos for the paged systems grid.

Filename matches the Zaparoo Core system id, e.g. SNES.svg, Genesis.svg,
TurboGrafx16.svg. The Tile delegate (src/ui/components/Tile.qml) resolves
these through the Resources singleton's coverUrl helper and the tinted-svg
image provider. Systems without a curated logo here fall through to a
procedural panel rendered in the paged grid.

Sources and licences: src/LICENSES/console-logos-ATTRIBUTION.txt,
src/LICENSES/wikimedia-public-domain-ATTRIBUTION.txt, and
src/LICENSES/NounProject-ATTRIBUTION.txt
