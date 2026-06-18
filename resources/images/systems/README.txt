Platform logos for the paged systems grid.

Filename matches the Zaparoo Core system id, e.g. SNES.svg, Genesis.svg,
TurboGrafx16.svg. The Tile delegate (src/ui/components/Tile.qml) resolves
these through the Resources singleton's coverUrl helper and the tinted-svg
image provider. Systems without a curated logo here fall through to a
procedural panel rendered in the paged grid.

These are the bundled defaults. Users can override any system's artwork by
dropping a file in the customization root's systems/ subfolder (named by
system id); see docs/customization.md. Overrides are served as-is, bypassing
the tint pipeline used for these bundled logos.

Sources and licences: src/LICENSES/console-logos-ATTRIBUTION.txt,
src/LICENSES/wikimedia-public-domain-ATTRIBUTION.txt, and
src/LICENSES/NounProject-ATTRIBUTION.txt
