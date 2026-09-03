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

    // Metadata tag-label vocabulary, shared by every surface that renders a
    // game's or system's detail table: `BrowseDetailPane` (the list-view
    // sidebar) and `GameInfoModal` (the details modal).
    //
    // It lives here because the labels are user-visible strings and the Rust
    // side must not own them. `game_info.rs` used to emit its table with
    // title-cased English labels baked in (`display_label()`), so the whole
    // details table shipped untranslated regardless of the UI language --
    // CLAUDE.md requires every user-visible string to go through
    // `qsTr()`/`tr()`, and a label built in Rust cannot. The models now emit a
    // tag *type* and the label is chosen here.
    //
    // `_metadataKey` normalizes both spellings a producer might send: the
    // canonical type (`release_date`, straight off Core's tag) and the older
    // English label some models still emit (`Release date`). That is what
    // lets one vocabulary serve every producer without rewriting all four of
    // them in the same pass.
    function _metadataKey(label: string): string {
        return label.trim().toLowerCase().replace(/[ \-]/g, "_");
    }

    // Full label. Unknown types (Core passes through whatever a scraper
    // wrote) fall back to the producer's own string with its first letter
    // capitalized, so a novel tag still reads as a label rather than
    // vanishing.
    function metadataLabel(label: string): string {
        const key = root._metadataKey(label);
        if (key === "system")
            return qsTr("System");
        if (key === "platform")
            return qsTr("Platform");
        if (key === "category")
            return qsTr("Category");
        if (key === "year")
            return qsTr("Year");
        if (key === "release_date")
            return qsTr("Release date");
        if (key === "genre")
            return qsTr("Genre");
        if (key === "players")
            return qsTr("Players");
        if (key === "play_mode")
            return qsTr("Play mode");
        if (key === "cooperative")
            return qsTr("Cooperative");
        if (key === "developer")
            return qsTr("Developer");
        if (key === "publisher")
            return qsTr("Publisher");
        if (key === "manufacturer")
            return qsTr("Manufacturer");
        if (key === "rating")
            return qsTr("Rating");
        if (key === "filename")
            return qsTr("Filename");
        if (label === "")
            return "";
        return label.charAt(0).toUpperCase() + label.slice(1);
    }

    // Short form for narrow hosts. Empty when a type has no abbreviation, so
    // callers fall back to the full label rather than inventing one.
    function metadataShortLabel(label: string): string {
        const key = root._metadataKey(label);
        if (key === "system")
            return qsTr("Sys", "Short metadata label for System; keep 2-4 characters if possible");
        if (key === "platform")
            return qsTr("Plat", "Short metadata label for Platform; keep 2-4 characters if possible");
        if (key === "category")
            return qsTr("Cat", "Short metadata label for Category; keep 2-4 characters if possible");
        if (key === "year")
            return qsTr("Yr", "Short metadata label for Year; keep 2-4 characters if possible");
        if (key === "release_date")
            return qsTr("Date", "Short metadata label for Release date; keep 2-4 characters if possible");
        if (key === "genre")
            return qsTr("Gen", "Short metadata label for Genre; keep 2-4 characters if possible");
        if (key === "players")
            return qsTr("Plyr", "Short metadata label for Players; keep 2-4 characters if possible");
        if (key === "play_mode")
            return qsTr("Mode", "Short metadata label for Play mode; keep 2-4 characters if possible");
        if (key === "cooperative")
            return qsTr("Co-op", "Short metadata label for Cooperative; keep 2-5 characters if possible");
        if (key === "developer")
            return qsTr("Dev", "Short metadata label for Developer; keep 2-4 characters if possible");
        if (key === "publisher")
            return qsTr("Pub", "Short metadata label for Publisher; keep 2-4 characters if possible");
        if (key === "manufacturer")
            return qsTr("Mfr", "Short metadata label for Manufacturer; keep 2-4 characters if possible");
        if (key === "rating")
            return qsTr("Rtg", "Short metadata label for Rating; keep 2-4 characters if possible");
        if (key === "filename")
            return qsTr("File", "Short metadata label for Filename; keep 2-4 characters if possible");
        return "";
    }

    // Full label packed with its short form behind U+009C, Qt's alternative-
    // text separator: `Text` with `elide` set renders the short form instead
    // of eliding the long one when the box is too narrow. Types with no
    // abbreviation pack nothing, so they elide normally.
    function metadataElidableLabel(label: string): string {
        const full = root.metadataLabel(label);
        // Not `short`: it is a reserved word, and while the desktop QML
        // runtime tolerates it, the AOT `qmlcachegen` pass the static
        // MiSTer build uses rejects the file outright.
        const abbreviated = root.metadataShortLabel(label);
        return abbreviated === "" ? full : full + "\u009C" + abbreviated;
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
