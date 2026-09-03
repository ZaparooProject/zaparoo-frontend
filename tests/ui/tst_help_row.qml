// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

import QtQuick
import QtTest
import Zaparoo.Theme
import Zaparoo.Ui

TestCase {
    id: testCase

    name: "UiHelpRow"
    when: windowShown
    width: 316
    height: 216
    visible: true

    HelpRow {
        id: helpRow
        width: testCase.width
        height: Sizing.helpBarHeight
        entries: [
            {
                button: "Dpad",
                label: "Reposition"
            },
            {
                button: "ButtonA",
                label: "Reposition"
            },
            {
                button: "ButtonB",
                label: "Reposition"
            },
            {
                buttons: ["ButtonX", "ButtonY"],
                label: "Reposition"
            }
        ]
    }

    function initTestCase(): void {
        Sizing.screenWidth = testCase.width;
        Sizing.screenHeight = testCase.height;
        Sizing.bitmapType = true;
    }

    function cleanupTestCase(): void {
        Sizing.bitmapType = false;
        Sizing.screenWidth = 1280;
        Sizing.screenHeight = 720;
    }

    function test_240p_wraps_between_atomic_icon_label_groups(): void {
        compare(Sizing.tier, "240");
        compare(helpRow.wrapEntries, true);
        const flow = findChild(helpRow, "wrappedHelpRow");
        verify(flow !== null);

        let firstY = -1;
        let lastY = -1;
        let rowBounds = {};
        for (let i = 0; i < helpRow.entries.length; i++) {
            const entry = findChild(flow, "helpBarEntry-" + i);
            verify(entry !== null);
            const position = entry.mapToItem(flow, 0, 0);
            verify(position.x >= 0);
            verify(position.x + entry.width <= flow.width, "entry " + i + " must stay inside safe width");
            const rowKey = String(position.y);
            if (rowBounds[rowKey] === undefined)
                rowBounds[rowKey] = {
                    left: position.x,
                    right: position.x + entry.width
                };
            else {
                rowBounds[rowKey].left = Math.min(rowBounds[rowKey].left, position.x);
                rowBounds[rowKey].right = Math.max(rowBounds[rowKey].right, position.x + entry.width);
            }
            if (firstY < 0)
                firstY = position.y;
            lastY = Math.max(lastY, position.y);
        }
        verify(lastY > firstY, "overflowing help groups must use a second row");
        for (const rowKey of Object.keys(rowBounds)) {
            const bounds = rowBounds[rowKey];
            verify(Math.abs(bounds.left - (flow.width - bounds.right)) <= 1, "each wrapped help row must stay centered");
        }
        verify(helpRow.contentHeight <= helpRow.height, "wrapped rows must fit reserved help-bar height");
    }
}
