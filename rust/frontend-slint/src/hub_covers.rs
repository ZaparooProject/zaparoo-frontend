// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// The Hub/Resume cold-boot cover manifest (`hub_cover_manifest.rs`): a
// small list of paths - never image bytes - so the Hub's game tiles and
// the Resume tile can paint their real art on the first frame after a
// `MiSTer` boot instead of a placeholder glyph until Core answers.
//
// The bytes are already on the SD card before this process starts:
// they are Core's own thumbnail cache, and the frontend can open them
// directly. What is missing at boot is knowing which file, without a
// round trip first; this is that mapping.
//
// Core calls a `localPath` "opaque, transient and nonportable" - this
// persists one anyway, deliberately and with its author's agreement.
// The design accounts for why that warning exists: Core renames its
// thumbnail cache aside on every rescan, so a stored path goes stale
// routinely. A stale path simply fails to open and falls through to the
// ordinary request for that one tile: one extra round trip, never a
// wrong image.
//
// `CLAUDE.md`'s "do not persist Core metadata to disk" rule carries an
// explicit, scoped exception for exactly this file.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use zaparoo_app::covers::{HUB_TILE_MAX_SIZE, MAX_HUB_ENTRIES, MAX_LOCAL_IMAGE_BYTES};
use zaparoo_core::hub_layout::HubItemKind;
use zaparoo_core::media_types::MediaImageParams;

use crate::media_cache::{MediaCache, MediaKey};
use crate::router::{lock, Ctx};

const MANIFEST_FILE_NAME: &str = "hub_covers.toml";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
struct Manifest {
    hub_entries: Vec<ManifestEntry>,
    resume: Option<ManifestEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct ManifestEntry {
    system_id: String,
    path: String,
    local_path: String,
    max_size: u32,
}

fn manifest_path() -> PathBuf {
    zaparoo_core::platform_paths::cache_dir().join(MANIFEST_FILE_NAME)
}

/// Only a colocated `MiSTer` has the files to point at; everywhere else
/// this whole module is a no-op.
fn eligible() -> bool {
    crate::media_cache::should_request_local_path(HUB_TILE_MAX_SIZE)
}

/// Split from `manifest_path` so tests can drive it against their own
/// directory, the way `zaparoo_core::persist` splits its own IO.
fn read_manifest_from(path: &Path) -> Manifest {
    let Ok(contents) = std::fs::read_to_string(path) else {
        return Manifest::default();
    };
    toml::from_str(&contents).unwrap_or_default()
}

/// Write-temp-then-rename, so a kill mid-write cannot leave a half
/// written manifest for the next boot to trip over.
fn write_manifest_to(path: &Path, manifest: &Manifest) {
    use std::io::Write as _;

    let Ok(serialized) = toml::to_string_pretty(manifest) else {
        return;
    };
    let Some(parent) = path.parent() else { return };
    if std::fs::create_dir_all(parent).is_err() {
        return;
    }
    let tmp = parent.join(format!(".{MANIFEST_FILE_NAME}.tmp.{}", std::process::id()));
    let write = (|| -> std::io::Result<()> {
        let mut file = std::fs::File::create(&tmp)?;
        file.write_all(serialized.as_bytes())?;
        file.sync_all()?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    })();
    if let Err(e) = write {
        tracing::warn!("hub covers: could not write the manifest: {e}");
        let _ = std::fs::remove_file(&tmp);
    }
}

fn entry_key(entry: &ManifestEntry) -> MediaKey {
    MediaKey {
        media_id: None,
        system: entry.system_id.clone(),
        path: entry.path.clone(),
        max_size: entry.max_size,
    }
}

/// Open every path the manifest names and seed the in-memory cache, so
/// the Hub's first paint already has the art. Called once at startup,
/// synchronously: these are small, local, sequential reads, and the
/// whole point is to be done before the first frame.
pub fn seed(cache: &MediaCache) {
    if !eligible() {
        return;
    }
    let manifest = read_manifest_from(&manifest_path());
    let mut seeded = 0usize;
    for entry in manifest.hub_entries.into_iter().chain(manifest.resume) {
        let key = entry_key(&entry);
        if cache.is_cached(&key) {
            continue;
        }
        match crate::media_cache::read_local_image_file(&entry.local_path, MAX_LOCAL_IMAGE_BYTES) {
            Ok(bytes) => {
                if let Some(image) = crate::media_cache::decode_bytes(&bytes) {
                    cache.seed(key, image);
                    seeded += 1;
                } else {
                    tracing::debug!(path = %entry.local_path, "hub covers: undecodable");
                }
            }
            Err(e) => tracing::debug!(
                path = %entry.local_path,
                "hub covers: stale entry, falling through to Core: {e}"
            ),
        }
    }
    if seeded > 0 {
        tracing::debug!(seeded, "hub covers: seeded from the manifest");
    }
}

/// Ask Core for one thumbnail's path without touching the ordinary
/// fetch queue. Only ever runs while refreshing the manifest, which is
/// a layout change or a new resume entry, not a per-tile cost.
async fn fetch_local_path(
    client: &zaparoo_core::client::Client,
    preferred: &str,
    system_id: &str,
    path: &str,
) -> Option<String> {
    let mut image_types: Vec<String> = ["boxart", "image", "thumbnail", "boxart3d", "screenshot"]
        .iter()
        .map(ToString::to_string)
        .collect();
    if !preferred.is_empty() && preferred != "auto" {
        image_types.retain(|t| t != preferred);
        image_types.insert(0, preferred.to_string());
    }
    let result = client
        .media_image(MediaImageParams {
            media_id: None,
            system: system_id.to_string(),
            path: path.to_string(),
            image_types,
            max_size: Some(HUB_TILE_MAX_SIZE),
            delivery: Some(zaparoo_app::covers::DELIVERY_LOCAL_PATH.to_string()),
        })
        .await
        .ok()?;
    if result.delivery != zaparoo_app::covers::DELIVERY_LOCAL_PATH {
        return None;
    }
    result.local_path.filter(|p| !p.is_empty())
}

/// Rebuild the tile half of the manifest from the Hub's current game
/// shortcuts, leaving the resume half alone. Fired on a real layout
/// change, not on every cover fetch.
pub fn refresh_hub_entries(ctx: &Ctx) {
    if !eligible() {
        return;
    }
    let targets: Vec<(String, String)> = {
        let shared = lock(&ctx.shared);
        shared
            .hub
            .layout
            .visible()
            .filter(|item| {
                item.kind() == HubItemKind::ZapScript
                    && !item.system.is_empty()
                    && !item.path.is_empty()
            })
            .take(MAX_HUB_ENTRIES)
            .map(|item| (item.system.clone(), item.path.clone()))
            .collect()
    };
    if targets.is_empty() {
        let mut manifest = read_manifest_from(&manifest_path());
        if !manifest.hub_entries.is_empty() {
            manifest.hub_entries.clear();
            write_manifest_to(&manifest_path(), &manifest);
        }
        return;
    }
    let client = ctx.store.client();
    let preferred = lock(&ctx.shared).persist.settings.media_image_type.clone();
    ctx.handle.spawn(async move {
        let mut hub_entries = Vec::with_capacity(targets.len());
        for (system_id, path) in targets {
            let Some(local_path) = fetch_local_path(&client, &preferred, &system_id, &path).await
            else {
                continue;
            };
            hub_entries.push(ManifestEntry {
                system_id,
                path,
                local_path,
                max_size: HUB_TILE_MAX_SIZE,
            });
        }
        let mut manifest = read_manifest_from(&manifest_path());
        manifest.hub_entries = hub_entries;
        write_manifest_to(&manifest_path(), &manifest);
    });
}

/// Update or clear the resume half. `None` means there is nothing to
/// resume.
pub fn refresh_resume_entry(ctx: &Ctx, target: Option<(String, String)>) {
    if !eligible() {
        return;
    }
    let Some((system_id, path)) = target else {
        let mut manifest = read_manifest_from(&manifest_path());
        if manifest.resume.take().is_some() {
            write_manifest_to(&manifest_path(), &manifest);
        }
        return;
    };
    let client = ctx.store.client();
    let preferred = lock(&ctx.shared).persist.settings.media_image_type.clone();
    ctx.handle.spawn(async move {
        let local_path = fetch_local_path(&client, &preferred, &system_id, &path).await;
        let mut manifest = read_manifest_from(&manifest_path());
        manifest.resume = local_path.map(|local_path| ManifestEntry {
            system_id,
            path,
            local_path,
            max_size: HUB_TILE_MAX_SIZE,
        });
        write_manifest_to(&manifest_path(), &manifest);
    });
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    reason = "tests should fail fast on a filesystem they just set up"
)]
mod tests {
    use super::*;

    #[test]
    fn a_manifest_round_trips_through_its_file() {
        let dir = std::env::temp_dir().join("zaparoo-hub-covers-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch dir");
        let path = dir.join(MANIFEST_FILE_NAME);
        let manifest = Manifest {
            hub_entries: vec![ManifestEntry {
                system_id: "SNES".into(),
                path: "/games/SNES/Zelda.sfc".into(),
                local_path: "/media/fat/zaparoo/cache/thumbs/v2/a/b.png".into(),
                max_size: HUB_TILE_MAX_SIZE,
            }],
            resume: None,
        };
        write_manifest_to(&path, &manifest);
        let read = read_manifest_from(&path);
        assert_eq!(read.hub_entries.len(), 1);
        assert_eq!(read.hub_entries[0].system_id, "SNES");
        assert_eq!(read.hub_entries[0].max_size, HUB_TILE_MAX_SIZE);
        assert!(read.resume.is_none());
        // A missing or corrupt file reads as an empty manifest rather
        // than an error: the tiles just fetch normally.
        std::fs::write(&path, b"not toml at all {{{").expect("write");
        assert!(read_manifest_from(&path).hub_entries.is_empty());
        assert!(read_manifest_from(&dir.join("absent.toml"))
            .hub_entries
            .is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
