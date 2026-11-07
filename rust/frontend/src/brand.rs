// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

//! Prepare the header/screensaver logo at its painted size. Slint's software
//! renderer samples scaled bitmap images with nearest-neighbor filtering,
//! which skips source rows even when the requested image rendering is smooth.
//! A bounded per-window cache pays for filtered resizing only on a size or
//! surface-color change, never on a moving screensaver frame.
//!
//! The header logo setting (`zaparoo_app::branding`) can replace the logo
//! with the user's own image. That file is read and decoded off the event
//! loop, within the customization limits, into one bounded source kept in
//! memory; `Brand.custom` filters it to its painted size the same way. A
//! file that cannot be read or decoded shows the Zaparoo logo instead.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::io::{Cursor, Read};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use slint::ComponentHandle;
use zaparoo_app::branding::{self, HeaderLogo};
use zaparoo_app::customization as limits;

const DARK: &[u8] = include_bytes!("../../../resources/images/logo/logo-on-dark-600.png");
const LIGHT: &[u8] = include_bytes!("../../../resources/images/logo/logo-on-light-600.png");
const MAX_WIDTH: u32 = 1024;
const MAX_ENTRIES: usize = 4;
const MAX_BYTES: usize = 2 * 1024 * 1024;
/// The largest custom logo source kept: wide enough for the widest logo box
/// and as tall as one at the box's aspect, which is all a fitted image can
/// ever paint.
const SOURCE_WIDTH: u32 = MAX_WIDTH;
const SOURCE_HEIGHT: u32 = 256;

struct Entry {
    light: bool,
    width: u32,
    image: slint::Image,
    bytes: usize,
}

#[derive(Default)]
struct Cache {
    entries: VecDeque<Entry>,
    bytes: usize,
}

impl Cache {
    fn get(&mut self, light: bool, width: f32) -> Option<slint::Image> {
        if !width.is_finite() || width <= 0.0 {
            return None;
        }
        let width = (width.round() as u32).clamp(1, MAX_WIDTH);
        if let Some(index) = self
            .entries
            .iter()
            .position(|e| e.light == light && e.width == width)
        {
            let entry = self.entries.remove(index)?;
            let image = entry.image.clone();
            self.entries.push_back(entry);
            return Some(image);
        }
        let image = prepare(light, width)?;
        let bytes = image.size().width as usize * image.size().height as usize * 4;
        while self.bytes + bytes > MAX_BYTES || self.entries.len() >= MAX_ENTRIES {
            self.bytes -= self.entries.pop_front()?.bytes;
        }
        self.bytes += bytes;
        self.entries.push_back(Entry {
            light,
            width,
            image: image.clone(),
            bytes,
        });
        Some(image)
    }
}

/// Resize straight-alpha artwork to exactly `width` x `height` with a
/// Lanczos3 filter, returning premultiplied pixels. The paint-sized raster
/// every logo is drawn from: the software renderer samples bitmaps
/// nearest-neighbor, so the sharpness has to be in the raster.
pub(crate) fn resize_premultiplied(
    mut source: image::RgbaImage,
    width: u32,
    height: u32,
) -> image::RgbaImage {
    // Filter premultiplied channels so transparent padding cannot darken
    // antialiased edges. Clamp Lanczos overshoot back to valid coverage.
    for pixel in source.pixels_mut() {
        let alpha = u16::from(pixel.0[3]);
        for channel in &mut pixel.0[..3] {
            *channel = (u16::from(*channel) * alpha / 255) as u8;
        }
    }
    let mut scaled = image::imageops::resize(
        &source,
        width,
        height,
        image::imageops::FilterType::Lanczos3,
    );
    for pixel in scaled.pixels_mut() {
        let alpha = pixel.0[3];
        for channel in &mut pixel.0[..3] {
            *channel = (*channel).min(alpha);
        }
    }
    scaled
}

fn prepare(light: bool, width: u32) -> Option<slint::Image> {
    let source = image::load_from_memory(if light { LIGHT } else { DARK })
        .ok()?
        .to_rgba8();
    let height = (f64::from(width) * f64::from(source.height()) / f64::from(source.width()))
        .round()
        .max(1.0) as u32;
    let scaled = resize_premultiplied(source, width, height);
    Some(slint::Image::from_rgba8_premultiplied(
        slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(&scaled, width, height),
    ))
}

pub fn register(app: &crate::App) {
    let cache = RefCell::new(Cache::default());
    let brand = app.global::<crate::Brand>();
    brand.on_logo(move |light, width| cache.borrow_mut().get(light, width).unwrap_or_default());
    brand.on_custom(|revision, width, height| {
        CUSTOM
            .with(|custom| custom.borrow_mut().raster(revision, width, height))
            .unwrap_or_default()
    });
}

/// The user's logo image: the one source both heads draw from, and its
/// rasters at the sizes they paint it.
#[derive(Default)]
struct Custom {
    /// The file the source is (or is being) read from.
    path: String,
    source: Option<Arc<image::RgbaImage>>,
    /// The file could not be read or decoded.
    failed: bool,
    /// Matches a decode still running for `path`; any other is stale.
    ticket: u64,
    revision: i32,
    rasters: VecDeque<(i32, u32, u32, slint::Image)>,
    bytes: usize,
}

thread_local! {
    static CUSTOM: RefCell<Custom> = RefCell::new(Custom::default());
}

impl Custom {
    fn raster(&mut self, revision: i32, width: f32, height: f32) -> Option<slint::Image> {
        let source = self.source.clone()?;
        if revision != self.revision || !width.is_finite() || !height.is_finite() {
            return None;
        }
        let width = (width.round().max(1.0) as u32).min(MAX_WIDTH);
        let height = (height.round().max(1.0) as u32).min(MAX_WIDTH);
        if let Some(index) = self
            .rasters
            .iter()
            .position(|r| (r.0, r.1, r.2) == (revision, width, height))
        {
            let entry = self.rasters.remove(index)?;
            let image = entry.3.clone();
            self.rasters.push_back(entry);
            return Some(image);
        }
        let scaled = resize_premultiplied((*source).clone(), width, height);
        let image = slint::Image::from_rgba8_premultiplied(slint::SharedPixelBuffer::<
            slint::Rgba8Pixel,
        >::clone_from_slice(
            &scaled, width, height
        ));
        let bytes = width as usize * height as usize * 4;
        while self.bytes + bytes > MAX_BYTES || self.rasters.len() >= MAX_ENTRIES {
            let (_, w, h, _) = self.rasters.pop_front()?;
            self.bytes -= w as usize * h as usize * 4;
        }
        self.bytes += bytes;
        self.rasters
            .push_back((revision, width, height, image.clone()));
        Some(image)
    }

    fn set_source(&mut self, source: Option<image::RgbaImage>) {
        self.failed = source.is_none();
        self.source = source.map(Arc::new);
        self.revision = self.revision.wrapping_add(1);
        self.rasters.clear();
        self.bytes = 0;
    }

    fn clear(&mut self) {
        self.ticket = self.ticket.wrapping_add(1);
        self.path.clear();
        self.set_source(None);
        self.failed = false;
    }
}

/// Publish what the header shows for the logo settings as saved. A custom
/// image already loaded from `image` shows at once; another is read off the
/// event loop, and the box stays empty until it lands. The CRT head copies
/// `Brand` from this one (`dual_head`) and draws from the same source.
pub(crate) fn apply(app: &crate::App, setting: &str, image: &str) {
    let shown = branding::shown(setting, image);
    let brand = app.global::<crate::Brand>();
    if shown != HeaderLogo::Custom {
        CUSTOM.with(|custom| custom.borrow_mut().clear());
        brand.set_mode(shown.into());
        return;
    }
    let image = image.trim().to_string();
    let ticket = CUSTOM.with(|custom| {
        let mut custom = custom.borrow_mut();
        if custom.path == image && (custom.source.is_some() || custom.failed) {
            return None;
        }
        custom.ticket = custom.ticket.wrapping_add(1);
        custom.path.clone_from(&image);
        custom.set_source(None);
        custom.failed = false;
        Some(custom.ticket)
    });
    let Some(ticket) = ticket else {
        publish(app);
        return;
    };
    brand.set_mode(crate::HeaderLogo::Custom);
    brand.set_revision(CUSTOM.with(|custom| custom.borrow().revision));
    let weak = app.as_weak();
    let spawned = std::thread::Builder::new()
        .name("header-logo".into())
        .spawn(move || {
            let source = decode(Path::new(&image));
            let _ = weak.upgrade_in_event_loop(move |app| {
                let current = CUSTOM.with(|custom| {
                    let mut custom = custom.borrow_mut();
                    let current = custom.ticket == ticket;
                    if current {
                        if source.is_none() {
                            tracing::warn!("header logo image could not be read: {}", custom.path);
                        }
                        custom.set_source(source);
                    }
                    current
                });
                if current {
                    publish(&app);
                }
            });
        });
    if let Err(error) = spawned {
        tracing::warn!("header logo: could not start the reader: {error}");
        CUSTOM.with(|custom| custom.borrow_mut().set_source(None));
        publish(app);
    }
}

/// Read and decode `image` here and now, then publish it: the offline
/// snapshot tool's path, which has no second turn to wait for.
#[allow(dead_code, reason = "used by the snapshot tool only")]
pub(crate) fn apply_now(app: &crate::App, image: &str) {
    CUSTOM.with(|custom| {
        let mut custom = custom.borrow_mut();
        custom.clear();
        custom.path = image.to_string();
        custom.set_source(decode(Path::new(image)));
    });
    publish(app);
}

/// Push the loaded custom image to the view: its aspect and revision, or
/// the Zaparoo logo when it failed to load.
fn publish(app: &crate::App) {
    let brand = app.global::<crate::Brand>();
    CUSTOM.with(|custom| {
        let custom = custom.borrow();
        if let Some(source) = &custom.source {
            brand.set_custom_aspect(source.width() as f32 / source.height().max(1) as f32);
            brand.set_revision(custom.revision);
            brand.set_mode(crate::HeaderLogo::Custom);
        } else if custom.failed {
            brand.set_mode(crate::HeaderLogo::Zaparoo);
        }
    });
}

/// Read a logo image within the customization limits and bring it down to
/// `SOURCE_WIDTH` x `SOURCE_HEIGHT`, keeping its aspect. Straight alpha.
fn decode(path: &Path) -> Option<image::RgbaImage> {
    let svg = path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("svg"));
    let limit = if svg {
        limits::ART_SVG_BYTES
    } else {
        limits::ART_FILE_BYTES
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
    if svg {
        return decode_svg(&bytes);
    }
    let mut reader = image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .ok()?;
    let mut decode_limits = image::Limits::default();
    decode_limits.max_alloc = Some(limits::ART_DECODE_BYTES);
    decode_limits.max_image_width = Some(limits::ART_SOURCE_EDGE);
    decode_limits.max_image_height = Some(limits::ART_SOURCE_EDGE);
    reader.limits(decode_limits);
    let decoded = reader.decode().ok()?;
    if decoded.width() == 0 || decoded.height() == 0 {
        return None;
    }
    // thumbnail keeps the aspect and never enlarges a small bitmap.
    let fitted = if decoded.width() > SOURCE_WIDTH || decoded.height() > SOURCE_HEIGHT {
        decoded.thumbnail(SOURCE_WIDTH, SOURCE_HEIGHT)
    } else {
        decoded
    };
    Some(fitted.to_rgba8())
}

/// Render an SVG at its own aspect, as large as the source bounds allow.
/// Documents that reference other files or carry filters are refused, as
/// the customization pipeline refuses them.
fn decode_svg(bytes: &[u8]) -> Option<image::RgbaImage> {
    let svg = std::str::from_utf8(bytes).ok()?;
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
    if external.load(Ordering::Relaxed) || !tree.filters().is_empty() {
        return None;
    }
    let size = tree.size();
    let scale = (SOURCE_WIDTH as f32 / size.width()).min(SOURCE_HEIGHT as f32 / size.height());
    if !scale.is_finite() || scale <= 0.0 {
        return None;
    }
    let width = (size.width() * scale).round().max(1.0) as u32;
    let height = (size.height() * scale).round().max(1.0) as u32;
    let mut pixmap = resvg::tiny_skia::Pixmap::new(width, height)?;
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );
    // tiny-skia paints premultiplied; the resize expects straight alpha.
    let mut pixels = pixmap.take();
    for pixel in pixels.chunks_exact_mut(4) {
        let alpha = u16::from(pixel[3]);
        if alpha > 0 && alpha < 255 {
            for channel in &mut pixel[..3] {
                *channel = ((u16::from(*channel) * 255 + alpha / 2) / alpha).min(255) as u8;
            }
        }
    }
    image::RgbaImage::from_raw(width, height, pixels)
}

#[cfg(test)]
#[allow(clippy::expect_used, reason = "embedded logo preparation must succeed")]
mod tests {
    use super::*;

    #[test]
    fn rasters_match_paint_size_and_keep_valid_antialiased_edges() {
        for light in [false, true] {
            let image = prepare(light, 178).expect("embedded brand");
            assert_eq!((image.size().width, image.size().height), (178, 40));
            let pixels = image.to_rgba8_premultiplied().expect("CPU pixels");
            assert!(pixels.as_slice().iter().any(|p| p.a > 0 && p.a < 255));
            assert!(pixels
                .as_slice()
                .iter()
                .all(|p| p.r <= p.a && p.g <= p.a && p.b <= p.a));
        }
    }

    #[test]
    fn cache_reuses_images_evicts_lru_and_bounds_sizes_and_bytes() {
        let mut cache = Cache::default();
        let first = cache.get(false, 178.0).expect("first image");
        for width in [200.0, 250.0, 300.0] {
            cache.get(false, width).expect("additional size");
        }
        assert_eq!(cache.get(false, 178.0), Some(first.clone()));
        cache.get(true, 178.0).expect("other surface");
        assert!(cache.entries.iter().all(|e| e.width != 200));
        assert_eq!(cache.get(false, 178.0), Some(first));
        for width in [600.0, 800.0, 900.0, f32::MAX] {
            cache.get(false, width).expect("bounded raster");
            assert!(cache.bytes <= MAX_BYTES);
            assert!(cache.entries.len() <= MAX_ENTRIES);
            assert!(cache.entries.iter().all(|e| e.width <= MAX_WIDTH));
        }
        for invalid in [0.0, -1.0, f32::NAN, f32::INFINITY] {
            assert!(cache.get(false, invalid).is_none());
        }
    }

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("zaparoo-brand-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch dir");
        dir
    }

    #[test]
    fn a_custom_logo_keeps_its_aspect_within_the_source_bounds() {
        let dir = scratch("decode");
        let png = dir.join("wide.png");
        image::RgbaImage::from_pixel(4000, 500, image::Rgba([200, 10, 10, 255]))
            .save(&png)
            .expect("PNG");
        let decoded = decode(&png).expect("bitmap");
        assert_eq!((decoded.width(), decoded.height()), (1024, 128));
        let tall = dir.join("tall.png");
        image::RgbaImage::from_pixel(300, 600, image::Rgba([0, 0, 0, 255]))
            .save(&tall)
            .expect("PNG");
        let decoded = decode(&tall).expect("bitmap");
        assert_eq!((decoded.width(), decoded.height()), (128, 256));
        // An SVG keeps its own aspect instead of a square canvas.
        let svg = dir.join("logo.svg");
        std::fs::write(
            &svg,
            r#"<svg xmlns="http://www.w3.org/2000/svg" width="40" height="10"><rect width="40" height="10" fill="white"/></svg>"#,
        )
        .expect("SVG");
        let decoded = decode(&svg).expect("SVG");
        assert_eq!((decoded.width(), decoded.height()), (1024, 256));
        assert_eq!(decoded.get_pixel(512, 128).0, [255, 255, 255, 255]);
        let broken = dir.join("broken.png");
        std::fs::write(&broken, b"not an image").expect("file");
        assert!(decode(&broken).is_none());
        assert!(decode(&dir.join("missing.png")).is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn custom_rasters_follow_the_revision_and_paint_size() {
        let mut custom = Custom::default();
        assert!(custom.raster(0, 40.0, 10.0).is_none());
        custom.set_source(Some(image::RgbaImage::from_pixel(
            400,
            100,
            image::Rgba([255, 255, 255, 255]),
        )));
        let revision = custom.revision;
        let image = custom.raster(revision, 40.0, 10.0).expect("raster");
        assert_eq!((image.size().width, image.size().height), (40, 10));
        assert_eq!(custom.raster(revision, 40.0, 10.0), Some(image));
        // A stale revision draws nothing rather than the wrong image.
        assert!(custom.raster(revision - 1, 40.0, 10.0).is_none());
        for width in [100.0, 200.0, 300.0, 400.0, 2000.0] {
            custom.raster(revision, width, width / 4.0).expect("raster");
            assert!(custom.bytes <= MAX_BYTES);
            assert!(custom.rasters.len() <= MAX_ENTRIES);
        }
        custom.clear();
        assert!(custom.raster(custom.revision, 40.0, 10.0).is_none());
        assert!(!custom.failed);
    }
}
