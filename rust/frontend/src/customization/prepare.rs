//! Bounded disk/decode work. Only pixel buffers cross back to the UI thread.

use std::io::{Cursor, Read};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use zaparoo_app::customization as rules;

pub(super) struct Pixels {
    pub rgba: Vec<u8>,
    pub width: u32,
    pub height: u32,
    pub premultiplied: bool,
}

impl Pixels {
    pub fn into_image(self) -> slint::Image {
        let buffer = slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(
            &self.rgba,
            self.width,
            self.height,
        );
        if self.premultiplied {
            slint::Image::from_rgba8_premultiplied(buffer)
        } else {
            slint::Image::from_rgba8(buffer)
        }
    }
}

pub(super) fn decode(path: &Path, edge: u32) -> Option<Pixels> {
    let svg = path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("svg"));
    let limit = if svg {
        rules::ART_SVG_BYTES
    } else {
        rules::ART_FILE_BYTES
    };
    let file = std::fs::File::open(path).ok()?;
    let metadata = file.metadata().ok()?;
    if !metadata.is_file() || metadata.len() > limit {
        return None;
    }
    let mut bytes = Vec::new();
    file.take(limit + 1).read_to_end(&mut bytes).ok()?;
    if bytes.len() as u64 > limit {
        return None;
    }
    let edge = edge.clamp(1, rules::ART_OUTPUT_EDGE);
    if svg {
        return decode_svg(&bytes, edge);
    }
    let mut reader = image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .ok()?;
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(rules::ART_DECODE_BYTES);
    limits.max_image_width = Some(rules::ART_SOURCE_EDGE);
    limits.max_image_height = Some(rules::ART_SOURCE_EDGE);
    reader.limits(limits);
    let decoded = reader.decode().ok()?;
    // thumbnail preserves aspect ratio and does not enlarge a small bitmap.
    let resized = if decoded.width() > edge || decoded.height() > edge {
        decoded.thumbnail(edge, edge)
    } else {
        decoded
    };
    let rgba = resized.to_rgba8();
    Some(Pixels {
        width: rgba.width(),
        height: rgba.height(),
        rgba: rgba.into_raw(),
        premultiplied: false,
    })
}

fn decode_svg(bytes: &[u8], edge: u32) -> Option<Pixels> {
    let svg = std::str::from_utf8(bytes).ok()?;
    // Referenced images can bypass the bitmap decoder's limits or read another
    // filesystem path. Reject the document instead of silently changing it.
    let external = AtomicBool::new(false);
    let options = resvg::usvg::Options {
        image_href_resolver: resvg::usvg::ImageHrefResolver {
            resolve_data: Box::new(|_, _, _| {
                external.store(true, Ordering::Relaxed);
                None
            }),
            resolve_string: Box::new(|_, _| {
                external.store(true, Ordering::Relaxed);
                None
            }),
        },
        ..Default::default()
    };
    let tree = resvg::usvg::Tree::from_str(svg, &options).ok()?;
    // Filter graphs retain intermediate rasters outside the output pixel
    // budget. Plain paths, gradients, clipping and masks remain supported.
    if external.load(Ordering::Relaxed) || !tree.filters().is_empty() {
        return None;
    }
    let mut pixmap = resvg::tiny_skia::Pixmap::new(edge, edge)?;
    let size = tree.size();
    let scale = (edge as f32 / size.width()).min(edge as f32 / size.height());
    let dx = (edge as f32 - size.width() * scale) / 2.0;
    let dy = (edge as f32 - size.height() * scale) / 2.0;
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(scale, scale).post_translate(dx, dy),
        &mut pixmap.as_mut(),
    );
    Some(Pixels {
        rgba: pixmap.take(),
        width: edge,
        height: edge,
        premultiplied: true,
    })
}

#[cfg(test)]
#[allow(clippy::expect_used, reason = "bounded local artwork fixtures")]
mod tests {
    use super::*;

    #[test]
    fn bitmaps_preserve_color_and_aspect_at_display_size() {
        let scratch = super::super::tests::Scratch::new("sized");
        let path = scratch.0.join("art.png");
        image::RgbaImage::from_pixel(2048, 1024, image::Rgba([19, 47, 91, 255]))
            .save(&path)
            .expect("PNG");
        let pixels = decode(&path, 128).expect("bounded decode");
        assert_eq!((pixels.width, pixels.height), (128, 64));
        assert_eq!(&pixels.rgba[..4], &[19, 47, 91, 255]);
        let original = image::image_dimensions(&path).expect("original file");
        assert_eq!(original, (2048, 1024));
    }

    #[test]
    fn oversized_and_unsupported_art_falls_back_without_decoding() {
        let scratch = super::super::tests::Scratch::new("limits");
        let path = scratch.0.join("huge.png");
        std::fs::File::create(&path)
            .expect("file")
            .set_len(rules::ART_FILE_BYTES + 1)
            .expect("sparse size");
        assert!(decode(&path, 128).is_none());
        image::RgbaImage::new(rules::ART_SOURCE_EDGE + 1, 1)
            .save(&path)
            .expect("wide PNG");
        assert!(decode(&path, 128).is_none());
        let path = scratch.0.join("art.svg");
        for body in [
            r#"<image href="/not/read.png" width="10" height="10"/>"#,
            r#"<filter id="blur"><feGaussianBlur stdDeviation="1"/></filter><rect width="10" height="10" filter="url(#blur)"/>"#,
        ] {
            std::fs::write(
                &path,
                format!(
                    r#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10">{body}</svg>"#
                ),
            )
            .expect("SVG");
            assert!(decode(&path, 128).is_none());
        }
    }
}
