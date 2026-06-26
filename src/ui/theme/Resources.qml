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
    //   * `systems/<id>` — the curated SVG set under resources/images/systems/,
    //     tinted by the image provider unless the optional color style has a
    //     matching PNG under resources/images/systems-color/.
    //   * `custom-image/<path>` — user-supplied override artwork (system art
    //     or Hub icons) from the customization root (`[custom] dir` in
    //     frontend.toml, or the default `.../zaparoo/custom/`). Served exactly
    //     as-is by the `custom-image` image provider; it never enters the
    //     tint pipeline and the three theme color tokens are ignored.
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
    // "tinted" is the default theme-tracking SVG style. "color" opts system
    // logos into the restored full-color PNG set when a matching asset exists.
    property string systemLogoStyle: "tinted"
    readonly property var _coloredSystemStems: [
        "3DO", "3DS", "AcornElectron", "AdventureVision",
        "Amiga", "Amiga1200", "Amiga500", "AmigaCD32",
        "Amstrad", "Android", "AppleII", "Aquarius",
        "Arcade", "Arcadia", "Archimedes", "Astrocade",
        "Atari2600", "Atari5200", "Atari7800", "Atari800",
        "AtariLynx", "AtariST", "AtariXEGS", "Atomiswave",
        "BBCMicro", "C16", "C64", "CDI",
        "CPS1", "CPS2", "CPS3", "CasioPV1000",
        "ChannelF", "ColecoAdam", "ColecoVision", "CreatiVision",
        "DAPHNE", "DOS", "Dreamcast", "FDS",
        "FM7", "FMTowns", "GBA", "GBA2P",
        "Gaelco", "Gamate", "GameCom", "GameCube",
        "GameGear", "GameMaster", "GameNWatch", "Gameboy",
        "Gameboy2P", "GameboyColor", "Genesis", "Genesis.eu",
        "Genesis.jp", "GenesisMSU", "Hikaru", "Intellivision",
        "Jaguar", "JaguarCD", "Lynx48", "MSX",
        "MSX1", "MSX2", "MSX2Plus", "MacOS",
        "MasterSystem", "MasterSystem.jp", "MegaCD", "MegaCD.us",
        "MegaDuck", "Model1", "Model2", "Model3",
        "NAOMI", "NAOMI2", "NDS", "NES",
        "NES.jp", "NGage", "Namco22", "NeoGeo",
        "NeoGeoAES", "NeoGeoCD", "NeoGeoMVS", "NeoGeoPocket",
        "NeoGeoPocketColor", "Nintendo64", "Odyssey2", "Oric",
        "PC88", "PC98", "PCFX", "PET2001",
        "PS2", "PS3", "PS4", "PS5",
        "PSP", "PSX", "Pico8", "PokemonMini",
        "SAMCoupe", "SG1000", "SGBMSU1", "SNES",
        "SNES.jp", "SNESMSU1", "Saturn", "ScummVM",
        "Sega32X", "Sega32X.jp", "SeriesXS", "Singe", "SordM5",
        "Spectravideo", "Sufami", "SuperACan", "SuperGameboy",
        "SuperGrafx", "SuperVision", "Switch", "TI994A",
        "TIC80", "TRS80", "Thomson", "TomyTutor",
        "Triforce", "TurboGrafx16", "TurboGrafx16.eu", "TurboGrafx16.jp",
        "TurboGrafx16CD", "TurboGrafx16CD.eu", "TurboGrafx16CD.jp", "VIC20",
        "Vectrex", "VideopacPlus", "VirtualBoy", "Vita",
        "Wii", "WiiU", "Windows", "WonderSwan",
        "WonderSwanColor", "X1", "X68000", "Xbox",
        "Xbox360", "XboxOne", "ZX81", "ZXSpectrum",
        "iOS"
    ]

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

    function _coloredSystemUrl(artworkKey: string): string {
        const stem = artworkKey.substring("systems/".length);
        if (systemLogoStyle === "color" && _coloredSystemStems.indexOf(stem) >= 0)
            return baseUrl + "images/systems-color/" + stem + ".png";
        return "";
    }

    function coverUrl(key: string, foreground: var, secondary: var, background: var): url {
        if (key === "")
            return "";

        if (key.startsWith("custom-image/"))
            return "image://custom-image/" + key.substring("custom-image/".length);

        if (key.startsWith("media-image/"))
            return "image://media-image/" + key.substring("media-image/".length);

        // System logos normally go through the tinted-svg provider so their
        // color tracks the theme. The optional color style short-circuits to the
        // restored PNG set when available. Hub category icons and UI glyphs stay
        // tinted. The _systemArtworkKey remap (MacPlus -> MacOS, SVI328 ->
        // Spectravideo) applies only to systems/ paths.
        if (key.startsWith("systems/") || key.startsWith("categories/") || key.startsWith("icons/")) {
            const artworkKey = key.startsWith("systems/") ? _systemArtworkKey(key) : key;
            if (key.startsWith("systems/")) {
                const colored = _coloredSystemUrl(artworkKey);
                if (colored !== "")
                    return colored;
            }
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
