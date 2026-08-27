// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
// cxx-qt 0.8 patches `isFinal: true` on singleton properties but the
// qmltypes schema has no `isFinal` slot, so every read of a Browse
// singleton trips qmllint's "Member can be shadowed" check. Suppress
// the compiler category file-wide until the schema grows the slot.
// qmllint disable compiler
pragma Singleton

import QtQuick
import Zaparoo.Browse as Browse

// Locale-aware number formatting for user-visible counts (media/scrape
// totals, file counts). Nothing in the app formatted numbers for locale
// before round 9 -- the raw i32 crossed the cxx-qt bridge and
// `qsTr("%1 indexed").arg(total)` stringified it plainly, so `100000`
// rendered as `100000` instead of a locale-grouped `100,000`.
//
// Lives in `Zaparoo.Ui`, not `Zaparoo.Theme` -- `Zaparoo.Theme` is
// deliberately dependency-free of `Zaparoo.Browse` (see Motion.qml's doc
// comment), but `locale()` needs `Browse.Settings.current_language` to
// follow the app's own language setting rather than the host OS locale.
// `HeaderBar.qml` (already in `Zaparoo.Ui`) established the same pattern
// for the clock; `locale()` here lifts that logic to one shared copy.
QtObject {
    id: root

    function locale(): var {
        const language = Browse.Settings.current_language;
        if (language === "" || language === "auto")
            return Qt.locale();
        return Qt.locale(language);
    }

    // Digit-grouped integer count, e.g. "100,000" (or the locale's own
    // grouping/digit convention). Precision pinned to 0 -- bare
    // `Number.toLocaleString(locale)` defaults to 2 decimal places, which
    // would render an integer count as "100,000.00".
    function count(n: int): string {
        return Number(n).toLocaleString(root.locale(), "f", 0);
    }

    // The dim suffix for a folder/root row -- the row's distinguisher (see
    // games.rs's `root_distinguishers`, which overlays a sibling-diffed
    // distinguisher onto the same `disambiguatingTags` channel a folder
    // normally leaves blank) when it has one, otherwise the bare item
    // count. Never both: a distinguisher only ever appears on a `root` row
    // (disambiguating same-named sibling roots), a count only ever appears
    // on a `directory` row Core hasn't already collapsed to a single
    // playable item (see `fileCount`'s own doc comment on `games.rs`'s
    // `FILE_COUNT_ROLE` -- a media-capable directory reports 0 here), and
    // those two cases don't overlap -- so no separator between them is
    // needed. No "item(s)" word on the count: this slot is shared with
    // ScrollingCaption's short 2-4 char disambiguating tags ("US"/"EU"),
    // which `Text.ElideLeft`s from the front when it overflows -- a
    // translated word phrase reliably overflowed and truncated down to a
    // bare "...tem(s)", while the number alone reads unambiguously since a
    // folder row never shows anything else in this slot. Hidden (empty
    // string) when `fileCount` is 0 -- Core omits the field on a root whose
    // exact count it couldn't compute in time, so 0 doesn't reliably mean
    // "empty," and showing "0" next to a folder that likely has content
    // would be actively misleading. One shared implementation for every
    // caller (BrowseList row, grid Tile caption, footer ActiveLabel).
    function folderCountSuffix(distinguisher: string, fileCount: int): string {
        if (distinguisher !== "")
            return distinguisher;
        return fileCount > 0 ? root.count(fileCount) : "";
    }

    // Chooses between a game row's own disambiguation tags and a folder/
    // root row's item-count suffix, keyed on the shared `entryType` role
    // every Browse list/grid model now publishes (games.rs, favorites.rs,
    // recents.rs, systems.rs, favorite_systems.rs).
    function rowSuffix(entryType: string, disambiguatingTags: string, fileCount: int): string {
        if (entryType === "directory" || entryType === "root")
            return root.folderCountSuffix(disambiguatingTags, fileCount);
        return disambiguatingTags;
    }
}
