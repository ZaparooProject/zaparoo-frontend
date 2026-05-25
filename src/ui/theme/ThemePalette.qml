// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
pragma Singleton
import QtQuick
import Zaparoo.Browse as Browse

// Shared theme-data loader. Owns JSON loading and exposes both palette
// lookups and full theme objects so visual singletons/components do not
// each grow their own loader path.
QtObject {
    property bool crtNativePath: false
    readonly property string currentThemeId: crtNativePath ? "crt" : Browse.Settings.current_theme
    property var _themeCache: ({})

    function themeData(themeId: string): var {
        return ThemePalette._loadTheme(themeId);
    }

    function currentTheme(): var {
        return ThemePalette.themeData(ThemePalette.currentThemeId);
    }

    function colorValue(path: string, fallback: color): color {
        const resolved = ThemePalette._lookup(ThemePalette.currentTheme(), path);
        return typeof resolved === "string" && resolved !== "" ? resolved : fallback;
    }

    function _themeUrl(themeId: string): string {
        if (themeId === "default" || themeId === "crt")
            return "qrc:/qt/qml/Zaparoo/Theme/browse-themes/" + themeId + ".json";
        const base = Browse.Settings.themes_directory_url;
        return base !== "" ? base + themeId + ".json" : "";
    }

    function _loadTheme(themeId: string): var {
        if (ThemePalette._themeCache[themeId] !== undefined)
            return ThemePalette._themeCache[themeId];

        const url = ThemePalette._themeUrl(themeId);
        if (url === "") {
            ThemePalette._themeCache[themeId] = null;
            return null;
        }

        const req = new XMLHttpRequest();
        req.open("GET", url, false);
        req.send();

        if (req.status !== 0 && (req.status < 200 || req.status >= 300)) {
            console.warn("ThemePalette: failed to load theme '" + themeId + "' from " + url + " (status " + req.status + ")");
            ThemePalette._themeCache[themeId] = null;
            return null;
        }

        try {
            ThemePalette._themeCache[themeId] = JSON.parse(req.responseText);
        } catch (err) {
            console.warn("ThemePalette: invalid JSON in theme '" + themeId + "': " + err);
            ThemePalette._themeCache[themeId] = null;
        }

        return ThemePalette._themeCache[themeId];
    }

    function _lookup(themeData: var, path: string): var {
        if (themeData === null || typeof themeData !== "object" || path === "")
            return undefined;
        const parts = path.split(".");
        let current = themeData;
        for (let i = 0; i < parts.length; i++) {
            const part = parts[i];
            if (current === null || typeof current !== "object" || !(part in current))
                return undefined;
            current = current[part];
        }
        return current;
    }
}
