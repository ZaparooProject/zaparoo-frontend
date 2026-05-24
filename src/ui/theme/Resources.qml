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
    //   * `systems/<id>` — the 144-asset curated PNG set under
    //     resources/images/systems/, ships as PNG.
    //   * `media-image/<encoded>` — media images (boxart, screenshot,
    //     wheel, titleshot, map, marquee, fanart, generic image)
    //     cached in process memory by `media_image_cache.rs`, served
    //     via the `media-image` QQuickImageProvider registered on the
    //     QML engine. The URL bypasses qrc entirely; QtQuick calls
    //     `requestImage` with the encoded key, the Rust side decodes
    //     back to `(systemId, path)` and returns bytes.
    //   * everything else (categories, icons/Folder, icons/File, …) —
    //     SVG. QtSvg rasterizes the source on first load and the
    //     result lands in QPixmapCache, so paint cost matches PNG
    //     after the one-shot decode.

    // Base URL for everything under `resources/` in the embedded qrc.
    readonly property string baseUrl: "qrc:/qt/qml/Zaparoo/App/resources/"
    // Single-letter directory under resources/images/buttons/ — "a"/"b"/"c"/"d"
    // back the user-facing "Style A/B/C/D" picker. MainLayout binds this to
    // Browse.Settings.current_button_layout; the default keeps early
    // evaluation on Style A (the legacy Nintendo-style glyph set).
    property string buttonLayout: "a"

    // Empty key returns an empty URL so the caller can use it as a
    // "no cover" sentinel.
    function coverUrl(key: string): url {
        if (key === "")
            return "";

        if (key.startsWith("media-image/"))
            return "image://media-image/" + key.substring("media-image/".length);

        const ext = key.startsWith("systems/") ? "png" : "svg";
        return baseUrl + "images/" + key + "." + ext;
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
