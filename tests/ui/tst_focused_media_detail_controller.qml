// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
// var-typed function property bindings (identityForIndex, loadForIndex) can't
// be statically typed by the QML compiler. Suppress the compiler category.
// qmllint disable compiler

import QtQuick
import QtTest
import Zaparoo.Ui

TestCase {
    id: testCase
    name: "FocusedMediaDetailController"

    property int itemCount: 0
    property int currentIndex: 0
    property int loadCount: 0
    property int clearCount: 0
    property var identities: []
    property var loadedIndices: []
    property bool rapidScrollActive: false

    FocusedMediaDetailController {
        id: controller

        enabled: false
        debounceMs: 5
        itemCount: testCase.itemCount
        currentIndex: testCase.currentIndex
        rapidScrollActive: testCase.rapidScrollActive
        identityForIndex: index => testCase.identities[index] ?? ""
        loadForIndex: index => {
            testCase.loadCount += 1;
            testCase.loadedIndices.push(index);
        }
        clearDetail: () => {
            testCase.clearCount += 1;
        }
    }

    function resetState(): void {
        controller.enabled = false;
        testCase.itemCount = 0;
        testCase.currentIndex = 0;
        testCase.loadCount = 0;
        testCase.clearCount = 0;
        testCase.identities = [];
        testCase.loadedIndices = [];
        testCase.rapidScrollActive = false;
        wait(10);
        testCase.clearCount = 0;
    }

    function init(): void {
        resetState();
    }

    function cleanup(): void {
        resetState();
    }

    function test_loads_after_debounce_for_selected_identity(): void {
        testCase.identities = ["NES\n/a", "NES\n/b"];
        testCase.itemCount = 2;
        controller.enabled = true;

        wait(20);

        compare(testCase.loadCount, 1);
        compare(testCase.loadedIndices[0], 0);
    }

    function test_count_change_with_same_identity_does_not_reload(): void {
        testCase.identities = ["NES\n/a", "NES\n/b"];
        testCase.itemCount = 2;
        controller.enabled = true;
        wait(20);

        testCase.identities = ["NES\n/a", "NES\n/b", "SNES\n/c"];
        testCase.itemCount = 3;
        wait(20);

        compare(testCase.loadCount, 1);
    }

    function test_index_change_loads_new_identity_once(): void {
        testCase.identities = ["NES\n/a", "NES\n/b"];
        testCase.itemCount = 2;
        controller.enabled = true;
        wait(20);

        testCase.currentIndex = 1;
        wait(20);

        compare(testCase.loadCount, 2);
        compare(testCase.loadedIndices[1], 1);
    }

    function test_empty_identity_clears_and_suppresses_load(): void {
        testCase.identities = [""];
        testCase.itemCount = 1;
        controller.enabled = true;
        wait(20);

        compare(testCase.loadCount, 0);
        compare(testCase.clearCount, 1);
    }

    function test_disable_clears_detail(): void {
        testCase.identities = ["NES\n/a"];
        testCase.itemCount = 1;
        controller.enabled = true;
        wait(20);

        controller.enabled = false;
        wait(1);

        compare(testCase.clearCount, 1);
    }

    function test_clear_transient_reloads_same_identity(): void {
        testCase.identities = ["NES\n/a"];
        testCase.itemCount = 1;
        controller.enabled = true;
        wait(20);

        controller.clearTransient();
        wait(20);

        compare(testCase.clearCount, 1);
        compare(testCase.loadCount, 2);
    }

    function test_rapid_scroll_hides_detail_and_reloads_after_stop(): void {
        testCase.identities = ["NES\n/a", "NES\n/b"];
        testCase.itemCount = 2;
        controller.enabled = true;
        wait(20);

        testCase.rapidScrollActive = true;
        wait(1);
        testCase.currentIndex = 1;
        wait(20);

        compare(testCase.loadCount, 1);
        verify(testCase.clearCount >= 1);

        testCase.rapidScrollActive = false;
        wait(20);

        compare(testCase.loadCount, 2);
        compare(testCase.loadedIndices[1], 1);
    }
}
