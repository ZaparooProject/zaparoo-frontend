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

mod prepare;

use crate::router::Ctx;
use crate::{App, Screen, Shell};
use slint::ComponentHandle;
use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::sync::{OnceLock, RwLock};

use zaparoo_app::customization as rules;

/// `(namespace, key) -> file`, filled by the scan.
static ART: OnceLock<RwLock<HashMap<(String, String), PathBuf>>> = OnceLock::new();
static ROOT: OnceLock<RwLock<PathBuf>> = OnceLock::new();
static NAMES: OnceLock<RwLock<HashMap<String, String>>> = OnceLock::new();
static GENERATION: AtomicU64 = AtomicU64::new(0);

fn art() -> &'static RwLock<HashMap<(String, String), PathBuf>> {
    ART.get_or_init(|| RwLock::new(HashMap::new()))
}

/// Register the root and the name table. Cheap: no filesystem access.
pub fn configure<S: std::hash::BuildHasher>(root: PathBuf, names: HashMap<String, String, S>) {
    stop();
    *ROOT
        .get_or_init(|| RwLock::new(PathBuf::new()))
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = root;
    *NAMES
        .get_or_init(|| RwLock::new(HashMap::new()))
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = rules::normalize_system_names(names);
    art()
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clear();
}

/// A system's user display name, or `None` to fall back to the
/// localized and Core names.
pub fn system_name(system_id: &str) -> Option<String> {
    let names = NAMES.get()?.read().ok()?;
    rules::system_name(&names, system_id).map(str::to_string)
}

/// Walk both namespaces and record what is there. Blocking; the caller
/// runs it off the event loop.
pub fn scan() -> usize {
    let generation = GENERATION.load(Ordering::Acquire);
    let Some(root) = ROOT
        .get()
        .and_then(|root| root.read().ok().map(|root| root.clone()))
    else {
        return 0;
    };
    let mut found = HashMap::new();
    for namespace in rules::NAMESPACES {
        found.extend(scan_namespace(&root, namespace));
    }
    let count = found.len();
    match art().write() {
        Ok(mut map) => {
            if GENERATION.load(Ordering::Acquire) != generation {
                return 0;
            }
            *map = found;
        }
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
    let mut bytes = 0;
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
        let key = (namespace.to_string(), rules::match_key(stem));
        bytes += key.0.capacity() + key.1.capacity() + path.as_os_str().len() + 256;
        if bytes > rules::ART_INDEX_BYTES / rules::NAMESPACES.len()
            || map.len() >= rules::ART_INDEX_ENTRIES / rules::NAMESPACES.len()
        {
            tracing::warn!(
                namespace,
                "override index budget reached; remaining files ignored"
            );
            break;
        }
        map.insert(key, path);
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

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct Key {
    namespace: &'static str,
    id: String,
    size: u32,
}

struct Cached {
    image: Option<slint::Image>,
    bytes: usize,
    used: u64,
}

#[derive(Default)]
struct Cache {
    entries: HashMap<Key, Cached>,
    bytes: usize,
    clock: u64,
}

enum Lookup {
    Missing,
    Rejected,
    Ready(slint::Image),
}

impl Cache {
    fn get(&mut self, key: &Key) -> Lookup {
        self.clock += 1;
        let Some(hit) = self.entries.get_mut(key) else {
            return Lookup::Missing;
        };
        hit.used = self.clock;
        hit.image.clone().map_or(Lookup::Rejected, Lookup::Ready)
    }

    fn insert(&mut self, key: Key, image: Option<slint::Image>, limit: usize) {
        let bytes = image.as_ref().map_or(0, |image| {
            let size = image.size();
            size.width as usize * size.height as usize * 4
        }) + key.id.capacity()
            + 256;
        if let Some(old) = self.entries.remove(&key) {
            self.bytes -= old.bytes;
        }
        while self.bytes.saturating_add(bytes) > limit {
            let oldest = self
                .entries
                .iter()
                .min_by_key(|(_, e)| e.used)
                .map(|(k, _)| k.clone());
            let Some(oldest) = oldest else { return };
            if let Some(old) = self.entries.remove(&oldest) {
                self.bytes -= old.bytes;
            }
        }
        self.clock += 1;
        self.bytes += bytes;
        self.entries.insert(
            key,
            Cached {
                image,
                bytes,
                used: self.clock,
            },
        );
    }
}

#[derive(Clone)]
struct Job {
    key: Key,
    path: PathBuf,
}

#[derive(Default)]
struct Queue {
    pending: VecDeque<Job>,
    running: Option<Key>,
}

impl Queue {
    fn push(&mut self, job: Job) {
        if self.running.as_ref() == Some(&job.key) {
            return;
        }
        // Repeated visible demand moves to the front; obsolete pages cannot
        // delay the current page behind an unbounded FIFO of SD reads.
        self.pending.retain(|queued| queued.key != job.key);
        self.pending.push_front(job);
        self.pending.truncate(rules::ART_PENDING);
    }

    fn take(&mut self) -> Option<Job> {
        let job = self.pending.pop_front()?;
        self.running = Some(job.key.clone());
        Some(job)
    }
}

struct Worker {
    queue: Mutex<Queue>,
    wake: tokio::sync::Notify,
}

thread_local! {
    static DECODED: RefCell<Cache> = RefCell::new(Cache::default());
    static WORKER: RefCell<Option<Arc<Worker>>> = const { RefCell::new(None) };
}

/// Start one worker per application lifetime. It waits for UI delivery before
/// decoding again, bounding both native decode scratch and queued pixel data.
pub fn start(ctx: &Arc<Ctx>, app: &App) -> crate::scoped_task::ScopedTask {
    stop();
    let generation = GENERATION.load(Ordering::Acquire);
    let worker = Arc::new(Worker {
        queue: Mutex::new(Queue::default()),
        wake: tokio::sync::Notify::new(),
    });
    WORKER.with(|slot| *slot.borrow_mut() = Some(worker.clone()));
    let weak = app.as_weak();
    let ctx = ctx.clone();
    let task = ctx.handle.clone().spawn(async move {
        loop {
            let notified = worker.wake.notified();
            let job = worker
                .queue
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .take();
            let Some(job) = job else {
                notified.await;
                continue;
            };
            let path = job.path;
            let size = job.key.size;
            let pixels = tokio::task::spawn_blocking(move || prepare::decode(&path, size))
                .await
                .ok()
                .flatten();
            let ctx = ctx.clone();
            let (done, delivered) = tokio::sync::oneshot::channel();
            if weak
                .upgrade_in_event_loop(move |app| {
                    if GENERATION.load(Ordering::Acquire) == generation {
                        let namespace = job.key.namespace;
                        DECODED.with(|cache| {
                            cache.borrow_mut().insert(
                                job.key,
                                pixels.map(prepare::Pixels::into_image),
                                rules::ART_CACHE_BYTES,
                            );
                        });
                        match (namespace, app.global::<Shell>().get_active_screen()) {
                            ("hub", Screen::Hub) => crate::hub::render(&ctx, &app),
                            ("systems", Screen::Systems | Screen::FavoriteSystems) => {
                                crate::systems::render(&ctx, &app);
                            }
                            _ => {}
                        }
                    }
                    let _ = done.send(());
                })
                .is_err()
            {
                return;
            }
            if delivered.await.is_err() || GENERATION.load(Ordering::Acquire) != generation {
                return;
            }
            worker
                .queue
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .running = None;
        }
    });
    let mut owned = crate::scoped_task::ScopedTask::default();
    owned.replace(task);
    owned
}

pub fn stop() {
    GENERATION.fetch_add(1, Ordering::AcqRel);
    WORKER.with(|slot| *slot.borrow_mut() = None);
    DECODED.with(|cache| *cache.borrow_mut() = Cache::default());
}

/// Lookups never read a file or decode. A miss schedules bounded preparation;
/// callers keep their built-in art until the prepared image lands.
fn image_for(namespace: &'static str, id: &str, size: u32) -> Option<slint::Image> {
    let key = Key {
        namespace,
        id: rules::match_key(id),
        size: size.clamp(1, rules::ART_OUTPUT_EDGE),
    };
    match DECODED.with(|cache| cache.borrow_mut().get(&key)) {
        Lookup::Ready(image) => return Some(image),
        Lookup::Rejected => return None,
        Lookup::Missing => {}
    }
    let path = override_path(namespace, id)?;
    WORKER.with(|slot| {
        if let Some(worker) = slot.borrow().as_ref() {
            worker
                .queue
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(Job { key, path });
            worker.wake.notify_one();
        }
    });
    None
}

pub fn system_image(system_id: &str, size: u32) -> Option<slint::Image> {
    image_for("systems", system_id, size)
}

pub fn hub_image(id: &str, size: u32) -> Option<slint::Image> {
    image_for("hub", id, size)
}

/// Discovery is separate from decoding so Hub resolution retains a fallback.
pub fn has_hub_override(id: &str) -> bool {
    override_path("hub", id).is_some()
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
    pub(super) struct Scratch(pub(super) PathBuf);

    impl Scratch {
        pub(super) fn new(name: &str) -> Self {
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

    fn key(id: &str, size: u32) -> Key {
        Key {
            namespace: "systems",
            id: id.into(),
            size,
        }
    }

    #[test]
    fn image_cache_is_byte_capped_lru_and_keys_include_display_size() {
        let mut cache = Cache::default();
        let image = || {
            Some(slint::Image::from_rgba8(slint::SharedPixelBuffer::new(
                4, 4,
            )))
        };
        cache.insert(key("a", 4), image(), 768);
        cache.insert(key("b", 4), image(), 768);
        assert!(matches!(cache.get(&key("a", 4)), Lookup::Ready(_)));
        cache.insert(key("c", 4), image(), 768);
        assert!(matches!(cache.get(&key("b", 4)), Lookup::Missing));
        assert!(matches!(cache.get(&key("a", 4)), Lookup::Ready(_)));
        assert!(matches!(cache.get(&key("a", 8)), Lookup::Missing));
        assert!(cache.bytes <= 768);
        cache.insert(key("broken", 4), None, 768);
        assert!(matches!(cache.get(&key("broken", 4)), Lookup::Rejected));
        assert!(cache.bytes <= 768);
    }

    #[test]
    fn pending_work_is_bounded_deduplicated_and_recent_first() {
        let mut queue = Queue::default();
        let job = |id: &str| Job {
            key: key(id, 128),
            path: PathBuf::from(id),
        };
        queue.push(job("active"));
        assert!(queue.take().is_some());
        queue.push(job("active"));
        assert!(
            queue.pending.is_empty(),
            "in-flight request is not duplicated"
        );
        for n in 0..100 {
            queue.push(job(&n.to_string()));
        }
        assert_eq!(queue.pending.len(), rules::ART_PENDING);
        queue.push(job("98"));
        assert_eq!(queue.pending.len(), rules::ART_PENDING);
        assert_eq!(queue.take().expect("next").key.id, "98");
    }

    #[test]
    fn cold_lookup_queues_work_without_reading_or_decoding() {
        stop();
        let worker = Arc::new(Worker {
            queue: Mutex::new(Queue::default()),
            wake: tokio::sync::Notify::new(),
        });
        WORKER.with(|slot| *slot.borrow_mut() = Some(worker.clone()));
        art().write().expect("art index").insert(
            ("systems".into(), "test".into()),
            PathBuf::from("/no-file-needed.png"),
        );
        assert!(system_image("TEST", 128).is_none());
        assert_eq!(worker.queue.lock().expect("queue").pending.len(), 1);
        assert!(DECODED.with(|cache| cache.borrow().entries.is_empty()));
        stop();
    }

    #[test]
    fn an_override_decodes_to_an_image() {
        let scratch = Scratch::new("decode");
        let root = &scratch.0;
        let png = root.join("art.png");
        let pixels = image::RgbaImage::from_pixel(4, 2, image::Rgba([9, 9, 9, 255]));
        pixels.save(&png).expect("encode");
        let decoded = prepare::decode(&png, 256).expect("decoded").into_image();
        assert_eq!(decoded.size().width, 4);
        assert_eq!(decoded.size().height, 2);

        let svg = root.join("art.svg");
        std::fs::write(
            &svg,
            br##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10"><rect width="10" height="10" fill="#fff"/></svg>"##,
        )
        .expect("write");
        let decoded = prepare::decode(&svg, 256).expect("rasterized").into_image();
        assert_eq!(decoded.size().width, decoded.size().height);

        // Anything that is not an image at all just has no override.
        let broken = root.join("art.webp");
        std::fs::write(&broken, b"not an image").expect("write");
        assert!(prepare::decode(&broken, 256).is_none());
    }
}
