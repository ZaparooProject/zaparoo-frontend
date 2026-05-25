// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
pragma Singleton
import QtQuick
// Browse-theme profiles loaded from bundled JSON. The theme files stay
// data-only; this singleton maps their semantic sections (`header`,
// `status`, `grid`, `list`, `detail`, `footer`, `surface`) into the
// QML tree with lightweight token resolution against `Sizing`.
QtObject {
    readonly property string currentThemeId: ThemePalette.currentThemeId

    function currentProfile(viewId: string): var {
        return BrowseLayouts.themeProfile(BrowseLayouts.currentThemeId, viewId);
    }

    function themeProfile(themeId: string, viewId: string): var {
        const themeData = ThemePalette.themeData(themeId);
        if (themeData === null || typeof themeData !== "object")
            return null;
        if (!(viewId in themeData)) {
            console.warn("BrowseLayouts: missing view profile '" + viewId + "' in theme '" + themeId + "'");
            return null;
        }
        return themeData[viewId];
    }

    function hasValue(profile: var, path: string): bool {
        return BrowseLayouts._lookup(profile, path) !== undefined;
    }

    function boolValue(profile: var, path: string, fallback: bool): bool {
        const resolved = BrowseLayouts._resolve(profile, BrowseLayouts._lookup(profile, path), {});
        return typeof resolved === "boolean" ? resolved : fallback;
    }

    function numberValue(profile: var, path: string, fallback: int): int {
        const resolved = BrowseLayouts._resolve(profile, BrowseLayouts._lookup(profile, path), {});
        return typeof resolved === "number" && isFinite(resolved) ? resolved : fallback;
    }

    function stringValue(profile: var, path: string, fallback: string): string {
        const resolved = BrowseLayouts._resolve(profile, BrowseLayouts._lookup(profile, path), {});
        return typeof resolved === "string" ? resolved : fallback;
    }

    function _lookup(profile: var, path: string): var {
        if (profile === null || typeof profile !== "object" || path === "")
            return undefined;
        const parts = path.split(".");
        let current = profile;
        for (let i = 0; i < parts.length; i++) {
            const part = parts[i];
            if (current === null || typeof current !== "object" || !(part in current))
                return undefined;
            current = current[part];
        }
        return current;
    }

    function _splitArgs(argsText: string): var {
        const parts = [];
        let start = 0;
        let depth = 0;
        for (let i = 0; i < argsText.length; i++) {
            const ch = argsText[i];
            if (ch === "(")
                depth++;
            else if (ch === ")")
                depth--;
            else if (ch === "," && depth === 0) {
                parts.push(argsText.substring(start, i).trim());
                start = i + 1;
            }
        }
        parts.push(argsText.substring(start).trim());
        return parts;
    }

    function _resolve(profile: var, value: var, seenRefs: var): var {
        if (typeof value !== "string")
            return value;

        if (value.startsWith("pctW:"))
            return Sizing.pctW(Number(value.substring("pctW:".length)));
        if (value.startsWith("pctH:"))
            return Sizing.pctH(Number(value.substring("pctH:".length)));
        if (value.startsWith("fontSize:"))
            return Sizing.fontSize(Number(value.substring("fontSize:".length)));
        if (value === "cornerRadius")
            return Sizing.cornerRadius;
        if (value === "headerSideMargin")
            return Sizing.headerSideMargin;
        if (value.startsWith("ref:")) {
            const refPath = value.substring("ref:".length);
            if (seenRefs[refPath] === true) {
                console.warn("BrowseLayouts: circular ref '" + refPath + "'");
                return undefined;
            }
            seenRefs[refPath] = true;
            return BrowseLayouts._resolve(profile, BrowseLayouts._lookup(profile, refPath), seenRefs);
        }

        const fnMatch = value.match(/^([a-z]+)\((.*)\)$/);
        if (fnMatch !== null) {
            const fnName = fnMatch[1];
            const args = BrowseLayouts._splitArgs(fnMatch[2]).map(arg => BrowseLayouts._resolve(profile, arg, seenRefs));
            if (args.length === 2 && fnName === "min")
                return Math.min(args[0], args[1]);
            if (args.length === 2 && fnName === "max")
                return Math.max(args[0], args[1]);
            if (fnName === "sum")
                return args.reduce((total, arg) => total + arg, 0);
        }

        const numeric = Number(value);
        if (!isNaN(numeric))
            return numeric;

        return value;
    }
}
