// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// User overrides from the customization folder (`docs/customization.md`):
// artwork under `systems/` and `hub/`, plus the `[custom.system_names]`
// display names. The folder is scanned once, off the event loop, after
// the first frame - on `MiSTer` it lives on the SD card and the frontend
// must not wait on it to paint. Overrides are user files, so they are
// read from disk rather than embedded, and they are never tinted: what
// the user supplied is what shows.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{OnceLock, RwLock};

use zaparoo_app::customization as rules;

/// `(namespace, key) -> file`, filled by the scan.
static ART: OnceLock<RwLock<HashMap<(String, String), PathBuf>>> = OnceLock::new();
static ROOT: OnceLock<PathBuf> = OnceLock::new();
static NAMES: OnceLock<HashMap<String, String>> = OnceLock::new();

fn art() -> &'static RwLock<HashMap<(String, String), PathBuf>> {
    ART.get_or_init(|| RwLock::new(HashMap::new()))
}

/// Register the root and the name table. Cheap: no filesystem access.
pub fn configure<S: std::hash::BuildHasher>(root: PathBuf, names: HashMap<String, String, S>) {
    let _ = ROOT.set(root);
    let _ = NAMES.set(rules::normalize_system_names(names));
}

/// A system's user display name, or `None` to fall back to the
/// localized and Core names.
pub fn system_name(system_id: &str) -> Option<String> {
    let names = NAMES.get()?;
    rules::system_name(names, system_id).map(str::to_string)
}

/// Walk both namespaces and record what is there. Blocking; the caller
/// runs it off the event loop.
pub fn scan() -> usize {
    let Some(root) = ROOT.get() else {
        return 0;
    };
    let mut found = HashMap::new();
    for namespace in rules::NAMESPACES {
        found.extend(scan_namespace(root, namespace));
    }
    let count = found.len();
    match art().write() {
        Ok(mut map) => *map = found,
        Err(e) => tracing::warn!("image override map poisoned: {e}"),
    }
    tracing::info!("image overrides: {count} file(s) found");
    count
}

fn scan_namespace(root: &Path, namespace: &str) -> HashMap<(String, String), PathBuf> {
    let mut map = HashMap::new();
    let dir = root.join(namespace);
    let Ok(entries) = std::fs::read_dir(&dir) else {
        // The normal zero-config case: no folder, no overrides.
        tracing::debug!("no {namespace} overrides in {}", dir.display());
        return map;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(extension) = path.extension().and_then(|e| e.to_str()) else {
            continue;
        };
        if !rules::is_allowed_extension(extension) {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        tracing::info!("{namespace} image override: {stem} -> {}", path.display());
        map.insert((namespace.to_string(), rules::match_key(stem)), path);
    }
    map
}

fn override_path(namespace: &str, id: &str) -> Option<PathBuf> {
    art()
        .read()
        .ok()?
        .get(&(namespace.to_string(), rules::match_key(id)))
        .cloned()
}

thread_local! {
    /// Decoded overrides, kept per id. `slint::Image` is not `Send`, so
    /// the cache lives on the event loop thread that draws them.
    static DECODED: std::cell::RefCell<HashMap<(String, String), Option<slint::Image>>> =
        std::cell::RefCell::new(HashMap::new());
}

/// The user's artwork for an id, decoded and cached, or `None` when
/// there is no override (the normal case).
fn image_for(namespace: &str, id: &str) -> Option<slint::Image> {
    let key = (namespace.to_string(), rules::match_key(id));
    if let Some(hit) = DECODED.with(|c| c.borrow().get(&key).cloned()) {
        return hit;
    }
    let decoded = override_path(namespace, id).and_then(|path| decode(&path));
    DECODED.with(|c| c.borrow_mut().insert(key, decoded.clone()));
    decoded
}

pub fn system_image(system_id: &str) -> Option<slint::Image> {
    image_for("systems", system_id)
}

pub fn hub_image(id: &str) -> Option<slint::Image> {
    image_for("hub", id)
}

/// Decode one override file. SVG goes through the same rasterizer the
/// bundled glyphs use, at the tallest size a tile can ask for; the
/// bitmap formats decode at their native size and the view scales them.
fn decode(path: &Path) -> Option<slint::Image> {
    const SVG_PX: u32 = 256;
    let extension = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    if extension.eq_ignore_ascii_case("svg") {
        let svg = std::fs::read_to_string(path).ok()?;
        return crate::glyphs::rasterize_svg(&svg, SVG_PX);
    }
    let bytes = std::fs::read(path).ok()?;
    let decoded = image::load_from_memory(&bytes)
        .map_err(|e| tracing::warn!("override {} could not be decoded: {e}", path.display()))
        .ok()?
        .to_rgba8();
    let (width, height) = decoded.dimensions();
    Some(slint::Image::from_rgba8(slint::SharedPixelBuffer::<
        slint::Rgba8Pixel,
    >::clone_from_slice(
        &decoded.into_raw(),
        width,
        height,
    )))
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    reason = "tests should fail fast on a filesystem they just set up"
)]
mod tests {
    use super::*;

    /// A scratch folder for one test, removed on the way out. Nextest
    /// runs each test in its own process, so the globals stay clean.
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(name: &str) -> Self {
            let path = std::env::temp_dir().join(format!("zaparoo-custom-{name}"));
            let _ = std::fs::remove_dir_all(&path);
            std::fs::create_dir_all(&path).expect("scratch dir");
            Self(path)
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn a_scan_keys_by_namespace_and_lowercased_stem() {
        let scratch = Scratch::new("scan");
        let root = &scratch.0;
        std::fs::create_dir_all(root.join("systems")).expect("systems");
        std::fs::create_dir_all(root.join("hub")).expect("hub");
        std::fs::write(root.join("systems/SNES.PNG"), b"x").expect("write");
        std::fs::write(root.join("hub/Favorites.svg"), b"<svg/>").expect("write");
        // Not an image type, and so not an override.
        std::fs::write(root.join("systems/notes.txt"), b"x").expect("write");

        let systems = scan_namespace(root, "systems");
        let hub = scan_namespace(root, "hub");
        assert_eq!(
            systems.get(&("systems".to_string(), "snes".to_string())),
            Some(&root.join("systems/SNES.PNG"))
        );
        assert_eq!(systems.len(), 1, "the text file is not artwork");
        assert_eq!(
            hub.get(&("hub".to_string(), "favorites".to_string())),
            Some(&root.join("hub/Favorites.svg"))
        );
        // A missing folder is the zero-config case, not an error.
        assert!(scan_namespace(root, "nothing-here").is_empty());
    }

    #[test]
    fn an_override_decodes_to_an_image() {
        let scratch = Scratch::new("decode");
        let root = &scratch.0;
        let png = root.join("art.png");
        let pixels = image::RgbaImage::from_pixel(4, 2, image::Rgba([9, 9, 9, 255]));
        pixels.save(&png).expect("encode");
        let decoded = decode(&png).expect("decoded");
        assert_eq!(decoded.size().width, 4);
        assert_eq!(decoded.size().height, 2);

        let svg = root.join("art.svg");
        std::fs::write(
            &svg,
            br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"><rect width="10" height="10" fill="#fff"/></svg>"##,
        )
        .expect("write");
        let decoded = decode(&svg).expect("rasterized");
        assert_eq!(decoded.size().width, decoded.size().height);

        // Anything that is not an image at all just has no override.
        let broken = root.join("art.webp");
        std::fs::write(&broken, b"not an image").expect("write");
        assert!(decode(&broken).is_none());
    }
}
