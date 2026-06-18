// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// User-supplied image overrides.
//
// The frontend scans a single customization root once at startup (the
// `[custom] dir` config value, or `platform_paths::custom_dir()` when
// unset). Inside that root, two namespaced subfolders hold override images:
//
//   * `systems/` — system artwork, keyed by Zaparoo system id (case-exact).
//   * `hub/`     — Hub icons, keyed by category id (`Arcade`, `Console`, …)
//                  or action id (`resume`, `favorites`, `recents`,
//                  `settings`).
//
// Any file whose stem matches an id and whose extension is an allowed image
// type is stored in a process-lifetime map keyed by `(namespace, stem)`.
//
// The `custom-image` image provider in C++ uses `override_path` to build the
// cover key and `is_in_override_dir` to validate that the path it receives
// (decoded from the image URL) has not been tampered with to escape the
// configured root.
//
// MiSTer note: the root is typically `/media/fat/zaparoo/custom/` (SD card).
// Scanning once at startup keeps the `/tmp` tmpfs free from copies and avoids
// repeated SD reads during browse. The user must restart the frontend after
// adding or removing override images. A missing root or subfolder is the
// normal zero-config case and is treated as "no overrides".

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use tracing::{debug, info};

/// Image extensions accepted as user override artwork.
/// SVG overrides are rendered via `QSvgRenderer`; all others via `QImage`.
const ALLOWED_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "webp", "bmp", "svg"];

/// Override namespaces. Each maps to a same-named subfolder under the root.
const NAMESPACES: &[&str] = &["systems", "hub"];

static OVERRIDES: OnceLock<HashMap<(String, String), PathBuf>> = OnceLock::new();
static OVERRIDE_ROOT: OnceLock<PathBuf> = OnceLock::new();

/// Normalize a file stem or lookup id for matching. Matching is
/// case-insensitive, so `Arcade.png`, `arcade.png`, and `ARCADE.png` all
/// resolve the `Arcade` category. Applied identically on scan and lookup so
/// the two can never drift.
fn match_key(id: &str) -> String {
    id.to_ascii_lowercase()
}

/// Scan `root`'s namespace subfolders and build the lookup map. Pure: takes a
/// root path and returns the map without touching globals, so it is unit
/// testable (the `OnceLock`-backed `scan` is process-global and set-once).
fn scan_root(root: &Path) -> HashMap<(String, String), PathBuf> {
    let mut map = HashMap::new();
    for &ns in NAMESPACES {
        let dir = root.join(ns);
        match std::fs::read_dir(&dir) {
            Ok(entries) => {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if !path.is_file() {
                        continue;
                    }
                    let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
                        continue;
                    };
                    if !ALLOWED_EXTENSIONS
                        .iter()
                        .any(|&a| a.eq_ignore_ascii_case(ext))
                    {
                        continue;
                    }
                    let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                        continue;
                    };
                    info!("{ns} image override: {} -> {}", stem, path.display());
                    // Last writer wins if multiple files map to the same key
                    // (different extensions like `SNES.png`/`SNES.jpg`, or just
                    // different case like `SNES.png`/`snes.png`). The directory
                    // iteration order is OS-defined; users should provide only
                    // one file per id.
                    map.insert((ns.to_string(), match_key(stem)), path);
                }
            }
            Err(e) => {
                // A missing subfolder is the common zero-config case, not an
                // error — log at debug so a real permission problem is still
                // discoverable without spamming every clean startup.
                debug!("no {ns} overrides in {}: {e}", dir.display());
            }
        }
    }
    map
}

/// Scan the configured customization root and populate the lookup map.
/// Call exactly once during `zaparoo_rust_init`. Subsequent calls are
/// silent no-ops (`OnceLock` semantics).
pub fn scan(root: &Path) {
    let _ = OVERRIDE_ROOT.set(root.to_path_buf());
    if !root.exists() {
        debug!(
            "customization root {} does not exist; no overrides loaded",
            root.display()
        );
    }
    let map = scan_root(root);
    info!(
        "image overrides: {} file(s) loaded from {}",
        map.len(),
        root.display()
    );
    let _ = OVERRIDES.set(map);
}

/// Return the override path for `id` in `namespace` (case-insensitive stem
/// match), or `None` if no override is registered.
pub fn override_path(namespace: &str, id: &str) -> Option<PathBuf> {
    OVERRIDES
        .get()?
        .get(&(namespace.to_string(), match_key(id)))
        .cloned()
}

/// Return `true` if `path` is inside the configured customization root.
/// Used by the C++ `custom-image` provider to validate that the decoded path
/// from the image URL has not been manipulated to escape the root.
///
/// Both the root and the candidate path are canonicalized first so the prefix
/// check resolves symlinks and `..` components to their true on-disk targets.
/// A lexical `starts_with` would let a symlink (or `custom/systems/../../etc`)
/// under the root point outside it and still pass. Canonicalization requires
/// the path to exist, so a missing file fails closed — which is correct, the
/// provider only ever serves files that do exist.
pub fn is_in_override_dir(path: &Path) -> bool {
    let Some(root) = OVERRIDE_ROOT.get() else {
        return false;
    };
    let (Ok(root), Ok(path)) = (root.canonicalize(), path.canonicalize()) else {
        return false;
    };
    path.starts_with(root)
}

/// FFI entry point for the C++ `custom-image` provider. Returns `true` when
/// the byte slice `path_ptr..path_ptr+path_len` is valid UTF-8 and the
/// resulting path is inside the configured customization root.
///
/// # Safety
///
/// `path_ptr` must point to `path_len` bytes of valid memory that remain
/// live for the duration of this call. An empty slice (null or zero len)
/// returns `false`.
#[no_mangle]
pub unsafe extern "C" fn zaparoo_override_image_is_in_override_dir(
    path_ptr: *const u8,
    path_len: usize,
) -> bool {
    if path_ptr.is_null() || path_len == 0 {
        return false;
    }
    // SAFETY: caller guarantees a valid slice for this call.
    let bytes = unsafe { std::slice::from_raw_parts(path_ptr, path_len) };
    let Ok(s) = std::str::from_utf8(bytes) else {
        return false;
    };
    is_in_override_dir(Path::new(s))
}

/// Return the configured customization root, if any. Exposed so the C++
/// provider can log it alongside validation failures.
pub fn override_dir() -> Option<PathBuf> {
    OVERRIDE_ROOT.get().cloned()
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::unwrap_used,
        clippy::panic,
        reason = "tests should fail-fast on unexpected errors"
    )]

    use super::{match_key, scan_root};
    use std::fs;

    #[test]
    fn match_key_lowercases() {
        assert_eq!(match_key("Arcade"), "arcade");
        assert_eq!(match_key("SNES"), "snes");
        assert_eq!(match_key("recents"), "recents");
    }

    #[test]
    fn scan_root_keys_by_namespace_and_lowercased_stem() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        fs::create_dir_all(root.join("systems")).expect("mk systems");
        fs::create_dir_all(root.join("hub")).expect("mk hub");
        // Mixed-case filenames are normalized to lowercase keys.
        fs::write(root.join("systems/SNES.png"), b"x").expect("write");
        fs::write(root.join("hub/Favorites.svg"), b"<svg/>").expect("write");

        let map = scan_root(root);
        assert_eq!(
            map.get(&("systems".to_string(), "snes".to_string())),
            Some(&root.join("systems/SNES.png"))
        );
        assert_eq!(
            map.get(&("hub".to_string(), "favorites".to_string())),
            Some(&root.join("hub/Favorites.svg"))
        );
        // A system file does not leak into the hub namespace.
        assert!(!map.contains_key(&("hub".to_string(), "snes".to_string())));
    }

    #[test]
    fn scan_root_ignores_disallowed_extensions() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        fs::create_dir_all(root.join("systems")).expect("mk systems");
        fs::write(root.join("systems/SNES.txt"), b"x").expect("write");
        fs::write(root.join("systems/Genesis.png"), b"x").expect("write");

        let map = scan_root(root);
        assert!(!map.contains_key(&("systems".to_string(), "snes".to_string())));
        assert_eq!(
            map.get(&("systems".to_string(), "genesis".to_string())),
            Some(&root.join("systems/Genesis.png"))
        );
    }

    #[test]
    fn scan_root_missing_subfolders_is_empty() {
        let dir = tempfile::tempdir().expect("tempdir");
        let map = scan_root(dir.path());
        assert!(map.is_empty());
    }

    #[test]
    fn scan_root_extension_and_filename_case_insensitive() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        fs::create_dir_all(root.join("systems")).expect("mk systems");
        // Both the extension case (.PNG) and the stem case (NES) are ignored.
        fs::write(root.join("systems/NES.PNG"), b"x").expect("write");
        let map = scan_root(root);
        assert_eq!(
            map.get(&("systems".to_string(), "nes".to_string())),
            Some(&root.join("systems/NES.PNG"))
        );
    }
}
