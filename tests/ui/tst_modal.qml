// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

import QtQuick
import QtTest
import Zaparoo.Theme
import Zaparoo.Ui

TestCase {
    id: testCase
    name: "UiModal"
    when: windowShown
    width: 640
    height: 480
    visible: true

    Modal {
        id: modal
        anchors.fill: parent
        open: true
        kind: "action_error"
    }

    function initTestCase(): void {
        Sizing.screenWidth = testCase.width;
        Sizing.screenHeight = testCase.height;
    }

    function init(): void {
        modal.kind = "action_error";
        modal.title = "";
        modal.body = "";
        modal.buttonLabel = "OK";
        modal.confirmYesLabel = "Yes";
        modal.confirmNoLabel = "No";
        modal.panelMaxWidth = Sizing.pctH(90);
    }

    function _panelWidth(): int {
        const panel = findChild(modal, "modalPanel");
        verify(panel !== null);
        return panel.width;
    }

    // Mirrors tst_context_menu.qml's `test_panel_width_tracks_longest_label`:
    // a short prompt no longer opens a panel sized for the widest case.
    function test_panel_width_tracks_title_length(): void {
        modal.title = "Hi";
        const shortWidth = _panelWidth();
        verify(shortWidth >= modal._minPanelWidth);

        modal.title = "A Considerably Longer Modal Title That Needs More Room";
        const longWidth = _panelWidth();
        verify(longWidth > shortWidth);
    }

    function test_panel_width_tracks_button_labels(): void {
        modal.kind = "confirm";
        modal.title = "Confirm?";
        modal.confirmNoLabel = "No";
        modal.confirmYesLabel = "Yes";
        const shortWidth = _panelWidth();

        modal.confirmNoLabel = "Absolutely Not";
        modal.confirmYesLabel = "Yes, Definitely Proceed";
        const longWidth = _panelWidth();
        verify(longWidth > shortWidth);
    }

    // A long body paragraph must not force a full-width panel — it wraps
    // instead, capped at the low end of the 45-75 character readable
    // measure (see Modal.qml's `_bodyMaxLineWidth`).
    function test_body_width_is_capped_not_full_bleed(): void {
        modal.kind = "action_error";
        modal.title = "Notice";
        modal.body = "This is a considerably long body paragraph that would, if left " + "unclamped, stretch the modal panel out to the full width of the " + "viewport instead of wrapping onto additional lines like a normal " + "paragraph of readable prose is expected to do in any well-behaved " + "dialog implementation.";
        const width = _panelWidth();
        verify(width < Sizing.pctW(92), "an unbounded body must not force a near-full-width panel");
        verify(width >= modal._minPanelWidth);
    }

    // `panelMaxWidth` still clamps the content-driven width from above —
    // a caller-supplied cap is never exceeded, even for very long content.
    function test_panel_max_width_still_clamps(): void {
        modal.kind = "confirm";
        modal.title = "A Considerably Longer Modal Title That Needs More Room";
        modal.confirmNoLabel = "Absolutely Not Going To Happen";
        modal.confirmYesLabel = "Yes, Definitely Proceed Right Now";
        modal.panelMaxWidth = Sizing.pctW(20);
        const width = _panelWidth();
        verify(width <= Sizing.pctW(20) + 1, "panelMaxWidth must still cap the panel");
    }

    // Shell content is opaque to Modal (an arbitrary caller-supplied Item),
    // so it keeps the old percentage-of-viewport sizing rather than trying
    // to measure it — this is the one kind the content-driven formula does
    // not apply to.
    function test_shell_kind_keeps_percentage_sizing(): void {
        modal.kind = "shell";
        modal.title = "";
        modal.panelMaxWidth = Sizing.pctH(90);
        const width = _panelWidth();
        compare(width, Sizing.px(Math.min(testCase.width * 0.78, Sizing.pctH(90))));
    }
}
