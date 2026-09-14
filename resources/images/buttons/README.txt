Help-bar controller glyphs.

One folder per button style: style_a, style_b, style_c, style_d and
style_e. The letters are deliberately neutral labels for a glyph set, not
controller makers' names, and the code, config and this folder never spell
a brand. style_d is the default and style_e is the keyboard set; the
`button_layout` setting picks one, or `auto` follows the connected
controller (rust/zaparoo-app/src/buttons.rs).

Each style carries the same eleven files: FaceNorth.svg, FaceSouth.svg,
FaceEast.svg, FaceWest.svg, ShoulderL.svg, ShoulderR.svg, Dpad.svg,
DpadUp.svg, DpadDown.svg, DpadLeft.svg and DpadRight.svg. Positions are
physical, so FaceSouth is the bottom face button whatever it is labeled.
Add a new style by adding a folder with the same eleven names and
registering it in STYLES.

The frontend embeds these SVGs and rasterizes them at runtime under the
`buttons/<style>/<Name>` glyph keys (rust/frontend/src/glyphs.rs).

Source and license: LICENSES/Kenney-ATTRIBUTION.txt
