// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

//! The media list rules shared by Games, Favorites and Recents: row
//! titles and suffixes, root handling, the linear and list paging math,
//! the focused-detail debounce policy, the selection persist debounce and
//! the item menu. Ported from `MediaListScreen.qml`, `GamesScreen.qml`,
//! `BrowseList.qml`, `FocusedMediaDetailController.qml`, `Format.qml` and
//! `models/games.rs`.

use crate::layouts::{Axis, List};

/// Rows a detailed list shows at once (`GamesScreen._listPageSize`).
pub const LIST_VISIBLE_ROWS: usize = 10;
/// Portrait non-CRT lists show more rows along their long axis.
pub const TATE_LIST_VISIBLE_ROWS: usize = 16;
/// Rows fetched per chunk while held rapid scrolling or restoring.
pub const RAPID_FETCH_CHUNK: u32 = 300;
/// Ceiling for one jump-to-letter fetch (Core's `max_results` cap).
pub const JUMP_FETCH_CEILING: u32 = 1000;
/// The focused-detail load waits this long after the last move.
pub const DETAIL_DEBOUNCE_MS: u64 = 220;
/// Selection writes coalesce over this window during a held move.
pub const PERSIST_DEBOUNCE_MS: u64 = 250;
/// A second page flip within this window shows the rapid letter badge.
pub const RAPID_FLIP_WINDOW_MS: u64 = 600;
/// The badge clears this long after the last flip.
pub const RAPID_LETTER_HOLD_MS: u64 = 700;
/// Prefetch this many pages of covers past the visible page.
pub const COVER_PREFETCH_NEXT_PAGES: usize = 1;
/// And this many before it.
pub const COVER_PREFETCH_PREVIOUS_PAGES: usize = 1;

/// Visible rows for the detailed list (`GamesScreen._listPageSize`).
pub fn list_visible_rows(crt_native_path: bool, rotated: bool) -> usize {
    if !crt_native_path && rotated {
        TATE_LIST_VISIBLE_ROWS
    } else {
        LIST_VISIBLE_ROWS
    }
}

/// Core's browse entry kinds as the rules distinguish them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryType {
    Media,
    Directory,
    Root,
    Other,
}

impl EntryType {
    pub fn parse(raw: &str) -> Self {
        match raw {
            "media" => Self::Media,
            "directory" => Self::Directory,
            "root" => Self::Root,
            _ => Self::Other,
        }
    }

    /// Directories and system roots both browse.
    pub fn is_folder(self) -> bool {
        matches!(self, Self::Directory | Self::Root)
    }
}

/// `is_media_capable_entry`: media, or a directory that carries a media
/// identity or a zapscript (a folder or zip Core launches as one item).
pub fn is_media_capable(entry_type: EntryType, has_media_id: bool, zap_script: &str) -> bool {
    entry_type == EntryType::Media
        || (entry_type == EntryType::Directory && (has_media_id || !zap_script.is_empty()))
}

/// A `root` row addressing a real filesystem folder (one of the system's
/// configured game directories), as opposed to a virtual-scheme route.
pub fn is_filesystem_root(entry_type: EntryType, path: &str) -> bool {
    entry_type == EntryType::Root && !path.is_empty() && !path.contains("://")
}

/// A singleton media container: a directory with a media id launches like
/// a game and never browses.
pub fn is_singleton_container(entry_type: EntryType, has_media_id: bool) -> bool {
    entry_type == EntryType::Directory && has_media_id
}

/// Accept on this row browses into it (folders that are not media capable).
pub fn browses(entry_type: EntryType, has_media_id: bool, zap_script: &str) -> bool {
    entry_type.is_folder() && !is_media_capable(entry_type, has_media_id, zap_script)
}

/// The on-disk file name without its extension, else `name`.
pub fn file_stem_or_name(path: &str, name: &str) -> String {
    let file = path
        .trim_end_matches(['/', '\\'])
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or_default();
    let stem = file.rsplit_once('.').map_or(file, |(stem, _)| stem).trim();
    if stem.is_empty() {
        name.to_string()
    } else {
        stem.to_string()
    }
}

/// Core's cleaned title, falling back to the file stem when Core sent none.
pub fn display_title(name: &str, path: &str) -> String {
    if name.is_empty() {
        file_stem_or_name(path, name)
    } else {
        name.to_string()
    }
}

/// The row title honoring Show original filenames.
pub fn display_name(name: &str, path: &str, show_original_filenames: bool) -> String {
    if show_original_filenames {
        file_stem_or_name(path, name)
    } else {
        display_title(name, path)
    }
}

/// The last path component (`GamesScreen._folderNameForPath`).
pub fn folder_name_for_path(path: &str) -> String {
    let trimmed = path.trim_end_matches(['/', '\\']);
    trimmed
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or_default()
        .to_string()
}

/// Inside a navigated folder once the stack holds more than the root.
pub fn at_folder_level(path_stack_len: usize) -> bool {
    path_stack_len > 1
}

/// The screen title: the folder's own name below the system root, else
/// the system's display name (`GamesScreen.topStripTitleProvider`).
pub fn screen_title(path_stack_len: usize, current_path: &str, system_name: &str) -> String {
    if at_folder_level(path_stack_len) {
        let folder = folder_name_for_path(current_path);
        if !folder.is_empty() {
            return folder;
        }
    }
    system_name.to_string()
}

/// `Format.rowSuffix`: folders show their distinguisher, else their file
/// count; media rows show their disambiguating tags.
pub fn row_suffix(
    entry_type: EntryType,
    tags_display: &str,
    file_count: u32,
    count: &dyn Fn(u32) -> String,
) -> String {
    if entry_type.is_folder() {
        if !tags_display.is_empty() {
            return tags_display.to_string();
        }
        return if file_count > 0 {
            count(file_count)
        } else {
            String::new()
        };
    }
    tags_display.to_string()
}

/// The file count a folder row carries: media-capable directories never
/// show one (`GamesModel::file_count_at`).
pub fn effective_file_count(media_capable: bool, file_count: u32) -> u32 {
    if media_capable {
        0
    } else {
        file_count
    }
}

/// Per-entry distinguisher for a page of identically named system roots:
/// the first path component where the roots disagree. `roots` holds the
/// root path for root entries with a path and `None` for every other row.
pub fn root_distinguishers(roots: &[Option<&str>]) -> Vec<String> {
    let mut result = vec![String::new(); roots.len()];
    let indexed: Vec<(usize, Vec<&str>)> = roots
        .iter()
        .enumerate()
        .filter_map(|(i, root)| {
            root.map(|path| (i, path.split('/').filter(|s| !s.is_empty()).collect()))
        })
        .collect();
    if indexed.len() < 2 {
        return result;
    }
    let max_len = indexed.iter().map(|(_, c)| c.len()).max().unwrap_or(0);
    let Some(diverge_at) = (0..max_len).find(|&idx| {
        let first = indexed[0].1.get(idx);
        indexed.iter().any(|(_, c)| c.get(idx) != first)
    }) else {
        return result;
    };
    for (entry_idx, comps) in &indexed {
        if let Some(part) = comps.get(diverge_at) {
            result[*entry_idx] = (*part).to_string();
        }
    }
    result
}

fn is_strict_ancestor_path(parent: &str, child: &str) -> bool {
    if parent.is_empty() || child.is_empty() || parent == child {
        return false;
    }
    child
        .strip_prefix(parent)
        .is_some_and(|rest| rest.starts_with('/'))
}

/// Keep flags for a roots page: a root that is a strict ancestor of a
/// sibling root is dropped (Core surfaced the shared parent alongside the
/// per-system folders). Non-root and blank-path rows always stay.
pub fn dedup_roots_drop_ancestors(roots: &[Option<&str>]) -> Vec<bool> {
    let paths: Vec<&str> = roots
        .iter()
        .flatten()
        .map(|p| p.trim_end_matches('/'))
        .collect();
    roots
        .iter()
        .map(|root| {
            root.is_none_or(|path| {
                let candidate = path.trim_end_matches('/');
                !paths
                    .iter()
                    .any(|other| is_strict_ancestor_path(candidate, other))
            })
        })
        .collect()
}

/// A single-root system skips its one-entry roots level.
pub fn single_root_auto_nav(at_root: bool, entry_types: &[EntryType]) -> bool {
    at_root && entry_types.len() == 1 && entry_types[0] == EntryType::Root
}

/// `jump_fetch_limit`: the gap to the target plus one page, in whole
/// pages, clamped to the Core ceiling.
pub fn jump_fetch_limit(target_index: usize, count: usize, page_size: usize) -> u32 {
    let page = page_size.max(1);
    let gap = target_index.saturating_sub(count) + page;
    let pages = gap.div_ceil(page);
    u32::try_from((pages * page).clamp(page, JUMP_FETCH_CEILING as usize)).unwrap_or(u32::MAX)
}

/// A letter bucket's first item sits after every leading directory
/// (`GamesScreen.jumpToItem`).
pub fn jump_target(total_dirs: usize, item_offset: usize) -> usize {
    total_dirs + item_offset
}

/// The rapid-paging badge letter: the title's first character, upper-cased,
/// `#` for a blank title.
pub fn rapid_letter(title: &str) -> String {
    title
        .trim()
        .chars()
        .next()
        .map_or_else(|| "#".to_string(), |c| c.to_uppercase().collect())
}

/// A second flip within the window shows the badge.
pub fn rapid_flip(since_last_flip_ms: Option<u64>) -> bool {
    since_last_flip_ms.is_some_and(|ms| ms < RAPID_FLIP_WINDOW_MS)
}

/// The rows to fetch covers for around the visible page: the page, the
/// next pages, then the previous ones (`prefetch_around_plan`).
pub fn prefetch_rows(count: usize, page_size: usize, first_visible_row: usize) -> Vec<usize> {
    if count == 0 {
        return Vec::new();
    }
    let page_size = page_size.max(1);
    let first = first_visible_row.min(count - 1);
    let current_end = (first + page_size).min(count);
    let next_end = (current_end + page_size * COVER_PREFETCH_NEXT_PAGES).min(count);
    let previous_start = first.saturating_sub(page_size * COVER_PREFETCH_PREVIOUS_PAGES);
    (first..current_end)
        .chain(current_end..next_end)
        .chain(previous_start..first)
        .collect()
}

/// The screen numbers `list_geometry` needs beside the list profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ListFrame {
    pub screen_width: i32,
    pub screen_height: i32,
    pub header_bottom: i32,
    pub status_top_margin: i32,
    pub strip_height: i32,
    pub help_bar_height: i32,
    pub tier_240: bool,
    /// `Sizing.pctH(6)`, the part of the 240p card bottom margin the help
    /// bar already covers.
    pub safe_bottom_gap: i32,
    /// Rows the screen wants on screen (0 lets the row height decide).
    pub target_rows: usize,
    /// `Sizing.pctH(3)` and `Sizing.pctH(6)`.
    pub min_row_height: i32,
    pub default_row_height: i32,
}

/// The list card, its list and detail sections, and the row metrics
/// (MediaListScreen.qml's card anchors through `BrowseListDetailView`
/// and `BrowseList`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ListGeometry {
    pub card_x: i32,
    pub card_y: i32,
    pub card_width: i32,
    pub card_height: i32,
    pub list_x: i32,
    pub list_y: i32,
    pub list_width: i32,
    pub list_height: i32,
    pub detail_x: i32,
    pub detail_y: i32,
    pub detail_width: i32,
    pub detail_height: i32,
    pub row_height: i32,
    pub visible_rows: usize,
}

/// The rows that fit the list section (`BrowseList.visibleRowCount`).
pub fn list_visible_count(
    content_height: i32,
    row_height: i32,
    row_spacing: i32,
    target_rows: usize,
) -> usize {
    if target_rows > 0 {
        return target_rows;
    }
    let stride = (row_height + row_spacing).max(1);
    usize::try_from((content_height + row_spacing) / stride)
        .unwrap_or(0)
        .max(1)
}

pub fn list_geometry(list: &List, frame: &ListFrame) -> ListGeometry {
    let card_x = list.card_side_margin;
    let card_y =
        frame.header_bottom + frame.status_top_margin + frame.strip_height + list.card_top_margin;
    let bottom = if frame.tier_240 {
        frame.help_bar_height + list.card_bottom_margin - frame.safe_bottom_gap
    } else {
        list.card_bottom_margin
    };
    let card_width = (frame.screen_width - 2 * card_x).max(0);
    let card_height = (frame.screen_height - card_y - bottom).max(0);
    let total = (list.list_share + list.detail_share).max(1);
    let vertical = list.content_axis == Axis::Vertical;
    let extent = if vertical { card_height } else { card_width };
    let list_span =
        ((extent - list.divider_width) * list.list_share / total + list.divider_margin).max(0);
    let detail_span = (extent - list_span - list.divider_width).max(0);
    let (list_width, list_height, detail_x, detail_y, detail_width, detail_height) = if vertical {
        (
            card_width,
            list_span,
            0,
            list_span + list.divider_width,
            card_width,
            detail_span,
        )
    } else {
        (
            list_span,
            card_height,
            list_span + list.divider_width,
            0,
            detail_span,
            card_height,
        )
    };
    let content_height = (list_height - list.card_padding_top - list.card_padding_bottom).max(0);
    let row_height = list_row_height(
        list.row_height,
        content_height,
        list.row_spacing,
        frame.target_rows,
        frame.min_row_height,
        frame.default_row_height,
    );
    ListGeometry {
        card_x,
        card_y,
        card_width,
        card_height,
        list_x: 0,
        list_y: 0,
        list_width,
        list_height,
        detail_x,
        detail_y,
        detail_width,
        detail_height,
        row_height,
        visible_rows: list_visible_count(
            content_height,
            row_height,
            list.row_spacing,
            frame.target_rows,
        ),
    }
}

/// The detail table's fixed media rows (`detail_tags_from_tags`): each key
/// collects every tag whose type matches one of its aliases; empty rows
/// drop out. `tags` carries `(type, display value)` pairs.
pub fn detail_rows_from_tags(tags: &[(String, String)]) -> Vec<(&'static str, String)> {
    const ROWS: &[(&str, &[&str])] = &[
        ("year", &["year", "release date", "release_date"]),
        ("genre", &["genre", "gamegenre"]),
        ("players", &["players"]),
        ("developer", &["developer"]),
        ("publisher", &["publisher"]),
        ("rating", &["rating"]),
    ];
    ROWS.iter()
        .map(|(key, aliases)| {
            let value = tags
                .iter()
                .filter(|(tag_type, value)| {
                    aliases
                        .iter()
                        .any(|alias| tag_type.eq_ignore_ascii_case(alias))
                        && !value.trim().is_empty()
                })
                .map(|(_, value)| value.trim().to_string())
                .collect::<Vec<_>>()
                .join(", ");
            (*key, value)
        })
        .filter(|(_, value)| !value.is_empty())
        .collect()
}

/// Outcome of a linear (list) move.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinearMove {
    /// Land on `index`; `fetch` asks for the next page as well.
    To { index: usize, fetch: bool },
    /// Nothing to land on yet: fetch the next page.
    FetchMore,
    /// Stay put; `fetch` still asks for the next page.
    Stay { fetch: bool },
}

/// `GamesScreen._performLinearMove`: wrap at the ends, but past the last
/// loaded row with more coming, fetch instead of wrapping; near the loaded
/// edge, fetch alongside the move.
pub fn linear_move(current: usize, count: usize, delta: i64, has_more: bool) -> LinearMove {
    if count == 0 {
        return LinearMove::Stay { fetch: false };
    }
    let count_i = count as i64;
    let mut next = current as i64 + delta;
    if next < 0 {
        next = count_i - 1;
    } else if next >= count_i {
        if has_more {
            return LinearMove::FetchMore;
        }
        next = 0;
    }
    let next = next as usize;
    let near_end = next + 2 >= count;
    if next == current {
        return LinearMove::Stay { fetch: near_end };
    }
    LinearMove::To {
        index: next,
        fetch: near_end && has_more,
    }
}

/// The list keeps a screenful loaded past the selection
/// (`GamesScreen._prefetchListTail`).
pub fn list_tail_prefetch(
    index: usize,
    count: usize,
    visible_rows: usize,
    has_more: bool,
    loading_more: bool,
) -> bool {
    !loading_more && has_more && index + visible_rows >= count
}

/// One list page holds fewer rows than a screenful: fill it
/// (`GamesScreen._fillListPage`).
pub fn list_fill_page(count: usize, visible_rows: usize, has_more: bool) -> bool {
    count > 0 && count <= visible_rows && has_more
}

/// More rows exist beyond the loaded count (`GamesScreen._listHasMore`).
pub fn list_has_more(count: usize, known_total: Option<usize>, has_next_page: bool) -> bool {
    has_next_page || known_total.is_some_and(|total| total > count)
}

/// `BrowseList` row window: the selection sits on the center slot and the
/// window clamps to the loaded rows.
pub fn list_view_top(
    current: usize,
    count: usize,
    visible_rows: usize,
    center_slot: Option<usize>,
) -> usize {
    let visible = visible_rows.max(1);
    let center = center_slot.map_or((visible - 1) / 2, |slot| slot.min(visible - 1));
    let max_top = count.saturating_sub(visible);
    current.saturating_sub(center).min(max_top)
}

/// `BrowseList.rowHeight`: the profile's fixed height, else the rows that
/// fit the target count (never below `min_height`), else `default_height`.
pub fn list_row_height(
    profile_row_height: i32,
    content_height: i32,
    row_spacing: i32,
    target_rows: usize,
    min_height: i32,
    default_height: i32,
) -> i32 {
    if profile_row_height > 0 {
        return profile_row_height;
    }
    if target_rows == 0 {
        return default_height;
    }
    let rows = i32::try_from(target_rows).unwrap_or(i32::MAX);
    ((content_height - row_spacing * (rows - 1)) / rows).max(min_height)
}

/// The detailed list's page and item cues (`MediaListScreen._list*`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "one flag per chevron and page cue the strip binds"
)]
pub struct ListPaging {
    pub total_items: usize,
    pub total_pages: usize,
    pub current_page: usize,
    pub has_pages_above: bool,
    pub has_pages_below: bool,
    pub has_items_above: bool,
    pub has_items_below: bool,
}

pub fn list_paging(
    current: usize,
    count: usize,
    total_override: Option<usize>,
    total_known: bool,
    page_size: usize,
    has_more: bool,
) -> ListPaging {
    let page_size = page_size.max(1);
    let total_items = match total_override {
        Some(total) if total_known => total,
        _ => count,
    };
    let total_pages = total_items.div_ceil(page_size).max(1);
    let current_page = current / page_size;
    ListPaging {
        total_items,
        total_pages,
        current_page,
        has_pages_above: current_page > 0,
        has_pages_below: current_page + 1 < total_pages || has_more,
        has_items_above: current > 0,
        has_items_below: current + 1 < total_items || has_more,
    }
}

/// `MediaListScreen._state`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Loading,
    Error,
    Empty,
    Ready,
}

impl State {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Loading => "loading",
            Self::Error => "error",
            Self::Empty => "empty",
            Self::Ready => "",
        }
    }
}

pub fn screen_state(loading: bool, cue_visible: bool, has_error: bool, count: usize) -> State {
    if loading || cue_visible {
        State::Loading
    } else if has_error {
        State::Error
    } else if count == 0 {
        State::Empty
    } else {
        State::Ready
    }
}

/// The page menu opens on a ready list, or an empty one when the screen
/// opted in (a filter that matches nothing must stay clearable).
pub fn page_menu_allowed(state: State, enabled_when_empty: bool) -> bool {
    state == State::Ready || (enabled_when_empty && state == State::Empty)
}

/// Which list the menu belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Owner {
    Games,
    Favorites,
    Recents,
}

/// Everything `buildContextMenuEntries` reads for a media row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(
    clippy::struct_excessive_bools,
    reason = "one flag per gate the Qt menu builder reads"
)]
pub struct MenuInput {
    pub owner: Owner,
    pub entry_type: EntryType,
    pub media_capable: bool,
    pub pinnable_root: bool,
    pub has_nfc: bool,
    pub is_favorite: bool,
    pub is_arcade_system: bool,
    pub has_launchers: bool,
    pub media_busy: bool,
}

/// The menu ids in display order, empty when the row gets no menu.
pub fn context_entries(input: &MenuInput) -> Vec<&'static str> {
    let folder = input.entry_type.is_folder() && !input.media_capable;
    if input.owner != Owner::Recents {
        if input.entry_type == EntryType::Root && !input.media_capable && !input.pinnable_root {
            return Vec::new();
        }
        if folder {
            return if input.owner == Owner::Games {
                vec!["add_to_hub"]
            } else {
                Vec::new()
            };
        }
    } else if folder {
        return Vec::new();
    }
    let mut entries = vec!["more_info"];
    if input.owner != Owner::Recents {
        entries.push("toggle_favorite");
        if input.owner == Owner::Games && input.has_launchers {
            entries.push("change_launcher");
        }
    }
    if input.has_nfc {
        entries.push("write_card");
    }
    entries.push("qr_code");
    if input.is_arcade_system {
        entries.push("discover");
    }
    entries.push("add_to_hub");
    if !input.media_busy {
        entries.push("scrape_game");
    }
    entries
}

/// `GamesScreen.contextMenuEnabledAt`.
pub fn context_menu_enabled(entry_type: EntryType, media_capable: bool, path: &str) -> bool {
    media_capable || entry_type == EntryType::Directory || is_filesystem_root(entry_type, path)
}

/// Escape zapscript separators while leaving path text readable.
fn escape_zapscript_arg(value: &str) -> String {
    let mut escaped = String::with_capacity(value.len());
    for ch in value.chars() {
        if matches!(ch, '^' | '?' | ',' | '&' | '|') {
            escaped.push('^');
        }
        escaped.push(ch);
    }
    escaped
}

/// Core-backed random launch for the browse scope: the folder when inside
/// one (recursive), else the whole system; favorites-only adds the tag.
pub fn random_launch_text(
    current_path: &str,
    current_system_id: &str,
    favorites_only: bool,
) -> Option<String> {
    let scope = if !current_path.is_empty() {
        escape_zapscript_arg(current_path)
    } else if !current_system_id.is_empty() {
        escape_zapscript_arg(current_system_id)
    } else {
        return None;
    };
    let tags = if favorites_only {
        "?tags=user:favorite"
    } else {
        ""
    };
    Some(format!("**launch.random:{scope}{tags}"))
}

/// What a row's cover slot shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CoverState {
    /// A plain folder: the folder glyph.
    Folder,
    /// Art is cached: paint it.
    Art,
    /// A fetch may still land: stay blank.
    Pending,
    /// Core confirmed there is no cover: the file chip (or the system
    /// logo on the flat lists).
    Absent,
}

/// `cover_key_for`: folders get the glyph; otherwise cached art wins, a
/// confirmed miss shows the chip and everything else stays blank.
#[allow(
    clippy::fn_params_excessive_bools,
    reason = "the four cache and entry facts the Qt key policy reads"
)]
pub fn cover_state(
    entry_type: EntryType,
    media_capable: bool,
    has_cover: bool,
    cached: bool,
    negative: bool,
) -> CoverState {
    if !media_capable && entry_type.is_folder() {
        return CoverState::Folder;
    }
    if cached && has_cover {
        CoverState::Art
    } else if !has_cover || negative {
        CoverState::Absent
    } else {
        CoverState::Pending
    }
}

/// What the focused-detail controller asks its host to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetailStep {
    /// Drop the pane's transient rows.
    Clear,
    /// Show the row's local metadata at once.
    Peek(usize),
    /// Start (or restart) the debounce timer.
    Arm,
    /// Stop the debounce timer.
    Disarm,
}

/// `FocusedMediaDetailController`: peek immediately, load after the
/// debounce, never reload the same identity, clear on empty or rapid.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FocusedDetail {
    requested: String,
    pending: Option<(String, usize)>,
    peeked: String,
}

impl FocusedDetail {
    pub fn new() -> Self {
        Self::default()
    }

    fn reset(&mut self, clear: bool) -> Vec<DetailStep> {
        self.requested.clear();
        self.pending = None;
        self.peeked.clear();
        let mut steps = vec![DetailStep::Disarm];
        if clear {
            steps.push(DetailStep::Clear);
        }
        steps
    }

    /// The selection, count, layout or rapid state changed.
    pub fn schedule(
        &mut self,
        enabled: bool,
        rapid: bool,
        identity: &str,
        index: usize,
        force: bool,
    ) -> Vec<DetailStep> {
        if !enabled {
            return Vec::new();
        }
        if rapid || identity.is_empty() {
            return self.reset(true);
        }
        let mut steps = Vec::new();
        if self.peeked != identity {
            self.peeked = identity.to_string();
            steps.push(DetailStep::Peek(index));
        }
        if !force && identity == self.requested {
            return steps;
        }
        self.pending = Some((identity.to_string(), index));
        steps.push(DetailStep::Arm);
        steps
    }

    /// The controller was turned off; `clear` mirrors `clearOnDisable`.
    pub fn disable(&mut self, clear: bool) -> Vec<DetailStep> {
        self.reset(clear)
    }

    /// `clearTransient`: forget everything and reload the current row.
    pub fn clear_transient(
        &mut self,
        enabled: bool,
        rapid: bool,
        identity: &str,
        index: usize,
    ) -> Vec<DetailStep> {
        let mut steps = self.reset(true);
        if !rapid {
            steps.extend(self.schedule(enabled, rapid, identity, index, false));
        }
        steps
    }

    /// The debounce fired: the row to load, if the selection still matches.
    pub fn fire(&mut self, enabled: bool, identity_now: &str) -> Option<usize> {
        let (identity, index) = self.pending.take()?;
        if !enabled || identity.is_empty() || identity != identity_now {
            return None;
        }
        self.requested = identity;
        Some(index)
    }
}

/// The debounced selection persist (`GamesScreen`'s `persistDebounce`):
/// moves schedule a path, hold release and Accept flush it, a scope
/// replacement discards it so the outgoing folder's snap never lands in
/// the incoming level's slot.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SelectionPersist {
    pending: Option<String>,
    /// The router is restoring a selection: writes are bookkeeping.
    pub suppressed: bool,
    replacing: bool,
}

impl SelectionPersist {
    pub fn new() -> Self {
        Self::default()
    }

    /// A move landed on `path`; true when the write was scheduled.
    pub fn schedule(&mut self, path: &str) -> bool {
        if self.suppressed || self.replacing || path.is_empty() {
            return false;
        }
        self.pending = Some(path.to_string());
        true
    }

    pub fn is_pending(&self) -> bool {
        self.pending.is_some()
    }

    /// Drop the scheduled write without committing it.
    pub fn discard(&mut self) {
        self.pending = None;
    }

    /// Commit now: the path to write, if any.
    pub fn flush(&mut self) -> Option<String> {
        self.pending.take()
    }

    /// The model is about to replace its rows.
    pub fn begin_replacement(&mut self) {
        self.discard();
        self.replacing = true;
    }

    pub fn end_replacement(&mut self) {
        self.replacing = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn count(n: u32) -> String {
        n.to_string()
    }

    #[test]
    fn media_capable_rule() {
        assert!(is_media_capable(EntryType::Media, false, ""));
        assert!(is_media_capable(EntryType::Directory, true, ""));
        assert!(is_media_capable(EntryType::Directory, false, "**launch:x"));
        assert!(!is_media_capable(EntryType::Directory, false, ""));
        assert!(!is_media_capable(EntryType::Root, true, ""));
    }

    #[test]
    fn filesystem_root_excludes_virtual_schemes() {
        assert!(is_filesystem_root(EntryType::Root, "/media/fat/games/NES"));
        assert!(!is_filesystem_root(EntryType::Root, "mame-arcade://"));
        assert!(!is_filesystem_root(EntryType::Root, ""));
        assert!(!is_filesystem_root(EntryType::Directory, "/x"));
    }

    #[test]
    fn file_stem_handles_separators_and_blank_stems() {
        assert_eq!(
            file_stem_or_name("/g/Sonic (USA).md", "Sonic"),
            "Sonic (USA)"
        );
        assert_eq!(file_stem_or_name("C:\\g\\Sonic.md\\", "Sonic"), "Sonic");
        assert_eq!(file_stem_or_name("", "Sonic"), "Sonic");
        assert_eq!(file_stem_or_name("/g/.hidden", "Fallback"), "Fallback");
    }

    #[test]
    fn display_name_prefers_core_title_unless_original_filenames() {
        assert_eq!(
            display_name("Friendly Alias", "/g/InternalContainer.zip", false),
            "Friendly Alias"
        );
        assert_eq!(
            display_name("Friendly Alias", "/g/InternalContainer.zip", true),
            "InternalContainer"
        );
        assert_eq!(display_name("", "/g/D (Disc 1).chd", false), "D (Disc 1)");
    }

    #[test]
    fn screen_title_uses_the_folder_name_only_below_the_root() {
        assert_eq!(
            screen_title(1, "/media/fat/games/SMS", "Master System"),
            "Master System"
        );
        assert_eq!(
            screen_title(2, "/media/fat/games/SMS/RPGs/", "Master System"),
            "RPGs"
        );
        assert_eq!(screen_title(2, "", "Master System"), "Master System");
    }

    #[test]
    fn row_suffix_folds_folder_counts_and_ignores_them_for_media() {
        assert_eq!(row_suffix(EntryType::Directory, "", 5, &count), "5");
        assert_eq!(row_suffix(EntryType::Root, "usb0", 5, &count), "usb0");
        assert_eq!(row_suffix(EntryType::Directory, "", 0, &count), "");
        assert_eq!(row_suffix(EntryType::Media, "US", 5, &count), "US");
        assert_eq!(row_suffix(EntryType::Media, "", 5, &count), "");
        assert_eq!(effective_file_count(true, 5), 0);
        assert_eq!(effective_file_count(false, 5), 5);
    }

    #[test]
    fn root_distinguishers_finds_the_first_diverging_component() {
        let roots = [Some("/media/fat/games/NES"), Some("/media/usb0/games/NES")];
        assert_eq!(root_distinguishers(&roots), vec!["fat", "usb0"]);
    }

    #[test]
    fn root_distinguishers_uses_a_later_component_when_the_earlier_ones_match() {
        let roots = [Some("/media/fat/games/NES"), Some("/media/fat/games2/NES")];
        assert_eq!(root_distinguishers(&roots), vec!["games", "games2"]);
    }

    #[test]
    fn root_distinguishers_blank_for_a_single_root() {
        assert_eq!(
            root_distinguishers(&[Some("/media/fat/games/NES")]),
            vec![""]
        );
    }

    #[test]
    fn root_distinguishers_blank_when_every_root_is_identical() {
        let roots = [Some("/media/fat/games/NES"), Some("/media/fat/games/NES")];
        assert_eq!(root_distinguishers(&roots), vec!["", ""]);
    }

    #[test]
    fn root_distinguishers_ignores_non_root_and_blank_path_entries() {
        let roots = [
            None,
            Some("/media/fat/games/NES"),
            None,
            Some("/media/usb0/games/NES"),
        ];
        assert_eq!(root_distinguishers(&roots), vec!["", "fat", "", "usb0"]);
    }

    #[test]
    fn root_distinguishers_three_way_split() {
        let roots = [
            Some("/media/fat/games/NES"),
            Some("/media/usb0/games/NES"),
            Some("/media/usb1/games/NES"),
        ];
        assert_eq!(root_distinguishers(&roots), vec!["fat", "usb0", "usb1"]);
    }

    #[test]
    fn dedup_roots_drops_ancestor_root_when_sibling_is_descendant() {
        let roots = [
            Some("/media/fat/games/MegaDrive"),
            Some("/media/fat/games/Genesis"),
            Some("/media/fat/games"),
        ];
        assert_eq!(dedup_roots_drop_ancestors(&roots), vec![true, true, false]);
    }

    #[test]
    fn dedup_roots_passes_through_unrelated_roots() {
        let roots = [Some("/media/fat/games/NES"), Some("/media/usb0/games/NES")];
        assert_eq!(dedup_roots_drop_ancestors(&roots), vec![true, true]);
    }

    #[test]
    fn dedup_roots_ignores_non_root_entries_and_keeps_blank_paths() {
        let roots = [None, Some("/media/fat/games/NES/sub"), Some("")];
        assert_eq!(dedup_roots_drop_ancestors(&roots), vec![true, true, true]);
    }

    #[test]
    fn jump_fetch_limit_sizes_to_gap_plus_one_page_rounded_up() {
        assert_eq!(jump_fetch_limit(570, 113, 100), 600);
    }

    #[test]
    fn jump_fetch_limit_clamps_to_ceiling_for_far_targets() {
        assert_eq!(jump_fetch_limit(5000, 113, 100), JUMP_FETCH_CEILING);
    }

    #[test]
    fn jump_fetch_limit_floors_at_one_page_when_target_already_loaded() {
        assert_eq!(jump_fetch_limit(113, 113, 100), 100);
        assert_eq!(jump_fetch_limit(50, 113, 100), 100);
    }

    #[test]
    fn rapid_letter_upper_cases_and_falls_back_to_hash() {
        assert_eq!(rapid_letter("  sonic"), "S");
        assert_eq!(rapid_letter(""), "#");
        assert_eq!(rapid_letter("ßeta"), "SS");
        assert!(rapid_flip(Some(100)));
        assert!(!rapid_flip(Some(600)));
        assert!(!rapid_flip(None));
    }

    #[test]
    fn prefetch_rows_orders_current_next_then_previous() {
        assert_eq!(
            prefetch_rows(50, 4, 8),
            vec![8, 9, 10, 11, 12, 13, 14, 15, 4, 5, 6, 7]
        );
        assert_eq!(prefetch_rows(6, 4, 4), vec![4, 5, 0, 1, 2, 3]);
        assert!(prefetch_rows(0, 4, 0).is_empty());
    }

    fn frame() -> ListFrame {
        ListFrame {
            screen_width: 1280,
            screen_height: 720,
            header_bottom: 68,
            status_top_margin: 7,
            strip_height: 50,
            help_bar_height: 43,
            tier_240: false,
            safe_bottom_gap: 43,
            target_rows: 10,
            min_row_height: 22,
            default_row_height: 43,
        }
    }

    #[test]
    fn list_geometry_splits_the_card_by_shares_and_fits_the_target_rows() {
        let inputs = crate::sizing::Inputs {
            screen_width: 1280.0,
            screen_height: 720.0,
            crt_native_path: false,
            bitmap_type: false,
            swap_percentage_axes: false,
            interface_profile: crate::sizing::InterfaceProfile::Standard,
        };
        let profile = crate::layouts::profile(
            crate::layouts::ThemeId::Default,
            crate::layouts::View::GamesList,
            &inputs,
        );
        let crate::layouts::Body::List { list, .. } = profile.body else {
            unreachable!("games list resolves to a list body");
        };
        let g = list_geometry(&list, &frame());
        assert_eq!(g.card_x, list.card_side_margin);
        assert_eq!(g.card_y, 68 + 7 + 50 + list.card_top_margin);
        assert_eq!(g.card_width, 1280 - 2 * list.card_side_margin);
        assert_eq!(g.card_height, 720 - g.card_y - list.card_bottom_margin);
        assert_eq!(g.list_width, (g.card_width - 1) * 2 / 3);
        assert_eq!(g.detail_x, g.list_width + 1);
        assert_eq!(g.detail_width, g.card_width - g.list_width - 1);
        assert_eq!(g.visible_rows, 10);
        let content = g.list_height - list.card_padding_top - list.card_padding_bottom;
        assert_eq!(g.row_height, (content - 9 * list.row_spacing) / 10);
    }

    #[test]
    fn list_geometry_derives_the_row_count_without_a_target() {
        let inputs = crate::sizing::Inputs {
            screen_width: 1280.0,
            screen_height: 720.0,
            crt_native_path: false,
            bitmap_type: false,
            swap_percentage_axes: false,
            interface_profile: crate::sizing::InterfaceProfile::Standard,
        };
        let profile = crate::layouts::profile(
            crate::layouts::ThemeId::Default,
            crate::layouts::View::SystemsList,
            &inputs,
        );
        let crate::layouts::Body::List { list, .. } = profile.body else {
            unreachable!("systems list resolves to a list body");
        };
        let g = list_geometry(
            &list,
            &ListFrame {
                target_rows: 0,
                ..frame()
            },
        );
        assert_eq!(g.row_height, 43);
        let content = g.list_height - list.card_padding_top - list.card_padding_bottom;
        assert_eq!(
            i32::try_from(g.visible_rows).unwrap_or(0),
            (content + list.row_spacing) / (43 + list.row_spacing)
        );
        assert_eq!(list_visible_count(100, 43, 5, 0), 2);
        assert_eq!(list_visible_count(10, 43, 5, 0), 1);
    }

    #[test]
    fn detail_rows_collect_aliases_and_drop_empties() {
        let tags = vec![
            ("Year".to_string(), "1991".to_string()),
            ("release_date".to_string(), "1991-06-23".to_string()),
            ("genre".to_string(), " Platformer ".to_string()),
            ("players".to_string(), String::new()),
            ("cheevos".to_string(), "yes".to_string()),
        ];
        assert_eq!(
            detail_rows_from_tags(&tags),
            vec![
                ("year", "1991, 1991-06-23".to_string()),
                ("genre", "Platformer".to_string())
            ]
        );
        assert!(detail_rows_from_tags(&[]).is_empty());
    }

    #[test]
    fn linear_move_wraps_and_fetches_at_the_loaded_edge() {
        assert_eq!(
            linear_move(0, 30, -1, false),
            LinearMove::To {
                index: 29,
                fetch: false
            }
        );
        assert_eq!(
            linear_move(29, 30, 1, false),
            LinearMove::To {
                index: 0,
                fetch: false
            }
        );
        assert_eq!(linear_move(29, 30, 1, true), LinearMove::FetchMore);
        assert_eq!(
            linear_move(27, 30, 1, true),
            LinearMove::To {
                index: 28,
                fetch: true
            }
        );
        assert_eq!(
            linear_move(27, 30, 1, false),
            LinearMove::To {
                index: 28,
                fetch: false
            }
        );
        assert_eq!(
            linear_move(0, 0, 1, true),
            LinearMove::Stay { fetch: false }
        );
        assert_eq!(
            linear_move(0, 1, 1, false),
            LinearMove::Stay { fetch: true }
        );
        assert_eq!(
            linear_move(0, 30, 10, false),
            LinearMove::To {
                index: 10,
                fetch: false
            }
        );
    }

    #[test]
    fn list_tail_and_fill_rules() {
        assert!(list_tail_prefetch(25, 30, 10, true, false));
        assert!(!list_tail_prefetch(25, 30, 10, true, true));
        assert!(!list_tail_prefetch(10, 30, 10, true, false));
        assert!(list_fill_page(6, 10, true));
        assert!(!list_fill_page(11, 10, true));
        assert!(list_has_more(30, Some(31), false));
        assert!(!list_has_more(30, Some(30), false));
        assert!(list_has_more(30, None, true));
    }

    #[test]
    fn list_view_top_centers_and_clamps() {
        assert_eq!(list_view_top(0, 60, 10, None), 0);
        assert_eq!(list_view_top(5, 60, 10, None), 1);
        assert_eq!(list_view_top(59, 60, 10, None), 50);
        assert_eq!(list_view_top(5, 60, 10, Some(0)), 5);
        assert_eq!(list_view_top(3, 5, 10, None), 0);
    }

    #[test]
    fn list_row_height_prefers_profile_then_target_fit() {
        assert_eq!(list_row_height(12, 300, 2, 10, 20, 40), 12);
        assert_eq!(list_row_height(0, 300, 2, 10, 20, 40), 28);
        assert_eq!(list_row_height(0, 100, 2, 10, 20, 40), 20);
        assert_eq!(list_row_height(0, 300, 2, 0, 20, 40), 40);
    }

    #[test]
    fn list_paging_tracks_the_lists_own_page_size() {
        let paging = list_paging(0, 30, Some(30), true, 10, false);
        assert_eq!(paging.current_page, 0);
        assert!(!paging.has_pages_above);
        assert_eq!(paging.total_pages, 3);
        let paging = list_paging(10, 30, Some(30), true, 10, false);
        assert_eq!(paging.current_page, 1);
        assert!(paging.has_pages_above);
        assert!(paging.has_pages_below);
        let paging = list_paging(29, 30, None, false, 10, true);
        assert_eq!(paging.total_items, 30);
        assert!(paging.has_pages_below);
        assert!(paging.has_items_below);
        let paging = list_paging(29, 30, Some(200), false, 10, false);
        assert_eq!(paging.total_items, 30);
        assert!(!paging.has_items_below);
    }

    #[test]
    fn screen_state_and_page_menu_gate() {
        assert_eq!(screen_state(true, false, false, 3), State::Loading);
        assert_eq!(screen_state(false, true, false, 3), State::Loading);
        assert_eq!(screen_state(false, false, true, 3), State::Error);
        assert_eq!(screen_state(false, false, false, 0), State::Empty);
        assert_eq!(screen_state(false, false, false, 3), State::Ready);
        assert!(page_menu_allowed(State::Ready, false));
        assert!(!page_menu_allowed(State::Empty, false));
        assert!(page_menu_allowed(State::Empty, true));
        assert!(!page_menu_allowed(State::Loading, true));
    }

    fn menu(owner: Owner) -> MenuInput {
        MenuInput {
            owner,
            entry_type: EntryType::Media,
            media_capable: true,
            pinnable_root: false,
            has_nfc: false,
            is_favorite: false,
            is_arcade_system: false,
            has_launchers: false,
            media_busy: false,
        }
    }

    #[test]
    fn games_menu_orders_details_first_and_maintenance_last() {
        let input = MenuInput {
            has_nfc: true,
            has_launchers: true,
            is_arcade_system: true,
            ..menu(Owner::Games)
        };
        assert_eq!(
            context_entries(&input),
            vec![
                "more_info",
                "toggle_favorite",
                "change_launcher",
                "write_card",
                "qr_code",
                "discover",
                "add_to_hub",
                "scrape_game"
            ]
        );
        let busy = MenuInput {
            media_busy: true,
            ..menu(Owner::Games)
        };
        assert_eq!(
            context_entries(&busy),
            vec!["more_info", "toggle_favorite", "qr_code", "add_to_hub"]
        );
    }

    #[test]
    fn favorites_menu_has_no_launcher_override() {
        let input = MenuInput {
            has_launchers: true,
            ..menu(Owner::Favorites)
        };
        assert_eq!(
            context_entries(&input),
            vec![
                "more_info",
                "toggle_favorite",
                "qr_code",
                "add_to_hub",
                "scrape_game"
            ]
        );
    }

    #[test]
    fn recents_menu_has_no_favorite_toggle() {
        assert_eq!(
            context_entries(&menu(Owner::Recents)),
            vec!["more_info", "qr_code", "add_to_hub", "scrape_game"]
        );
    }

    #[test]
    fn folder_rows_get_only_add_to_hub_on_games() {
        let folder = MenuInput {
            entry_type: EntryType::Directory,
            media_capable: false,
            ..menu(Owner::Games)
        };
        assert_eq!(context_entries(&folder), vec!["add_to_hub"]);
        let virtual_root = MenuInput {
            entry_type: EntryType::Root,
            media_capable: false,
            ..menu(Owner::Games)
        };
        assert!(context_entries(&virtual_root).is_empty());
        let fs_root = MenuInput {
            pinnable_root: true,
            ..virtual_root
        };
        assert_eq!(context_entries(&fs_root), vec!["add_to_hub"]);
        let favorites_folder = MenuInput {
            owner: Owner::Favorites,
            ..folder
        };
        assert!(context_entries(&favorites_folder).is_empty());
        assert!(context_menu_enabled(EntryType::Directory, false, "/x"));
        assert!(context_menu_enabled(EntryType::Root, false, "/x"));
        assert!(!context_menu_enabled(EntryType::Root, false, "mame://"));
    }

    #[test]
    fn random_launch_scopes_to_folder_then_system() {
        assert_eq!(
            random_launch_text("/games/a,b", "NES", false).as_deref(),
            Some("**launch.random:/games/a^,b")
        );
        assert_eq!(
            random_launch_text("", "NES", true).as_deref(),
            Some("**launch.random:NES?tags=user:favorite")
        );
        assert_eq!(random_launch_text("", "", false), None);
    }

    #[test]
    fn cover_state_follows_the_qt_key_policy() {
        assert_eq!(
            cover_state(EntryType::Directory, false, true, false, false),
            CoverState::Folder
        );
        assert_eq!(
            cover_state(EntryType::Directory, true, true, false, false),
            CoverState::Pending
        );
        assert_eq!(
            cover_state(EntryType::Media, true, true, true, false),
            CoverState::Art
        );
        assert_eq!(
            cover_state(EntryType::Media, true, false, true, false),
            CoverState::Absent
        );
        assert_eq!(
            cover_state(EntryType::Media, true, true, false, true),
            CoverState::Absent
        );
        assert_eq!(
            cover_state(EntryType::Media, true, true, false, false),
            CoverState::Pending
        );
    }

    #[test]
    fn detail_loads_after_debounce_for_selected_identity() {
        let mut d = FocusedDetail::new();
        let steps = d.schedule(true, false, "NES\n/a", 0, false);
        assert_eq!(steps, vec![DetailStep::Peek(0), DetailStep::Arm]);
        assert_eq!(d.fire(true, "NES\n/a"), Some(0));
    }

    #[test]
    fn detail_count_change_with_same_identity_does_not_reload() {
        let mut d = FocusedDetail::new();
        d.schedule(true, false, "NES\n/a", 0, false);
        assert_eq!(d.fire(true, "NES\n/a"), Some(0));
        assert!(d.schedule(true, false, "NES\n/a", 0, false).is_empty());
        assert_eq!(d.fire(true, "NES\n/a"), None);
    }

    #[test]
    fn detail_index_change_loads_new_identity_once() {
        let mut d = FocusedDetail::new();
        d.schedule(true, false, "NES\n/a", 0, false);
        assert_eq!(d.fire(true, "NES\n/a"), Some(0));
        let steps = d.schedule(true, false, "NES\n/b", 1, false);
        assert_eq!(steps, vec![DetailStep::Peek(1), DetailStep::Arm]);
        assert_eq!(d.fire(true, "NES\n/b"), Some(1));
    }

    #[test]
    fn detail_empty_identity_clears_and_suppresses_load() {
        let mut d = FocusedDetail::new();
        let steps = d.schedule(true, false, "", 0, false);
        assert_eq!(steps, vec![DetailStep::Disarm, DetailStep::Clear]);
        assert_eq!(d.fire(true, ""), None);
    }

    #[test]
    fn detail_disable_clears_detail() {
        let mut d = FocusedDetail::new();
        d.schedule(true, false, "NES\n/a", 0, false);
        assert_eq!(d.disable(true), vec![DetailStep::Disarm, DetailStep::Clear]);
        assert!(d.schedule(false, false, "NES\n/a", 0, false).is_empty());
    }

    #[test]
    fn detail_clear_transient_reloads_same_identity() {
        let mut d = FocusedDetail::new();
        d.schedule(true, false, "NES\n/a", 0, false);
        assert_eq!(d.fire(true, "NES\n/a"), Some(0));
        let steps = d.clear_transient(true, false, "NES\n/a", 0);
        assert_eq!(
            steps,
            vec![
                DetailStep::Disarm,
                DetailStep::Clear,
                DetailStep::Peek(0),
                DetailStep::Arm
            ]
        );
        assert_eq!(d.fire(true, "NES\n/a"), Some(0));
    }

    #[test]
    fn detail_rapid_scroll_hides_detail_and_reloads_after_stop() {
        let mut d = FocusedDetail::new();
        d.schedule(true, false, "NES\n/a", 0, false);
        assert_eq!(d.fire(true, "NES\n/a"), Some(0));
        let steps = d.schedule(true, true, "NES\n/b", 1, false);
        assert_eq!(steps, vec![DetailStep::Disarm, DetailStep::Clear]);
        assert_eq!(d.fire(true, "NES\n/b"), None);
        let steps = d.schedule(true, false, "NES\n/b", 1, false);
        assert_eq!(steps, vec![DetailStep::Peek(1), DetailStep::Arm]);
        assert_eq!(d.fire(true, "NES\n/b"), Some(1));
    }

    #[test]
    fn detail_stale_fire_is_dropped() {
        let mut d = FocusedDetail::new();
        d.schedule(true, false, "NES\n/a", 0, false);
        assert_eq!(d.fire(true, "NES\n/b"), None);
    }

    #[test]
    fn persist_user_move_still_persists_its_row() {
        let mut p = SelectionPersist::new();
        assert!(p.schedule("/child/game3"));
        assert_eq!(p.flush().as_deref(), Some("/child/game3"));
        assert_eq!(p.flush(), None);
    }

    #[test]
    fn persist_model_replacement_does_not_persist_the_index_snap() {
        let mut p = SelectionPersist::new();
        assert!(p.schedule("/child/game5"));
        assert_eq!(p.flush().as_deref(), Some("/child/game5"));
        p.begin_replacement();
        assert!(!p.schedule("/child/game0"));
        assert!(!p.is_pending());
    }

    #[test]
    fn persist_model_replacement_discards_a_pending_write() {
        let mut p = SelectionPersist::new();
        p.schedule("/child/game2");
        p.begin_replacement();
        assert_eq!(p.flush(), None);
    }

    #[test]
    fn persist_resumes_after_the_replacement_settles() {
        let mut p = SelectionPersist::new();
        p.begin_replacement();
        p.end_replacement();
        assert!(p.schedule("/child/game4"));
        assert_eq!(p.flush().as_deref(), Some("/child/game4"));
    }

    #[test]
    fn persist_router_suppression_still_blocks_persist() {
        let mut p = SelectionPersist::new();
        p.suppressed = true;
        assert!(!p.schedule("/child/game6"));
        assert_eq!(p.flush(), None);
    }
}
