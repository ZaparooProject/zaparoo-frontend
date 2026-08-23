// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0
// cxx-qt 0.8 patches `isFinal: true` on singleton properties but the
// qmltypes schema has no `isFinal` slot, so every read of a Browse
// singleton trips qmllint's "Member can be shadowed" check. Suppress
// the compiler category file-wide until the schema grows the slot.
// qmllint disable compiler
pragma Singleton

import QtQuick
import Zaparoo.Browse as Browse

// Locale-aware number formatting for user-visible counts (media/scrape
// totals, file counts). Nothing in the app formatted numbers for locale
// before round 9 -- the raw i32 crossed the cxx-qt bridge and
// `qsTr("%1 indexed").arg(total)` stringified it plainly, so `100000`
// rendered as `100000` instead of a locale-grouped `100,000`.
//
// Lives in `Zaparoo.Ui`, not `Zaparoo.Theme` -- `Zaparoo.Theme` is
// deliberately dependency-free of `Zaparoo.Browse` (see Motion.qml's doc
// comment), but `locale()` needs `Browse.Settings.current_language` to
// follow the app's own language setting rather than the host OS locale.
// `HeaderBar.qml` (already in `Zaparoo.Ui`) established the same pattern
// for the clock; `locale()` here lifts that logic to one shared copy.
QtObject {
    id: root

    function locale(): var {
        const language = Browse.Settings.current_language;
        if (language === "" || language === "auto")
            return Qt.locale();
        return Qt.locale(language);
    }

    // Digit-grouped integer count, e.g. "100,000" (or the locale's own
    // grouping/digit convention). Precision pinned to 0 -- bare
    // `Number.toLocaleString(locale)` defaults to 2 decimal places, which
    // would render an integer count as "100,000.00".
    function count(n: int): string {
        return Number(n).toLocaleString(root.locale(), "f", 0);
    }
}
