// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

import QtQuick
import QtTest
import Zaparoo.Theme
import Zaparoo.Ui

// Direct SettingsSectionHeader coverage (round 6, item 3). The header
// became a filled full-width band rather than a bigger/bolder label,
// because bitmap mode (--crt, or embedded 240p) quantizes fontSection and
// fontBody to the same 8px and Theme.fontUi's bitmap face has one weight —
// size and weight are unavailable as signals there, so the break has to be
// a rectangle and a color step instead. See docs/style.md -> "Settings
// section headers".
TestCase {
    id: testCase
    name: "UiSettingsSectionHeader"
    when: windowShown
    width: 640
    height: 480
    visible: true

    function _linearChannel(value: real): real {
        return value <= 0.04045 ? value / 12.92 : Math.pow((value + 0.055) / 1.055, 2.4);
    }

    function _relativeLuminance(value: color): real {
        return 0.2126 * _linearChannel(value.r) + 0.7152 * _linearChannel(value.g) + 0.0722 * _linearChannel(value.b);
    }

    function _contrastRatio(first: color, second: color): real {
        const firstLuminance = _relativeLuminance(first);
        const secondLuminance = _relativeLuminance(second);
        return (Math.max(firstLuminance, secondLuminance) + 0.05) / (Math.min(firstLuminance, secondLuminance) + 0.05);
    }

    SettingsSectionHeader {
        id: header
        width: 400
        contentInset: 20
        label: "Analog video"
    }

    function cleanup(): void {
        Theme.colorSchemeId = ColorSchemes.defaultId;
    }

    function test_band_spans_the_full_component_width_and_uses_border_mid(): void {
        const band = findChild(header, "settingsSectionHeaderBand");
        verify(band !== null);
        compare(band.x, 0);
        compare(band.width, header.width, "the band must span edge-to-edge, not just the card-padding-inset content area");
        compare(band.color.toString(), Theme.borderMid.toString());
    }

    function test_label_carries_content_inset_plus_the_shared_field_inset_and_elides(): void {
        const label = findChild(header, "settingsSectionHeaderLabel");
        verify(label !== null);
        compare(label.text, "Analog video");
        compare(label.anchors.leftMargin, header.contentInset + Sizing.pctW(2));
        compare(label.anchors.rightMargin, header.contentInset + Sizing.pctW(2));
        compare(label.elide, Text.ElideRight);
    }

    // Every preset in the catalog must keep the label legible against the
    // band. 4.0:1 floor matches the plan's stated guarantee — comfortably
    // under the measured 4.11-7.71:1 range across the round-6 catalog (see
    // docs/style.md -> "Settings section headers").
    function test_text_primary_clears_contrast_against_the_band_on_every_preset(): void {
        for (let i = 0; i < ColorSchemes.ids.length; i++) {
            const id = ColorSchemes.ids[i];
            Theme.colorSchemeId = id;
            verify(_contrastRatio(Theme.textPrimary, Theme.borderMid) >= 4.0, id + " textPrimary/borderMid " + _contrastRatio(Theme.textPrimary, Theme.borderMid));
        }
    }

    // borderMid must actually read as a distinct block against the card
    // it sits on, not just barely differ — otherwise the band carries no
    // visual break at all. 1.5:1 floor matches the measured 1.61-2.26:1
    // range across the round-6 catalog.
    function test_border_mid_separates_from_surface_card_on_every_preset(): void {
        for (let i = 0; i < ColorSchemes.ids.length; i++) {
            const id = ColorSchemes.ids[i];
            Theme.colorSchemeId = id;
            verify(_contrastRatio(Theme.borderMid, Theme.surfaceCard) >= 1.5, id + " borderMid/surfaceCard " + _contrastRatio(Theme.borderMid, Theme.surfaceCard));
        }
    }
}
