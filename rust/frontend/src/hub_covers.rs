// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// The Hub/Resume cold-boot cover manifest (`hub_cover_manifest.rs`): a
// small list of paths - never image bytes - so the Hub's game tiles and
// the Resume tile can paint their real art on the first frame after a
// cold start instead of a placeholder glyph until Core answers.
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
// The project's "do not persist Core metadata to disk" rule carries an
// explicit, scoped exception for exactly this file.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, PoisonError};

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

/// Every read-modify-write of the manifest holds this, so the tile half
/// and the resume half, refreshed by separate tasks, cannot overwrite
/// each other's update.
static MANIFEST_WRITE: Mutex<()> = Mutex::new(());
/// Each refresh of a half takes a new generation; a slower, older refresh
/// that finishes after a newer one drops its result instead of
/// replacing the newer one.
static HUB_GENERATION: AtomicU64 = AtomicU64::new(0);
static RESUME_GENERATION: AtomicU64 = AtomicU64::new(0);
/// The first Hub rebuild checks once whether the manifest has a tile
/// half at all; later ones rely on layout changes.
static HUB_CHECKED: AtomicBool = AtomicBool::new(false);

fn manifest_path() -> PathBuf {
    zaparoo_core::platform_paths::cache_dir().join(MANIFEST_FILE_NAME)
}

/// Only a frontend that can open Core's files (a colocated `MiSTer`, or an
/// embedding host running Core as the same app) has the files to point
/// at; everywhere else this whole module is a no-op.
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

/// Read, change and write the manifest at `path` as one step against
/// every other writer in this process. `update` returns false to leave
/// the file as it was.
fn update_manifest_at(path: &Path, update: impl FnOnce(&mut Manifest) -> bool) {
    let _guard = MANIFEST_WRITE
        .lock()
        .unwrap_or_else(PoisonError::into_inner);
    let mut manifest = read_manifest_from(path);
    if update(&mut manifest) {
        write_manifest_to(path, &manifest);
    }
}

fn update_manifest(update: impl FnOnce(&mut Manifest) -> bool) {
    update_manifest_at(&manifest_path(), update);
}

fn entry_key(entry: &ManifestEntry) -> MediaKey {
    MediaKey {
        media_id: None,
        system: entry.system_id.clone(),
        path: entry.path.clone(),
        max_size: entry.max_size,
        image_type: None,
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
            delivery: Some(zaparoo_core::media_types::MEDIA_IMAGE_DELIVERY_LOCAL_PATH.to_string()),
        })
        .await
        .ok()?;
    if result.delivery != zaparoo_core::media_types::MEDIA_IMAGE_DELIVERY_LOCAL_PATH {
        return None;
    }
    result.local_path.filter(|p| !p.is_empty())
}

/// The Hub's game shortcuts the tile half covers, in layout order.
fn hub_targets(ctx: &Ctx) -> Vec<(String, String)> {
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
}

/// Resolve once the client has a live link, so a refresh fired while
/// Core is still starting does not record every tile as missing.
async fn wait_for_core(client: &zaparoo_core::client::Client) {
    let mut state = client.connection.subscribe();
    let _ = state
        .wait_for(|s| *s == zaparoo_core::client::ConnectionState::Connected)
        .await;
}

/// The first Hub rebuild of the process: write the tile half when the
/// manifest has none yet, so a Hub whose layout never changes still
/// gets its covers seeded on the next cold start.
pub fn ensure_hub_entries(ctx: &Ctx) {
    if !eligible() || HUB_CHECKED.swap(true, Ordering::AcqRel) {
        return;
    }
    if read_manifest_from(&manifest_path()).hub_entries.is_empty() && !hub_targets(ctx).is_empty() {
        refresh_hub_entries(ctx);
    }
}

/// Rebuild the tile half of the manifest from the Hub's current game
/// shortcuts, leaving the resume half alone. Fired on a real layout
/// change, not on every cover fetch.
pub fn refresh_hub_entries(ctx: &Ctx) {
    if !eligible() {
        return;
    }
    HUB_CHECKED.store(true, Ordering::Release);
    let generation = HUB_GENERATION.fetch_add(1, Ordering::AcqRel) + 1;
    let targets = hub_targets(ctx);
    if targets.is_empty() {
        update_manifest(|manifest| {
            if HUB_GENERATION.load(Ordering::Acquire) != generation
                || manifest.hub_entries.is_empty()
            {
                return false;
            }
            manifest.hub_entries.clear();
            true
        });
        return;
    }
    let client = ctx.store.client();
    let preferred = lock(&ctx.shared).persist.settings.media_image_type.clone();
    ctx.handle.spawn(async move {
        wait_for_core(&client).await;
        let mut hub_entries = Vec::with_capacity(targets.len());
        for (system_id, path) in targets {
            if HUB_GENERATION.load(Ordering::Acquire) != generation {
                return;
            }
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
        update_manifest(|manifest| {
            if HUB_GENERATION.load(Ordering::Acquire) != generation {
                return false;
            }
            manifest.hub_entries = hub_entries;
            true
        });
    });
}

/// Update or clear the resume half. `None` means there is nothing to
/// resume.
pub fn refresh_resume_entry(ctx: &Ctx, target: Option<(String, String)>) {
    if !eligible() {
        return;
    }
    let generation = RESUME_GENERATION.fetch_add(1, Ordering::AcqRel) + 1;
    let Some((system_id, path)) = target else {
        update_manifest(|manifest| {
            RESUME_GENERATION.load(Ordering::Acquire) == generation
                && manifest.resume.take().is_some()
        });
        return;
    };
    let client = ctx.store.client();
    let preferred = lock(&ctx.shared).persist.settings.media_image_type.clone();
    ctx.handle.spawn(async move {
        wait_for_core(&client).await;
        let local_path = fetch_local_path(&client, &preferred, &system_id, &path).await;
        update_manifest(|manifest| {
            if RESUME_GENERATION.load(Ordering::Acquire) != generation {
                return false;
            }
            manifest.resume = local_path.map(|local_path| ManifestEntry {
                system_id,
                path,
                local_path,
                max_size: HUB_TILE_MAX_SIZE,
            });
            true
        });
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

    #[test]
    fn a_hosted_start_makes_the_manifest_eligible() {
        // What `run_application` configures before seeding on a hosted
        // build, with no Core transport yet.
        let (reads, local) = zaparoo_app::covers::startup_local_path(true, false, false);
        crate::media_cache::configure_local_path(reads, local);
        assert!(eligible());
        crate::media_cache::configure_local_path(false, false);
        assert!(!eligible());
    }

    #[test]
    fn concurrent_half_updates_keep_both_halves() {
        let dir = std::env::temp_dir().join(format!(
            "zaparoo-hub-covers-concurrent-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch dir");
        let path = dir.join(MANIFEST_FILE_NAME);
        let entry = |name: &str| ManifestEntry {
            system_id: "SNES".into(),
            path: format!("/games/{name}.sfc"),
            local_path: format!("/thumbs/{name}.png"),
            max_size: HUB_TILE_MAX_SIZE,
        };
        std::thread::scope(|scope| {
            for round in 0..20 {
                let path = &path;
                let hub = entry(&format!("hub{round}"));
                let resume = entry(&format!("resume{round}"));
                scope.spawn(move || {
                    update_manifest_at(path, |m| {
                        m.hub_entries = vec![hub];
                        true
                    });
                });
                scope.spawn(move || {
                    update_manifest_at(path, |m| {
                        m.resume = Some(resume);
                        true
                    });
                });
            }
        });
        let read = read_manifest_from(&path);
        assert_eq!(read.hub_entries.len(), 1);
        assert!(read.resume.is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
