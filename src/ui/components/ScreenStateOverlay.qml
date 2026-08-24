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
    // errorMessage is state only. It may contain backend diagnostics and must
    // never be rendered; technical detail belongs in logs.
    property string errorMessage: ""
    property int count: 0
    property string emptyText: qsTr("Nothing here")
    property string loadingText: qsTr("Loading…")
    property string errorText: qsTr("Check Zaparoo Core and try again.")
    property int loadingDelayMs: 300
    property int minimumLoadingVisibleMs: 200
    // Vertical center for the loading cue, in overlay-local coordinates.
    // Defaults to the overlay's own center (matching Empty/Error below);
    // a caller whose visible content rect is offset from that — the
    // games/systems grid sits below the header, for instance — overrides
    // this so the cue lands at the exact y the global transition cue
    // centered on, and the handoff between the two does not jump.
    property real cueCenterY: overlay.height / 2
    readonly property bool loadingVisible: overlay.enabled && delayedLoading.showing
    // Named `viewState` rather than `state` — `Item.state` is a
    // built-in slot wired to `states:` / `transitions:`, and shadowing
    // it would silently break any future maintainer who adds state
    // animations to the overlay or a subclass. During the loading-delay
    // window, report Ready so Empty/Error text does not flash before the
    // async result settles.
    readonly property string viewState: overlay.loadingVisible ? "loading" : (overlay.loading ? "ready" : (overlay.errorMessage !== "" ? "error" : (overlay.count === 0 ? "empty" : "ready")))

    visible: overlay.enabled && overlay.viewState !== "ready"

    // Loading state shares the delayed LoadingIndicator path with the
    // global transition overlay — single visual vocabulary for "in
    // flight" without flashing on sub-threshold loads. Positioned on
    // `cueCenterY` rather than living in the Column below: it must line
    // up with the global cue it hands off from, which Empty/Error text
    // (a terminal state, never shown mid-transition) has no need to.
    DelayedLoadingIndicator {
        id: delayedLoading
        objectName: "screenStateLoadingCue"
        x: Sizing.center(overlay.width, width)
        y: Sizing.px(overlay.cueCenterY - height / 2)
        active: overlay.enabled && overlay.loading
        delayMs: overlay.loadingDelayMs
        minimumVisibleMs: overlay.minimumLoadingVisibleMs
        text: overlay.loadingText
    }

    Column {
        x: Sizing.center(parent.width, width)
        y: Sizing.center(parent.height, height)
        spacing: Sizing.pctH(0.6)

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
            font.pixelSize: Sizing.fontSection
            color: Theme.textPrimary
            horizontalAlignment: Text.AlignHCenter
            renderType: Text.NativeRendering
        }

        Text {
            anchors.horizontalCenter: parent.horizontalCenter
            visible: overlay.viewState === "error" && overlay.errorText !== ""
            text: overlay.errorText
            font.family: Theme.fontUi
            font.pixelSize: Sizing.fontCaption
            color: Theme.textPrimary
            wrapMode: Text.WordWrap
            horizontalAlignment: Text.AlignHCenter
            width: Sizing.px(overlay.width * 0.7)
            renderType: Text.NativeRendering
        }
    }
}
