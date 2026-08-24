// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
//
// Hub/Resume cold-boot cover manifest — a small path list (not image
// bytes) that lets the Hub's zapscript-linked game tiles and the Resume
// tile paint their real cover art on the very first frame after a
// `MiSTer` boot, instead of the bundled placeholder glyph until Core
// answers the first `media.image` round trip.
//
// On a colocated `MiSTer` the cover bytes are already on the SD card
// before the frontend even starts — they are Core's own thumbnail cache
// (`/media/fat/zaparoo/cache/thumbs/v2/...`), and the frontend already
// knows how to read them directly (`media_image_cache.rs`'s
// `should_request_local_path`/`read_local_image_file`, gated on
// `max_size > 0 && is_mister && core_is_local`). What is missing at boot
// is knowing *which* file to open without a Core round trip first — this
// module persists that mapping.
//
// Core's own API doc calls a `localPath` response "opaque, transient,
// and nonportable: read it immediately, never persist it" — this
// deliberately does so anyway (a decision made with, not around, Core's
// author). The design still accounts for the reason that warning exists:
// Core's `wipe()` renames its thumbnail-cache version dir aside after
// every scrape/reindex, and `reapStaleVersions` drops any non-current
// version dir on startup, so a persisted path routinely goes stale. A
// stale path here simply fails to open (`seed_from_manifest`) and falls
// straight through to the normal Core request the first time that tile's
// cover is actually asked for — staleness costs one extra round trip on
// that one tile, never a wrong image or a crash.
//
// CLAUDE.md's "Do not persist Core metadata... to disk" rule carries an
// explicit, scoped exception for this manifest — see that file and
// `media_image_cache.rs`'s "Memory only — never disk" module doc, which
// stays true of the image bytes themselves; only a path list is ever
// written here.
//
// Not on desktop or a remote Core: `localPath` delivery requires
// `is_mister && core_is_local`, so `seed_from_manifest`/`refresh_*` are
// all silent no-ops there — those runtimes keep the ordinary fetch path
// plus prefetch priority instead.
//
// Bounded by construction: at most `hubGridColumns × hubGridRows` (21)
// zapscript tiles, plus one Resume slot — see `MAX_HUB_ENTRIES` below.

use crate::media_image_cache::{
    global_media_image_cache, preferred_image_types, read_local_image_file,
    should_request_local_path, MediaImageCache, MediaKey, MAX_LOCAL_IMAGE_BYTES,
};
use crate::models::{global_store, with_hub_layout_read};
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};
use tracing::{debug, warn};
use zaparoo_core::hub_layout::HubItemKind;
use zaparoo_core::media_types::{MediaImageParams, MEDIA_IMAGE_DELIVERY_LOCAL_PATH};
use zaparoo_core::platform_paths::cache_dir;
use zaparoo_core::runtime;

const MANIFEST_FILE_NAME: &str = "hub_covers.toml";
/// Matches `Tile.qml`'s default `coverSourceSize` (256) — Hub tiles never
/// decode real art any larger than this, so there is nothing to gain
/// requesting a bigger thumbnail from Core for the manifest. Keep in
/// sync with `Tile.qml:coverSourceSize` if that default ever changes.
const HUB_TILE_MAX_SIZE: u32 = 256;
/// Hard cap matching the Hub's own maximum tile count — see
/// `Sizing.hubGridColumns`/`hubGridRows` in QML (7×3 at 540/720/1080).
/// Purely a sanity backstop; a real Hub layout never exceeds this.
const MAX_HUB_ENTRIES: usize = 21;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct ManifestEntry {
    system_id: String,
    path: String,
    local_path: String,
    max_size: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
struct Manifest {
    /// Hub `zapscript` tiles linked to a game. Replaced wholesale by
    /// `refresh_hub_entries` on every persisted hub-layout change.
    hub_entries: Vec<ManifestEntry>,
    /// The Resume tile's own entry, if any. Replaced by
    /// `refresh_resume_entry` whenever the resume-latest RPC result
    /// changes. `None` clears it (e.g. no resumable game).
    resume: Option<ManifestEntry>,
}

fn manifest_path() -> PathBuf {
    cache_dir().join(MANIFEST_FILE_NAME)
}

fn eligible() -> bool {
    runtime::current().is_mister() && crate::models::core_is_local()
}

/// Pure(ish) read against an explicit path — split out from
/// `manifest_path()`'s real-`cache_dir()` resolution so tests can drive
/// it against a tempdir, matching `zaparoo_core::persist`'s
/// `save_to`/`load_from` split.
fn read_manifest_from(path: &Path) -> Manifest {
    let Ok(contents) = std::fs::read_to_string(path) else {
        return Manifest::default();
    };
    toml::from_str(&contents).unwrap_or_default()
}

fn read_manifest() -> Manifest {
    read_manifest_from(&manifest_path())
}

/// Same shape as `zaparoo_core::persist::save_to` — write-temp-then-
/// rename, so a kill mid-write can never leave a half-written manifest
/// for `seed_from_manifest` to trip over on the next boot. Split from
/// `manifest_path()`'s resolution for the same testability reason as
/// `read_manifest_from`.
fn write_manifest_to(path: &Path, manifest: &Manifest) {
    let Ok(serialized) = toml::to_string_pretty(manifest) else {
        return;
    };
    let Some(parent) = path.parent() else { return };
    if std::fs::create_dir_all(parent).is_err() {
        return;
    }
    let tmp_path = parent.join(format!(".{MANIFEST_FILE_NAME}.tmp.{}", std::process::id()));
    let write_result = (|| -> std::io::Result<()> {
        let mut file = std::fs::File::create(&tmp_path)?;
        file.write_all(serialized.as_bytes())?;
        file.sync_all()?;
        std::fs::rename(&tmp_path, path)?;
        Ok(())
    })();
    if let Err(e) = write_result {
        warn!("hub_cover_manifest: could not write manifest: {e}");
        let _ = std::fs::remove_file(&tmp_path);
    }
}

fn write_manifest(manifest: &Manifest) {
    write_manifest_to(&manifest_path(), manifest);
}

/// Read the manifest and open each entry's `local_path` directly,
/// seeding the in-memory image cache so `MediaImageCache::is_cached` is
/// already true by the time the Hub asks for these covers — see this
/// module's own doc comment. Called once at startup, synchronously
/// (small, local, sequential reads; no network, no `spawn_blocking`
/// needed the way the async fetch path requires it). A missing or
/// unreadable manifest is a silent no-op: nothing to seed on a first-ever
/// boot, and a corrupt file just means every tile falls through to the
/// normal Core request exactly as if this feature did not exist.
pub fn seed_from_manifest() {
    if !eligible() {
        return;
    }
    let manifest = read_manifest();
    let cache = global_media_image_cache();
    let mut seeded = 0usize;
    for entry in manifest.hub_entries.into_iter().chain(manifest.resume) {
        // Exact same construction `resolve_media_cover_key`/
        // `resume_cover_key_for` use for their own lookups — `max_size`
        // is not part of the cache key (only the fetch-request
        // parameter), so it must not be included here either, or the
        // seeded entry silently never matches a real lookup.
        let key = MediaKey::new(entry.system_id, entry.path).with_current_cover_preference();
        if cache.is_cached(&key) {
            continue;
        }
        match read_local_image_file(&entry.local_path, MAX_LOCAL_IMAGE_BYTES) {
            Ok(bytes) => {
                cache.seed_local(key, bytes);
                seeded += 1;
            }
            Err(e) => debug!(
                local_path = %entry.local_path,
                "hub_cover_manifest: stale manifest entry, will fall through to Core: {e}"
            ),
        }
    }
    if seeded > 0 {
        debug!(seeded, "hub_cover_manifest: seeded covers from manifest");
    }
}

/// Ask Core directly for the on-disk thumbnail path for one `(system,
/// path)` pair, without touching the ordinary fetch queue at all — this
/// is a separate, occasional request issued only when refreshing the
/// manifest (a hub-layout change or a new resume entry), not a duplicate
/// of the request that already runs for an on-screen tile. Returns
/// `None` on any failure; the manifest entry is simply skipped or
/// cleared this round, and the next refresh (or a live cover fetch) can
/// still recover it — never a hard error.
async fn fetch_local_path(system_id: &str, path: &str) -> Option<String> {
    if !should_request_local_path(HUB_TILE_MAX_SIZE) {
        return None;
    }
    let mut params = MediaImageParams::for_media(system_id, path);
    if let Some(preference) = MediaImageCache::current_cover_preference_type() {
        params.image_types = preferred_image_types(&preference);
    }
    params.max_size = Some(HUB_TILE_MAX_SIZE);
    params.delivery = Some(MEDIA_IMAGE_DELIVERY_LOCAL_PATH.to_string());
    let result = global_store().client().media_image(params).await.ok()?;
    if result.delivery != MEDIA_IMAGE_DELIVERY_LOCAL_PATH {
        return None;
    }
    result.local_path.filter(|p| !p.is_empty())
}

/// Rebuild the `hub_entries` section of the manifest from the Hub's
/// current `zapscript` tiles, leaving `resume` untouched. Spawned
/// fire-and-forget from `with_hub_layout_mut` — the single chokepoint
/// every persisted hub-layout mutation (add/remove/move/reset) already
/// funnels through, so this fires on a real, infrequent layout change,
/// not on every cover fetch. No-op off a colocated `MiSTer`.
pub fn refresh_hub_entries() {
    if !eligible() {
        return;
    }
    let targets: Vec<(String, String)> = with_hub_layout_read(|layout| {
        layout
            .visible()
            .filter(|item| {
                item.kind() == HubItemKind::ZapScript
                    && !item.system.is_empty()
                    && !item.path.is_empty()
            })
            .take(MAX_HUB_ENTRIES)
            .map(|item| (item.system.clone(), item.path.clone()))
            .collect()
    });
    if targets.is_empty() {
        let mut manifest = read_manifest();
        if !manifest.hub_entries.is_empty() {
            manifest.hub_entries.clear();
            write_manifest(&manifest);
        }
        return;
    }
    crate::models::global_handle().spawn(async move {
        let mut hub_entries = Vec::with_capacity(targets.len());
        for (system_id, path) in targets {
            let Some(local_path) = fetch_local_path(&system_id, &path).await else {
                continue;
            };
            hub_entries.push(ManifestEntry {
                system_id,
                path,
                local_path,
                max_size: HUB_TILE_MAX_SIZE,
            });
        }
        let mut manifest = read_manifest();
        manifest.hub_entries = hub_entries;
        write_manifest(&manifest);
    });
}

/// Update (or clear) the `resume` section of the manifest, leaving
/// `hub_entries` untouched. Called from `sync_resume_state` whenever the
/// resume-latest RPC result changes. `None` clears the section (no
/// resumable game). No-op off a colocated `MiSTer`.
pub fn refresh_resume_entry(target: Option<(String, String)>) {
    if !eligible() {
        return;
    }
    let Some((system_id, path)) = target else {
        let mut manifest = read_manifest();
        if manifest.resume.is_some() {
            manifest.resume = None;
            write_manifest(&manifest);
        }
        return;
    };
    crate::models::global_handle().spawn(async move {
        let resume = fetch_local_path(&system_id, &path)
            .await
            .map(|local_path| ManifestEntry {
                system_id,
                path,
                local_path,
                max_size: HUB_TILE_MAX_SIZE,
            });
        let mut manifest = read_manifest();
        manifest.resume = resume;
        write_manifest(&manifest);
    });
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::unwrap_used,
        reason = "tests should fail-fast on unexpected errors"
    )]

    use super::{read_manifest_from, write_manifest_to, Manifest, ManifestEntry};

    fn entry(system_id: &str, path: &str, local_path: &str) -> ManifestEntry {
        ManifestEntry {
            system_id: system_id.to_string(),
            path: path.to_string(),
            local_path: local_path.to_string(),
            max_size: 256,
        }
    }

    #[test]
    fn read_manifest_defaults_when_file_is_missing() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("does-not-exist.toml");
        assert_eq!(read_manifest_from(&path), Manifest::default());
    }

    #[test]
    fn read_manifest_defaults_on_malformed_toml() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("hub_covers.toml");
        std::fs::write(&path, "this is not [ valid toml").expect("write");
        assert_eq!(read_manifest_from(&path), Manifest::default());
    }

    #[test]
    fn write_then_read_round_trips_both_sections() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("hub_covers.toml");
        let manifest = Manifest {
            hub_entries: vec![
                entry("SNES", "/roms/snes/mario.sfc", "/thumbs/a.webp"),
                entry("NES", "/roms/nes/metroid.nes", "/thumbs/b.webp"),
            ],
            resume: Some(entry("NES", "/roms/nes/zelda.nes", "/thumbs/c.webp")),
        };
        write_manifest_to(&path, &manifest);
        assert_eq!(read_manifest_from(&path), manifest);
    }

    #[test]
    fn write_manifest_is_atomic_and_leaves_no_tmp_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("hub_covers.toml");
        let manifest = Manifest {
            hub_entries: vec![entry("SNES", "/roms/snes/mario.sfc", "/thumbs/a.webp")],
            resume: None,
        };
        write_manifest_to(&path, &manifest);
        assert!(path.exists());
        let leftovers: Vec<_> = std::fs::read_dir(dir.path())
            .expect("read_dir")
            .filter_map(Result::ok)
            .filter(|e| e.file_name().to_string_lossy().contains(".tmp."))
            .collect();
        assert!(
            leftovers.is_empty(),
            "a successful write must not leave a tmp file behind: {leftovers:?}"
        );
    }

    #[test]
    fn write_manifest_creates_parent_directories() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir
            .path()
            .join("nested")
            .join("sub")
            .join("hub_covers.toml");
        write_manifest_to(&path, &Manifest::default());
        assert!(path.exists(), "manifest was not created at {path:?}");
    }

    #[test]
    fn write_then_write_again_overwrites_cleanly() {
        // A second refresh (e.g. the hub layout changed again) must fully
        // replace the previous hub_entries, not merge with them.
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("hub_covers.toml");
        write_manifest_to(
            &path,
            &Manifest {
                hub_entries: vec![entry("SNES", "/a", "/thumbs/a.webp")],
                resume: None,
            },
        );
        let second = Manifest {
            hub_entries: vec![entry("NES", "/b", "/thumbs/b.webp")],
            resume: None,
        };
        write_manifest_to(&path, &second);
        assert_eq!(read_manifest_from(&path), second);
    }
}
