// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

use crate::media_image_cache::{
    global_media_image_cache, MediaImageCache, MediaImageUpdate, MediaKey,
};
use crate::models::tag_utils::tag_display_value;
use crate::models::{global_handle, global_store};
use cxx_qt::{CxxQtType, Threading};
use cxx_qt_lib::QString;
use std::collections::BTreeSet;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::broadcast::error::RecvError;
use tokio::task::JoinHandle;
use tracing::warn;
use zaparoo_core::media_types::{MediaMeta, MediaMetaParams, CORE_SERVEABLE_IMAGE_TYPES};

#[derive(Default)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "the bools are independent qproperties surfaced to QML; collapsing them \
              into an enum would force the QML side to read a single state property \
              and re-derive each flag locally, which is exactly the work the bridge \
              avoids"
)]
pub struct GameInfoRust {
    loading: bool,
    error_message: QString,
    title: QString,
    description: QString,
    detail_tags: QString,
    /// Core has the row indexed but the file behind it is gone. Surfaced so
    /// the modal can say so; a missing game otherwise looks completely
    /// normal right up until the launch fails.
    media_missing: bool,
    image_key: QString,
    image_index: i32,
    image_count: i32,
    image_can_prev: bool,
    image_can_next: bool,
    detail_image_keys: Vec<MediaKey>,
    seq: Arc<AtomicU64>,
    cover_subscription: Option<JoinHandle<()>>,
}

#[cxx_qt::bridge]
pub mod ffi {
    unsafe extern "C++" {
        include!("model_includes.h");

        type QString = cxx_qt_lib::QString;
    }

    unsafe extern "RustQt" {
        #[qobject]
        #[qml_element]
        #[qml_singleton]
        #[qproperty(bool, loading)]
        #[qproperty(QString, error_message)]
        #[qproperty(QString, title)]
        #[qproperty(QString, description)]
        #[qproperty(QString, detail_tags)]
        #[qproperty(bool, media_missing)]
        #[qproperty(QString, image_key)]
        #[qproperty(i32, image_index)]
        #[qproperty(i32, image_count)]
        #[qproperty(bool, image_can_prev)]
        #[qproperty(bool, image_can_next)]
        type GameInfo = super::GameInfoRust;

        #[qinvokable]
        fn load(
            self: Pin<&mut GameInfo>,
            system_id: &QString,
            path: &QString,
            fallback_title: &QString,
        );

        #[qinvokable]
        fn clear(self: Pin<&mut GameInfo>);

        #[qinvokable]
        fn cycle_image(self: Pin<&mut GameInfo>, delta: i32);
    }

    impl cxx_qt::Threading for GameInfo {}
}

impl ffi::GameInfo {
    fn load(
        mut self: Pin<&mut Self>,
        system_id: &QString,
        path: &QString,
        fallback_title: &QString,
    ) {
        self.as_mut().ensure_cover_subscription();
        let system = system_id.to_string();
        let path = path.to_string();
        let fallback_title = fallback_title.to_string();
        self.as_mut().rust().seq.fetch_add(1, Ordering::SeqCst);
        clear_detail_images(self.as_mut());
        self.as_mut().set_error_message(QString::default());
        self.as_mut()
            .set_title(QString::from(fallback_title.trim()));
        self.as_mut().set_description(QString::default());
        self.as_mut().set_detail_tags(QString::default());
        self.as_mut().set_media_missing(false);
        if system.trim().is_empty() || path.trim().is_empty() {
            self.as_mut().set_loading(false);
            return;
        }
        self.as_mut().set_loading(true);
        let seq = self.rust().seq.clone();
        let ticket = seq.load(Ordering::SeqCst);
        let qt_thread = self.qt_thread();
        let store = global_store();
        global_handle().spawn(async move {
            let result = store
                .client()
                .media_meta(MediaMetaParams::for_media(system.clone(), path.clone()))
                .await;
            let _ = qt_thread.queue(move |mut model| {
                if seq.load(Ordering::SeqCst) != ticket {
                    return;
                }
                model.as_mut().set_loading(false);
                match result {
                    Ok(result) => {
                        let meta_title = result.media.title.name.trim();
                        if !meta_title.is_empty() {
                            model.as_mut().set_title(QString::from(meta_title));
                        }
                        model.as_mut().set_description(QString::from(
                            description_from_meta(&result.media).as_str(),
                        ));
                        model.as_mut().set_detail_tags(QString::from(
                            detail_tags_from_meta(&result.media, &path).as_str(),
                        ));
                        model.as_mut().set_media_missing(result.media.is_missing);
                        install_detail_images(
                            model.as_mut(),
                            detail_image_keys_from_meta(&result.media, &system, &path),
                        );
                    }
                    Err(e) => {
                        warn!("game info fetch failed for {path}: {}", e.message);
                        model
                            .as_mut()
                            .set_error_message(QString::from(e.message.as_str()));
                    }
                }
            });
        });
    }

    fn clear(mut self: Pin<&mut Self>) {
        self.as_mut().rust().seq.fetch_add(1, Ordering::SeqCst);
        self.as_mut().set_loading(false);
        self.as_mut().set_error_message(QString::default());
        self.as_mut().set_title(QString::default());
        self.as_mut().set_description(QString::default());
        self.as_mut().set_detail_tags(QString::default());
        self.as_mut().set_media_missing(false);
        clear_detail_images(self);
    }

    fn cycle_image(self: Pin<&mut Self>, delta: i32) {
        if delta == 0 || self.image_count <= 1 {
            return;
        }
        let current = self.image_index;
        let next = (current + delta).clamp(0, self.image_count - 1);
        if next == current {
            return;
        }
        set_detail_image_index(self, next);
    }

    fn ensure_cover_subscription(mut self: Pin<&mut Self>) {
        if self.cover_subscription.is_some() {
            return;
        }
        let cache = global_media_image_cache();
        let mut rx = cache.subscribe();
        let qt_thread = self.qt_thread();
        let handle = global_handle().spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(update) => {
                        let _ = qt_thread.queue(move |model| {
                            notify_cover_update(model, &update);
                        });
                    }
                    Err(RecvError::Lagged(_)) => {}
                    Err(RecvError::Closed) => break,
                }
            }
        });
        self.as_mut().rust_mut().cover_subscription = Some(handle);
    }
}

/// Canonical tag types rendered first, in this order, when the row has
/// them. Everything else Core sent follows in its own order.
///
/// These are *types*, not labels: the label is chosen in QML
/// (`Format.metadataLabel`) so it can go through `qsTr()`. Emitting
/// title-cased English from here is what left the whole details table
/// untranslated regardless of the UI language.
const ORDERED_TAG_TYPES: &[&str] = &[
    "system",
    "platform",
    "year",
    "release_date",
    "genre",
    "players",
    "play_mode",
    "cooperative",
    "developer",
    "publisher",
    "rating",
];

/// Property keys already rendered somewhere else in the modal, so folding
/// them into the tag table would duplicate them.
const PROPERTY_KEYS_RENDERED_ELSEWHERE: &[&str] = &["description"];

fn description_from_meta(meta: &MediaMeta) -> String {
    meta.title
        .properties
        .get("property:description")
        .or_else(|| meta.properties.get("property:description"))
        .map(|property| property.text.trim().to_string())
        .filter(|text| !text.is_empty())
        .unwrap_or_default()
}

fn detail_tags_from_meta(meta: &MediaMeta, path: &str) -> String {
    let source = if meta.title.tags.is_empty() {
        meta.tags.as_slice()
    } else {
        meta.title.tags.as_slice()
    };
    let mut rows: Vec<(String, String)> = Vec::new();
    let mut seen_types = BTreeSet::<String>::new();

    for tag_type in ORDERED_TAG_TYPES {
        for tag in source.iter().filter(|tag| {
            normalize_tag_type(&tag.tag_type) == *tag_type && !tag_display_value(tag).is_empty()
        }) {
            seen_types.insert(normalize_tag_type(&tag.tag_type));
            rows.push((normalize_tag_type(&tag.tag_type), tag_display_value(tag)));
        }
    }
    for tag in source.iter().filter(|tag| {
        !is_ordered_tag(&tag.tag_type)
            && !tag.tag_type.trim().is_empty()
            && !tag_display_value(tag).is_empty()
    }) {
        seen_types.insert(normalize_tag_type(&tag.tag_type));
        rows.push((normalize_tag_type(&tag.tag_type), tag_display_value(tag)));
    }

    // Core knows the system even when it emitted no `system` tag for the
    // row -- `title.system.name` is always populated. Without this the
    // details modal could show a table with no idea what the game runs on,
    // which is the one field the user is most likely to already want.
    if !seen_types.contains("system") {
        let system_name = meta.title.system.name.trim();
        if !system_name.is_empty() {
            rows.insert(0, ("system".to_string(), system_name.to_string()));
            seen_types.insert("system".to_string());
        }
    }

    // Everything else the scraper stored. Core splits metadata between tags
    // (curated types) and properties (free-form key/value), and which side a
    // given field lands on is the scraper's choice, not the frontend's --
    // a gamelist.xml import can put the rating in either. Reading only
    // `property:description` meant every other scraped property was dropped
    // on the floor and the modal looked like Core had nothing, when it
    // often had plenty. Title-level wins over ROM-level, same precedence
    // `description_from_meta` uses.
    rows.extend(extra_property_rows(meta, &seen_types));

    let filename = file_stem_or_name(path);
    if !filename.is_empty() {
        rows.push(("filename".to_string(), filename));
    }
    rows.into_iter()
        .map(|(tag_type, value)| {
            let value = value.replace(['\t', '\r', '\n'], " ");
            format!("{tag_type}\t{value}")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Text properties worth showing as table rows, in a stable order.
///
/// Skips image payload keys (the carousel owns those), anything already
/// rendered elsewhere, and any key whose type duplicates a tag the row
/// already carries -- a scraper that wrote both is describing one field.
fn extra_property_rows(meta: &MediaMeta, seen_types: &BTreeSet<String>) -> Vec<(String, String)> {
    let mut rows: Vec<(String, String)> = Vec::new();
    let mut emitted = BTreeSet::<String>::new();
    for properties in [&meta.title.properties, &meta.properties] {
        let mut keys: Vec<&String> = properties.keys().collect();
        keys.sort();
        for key in keys {
            let Some(tag_type) = property_row_type(key) else {
                continue;
            };
            if seen_types.contains(&tag_type) || !emitted.insert(tag_type.clone()) {
                continue;
            }
            let Some(property) = properties.get(key) else {
                continue;
            };
            let text = property.text.trim();
            if text.is_empty() {
                continue;
            }
            rows.push((tag_type, text.to_string()));
        }
    }
    rows
}

/// `property:rating` -> `rating`. `None` for image payloads and for
/// anything already rendered elsewhere in the modal.
fn property_row_type(key: &str) -> Option<String> {
    let name = key.strip_prefix("property:")?.trim();
    if name.is_empty() || name.starts_with("image") {
        return None;
    }
    let normalized = normalize_tag_type(name);
    if PROPERTY_KEYS_RENDERED_ELSEWHERE.contains(&normalized.as_str()) {
        return None;
    }
    Some(normalized)
}

fn is_ordered_tag(tag_type: &str) -> bool {
    ORDERED_TAG_TYPES.contains(&normalize_tag_type(tag_type).as_str())
}

/// Canonical tag-type key: trimmed, lowercase, spaces and dashes folded to
/// underscores, so `Release Date`, `release-date` and `release_date` all
/// resolve to the one key `Format.metadataLabel` looks up.
fn normalize_tag_type(tag_type: &str) -> String {
    tag_type
        .trim()
        .to_ascii_lowercase()
        .replace([' ', '-'], "_")
}

fn image_type_from_property_key(key: &str) -> Option<String> {
    let suffix = key.strip_prefix("property:image")?;
    if suffix.is_empty() {
        return Some("image".to_string());
    }
    Some(suffix.trim_start_matches('-').to_string()).filter(|image_type| !image_type.is_empty())
}

fn detail_image_keys_from_meta(meta: &MediaMeta, system: &str, path: &str) -> Vec<MediaKey> {
    let mut seen = BTreeSet::<String>::new();
    let mut ordered = Vec::<String>::new();
    for image_type in meta
        .available_image_types
        .iter()
        .chain(meta.title.available_image_types.iter())
    {
        if !image_type.trim().is_empty() && seen.insert(image_type.clone()) {
            ordered.push(image_type.clone());
        }
    }
    if ordered.is_empty() {
        for key in meta
            .title
            .properties
            .keys()
            .chain(meta.properties.keys())
            .filter_map(|key| image_type_from_property_key(key))
        {
            seen.insert(key);
        }
        if seen.remove("image") {
            ordered.push("image".to_string());
        }
        ordered.extend(seen);
    }
    ordered
        .into_iter()
        // Filter to the types Core can actually serve, the same way the
        // browse pane's carousel does (`games.rs`'s
        // `ordered_detail_image_keys`). Without it `image_count` counts
        // types Core stores but never hands back as bytes, so a left/right
        // cycle lands on a slot whose fetch always misses and the modal
        // sits on "Loading image…" with no timeout and no error state.
        .filter(|image_type| CORE_SERVEABLE_IMAGE_TYPES.contains(&image_type.as_str()))
        .map(|image_type| MediaKey::with_image_type(system, path, image_type))
        .collect()
}

fn clear_detail_images(mut model: Pin<&mut ffi::GameInfo>) {
    model.as_mut().rust_mut().detail_image_keys.clear();
    model.as_mut().set_image_key(QString::default());
    model.as_mut().set_image_index(0);
    model.as_mut().set_image_count(0);
    model.as_mut().set_image_can_prev(false);
    model.as_mut().set_image_can_next(false);
}

fn install_detail_images(mut model: Pin<&mut ffi::GameInfo>, keys: Vec<MediaKey>) {
    model.as_mut().rust_mut().detail_image_keys = keys;
    set_detail_image_index(model, 0);
}

fn set_detail_image_index(mut model: Pin<&mut ffi::GameInfo>, index: i32) {
    let count = i32::try_from(model.detail_image_keys.len()).unwrap_or(i32::MAX);
    let clamped = if count <= 0 {
        0
    } else {
        index.clamp(0, count - 1)
    };
    model.as_mut().set_image_index(clamped);
    model.as_mut().set_image_count(count);
    model.as_mut().set_image_can_prev(clamped > 0);
    model
        .as_mut()
        .set_image_can_next(count > 0 && clamped < count - 1);
    sync_current_image_key(model);
}

fn sync_current_image_key(mut model: Pin<&mut ffi::GameInfo>) {
    let index = model.image_index;
    if index < 0 {
        model.as_mut().set_image_key(QString::default());
        return;
    }
    let Some(key) = model.detail_image_keys.get(index as usize).cloned() else {
        model.as_mut().set_image_key(QString::default());
        return;
    };
    let cache = global_media_image_cache();
    if cache.is_cached(&key) {
        model
            .as_mut()
            .set_image_key(QString::from(MediaImageCache::image_key_for(&key).as_str()));
    } else {
        cache.enqueue_with_media_id(key, None, 1);
        model.as_mut().set_image_key(QString::default());
    }
}

fn notify_cover_update(mut model: Pin<&mut ffi::GameInfo>, update: &MediaImageUpdate) {
    let current = model
        .detail_image_keys
        .get(model.image_index as usize)
        .cloned();
    if !current
        .as_ref()
        .is_some_and(|current| current == &update.key)
    {
        return;
    }
    if update.ext.is_some() {
        model.as_mut().set_image_key(QString::from(
            MediaImageCache::image_key_for(&update.key).as_str(),
        ));
    } else {
        model.as_mut().set_image_key(QString::default());
    }
}

fn file_stem_or_name(path: &str) -> String {
    let file = path
        .trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or_default();
    file.rsplit_once('.')
        .map_or(file, |(stem, _)| stem)
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::{detail_image_keys_from_meta, detail_tags_from_meta, normalize_tag_type};
    use std::collections::HashMap;
    use zaparoo_core::media_types::{MediaMeta, MediaMetaProperty, TagInfo};

    fn tag(tag_type: &str, value: &str) -> TagInfo {
        TagInfo {
            tag: value.to_string(),
            tag_type: tag_type.to_string(),
            label: String::new(),
        }
    }

    fn property(text: &str) -> MediaMetaProperty {
        MediaMetaProperty {
            text: text.to_string(),
            content_type: String::new(),
            extension: None,
            blob_size: 0,
        }
    }

    fn rows(detail: &str) -> Vec<(String, String)> {
        detail
            .lines()
            .filter_map(|line| line.split_once('\t'))
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .collect()
    }

    fn value_for(detail: &str, key: &str) -> Option<String> {
        rows(detail)
            .into_iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v)
    }

    /// The table must carry tag *types*, never English labels: the label is
    /// chosen in QML so it can go through `qsTr()`.
    #[test]
    fn detail_rows_are_keyed_by_canonical_tag_type() {
        let mut meta = MediaMeta::default();
        meta.title.tags = vec![tag("Release Date", "1994-03-19"), tag("genre", "RPG")];
        let detail = detail_tags_from_meta(&meta, "/games/SNES/game.sfc");
        assert_eq!(
            value_for(&detail, "release_date").as_deref(),
            Some("1994-03-19")
        );
        assert_eq!(value_for(&detail, "genre").as_deref(), Some("RPG"));
        assert!(value_for(&detail, "Release date").is_none());
    }

    #[test]
    fn ordered_types_lead_and_filename_trails() {
        let mut meta = MediaMeta::default();
        meta.title.tags = vec![tag("publisher", "Squaresoft"), tag("year", "1994")];
        let keys: Vec<String> = rows(&detail_tags_from_meta(&meta, "/games/SNES/game.sfc"))
            .into_iter()
            .map(|(k, _)| k)
            .collect();
        assert_eq!(keys.first().map(String::as_str), Some("year"));
        assert_eq!(keys.last().map(String::as_str), Some("filename"));
    }

    /// Whether a scraper stored the rating as a tag or as a property is its
    /// own choice. Reading only `property:description` meant every other
    /// scraped property was dropped and the modal looked like Core had
    /// nothing to show.
    #[test]
    fn scraped_properties_other_than_description_become_rows() {
        let mut meta = MediaMeta::default();
        meta.title.properties = HashMap::from([
            ("property:rating".to_string(), property("4.5")),
            ("property:description".to_string(), property("A long blurb")),
            ("property:image-boxart".to_string(), property("ignored")),
        ]);
        let detail = detail_tags_from_meta(&meta, "/games/SNES/game.sfc");
        assert_eq!(value_for(&detail, "rating").as_deref(), Some("4.5"));
        assert!(
            value_for(&detail, "description").is_none(),
            "description has its own block in the modal"
        );
        assert!(
            !detail.contains("image"),
            "image payload keys belong to the carousel, not the table"
        );
    }

    #[test]
    fn multiline_property_values_stay_within_one_row() {
        let mut meta = MediaMeta::default();
        meta.title.properties = HashMap::from([(
            "property:developer".to_string(),
            property("First line\nSecond\tfield\rThird line"),
        )]);
        let detail = detail_tags_from_meta(&meta, "/games/SNES/game.sfc");
        assert_eq!(
            value_for(&detail, "developer").as_deref(),
            Some("First line Second field Third line")
        );
        assert_eq!(
            rows(&detail)
                .iter()
                .filter(|(key, _)| key == "developer")
                .count(),
            1
        );
    }

    #[test]
    fn a_tag_wins_over_a_property_of_the_same_type() {
        let mut meta = MediaMeta::default();
        meta.title.tags = vec![tag("rating", "from tag")];
        meta.title.properties =
            HashMap::from([("property:rating".to_string(), property("from property"))]);
        let detail = detail_tags_from_meta(&meta, "/games/SNES/game.sfc");
        assert_eq!(value_for(&detail, "rating").as_deref(), Some("from tag"));
        assert_eq!(
            rows(&detail).iter().filter(|(k, _)| k == "rating").count(),
            1
        );
    }

    /// Core populates `title.system.name` even when it emits no `system`
    /// tag, and the system is the field a user is most likely to want.
    #[test]
    fn system_falls_back_to_the_title_system_name() {
        let mut meta = MediaMeta::default();
        meta.title.system.name = "Super Nintendo".to_string();
        let detail = detail_tags_from_meta(&meta, "/games/SNES/game.sfc");
        assert_eq!(
            value_for(&detail, "system").as_deref(),
            Some("Super Nintendo")
        );
    }

    #[test]
    fn an_explicit_system_tag_wins_over_the_title_system_name() {
        let mut meta = MediaMeta::default();
        meta.title.system.name = "Super Nintendo".to_string();
        meta.title.tags = vec![tag("system", "SNES")];
        let detail = detail_tags_from_meta(&meta, "/games/SNES/game.sfc");
        assert_eq!(value_for(&detail, "system").as_deref(), Some("SNES"));
        assert_eq!(
            rows(&detail).iter().filter(|(k, _)| k == "system").count(),
            1
        );
    }

    /// An unserveable type in the carousel is a slot whose fetch always
    /// misses, so the modal would sit on "Loading image…" forever.
    #[test]
    fn carousel_drops_image_types_core_cannot_serve() {
        let meta = MediaMeta {
            available_image_types: vec![
                "boxart".to_string(),
                "boxart3d".to_string(),
                "thumbnail".to_string(),
                "screenshot".to_string(),
            ],
            ..Default::default()
        };
        let keys = detail_image_keys_from_meta(&meta, "SNES", "/games/SNES/game.sfc");
        assert_eq!(keys.len(), 2, "only the serveable types may become slots");
    }

    #[test]
    fn tag_type_normalization_folds_spacing_and_case() {
        assert_eq!(normalize_tag_type(" Release Date "), "release_date");
        assert_eq!(normalize_tag_type("play-mode"), "play_mode");
        assert_eq!(normalize_tag_type("GENRE"), "genre");
    }
}
