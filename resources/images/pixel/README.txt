Pixel art for the small UI glyphs on the bitmap-font scenes.

Where the 6x8 bitmap face is in use (CRT, and MiSTer outputs under 400 px
tall) a vector icon squeezed into a 6-9 px slot turns to grey blur. The keys
here replace their vector source under the same key ("status/WiFi",
"icons/Heart") on those scenes only; rust/frontend/src/glyphs.rs embeds them
(PIXEL_ART) and blits each at the largest whole multiple of its grid that
fits the slot, centered on whole pixels, at 2x at most. A slot smaller than
the art, or three times its size or more (the same icon as a Hub tile's
art), keeps the vector source.

One row of pixels per line:

  .  clear
  #  ink: the glyph's tint, or the favorite heart's fill
  +  the favorite heart's keyline

Keep the art no larger than the smallest slot it is drawn in on CRT
(status icons 8 px, scroll and chevron arrows 6 px, the list heart 7 px), and
draw every new file's key from the vector set it stands in for.

Original artwork for this project.
