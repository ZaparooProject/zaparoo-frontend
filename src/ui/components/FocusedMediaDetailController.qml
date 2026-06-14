// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
// var-typed function properties (identityForIndex, loadForIndex, clearDetail)
// and var-typed mediaModel cannot be statically typed. Structural; suppress compiler.
// qmllint disable compiler

import QtQuick

Item {
    id: root

    property int itemCount: 0
    property int currentIndex: 0
    property int debounceMs: 220
    property var mediaModel: null
    property var identityForIndex: null
    property var loadForIndex: null
    property var clearDetail: null
    property bool clearOnDisable: true
    property bool rapidScrollActive: false

    property string _requestedKey: ""
    property string _pendingKey: ""
    property int _pendingIndex: -1

    visible: false

    function requestNow(): void {
        root._schedule(true);
    }

    function clearTransient(): void {
        root._resetTransientState(true);
        if (!root.rapidScrollActive)
            root._schedule(false);
    }

    function _identityAt(index: int): string {
        if (!root.enabled || root.itemCount <= 0 || index < 0 || index >= root.itemCount)
            return "";
        if (typeof root.identityForIndex === "function") {
            const custom = root.identityForIndex(index);
            return custom ?? "";
        }
        if (root.mediaModel === null || typeof root.mediaModel.system_id_at !== "function" || typeof root.mediaModel.path_at !== "function")
            return "";
        const systemId = root.mediaModel.system_id_at(index);
        const path = root.mediaModel.path_at(index);
        return systemId !== "" && path !== "" ? systemId + "\n" + path : "";
    }

    function _clearDetail(): void {
        if (typeof root.clearDetail === "function")
            root.clearDetail();
        else if (root.mediaModel !== null && typeof root.mediaModel.clear_current_detail === "function")
            root.mediaModel.clear_current_detail();
    }

    function _resetTransientState(clearDetail: bool): void {
        detailLoadDebounce.stop();
        root._requestedKey = "";
        root._pendingKey = "";
        root._pendingIndex = -1;
        if (clearDetail)
            root._clearDetail();
    }

    function _schedule(force: bool): void {
        if (!root.enabled)
            return;
        if (root.rapidScrollActive) {
            root._resetTransientState(true);
            return;
        }
        const key = root._identityAt(root.currentIndex);
        if (key === "") {
            root._resetTransientState(true);
            return;
        }
        if (!force && key === root._requestedKey)
            return;
        root._pendingKey = key;
        root._pendingIndex = root.currentIndex;
        detailLoadDebounce.restart();
    }

    function _loadPending(): void {
        if (!root.enabled || root._pendingKey === "" || root._pendingIndex < 0)
            return;
        const key = root._identityAt(root._pendingIndex);
        if (key === "" || key !== root._pendingKey)
            return;
        root._requestedKey = key;
        if (typeof root.loadForIndex === "function")
            root.loadForIndex(root._pendingIndex);
        else if (root.mediaModel !== null && typeof root.mediaModel.load_detail_at === "function")
            root.mediaModel.load_detail_at(root._pendingIndex);
    }

    onEnabledChanged: {
        if (enabled) {
            root._schedule(false);
        } else {
            root._resetTransientState(root.clearOnDisable);
        }
    }
    onRapidScrollActiveChanged: {
        if (rapidScrollActive)
            root._resetTransientState(true);
        else
            root._schedule(false);
    }
    onCurrentIndexChanged: root._schedule(false)
    onItemCountChanged: root._schedule(false)
    Component.onCompleted: root._schedule(false)

    Timer {
        id: detailLoadDebounce
        interval: root.debounceMs
        repeat: false
        onTriggered: root._loadPending()
    }
}
