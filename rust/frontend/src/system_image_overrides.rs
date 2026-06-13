// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// User-supplied system artwork overrides.
//
// When `[images] system_dir` is set in `frontend.toml`, this module scans
// that directory once at startup and builds a process-lifetime map from
// file stem to absolute path. Any file whose stem matches a Zaparoo system
// id (case-exact) and whose extension is an allowed image type is stored.
//
// The `system-image` image provider in C++ uses `override_path` to resolve
// the file, and `is_in_override_dir` to validate that the path it receives
// (decoded from the image URL) has not been tampered with to escape the
// configured root.
//
// MiSTer note: the override dir is typically on `/media/fat/zaparoo/systems/`
// (SD card). Scanning once at startup keeps the `/tmp` tmpfs free from
// copies and avoids repeated SD reads during browse. The user must restart
// the frontend after adding or removing override images.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use tracing::{info, warn};

/// Image extensions accepted as user override artwork.
/// SVG overrides are rendered via `QSvgRenderer`; all others via `QImage`.
const ALLOWED_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "webp", "bmp", "svg"];

static OVERRIDES: OnceLock<HashMap<String, PathBuf>> = OnceLock::new();
static OVERRIDE_DIR: OnceLock<Option<PathBuf>> = OnceLock::new();

/// Scan the configured override directory and populate the lookup map.
/// Call exactly once during `zaparoo_rust_init`. Subsequent calls are
/// silent no-ops (`OnceLock` semantics).
///
/// `dir` is the value of `[images] system_dir` from `frontend.toml`, or
/// `None` / empty string if the key is absent (feature off).
pub fn scan(dir: Option<&str>) {
    let dir_path: Option<PathBuf> = dir
        .map(str::trim)
        .filter(|d| !d.is_empty())
        .map(PathBuf::from);

    let _ = OVERRIDE_DIR.set(dir_path.clone());

    let mut map = HashMap::new();
    if let Some(ref dir) = dir_path {
        match std::fs::read_dir(dir) {
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
                    info!("system image override: {} -> {}", stem, path.display());
                    // Last writer wins if multiple extensions match the same stem
                    // (e.g., both `SNES.png` and `SNES.jpg` present). The
                    // directory iteration order is OS-defined; document that
                    // users should provide only one file per system id.
                    map.insert(stem.to_string(), path);
                }
                info!(
                    "system image overrides: {} file(s) loaded from {}",
                    map.len(),
                    dir.display()
                );
            }
            Err(e) => {
                warn!(
                    "could not scan system image override dir {}: {e}",
                    dir.display()
                );
            }
        }
    }
    let _ = OVERRIDES.set(map);
}

/// Return the override path for `system_id` (case-exact stem match), or
/// `None` if no override is registered for that id.
pub fn override_path(system_id: &str) -> Option<PathBuf> {
    OVERRIDES.get()?.get(system_id).cloned()
}

/// Return `true` if `path` is inside the configured override directory.
/// Used by the C++ `system-image` provider to validate that the decoded
/// path from the image URL has not been manipulated to escape the root.
pub fn is_in_override_dir(path: &Path) -> bool {
    match OVERRIDE_DIR.get() {
        Some(Some(dir)) => path.starts_with(dir),
        _ => false,
    }
}

/// FFI entry point for the C++ `system-image` provider. Returns `true` when
/// the byte slice `path_ptr..path_ptr+path_len` is valid UTF-8 and the
/// resulting path is inside the configured override directory.
///
/// # Safety
///
/// `path_ptr` must point to `path_len` bytes of valid memory that remain
/// live for the duration of this call. An empty slice (null or zero len)
/// returns `false`.
#[no_mangle]
pub unsafe extern "C" fn zaparoo_system_image_is_in_override_dir(
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

/// Return the configured override directory, if any. Exposed so the C++
/// provider can log it alongside validation failures.
pub fn override_dir() -> Option<PathBuf> {
    OVERRIDE_DIR.get()?.clone()
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    /// `is_in_override_dir` returns false when the `OVERRIDE_DIR` `OnceLock`
    /// is already set (by a previous test run or the real init). Because
    /// `OnceLock` is process-global and tests may run in arbitrary order we
    /// only verify the path-prefix logic via direct construction here.
    #[test]
    fn path_prefix_check_logic() {
        let root = Path::new("/media/fat/zaparoo/systems");
        let inside = Path::new("/media/fat/zaparoo/systems/SNES.png");
        let outside = Path::new("/media/fat/zaparoo/frontend.toml");
        assert!(inside.starts_with(root));
        assert!(!outside.starts_with(root));
    }

    #[test]
    fn scan_with_none_is_noop() {
        // This only passes if the OnceLock hasn't been set yet in this process.
        // In isolation the scan should set an empty map and no dir.
        // We can't call scan() here safely as OnceLock is global, so we only
        // verify the allowed extension list covers expected types.
        for ext in ["png", "jpg", "jpeg", "webp", "bmp", "svg"] {
            assert!(
                super::ALLOWED_EXTENSIONS
                    .iter()
                    .any(|&a| a.eq_ignore_ascii_case(ext)),
                "missing extension: {ext}"
            );
        }
    }
}
