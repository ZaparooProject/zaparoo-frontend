// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
pragma Singleton

import QtQuick

QtObject {
    readonly property string arcadeId: "Arcade"
    readonly property string computerId: "Computer"
    readonly property string consoleId: "Console"
    readonly property string handheldId: "Handheld"
    readonly property string otherId: "Other"

    function canonicalize(id: string): string {
        if (id === "Computers")
            return computerId;
        if (id === "Consoles")
            return consoleId;
        if (id === "Handhelds")
            return handheldId;
        return id;
    }

    function coverKey(id: string): string {
        const category = canonicalize(id);
        if (category === arcadeId)
            return "categories/Arcade";
        if (category === computerId)
            return "categories/Computer";
        if (category === consoleId)
            return "categories/Console";
        if (category === handheldId)
            return "categories/Handheld";
        return "categories/" + category;
    }
}
