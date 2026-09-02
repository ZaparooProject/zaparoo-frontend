// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

import QtQuick
import QtTest
import Zaparoo.Theme
import Zaparoo.Ui
import Zaparoo.Browse as Browse

// Round 10 coverage: the fixed tag-label column (replacing the old
// accumulate-forever `_labelColumnWidth`) and the scroll-chevron
// hide-when-not-scrollable rule (matching PageIndicator.qml's own
// single-page hide rule). Both are pure computed properties that don't
// depend on Browse.GameInfo's live RPC data, so they're testable without
// driving that singleton's `load()`.
TestCase {
    id: testCase
    name: "UiGameInfoModal"
    when: windowShown
    width: 640
    height: 480
    visible: true

    GameInfoModal {
        id: modal
        anchors.fill: parent
        open: true
    }

    function initTestCase(): void {
        Sizing.screenWidth = testCase.width;
        Sizing.screenHeight = testCase.height;
    }

    // Fixed against the known ordered tag *types* in game_info.rs's
    // `ORDERED_TAG_TYPES`, resolved to labels through Format — a pure
    // function of that fixed list and the body font metrics, not of
    // whatever tags the currently-loaded game happens to carry.
    function test_label_column_width_is_positive_and_fixed(): void {
        verify(modal._labelColumnWidth > 0);
        const before = modal._labelColumnWidth;
        // Re-reading must be stable — nothing about opening/closing or
        // the absence of live tag data should perturb a value derived
        // entirely from the fixed label set and the font.
        modal.open = false;
        modal.open = true;
        compare(modal._labelColumnWidth, before, "label column width must not depend on modal open/close state");
    }

    // Before any game is loaded, Browse.GameInfo starts idle (not
    // loading, no error, no tags, no cover) — the flickable has nothing
    // to scroll, so both round-10 chevrons must stay hidden entirely
    // rather than showing two permanently-dim arrows pointing at nothing.
    function test_scroll_chevrons_hidden_when_content_does_not_overflow(): void {
        compare(modal._scrollable, false, "an idle GameInfo with no tags/description/cover has nothing to scroll");
        const up = findChild(modal, "gameInfoScrollUp");
        const down = findChild(modal, "gameInfoScrollDown");
        verify(up !== null);
        verify(down !== null);
        compare(up.visible, false);
        compare(down.visible, false);
    }

    // Both chevrons hang OUTSIDE the flickable — the up one above
    // `flick.top`, the down one below `flick.bottom` — so each needs its own
    // height plus its margin of clear space on that side. The up chevron's gap
    // was `pctH(2)` against a `pctH(3.5)` requirement, so it drew through what
    // sat above the flickable whenever the body scrolled. Anchors resolve regardless of
    // `visible`, so this is checkable without driving Browse.GameInfo's live
    // data, matching how the rest of this file works.
    function test_scroll_chevrons_do_not_overlap_their_neighbours(): void {
        const up = findChild(modal, "gameInfoScrollUp");
        const down = findChild(modal, "gameInfoScrollDown");
        const title = findChild(modal, "gameInfoTitle");
        verify(up !== null);
        verify(down !== null);
        verify(title !== null, "title needs an objectName for this assertion");
        verify(up.height > 0, "chevron must have resolved geometry");
        verify(up.y >= title.y + title.height, "up chevron (" + up.y + ") must clear the title (" + (title.y + title.height) + ")");
        // Against the PANEL, not the modal: both chevrons are children of the
        // panel, so their `y` is panel-relative, while `modal.height` is the
        // full-screen height. Comparing to the latter would pass even for a
        // chevron hanging below the card.
        verify(down.y + down.height <= down.parent.height, "down chevron (" + (down.y + down.height) + ") must stay inside the card (" + down.parent.height + ")");
    }

    // Every label in the table has to come from Format, so it goes through
    // qsTr(). game_info.rs used to emit title-cased English labels itself,
    // which meant the whole details table shipped untranslated no matter
    // what the interface language was set to.
    function test_tag_labels_resolve_through_the_shared_vocabulary(): void {
        compare(Format.metadataLabel("release_date"), Format.metadataLabel("Release date"), "canonical type and legacy label must resolve to one label");
        compare(Format.metadataLabel("play-mode"), Format.metadataLabel("play_mode"), "dashes and underscores are the same key");
        verify(Format.metadataLabel("rating").length > 0);
        // An unknown passthrough type still reads as a label rather than
        // disappearing or rendering raw.
        compare(Format.metadataLabel("cheevos"), "Cheevos");
        compare(Format.metadataLabel(""), "");
    }

    // The column is measured off translated labels, so it has to track the
    // vocabulary rather than a hardcoded English list.
    function test_label_column_measures_the_translated_vocabulary(): void {
        let widest = 0;
        for (let i = 0; i < modal._knownTagTypes.length; i++)
            widest = Math.max(widest, Format.metadataLabel(modal._knownTagTypes[i]).length);
        verify(widest > 0, "the known vocabulary must resolve to non-empty labels");
        verify(modal._labelColumnWidth > 0);
    }

    // docs/style.md's card recipe describes a *selectable* surface. The
    // cover is neither selectable nor on a background needing contrast
    // help, and a full-width plate behind PreserveAspectFit art framed
    // every portrait cover in gutters that read as a border.
    function test_cover_slot_has_no_card_plate_or_border(): void {
        const slot = findChild(modal, "gameInfoCoverSlot");
        verify(slot !== null, "cover slot needs an objectName for this assertion");
        verify(!(slot instanceof Rectangle), "the cover slot must not be a filled/bordered card");
    }

    // Panel sizes to content, capped. It used to be pinned to
    // `parent.height - pctH(16)` regardless, so a game with a cover and a
    // few tags got ~84% of the screen as mostly empty panel while every
    // other modal in the app sizes to what it holds.
    function test_panel_height_tracks_content_under_a_cap(): void {
        const panel = findChild(modal, "gameInfoPanel");
        verify(panel !== null, "panel needs an objectName for this assertion");
        verify(panel.height > 0);
        verify(panel.height <= panel._maxHeight, "panel must never exceed its cap");
        // Idle GameInfo has no cover, tags or description, so the panel
        // must collapse well short of the cap rather than sitting on it.
        compare(Browse.GameInfo.detail_tags, "", "precondition: no tag data loaded");
        verify(panel.height < panel._maxHeight, "an empty details panel must not claim the full cap");
    }

    // Same 92% ceiling Modal.qml clamps every other modal to; this one used
    // to sit at ~94%, wider than any other modal is allowed to be.
    function test_panel_width_respects_the_shared_ceiling(): void {
        const panel = findChild(modal, "gameInfoPanel");
        verify(panel !== null);
        verify(panel.width <= Math.ceil(modal.width * 0.92), "panel (" + panel.width + ") must stay within 92% of " + modal.width);
    }

    // Centered by item position with the glyphs left-aligned, never
    // Text.AlignHCenter: a centered glyph run straddles a half pixel and
    // softens, which CLAUDE.md rules out on user-visible text.
    function test_title_is_centered_by_item_position(): void {
        const title = findChild(modal, "gameInfoTitle");
        verify(title !== null);
        compare(title.horizontalAlignment, Text.AlignLeft, "glyphs stay left-aligned inside a positioned box");
        compare(title.x, Math.floor(title.x), "the title box must land on an integer pixel");
        if (modal._titleFits)
            compare(title.x, Sizing.center(title.parent.width, title.width), "a fitting title centers by item position");
    }
}
