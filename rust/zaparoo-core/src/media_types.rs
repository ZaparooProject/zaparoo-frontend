// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

use serde::{Deserialize, Deserializer, Serialize};
use std::collections::HashMap;

/// Serde helper: deserializes `null` as the type's `Default` instead of
/// failing. Used on `Vec<T>` fields where Core may emit `null` for an empty
/// collection (Go nil slices marshal as `null`, not `[]`).
fn deserialize_null_default<'de, D, T>(d: D) -> Result<T, D::Error>
where
    D: Deserializer<'de>,
    T: Default + Deserialize<'de>,
{
    let opt = Option::deserialize(d)?;
    Ok(opt.unwrap_or_default())
}

/// Serde default for `has_cover`: true when the field is absent so that
/// clients receiving responses from older Core builds (which don't emit
/// `hasCover`) still request covers rather than silently skipping them.
fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemInfo {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub release_date: Option<String>,
    #[serde(default)]
    pub manufacturer: Option<String>,
}

/// Parameters for `media.search`. Mirrors Core's `SearchParams`
/// surface — text query, system filter, tag filter, alphabetical
/// letter, and cursor for pagination. Every field is optional;
/// `skip_serializing_if` keeps absent fields off the wire so Core sees
/// the same shape it would for a hand-rolled minimal request.
//
// Core also accepts a `fuzzySystem` boolean for LLM clients that may
// misspell system ids; the frontend composes ids from canonical Core
// data so a mismatch would be a bug, and we deliberately do not
// surface that flag here.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaSearchParams {
    /// Free-text search across media names. Empty = match anything (the
    /// other filters narrow the result set).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub systems: Vec<String>,
    /// Page size. `None` lets Core pick its default (currently 100).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_results: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    /// Limit results to media tagged with all of the given tags.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    /// Filter to entries whose name starts with the given letter.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub letter: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct TagInfo {
    pub tag: String,
    #[serde(rename = "type", default)]
    pub tag_type: String,
    #[serde(default)]
    pub label: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaItem {
    /// Opaque media database row ID. Treat as ephemeral — valid only
    /// for the current Core session and only meaningful when used as a
    /// shorthand for `(system, path)` on follow-up
    /// `media.image`/`media.meta` requests in the same session.
    #[serde(default)]
    pub media_id: Option<i64>,
    pub name: String,
    pub path: String,
    #[serde(default)]
    pub zap_script: String,
    #[serde(default)]
    pub system: System,
    #[serde(default)]
    pub tags: Vec<TagInfo>,
    /// Subset of `tags` whose values differ across same-named siblings of this
    /// title (region, disc, rev, builddate, lang, ...), already ordered by
    /// Core's display priority. Omitted when the title has nothing to
    /// disambiguate. Rendered as variant badges in the browse UI.
    #[serde(default)]
    pub disambiguating_tags: Vec<TagInfo>,
    /// Path relative to the system's root (`SearchResultMedia.relativePath`).
    /// `None` when Core was unable to derive one (e.g. media outside any
    /// indexed root).
    #[serde(default)]
    pub relative_path: Option<String>,
}

/// System sub-object returned by `media.search`/`media.lookup`. Mirrors
/// Core's full `models.System` shape — DB-stored fields plus the
/// metadata enrichment (`releaseDate`, `manufacturer`) Core derives from
/// its static asset bundle. Distinct from `SystemInfo` (the bare list
/// row) so future refactors that reshape one don't accidentally drag the
/// other along.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct System {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub release_date: Option<String>,
    #[serde(default)]
    pub manufacturer: Option<String>,
}

/// Trimmed `system` sub-object returned inside `media.meta`'s title
/// block. Core deliberately omits the static-asset enrichment here
/// (`MediaMetaSystemResponse` is DB-only), so we keep the type distinct
/// from `System` to make that contract visible.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaMetaSystemRef {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
}

/// Cursor-based pagination envelope shared by `media.search` and
/// `media.browse`. Fields are all defaulted so an absent envelope (e.g.
/// browse-root response with no file results) deserializes to "no more
/// pages."
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Pagination {
    #[serde(default)]
    pub has_next_page: bool,
    #[serde(default)]
    pub page_size: u32,
    #[serde(default)]
    pub next_cursor: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaSearchResult {
    pub results: Vec<MediaItem>,
    /// Pagination envelope. Absent when there are no results to page
    /// through, matching `MediaBrowseResult`/`MediaHistoryResult`.
    #[serde(default)]
    pub pagination: Option<Pagination>,
    /// Total result count across all pages. Core can return `-1` to
    /// signal "unknown / unbounded" so this is intentionally not used as
    /// an iteration bound; treat it as a UI hint only.
    #[serde(default)]
    pub total: i64,
}

impl MediaSearchResult {
    pub fn has_next_page(&self) -> bool {
        self.pagination.as_ref().is_some_and(|p| p.has_next_page)
    }

    pub fn next_cursor(&self) -> Option<String> {
        self.pagination.as_ref().and_then(|p| p.next_cursor.clone())
    }
}

// Core's `media.browse` also accepts a `fuzzySystem` boolean that lets
// LLM clients route a misspelt system id through fuzzy matching. The
// frontend composes its system ids from canonical Core data (the
// `systems` RPC), so a mismatch here would be a frontend bug — we
// deliberately do not surface that flag, to keep bugs visible rather
// than papered over.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaBrowseParams {
    #[serde(skip_serializing_if = "String::is_empty")]
    pub path: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub systems: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_results: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    /// Filter to entries whose name starts with the given letter.
    /// Validated by Core against a single-letter pattern.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub letter: Option<String>,
    /// Sort order for results. Accepted values: `name-asc`, `name-desc`,
    /// `filename-asc`, `filename-desc`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sort: Option<String>,
}

// `Default` is implemented manually below so that `has_cover` defaults to
// `true` (request covers unless Core explicitly says otherwise) rather than
// the Rust zero-value `false`. The serde `default = "default_true"` handles
// the wire-protocol case (field absent); this impl handles Rust callers that
// use `..BrowseEntry::default()` in tests and struct-update syntax.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowseEntry {
    /// Opaque media database row ID. Present on `media` entries and
    /// singleton media-container `directory` entries on zip-as-directory
    /// platforms. Treat as ephemeral — valid only for the current Core session.
    #[serde(default)]
    pub media_id: Option<i64>,
    pub name: String,
    pub path: String,
    #[serde(rename = "type", default)]
    pub entry_type: String,
    #[serde(default)]
    pub file_count: u32,
    #[serde(default)]
    pub system_id: String,
    #[serde(default)]
    pub system_ids: Vec<String>,
    #[serde(default)]
    pub zap_script: String,
    #[serde(default)]
    pub relative_path: String,
    #[serde(default)]
    pub group: String,
    #[serde(default)]
    pub description: String,
    /// Tags attached to media-capable entries. Empty for normal folders;
    /// Core can populate this on singleton media-container directories.
    #[serde(default)]
    pub tags: Vec<TagInfo>,
    /// Subset of `tags` whose values differ across same-named siblings of this
    /// title, ordered by Core's display priority. Omitted when there is
    /// nothing to disambiguate. Rendered as variant badges in the browse UI.
    #[serde(default)]
    pub disambiguating_tags: Vec<TagInfo>,
    /// When `false`, Core has confirmed this media has no cover image in
    /// its properties tables. Defaults to `true` when the field is absent
    /// (older Core builds don't send it) so cover requests are still made.
    /// Only meaningful for `media` entries; folders always behave as `true`.
    #[serde(default = "default_true")]
    pub has_cover: bool,
}

impl Default for BrowseEntry {
    fn default() -> Self {
        Self {
            media_id: None,
            name: String::new(),
            path: String::new(),
            entry_type: String::new(),
            file_count: 0,
            system_id: String::new(),
            system_ids: Vec::new(),
            zap_script: String::new(),
            relative_path: String::new(),
            group: String::new(),
            description: String::new(),
            tags: Vec::new(),
            disambiguating_tags: Vec::new(),
            // Default to true so callers that don't set this field (tests,
            // struct-update syntax) still request covers from Core.
            has_cover: true,
        }
    }
}

impl BrowseEntry {
    /// Upstream entry types are `root`, `directory`, and `media`. Both
    /// roots and directories are navigable containers; media entries are
    /// leaves.
    pub fn is_folder(&self) -> bool {
        self.entry_type == "directory" || self.entry_type == "root"
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaBrowseResult {
    #[serde(default)]
    pub path: String,
    pub entries: Vec<BrowseEntry>,
    #[serde(default)]
    pub total_files: u32,
    #[serde(default)]
    pub pagination: Option<Pagination>,
}

impl MediaBrowseResult {
    pub fn has_next_page(&self) -> bool {
        self.pagination.as_ref().is_some_and(|p| p.has_next_page)
    }

    pub fn next_cursor(&self) -> Option<String> {
        self.pagination.as_ref().and_then(|p| p.next_cursor.clone())
    }
}

// Parameters for `media.browse.index`. The scope fields mirror
// `MediaBrowseParams` so the returned index describes the exact list
// `media.browse` would page through for the same scope. As with browse, the
// `fuzzySystem` flag is deliberately not surfaced: the frontend composes
// canonical system ids, so a mismatch is a bug rather than something to fuzz
// over.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaBrowseIndexParams {
    #[serde(skip_serializing_if = "String::is_empty")]
    pub path: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub systems: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sort: Option<String>,
}

/// One first-character section of a browse list. `key`/`label` are opaque so a
/// future locale-aware scheme (pinyin/kana/hangul) needs no client change.
/// `cursor` is an ordinary `media.browse` cursor positioned just before the
/// bucket's first row; an empty string means the bucket begins the list (browse
/// with no cursor).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowseIndexGroup {
    pub key: String,
    pub label: String,
    #[serde(default)]
    pub count: u32,
    #[serde(default)]
    pub cursor: String,
    /// 0-based position of the bucket's first item among the scope's files
    /// (excludes leading directories, which the client adds). Authoritative
    /// browse-order position from Core, used to jump to the bucket.
    #[serde(default)]
    pub offset: u32,
}

/// Response for `media.browse.index`. `scheme` is the collation used to derive
/// the buckets (`latin`, or `none` when no rail applies, in which case `groups`
/// is empty).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaBrowseIndexResult {
    #[serde(default)]
    pub scheme: String,
    #[serde(default)]
    pub total_files: u32,
    #[serde(default)]
    pub groups: Vec<BrowseIndexGroup>,
}

impl MediaBrowseIndexResult {
    /// Serialize the buckets as a JSON array of `{key,label,count,cursor}`
    /// objects. The frontend surfaces this string to QML, where the
    /// jump-to-letter picker parses it (the pickers already consume `var`
    /// arrays, so a JSON string fits that convention). Returns `[]` on the
    /// (practically impossible) serialize failure so QML always gets valid
    /// JSON to parse.
    #[must_use]
    pub fn groups_json(&self) -> String {
        serde_json::to_string(&self.groups).unwrap_or_else(|_| "[]".to_string())
    }
}

/// Parameters for `media.history`. Cursor-driven pagination shares the
/// same shape as `media.browse`/`media.search`; fields are optional and
/// `skip_serializing_if` keeps the on-the-wire object minimal.
//
// Core also accepts a `fuzzySystem` boolean for LLM clients that may
// misspell system ids; the frontend composes ids from canonical Core
// data so a mismatch would be a bug, and we deliberately do not
// surface that flag here.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaHistoryParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub systems: Vec<String>,
}

/// One entry in `media.history`. Field shapes mirror Core's docs; we
/// don't need `started_at`/`ended_at`/`play_time` for the launch UI yet
/// but keep them deserialised so future "most-played" / "last-played"
/// captions don't need a re-roundtrip.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaHistoryEntry {
    /// Opaque media database row ID. Omitted when the history path
    /// cannot be resolved in the current media database. Treat as
    /// ephemeral — valid only for the current Core session.
    #[serde(default)]
    pub media_id: Option<i64>,
    #[serde(default)]
    pub system_id: String,
    #[serde(default)]
    pub system_name: String,
    #[serde(default)]
    pub media_name: String,
    #[serde(default)]
    pub media_path: String,
    #[serde(default)]
    pub launcher_id: String,
    #[serde(default)]
    pub started_at: String,
    /// `None` while a session is still in progress; Core only emits a
    /// value once the session has ended.
    #[serde(default)]
    pub ended_at: Option<String>,
    #[serde(default)]
    pub play_time: u64,
}

/// Response envelope for `media.history`. Pagination is "only present
/// when entries are returned" per Core's docs, so wrap it in `Option`
/// the same way `MediaBrowseResult` does.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaHistoryResult {
    #[serde(default)]
    pub entries: Vec<MediaHistoryEntry>,
    #[serde(default)]
    pub pagination: Option<Pagination>,
}

impl MediaHistoryResult {
    pub fn has_next_page(&self) -> bool {
        self.pagination.as_ref().is_some_and(|p| p.has_next_page)
    }

    pub fn next_cursor(&self) -> Option<String> {
        self.pagination.as_ref().and_then(|p| p.next_cursor.clone())
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaHistoryLatestEntry {
    #[serde(default)]
    pub system_id: String,
    #[serde(default)]
    pub system_name: String,
    #[serde(default)]
    pub media_name: String,
    #[serde(default)]
    pub media_path: String,
    #[serde(default)]
    pub launcher_id: String,
    #[serde(default)]
    pub started_at: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaHistoryLatestResult {
    #[serde(default)]
    pub entry: Option<MediaHistoryLatestEntry>,
}

/// Parameters for single-image `media.image`. Core identifies the
/// media row by `media_id` when available, otherwise by `(system,
/// path)` where `path` is the canonical indexed media path returned by
/// `media.search`/`media.browse`. `image_types` is an optional
/// preference list documented in methods.md.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaImageParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub media_id: Option<i64>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub system: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub path: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub image_types: Vec<String>,
    /// Maximum dimension (width or height) in pixels. Core resizes the
    /// image to fit within a `max_size × max_size` bounding box before
    /// returning it. Omit (or send 0) to receive the full-resolution blob.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_size: Option<u32>,
}

impl MediaImageParams {
    pub fn for_media(system: impl Into<String>, path: impl Into<String>) -> Self {
        Self {
            media_id: None,
            system: system.into(),
            path: path.into(),
            image_types: Vec::new(),
            max_size: None,
        }
    }

    pub fn for_media_id(media_id: i64) -> Self {
        Self {
            media_id: Some(media_id),
            system: String::new(),
            path: String::new(),
            image_types: Vec::new(),
            max_size: None,
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaImageResult {
    #[serde(default)]
    pub content_type: String,
    /// File extension without a leading dot, derived by Core from the
    /// MIME type or source path. `None` when Core could not derive one
    /// — distinct from `Some("")`. Prefer this over sniffing
    /// `content_type` or the binary payload.
    #[serde(default)]
    pub extension: Option<String>,
    /// Base64-encoded image bytes.
    #[serde(default)]
    pub data: String,
    #[serde(default)]
    pub type_tag: String,
}

/// Parameters for `media.meta`. Identifies the media row by `media_id`
/// when available, otherwise by `(system, path)`. The result includes
/// ROM-level and title-level metadata — tags, properties (text or
/// binary with `extension` + `contentType`), and the shared title block.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaMetaParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub media_id: Option<i64>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub system: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub path: String,
}

impl MediaMetaParams {
    pub fn for_media(system: impl Into<String>, path: impl Into<String>) -> Self {
        Self {
            media_id: None,
            system: system.into(),
            path: path.into(),
        }
    }

    pub fn for_media_id(media_id: i64) -> Self {
        Self {
            media_id: Some(media_id),
            system: String::new(),
            path: String::new(),
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaMetaResult {
    pub media: MediaMeta,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaMeta {
    #[serde(default)]
    pub path: String,
    #[serde(default)]
    pub parent_dir: String,
    #[serde(default)]
    pub is_missing: bool,
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub tags: Vec<TagInfo>,
    /// ROM-level scraped properties keyed by canonical type tag (e.g.
    /// `property:description`, `property:image-boxart`).
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub properties: HashMap<String, MediaMetaProperty>,
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub available_image_types: Vec<String>,
    #[serde(default)]
    pub title: MediaMetaTitle,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaMetaTitle {
    #[serde(default)]
    pub slug: String,
    /// Optional secondary slug (alternate title form). `None` when the
    /// title has no secondary form on record.
    #[serde(default)]
    pub secondary_slug: Option<String>,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub slug_length: u32,
    #[serde(default)]
    pub slug_word_count: u32,
    #[serde(default)]
    pub system: MediaMetaSystemRef,
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub tags: Vec<TagInfo>,
    /// Title-level scraped properties shared by all rows under the same
    /// title slug.
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub properties: HashMap<String, MediaMetaProperty>,
    #[serde(default, deserialize_with = "deserialize_null_default")]
    pub available_image_types: Vec<String>,
}

/// One scraped property attached to a media row or title. Binary
/// payloads are not included in `media.meta`; use `media.image` for
/// image bytes. `blob_size` is present when Core has binary metadata.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaMetaProperty {
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub content_type: String,
    /// File extension without a leading dot, when Core can derive one
    /// from the MIME type or source path. `None` for text-only
    /// properties or when Core could not derive an extension.
    #[serde(default)]
    pub extension: Option<String>,
    #[serde(default)]
    pub blob_size: i64,
}

/// Parameters for `media.lookup` — fuzzy title resolution against the
/// scraped catalog. `system` and `name` are required; the frontend
/// composes both from canonical Core data, so we deliberately do not
/// expose Core's `fuzzySystem` flag (it exists for LLM clients that may
/// misspell ids; a frontend mismatch is a bug to fix, not paper over).
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaLookupParams {
    pub system: String,
    pub name: String,
}

/// Result envelope for `media.lookup`. Core returns `{match: null}` for
/// `ErrNoMatch` / `ErrLowConfidence` rather than raising a JSON-RPC
/// error, so `match_: None` is the "no match found" case (not an error
/// signal — the call itself succeeded).
#[derive(Debug, Clone, Default, Deserialize)]
pub struct MediaLookupResult {
    // `match` is a Rust keyword; the field is renamed via serde while
    // the wire form stays `match`. Same pattern as
    // `BrowseEntry.entry_type` and `TagInfo.tag_type`.
    #[serde(rename = "match", default)]
    pub match_: Option<MediaLookupMatch>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaLookupMatch {
    pub name: String,
    pub path: String,
    #[serde(default)]
    pub zap_script: String,
    #[serde(default)]
    pub system: System,
    #[serde(default)]
    pub tags: Vec<TagInfo>,
    /// Path relative to the system's root, when Core was able to derive
    /// one. Mirrors `MediaItem.relative_path`.
    #[serde(default)]
    pub relative_path: Option<String>,
    /// Match confidence in `[0, 1]`. Below Core's threshold the match
    /// would be returned as `{match: null}`, so any value here is
    /// already considered "high enough"; the field is exposed so a UI
    /// can surface the raw score.
    #[serde(default)]
    pub confidence: f64,
}

/// Parameters for `media.history.top` — most-played aggregates over the
/// session log. `since` is an RFC3339 timestamp; `limit` caps the
/// returned entry count.
//
// Core also accepts a `fuzzySystem` boolean for LLM clients; the
// frontend composes ids from canonical Core data so a mismatch would
// be a bug, and we deliberately do not surface that flag here.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaHistoryTopParams {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub systems: Vec<String>,
    /// RFC3339 timestamp; entries with a `last_played_at` earlier than
    /// this are excluded.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub since: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaHistoryTopEntry {
    #[serde(default)]
    pub system_id: String,
    #[serde(default)]
    pub system_name: String,
    #[serde(default)]
    pub media_name: String,
    #[serde(default)]
    pub media_path: String,
    /// RFC3339 timestamp of the most recent session for this media.
    #[serde(default)]
    pub last_played_at: String,
    /// Cumulative play time in seconds.
    #[serde(default)]
    pub total_play_time: u64,
    #[serde(default)]
    pub session_count: u32,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaHistoryTopResult {
    #[serde(default)]
    pub entries: Vec<MediaHistoryTopEntry>,
}

/// Parameters for `media.tags` — list the available tag index, optionally
/// scoped to a system filter. Core's handler reuses `SearchParams` on
/// the wire but only consults `systems`/`fuzzySystem`, so we expose a
/// trimmed type here. (As elsewhere, `fuzzySystem` is intentionally
/// omitted; the frontend composes canonical ids.)
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaTagsParams {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub systems: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct MediaTagsResult {
    #[serde(default)]
    pub tags: Vec<TagInfo>,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaTagsUpdateParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub media_id: Option<i64>,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub system: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub path: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub add: Vec<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub remove: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct MediaTagsUpdateResult {
    #[serde(default)]
    pub tags: Vec<TagInfo>,
}

/// Parameters for `media.generate` — triggers a (re)build of Core's media
/// database. `systems` optionally narrows the scope; `None` indexes every
/// configured system. `fuzzySystem` is intentionally omitted: it exists
/// in Core for LLM clients that may misspell ids, and the frontend
/// composes ids from canonical Core data so a mismatch would be a bug
/// to fix rather than paper over.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaIndexParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub systems: Option<Vec<String>>,
}

/// Parameters for `media.scrape` — runs the named scraper across the
/// indexed media database. `scraper_id` is required server-side
/// (validated as `min=1`); the frontend resolves it from the `scrapers`
/// RPC. `systems` optionally narrows the run; `force` re-scrapes media
/// already attached to a title slug.
#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaScrapeParams {
    pub scraper_id: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub systems: Vec<String>,
    #[serde(default)]
    pub force: bool,
}

/// Snapshot of Core's media database build state. Mirrors the upstream
/// `IndexingStatusResponse` shape verbatim — every numeric field is
/// optional because Core only populates them while a build is actually
/// in progress (or reports `total_media`/`total_files` after the build
/// settles).
///
/// Used for both the `media.indexing` notification body and the
/// `database` block of the `media` query response, so we keep it in
/// one place.
#[allow(
    clippy::struct_excessive_bools,
    reason = "wire-faithful mirror of Core's IndexingStatusResponse"
)]
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexingStatusResponse {
    #[serde(default)]
    pub total_steps: Option<i32>,
    #[serde(default)]
    pub current_step: Option<i32>,
    #[serde(default)]
    pub current_step_display: Option<String>,
    #[serde(default)]
    pub total_files: Option<i32>,
    #[serde(default)]
    pub total_media: Option<i32>,
    #[serde(default)]
    pub exists: bool,
    #[serde(default)]
    pub indexing: bool,
    #[serde(default)]
    pub optimizing: bool,
    #[serde(default)]
    pub paused: bool,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScrapeSystemProgressResponse {
    #[serde(default)]
    pub system_id: String,
    #[serde(default)]
    pub system_name: String,
    #[serde(default)]
    pub processed: i32,
    #[serde(default)]
    pub total: i32,
    #[serde(default)]
    pub matched: i32,
    #[serde(default)]
    pub skipped: i32,
}

/// Snapshot of Core's scraper progress. Mirrors `ScrapingStatusResponse`
/// from upstream. `total_steps` / `current_step` carry whole-run system
/// progress; flat counters remain the per-current-system compatibility
/// surface.
#[allow(
    clippy::struct_excessive_bools,
    reason = "wire-faithful mirror of Core's ScrapingStatusResponse"
)]
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScrapingStatusResponse {
    #[serde(default)]
    pub current_step: Option<i32>,
    #[serde(default)]
    pub current_step_display: Option<String>,
    #[serde(default)]
    pub total_steps: Option<i32>,
    #[serde(default)]
    pub current_system: Option<ScrapeSystemProgressResponse>,
    #[serde(default)]
    pub scraper_id: String,
    #[serde(default)]
    pub system_id: String,
    #[serde(default)]
    pub state: String,
    #[serde(default)]
    pub error: String,
    #[serde(default)]
    pub processed: i32,
    #[serde(default)]
    pub total: i32,
    #[serde(default)]
    pub matched: i32,
    #[serde(default)]
    pub skipped: i32,
    #[serde(default)]
    pub total_scraped: i32,
    #[serde(default)]
    pub force: Option<bool>,
    #[serde(default)]
    pub scraping: bool,
    #[serde(default)]
    pub done: bool,
    #[serde(default)]
    pub paused: bool,
}

/// Currently-active media as reported by `media`. The frontend does not
/// surface this surface yet, but the field is part of the documented
/// `media` envelope so we deserialise it for forward-compatibility —
/// trimming it would mean the next consumer has to re-extend the wire
/// type before they can use it.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActiveMediaInfo {
    #[serde(default)]
    pub started: String,
    #[serde(default)]
    pub launcher_id: String,
    #[serde(default)]
    pub system_id: String,
    #[serde(default)]
    pub system_name: String,
    #[serde(default)]
    pub media_path: String,
    #[serde(default)]
    pub media_name: String,
    #[serde(default)]
    pub launcher_controls: Vec<String>,
    #[serde(default)]
    pub media_id: Option<i64>,
    #[serde(default)]
    pub relative_path: Option<String>,
    #[serde(default)]
    pub zap_script: String,
}

/// Response envelope for the `media` query. Carries both the database
/// build state (used for the first-run gate / status pill) and the
/// active-media list (carried for completeness — see
/// `ActiveMediaInfo`).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaResult {
    #[serde(default)]
    pub database: IndexingStatusResponse,
    #[serde(default)]
    pub active: Vec<ActiveMediaInfo>,
}

/// One scraper Core knows how to run. `id` is the value to pass to
/// `media.scrape.scraperId`; `name` is a human label; `supported_systems`
/// is the system-id allow-list — empty means "supports every indexed
/// system."
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ScraperInfo {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub supported_systems: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ScrapersResult {
    #[serde(default)]
    pub scrapers: Vec<ScraperInfo>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SystemDefault {
    pub system: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub launcher: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub before_exit: String,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsResult {
    #[serde(default)]
    pub system_defaults: Vec<SystemDefault>,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct LogDownloadResult {
    #[serde(default)]
    pub filename: String,
    #[serde(default)]
    pub size: u64,
    #[serde(default)]
    pub content: String,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct HealthResult {
    #[serde(default)]
    pub status: String,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TokenInfo {
    #[serde(default, rename = "type")]
    pub token_type: String,
    #[serde(default)]
    pub uid: String,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub data: String,
    #[serde(default)]
    pub scan_time: String,
    #[serde(default)]
    pub reader_id: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct TokensResult {
    #[serde(default)]
    pub active: Vec<TokenInfo>,
    #[serde(default)]
    pub last: Option<TokenInfo>,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct LaunchEntry {
    #[serde(default, rename = "type")]
    pub token_type: String,
    #[serde(default)]
    pub uid: String,
    #[serde(default)]
    pub text: String,
    #[serde(default)]
    pub data: String,
    #[serde(default)]
    pub success: bool,
    #[serde(default)]
    pub time: String,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
pub struct TokensHistoryResult {
    #[serde(default)]
    pub entries: Vec<LaunchEntry>,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateSettingsParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_defaults: Option<Vec<SystemDefault>>,
}

#[derive(Debug, Clone, Default, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LauncherInfo {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub system_id: String,
    #[serde(default)]
    pub system_name: String,
    #[serde(default)]
    pub groups: Vec<String>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LaunchersResult {
    #[serde(default)]
    pub launchers: Vec<LauncherInfo>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct RunParams {
    pub text: String,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct ReadersWriteParams {
    pub text: String,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct SystemsParams {}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct SystemsResult {
    pub systems: Vec<SystemInfo>,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReaderInfo {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub reader_id: String,
    #[serde(default)]
    pub driver: String,
    #[serde(default)]
    pub info: String,
    #[serde(default)]
    pub connected: bool,
    #[serde(default)]
    pub capabilities: Vec<String>,
}

impl ReaderInfo {
    pub fn is_nfc_reader(&self) -> bool {
        if !self.connected {
            return false;
        }
        let driver = self.driver.to_lowercase();
        let info = self.info.to_lowercase();
        let has_nfc_capability = self
            .capabilities
            .iter()
            .any(|capability| capability.eq_ignore_ascii_case("nfc"));
        has_nfc_capability
            || driver.contains("pn532")
            || driver.contains("acr122")
            || driver.contains("rc522")
            || info.contains("pn532")
            || info.contains("acr122")
            || info.contains("rc522")
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct ReadersResult {
    #[serde(default)]
    pub readers: Vec<ReaderInfo>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct VersionResult {
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub platform: String,
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::expect_used,
        clippy::unwrap_used,
        clippy::panic,
        reason = "tests should fail-fast on unexpected errors"
    )]

    use super::{
        BrowseEntry, BrowseIndexGroup, HealthResult, IndexingStatusResponse, LaunchersResult,
        LogDownloadResult, MediaBrowseIndexParams, MediaBrowseIndexResult, MediaBrowseParams,
        MediaBrowseResult, MediaHistoryLatestResult, MediaHistoryParams, MediaHistoryResult,
        MediaHistoryTopParams, MediaHistoryTopResult, MediaImageParams, MediaImageResult,
        MediaIndexParams, MediaLookupParams, MediaLookupResult, MediaMetaParams, MediaMetaResult,
        MediaResult, MediaScrapeParams, MediaSearchParams, MediaSearchResult, MediaTagsParams,
        MediaTagsResult, ReaderInfo, ReadersResult, ScrapersResult, ScrapingStatusResponse,
        SettingsResult, SystemDefault, SystemsResult, TagInfo, TokensHistoryResult, TokensResult,
        UpdateSettingsParams, VersionResult,
    };

    #[test]
    fn is_folder_accepts_directory_and_root() {
        let directory = BrowseEntry {
            entry_type: "directory".into(),
            ..BrowseEntry::default()
        };
        let root = BrowseEntry {
            entry_type: "root".into(),
            ..BrowseEntry::default()
        };
        let media = BrowseEntry {
            entry_type: "media".into(),
            ..BrowseEntry::default()
        };
        assert!(directory.is_folder());
        assert!(root.is_folder());
        assert!(!media.is_folder());
    }

    #[test]
    fn is_folder_unknown_type_is_false() {
        for entry_type in ["", "folder", "file", "symlink", "archive", "DIRECTORY"] {
            let entry = BrowseEntry {
                entry_type: entry_type.into(),
                ..BrowseEntry::default()
            };
            assert!(
                !entry.is_folder(),
                "entry_type={entry_type:?} should not be classified as folder"
            );
        }
    }

    #[test]
    fn systems_result_deserialises_camelcase_payload() {
        let json = r#"{"systems":[{"id":"NES","name":"Nintendo","category":"Consoles"}]}"#;
        let result: SystemsResult = serde_json::from_str(json).expect("parse");
        assert_eq!(result.systems.len(), 1);
        assert_eq!(result.systems[0].id, "NES");
        assert_eq!(result.systems[0].category, "Consoles");
    }

    #[test]
    fn system_info_category_defaults_to_empty_when_missing() {
        let json = r#"{"systems":[{"id":"x","name":"X"}]}"#;
        let result: SystemsResult = serde_json::from_str(json).expect("parse");
        assert_eq!(result.systems[0].category, "");
    }

    #[test]
    fn media_search_result_parses_nested_pagination() {
        let json = r#"{
            "results": [
                {
                    "name":"Game","path":"/p","zapScript":"s",
                    "system":{"id":"NES","name":"Nintendo","category":"Console","manufacturer":"Nintendo"},
                    "tags":[],
                    "relativePath":"smb.nes"
                }
            ],
            "total": -1,
            "pagination": {
                "hasNextPage": true,
                "pageSize": 100,
                "nextCursor": "abc"
            }
        }"#;
        let result: MediaSearchResult = serde_json::from_str(json).expect("parse");
        assert_eq!(result.results.len(), 1);
        assert!(result.has_next_page());
        assert_eq!(result.next_cursor().as_deref(), Some("abc"));
        let pagination = result.pagination.as_ref().expect("pagination");
        assert_eq!(pagination.page_size, 100);
        assert_eq!(pagination.next_cursor.as_deref(), Some("abc"));
        // Core sends -1 when total is unknown/unbounded; the field is
        // signed so the sentinel survives round-trip.
        assert_eq!(result.total, -1);
        let item = &result.results[0];
        assert_eq!(item.system.id, "NES");
        assert_eq!(item.system.name, "Nintendo");
        assert_eq!(item.system.category, "Console");
        assert_eq!(item.system.manufacturer.as_deref(), Some("Nintendo"));
        assert!(item.system.release_date.is_none());
        assert_eq!(item.relative_path.as_deref(), Some("smb.nes"));
        assert_eq!(item.zap_script, "s");
    }

    #[test]
    fn media_search_result_defaults_pagination_when_missing() {
        let json = r#"{"results":[]}"#;
        let result: MediaSearchResult = serde_json::from_str(json).expect("parse");
        assert!(!result.has_next_page());
        assert!(result.next_cursor().is_none());
        assert!(result.pagination.is_none());
        assert_eq!(result.total, 0);
    }

    #[test]
    fn media_search_item_defaults_tags_when_missing() {
        let json =
            r#"{"results":[{"name":"G","path":"/p","zapScript":"s","system":{"id":"NES"}}]}"#;
        let result: MediaSearchResult = serde_json::from_str(json).expect("parse");
        assert!(result.results[0].tags.is_empty());
        assert!(result.results[0].disambiguating_tags.is_empty());
        assert!(result.results[0].relative_path.is_none());
    }

    #[test]
    fn media_search_item_parses_disambiguating_tags() {
        let json = r#"{"results":[{
            "name":"Sonic","path":"/p","zapScript":"s","system":{"id":"Genesis"},
            "tags":[{"tag":"us","type":"region"},{"tag":"1","type":"disc"}],
            "disambiguatingTags":[{"tag":"us","type":"region"},{"tag":"1","type":"disc"}]
        }]}"#;
        let result: MediaSearchResult = serde_json::from_str(json).expect("parse");
        let item = &result.results[0];
        assert_eq!(item.disambiguating_tags.len(), 2);
        assert_eq!(item.disambiguating_tags[0].tag, "us");
        assert_eq!(item.disambiguating_tags[0].tag_type, "region");
        assert_eq!(item.disambiguating_tags[1].tag_type, "disc");
    }

    #[test]
    fn browse_entry_parses_disambiguating_tags() {
        let json = r#"{
            "name":"Sonic","path":"/p","type":"media",
            "disambiguatingTags":[{"tag":"eu","type":"region"}]
        }"#;
        let entry: BrowseEntry = serde_json::from_str(json).expect("parse");
        assert_eq!(entry.disambiguating_tags.len(), 1);
        assert_eq!(entry.disambiguating_tags[0].tag, "eu");
        assert_eq!(entry.disambiguating_tags[0].tag_type, "region");
    }

    #[test]
    fn media_search_params_omits_unset_fields() {
        let params = MediaSearchParams::default();
        let json = serde_json::to_value(&params).expect("serialise");
        let object = json.as_object().expect("object");
        assert!(object.is_empty(), "expected empty object, got {object:?}");
    }

    #[test]
    fn media_search_params_serialises_full_surface() {
        let params = MediaSearchParams {
            query: Some("mario".into()),
            systems: vec!["SNES".into()],
            max_results: Some(50),
            cursor: Some("opaque".into()),
            tags: vec!["region:usa".into()],
            letter: Some("M".into()),
        };
        let json = serde_json::to_value(&params).expect("serialise");
        let object = json.as_object().expect("object");
        assert_eq!(object.get("query").and_then(|v| v.as_str()), Some("mario"));
        assert_eq!(
            object.get("maxResults").and_then(serde_json::Value::as_u64),
            Some(50)
        );
        assert_eq!(
            object.get("cursor").and_then(|v| v.as_str()),
            Some("opaque")
        );
        assert_eq!(object.get("letter").and_then(|v| v.as_str()), Some("M"));
        assert_eq!(
            object.get("tags").and_then(|v| v.as_array()).map(Vec::len),
            Some(1)
        );
        assert!(!object.contains_key("fuzzySystem"));
    }

    #[test]
    fn readers_result_deserialises_current_shape() {
        let json = r#"{
            "readers": [
                {
                    "id": "/dev/ttyUSB0",
                    "readerId": "pn532-ujqixjv6",
                    "driver": "pn532",
                    "info": "PN532 (1-2.3.1)",
                    "capabilities": ["read", "write"],
                    "connected": true
                }
            ]
        }"#;
        let result: ReadersResult = serde_json::from_str(json).expect("parse");
        assert_eq!(result.readers.len(), 1);
        assert_eq!(result.readers[0].id, "/dev/ttyUSB0");
        assert_eq!(result.readers[0].reader_id, "pn532-ujqixjv6");
        assert!(result.readers[0].is_nfc_reader());
    }

    #[test]
    fn nfc_reader_detection_requires_connection() {
        let connected = ReaderInfo {
            driver: "acr122usb".into(),
            connected: true,
            ..ReaderInfo::default()
        };
        let disconnected = ReaderInfo {
            driver: "pn532".into(),
            connected: false,
            ..ReaderInfo::default()
        };
        assert!(connected.is_nfc_reader());
        assert!(!disconnected.is_nfc_reader());
    }

    #[test]
    fn media_browse_result_parses_envelope_and_pagination() {
        let json = r#"{
            "path": "/games",
            "entries": [
                {"name":"NES","path":"/games/NES","type":"directory","fileCount":42},
                {"name":"SMB","path":"/games/NES/smb.nes","type":"media","systemId":"NES","zapScript":"@NES/smb","relativePath":"NES/smb.nes","description":"A platformer."}
            ],
            "totalFiles": 150,
            "pagination": {"hasNextPage": true, "pageSize": 100, "nextCursor": "x"}
        }"#;
        let result: MediaBrowseResult = serde_json::from_str(json).expect("parse");
        assert_eq!(result.path, "/games");
        assert_eq!(result.entries.len(), 2);
        assert!(result.entries[0].is_folder());
        assert!(!result.entries[1].is_folder());
        assert_eq!(result.entries[1].system_id, "NES");
        assert_eq!(result.entries[1].relative_path, "NES/smb.nes");
        assert_eq!(result.entries[1].description, "A platformer.");
        assert_eq!(result.total_files, 150);
        let pagination = result.pagination.expect("pagination present");
        assert!(pagination.has_next_page);
        assert_eq!(pagination.next_cursor.as_deref(), Some("x"));
    }

    #[test]
    fn media_browse_index_result_parses_scheme_and_groups() {
        let json = r##"{
            "scheme": "latin",
            "totalFiles": 150,
            "groups": [
                {"key":"#","label":"#","count":3,"cursor":"","offset":0},
                {"key":"A","label":"A","count":12,"cursor":"opaqueA","offset":10}
            ]
        }"##;
        let result: MediaBrowseIndexResult = serde_json::from_str(json).expect("parse");
        assert_eq!(result.scheme, "latin");
        assert_eq!(result.total_files, 150);
        assert_eq!(result.groups.len(), 2);
        // The list-leading bucket has an empty cursor (browse from the top).
        assert_eq!(result.groups[0].key, "#");
        assert_eq!(result.groups[0].cursor, "");
        assert_eq!(result.groups[0].offset, 0);
        assert_eq!(result.groups[1].key, "A");
        assert_eq!(result.groups[1].count, 12);
        assert_eq!(result.groups[1].cursor, "opaqueA");
        assert_eq!(result.groups[1].offset, 10);
    }

    #[test]
    fn media_browse_index_groups_json_emits_camel_case_objects() {
        let result = MediaBrowseIndexResult {
            scheme: "latin".into(),
            total_files: 4,
            groups: vec![BrowseIndexGroup {
                key: "A".into(),
                label: "A".into(),
                count: 4,
                cursor: "opaqueA".into(),
                offset: 10,
            }],
        };
        // QML JSON.parse consumes this string; keys must match what the grid
        // reads (key/label/count/cursor/offset).
        let parsed: serde_json::Value =
            serde_json::from_str(&result.groups_json()).expect("parse json");
        let first = &parsed.as_array().expect("array")[0];
        assert_eq!(first.get("key").and_then(|v| v.as_str()), Some("A"));
        assert_eq!(first.get("label").and_then(|v| v.as_str()), Some("A"));
        assert_eq!(
            first.get("count").and_then(serde_json::Value::as_u64),
            Some(4)
        );
        assert_eq!(
            first.get("cursor").and_then(|v| v.as_str()),
            Some("opaqueA")
        );
        assert_eq!(
            first.get("offset").and_then(serde_json::Value::as_u64),
            Some(10)
        );
    }

    #[test]
    fn media_browse_index_params_systems_only_omits_path_and_sort() {
        let params = MediaBrowseIndexParams {
            systems: vec!["SNES".into()],
            ..MediaBrowseIndexParams::default()
        };
        let json = serde_json::to_value(&params).expect("serialise");
        let object = json.as_object().expect("object");
        assert!(!object.contains_key("path"));
        assert!(!object.contains_key("sort"));
        assert_eq!(
            object
                .get("systems")
                .and_then(|v| v.as_array())
                .map(Vec::len),
            Some(1)
        );
    }

    #[test]
    fn media_browse_params_systems_only_omits_path_and_cursor() {
        let params = MediaBrowseParams {
            systems: vec!["SNES".into()],
            max_results: Some(100),
            ..MediaBrowseParams::default()
        };
        let json = serde_json::to_value(&params).expect("serialise");
        let object = json.as_object().expect("object");
        assert!(!object.contains_key("path"));
        assert!(!object.contains_key("cursor"));
        assert_eq!(
            object
                .get("systems")
                .and_then(|v| v.as_array())
                .map(Vec::len),
            Some(1)
        );
        assert_eq!(
            object.get("maxResults").and_then(serde_json::Value::as_u64),
            Some(100)
        );
    }

    #[test]
    fn media_browse_params_path_systems_cursor_round_trip() {
        let params = MediaBrowseParams {
            path: "/roms/SNES".into(),
            systems: vec!["SNES".into()],
            max_results: Some(100),
            cursor: Some("opaque".into()),
            letter: Some("M".into()),
            sort: Some("name-asc".into()),
        };
        let json = serde_json::to_value(&params).expect("serialise");
        let object = json.as_object().expect("object");
        assert_eq!(
            object.get("path").and_then(|v| v.as_str()),
            Some("/roms/SNES")
        );
        assert_eq!(
            object.get("cursor").and_then(|v| v.as_str()),
            Some("opaque")
        );
        assert_eq!(object.get("letter").and_then(|v| v.as_str()), Some("M"));
        assert_eq!(
            object.get("sort").and_then(|v| v.as_str()),
            Some("name-asc")
        );
        assert_eq!(
            object
                .get("systems")
                .and_then(|v| v.as_array())
                .map(Vec::len),
            Some(1)
        );
        assert!(!object.contains_key("fuzzySystem"));
    }

    #[test]
    fn browse_entry_parses_system_id_and_system_ids() {
        let json = r#"{
            "name":"SNES","path":"/roms/SNES","type":"root","fileCount":12,
            "systemId":"SNES","systemIds":["SNES"]
        }"#;
        let entry: BrowseEntry = serde_json::from_str(json).expect("parse");
        assert_eq!(entry.system_id, "SNES");
        assert_eq!(entry.system_ids, vec!["SNES".to_string()]);
    }

    #[test]
    fn browse_entry_parses_system_ids_only_for_multi_system_route() {
        let json = r#"{
            "name":"shared","path":"/roms/shared","type":"root","fileCount":42,
            "systemIds":["SNES","NES"]
        }"#;
        let entry: BrowseEntry = serde_json::from_str(json).expect("parse");
        assert_eq!(entry.system_id, "");
        assert_eq!(
            entry.system_ids,
            vec!["SNES".to_string(), "NES".to_string()]
        );
    }

    #[test]
    fn media_browse_result_omits_pagination_when_no_files() {
        let json = r#"{"entries":[{"name":"/","path":"","type":"root","fileCount":0}]}"#;
        let result: MediaBrowseResult = serde_json::from_str(json).expect("parse");
        assert!(result.pagination.is_none());
        assert_eq!(result.path, "");
        assert_eq!(result.total_files, 0);
        assert!(result.entries[0].is_folder());
    }

    #[test]
    fn media_history_result_parses_documented_payload() {
        let json = r#"{
            "entries": [
                {
                    "systemId": "SNES",
                    "systemName": "Super Nintendo Entertainment System",
                    "mediaName": "Super Mario World",
                    "mediaPath": "/roms/snes/Super Mario World (USA).sfc",
                    "launcherId": "SNES",
                    "startedAt": "2025-01-22T14:30:00Z",
                    "endedAt": "2025-01-22T15:15:30Z",
                    "playTime": 2730
                }
            ],
            "pagination": {"hasNextPage": false, "pageSize": 10}
        }"#;
        let result: MediaHistoryResult = serde_json::from_str(json).expect("parse");
        assert_eq!(result.entries.len(), 1);
        let entry = &result.entries[0];
        assert_eq!(entry.system_id, "SNES");
        assert_eq!(entry.media_name, "Super Mario World");
        assert_eq!(entry.media_path, "/roms/snes/Super Mario World (USA).sfc");
        assert_eq!(entry.launcher_id, "SNES");
        assert_eq!(entry.play_time, 2730);
        assert!(!result.has_next_page());
        assert!(result.next_cursor().is_none());
    }

    #[test]
    fn media_history_result_handles_empty_envelope() {
        let result: MediaHistoryResult = serde_json::from_str("{}").expect("parse");
        assert!(result.entries.is_empty());
        assert!(result.pagination.is_none());
    }

    #[test]
    fn media_history_latest_result_parses_documented_payload() {
        let json = r#"{
            "entry": {
                "systemId": "SNES",
                "systemName": "Super Nintendo Entertainment System",
                "mediaName": "Super Mario World",
                "mediaPath": "/roms/snes/Super Mario World (USA).sfc",
                "launcherId": "SNES",
                "startedAt": "2025-01-22T14:30:00Z"
            }
        }"#;
        let result: MediaHistoryLatestResult = serde_json::from_str(json).expect("parse");
        let entry = result.entry.expect("entry");
        assert_eq!(entry.system_id, "SNES");
        assert_eq!(entry.media_name, "Super Mario World");
        assert_eq!(entry.media_path, "/roms/snes/Super Mario World (USA).sfc");
        assert_eq!(entry.launcher_id, "SNES");
        assert_eq!(entry.started_at, "2025-01-22T14:30:00Z");
    }

    #[test]
    fn media_history_latest_result_handles_no_entry() {
        let result: MediaHistoryLatestResult =
            serde_json::from_str(r#"{"entry":null}"#).expect("parse");
        assert!(result.entry.is_none());
        let result: MediaHistoryLatestResult = serde_json::from_str("{}").expect("parse");
        assert!(result.entry.is_none());
    }

    #[test]
    fn tag_info_parses_label() {
        let tag: TagInfo =
            serde_json::from_str(r#"{"type":"developer","tag":"nintendo","label":"Nintendo"}"#)
                .expect("parse");
        assert_eq!(tag.tag_type, "developer");
        assert_eq!(tag.tag, "nintendo");
        assert_eq!(tag.label, "Nintendo");
    }

    #[test]
    fn media_history_params_omits_unset_fields() {
        let params = MediaHistoryParams::default();
        let json = serde_json::to_value(&params).expect("serialise");
        let object = json.as_object().expect("object");
        assert!(object.is_empty());
    }

    #[test]
    fn media_history_params_serialises_cursor_and_systems() {
        let params = MediaHistoryParams {
            limit: Some(50),
            cursor: Some("opaque".into()),
            systems: vec!["SNES".into()],
        };
        let json = serde_json::to_value(&params).expect("serialise");
        let object = json.as_object().expect("object");
        assert_eq!(
            object.get("limit").and_then(serde_json::Value::as_u64),
            Some(50)
        );
        assert_eq!(
            object.get("cursor").and_then(|v| v.as_str()),
            Some("opaque")
        );
        assert_eq!(
            object
                .get("systems")
                .and_then(|v| v.as_array())
                .map(Vec::len),
            Some(1)
        );
        assert!(!object.contains_key("fuzzySystem"));
    }

    #[test]
    fn media_image_params_for_media_serialises_pair_only() {
        let params = MediaImageParams::for_media("SNES", "/roms/snes/Super Mario World.sfc");
        let json = serde_json::to_value(&params).expect("serialise");
        let object = json.as_object().expect("object");
        assert_eq!(object.get("system").and_then(|v| v.as_str()), Some("SNES"));
        assert_eq!(
            object.get("path").and_then(|v| v.as_str()),
            Some("/roms/snes/Super Mario World.sfc"),
        );
        assert!(!object.contains_key("imageTypes"));
        assert!(!object.contains_key("mediaId"));
    }

    #[test]
    fn media_image_params_emits_image_types_when_set() {
        let params = MediaImageParams {
            image_types: vec!["boxart".into(), "image".into()],
            ..MediaImageParams::for_media("SNES", "/p")
        };
        let json = serde_json::to_value(&params).expect("serialise");
        let object = json.as_object().expect("object");
        let arr = object
            .get("imageTypes")
            .and_then(|v| v.as_array())
            .expect("imageTypes array");
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0].as_str(), Some("boxart"));
    }

    #[test]
    fn media_image_params_for_media_id_serialises_id_only() {
        let params = MediaImageParams {
            image_types: vec!["boxart".into(), "image".into()],
            ..MediaImageParams::for_media_id(42)
        };
        let json = serde_json::to_value(&params).expect("serialise");
        let object = json.as_object().expect("object");
        assert_eq!(
            object.get("mediaId").and_then(serde_json::Value::as_i64),
            Some(42),
        );
        assert!(!object.contains_key("system"));
        assert!(!object.contains_key("path"));
        assert_eq!(
            object
                .get("imageTypes")
                .and_then(|v| v.as_array())
                .map(Vec::len),
            Some(2),
        );
    }

    #[test]
    fn media_image_result_parses_extension_and_payload() {
        let json = r#"{
            "contentType":"image/png",
            "extension":"png",
            "data":"iVBORw0KGgo=",
            "typeTag":"property:image-boxart"
        }"#;
        let result: MediaImageResult = serde_json::from_str(json).expect("parse");
        assert_eq!(result.content_type, "image/png");
        assert_eq!(result.extension.as_deref(), Some("png"));
        assert_eq!(result.data, "iVBORw0KGgo=");
        assert_eq!(result.type_tag, "property:image-boxart");
    }

    #[test]
    fn media_image_result_extension_defaults_when_absent() {
        let json = r#"{"contentType":"image/png","data":"x","typeTag":"property:image"}"#;
        let result: MediaImageResult = serde_json::from_str(json).expect("parse");
        assert!(result.extension.is_none());
    }

    #[test]
    fn media_meta_params_serialises_pair() {
        let params = MediaMetaParams::for_media("SNES", "/roms/snes/x.sfc");
        let json = serde_json::to_value(&params).expect("serialise");
        let object = json.as_object().expect("object");
        assert_eq!(object.get("system").and_then(|v| v.as_str()), Some("SNES"));
        assert_eq!(
            object.get("path").and_then(|v| v.as_str()),
            Some("/roms/snes/x.sfc")
        );
        assert!(!object.contains_key("mediaId"));
    }

    #[test]
    fn media_meta_params_for_media_id_serialises_id_only() {
        let params = MediaMetaParams::for_media_id(42);
        let json = serde_json::to_value(&params).expect("serialise");
        let object = json.as_object().expect("object");
        assert_eq!(
            object.get("mediaId").and_then(serde_json::Value::as_i64),
            Some(42),
        );
        assert!(!object.contains_key("system"));
        assert!(!object.contains_key("path"));
    }

    #[test]
    fn media_meta_result_parses_documented_payload() {
        // Mirrors the documented example from
        // /home/callan/dev/zaparoo-core/docs/api/methods.md (media.meta
        // section). Properties cover both binary (boxart) and text
        // (description) variants so the extension/content_type plumbing
        // exercises both paths.
        let json = r#"{
            "media": {
                "path": "/roms/snes/Super Mario World.sfc",
                "parentDir": "/roms/snes",
                "isMissing": false,
                "tags": [{"type":"region","tag":"usa"}],
                "properties": {
                    "property:image-boxart": {
                        "text": "/scrape/smw.png",
                        "contentType": "image/png",
                        "extension": "png",
                        "blobSize": 1234
                    }
                },
                "title": {
                    "slug": "super mario world",
                    "name": "Super Mario World",
                    "slugLength": 17,
                    "slugWordCount": 3,
                    "system": {"id":"SNES"},
                    "tags": [{"type":"developer","tag":"Nintendo"}],
                    "properties": {
                        "property:description": {
                            "text": "A platformer.",
                            "contentType": ""
                        }
                    }
                }
            }
        }"#;
        let result: MediaMetaResult = serde_json::from_str(json).expect("parse");
        assert_eq!(result.media.path, "/roms/snes/Super Mario World.sfc");
        assert_eq!(result.media.parent_dir, "/roms/snes");
        assert!(!result.media.is_missing);
        assert_eq!(result.media.tags.len(), 1);
        let boxart = result
            .media
            .properties
            .get("property:image-boxart")
            .expect("boxart property");
        assert_eq!(boxart.content_type, "image/png");
        assert_eq!(boxart.extension.as_deref(), Some("png"));
        assert_eq!(boxart.blob_size, 1234);
        assert_eq!(result.media.title.slug, "super mario world");
        assert_eq!(result.media.title.slug_length, 17);
        assert_eq!(result.media.title.system.id, "SNES");
        let description = result
            .media
            .title
            .properties
            .get("property:description")
            .expect("description property");
        assert_eq!(description.text, "A platformer.");
        assert_eq!(description.blob_size, 0);
        assert!(description.extension.is_none());
    }

    #[test]
    fn media_meta_result_handles_empty_properties() {
        let json = r#"{
            "media": {
                "path": "/p", "parentDir": "/", "isMissing": false,
                "tags": [], "properties": {},
                "title": {
                    "slug": "x", "name": "X", "slugLength": 1,
                    "slugWordCount": 1, "system": {"id":"NES"},
                    "tags": [], "properties": {}
                }
            }
        }"#;
        let result: MediaMetaResult = serde_json::from_str(json).expect("parse");
        assert!(result.media.properties.is_empty());
        assert!(result.media.title.properties.is_empty());
    }

    #[test]
    fn media_lookup_params_omits_optional_fields_and_serialises_required() {
        let params = MediaLookupParams {
            system: "SNES".into(),
            name: "Super Mario World".into(),
        };
        let json = serde_json::to_value(&params).expect("serialise");
        let object = json.as_object().expect("object");
        assert_eq!(object.get("system").and_then(|v| v.as_str()), Some("SNES"));
        assert_eq!(
            object.get("name").and_then(|v| v.as_str()),
            Some("Super Mario World")
        );
        assert!(!object.contains_key("fuzzySystem"));
    }

    #[test]
    fn media_lookup_result_parses_match_payload() {
        let json = r#"{
            "match": {
                "name": "Super Mario World",
                "path": "/roms/snes/Super Mario World (USA).sfc",
                "zapScript": "@SNES/Super Mario World",
                "system": {"id":"SNES","name":"Super Nintendo","category":"Console"},
                "tags": [{"type":"region","tag":"usa"}],
                "relativePath": "Super Mario World (USA).sfc",
                "confidence": 0.97
            }
        }"#;
        let result: MediaLookupResult = serde_json::from_str(json).expect("parse");
        let m = result.match_.as_ref().expect("match present");
        assert_eq!(m.name, "Super Mario World");
        assert_eq!(m.path, "/roms/snes/Super Mario World (USA).sfc");
        assert_eq!(m.zap_script, "@SNES/Super Mario World");
        assert_eq!(m.system.id, "SNES");
        assert_eq!(m.system.name, "Super Nintendo");
        assert_eq!(m.tags.len(), 1);
        assert_eq!(
            m.relative_path.as_deref(),
            Some("Super Mario World (USA).sfc")
        );
        assert!((m.confidence - 0.97).abs() < f64::EPSILON);
    }

    #[test]
    fn media_lookup_result_treats_null_match_as_no_match() {
        // Core returns `{match: null}` for both `ErrNoMatch` and
        // `ErrLowConfidence` — neither raises a JSON-RPC error, so the
        // wrapper has to model "no match" as success-with-None.
        let result: MediaLookupResult = serde_json::from_str(r#"{"match": null}"#).expect("parse");
        assert!(result.match_.is_none());
    }

    #[test]
    fn media_history_top_params_omits_unset_fields() {
        let params = MediaHistoryTopParams::default();
        let json = serde_json::to_value(&params).expect("serialise");
        let object = json.as_object().expect("object");
        assert!(object.is_empty());
    }

    #[test]
    fn media_history_top_params_serialises_full_surface() {
        let params = MediaHistoryTopParams {
            systems: vec!["SNES".into()],
            since: Some("2025-01-01T00:00:00Z".into()),
            limit: Some(10),
        };
        let json = serde_json::to_value(&params).expect("serialise");
        let object = json.as_object().expect("object");
        assert_eq!(
            object.get("since").and_then(|v| v.as_str()),
            Some("2025-01-01T00:00:00Z")
        );
        assert_eq!(
            object.get("limit").and_then(serde_json::Value::as_u64),
            Some(10)
        );
        assert_eq!(
            object
                .get("systems")
                .and_then(|v| v.as_array())
                .map(Vec::len),
            Some(1)
        );
        assert!(!object.contains_key("fuzzySystem"));
    }

    #[test]
    fn media_history_top_result_parses_aggregate_payload() {
        let json = r#"{
            "entries": [
                {
                    "systemId": "SNES",
                    "systemName": "Super Nintendo",
                    "mediaName": "Super Mario World",
                    "mediaPath": "/roms/snes/smw.sfc",
                    "lastPlayedAt": "2026-04-30T12:00:00Z",
                    "totalPlayTime": 7200,
                    "sessionCount": 4
                }
            ]
        }"#;
        let result: MediaHistoryTopResult = serde_json::from_str(json).expect("parse");
        assert_eq!(result.entries.len(), 1);
        let e = &result.entries[0];
        assert_eq!(e.system_id, "SNES");
        assert_eq!(e.media_name, "Super Mario World");
        assert_eq!(e.last_played_at, "2026-04-30T12:00:00Z");
        assert_eq!(e.total_play_time, 7200);
        assert_eq!(e.session_count, 4);
    }

    #[test]
    fn media_history_top_result_handles_empty_envelope() {
        let result: MediaHistoryTopResult = serde_json::from_str("{}").expect("parse");
        assert!(result.entries.is_empty());
    }

    #[test]
    fn media_tags_params_omits_unset_systems() {
        let params = MediaTagsParams::default();
        let json = serde_json::to_value(&params).expect("serialise");
        let object = json.as_object().expect("object");
        assert!(object.is_empty());
        assert!(!object.contains_key("fuzzySystem"));
    }

    #[test]
    fn media_tags_params_serialises_systems() {
        let params = MediaTagsParams {
            systems: vec!["SNES".into(), "NES".into()],
        };
        let json = serde_json::to_value(&params).expect("serialise");
        let object = json.as_object().expect("object");
        assert_eq!(
            object
                .get("systems")
                .and_then(|v| v.as_array())
                .map(Vec::len),
            Some(2)
        );
        assert!(!object.contains_key("fuzzySystem"));
    }

    #[test]
    fn media_tags_result_parses_payload() {
        let json = r#"{
            "tags": [
                {"type":"region","tag":"usa"},
                {"type":"developer","tag":"Nintendo"}
            ]
        }"#;
        let result: MediaTagsResult = serde_json::from_str(json).expect("parse");
        assert_eq!(result.tags.len(), 2);
        assert_eq!(result.tags[0].tag_type, "region");
        assert_eq!(result.tags[0].tag, "usa");
    }

    #[test]
    fn media_tags_result_handles_empty_envelope() {
        let result: MediaTagsResult = serde_json::from_str("{}").expect("parse");
        assert!(result.tags.is_empty());
    }

    #[test]
    fn settings_result_parses_system_defaults() {
        let json = r#"{
            "systemDefaults": [
                {"system":"SNES","launcher":"snes9x","beforeExit":"echo bye"},
                {"system":"Genesis"}
            ]
        }"#;
        let result: SettingsResult = serde_json::from_str(json).expect("parse");
        assert_eq!(result.system_defaults.len(), 2);
        assert_eq!(result.system_defaults[0].system, "SNES");
        assert_eq!(result.system_defaults[0].launcher, "snes9x");
        assert_eq!(result.system_defaults[0].before_exit, "echo bye");
        assert_eq!(result.system_defaults[1].launcher, "");
    }

    #[test]
    fn log_download_result_parses_documented_example() {
        let json = r#"{
            "filename": "zaparoo.log",
            "size": 1024,
            "content": "MjAyNC0wOS0yNFQxNzowMDowMC4wMDBaIElORk8="
        }"#;

        let result: LogDownloadResult = serde_json::from_str(json).expect("parse");

        assert_eq!(result.filename, "zaparoo.log");
        assert_eq!(result.size, 1024);
        assert_eq!(result.content, "MjAyNC0wOS0yNFQxNzowMDowMC4wMDBaIElORk8=");
    }

    #[test]
    fn health_result_parses_documented_example() {
        let result: HealthResult = serde_json::from_str(r#"{"status":"ok"}"#).expect("parse");
        assert_eq!(result.status, "ok");
    }

    #[test]
    fn tokens_result_parses_documented_example() {
        let json = r#"{
            "active": [],
            "last": {
                "type": "",
                "uid": "",
                "text": "**launch.system:snes",
                "data": "",
                "scanTime": "2024-09-24T17:49:42.938167429+08:00"
            }
        }"#;

        let result: TokensResult = serde_json::from_str(json).expect("parse");

        assert!(result.active.is_empty());
        let last = result.last.expect("last token");
        assert_eq!(last.text, "**launch.system:snes");
        assert_eq!(last.scan_time, "2024-09-24T17:49:42.938167429+08:00");
    }

    #[test]
    fn tokens_history_result_parses_documented_example() {
        let json = r#"{
            "entries": [
                {
                    "data": "",
                    "success": true,
                    "text": "**launch.system:snes",
                    "time": "2024-09-24T17:49:42.938167429+08:00",
                    "type": "",
                    "uid": ""
                }
            ]
        }"#;

        let result: TokensHistoryResult = serde_json::from_str(json).expect("parse");

        assert_eq!(result.entries.len(), 1);
        assert!(result.entries[0].success);
        assert_eq!(result.entries[0].text, "**launch.system:snes");
    }

    #[test]
    fn update_settings_params_serialises_system_defaults() {
        let params = UpdateSettingsParams {
            system_defaults: Some(vec![SystemDefault {
                system: "SNES".into(),
                launcher: "snes9x".into(),
                before_exit: String::new(),
            }]),
        };
        let json = serde_json::to_value(&params).expect("serialise");
        let defaults = json
            .get("systemDefaults")
            .and_then(|v| v.as_array())
            .expect("defaults array");
        assert_eq!(defaults.len(), 1);
        assert_eq!(
            defaults[0].get("system").and_then(|v| v.as_str()),
            Some("SNES")
        );
        assert_eq!(
            defaults[0].get("launcher").and_then(|v| v.as_str()),
            Some("snes9x")
        );
        assert!(!defaults[0]
            .as_object()
            .expect("object")
            .contains_key("beforeExit"));
    }

    #[test]
    fn launchers_result_parses_payload() {
        let json = r#"{
            "launchers": [
                {"id":"snes9x","systemId":"SNES","systemName":"Super Nintendo","groups":["libretro"]}
            ]
        }"#;
        let result: LaunchersResult = serde_json::from_str(json).expect("parse");
        assert_eq!(result.launchers.len(), 1);
        assert_eq!(result.launchers[0].id, "snes9x");
        assert_eq!(result.launchers[0].system_id, "SNES");
        assert_eq!(result.launchers[0].groups, vec!["libretro"]);
    }

    #[test]
    fn version_result_parses_populated_payload() {
        let json = r#"{"version":"1.2.3","platform":"mister"}"#;
        let result: VersionResult = serde_json::from_str(json).expect("parse");
        assert_eq!(result.version, "1.2.3");
        assert_eq!(result.platform, "mister");
    }

    #[test]
    fn version_result_missing_fields_default_to_empty() {
        let result: VersionResult = serde_json::from_str("{}").expect("parse");
        assert_eq!(result.version, "");
        assert_eq!(result.platform, "");
    }

    #[test]
    fn media_index_params_defaults_to_empty_object() {
        let params = MediaIndexParams::default();
        let json = serde_json::to_value(&params).expect("serialise");
        let object = json.as_object().expect("object");
        assert!(object.is_empty());
        assert!(!object.contains_key("fuzzySystem"));
    }

    #[test]
    fn media_index_params_serialises_systems_when_set() {
        let params = MediaIndexParams {
            systems: Some(vec!["SNES".into(), "NES".into()]),
        };
        let json = serde_json::to_value(&params).expect("serialise");
        let object = json.as_object().expect("object");
        assert_eq!(
            object
                .get("systems")
                .and_then(|v| v.as_array())
                .map(Vec::len),
            Some(2)
        );
        assert!(!object.contains_key("fuzzySystem"));
    }

    #[test]
    fn media_scrape_params_serialises_required_scraper_id() {
        let params = MediaScrapeParams {
            scraper_id: "screenscraper".into(),
            ..MediaScrapeParams::default()
        };
        let json = serde_json::to_value(&params).expect("serialise");
        let object = json.as_object().expect("object");
        assert_eq!(
            object.get("scraperId").and_then(|v| v.as_str()),
            Some("screenscraper")
        );
        // `force` is not skipped; default is `false`.
        assert_eq!(
            object.get("force").and_then(serde_json::Value::as_bool),
            Some(false)
        );
        // `systems` is skipped when empty.
        assert!(!object.contains_key("systems"));
    }

    #[test]
    fn media_scrape_params_serialises_systems_and_force() {
        let params = MediaScrapeParams {
            scraper_id: "screenscraper".into(),
            systems: vec!["SNES".into()],
            force: true,
        };
        let json = serde_json::to_value(&params).expect("serialise");
        let object = json.as_object().expect("object");
        assert_eq!(
            object.get("force").and_then(serde_json::Value::as_bool),
            Some(true)
        );
        assert_eq!(
            object
                .get("systems")
                .and_then(|v| v.as_array())
                .map(Vec::len),
            Some(1)
        );
    }

    #[test]
    fn indexing_status_response_parses_running_payload() {
        let json = r#"{
            "totalSteps": 4,
            "currentStep": 2,
            "currentStepDisplay": "Indexing SNES",
            "totalFiles": 1234,
            "totalMedia": 567,
            "exists": true,
            "indexing": true,
            "optimizing": false,
            "paused": false
        }"#;
        let result: IndexingStatusResponse = serde_json::from_str(json).expect("parse");
        assert_eq!(result.total_steps, Some(4));
        assert_eq!(result.current_step, Some(2));
        assert_eq!(
            result.current_step_display.as_deref(),
            Some("Indexing SNES")
        );
        assert_eq!(result.total_files, Some(1234));
        assert_eq!(result.total_media, Some(567));
        assert!(result.exists);
        assert!(result.indexing);
        assert!(!result.optimizing);
        assert!(!result.paused);
    }

    #[test]
    fn indexing_status_response_handles_idle_payload() {
        // Core sends `*int omitempty` for the counters; an idle Core
        // (no build in flight) reports only the booleans.
        let json = r#"{"exists": true, "indexing": false, "optimizing": false, "paused": false}"#;
        let result: IndexingStatusResponse = serde_json::from_str(json).expect("parse");
        assert!(result.total_steps.is_none());
        assert!(result.current_step.is_none());
        assert!(result.current_step_display.is_none());
        assert!(result.total_files.is_none());
        assert!(result.total_media.is_none());
        assert!(result.exists);
    }

    #[test]
    fn scraping_status_response_parses_running_payload() {
        let json = r#"{
            "scraperId": "screenscraper",
            "systemId": "SNES",
            "state": "running",
            "totalSteps": 5,
            "currentStep": 2,
            "currentStepDisplay": "Super Nintendo",
            "currentSystem": {"systemId": "SNES", "systemName": "Super Nintendo", "processed": 12, "total": 200, "matched": 10, "skipped": 2},
            "processed": 12,
            "total": 200,
            "matched": 10,
            "skipped": 2,
            "totalScraped": 50,
            "force": true,
            "scraping": true,
            "done": false,
            "paused": false
        }"#;
        let result: ScrapingStatusResponse = serde_json::from_str(json).expect("parse");
        assert_eq!(result.scraper_id, "screenscraper");
        assert_eq!(result.system_id, "SNES");
        assert_eq!(result.state, "running");
        assert_eq!(result.total_steps, Some(5));
        assert_eq!(result.current_step, Some(2));
        assert_eq!(
            result.current_step_display.as_deref(),
            Some("Super Nintendo")
        );
        let current = result.current_system.as_ref().expect("current system");
        assert_eq!(current.system_id, "SNES");
        assert_eq!(current.system_name, "Super Nintendo");
        assert_eq!(current.processed, 12);
        assert_eq!(result.processed, 12);
        assert_eq!(result.total, 200);
        assert_eq!(result.matched, 10);
        assert_eq!(result.skipped, 2);
        assert_eq!(result.total_scraped, 50);
        assert_eq!(result.force, Some(true));
        assert!(result.scraping);
        assert!(!result.done);
    }

    #[test]
    fn media_result_parses_envelope_with_database_and_active() {
        let json = r#"{
            "database": {"exists": true, "indexing": false, "optimizing": false, "paused": false, "totalMedia": 42},
            "active": [
                {
                    "started": "2026-05-03T12:00:00Z",
                    "launcherId": "SNES",
                    "systemId": "SNES",
                    "systemName": "Super Nintendo",
                    "mediaPath": "/p",
                    "mediaName": "X",
                    "zapScript": "@SNES/X"
                }
            ]
        }"#;
        let result: MediaResult = serde_json::from_str(json).expect("parse");
        assert!(result.database.exists);
        assert_eq!(result.database.total_media, Some(42));
        assert_eq!(result.active.len(), 1);
        assert_eq!(result.active[0].system_id, "SNES");
        assert_eq!(result.active[0].zap_script, "@SNES/X");
    }

    #[test]
    fn scrapers_result_parses_payload() {
        let json = r#"{
            "scrapers": [
                {"id":"screenscraper","name":"ScreenScraper","supportedSystems":["SNES","NES"]},
                {"id":"local","name":"Local"}
            ]
        }"#;
        let result: ScrapersResult = serde_json::from_str(json).expect("parse");
        assert_eq!(result.scrapers.len(), 2);
        assert_eq!(result.scrapers[0].id, "screenscraper");
        assert_eq!(result.scrapers[0].supported_systems, vec!["SNES", "NES"]);
        assert!(result.scrapers[1].supported_systems.is_empty());
    }

    #[test]
    fn media_meta_properties_null_defaults_to_empty_maps() {
        let json = r#"{
            "media": {
                "properties": null,
                "title": {"properties": null}
            }
        }"#;
        let result: MediaMetaResult = serde_json::from_str(json).expect("parse");
        assert!(result.media.properties.is_empty());
        assert!(result.media.title.properties.is_empty());
    }

    #[test]
    fn browse_entry_has_cover_absent_defaults_to_true() {
        // Older Core builds don't send `hasCover`; the frontend must
        // still request covers for those entries.
        let json = r#"{"name":"Zelda","path":"/roms/NES/zelda.nes","type":"media"}"#;
        let entry: BrowseEntry = serde_json::from_str(json).expect("parse");
        assert!(entry.has_cover, "absent hasCover should default to true");
    }

    #[test]
    fn browse_entry_has_cover_false_is_honoured() {
        // Core sends hasCover=false when it has confirmed no image property
        // row for the entry. The frontend should skip the cover request.
        let json =
            r#"{"name":"Zelda","path":"/roms/NES/zelda.nes","type":"media","hasCover":false}"#;
        let entry: BrowseEntry = serde_json::from_str(json).expect("parse");
        assert!(!entry.has_cover, "hasCover=false must be preserved");
    }

    #[test]
    fn browse_entry_default_has_cover_true() {
        // BrowseEntry::default() must give has_cover=true so struct-update
        // syntax in tests/callers doesn't accidentally suppress cover requests.
        let entry = BrowseEntry::default();
        assert!(entry.has_cover, "Default::has_cover must be true");
    }
}
