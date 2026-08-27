// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

import QtQuick
import QtTest
import Zaparoo.Ui

// Coverage for Format.folderCountSuffix/rowSuffix -- the shared
// composition behind the folder/root item-count dim suffix (BrowseList
// row, grid Tile caption, footer ActiveLabel). Format.count() itself is
// already covered indirectly via tst_status_line.qml.
//
// The suffix is a bare locale-formatted number, not a "N item(s)" word
// phrase: this slot is shared with ScrollingCaption's short 2-4 char
// disambiguating tags ("US"/"EU"), which Text.ElideLeft's from the front
// when it overflows -- a translated word phrase reliably overflowed and
// truncated down to a bare "...tem(s)" on real hardware.
TestCase {
    id: testCase
    name: "UiFormat"
    when: windowShown

    function test_folder_count_suffix_hidden_when_count_is_zero_or_unknown(): void {
        // Core omits fileCount on a root it couldn't compute in time --
        // 0 must not read as "confirmed empty".
        compare(Format.folderCountSuffix("", 0), "");
        compare(Format.folderCountSuffix("", -1), "");
    }

    function test_folder_count_suffix_shows_the_count_alone_with_no_distinguisher(): void {
        compare(Format.folderCountSuffix("", 42), Format.count(42));
    }

    // A distinguisher only ever appears on a `root` row (disambiguating
    // same-named sibling roots, games.rs's `root_distinguishers`); a count
    // only ever appears on a `directory` row Core hasn't already collapsed
    // to a single playable item (a media-capable directory reports
    // fileCount 0 -- see games.rs's `FILE_COUNT_ROLE` data-arm comment).
    // Those two cases never overlap in practice, so the distinguisher wins
    // outright with no separator to compose -- folders don't carry tags and
    // games don't carry counts.
    function test_folder_count_suffix_prefers_the_distinguisher_over_the_count(): void {
        compare(Format.folderCountSuffix("fat", 18), "fat");
        compare(Format.folderCountSuffix("usb0", 0), "usb0");
    }

    function test_row_suffix_uses_tags_unchanged_for_media_rows(): void {
        compare(Format.rowSuffix("media", "US", 999), "US");
        compare(Format.rowSuffix("media", "", 999), "");
    }

    function test_row_suffix_uses_folder_count_for_directory_and_root_rows(): void {
        compare(Format.rowSuffix("directory", "", 5), Format.count(5));
        compare(Format.rowSuffix("root", "fat", 5), "fat");
    }

    function test_row_suffix_hidden_for_a_folder_with_no_known_count(): void {
        compare(Format.rowSuffix("directory", "", 0), "");
    }

    // A directory Core already collapsed to a single playable item (media_id
    // or a direct zap_script -- games.rs's `is_media_capable_entry`) reports
    // fileCount 0 at the source (`FILE_COUNT_ROLE`/`file_count_at`), the
    // same signal an uncomputed root count uses -- it renders as the game
    // everywhere else in the tile, so a "1" beside it read as a bug, not a
    // folder count.
    function test_row_suffix_suppresses_count_for_a_collapsed_single_item_folder(): void {
        compare(Format.rowSuffix("directory", "", 0), "");
    }
}
