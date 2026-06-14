// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
pragma Singleton
import QtQuick

// Centralizes the qrc layout for embedded resources so the rule
// (`qrc:/qt/qml/Zaparoo/App/resources/...`) lives in exactly one place.
// Tile.qml and MainLayout.qml's prefetch repeater both build cover URLs
// from a `coverKey`; without a shared helper, a future change to the
// resource path or image format silently misses one of the two sites
// and breaks the QPixmapCache match between prefetch and visible Image.
QtObject {
    // Build a cover image URL from a `coverKey`.
    // Extension/scheme is chosen by directory:
    //   * `systems/<id>` — the curated SVG set under
    //     resources/images/systems/, tinted by the image provider.
    //   * `system-image/<path>` — user-supplied override artwork from the
    //     directory configured via `[images] system_dir` in frontend.toml.
    //     Served as-is (no tint) by the `system-image` image provider; the
    //     three theme color tokens are ignored for overrides.
    //   * `media-image/<encoded>` — media images (boxart, screenshot,
    //     wheel, titleshot, map, marquee, fanart, generic image)
    //     cached in process memory by `media_image_cache.rs`, served
    //     via the `media-image` QQuickImageProvider registered on the
    //     QML engine. The URL bypasses qrc entirely; QtQuick calls
    //     `requestImage` with the encoded key, the Rust side decodes
    //     back to `(systemId, path)` and returns bytes.
    //   * `categories/<name>` — curated Hub category icons, shipped
    //     as SVG source.
    //   * everything else (icons/Folder, icons/File, …) — SVG.

    // Base URL for everything under `resources/` in the embedded qrc.
    readonly property string baseUrl: "qrc:/qt/qml/Zaparoo/App/resources/"
    // Single-letter directory under resources/images/buttons/ — "a"/"b"/"c"/"d"
    // back the user-facing "Style A/B/C/D" picker. MainLayout binds this to
    // Browse.Settings.current_button_layout; the default keeps early
    // evaluation on Style A (the legacy glyph set).
    property string buttonLayout: "a"

    // Empty key returns an empty URL so the caller can use it as a
    // "no cover" sentinel.
    function _colorToken(colorValue: var): string {
        const text = String(colorValue === undefined ? "#ffffff" : colorValue);
        return text.charAt(0) === "#" ? text.substring(1) : text;
    }

    function _systemArtworkKey(key: string): string {
        if (key === "systems/MacPlus")
            return "systems/MacOS";
        if (key === "systems/SVI328")
            return "systems/Spectravideo";
        return key;
    }

    function coverUrl(key: string, foreground: var, secondary: var, background: var): url {
        if (key === "")
            return "";

        if (key.startsWith("system-image/"))
            return "image://system-image/" + key.substring("system-image/".length);

        if (key.startsWith("media-image/"))
            return "image://media-image/" + key.substring("media-image/".length);

        // System logos, Hub category icons, and UI glyphs (folders, file, action
        // icons) all go through the tinted-svg provider so their color tracks the
        // theme ramp. The _systemArtworkKey remap (MacPlus -> MacOS, SVI328 ->
        // Spectravideo) applies only to systems/ paths.
        if (key.startsWith("systems/") || key.startsWith("categories/") || key.startsWith("icons/")) {
            const artworkKey = key.startsWith("systems/") ? _systemArtworkKey(key) : key;
            const effectiveSecondary = background === undefined ? foreground : secondary;
            const effectiveBackground = background === undefined ? secondary : background;
            const fg = _colorToken(foreground);
            const second = _colorToken(effectiveSecondary === undefined ? foreground : effectiveSecondary);
            const bg = _colorToken(effectiveBackground === undefined ? "#000000" : effectiveBackground);
            return "image://tinted-svg/" + fg + "/" + second + "/" + bg + "/images/" + artworkKey + ".svg";
        }

        return baseUrl + "images/" + key + ".svg";
    }

    // Top-right HUD host-status icons (NFC/Wi-Fi/LAN/Bluetooth).
    function statusIconUrl(name: string): url {
        if (name === "")
            return "";

        return baseUrl + "images/status/" + name + ".svg";
    }

    // General-purpose UI glyphs (folder, file, loading spinner, settings,
    // nav arrows, D-pad, ...) under resources/images/icons/. Gamepad
    // button glyphs (ButtonA/B/X/Y/L/R) live separately under
    // resources/images/buttons/<layout>/ and ship as PNG so the
    // antialiased button-face shading survives intact.
    function iconUrl(name: string): url {
        if (name === "")
            return "";

        if (name.startsWith("Button"))
            return baseUrl + "images/buttons/" + buttonLayout + "/" + name + ".png";

        const ext = name.startsWith("Dpad") ? "png" : "svg";
        return baseUrl + "images/icons/" + name + "." + ext;
    }
}
