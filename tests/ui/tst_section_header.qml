// Zaparoo Frontend
// Copyright (c) 2026 Wizzo Pty Ltd and the Zaparoo Project contributors.
// SPDX-License-Identifier: LicenseRef-PolyForm-Noncommercial-1.0.0

import QtQuick
import QtTest
import Zaparoo.Theme
import Zaparoo.Ui

// Direct SectionHeader coverage. A heading is a metadata-coloured label on
// a hairline rule -- colour and a rectangle, the two signals that survive
// bitmap mode (--crt, embedded 240p), where fontSection/fontBody quantize
// to the same 8px strike and the bitmap face has one weight. It has to
// read the same on the settings card and inside a modal panel, so the
// contrast floors are checked against both grounds. See docs/style.md ->
// "Section headings".
TestCase {
    id: testCase
    name: "UiSectionHeader"
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

    SectionHeader {
        id: header
        width: 400
        label: "Analog video"
    }

    function cleanup(): void {
        Theme.colorSchemeId = ColorSchemes.defaultId;
    }

    function test_rule_spans_the_full_width_at_the_bottom_in_border_mid(): void {
        const rule = findChild(header, "sectionHeaderRule");
        verify(rule !== null);
        compare(rule.x, 0);
        compare(rule.width, header.width, "the rule must run the full row width");
        compare(rule.height, Sizing.stroke(1), "a hairline, not a band");
        compare(rule.y + rule.height, header.implicitHeight, "the rule closes the heading");
        compare(rule.color.toString(), Theme.borderMid.toString());
    }

    function test_label_is_metadata_coloured_and_inset_like_a_field_label(): void {
        const label = findChild(header, "sectionHeaderLabel");
        verify(label !== null);
        compare(label.text, "Analog video");
        compare(label.color.toString(), Theme.textLabel.toString());
        compare(label.font.pixelSize, Sizing.fontBody);
        compare(label.anchors.leftMargin, Sizing.pctW(2));
        compare(label.anchors.rightMargin, Sizing.pctW(2));
        compare(label.elide, Text.ElideRight);
        verify(header.implicitHeight > label.implicitHeight, "the heading reserves room above the label and for the rule");
    }

    // The label sits on the settings card and on a modal panel. 3.0:1 is
    // the floor the palette already holds for `textLabel` on `surfaceCard`
    // (tst_color_schemes); the panel is one rung deeper, so it must clear
    // the same floor there.
    function test_label_clears_contrast_on_card_and_panel_on_every_preset(): void {
        for (let i = 0; i < ColorSchemes.ids.length; i++) {
            const id = ColorSchemes.ids[i];
            Theme.colorSchemeId = id;
            verify(_contrastRatio(Theme.textLabel, Theme.surfaceCard) >= 3.0, id + " textLabel/surfaceCard " + _contrastRatio(Theme.textLabel, Theme.surfaceCard));
            verify(_contrastRatio(Theme.textLabel, Theme.bgPanel) >= 3.0, id + " textLabel/bgPanel " + _contrastRatio(Theme.textLabel, Theme.bgPanel));
        }
    }

    // The rule must read as a break on both grounds, not just barely differ.
    // 1.5:1 matches the floor `tst_color_schemes` holds for the card frame.
    function test_rule_separates_from_card_and_panel_on_every_preset(): void {
        for (let i = 0; i < ColorSchemes.ids.length; i++) {
            const id = ColorSchemes.ids[i];
            Theme.colorSchemeId = id;
            verify(_contrastRatio(Theme.borderMid, Theme.surfaceCard) >= 1.5, id + " borderMid/surfaceCard " + _contrastRatio(Theme.borderMid, Theme.surfaceCard));
            verify(_contrastRatio(Theme.borderMid, Theme.bgPanel) >= 1.5, id + " borderMid/bgPanel " + _contrastRatio(Theme.borderMid, Theme.bgPanel));
        }
    }
}
