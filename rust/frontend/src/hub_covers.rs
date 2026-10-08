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
// A third section does the same for the browse page a game was launched
// from, so coming back from that game shows its covers without a round
// trip each. It is written when a launch starts, not while browsing.
//
// The project's "do not persist Core metadata to disk" rule carries an
// explicit, scoped exception for exactly this file.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, PoisonError};

use serde::{Deserialize, Serialize};
use slint::ComponentHandle;
use zaparoo_app::covers::{
    Fit, HUB_TILE_MAX_SIZE, MAX_BROWSE_COVER_SIZE, MAX_BROWSE_ENTRIES, MAX_HUB_ENTRIES,
    MAX_LOCAL_IMAGE_BYTES,
};
use zaparoo_core::hub_layout::HubItemKind;
use zaparoo_core::media_types::MediaImageParams;

use crate::media_cache::{MediaCache, MediaKey};
use crate::router::{lock, Ctx};

const MANIFEST_FILE_NAME: &str = "hub_covers.toml";
const MAX_MANIFEST_BYTES: u64 = 64 * 1024;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
struct Manifest {
    hub_entries: Vec<ManifestEntry>,
    resume: Option<ManifestEntry>,
    browse: Vec<BrowseEntry>,
}

/// One browse cover, with every field of the key it is cached under: a
/// restore at another scene size computes other keys and just misses.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct BrowseEntry {
    media_id: Option<i64>,
    system_id: String,
    path: String,
    local_path: String,
    max_size: u32,
    fit_width: u32,
    fit_height: u32,
    #[serde(default)]
    fit_crisp: bool,
}

impl BrowseEntry {
    fn key(&self) -> MediaKey {
        MediaKey {
            media_id: self.media_id,
            system: self.system_id.clone(),
            path: self.path.clone(),
            max_size: self.max_size,
            fit: Fit {
                width: self.fit_width,
                height: self.fit_height,
                crisp: self.fit_crisp,
            },
            image_type: None,
        }
    }

    fn of(key: MediaKey, local_path: String) -> Self {
        Self {
            media_id: key.media_id,
            system_id: key.system,
            path: key.path,
            local_path,
            max_size: key.max_size,
            fit_width: key.fit.width,
            fit_height: key.fit.height,
            fit_crisp: key.fit.crisp,
        }
    }
}

/// The screens whose covers the browse section holds.
fn is_browse_screen(token: &str) -> bool {
    matches!(token, "games" | "favorites" | "recents" | "search-results")
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
    use std::io::Read as _;
    let read = || -> Option<Manifest> {
        let file = std::fs::File::open(path).ok()?;
        let mut contents = String::new();
        file.take(MAX_MANIFEST_BYTES + 1)
            .read_to_string(&mut contents)
            .ok()?;
        if contents.len() as u64 > MAX_MANIFEST_BYTES {
            return None;
        }
        let mut manifest: Manifest = toml::from_str(&contents).ok()?;
        manifest.hub_entries.truncate(MAX_HUB_ENTRIES);
        manifest.browse.truncate(MAX_BROWSE_ENTRIES);
        Some(manifest)
    };
    read().unwrap_or_default()
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

fn entry_key(cache: &MediaCache, entry: &ManifestEntry) -> MediaKey {
    MediaKey {
        media_id: None,
        system: entry.system_id.clone(),
        path: entry.path.clone(),
        max_size: entry.max_size,
        fit: cache.hub_fit(),
        image_type: None,
    }
}

fn split_visible(
    manifest: Manifest,
    visible: &[(String, String)],
    resume_visible: bool,
) -> (Vec<ManifestEntry>, Vec<ManifestEntry>) {
    let (mut now, mut later): (Vec<_>, Vec<_>) =
        manifest.hub_entries.into_iter().partition(|entry| {
            visible
                .iter()
                .any(|(system, path)| *system == entry.system_id && *path == entry.path)
        });
    if let Some(resume) = manifest.resume {
        if resume_visible {
            now.push(resume);
        } else {
            later.push(resume);
        }
    }
    (now, later)
}

fn seed_entry(cache: &MediaCache, entry: &ManifestEntry, epoch: u64) -> bool {
    if entry.max_size > HUB_TILE_MAX_SIZE {
        return false;
    }
    seed_key(
        cache,
        entry_key(cache, entry),
        &entry.local_path,
        entry.max_size,
        epoch,
    )
}

fn seed_browse_entry(cache: &MediaCache, entry: &BrowseEntry, epoch: u64) -> bool {
    if entry.max_size > MAX_BROWSE_COVER_SIZE {
        return false;
    }
    seed_key(cache, entry.key(), &entry.local_path, entry.max_size, epoch)
}

fn seed_key(
    cache: &MediaCache,
    key: MediaKey,
    local_path: &str,
    max_size: u32,
    epoch: u64,
) -> bool {
    if max_size == 0 || cache.seed_epoch() != epoch || cache.is_cached(&key) {
        return false;
    }
    let Some(image) = load_entry(local_path, max_size, key.fit) else {
        return false;
    };
    cache.seed_current(key, image, local_path, epoch)
}

fn load_entry(
    local_path: &str,
    max_size: u32,
    fit: Fit,
) -> Option<crate::media_cache::DecodedImage> {
    let bytes =
        crate::media_cache::read_local_image_file(local_path, MAX_LOCAL_IMAGE_BYTES).ok()?;
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .ok()?;
    let mut limits = image::Limits::default();
    limits.max_alloc = Some(zaparoo_app::customization::ART_DECODE_BYTES);
    limits.max_image_width = Some(zaparoo_app::customization::ART_SOURCE_EDGE);
    limits.max_image_height = Some(zaparoo_app::customization::ART_SOURCE_EDGE);
    reader.limits(limits);
    let decoded = reader.decode().ok()?;
    // Whether an image this small is enlarged depends on its type, which
    // only Core's answer carries, so it is left to the ordinary request.
    if fit.enlarged(decoded.width(), decoded.height()).is_some() {
        return None;
    }
    // A box caps the image on its own; without one the tier does.
    let decoded =
        if fit == Fit::SOURCE && (decoded.width() > max_size || decoded.height() > max_size) {
            decoded.thumbnail(max_size, max_size)
        } else {
            decoded
        };
    Some(crate::media_cache::fitted(decoded, fit, false))
}

/// Called after Hub geometry and persisted focus are seated, before app.run.
/// Only a Hub restore reads its visible page synchronously. A non-Hub restore
/// does not even open the manifest on the UI thread; a restore into a browse
/// screen seeds that screen's covers first, while its rows are still loading.
pub fn seed_startup(ctx: &std::sync::Arc<Ctx>, app: &crate::App) -> crate::scoped_task::ScopedTask {
    if !eligible() {
        return crate::scoped_task::ScopedTask::default();
    }
    let epoch = ctx.media.seed_epoch();
    let restores_browse = is_browse_screen(&lock(&ctx.shared).persist.active_screen);
    let foreground = {
        let shared = lock(&ctx.shared);
        !matches!(
            shared.persist.active_screen.as_str(),
            "systems"
                | "favorite-systems"
                | "games"
                | "favorites"
                | "recents"
                | "search"
                | "search-results"
                | "settings"
                | "about"
        )
    };
    let deferred = if foreground {
        let (visible, resume_visible) = crate::hub::visible_cover_targets(ctx);
        let (now, later) = split_visible(
            read_manifest_from(&manifest_path()),
            &visible,
            resume_visible,
        );
        for entry in now {
            seed_entry(&ctx.media, &entry, epoch);
        }
        crate::hub::rebuild(ctx, app);
        Some(later)
    } else {
        None
    };
    let ctx = ctx.clone();
    let weak = app.as_weak();
    let task = ctx.handle.clone().spawn(async move {
        let (browse, entries) = match deferred {
            Some(entries) => (Vec::new(), entries),
            None => tokio::task::spawn_blocking(move || {
                let manifest = read_manifest_from(&manifest_path());
                let browse = if restores_browse {
                    manifest.browse
                } else {
                    Vec::new()
                };
                let entries = manifest
                    .hub_entries
                    .into_iter()
                    .chain(manifest.resume)
                    .collect();
                (browse, entries)
            })
            .await
            .unwrap_or_default(),
        };
        let mut landed = Vec::new();
        for entry in browse {
            if ctx.media.seed_epoch() != epoch || *ctx.dormant.borrow() {
                return;
            }
            let key = entry.key();
            let cache = ctx.media.clone();
            let seeded =
                tokio::task::spawn_blocking(move || seed_browse_entry(&cache, &entry, epoch))
                    .await
                    .unwrap_or(false);
            if seeded {
                landed.push(key);
            }
        }
        for entry in entries {
            if ctx.media.seed_epoch() != epoch || *ctx.dormant.borrow() {
                return;
            }
            let key = entry_key(&ctx.media, &entry);
            let cache = ctx.media.clone();
            let seeded = tokio::task::spawn_blocking(move || seed_entry(&cache, &entry, epoch))
                .await
                .unwrap_or(false);
            if seeded {
                landed.push(key);
            }
        }
        if !landed.is_empty() && ctx.media.seed_epoch() == epoch {
            let _ = weak.upgrade_in_event_loop(move |app| {
                if ctx.media.seed_epoch() == epoch {
                    crate::deliver_covers(&ctx, &app, &landed);
                }
            });
        }
    });
    let mut owned = crate::scoped_task::ScopedTask::default();
    owned.replace(task);
    owned
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
    if hub_targets(ctx).is_empty() {
        return;
    }
    let ctx = ctx.clone();
    ctx.handle.clone().spawn(async move {
        let missing = tokio::task::spawn_blocking(|| {
            read_manifest_from(&manifest_path()).hub_entries.is_empty()
        })
        .await
        .unwrap_or(false);
        if missing {
            refresh_hub_entries(&ctx);
        }
    });
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

/// Record the covers of the browse page on screen, for the cold start
/// that follows a launched game. Called as a launch starts and as the
/// frontend goes dormant, never while browsing: the manifest is on the
/// SD card. Nothing to record leaves the previous page in place; its
/// keys just miss if the restore lands somewhere else.
pub fn record_browse_page(ctx: &Ctx) {
    if !eligible() || !is_browse_screen(&lock(&ctx.shared).persist.active_screen) {
        return;
    }
    let browse: Vec<BrowseEntry> = ctx
        .media
        .wanted_local_paths(MAX_BROWSE_ENTRIES)
        .into_iter()
        .map(|(key, local_path)| BrowseEntry::of(key, local_path))
        .collect();
    if browse.is_empty() {
        return;
    }
    ctx.handle.spawn_blocking(move || {
        update_manifest(|manifest| {
            if manifest.browse == browse {
                return false;
            }
            manifest.browse = browse;
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

/// The preferred artwork type changed: every recorded path names a file
/// of the old type, so both halves are resolved again. A Resume tile
/// that has not loaded yet is left to its own load.
pub fn refresh_for_image_type(ctx: &Ctx) {
    refresh_hub_entries(ctx);
    let resume = {
        let shared = lock(&ctx.shared);
        let resume = &shared.hub.resume;
        (resume.requested && !resume.loading).then(|| {
            resume
                .entry
                .as_ref()
                .filter(|e| !e.system_id.is_empty() && !e.media_path.is_empty())
                .map(|e| (e.system_id.clone(), e.media_path.clone()))
        })
    };
    if let Some(target) = resume {
        refresh_resume_entry(ctx, target);
    }
}

#[cfg(test)]
#[allow(
    clippy::expect_used,
    reason = "tests should fail fast on a filesystem they just set up"
)]
mod tests {
    use super::*;

    fn entry(path: &str) -> ManifestEntry {
        ManifestEntry {
            system_id: "NES".into(),
            path: path.into(),
            local_path: String::new(),
            max_size: HUB_TILE_MAX_SIZE,
        }
    }

    #[test]
    fn seed_partition_uses_restored_visible_identities_not_manifest_order() {
        let manifest = Manifest {
            hub_entries: vec![entry("offscreen"), entry("visible"), entry("deleted")],
            resume: Some(entry("resume")),
            browse: Vec::new(),
        };
        let (now, later) =
            split_visible(manifest.clone(), &[("NES".into(), "visible".into())], false);
        assert_eq!(
            now.iter().map(|e| e.path.as_str()).collect::<Vec<_>>(),
            ["visible"]
        );
        assert_eq!(
            later.iter().map(|e| e.path.as_str()).collect::<Vec<_>>(),
            ["offscreen", "deleted", "resume"]
        );
        let (now, later) = split_visible(manifest, &[], true);
        assert_eq!(now[0].path, "resume");
        assert_eq!(later.len(), 3);
    }

    #[test]
    fn disk_seed_never_overwrites_live_results_or_survives_a_trim() {
        let dir = std::env::temp_dir().join(format!("zaparoo-hub-seed-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("scratch dir");
        let path = dir.join("cover.png");
        image::RgbaImage::new(512, 256).save(&path).expect("PNG");
        let mut entry = entry("game");
        entry.local_path = path.to_string_lossy().into_owned();
        let cache = MediaCache::new();
        let epoch = cache.seed_epoch();
        assert!(seed_entry(&cache, &entry, epoch));
        let image = cache.get(&entry_key(&cache, &entry)).expect("seeded");
        assert_eq!((image.buffer.width(), image.buffer.height()), (256, 128));
        assert!(!seed_entry(&cache, &entry, epoch), "newer cache value wins");
        cache.clear_decoded();
        assert!(
            !seed_entry(&cache, &entry, epoch),
            "trim retires pending disk work"
        );
        assert!(seed_entry(&cache, &entry, cache.seed_epoch()));
        let epoch = cache.seed_epoch();
        cache.clear();
        assert!(
            !seed_entry(&cache, &entry, epoch),
            "rescan retires old thumbnail work"
        );
        std::fs::remove_dir_all(dir).expect("cleanup");
    }

    #[test]
    fn a_browse_cover_seeds_under_its_own_key_and_keeps_its_file() {
        let dir = std::env::temp_dir().join(format!("zaparoo-browse-seed-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("scratch dir");
        let path = dir.join("cover.png");
        image::RgbaImage::new(512, 256).save(&path).expect("PNG");
        let local_path = path.to_string_lossy().into_owned();
        let entry = BrowseEntry {
            media_id: Some(7),
            system_id: "SNES".into(),
            path: "/games/SNES/Zelda.sfc".into(),
            local_path: local_path.clone(),
            max_size: 512,
            fit_width: 200,
            fit_height: 200,
            fit_crisp: false,
        };
        let cache = MediaCache::new();
        let epoch = cache.seed_epoch();

        assert!(seed_browse_entry(&cache, &entry, epoch));

        let image = cache.get(&entry.key()).expect("seeded");
        assert_eq!((image.buffer.width(), image.buffer.height()), (200, 100));
        // The next launch records it again, from the same file.
        cache.request_wanted(vec![entry.key()]);
        let recorded: Vec<BrowseEntry> = cache
            .wanted_local_paths(MAX_BROWSE_ENTRIES)
            .into_iter()
            .map(|(key, path)| BrowseEntry::of(key, path))
            .collect();
        assert_eq!(recorded, vec![entry.clone()]);

        // A cover smaller than a box that enlarges captures is left to
        // the ordinary request: only Core's answer says what type it is.
        let small = BrowseEntry {
            media_id: Some(10),
            fit_width: 1024,
            fit_height: 1024,
            fit_crisp: true,
            ..entry.clone()
        };
        assert!(!seed_browse_entry(&cache, &small, epoch));
        assert!(!cache.is_cached(&small.key()));

        // A tier no browse cover uses, and a file that has gone, both
        // fall through to the ordinary request.
        let oversized = BrowseEntry {
            media_id: Some(8),
            max_size: MAX_BROWSE_COVER_SIZE + 1,
            ..entry.clone()
        };
        assert!(!seed_browse_entry(&cache, &oversized, epoch));
        std::fs::remove_dir_all(dir).expect("cleanup");
        let gone = BrowseEntry {
            media_id: Some(9),
            ..entry
        };
        assert!(!seed_browse_entry(&cache, &gone, epoch));
        assert!(!cache.is_cached(&gone.key()));
    }

    #[test]
    fn a_manifest_holds_no_more_than_its_caps() {
        let dir = std::env::temp_dir().join(format!("zaparoo-browse-cap-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch dir");
        let path = dir.join(MANIFEST_FILE_NAME);
        let browse = (0..MAX_BROWSE_ENTRIES as i64 + 5)
            .map(|id| BrowseEntry {
                media_id: Some(id),
                system_id: "SNES".into(),
                path: format!("/games/{id}.sfc"),
                local_path: format!("/thumbs/{id}.webp"),
                max_size: 256,
                fit_width: 100,
                fit_height: 100,
                fit_crisp: false,
            })
            .collect();
        write_manifest_to(
            &path,
            &Manifest {
                browse,
                ..Manifest::default()
            },
        );
        assert_eq!(read_manifest_from(&path).browse.len(), MAX_BROWSE_ENTRIES);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_manifest_round_trips_through_its_file() {
        let dir = std::env::temp_dir().join("zaparoo-hub-covers-test");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch dir");
        let path = dir.join(MANIFEST_FILE_NAME);
        let browse = BrowseEntry {
            media_id: Some(42),
            system_id: "SNES".into(),
            path: "/games/SNES/Metroid.sfc".into(),
            local_path: "/media/fat/zaparoo/cache/thumbs/v2/a/c.webp".into(),
            max_size: 512,
            fit_width: 227,
            fit_height: 208,
            fit_crisp: false,
        };
        let manifest = Manifest {
            hub_entries: vec![ManifestEntry {
                system_id: "SNES".into(),
                path: "/games/SNES/Zelda.sfc".into(),
                local_path: "/media/fat/zaparoo/cache/thumbs/v2/a/b.png".into(),
                max_size: HUB_TILE_MAX_SIZE,
            }],
            resume: None,
            browse: vec![browse.clone()],
        };
        write_manifest_to(&path, &manifest);
        let read = read_manifest_from(&path);
        assert_eq!(read.browse, vec![browse]);
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
