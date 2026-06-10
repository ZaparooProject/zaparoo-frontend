// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
// Shared Loading / Error / Empty / Ready overlay for the data screens.
// The four-state vocabulary is the locked decision in MVP_PLAN.md;
// this component is the single rendering surface that implements it.
// Callers expose their model's `loading`, `error_message`, and `count`
// here and the overlay derives `state` internally so the same ternary
// isn't repeated at every binding site. CategoriesModel doesn't have
// a `loading` qproperty (eager bind_to_endpoint! load) — leaving the
// `loading` property at its default `false` is the supported usage.

import QtQuick
import Zaparoo.Theme

// Software-rendering safe: only Item, Column, Text. No transforms,
// no shaders, no animations — state changes are atomic per the
// "Plain text Loading state" decision; skeletons would register
// slower than our ~200 ms loads anyway.
Item {
    id: overlay

    property bool enabled: true
    property bool loading: false
    property string errorMessage: ""
    property int count: 0
    property string emptyText: qsTr("Nothing here")
    property string loadingText: qsTr("Loading…")
    property int loadingDelayMs: 300
    property int minimumLoadingVisibleMs: 200
    readonly property bool loadingVisible: overlay.enabled && delayedLoading.showing
    // Named `viewState` rather than `state` — `Item.state` is a
    // built-in slot wired to `states:` / `transitions:`, and shadowing
    // it would silently break any future maintainer who adds state
    // animations to the overlay or a subclass. During the loading-delay
    // window, report Ready so Empty/Error text does not flash before the
    // async result settles.
    readonly property string viewState: overlay.loadingVisible ? "loading" : (overlay.loading ? "ready" : (overlay.errorMessage !== "" ? "error" : (overlay.count === 0 ? "empty" : "ready")))

    visible: overlay.enabled && overlay.viewState !== "ready"

    Column {
        x: Sizing.center(parent.width, width)
        y: Sizing.center(parent.height, height)
        spacing: Sizing.pctH(0.6)

        // Loading state shares the delayed LoadingIndicator path with the
        // global transition overlay — single visual vocabulary for "in
        // flight" without flashing on sub-threshold loads. Error/Empty
        // stay as plain text since they are terminal states.
        DelayedLoadingIndicator {
            id: delayedLoading
            anchors.horizontalCenter: parent.horizontalCenter
            active: overlay.enabled && overlay.loading
            delayMs: overlay.loadingDelayMs
            minimumVisibleMs: overlay.minimumLoadingVisibleMs
            text: overlay.loadingText
        }

        Text {
            anchors.horizontalCenter: parent.horizontalCenter
            visible: overlay.viewState === "error" || overlay.viewState === "empty"
            text: {
                if (overlay.viewState === "error")
                    return qsTr("Failed to load");

                if (overlay.viewState === "empty")
                    return overlay.emptyText;

                return "";
            }
            font.family: Theme.fontUi
            font.pixelSize: Sizing.fontSize(2.9)
            color: Theme.textPrimary
            horizontalAlignment: Text.AlignHCenter
            renderType: Text.NativeRendering
        }

        Text {
            anchors.horizontalCenter: parent.horizontalCenter
            visible: overlay.viewState === "error" && overlay.errorMessage !== ""
            text: overlay.errorMessage
            font.family: Theme.fontUi
            font.pixelSize: Sizing.fontSize(2.4)
            color: Theme.textPrimary
            wrapMode: Text.WordWrap
            horizontalAlignment: Text.AlignHCenter
            width: Sizing.px(overlay.width * 0.7)
            renderType: Text.NativeRendering
        }
    }
}
