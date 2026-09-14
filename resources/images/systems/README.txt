Platform logos for the paged systems grid.

Filename matches the Zaparoo Core system id, e.g. SNES.svg, Genesis.svg,
TurboGrafx16.svg. These SVGs are the source art: the frontend embeds
pre-rasterized PNG copies from rust/frontend/assets/systems/
(<id>.png, 160 px tall, tinted at runtime; see
rust/frontend/src/system_logos.rs). Run `just logos` after changing a logo
here; `just lint` fails when the embedded set drifts from these sources.
Systems without a logo fall back to their name as a wordmark on the tile.

These are the bundled defaults. Users can override any system's artwork by
dropping a file in the customization root's systems/ subfolder (named by
system id); see docs/customization.md. Overrides are served as-is, bypassing
the tint pipeline used for these bundled logos.

Sources and licences: LICENSES/console-logos-ATTRIBUTION.txt,
LICENSES/wikimedia-public-domain-ATTRIBUTION.txt, and
LICENSES/NounProject-ATTRIBUTION.txt
