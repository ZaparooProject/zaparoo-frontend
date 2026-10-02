// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

//! Prepare the header/screensaver logo at its painted size. Slint's software
//! renderer samples scaled bitmap images with nearest-neighbor filtering,
//! which skips source rows even when the requested image rendering is smooth.
//! A bounded per-window cache pays for filtered resizing only on a size or
//! surface-color change, never on a moving screensaver frame.

use std::cell::RefCell;
use std::collections::VecDeque;

use slint::ComponentHandle;

const DARK: &[u8] = include_bytes!("../../../resources/images/logo/logo-on-dark-600.png");
const LIGHT: &[u8] = include_bytes!("../../../resources/images/logo/logo-on-light-600.png");
const MAX_WIDTH: u32 = 1024;
const MAX_ENTRIES: usize = 4;
const MAX_BYTES: usize = 2 * 1024 * 1024;

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

fn prepare(light: bool, width: u32) -> Option<slint::Image> {
    let mut source = image::load_from_memory(if light { LIGHT } else { DARK })
        .ok()?
        .to_rgba8();
    let height = (f64::from(width) * f64::from(source.height()) / f64::from(source.width()))
        .round()
        .max(1.0) as u32;
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
    Some(slint::Image::from_rgba8_premultiplied(
        slint::SharedPixelBuffer::<slint::Rgba8Pixel>::clone_from_slice(&scaled, width, height),
    ))
}

pub fn register(app: &crate::App) {
    let cache = RefCell::new(Cache::default());
    app.global::<crate::Brand>()
        .on_logo(move |light, width| cache.borrow_mut().get(light, width).unwrap_or_default());
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
}
