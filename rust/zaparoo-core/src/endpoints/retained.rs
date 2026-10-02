//! Allocation accounting for inactive endpoint payloads. Exhaustive field
//! patterns force new fields to participate before an endpoint can compile.

use crate::media_types::{
    BrowseEntry, LauncherInfo, LaunchersResult, MediaBrowseResult, MediaHistoryEntry,
    MediaHistoryResult, MediaItem, MediaSearchResult, Pagination, System, SystemInfo,
    SystemsResult, TagInfo,
};
use crate::systems_catalog::CatalogData;

pub(super) trait HeapBytes {
    fn heap_bytes(&self) -> usize;
}

pub(super) fn bytes<T: HeapBytes>(value: &T) -> usize {
    size_of::<T>().saturating_add(value.heap_bytes())
}

fn allocation(capacity: usize) -> usize {
    // Account for retained capacity, not length, plus allocator bookkeeping.
    if capacity == 0 {
        0
    } else {
        capacity.saturating_add(32)
    }
}

impl HeapBytes for String {
    fn heap_bytes(&self) -> usize {
        allocation(self.capacity())
    }
}

impl<T: HeapBytes> HeapBytes for Vec<T> {
    fn heap_bytes(&self) -> usize {
        self.iter().fold(
            allocation(self.capacity().saturating_mul(size_of::<T>())),
            |sum, value| sum.saturating_add(value.heap_bytes()),
        )
    }
}

impl<T: HeapBytes> HeapBytes for Option<T> {
    fn heap_bytes(&self) -> usize {
        self.as_ref().map_or(0, HeapBytes::heap_bytes)
    }
}

macro_rules! scalar {
    ($($ty:ty),+) => {$(
        impl HeapBytes for $ty {
            fn heap_bytes(&self) -> usize { 0 }
        }
    )+};
}
scalar!(bool, u32, u64, i64);

macro_rules! fields {
    ($ty:ty { $($field:ident),+ $(,)? }) => {
        impl HeapBytes for $ty {
            fn heap_bytes(&self) -> usize {
                let Self { $($field),+ } = self;
                0usize$(.saturating_add($field.heap_bytes()))+
            }
        }
    };
}

fields!(TagInfo {
    tag,
    tag_type,
    label,
    count
});
fields!(Pagination {
    has_next_page,
    page_size,
    next_cursor
});
fields!(BrowseEntry {
    media_id,
    name,
    path,
    entry_type,
    file_count,
    system_id,
    system_ids,
    zap_script,
    relative_path,
    group,
    description,
    tags,
    disambiguating_tags,
    has_cover,
    cover_color,
});
fields!(MediaBrowseResult {
    path,
    entries,
    total_files,
    total_dirs,
    pagination
});
fields!(System {
    id,
    name,
    category,
    release_date,
    manufacturer,
    zap_script
});
fields!(SystemInfo {
    id,
    name,
    category,
    release_date,
    manufacturer,
    media_count,
    zap_script
});
fields!(CatalogData {
    systems,
    categories
});
fields!(SystemsResult { systems });
fields!(MediaItem {
    media_id,
    name,
    path,
    zap_script,
    system,
    tags,
    disambiguating_tags,
    relative_path,
    has_cover,
    cover_color,
});
fields!(MediaSearchResult {
    results,
    pagination,
    total
});
fields!(MediaHistoryEntry {
    media_id,
    system_id,
    system_name,
    media_name,
    media_path,
    tags,
    launcher_id,
    started_at,
    ended_at,
    play_time,
    has_cover,
    cover_color,
});
fields!(MediaHistoryResult {
    entries,
    pagination
});
fields!(LauncherInfo {
    id,
    system_id,
    system_name,
    groups,
    available,
    availability_reason,
    detected
});
fields!(LaunchersResult { launchers });

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn history_cache_accounts_for_current_user_tags() {
        let empty = MediaHistoryEntry::default();
        let tagged = MediaHistoryEntry {
            tags: vec![TagInfo {
                tag: "hidden".into(),
                tag_type: "user".into(),
                ..TagInfo::default()
            }],
            ..MediaHistoryEntry::default()
        };
        assert!(bytes(&tagged) > bytes(&empty));
    }

    #[test]
    fn counts_unused_capacity_nested_tags_and_optional_strings() {
        let mut page = MediaBrowseResult::default();
        page.entries.reserve(10);
        let empty = bytes(&page);
        assert!(empty >= size_of::<MediaBrowseResult>() + 10 * size_of::<BrowseEntry>());
        let mut description = String::with_capacity(4096);
        description.push('x');
        page.entries.push(BrowseEntry {
            description,
            tags: vec![TagInfo {
                label: "L".repeat(512),
                ..Default::default()
            }],
            system_ids: vec!["NES".into()],
            cover_color: Some("#ffffff".into()),
            ..Default::default()
        });
        assert!(bytes(&page) >= empty + 4096 + 512 + 3 + 7);
        assert!(bytes(&page.clone()) <= bytes(&page));
    }
}
